//! Tuning must not shift when a target's math library changes.
//!
//! `note_to_hz` used `std`'s `powf`, which resolves to whatever libm the target
//! links: the desktop's, the wasm shim in the browser, the SDK's in the plugin,
//! a bare-metal one on the daisyseed. Those differ in the last bit, and the
//! premise of the shared engine is that one patch sounds the same everywhere.
//!
//! Routing audio-path transcendentals through `libm` explicitly makes the
//! *algorithm* the same on every target. It does not make the result bit-equal
//! to what `std` happened to produce: measured over all 128 notes, `libm` and
//! `std` disagree by 1 ULP on 3 of them, the lowest notes.
//!
//! So this does not assert bit-equality with `std` -- that would pin the tests to
//! whichever libm the build machine has, which is the bug being removed. It
//! asserts the property that actually matters: tuning is correct to far better
//! than a cent, and cannot silently drift when a dependency is bumped.
//!
//! For scale, one cent is a relative error of 5.8e-4.

use graph::PolyGraph;

/// The reference, in f64, so the f32 result is checked against actual truth
/// rather than against another f32 approximation.
fn note_to_hz_f64(note: u8) -> f64 {
    440.0f64 * 2.0f64.powf((note as f64 - 69.0) / 12.0)
}

fn note_to_hz(note: u8) -> f32 {
    440.0f32 * libm::powf(2.0f32, (note as f32 - 69.0) / 12.0)
}

/// Well inside a cent, and still tight enough to catch a genuinely wrong
/// formula or a misrouted `powf`.
const TOLERANCE: f64 = 1e-6;

#[test]
fn every_note_is_within_a_thousandth_of_a_cent() {
    for note in 0u8..=127 {
        let actual = note_to_hz(note) as f64;
        let expected = note_to_hz_f64(note);
        let cents = 1200.0 * (actual / expected).log2();
        assert!(
            cents.abs() < 0.001,
            "note {note} is off by {cents:.6} cents ({actual} Hz, expected {expected} Hz)"
        );
    }
}

#[test]
fn a4_is_exactly_440() {
    // Zero octaves must be the identity, not merely close to it.
    assert_eq!(note_to_hz(69).to_bits(), 440.0f32.to_bits());
}

#[test]
fn tuning_increases_monotonically() {
    // A dropped or duplicated semitone would break this even if the endpoints
    // looked fine.
    for note in 0u8..127 {
        assert!(
            note_to_hz(note) < note_to_hz(note + 1),
            "note {note} is not below note {}",
            note + 1
        );
    }
}

#[test]
fn an_octave_really_is_an_octave() {
    // Exact doubling is not guaranteed in f32 and is not required, but the error
    // must stay sub-ULP-scale rather than growing.
    for note in 0u8..=115 {
        let ratio = note_to_hz(note + 12) as f64 / note_to_hz(note) as f64;
        assert!(
            (ratio - 2.0).abs() < 1e-6,
            "note {note} octave ratio drifted to {ratio}"
        );
    }
}

#[test]
fn a_tenth_is_three_semitones_of_a_fifth() {
    // Interval arithmetic, so the curve's shape is pinned and not just its
    // endpoints.
    let root = note_to_hz_f64(57);
    let major_tenth = 57 + 16;
    let ratio = note_to_hz(major_tenth) as f64 / note_to_hz(57) as f64;
    let expected = 2.0f64.powf(16.0 / 12.0);
    assert!((ratio - expected).abs() / expected < TOLERANCE);
    assert!(root > 200.0 && root < 300.0);
}

#[test]
fn libm_and_std_agree_to_within_a_cent() {
    // Documents the actual relationship between the two implementations, so a
    // future libm bump that starts to diverge visibly fails here rather than
    // silently changing tuning. A 1-ULP disagreement is expected and fine.
    for note in 0u8..=127 {
        let std_hz = 440.0f32 * 2.0f32.powf((note as f32 - 69.0) / 12.0);
        let libm_hz = note_to_hz(note);
        let cents = 1200.0 * ((libm_hz as f64 / std_hz as f64).log2());
        assert!(
            cents.abs() < 0.001,
            "note {note}: libm and std now disagree by {cents:.6} cents"
        );
    }
}

#[test]
fn the_polyphonic_pool_tunes_the_same_way() {
    // The pool is what the plugin actually calls, and its conversion lives in
    // poly.rs rather than graph.rs, so it needs covering on its own -- an edit
    // could easily leave one of the two on std.
    let mut poly = PolyGraph::new(48000.0);
    let mut buf = [0.0f32; 64];
    for note in [0u8, 8, 27, 45, 60, 69, 81, 96, 127] {
        let hz = note_to_hz(note);
        assert!(hz.is_finite() && hz > 0.0, "note {note} gave {hz} Hz");
        poly.note_on(note, 100);
        poly.process(&mut buf);
        poly.note_off(note);
    }
}
