pub struct Smoother {
    pub current: f32,
    pub target: f32,
    pub coeff: f32,
}

impl Smoother {
    pub fn new(initial: f32, coeff: f32) -> Self {
        Self {
            current: initial,
            target: initial,
            coeff,
        }
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    pub fn next_sample(&mut self) -> f32 {
        self.current += (self.target - self.current) * self.coeff;
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-5;

    #[test]
    fn test_smoother_new() {
        let smoother = Smoother::new(0.5, 0.1);
        assert_eq!(smoother.current, 0.5);
        assert_eq!(smoother.target, 0.5);
        assert_eq!(smoother.coeff, 0.1);
    }

    #[test]
    fn test_set_target() {
        let mut smoother = Smoother::new(0.0, 0.1);
        smoother.set_target(1.0);
        assert_eq!(smoother.target, 1.0);
    }

    #[test]
    fn test_converges_to_target() {
        let mut smoother = Smoother::new(0.0, 0.5);
        smoother.set_target(1.0);

        let mut last_value = smoother.current;
        for _ in 0..100 {
            let value = smoother.next_sample();
            assert!(value > last_value - EPSILON);
            last_value = value;
        }

        // Should be very close to target after 100 iterations
        assert!((smoother.current - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_monotonic_increase() {
        let mut smoother = Smoother::new(0.0, 0.2);
        smoother.set_target(1.0);

        let mut last_value = 0.0;
        for _ in 0..20 {
            let value = smoother.next_sample();
            assert!(value >= last_value - EPSILON);
            assert!(value <= 1.0 + EPSILON);
            last_value = value;
        }
    }

    #[test]
    fn test_monotonic_decrease() {
        let mut smoother = Smoother::new(1.0, 0.2);
        smoother.set_target(0.0);

        let mut last_value = 1.0;
        for _ in 0..20 {
            let value = smoother.next_sample();
            assert!(value <= last_value + EPSILON);
            assert!(value >= 0.0 - EPSILON);
            last_value = value;
        }
    }

    #[test]
    fn test_coeff_affects_speed() {
        let mut smoother_slow = Smoother::new(0.0, 0.1);
        smoother_slow.set_target(1.0);

        let mut smoother_fast = Smoother::new(0.0, 0.9);
        smoother_fast.set_target(1.0);

        let slow_value = smoother_slow.next_sample();
        let fast_value = smoother_fast.next_sample();

        // Faster coefficient should move closer to target
        assert!(fast_value > slow_value);
    }

    #[test]
    fn test_already_at_target() {
        let mut smoother = Smoother::new(0.5, 0.1);
        smoother.set_target(0.5);

        for _ in 0..10 {
            let value = smoother.next_sample();
            assert!((value - 0.5).abs() < EPSILON);
        }
    }

    #[test]
    fn test_negative_values() {
        let mut smoother = Smoother::new(-1.0, 0.3);
        smoother.set_target(-0.5);

        let value = smoother.next_sample();
        assert!(value > -1.0);
        assert!(value < -0.5);
    }

    #[test]
    fn test_target_change_mid_smoothing() {
        let mut smoother = Smoother::new(0.0, 0.2);
        smoother.set_target(1.0);

        // Smooth halfway
        for _ in 0..5 {
            smoother.next_sample();
        }

        let mid_value = smoother.current;

        // Change target
        smoother.set_target(0.5);

        let value_after_change = smoother.next_sample();

        // Should move toward new target
        assert!(value_after_change < mid_value);
    }

    #[test]
    fn test_exponential_approach() {
        let mut smoother = Smoother::new(0.0, 0.5);
        smoother.set_target(1.0);

        let value1 = smoother.next_sample();
        let value2 = smoother.next_sample();
        let value3 = smoother.next_sample();

        let diff1 = value2 - value1;
        let diff2 = value3 - value2;

        // Steps should get smaller (exponential approach)
        assert!(diff2 < diff1);
    }

    #[test]
    fn test_small_coeff() {
        let mut smoother = Smoother::new(0.0, 0.01);
        smoother.set_target(1.0);

        let value = smoother.next_sample();

        // Very small coefficient should make very small steps
        assert!(value < 0.1);
        assert!(value > 0.0);
    }

    #[test]
    fn test_large_coeff() {
        let mut smoother = Smoother::new(0.0, 0.99);
        smoother.set_target(1.0);

        let value = smoother.next_sample();

        // Large coefficient should jump very close to target
        assert!(value > 0.9);
    }

    #[test]
    fn test_multiple_cycles() {
        let mut smoother = Smoother::new(0.0, 0.3);

        // First target
        smoother.set_target(1.0);
        for _ in 0..20 {
            smoother.next_sample();
        }
        assert!((smoother.current - 1.0).abs() < 0.01);

        // Second target
        smoother.set_target(0.0);
        for _ in 0..20 {
            smoother.next_sample();
        }
        assert!((smoother.current - 0.0).abs() < 0.01);

        // Third target
        smoother.set_target(0.5);
        for _ in 0..20 {
            smoother.next_sample();
        }
        assert!((smoother.current - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_precision() {
        let mut smoother = Smoother::new(0.0, 0.5);
        smoother.set_target(0.123456);

        for _ in 0..100 {
            smoother.next_sample();
        }

        // Should converge to precise target
        assert!((smoother.current - 0.123456).abs() < 0.0001);
    }
}
