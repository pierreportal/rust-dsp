use crate::patch::Module;

pub struct Filter {
    pub cutoff: f32,
    pub z: f32,
    pub sample_rate: f32,
}

impl Filter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            cutoff: 2000.0,
            z: 0.0,
            sample_rate,
        }
    }
}

impl Module for Filter {
    fn process(&mut self, input: f32) -> f32 {
        let x = libm::expf(-2.0 * core::f32::consts::PI * self.cutoff / self.sample_rate);
        let a = 1.0 - x;

        self.z = self.z + a * (input - self.z);
        self.z
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
        filter.cutoff = 1000.0;

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
        filter.cutoff = 100.0; // Low cutoff for strong smoothing

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
        filter.cutoff = 100.0;

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
    fn test_filter_cutoff_effect() {
        let mut filter_low = Filter::new(SAMPLE_RATE);
        filter_low.cutoff = 100.0;

        let mut filter_high = Filter::new(SAMPLE_RATE);
        filter_high.cutoff = 5000.0;

        let input = 1.0;

        let output_low = filter_low.process(input);
        let output_high = filter_high.process(input);

        // Higher cutoff should allow more of the signal through initially
        assert!(output_high > output_low);
    }

    #[test]
    fn test_filter_negative_input() {
        let mut filter = Filter::new(SAMPLE_RATE);
        filter.cutoff = 1000.0;

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
        filter.cutoff = 1000.0;

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
        filter.cutoff = 10000.0; // High cutoff

        // Process many samples with varying input
        for i in 0..10000 {
            let input = ((i as f32) * 0.01).sin();
            let output = filter.process(input);

            // Output should remain bounded
            assert!(output.abs() <= 1.5);
        }
    }
}
