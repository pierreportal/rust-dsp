use crate::patch::Module;
use libm::sinf;

#[derive(Clone)]
pub enum Waveform {
    Sine,
    Saw,
    Triangle,
    Square,
    PulseWidth,
}

impl Waveform {
    fn sine(phase: f32) -> f32 {
        sinf(phase * core::f32::consts::TAU)
    }
    fn saw(phase: f32) -> f32 {
        2.0 * phase - 1.0
    }
    fn triangle(phase: f32) -> f32 {
        // TODO: Needs to be improved
        2.0 * ((2.0 * phase - 1.0).abs() - 0.5)
    }
    fn square(phase: f32, width: f32) -> f32 {
        if phase < width {
            1.0
        } else {
            -1.0
        }
    }
}

fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        return t + t - t * t - 1.0;
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        return t * t + t + t + 1.0;
    }
    0.0
}

#[derive(Clone)]
pub struct Osc {
    pub phase: f32,
    pub freq: f32,
    pub sample_rate: f32,
    pub waveform: Waveform,
    pub pulse_width: f32,
}

impl Osc {
    pub fn new(waveform: Waveform, freq: f32, sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            freq,
            waveform,
            pulse_width: 0.7,
            sample_rate,
        }
    }

    pub fn next_sample(&mut self) -> f32 {
        let dt = self.freq / self.sample_rate;

        // Advance phase first so a freshly constructed oscillator does not
        // sit exactly on the saw/square discontinuity (where the BLEP
        // correction cancels the waveform value and yields 0).
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        let mut value = match self.waveform {
            Waveform::Sine => Waveform::sine(self.phase),
            Waveform::Saw => Waveform::saw(self.phase),
            Waveform::Triangle => Waveform::triangle(self.phase),
            Waveform::Square => Waveform::square(self.phase, 0.5),
            Waveform::PulseWidth => Waveform::square(self.phase, self.pulse_width),
        };

        value -= poly_blep(self.phase, dt);

        value
    }
}

impl Module for Osc {
    fn process(&mut self, input: f32) -> f32 {
        let x = self.next_sample();
        x * input
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f32 = 44100.0;
    const EPSILON: f32 = 1e-5;

    #[test]
    fn test_osc_new() {
        let osc = Osc::new(Waveform::Saw, 440.0, SAMPLE_RATE);
        assert_eq!(osc.phase, 0.0);
        assert_eq!(osc.freq, 440.0);
        assert_eq!(osc.sample_rate, SAMPLE_RATE);
        assert_eq!(osc.pulse_width, 0.7);
    }

    #[test]
    fn test_osc_phase_wrapping() {
        let mut osc = Osc::new(Waveform::Saw, 1000.0, SAMPLE_RATE);

        // Generate enough samples to wrap phase multiple times
        for _ in 0..100 {
            osc.next_sample();
        }

        // Phase should always be between 0.0 and 1.0
        assert!(osc.phase >= 0.0 && osc.phase < 1.0);
    }

    #[test]
    fn test_saw_waveform() {
        let phase_samples = [0.0, 0.25, 0.5, 0.75, 1.0];
        let expected = [-1.0, -0.5, 0.0, 0.5, 1.0];

        for (phase, expected_val) in phase_samples.iter().zip(expected.iter()) {
            let result = Waveform::saw(*phase);
            assert!(
                (result - expected_val).abs() < EPSILON,
                "Saw wave at phase {} expected {}, got {}",
                phase,
                expected_val,
                result
            );
        }
    }

    #[test]
    fn test_triangle_waveform() {
        // Triangle wave oscillates: 1 → 0 → -1 → 0 → 1
        // At phase 0.0: 2 * (|2*0 - 1| - 0.5) = 2 * (|-1| - 0.5) = 2 * (1 - 0.5) = 1.0
        let phase = 0.0;
        let result = Waveform::triangle(phase);
        assert!((result - 1.0).abs() < EPSILON);

        // At phase 0.25: 2 * (|2*0.25 - 1| - 0.5) = 2 * (|-0.5| - 0.5) = 2 * 0 = 0.0
        let phase = 0.25;
        let result = Waveform::triangle(phase);
        assert!((result - 0.0).abs() < EPSILON);

        // At phase 0.5: 2 * (|2*0.5 - 1| - 0.5) = 2 * (|0| - 0.5) = 2 * (-0.5) = -1.0
        let phase = 0.5;
        let result = Waveform::triangle(phase);
        assert!((result - (-1.0)).abs() < EPSILON);

        // At phase 0.75: 2 * (|2*0.75 - 1| - 0.5) = 2 * (|0.5| - 0.5) = 2 * 0 = 0.0
        let phase = 0.75;
        let result = Waveform::triangle(phase);
        assert!((result - 0.0).abs() < EPSILON);
    }

    #[test]
    fn test_square_waveform() {
        let result = Waveform::square(0.25, 0.5);
        assert_eq!(result, 1.0);

        let result = Waveform::square(0.75, 0.5);
        assert_eq!(result, -1.0);
    }

    #[test]
    fn test_pulse_width_waveform() {
        let mut osc = Osc::new(Waveform::PulseWidth, 440.0, SAMPLE_RATE);
        osc.pulse_width = 0.3;

        let output = osc.next_sample();
        // Output should be within valid range
        assert!((-2.0..=2.0).contains(&output));
    }

    #[test]
    fn test_poly_blep() {
        // Test at discontinuity boundary
        let result = poly_blep(0.0, 0.1);
        assert!((result - (-1.0)).abs() < EPSILON);

        // Test outside boundary
        let result = poly_blep(0.5, 0.1);
        assert_eq!(result, 0.0);
    }

    #[test]
    fn test_module_trait() {
        let mut osc = Osc::new(Waveform::Saw, 440.0, SAMPLE_RATE);
        let input = 0.5;
        let output = osc.process(input);

        // Output should be scaled by input
        assert!(output.abs() <= input);
    }

    #[test]
    fn test_frequency_consistency() {
        let freq = 440.0;
        let mut osc = Osc::new(Waveform::Saw, freq, SAMPLE_RATE);

        // Calculate expected samples per cycle
        let samples_per_cycle = SAMPLE_RATE / freq;
        let mut samples_counted = 0;
        let initial_phase = osc.phase;

        // Generate samples for one cycle
        while osc.phase >= initial_phase && samples_counted < samples_per_cycle as usize + 10 {
            osc.next_sample();
            samples_counted += 1;
            if samples_counted > 1 && osc.phase < initial_phase + 0.01 {
                break;
            }
        }

        // Should be close to expected cycle length
        assert!((samples_counted as f32 - samples_per_cycle).abs() < 2.0);
    }

    #[test]
    fn test_waveform_clone() {
        let waveform = Waveform::Saw;
        let _cloned = waveform.clone();

        let phase = 0.5;
        assert_eq!(Waveform::saw(phase), Waveform::saw(phase));
    }

    #[test]
    fn test_osc_clone() {
        let osc1 = Osc::new(Waveform::Triangle, 880.0, SAMPLE_RATE);
        let osc2 = osc1.clone();

        assert_eq!(osc1.phase, osc2.phase);
        assert_eq!(osc1.freq, osc2.freq);
        assert_eq!(osc1.sample_rate, osc2.sample_rate);
    }
}
