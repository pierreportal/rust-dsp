//! `connect` must only accept cables the engine can actually render.
//!
//! Every case here was reachable from a decoded patch, which is the untrusted
//! input a baked plugin accepts. Three of them panicked, and one of those
//! panicked on the audio thread during `process` rather than at load time, which
//! is the worst possible moment: a DAW session dies mid-playback.
//!
//! The contract is now that a rejected cable returns `false`, changes nothing,
//! and never reaches the render loop.

use graph::{GraphEngine, Kind};

const SR: f32 = 48000.0;

/// Runs `f`, returning whether it panicked. The panic hook is silenced so the
/// expected panics below do not spam the test output.
fn panics<F: FnOnce()>(f: F) -> bool {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    std::panic::set_hook(previous);
    result.is_err()
}

/// Two nodes with one input each, ready to be wired.
fn pair() -> (GraphEngine, u32, u32) {
    let mut g = GraphEngine::new(SR);
    let vca = 0;
    let osc = 1;
    assert!(g.add_node(vca, Kind::Vca as u32));
    assert!(g.add_node(osc, Kind::SineOsc as u32));
    (g, vca, osc)
}

#[test]
fn a_cable_from_a_node_that_does_not_exist_is_refused() {
    // Used to panic inside rebuild's topological sort.
    let (mut g, vca, _) = pair();
    assert!(!panics(|| {
        g.connect(99, 0, vca, 0);
    }));
    assert!(
        !g.connect(99, 0, vca, 0),
        "should report failure, not panic"
    );
}

#[test]
fn a_cable_to_a_node_that_does_not_exist_is_refused() {
    let (mut g, _, osc) = pair();
    assert!(!panics(|| {
        g.connect(osc, 0, 99, 0);
    }));
    assert!(!g.connect(osc, 0, 99, 0));
}

#[test]
fn a_cable_from_a_port_that_does_not_exist_is_refused() {
    // The dangerous one: this was accepted, then panicked inside `process` on
    // the audio thread, because rebuild validated only the target port.
    let (mut g, vca, osc) = pair();
    assert!(!panics(|| {
        g.connect(osc, 7, vca, 0);
    }));
    assert!(!g.connect(osc, 7, vca, 0), "out of range source port");

    // And crucially: rendering afterwards must be safe.
    let mut buf = vec![0.0f32; 64];
    assert!(!panics(|| {
        g.process(&mut buf);
    }));
}

#[test]
fn a_cable_to_a_port_that_does_not_exist_is_refused() {
    let (mut g, vca, osc) = pair();
    // Vca has two inputs, so port 9 is out of range.
    assert!(!g.connect(osc, 0, vca, 9));
}

#[test]
fn a_refused_cable_leaves_the_graph_untouched() {
    // Sine -> VCA -> Out, with an envelope opening the VCA. A bare VCA with no
    // envelope patched stays closed, so the envelope is what makes this a test
    // of the cable rather than of the VCA's default gain.
    let mut g = GraphEngine::new(SR);
    let midi = 0;
    let osc = 1;
    let env = 2;
    let vca = 3;
    let out = 4;
    g.add_node(midi, Kind::Midi as u32);
    g.add_node(osc, Kind::SineOsc as u32);
    g.add_node(env, Kind::Adsr as u32);
    g.add_node(vca, Kind::Vca as u32);
    g.add_node(out, Kind::Out as u32);
    assert!(g.connect(osc, 0, vca, 0), "the valid cable first");
    assert!(g.connect(midi, 0, env, 0), "gate -> env");
    assert!(g.connect(env, 0, vca, 1), "env -> vca gain");
    assert!(g.connect(vca, 0, out, 0));

    // Now the illegal ones, which must all be refused without disturbing any of it.
    assert!(!g.connect(osc, 5, vca, 0), "out of range source port");
    assert!(!g.connect(99, 0, vca, 0), "nonexistent source");
    assert!(!g.connect(osc, 0, 99, 0), "nonexistent target");
    assert!(!g.connect(osc, 0, vca, 9), "out of range target port");

    g.note_on_all(69);
    let mut buf = vec![0.0f32; 256];
    g.process(&mut buf);
    assert!(
        buf.iter().any(|s| s.abs() > 1e-4),
        "the valid cable should still carry signal, so a refused cable changed nothing"
    );
}

#[test]
fn a_self_loop_is_still_refused() {
    let (mut g, vca, _) = pair();
    assert!(!g.connect(vca, 0, vca, 0));
}

#[test]
fn a_cycle_is_still_refused() {
    let mut g = GraphEngine::new(SR);
    let a = 0;
    let b = 1;
    let c = 2;
    g.add_node(a, Kind::SineOsc as u32);
    g.add_node(b, Kind::Filter as u32);
    g.add_node(c, Kind::Vca as u32);

    assert!(g.connect(a, 0, b, 0));
    assert!(g.connect(b, 0, c, 0));
    // c -> b would close the loop.
    assert!(!g.connect(c, 0, b, 0));
}

#[test]
fn ports_that_do_not_exist_on_a_removed_node_are_refused() {
    let mut g = GraphEngine::new(SR);
    let a = 0;
    let b = 1;
    g.add_node(a, Kind::SineOsc as u32);
    g.add_node(b, Kind::Vca as u32);
    assert!(g.connect(a, 0, b, 0));

    // Removing the source leaves a cable pointing at nothing. Reconnecting must
    // not panic, and rendering must stay safe.
    g.remove_node(a);
    assert!(!panics(|| {
        let _ = g.connect(a, 0, b, 0);
    }));
    let mut buf = vec![0.0f32; 64];
    assert!(!panics(|| {
        g.process(&mut buf);
    }));
}

#[test]
fn every_module_kind_survives_a_fuzz_of_illegal_cables() {
    // Sweep every kind against every illegal source/target port and id, and
    // require that neither connecting nor rendering ever panics. This is the
    // property a baked plugin depends on: whatever the patch decoder lets
    // through, rendering must not abort.
    let kinds: &[Kind] = &[
        Kind::Osc,
        Kind::Adsr,
        Kind::Filter,
        Kind::Distortion,
        Kind::Vca,
        Kind::Mixer,
        Kind::AcidFilter,
        Kind::Constant,
        Kind::Out,
        Kind::Midi,
        Kind::SineOsc,
        Kind::SawOsc,
        Kind::SquareOsc,
        Kind::CC,
    ];

    for from_kind in kinds {
        for to_kind in kinds {
            let mut g = GraphEngine::new(SR);
            let from = 0;
            let to = 1;
            g.add_node(from, *from_kind as u32);
            g.add_node(to, *to_kind as u32);

            let bad = [
                (0, 0, to, from_kind.outputs().len() as u32),
                (0, from_kind.outputs().len() as u32, to, 0),
                (from, 0, to, to_kind.inputs().len() as u32),
                (from, 0, to, 99),
                (77, 0, to, 0),
                (from, 0, 77, 0),
            ];
            for &(f, fp, t, tp) in &bad {
                let label = format!("{from_kind:?} -> {to_kind:?} {f}:{fp} -> {t}:{tp}");
                assert!(
                    !panics(|| {
                        let _ = g.connect(f, fp, t, tp);
                    }),
                    "connect panicked for {label}"
                );
            }

            let mut buf = vec![0.0f32; 64];
            g.note_on_all(60);
            g.set_cc(74, 64);
            assert!(
                !panics(|| {
                    g.process(&mut buf);
                }),
                "process panicked for {from_kind:?} -> {to_kind:?}"
            );
        }
    }
}
