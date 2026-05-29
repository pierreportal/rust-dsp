pub trait Module {
    fn process(&mut self, input: f32) -> f32;
}

#[macro_export]
macro_rules! patch {
    ($($module:expr)=>+ $(,)?) => {{
        move |mut input| {
            $(
                input = $module.process(input);
            )+
            input
        }
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock module for testing
    struct Gain {
        gain: f32,
    }

    impl Gain {
        fn new(gain: f32) -> Self {
            Self { gain }
        }
    }

    impl Module for Gain {
        fn process(&mut self, input: f32) -> f32 {
            input * self.gain
        }
    }

    struct Offset {
        offset: f32,
    }

    impl Offset {
        fn new(offset: f32) -> Self {
            Self { offset }
        }
    }

    impl Module for Offset {
        fn process(&mut self, input: f32) -> f32 {
            input + self.offset
        }
    }

    #[test]
    fn test_single_module_patch() {
        let mut gain = Gain::new(2.0);
        let mut process = patch!(gain);

        let output = process(0.5);
        assert_eq!(output, 1.0);
    }

    #[test]
    fn test_two_module_patch() {
        let mut gain = Gain::new(2.0);
        let mut offset = Offset::new(1.0);
        let mut process = patch!(gain => offset);

        let output = process(0.5);
        // (0.5 * 2.0) + 1.0 = 2.0
        assert_eq!(output, 2.0);
    }

    #[test]
    fn test_multiple_module_patch() {
        let mut gain1 = Gain::new(2.0);
        let mut gain2 = Gain::new(3.0);
        let mut offset = Offset::new(1.0);
        let mut process = patch!(gain1 => gain2 => offset);

        let output = process(1.0);
        // ((1.0 * 2.0) * 3.0) + 1.0 = 7.0
        assert_eq!(output, 7.0);
    }

    #[test]
    fn test_patch_zero_input() {
        let mut gain = Gain::new(5.0);
        let mut process = patch!(gain);

        let output = process(0.0);
        assert_eq!(output, 0.0);
    }

    #[test]
    fn test_patch_negative_input() {
        let mut gain = Gain::new(2.0);
        let mut process = patch!(gain);

        let output = process(-0.5);
        assert_eq!(output, -1.0);
    }

    #[test]
    fn test_patch_ordering() {
        let mut offset = Offset::new(1.0);
        let mut gain = Gain::new(2.0);

        // offset then gain: (0.5 + 1.0) * 2.0 = 3.0
        let mut process1 = patch!(offset => gain);
        let output1 = process1(0.5);

        let mut offset2 = Offset::new(1.0);
        let mut gain2 = Gain::new(2.0);

        // gain then offset: (0.5 * 2.0) + 1.0 = 2.0
        let mut process2 = patch!(gain2 => offset2);
        let output2 = process2(0.5);

        assert_eq!(output1, 3.0);
        assert_eq!(output2, 2.0);
        assert_ne!(output1, output2);
    }

    #[test]
    fn test_patch_with_trailing_comma() {
        let mut gain = Gain::new(2.0);
        let mut process = patch!(gain,);

        let output = process(0.5);
        assert_eq!(output, 1.0);
    }

    #[test]
    fn test_patch_multiple_calls() {
        let mut gain = Gain::new(2.0);
        let mut process = patch!(gain);

        // Multiple calls should work
        let output1 = process(1.0);
        let output2 = process(2.0);
        let output3 = process(0.5);

        assert_eq!(output1, 2.0);
        assert_eq!(output2, 4.0);
        assert_eq!(output3, 1.0);
    }
}
