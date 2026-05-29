use crate::patch::Module;

pub struct Distortion {
    pub drive: f32,
    pub output_gain: f32,
}

impl Default for Distortion {
    fn default() -> Self {
        Self {
            drive: 10.0,
            output_gain: 1.0,
        }
    }
}

impl Distortion {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Module for Distortion {
    fn process(&mut self, input: f32) -> f32 {
        let x = input * self.drive;
        let y = libm::tanh(x as f64);
        y as f32 * self.output_gain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-5;

    #[test]
    fn test_distortion_new() {
        let dist = Distortion::new();
        assert_eq!(dist.drive, 10.0);
        assert_eq!(dist.output_gain, 1.0);
    }

    #[test]
    fn test_zero_input() {
        let mut dist = Distortion::new();
        let output = dist.process(0.0);
        assert!((output - 0.0).abs() < EPSILON);
    }

    #[test]
    fn test_soft_clipping() {
        let mut dist = Distortion::new();
        dist.drive = 10.0;

        // Large input should be soft-clipped by tanh
        let output = dist.process(10.0);

        // tanh saturates at ±1, so output should be close to 1.0
        assert!(output > 0.99);
        assert!(output <= 1.0);
    }

    #[test]
    fn test_negative_input() {
        let mut dist = Distortion::new();
        dist.drive = 5.0;

        let output = dist.process(-0.5);

        // Should handle negative inputs symmetrically
        assert!(output < 0.0);
    }

    #[test]
    fn test_symmetry() {
        let mut dist = Distortion::new();
        dist.drive = 8.0;

        let input = 0.3;
        let output_pos = dist.process(input);
        let output_neg = dist.process(-input);

        // tanh is symmetric: tanh(-x) = -tanh(x)
        assert!((output_pos + output_neg).abs() < EPSILON);
    }

    #[test]
    fn test_drive_effect() {
        let mut dist_low = Distortion::new();
        dist_low.drive = 1.0;

        let mut dist_high = Distortion::new();
        dist_high.drive = 20.0;

        let input = 0.5;

        let output_low = dist_low.process(input);
        let output_high = dist_high.process(input);

        // Higher drive should push signal closer to saturation
        assert!(output_high.abs() > output_low.abs());
    }

    #[test]
    fn test_output_gain() {
        let mut dist = Distortion::new();
        dist.drive = 5.0;
        dist.output_gain = 0.5;

        let input = 0.1;
        let output = dist.process(input);

        // Output gain should scale the result
        dist.output_gain = 1.0;
        let output_unity = dist.process(input);

        assert!((output * 2.0 - output_unity).abs() < 0.01);
    }

    #[test]
    fn test_linear_region() {
        let mut dist = Distortion::new();
        dist.drive = 1.0;

        // Small input should be approximately linear
        let input = 0.01;
        let output = dist.process(input);

        // For small x, tanh(x) ≈ x
        assert!((output - input).abs() < 0.001);
    }

    #[test]
    fn test_saturation_positive() {
        let mut dist = Distortion::new();
        dist.drive = 100.0;

        let output = dist.process(1.0);

        // Should saturate close to 1.0
        assert!(output > 0.999);
    }

    #[test]
    fn test_saturation_negative() {
        let mut dist = Distortion::new();
        dist.drive = 100.0;

        let output = dist.process(-1.0);

        // Should saturate close to -1.0
        assert!(output < -0.999);
    }

    #[test]
    fn test_module_trait() {
        let mut dist = Distortion::new();
        let input = 0.5;
        let output = dist.process(input);

        // Output should be bounded by tanh
        assert!(output.abs() <= 1.0);
    }

    #[test]
    fn test_harmonic_generation() {
        let mut dist = Distortion::new();
        dist.drive = 20.0;

        // Process sine-like input
        let mut max_out = 0.0_f32;
        for i in 0..100 {
            let phase = (i as f32) * 0.1;
            let sample = libm::sinf(phase);
            let output = dist.process(sample);
            max_out = max_out.max(output.abs());
        }

        // Distortion should add harmonics (non-linear processing)
        // Output should reach near saturation
        assert!(max_out > 0.9);
    }

    #[test]
    fn test_zero_drive() {
        let mut dist = Distortion::new();
        dist.drive = 0.0;

        let output = dist.process(1.0);

        // Zero drive should give zero output
        assert_eq!(output, 0.0);
    }

    #[test]
    fn test_various_inputs() {
        let mut dist = Distortion::new();
        dist.drive = 5.0;

        let inputs = [-1.0, -0.5, -0.1, 0.0, 0.1, 0.5, 1.0];

        for input in inputs {
            let output = dist.process(input);

            // Output should always be bounded
            assert!((-1.0..=1.0).contains(&output));

            // Sign should be preserved (except for zero)
            if input.abs() > EPSILON {
                assert_eq!(output.signum(), input.signum());
            }
        }
    }

    #[test]
    fn test_high_output_gain() {
        let mut dist = Distortion::new();
        dist.drive = 1.0;
        dist.output_gain = 2.0;

        let input = 0.1;
        let output = dist.process(input);

        // Output can exceed ±1 with output_gain > 1
        assert!(output.abs() <= 2.0);
    }
}
