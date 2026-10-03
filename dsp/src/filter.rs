use crate::patch::Module;
use crate::smoother::Smoother;

/// A one-pole low-pass filter.
///
/// `cutoff` holds the requested value; `cutoff_smoother` holds the value
/// actually in use, which glides toward it so that sweeping the cutoff cannot
/// click. Use [`Filter::set_cutoff`] to change it, since writing the field
/// directly would leave the two out of step.
#[derive(Clone, Copy)]
pub struct Filter {
    pub cutoff: f32,
    pub cutoff_smoother: Smoother,
    z: f32,
    coefficient: f32,
    last_cutoff: f32,
    sample_rate: f32,
}

impl Filter {
    pub fn new(sample_rate: f32) -> Self {
        let cutoff = 2000.0;

        let mut filter = Self {
            cutoff,
            cutoff_smoother: Smoother::from_time(cutoff, 0.0005, sample_rate),
            z: 0.0,
            coefficient: 0.0,
            // NaN never compares equal to itself, so the first call always
            // takes the branch that computes the coefficient.
            last_cutoff: f32::NAN,
            sample_rate,
        };

        filter.update_coefficient(cutoff);
        filter
    }
    pub fn set_cutoff(&mut self, cutoff: f32) {
        self.cutoff = cutoff.clamp(1.0, self.sample_rate * 0.45);
        self.cutoff_smoother.set_target(self.cutoff);
    }

    fn update_coefficient(&mut self, cutoff: f32) {
        // A settled parameter repeats its coefficient every sample; skipping
        // the rebuild saves an `expf` per sample with no change in output.
        if cutoff == self.last_cutoff {
            return;
        }
        self.last_cutoff = cutoff;

        let x = libm::expf(-2.0 * core::f32::consts::PI * cutoff / self.sample_rate);

        self.coefficient = 1.0 - x;
    }

    pub fn process_sample(&mut self, input: f32) -> f32 {
        let cutoff = self.cutoff_smoother.next_sample();
        self.update_coefficient(cutoff);

        self.z += self.coefficient * (input - self.z);
        self.z
    }
}

impl Module for Filter {
    fn process(&mut self, input: f32) -> f32 {
        self.process_sample(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f32 = 44100.0;

    #[test]
    fn test_filter_new() {
        let filter = Filter::new(SAMPLE_RATE);
        assert_eq!(filter.cutoff, 2000.0);
        assert_eq!(filter.z, 0.0);
        assert_eq!(filter.sample_rate, SAMPLE_RATE);
    }

    #[test]
    fn test_filter_dc_signal() {
        let mut filter = Filter::new(SAMPLE_RATE);
        filter.set_cutoff(1000.0);

        let input = 1.0;
        let mut output = 0.0;

        // Process enough samples for filter to settle
        for _ in 0..10000 {
            output = filter.process(input);
        }

        // For DC signal, output should converge to input
        assert!((output - input).abs() < 0.01);
    }

    #[test]
    fn test_filter_smoothing() {
        let mut filter = Filter::new(SAMPLE_RATE);
        filter.set_cutoff(100.0); // Low cutoff for strong smoothing

        // Step input
        let output1 = filter.process(1.0);
        let output2 = filter.process(1.0);
        let output3 = filter.process(1.0);

        // Output should gradually approach input
        assert!(output1 < output2);
        assert!(output2 < output3);
        assert!(output3 < 1.0);
    }

    #[test]
    fn test_filter_attenuates_high_freq() {
        let mut filter = Filter::new(SAMPLE_RATE);
        filter.set_cutoff(100.0);

        // Alternating signal (high frequency)
        let mut sum = 0.0;
        for i in 0..100 {
            let input = if i % 2 == 0 { 1.0 } else { -1.0 };
            let output = filter.process(input);
            sum += output.abs();
        }

        let avg_output = sum / 100.0;

        // High frequency content should be attenuated
        assert!(avg_output < 0.5);
    }

    #[test]
    fn test_filter_state_persistence() {
        let mut filter = Filter::new(SAMPLE_RATE);

        filter.process(1.0);
        let z_after_first = filter.z;

        filter.process(1.0);
        let z_after_second = filter.z;

        // State should change between calls
        assert_ne!(z_after_first, 0.0);
        assert_ne!(z_after_first, z_after_second);
    }

    #[test]
    fn test_filter_zero_input() {
        let mut filter = Filter::new(SAMPLE_RATE);

        // Set initial state
        filter.process(1.0);

        // Feed zeros
        let output1 = filter.process(0.0);
        let output2 = filter.process(0.0);

        // Output should decay toward zero
        assert!(output1 > output2);
        assert!(output2 > 0.0);
    }

    #[test]
    fn test_filter_negative_input() {
        let mut filter = Filter::new(SAMPLE_RATE);
        filter.set_cutoff(1000.0);

        let output = filter.process(-1.0);

        // Should handle negative inputs
        assert!(output < 0.0);
        assert!(output > -1.0);
    }

    #[test]
    fn test_module_trait() {
        let mut filter = Filter::new(SAMPLE_RATE);
        let input = 0.5;
        let output = filter.process(input);

        // Output should be less than input (smoothed)
        assert!(output < input);
        assert!(output >= 0.0);
    }

    #[test]
    fn test_filter_impulse_response() {
        let mut filter = Filter::new(SAMPLE_RATE);
        filter.set_cutoff(1000.0);

        // Impulse
        let output1 = filter.process(1.0);
        let output2 = filter.process(0.0);
        let output3 = filter.process(0.0);
        let output4 = filter.process(0.0);

        // Output should decay after impulse
        assert!(output1 > 0.0);
        assert!(output1 > output2);
        assert!(output2 > output3);
        assert!(output3 > output4);
    }

    #[test]
    fn test_filter_stability() {
        let mut filter = Filter::new(SAMPLE_RATE);
        filter.set_cutoff(10000.0); // High cutoff

        // Process many samples with varying input
        for i in 0..10000 {
            let input = ((i as f32) * 0.01).sin();
            let output = filter.process(input);

            // Output should remain bounded
            assert!(output.abs() <= 1.5);
        }
    }

    #[test]
    fn test_smoother_starts_on_the_cutoff() {
        let filter = Filter::new(SAMPLE_RATE);
        assert_eq!(filter.cutoff_smoother.current, filter.cutoff);
        assert_eq!(filter.cutoff_smoother.target, filter.cutoff);
    }

    /// The smoother is there to stop a cutoff sweep from clicking, so
    /// `process_sample` has to advance it and track its target.
    #[test]
    fn test_cutoff_changes_glide() {
        let mut filter = Filter::new(SAMPLE_RATE);
        filter.set_cutoff(8000.0);
        for _ in 0..50000 {
            filter.process(0.0);
        }

        filter.set_cutoff(100.0);
        assert_eq!(filter.cutoff_smoother.target, 100.0);
        assert!(
            filter.cutoff_smoother.current > 7000.0,
            "the cutoff in use jumped to {}",
            filter.cutoff_smoother.current
        );

        for _ in 0..50000 {
            filter.process(0.0);
        }
        assert!(
            (filter.cutoff_smoother.current - 100.0).abs() < 1.0,
            "the cutoff should reach its target, got {}",
            filter.cutoff_smoother.current
        );
    }

    #[test]
    fn test_cutoff_clamped() {
        let mut filter = Filter::new(SAMPLE_RATE);

        filter.set_cutoff(100_000.0);
        assert_eq!(filter.cutoff, SAMPLE_RATE * 0.45);

        filter.set_cutoff(0.1);
        assert_eq!(filter.cutoff, 1.0);
    }
}
