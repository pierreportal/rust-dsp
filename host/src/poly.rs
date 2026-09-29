use crate::control::Control;
use dsp::smoother::Smoother;

pub const LOWEST_MIDI_NOTE: u8 = 21;
pub const HIGHEST_MIDI_NOTE: u8 = 108;
const NUM_KEYS: usize = (HIGHEST_MIDI_NOTE - LOWEST_MIDI_NOTE + 1) as usize;
const GAIN_SMOOTHING_SECONDS: f32 = 0.005;
const SILENCE_RUN: u16 = 64;

fn midi_to_freq(midi_note: u8) -> f32 {
    440.0 * 2.0_f32.powf((midi_note as f32 - 69.0) / 12.0)
}

fn key_index(midi_note: u8) -> Option<usize> {
    if (LOWEST_MIDI_NOTE..=HIGHEST_MIDI_NOTE).contains(&midi_note) {
        Some((midi_note - LOWEST_MIDI_NOTE) as usize)
    } else {
        None
    }
}

#[derive(Clone, Copy)]
struct KeyVoice<T> {
    voice: T,
    held: bool,
    silent: u16,
}

pub struct PolyphonicVoice<T: Control + Copy> {
    keys: [KeyVoice<T>; NUM_KEYS],
    held_notes: usize,
    gain: Smoother,
}

impl<T: Control + Copy> PolyphonicVoice<T> {
    pub fn with_voices(sample_rate: f32, mut make_voice: impl FnMut(usize) -> T) -> Self {
        Self {
            keys: core::array::from_fn(|index| KeyVoice {
                voice: make_voice(index),
                held: false,
                silent: 0,
            }),
            held_notes: 0,
            gain: Smoother::from_time(1.0, GAIN_SMOOTHING_SECONDS, sample_rate),
        }
    }

    pub fn key_on(&mut self, midi_note: u8, vel: u8) {
        let Some(index) = key_index(midi_note) else {
            return;
        };

        let key = &mut self.keys[index];
        key.voice.set_freq(midi_to_freq(midi_note));
        key.voice.note_on(vel);
        key.silent = 0;

        if !key.held {
            key.held = true;
            self.held_notes += 1;
        }
    }

    pub fn key_off(&mut self, midi_note: u8) {
        let Some(index) = key_index(midi_note) else {
            return;
        };

        let key = &mut self.keys[index];
        key.voice.note_off();

        if key.held {
            key.held = false;
            self.held_notes -= 1;
        }
    }

    pub fn next_sample(&mut self) -> f32 {
        self.gain
            .set_target(1.0 / (self.held_notes.max(1) as f32).sqrt());

        let mut mix = 0.0;
        for key in &mut self.keys {
            if key.silent >= SILENCE_RUN {
                continue;
            }

            let sample = key.voice.next_sample();
            if sample == 0.0 {
                key.silent = key.silent.saturating_add(1);
            } else {
                key.silent = 0;
            }
            mix += sample;
        }

        mix * self.gain.next_sample()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAMP_STEP: f32 = 1.0 / 441.0;

    #[derive(Clone, Copy)]
    struct TestVoice {
        freq: f32,
        vel: u8,
        value: f32,
        target: f32,
    }

    impl TestVoice {
        fn new() -> Self {
            Self {
                freq: 0.0,
                vel: 0,
                value: 0.0,
                target: 0.0,
            }
        }
    }

    impl Control for TestVoice {
        fn next_sample(&mut self) -> f32 {
            if (self.target - self.value).abs() < RAMP_STEP {
                self.value = self.target;
            } else {
                self.value += RAMP_STEP * (self.target - self.value).signum();
            }
            self.value
        }
        fn set_freq(&mut self, freq: f32) {
            self.freq = freq;
        }
        fn note_on(&mut self, vel: u8) {
            self.vel = vel;
            self.target = 1.0;
        }
        fn note_off(&mut self) {
            self.target = 0.0;
        }
    }

    const SAMPLE_RATE: f32 = 44100.0;
    const EPSILON: f32 = 1e-5;
    const SETTLED: usize = 2000;
    const RAMP_STEPS_FOR_ONE: usize = 441;

    fn poly() -> PolyphonicVoice<TestVoice> {
        PolyphonicVoice::with_voices(SAMPLE_RATE, |_| TestVoice::new())
    }

    fn settled(poly: &mut PolyphonicVoice<TestVoice>) -> f32 {
        for _ in 0..SETTLED {
            poly.next_sample();
        }
        poly.next_sample()
    }

    fn voice_of(poly: &PolyphonicVoice<TestVoice>, midi_note: u8) -> &TestVoice {
        &poly.keys[(midi_note - LOWEST_MIDI_NOTE) as usize].voice
    }

    fn is_held(poly: &PolyphonicVoice<TestVoice>, midi_note: u8) -> bool {
        match key_index(midi_note) {
            Some(index) => poly.keys[index].held,
            None => false,
        }
    }

    #[test]
    fn silent_until_a_note_is_played() {
        let mut poly = poly();
        assert!(poly.next_sample().abs() < EPSILON);
    }

    #[test]
    fn a_note_played_after_a_long_idle_period_still_sounds() {
        let mut idle_then_played = poly();
        for _ in 0..44100 {
            idle_then_played.next_sample();
        }
        let mut played_immediately = poly();

        idle_then_played.key_on(60, 100);
        played_immediately.key_on(60, 100);

        for _ in 0..SETTLED {
            idle_then_played.next_sample();
            played_immediately.next_sample();
        }

        assert_eq!(
            idle_then_played.next_sample(),
            played_immediately.next_sample()
        );
    }

    #[test]
    fn a_released_note_is_not_cut_short_by_the_idle_skip() {
        let mut poly = poly();
        poly.key_on(60, 100);
        poly.key_on(64, 100);
        settled(&mut poly);

        poly.key_off(64);

        let mut fading = Vec::new();
        for _ in 0..(RAMP_STEPS_FOR_ONE - SILENCE_RUN as usize) {
            poly.next_sample();
            fading.push(voice_of(&poly, 64).value);
        }
        assert!(
            fading.iter().all(|v| *v > 0.0),
            "the released note must still be rendered while it releases"
        );
        assert!(
            fading.windows(2).all(|w| w[1] <= w[0]),
            "the released note must keep fading rather than being frozen"
        );

        settled(&mut poly);
        assert!((poly.next_sample() - 1.0).abs() < 0.01);
    }

    #[test]
    fn a_chord_sums_its_notes() {
        let mut poly = poly();
        poly.key_on(60, 100);
        poly.key_on(64, 100);
        poly.key_on(67, 100);

        let chord = settled(&mut poly);
        assert!((chord - 3.0_f32.sqrt()).abs() < 1e-3);

        poly.key_off(64);

        let two_notes = settled(&mut poly);
        assert!((two_notes - 2.0_f32.sqrt()).abs() < 1e-3);

        poly.key_off(60);
        poly.key_off(67);

        assert!(settled(&mut poly).abs() < EPSILON);
    }

    #[test]
    fn adding_a_note_does_not_jump_the_output_level() {
        let mut poly = poly();
        poly.key_on(60, 100);
        settled(&mut poly);

        poly.key_on(64, 100);

        assert!((poly.next_sample() - 1.0).abs() < 0.01);
    }

    #[test]
    fn releasing_a_note_does_not_jump_the_output_level() {
        let mut poly = poly();
        poly.key_on(60, 100);
        poly.key_on(64, 100);
        settled(&mut poly);

        poly.key_off(64);

        assert!((poly.next_sample() - 2.0_f32.sqrt()).abs() < 0.01);
    }

    #[test]
    fn notes_outside_the_key_range_are_ignored() {
        let mut poly = poly();
        poly.key_on(LOWEST_MIDI_NOTE - 1, 100);
        poly.key_on(HIGHEST_MIDI_NOTE + 1, 100);

        assert!(poly.next_sample().abs() < EPSILON);
        assert!(!is_held(&poly, LOWEST_MIDI_NOTE - 1));
        assert!(!is_held(&poly, HIGHEST_MIDI_NOTE + 1));
    }

    #[test]
    fn notes_on_both_ends_of_the_keyboard_are_mapped_to_distinct_voices() {
        let mut poly = poly();
        poly.key_on(LOWEST_MIDI_NOTE, 100);
        poly.key_on(HIGHEST_MIDI_NOTE, 100);

        assert!(is_held(&poly, LOWEST_MIDI_NOTE));
        assert!(is_held(&poly, HIGHEST_MIDI_NOTE));
        assert!(!is_held(&poly, LOWEST_MIDI_NOTE + 1));

        let chord = settled(&mut poly);
        assert!((chord - 2.0_f32.sqrt()).abs() < 1e-3);
    }

    #[test]
    fn repeated_note_keeps_a_single_voice_held() {
        let mut poly = poly();
        poly.key_on(60, 100);
        poly.key_on(60, 100);

        assert!(is_held(&poly, 60));
        assert!((settled(&mut poly) - 1.0).abs() < 1e-3);

        poly.key_off(60);

        assert!(!is_held(&poly, 60));
        assert!(settled(&mut poly).abs() < EPSILON);
    }

    #[test]
    fn each_note_gets_its_own_frequency() {
        let mut poly = poly();
        poly.key_on(69, 100);
        poly.key_on(81, 100);

        assert!((voice_of(&poly, 69).freq - 440.0).abs() < EPSILON);
        assert!((voice_of(&poly, 81).freq - 880.0).abs() < EPSILON);
    }

    #[test]
    fn velocity_reaches_the_underlying_voice() {
        let mut poly = poly();
        poly.key_on(60, 42);

        assert_eq!(voice_of(&poly, 60).vel, 42);
    }
}
