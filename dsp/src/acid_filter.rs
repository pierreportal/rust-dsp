use crate::patch::Module;
use crate::smoother::Smoother;
use core::f32::consts::PI;
use libm::tanf;

/// A 4-pole (24 dB/oct) resonant low-pass filter modelled after the RBJ
/// "cascaded one-pole" topology, which is the classic sound behind the
/// Roland TB-303 diode-ladder bass.
///
/// Instead of a single biquad, we cascade four one-pole low-pass stages so
/// the rolloff becomes a steep 24 dB/oct. Resonance is implemented as
/// feedback of the final stage's output back into the input, just like the
/// ladder filter. This produces the squelchy, self-oscillating character
/// that defines the acid bass sound.
///
/// The `resonance` value (0.0 .. ~0.95):
///   - 0.0  -> flat, no peak at the cutoff
///   - 0.5  -> a moderate, fat squelch
///   - 0.9  -> pronounced resonance that can exceed the input level
///   - ~0.95 -> borderline self-oscillation (the classic screaming acid timbre)
pub struct AcidFilter {
    pub sample_rate: f32,
    pub cutoff: f32,
    pub resonance: f32,
    pub cutoff_smoother: Smoother,
    pub resonance_smoother: Smoother,

    g: f32,
    feedback: f32,
    s1: f32,
    s2: f32,
    s3: f32,
    s4: f32,
}

impl AcidFilter {
    pub fn new(sample_rate: f32) -> Self {
        let mut filter = Self {
            sample_rate,
            cutoff: 300.0,
            resonance: 0.5,
            cutoff_smoother: Smoother::new(300.0, 0.0005),
            resonance_smoother: Smoother::new(0.5, 0.0005),
            g: 0.0,
            feedback: 0.0,
            s1: 0.0,
            s2: 0.0,
            s3: 0.0,
            s4: 0.0,
        };
        filter.update();
        filter
    }

    pub fn set_cutoff(&mut self, cutoff: f32) {
        self.cutoff = cutoff.clamp(20.0, self.sample_rate * 0.45);
        self.update();
    }

    pub fn set_resonance(&mut self, resonance: f32) {
        // Hard-clamp below 1.0 so the filter never explodes. The sweet spot
        // for acid is usually around 0.6 - 0.9.
        self.resonance = resonance.clamp(0.0, 0.95);
        self.update();
    }

    fn update(&mut self) {
        // Bilinear-style transform of the cutoff into a one-pole coefficient.
        // The cascade stays stable for cutoff < Nyquist.
        let t = tanf(PI * self.cutoff / self.sample_rate);
        self.g = t / (1.0 + t);

        // Feedback gain for the resonance. 4.0 is the classic "4012"/TB-303
        // ladder value; scaling by resonance lets us drive it to self-oscillation.
        self.feedback = self.resonance * 4.0;
    }

    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
        self.s3 = 0.0;
        self.s4 = 0.0;
    }

    pub fn process_sample(&mut self, input: f32) -> f32 {
        // Subtract the resonance feedback from the incoming signal, exactly
        // how the ladder filter's differential pair does it.
        let x = input - self.feedback;

        // Cascade of four one-pole low-pass stages.
        self.s1 += self.g * (x - self.s1);
        self.s2 += self.g * (self.s1 - self.s2);
        self.s3 += self.g * (self.s2 - self.s3);
        self.s4 += self.g * (self.s3 - self.s4);

        self.s4
    }
}

impl Module for AcidFilter {
    fn process(&mut self, input: f32) -> f32 {
        self.process_sample(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f32 = 44100.0;

    #[test]
    fn test_new() {
        let filter = AcidFilter::new(SAMPLE_RATE);
        assert_eq!(filter.sample_rate, SAMPLE_RATE);
        assert_eq!(filter.cutoff, 300.0);
        assert_eq!(filter.resonance, 0.5);
    }

    #[test]
    fn test_dc_passes_through() {
        let mut filter = AcidFilter::new(SAMPLE_RATE);
        filter.set_cutoff(1000.0);
        filter.set_resonance(0.0);

        let mut output = 0.0;
        for _ in 0..20000 {
            output = filter.process_sample(1.0);
        }
        // A low-pass passes DC unchanged (unity at f=0).
        assert!((output - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_zero_resonance_settles_to_dc() {
        let mut filter = AcidFilter::new(SAMPLE_RATE);
        filter.set_cutoff(500.0);
        filter.set_resonance(0.0);

        let mut output = 0.0;
        for _ in 0..5000 {
            output = filter.process_sample(1.0);
        }
        assert!(output > 0.9);
    }

    #[test]
    fn test_high_freq_attenuation() {
        // Feed an alternating signal (the highest audio frequency) and check
        // that the 4-pole filter attenuates it strongly at a low cutoff.
        let mut filter = AcidFilter::new(SAMPLE_RATE);
        filter.set_cutoff(100.0);
        filter.set_resonance(0.0);

        let mut sum = 0.0;
        for i in 0..1000 {
            let input = if i % 2 == 0 { 1.0 } else { -1.0 };
            sum += filter.process_sample(input).abs();
        }
        let avg = sum / 1000.0;
        assert!(avg < 0.1, "expected strong attenuation, got {}", avg);
    }

    #[test]
    fn test_resonance_boost() {
        // Higher resonance should produce a bigger peak around the cutoff
        // (i.e. larger magnitude signal for the same input).
        let input_sig = |res: f32| -> f32 {
            let mut f = AcidFilter::new(SAMPLE_RATE);
            f.set_cutoff(1500.0);
            f.set_resonance(res);
            let mut peak = 0.0_f32;
            // Drive it with a sine at the cutoff to excite the resonance.
            let mut phase = 0.0_f32;
            for _ in 0..2000 {
                let sample = (phase * core::f32::consts::TAU).sin();
                phase += 1500.0 / SAMPLE_RATE;
                let out = f.process_sample(sample);
                peak = peak.max(out.abs());
            }
            peak
        };

        let low = input_sig(0.0);
        let high = input_sig(0.9);
        assert!(
            high > low,
            "resonance should boost the signal near cutoff: low={low} high={high}"
        );
    }

    #[test]
    fn test_resonance_clamped() {
        let mut filter = AcidFilter::new(SAMPLE_RATE);
        filter.set_resonance(5.0);
        assert_eq!(filter.resonance, 0.95);

        filter.set_resonance(-3.0);
        assert_eq!(filter.resonance, 0.0);
    }

    #[test]
    fn test_cutoff_clamped() {
        let mut filter = AcidFilter::new(SAMPLE_RATE);
        filter.set_cutoff(100000.0);
        assert!(filter.cutoff <= SAMPLE_RATE * 0.45);

        filter.set_cutoff(1.0);
        assert_eq!(filter.cutoff, 20.0);
    }

    #[test]
    fn test_stability_at_high_resonance() {
        let mut filter = AcidFilter::new(SAMPLE_RATE);
        filter.set_cutoff(500.0);
        filter.set_resonance(0.95);

        for i in 0..20000 {
            let input = ((i as f32) * 0.05).sin();
            let output = filter.process_sample(input);
            assert!(output.is_finite());
            assert!(
                output.abs() < 50.0,
                "output should stay bounded, got {}",
                output
            );
        }
    }

    #[test]
    fn test_module_trait() {
        let mut filter = AcidFilter::new(SAMPLE_RATE);
        filter.set_cutoff(1000.0);
        filter.set_resonance(0.0);

        // The Module impl should delegate straight to process_sample and
        // produce the same value a single call would.
        let direct = filter.process_sample(0.5);
        filter.reset();

        let through_module = filter.process(0.5);
        assert_eq!(through_module, direct);
    }

    #[test]
    fn test_zero_input_decays() {
        let mut filter = AcidFilter::new(SAMPLE_RATE);
        filter.set_cutoff(800.0);
        filter.set_resonance(0.0);

        // Charge the filter with a 1.0 input...
        for _ in 0..5000 {
            filter.process_sample(1.0);
        }
        // ...then feed zeros; the cascade should decay toward zero.
        let mut out1 = filter.process_sample(0.0);
        for _ in 0..1000 {
            out1 = filter.process_sample(0.0);
        }
        assert!(out1.abs() < 0.5);
    }
}
