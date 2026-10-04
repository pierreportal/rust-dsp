//! Proves the render path does not allocate.
//!
//! Every other test in this crate checks that the engine produces the right
//! samples. This one checks that producing them is free, which is a different
//! property and the one that breaks first when someone optimises or refactors
//! something unrelated.
//!
//! The counter is thread-local and armed explicitly, so tests running in parallel
//! do not measure each other and setup outside the measured region costs nothing.

mod common;

use common::alloc::{allocations, counting, Counting};
use graph::{GraphEngine, Kind, PolyGraph};

#[global_allocator]
static GLOBAL: Counting = Counting;

const SR: f32 = 48000.0;

/// A patch exercising the parts most likely to allocate: several oscillators, a
/// filter, an envelope, a controller and an output, all wired together.
///
/// Uses only kinds this build actually has, so adding a module later cannot
/// silently turn this into a no-op test.
fn busy_patch() -> GraphEngine {
    let mut g = GraphEngine::new(SR);

    let midi = 0;
    let osc_a = 1;
    let osc_b = 2;
    let cc = 3;
    let env = 4;
    let filter = 5;
    let shaper = 6;
    let vca = 7;
    let out = 8;

    assert!(g.add_node(midi, Kind::Midi as u32), "midi");
    assert!(g.add_node(osc_a, Kind::SineOsc as u32), "osc a");
    assert!(g.add_node(osc_b, Kind::SawOsc as u32), "osc b");
    assert!(g.add_node(cc, Kind::CC as u32), "cc");
    assert!(g.add_node(env, Kind::Adsr as u32), "env");
    assert!(g.add_node(filter, Kind::Filter as u32), "filter");
    assert!(g.add_node(shaper, Kind::Distortion as u32), "shaper");
    assert!(g.add_node(vca, Kind::Vca as u32), "vca");
    assert!(g.add_node(out, Kind::Out as u32), "out");

    assert!(g.connect(osc_a, 0, filter, 0), "osc a -> filter");
    assert!(g.connect(osc_b, 0, filter, 0), "osc b -> filter");
    assert!(g.connect(cc, 0, filter, 1), "cc -> filter cv");
    assert!(g.connect(midi, 1, osc_a, 0), "pitch cv -> osc a");
    assert!(g.connect(midi, 1, osc_b, 0), "pitch cv -> osc b");
    assert!(g.connect(midi, 0, env, 0), "gate -> env");
    assert!(g.connect(env, 0, vca, 1), "env -> vca cv");
    assert!(g.connect(filter, 0, shaper, 0), "filter -> shaper");
    assert!(g.connect(shaper, 0, vca, 0), "shaper -> vca signal");
    assert!(g.connect(vca, 0, out, 0), "vca -> out");

    g.note_on_all(60);
    g.set_cc(74, 100);
    g
}

#[test]
fn rendering_a_block_does_not_allocate() {
    let mut g = busy_patch();
    let mut buf = vec![0.0f32; 256];

    // Warm up outside the measured region: the first call may legitimately
    // allocate once for lazily-initialised engine state, and that is not what this
    // test is about. Every block after it must be free.
    g.process(&mut buf);

    let _guard = counting();
    for _ in 0..64 {
        g.process(&mut buf);
    }
    let found = allocations();

    assert_eq!(
        found,
        0,
        "rendering allocated {found} times in 64 blocks ({:.3} per block); \
         look for a clone/vec/format!/to_string in the render path",
        found as f64 / 64.0
    );
}

#[test]
fn rendering_the_raw_voice_path_does_not_allocate() {
    // `process_raw` is what the voice pool calls, once per voice per block, so it
    // is the hotter of the two paths and worth measuring on its own.
    let mut g = busy_patch();
    let mut buf = vec![0.0f32; 256];
    g.process_raw(&mut buf);

    let _guard = counting();
    for _ in 0..64 {
        g.process_raw(&mut buf);
    }
    let found = allocations();

    assert_eq!(found, 0, "process_raw allocated {found} times in 64 blocks");
}

/// The same patch as `busy_patch`, built across every voice in the pool.
///
/// Without this the pool tests below are theatre: `PolyGraph::new` starts with
/// empty engines, `note_on` finds nothing to render and returns early, and the
/// loop measures an empty graph. It would have passed even with the original
/// per-block `order.clone()` still in place.
fn busy_pool() -> PolyGraph {
    let mut voices = PolyGraph::new(SR);

    let midi = 0;
    let osc_a = 1;
    let osc_b = 2;
    let cc = 3;
    let env = 4;
    let filter = 5;
    let shaper = 6;
    let vca = 7;
    let out = 8;

    assert!(voices.add_node(midi, Kind::Midi as u32), "midi");
    assert!(voices.add_node(osc_a, Kind::SineOsc as u32), "osc a");
    assert!(voices.add_node(osc_b, Kind::SawOsc as u32), "osc b");
    assert!(voices.add_node(cc, Kind::CC as u32), "cc");
    assert!(voices.add_node(env, Kind::Adsr as u32), "env");
    assert!(voices.add_node(filter, Kind::Filter as u32), "filter");
    assert!(voices.add_node(shaper, Kind::Distortion as u32), "shaper");
    assert!(voices.add_node(vca, Kind::Vca as u32), "vca");
    assert!(voices.add_node(out, Kind::Out as u32), "out");

    assert!(voices.connect(osc_a, 0, filter, 0), "osc a -> filter");
    assert!(voices.connect(osc_b, 0, filter, 0), "osc b -> filter");
    assert!(voices.connect(cc, 0, filter, 1), "cc -> filter cv");
    assert!(voices.connect(midi, 1, osc_a, 0), "pitch cv -> osc a");
    assert!(voices.connect(midi, 1, osc_b, 0), "pitch cv -> osc b");
    assert!(voices.connect(midi, 0, env, 0), "gate -> env");
    assert!(voices.connect(env, 0, vca, 1), "env -> vca cv");
    assert!(voices.connect(filter, 0, shaper, 0), "filter -> shaper");
    assert!(voices.connect(shaper, 0, vca, 0), "shaper -> vca signal");
    assert!(voices.connect(vca, 0, out, 0), "vca -> out");

    voices
}

#[test]
fn the_whole_voice_pool_does_not_allocate_while_held() {
    // The realistic case: a chord, held, rendered block after block. This is the
    // loop that runs for as long as the user holds the keys.
    let mut voices = busy_pool();
    voices.set_cc(74, 100);
    for note in [60u8, 64, 67] {
        voices.note_on(note, 100);
    }

    let mut buf = vec![0.0f32; 256];
    voices.process(&mut buf);

    let _guard = counting();
    for _ in 0..64 {
        voices.process(&mut buf);
    }
    let found = allocations();

    assert_eq!(
        found, 0,
        "a held 3-note chord allocated {found} times in 64 blocks; \
         PolyGraph::process is the loop that has to be free"
    );
}

#[test]
fn steering_during_playback_does_not_allocate() {
    // Voice stealing and releasing run on the audio thread too, not just the
    // steady state, and they touch the voice pool's own bookkeeping.
    let mut voices = busy_pool();
    voices.set_cc(74, 100);
    let mut buf = vec![0.0f32; 256];

    for note in 0..8u8 {
        voices.note_on(60 + note, 100);
    }
    voices.process(&mut buf);

    let _guard = counting();
    for i in 0..64 {
        if i % 8 == 0 {
            voices.note_off(60 + (i / 8) as u8);
        }
        if i % 16 == 0 {
            // More notes than voices: forces stealing on every one of these.
            voices.note_on(72 + (i / 16) as u8, 100);
        }
        voices.process(&mut buf);
    }
    let found = allocations();

    assert_eq!(
        found, 0,
        "note on/off and voice stealing allocated {found} times in 64 blocks"
    );
}

#[test]
fn the_counter_actually_detects_an_allocation() {
    // A test that cannot fail is worse than no test. Prove the instrument works
    // by allocating through it.
    let found = {
        let _guard = counting();
        let _noise: Vec<u8> = vec![0; 1024];
        allocations()
    };
    assert!(found >= 1, "the counting allocator is not counting");
}
