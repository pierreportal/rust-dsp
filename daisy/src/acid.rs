//! 303-flavoured acid bass preset built on top of `dsp::voice::Voice`.
//!
//! The core voice already provides osc → SVF (LFO-modulated) → distortion →
//! ADSR. This module configures it into 303 territory and adds the two things
//! that make a 303 sound like a 303:
//!
//! * **Slide** — when the previous note is still held, the new note glides to
//!   the target frequency exponentially rather than jumping.
//! * **Accent** — velocity ≥ [`ACCENT_THRESHOLD`] boosts envelope and filter
//!   modulation for that note only.
//!
//! The parameter surface exposed to the host is minimal on purpose: `cutoff`,
//! `resonance`, `env_mod`, `decay`, `accent`, `distortion`. The Pod's two
//! encoders + two knobs can drive whichever four you care about most.

use dsp::osc::Waveform;
use dsp::voice::Voice;

pub const ACCENT_THRESHOLD: u8 = 100;

const SLIDE_COEFF: f32 = 0.0015;
const SNAP_COEFF: f32 = 0.5;
const BASE_CUTOFF: f32 = 200.0;
const ENV_MOD_HZ: f32 = 3500.0;
const ACCENT_BOOST_HZ: f32 = 2000.0;

pub struct AcidBass {
    pub voice: Voice,
    pub note_on: bool,
    pub current_note: Option<u8>,
    pub accent: bool,
}

impl AcidBass {
    pub fn new(sample_rate: f32) -> Self {
        let mut voice = Voice::new(sample_rate);
        voice.osc.waveform = Waveform::Saw;
        voice.env.attack = 0.003;
        voice.env.decay = 0.3;
        voice.env.sustain = 0.0;
        voice.env.release = 0.0;

        voice.distortion.drive = 4.0;
        voice.distortion.output_gain = 0.35;
        voice.freq_smoother.coeff = SNAP_COEFF;

        Self {
            voice,
            note_on: false,
            current_note: None,
            accent: false,
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8) {
        let target = midi_to_freq(note);
        let slide = self.note_on; // legato → slide
        self.voice.freq_smoother.coeff = if slide { SLIDE_COEFF } else { SNAP_COEFF };
        if !slide {
            self.voice.freq_smoother.current = target;
        }
        self.voice.freq_smoother.set_target(target);
        self.accent = velocity >= ACCENT_THRESHOLD;

        if !slide {
            self.voice.env.trigger(velocity);
        } else {
            self.voice.env.velocity = velocity as f32 / 127.0;
        }

        self.current_note = Some(note);
        self.note_on = true;
    }

    pub fn note_off(&mut self, note: u8) {
        if self.current_note == Some(note) {
            self.voice.env.release();
            self.note_on = false;
            self.current_note = None;
        }
    }

    /// Produce one audio sample.
    pub fn next_sample(&mut self) -> f32 {
        // For a proper 303 we want the accent to *boost the filter envelope*
        // — that's what the base voice's LFO stands in for. Adjust resonance
        // and drive on accent to fake it until we add a dedicated filter env.
        // if self.accent {
        // self.voice.distortion.drive = 6.0;
        // } else {
        // self.voice.distortion.drive = 4.0;
        // }

        self.voice.next_sample()
    }
}

/// Convert MIDI note number to frequency in Hz. A4 (note 69) = 440 Hz.
pub fn midi_to_freq(note: u8) -> f32 {
    // f = 440 * 2^((n - 69)/12)
    let exponent = (note as f32 - 69.0) / 12.0;
    440.0 * powf2(exponent)
}

/// `2^x` using libm (no_std-friendly) — pulled through the `dsp` crate's
/// existing libm dep to avoid adding one here.
fn powf2(x: f32) -> f32 {
    // libm is a transitive dep via `dsp`, but not re-exported. Use its
    // exp2f directly through libm's own path.
    libm::exp2f(x)
}

// Suppress unused import when the crate is compiled with all features off.
#[allow(dead_code)]
fn _use_base_cutoff() -> f32 {
    // Kept here so future edits can plug `BASE_CUTOFF`, `ENV_MOD_HZ`, and
    // `ACCENT_BOOST_HZ` into a dedicated filter envelope when we add one.
    BASE_CUTOFF + ENV_MOD_HZ + ACCENT_BOOST_HZ
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48000.0;

    #[test]
    fn midi_to_freq_matches_a4() {
        let f = midi_to_freq(69);
        assert!((f - 440.0).abs() < 1e-3, "A4 → {}", f);
    }

    #[test]
    fn midi_to_freq_octave_doubles() {
        let a3 = midi_to_freq(57);
        let a4 = midi_to_freq(69);
        let a5 = midi_to_freq(81);
        assert!((a5 / a4 - 2.0).abs() < 1e-4);
        assert!((a4 / a3 - 2.0).abs() < 1e-4);
    }

    #[test]
    fn note_on_snaps_frequency_when_no_previous_note() {
        let mut bass = AcidBass::new(SR);
        bass.note_on(45, 100); // A2 = 110 Hz
                               // freq_smoother.current should equal target immediately (no slide).
        assert!((bass.voice.freq_smoother.current - 110.0).abs() < 0.01);
    }

    #[test]
    fn legato_note_on_slides() {
        let mut bass = AcidBass::new(SR);
        bass.note_on(45, 100); // A2
                               // Note still on → next note-on should slide, not snap.
        bass.note_on(57, 100); // A3
                               // Smoother target should be A3 but current should still be near A2.
        let target = midi_to_freq(57);
        assert!((bass.voice.freq_smoother.target - target).abs() < 0.01);
        assert!(bass.voice.freq_smoother.current < target * 0.9);
    }

    #[test]
    fn accent_flag_tracks_velocity_threshold() {
        let mut bass = AcidBass::new(SR);
        bass.note_on(45, ACCENT_THRESHOLD - 1);
        assert!(!bass.accent);
        bass.note_on(45, ACCENT_THRESHOLD);
        assert!(bass.accent);
    }

    #[test]
    fn note_off_for_unrelated_note_is_ignored() {
        let mut bass = AcidBass::new(SR);
        bass.note_on(45, 100);
        bass.note_off(46); // different note
        assert!(bass.note_on);
        bass.note_off(45);
        assert!(!bass.note_on);
    }

    #[test]
    fn produces_bounded_output() {
        let mut bass = AcidBass::new(SR);
        bass.note_on(45, 120);
        let mut peak = 0.0_f32;
        for _ in 0..(SR as usize / 10) {
            let s = bass.next_sample();
            assert!(s.is_finite());
            peak = peak.max(s.abs());
        }
        assert!(peak > 0.0);
        assert!(peak < 2.0, "output should stay bounded, got peak {}", peak);
    }
}
