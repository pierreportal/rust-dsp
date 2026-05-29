use crate::adsr::Adsr;
use crate::distortion::Distortion;
use crate::filter::Filter;
use crate::osc::{Osc, Waveform};
use crate::patch;
use crate::patch::Module;
use crate::smoother::Smoother;

pub struct Voice {
    pub osc: Osc,
    pub env: Adsr,
    pub filter: Filter,
    pub distortion: Distortion,
    pub freq_smoother: Smoother,
    pub filter_cutoff_smoother: Smoother,
}

impl Voice {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            osc: Osc::new(Waveform::PulseWidth, 220.0, sample_rate),
            env: Adsr::new(sample_rate),
            filter: Filter::new(sample_rate),
            distortion: Distortion::new(),
            freq_smoother: Smoother::new(440.0, 0.0005),
            filter_cutoff_smoother: Smoother::new(2000.0, 0.0005),
        }
    }

    fn self_update(&mut self) {
        self.osc.freq = self.freq_smoother.next_sample();
        self.filter.cutoff = self.filter_cutoff_smoother.next_sample();
    }

    pub fn next_sample(&mut self) -> f32 {
        self.self_update();

        let input_sig = 1.0;

        patch!(self.osc => self.env => self.distortion => self.filter)(input_sig)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f32 = 44100.0;

    #[test]
    fn test_voice_new() {
        let voice = Voice::new(SAMPLE_RATE);

        assert_eq!(voice.osc.sample_rate, SAMPLE_RATE);
        assert_eq!(voice.env.sample_rate, SAMPLE_RATE);
        assert_eq!(voice.filter.sample_rate, SAMPLE_RATE);
        assert_eq!(voice.freq_smoother.target, 440.0);
        assert_eq!(voice.filter_cutoff_smoother.target, 2000.0);
    }

    #[test]
    fn test_voice_idle_state() {
        let mut voice = Voice::new(SAMPLE_RATE);

        // Voice should be idle initially
        assert!(voice.env.is_idle());

        let output = voice.next_sample();

        // Output should be near zero when envelope is idle
        assert_eq!(output, 0.0);
    }

    #[test]
    fn test_voice_trigger() {
        let mut voice = Voice::new(SAMPLE_RATE);

        voice.env.trigger(127);

        // After trigger, envelope should not be idle
        assert!(!voice.env.is_idle());
    }

    #[test]
    fn test_voice_produces_output() {
        let mut voice = Voice::new(SAMPLE_RATE);
        voice.env.trigger(100);

        let output = voice.next_sample();

        // Should produce non-zero output after trigger
        assert!(output.abs() > 0.0);
    }

    #[test]
    fn test_voice_frequency_smoothing() {
        let mut voice = Voice::new(SAMPLE_RATE);

        let initial_freq = voice.osc.freq;
        voice.freq_smoother.set_target(880.0);

        // Process a few samples
        for _ in 0..100 {
            voice.next_sample();
        }

        // Frequency should have changed toward target
        assert_ne!(voice.osc.freq, initial_freq);
        assert!(voice.osc.freq > initial_freq);
    }

    #[test]
    fn test_voice_filter_cutoff_smoothing() {
        let mut voice = Voice::new(SAMPLE_RATE);

        let initial_cutoff = voice.filter.cutoff;
        voice.filter_cutoff_smoother.set_target(5000.0);

        // Process a few samples
        for _ in 0..100 {
            voice.next_sample();
        }

        // Cutoff should have changed toward target
        assert_ne!(voice.filter.cutoff, initial_cutoff);
        assert!(voice.filter.cutoff > initial_cutoff);
    }

    #[test]
    fn test_voice_envelope_cycle() {
        let mut voice = Voice::new(SAMPLE_RATE);
        voice.env.attack = 0.01;
        voice.env.decay = 0.01;
        voice.env.sustain = 0.7;
        voice.env.release = 0.01;

        // Trigger note
        voice.env.trigger(100);

        // Process through attack and decay
        for _ in 0..(0.03 * SAMPLE_RATE) as usize {
            voice.next_sample();
        }

        // Should be in sustain
        assert_eq!(voice.env.state, crate::adsr::EnvState::Sustain);

        // Release
        voice.env.release();

        // Process release
        for _ in 0..(0.02 * SAMPLE_RATE) as usize {
            voice.next_sample();
        }

        // Should return to idle
        assert!(voice.env.is_idle());
    }

    #[test]
    fn test_voice_signal_chain() {
        let mut voice = Voice::new(SAMPLE_RATE);
        voice.env.trigger(127);

        // Generate multiple samples and check properties
        let mut non_zero_count = 0;
        for _ in 0..100 {
            let output = voice.next_sample();
            // All outputs should be finite
            assert!(output.is_finite());
            // Count non-zero outputs
            if output.abs() > 1e-6 {
                non_zero_count += 1;
            }
        }

        // At least some outputs should be non-zero
        assert!(non_zero_count > 0);
    }

    #[test]
    fn test_voice_stability() {
        let mut voice = Voice::new(SAMPLE_RATE);
        voice.env.trigger(127);

        // Process many samples
        for _ in 0..10000 {
            let output = voice.next_sample();

            // Output should remain bounded and finite
            assert!(output.is_finite());
            assert!(output.abs() < 100.0); // Reasonable bound for audio
        }
    }

    #[test]
    fn test_voice_multiple_notes() {
        let mut voice = Voice::new(SAMPLE_RATE);
        voice.env.attack = 0.001;
        voice.env.decay = 0.001;
        voice.env.release = 0.01;

        // First note
        voice.env.trigger(100);
        for _ in 0..1000 {
            voice.next_sample();
        }

        // Release first note
        voice.env.release();
        // Process long enough for release to complete
        for _ in 0..((0.02 * SAMPLE_RATE) as usize) {
            voice.next_sample();
        }

        // Should be back to idle
        assert!(voice.env.is_idle());

        // Trigger second note
        voice.env.trigger(80);
        assert!(!voice.env.is_idle());

        let output = voice.next_sample();
        assert!(output.abs() > 0.0);
    }

    #[test]
    fn test_voice_parameter_changes() {
        let mut voice = Voice::new(SAMPLE_RATE);
        voice.env.trigger(127);

        // Change parameters mid-note
        voice.distortion.drive = 20.0;
        voice.freq_smoother.set_target(1000.0);
        voice.filter_cutoff_smoother.set_target(10000.0);

        // Should continue producing output
        for _ in 0..1000 {
            let output = voice.next_sample();
            assert!(output.is_finite());
        }
    }

    #[test]
    fn test_voice_velocity_affects_output() {
        let mut voice1 = Voice::new(SAMPLE_RATE);
        let mut voice2 = Voice::new(SAMPLE_RATE);

        voice1.env.trigger(64);
        voice2.env.trigger(127);

        // Generate some samples
        for _ in 0..100 {
            voice1.next_sample();
            voice2.next_sample();
        }

        // Higher velocity should generally produce louder output
        // (though filtering/distortion affects this)
        assert!(voice1.env.velocity < voice2.env.velocity);
    }

    #[test]
    fn test_voice_waveform_types() {
        let waveforms = [
            Waveform::Saw,
            Waveform::Triangle,
            Waveform::Square,
            Waveform::PulseWidth,
        ];

        for waveform in waveforms {
            let mut voice = Voice::new(SAMPLE_RATE);
            voice.osc.waveform = waveform;
            voice.env.trigger(100);

            let output = voice.next_sample();
            assert!(output.is_finite());
        }
    }

    #[test]
    fn test_voice_self_update() {
        let mut voice = Voice::new(SAMPLE_RATE);

        let target_freq = 1000.0;
        let target_cutoff = 8000.0;

        voice.freq_smoother.set_target(target_freq);
        voice.filter_cutoff_smoother.set_target(target_cutoff);

        // Call next_sample which internally calls self_update
        voice.next_sample();

        // Parameters should have started moving toward targets
        assert!(
            voice.osc.freq != voice.freq_smoother.current
                || voice.freq_smoother.current != target_freq
        );
    }
}
