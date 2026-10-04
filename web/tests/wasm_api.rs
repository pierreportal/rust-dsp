//! Integration test for the exact surface the AudioWorklet drives.
//!
//! The unit tests in `src/poly.rs` cover `PolyGraph` directly. This exercises
//! the `#[wasm_bindgen]` `Graph` wrapper instead — the same `add_node` /
//! `note_on(note, vel)` / `process` calls `public/graph-processor.js` makes —
//! so a signature or plumbing mistake between the pool and the JS layer fails
//! here rather than silently producing silence in the browser.

use web::Graph;

const SR: f32 = 48000.0;

/// Sine voice: Midi -> SineOsc(freq cv), Midi gate -> Adsr, both -> Vca -> Out.
fn sine_patch(g: &mut Graph) {
    g.add_node(0, 9); // Midi
    g.add_node(1, 10); // SineOsc
    g.add_node(2, 1); // Adsr
    g.add_node(3, 4); // Vca
    g.add_node(4, 8); // Out
    g.set_param(1, "freq", 440.0);
    g.set_param(2, "attack", 0.001);
    g.set_param(2, "decay", 0.001);
    g.set_param(2, "sustain", 0.3);
    g.set_param(2, "release", 0.1);
    g.connect(0, 1, 1, 0);
    g.connect(0, 0, 2, 0);
    g.connect(1, 0, 3, 0);
    g.connect(2, 0, 3, 1);
    g.connect(3, 0, 4, 0);
}

/// Goertzel magnitude of `freq` in `buf`, one second at SR (bins on whole Hz).
fn goertzel(buf: &[f32], freq: f32) -> f64 {
    let n = buf.len() as f64;
    let w = 2.0 * std::f64::consts::PI * freq as f64 / SR as f64;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    let mut s0;
    for &x in buf {
        s0 = x as f64 + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    (s1 * s1 + s2 * s2 - coeff * s1 * s2).sqrt() / n * 2.0
}

fn played_hz(note: u8) -> f32 {
    440.0 * 2.0f32.powf((note as f32 - 69.0) / 12.0)
}

#[test]
fn a_chord_through_the_wasm_api_is_polyphonic() {
    let mut g = Graph::new(SR);
    sine_patch(&mut g);

    // No notes yet: silence.
    let mut idle = vec![0.0f32; 128];
    g.process(&mut idle);
    assert!(idle.iter().all(|&s| s.abs() < 1e-7), "idle graph leaked");

    g.note_on(60, 127);
    g.note_on(64, 127);
    g.note_on(67, 127);

    let mut buf = vec![0.0f32; SR as usize * 2];
    g.process(&mut buf);
    let steady = &buf[SR as usize..];

    // Every held pitch must be present, each from its own voice. A monophonic
    // engine would only ever show the last note.
    for note in [60u8, 64, 67] {
        let mag = goertzel(steady, played_hz(note));
        assert!(mag > 0.02, "note {note} missing through the wasm API: {mag}");
    }

    // Releasing one note leaves the others ringing.
    g.note_off(60);
    let mut tail = vec![0.0f32; SR as usize / 4];
    g.process(&mut tail);
    assert!(
        goertzel(&tail, played_hz(67)) > 0.01,
        "releasing one note silenced the chord"
    );

    // And the mix stays inside full scale.
    assert!(
        steady.iter().all(|s| s.is_finite() && s.abs() <= 1.0),
        "poly mix escaped the limiter"
    );
}