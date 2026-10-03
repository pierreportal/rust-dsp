use crate::patch::Module;
use crate::smoother::Smoother;
use core::f32::consts::PI;
use libm::tanf;

/// Internal oversampling factor.
///
/// The feedback loop closes around the whole cascade, so its stability
/// depends on the pole frequency staying well below the rate the filter runs
/// at. Stilson and Smith showed four times oversampling is enough to keep the
/// loop stable across the entire cutoff range at self-oscillating resonance.
const OVERSAMPLE: f32 = 4.0;

/// Feedback gain at which the ladder breaks into self-oscillation. The TB-303
/// and the 4012 both live here, so `resonance` is scaled up to this value.
const SELF_OSCILLATION_FEEDBACK: f32 = 4.0;

/// Where the -3 dB point of four cascaded one-pole sections sits, relative to
/// the corner of a single section.
///
/// Each section contributes `1/sqrt(1 + (f/fp)^2)`, so the cascade has
/// magnitude `(1 + (f/fp)^2)^-2`, i.e. power `(1 + (f/fp)^2)^-4`. Halving the
/// power gives `(1 + (f/fp)^2)^4 = 2`, hence `f = fp * 0.435`. Placing the
/// sections above the requested cutoff by `1/0.435` is what puts the filter's
/// -3 dB point where the caller asked for it.
const CASCADE_CORNER_RATIO: f32 = 0.435;

/// A 4-pole (24 dB/oct) resonant low-pass filter modelled after the RBJ
/// "cascaded one-pole" topology, which is the classic sound behind the
/// Roland TB-303 diode-ladder bass.
///
/// Instead of a single biquad, we cascade four one-pole low-pass sections so
/// the rolloff becomes a steep 24 dB/oct. Resonance is implemented as feedback
/// of the final section's output back into the input, just like the ladder
/// filter. This produces the squelchy, self-oscillating character that defines
/// the acid bass sound.
///
/// `cutoff` is the -3 dB point of the whole cascade, not of one section; see
/// [`CASCADE_CORNER_RATIO`]. Above roughly `sample_rate * 0.196` every
/// section is already pinned at Nyquist and the corner stops rising with the
/// parameter.
///
/// The `resonance` value (0.0 .. 0.95):
///   - 0.0  -> flat, no peak at the cutoff
///   - 0.5  -> a moderate, fat squelch
///   - 0.9  -> pronounced resonance that can exceed the input level
///   - 0.95 -> borderline self-oscillation (the classic screaming acid timbre)
///
/// Because the loop subtracts signal rather than a constant, input is scaled by
/// `1 + feedback` before the loop closes. Without that compensation the filter
/// would lose `1/(1 + feedback)` of its low-frequency level, which is a 13.6 dB
/// dip once resonance reaches its maximum.
pub struct AcidFilter {
    pub sample_rate: f32,
    pub cutoff: f32,
    pub resonance: f32,
    pub cutoff_smoother: Smoother,
    pub resonance_smoother: Smoother,

    g: f32,
    last_cutoff: f32,
    s1: f32,
    s2: f32,
    s3: f32,
    s4: f32,
}

impl AcidFilter {
    pub fn new(sample_rate: f32) -> Self {
        let cutoff = 300.0;
        let resonance = 0.5;

        Self {
            sample_rate,
            cutoff,
            resonance,
            cutoff_smoother: Smoother::new(cutoff, 0.0005),
            resonance_smoother: Smoother::new(resonance, 0.0005),
            g: 0.0,
            // NaN never compares equal to itself, so the first call always
            // takes the branch that computes the coefficient.
            last_cutoff: f32::NAN,
            s1: 0.0,
            s2: 0.0,
            s3: 0.0,
            s4: 0.0,
        }
    }

    pub fn set_cutoff(&mut self, cutoff: f32) {
        self.cutoff = cutoff.clamp(20.0, self.sample_rate * 0.45);
        self.cutoff_smoother.set_target(self.cutoff);
    }

    pub fn set_resonance(&mut self, resonance: f32) {
        // Hard-clamp below self-oscillation so the filter never runs away. The
        // sweet spot for acid is usually around 0.6 - 0.9.
        self.resonance = resonance.clamp(0.0, 0.95);
        self.resonance_smoother.set_target(self.resonance);
    }

    /// One-pole coefficient for a section whose corner is `pole_cutoff`.
    ///
    /// This is the bilinear transform of a single analog pole: the pole lands
    /// at `(1-t)/(1+t)`, so the section's gain is `1 - pole = 2t/(1+t)`. The
    /// cutoff is prewarped as `t = tan(w/2)` with `w = 2*pi*fc/rate`, which is
    /// what keeps the digital -3 dB point on the requested frequency. Leaving
    /// the factor of two out of the gain halves the coefficient and drops the
    /// corner to roughly half of `pole_cutoff`.
    fn section_coefficient(pole_cutoff: f32, rate: f32) -> f32 {
        let t = tanf(PI * pole_cutoff / rate);
        2.0 * t / (1.0 + t)
    }

    /// Coefficient for the given cutoff, cached so that a settled parameter
    /// costs nothing per sample.
    ///
    /// A section's corner is placed above the cutoff to compensate for the
    /// cascade (see [`CASCADE_CORNER_RATIO`]), and pinned at Nyquist so the
    /// bilinear prewarp stays on its stable branch.
    fn coefficient_for(&mut self, cutoff: f32) -> f32 {
        if cutoff != self.last_cutoff {
            self.last_cutoff = cutoff;
            let pole_cutoff = (cutoff / CASCADE_CORNER_RATIO).min(self.sample_rate * 0.45);
            self.g = Self::section_coefficient(pole_cutoff, self.sample_rate * OVERSAMPLE);
        }
        self.g
    }

    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
        self.s3 = 0.0;
        self.s4 = 0.0;
    }

    pub fn process_sample(&mut self, input: f32) -> f32 {
        // Both smoothers advance once per input sample, so glide time is not
        // stretched by the oversampling factor.
        let cutoff = self.cutoff_smoother.next_sample();
        let resonance = self.resonance_smoother.next_sample();

        let g = self.coefficient_for(cutoff);
        let feedback = resonance * SELF_OSCILLATION_FEEDBACK;

        let mut output = 0.0;
        for _ in 0..(OVERSAMPLE as usize) {
            // The ladder's differential pair subtracts the feedback *signal*,
            // not a constant, so `s4` has to be the live cascade output.
            let x = input * (1.0 + feedback) - feedback * self.s4;

            // Cascade of four one-pole low-pass sections.
            self.s1 += g * (x - self.s1);
            self.s2 += g * (self.s1 - self.s2);
            self.s3 += g * (self.s2 - self.s3);
            self.s4 += g * (self.s3 - self.s4);

            output = self.s4;
        }

        output
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
    fn test_smoothers_start_on_the_parameter_values() {
        let filter = AcidFilter::new(SAMPLE_RATE);
        assert_eq!(filter.cutoff_smoother.current, filter.cutoff);
        assert_eq!(filter.cutoff_smoother.target, filter.cutoff);
        assert_eq!(filter.resonance_smoother.current, filter.resonance);
        assert_eq!(filter.resonance_smoother.target, filter.resonance);
    }

    #[test]
    fn test_dc_passes_through() {
        let mut filter = AcidFilter::new(SAMPLE_RATE);
        filter.set_cutoff(1000.0);
        filter.set_resonance(0.0);

        let mut output = 0.0;
        for _ in 0..200000 {
            output = filter.process_sample(1.0);
        }
        // A low-pass passes DC unchanged (unity at f=0).
        assert!((output - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_dc_passes_through_at_maximum_resonance() {
        let mut filter = AcidFilter::new(SAMPLE_RATE);
        filter.set_cutoff(1000.0);
        filter.set_resonance(0.95);

        let mut output = 0.0;
        for _ in 0..200000 {
            output = filter.process_sample(1.0);
        }
        // The `1 + feedback` input scaling exists to keep this at unity; a
        // bare feedback loop would settle at 1/(1+3.8) instead.
        assert!((output - 1.0).abs() < 0.01, "got {output}");
    }

    #[test]
    fn test_silence_stays_silent_at_any_resonance() {
        for resonance in [0.0, 0.25, 0.5, 0.75, 0.95] {
            let mut filter = AcidFilter::new(SAMPLE_RATE);
            filter.set_cutoff(1000.0);
            filter.set_resonance(resonance);

            for _ in 0..50000 {
                assert_eq!(filter.process_sample(0.0), 0.0);
            }
        }
    }

    #[test]
    fn test_zero_resonance_settles_to_dc() {
        let mut filter = AcidFilter::new(SAMPLE_RATE);
        filter.set_cutoff(500.0);
        filter.set_resonance(0.0);

        let mut output = 0.0;
        for _ in 0..200000 {
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

        for _ in 0..200000 {
            filter.process_sample(0.0);
        }

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
            for _ in 0..50000 {
                let sample = libm::sinf(phase * core::f32::consts::TAU);
                phase += 1500.0 / SAMPLE_RATE;
                let out = f.process_sample(sample);
                peak = peak.max(out.abs());
            }
            peak
        };

        let low = input_sig(0.0);
        let high = input_sig(0.9);
        assert!(
            high > low * 1.5,
            "resonance should boost the signal near cutoff: low={low} high={high}"
        );
    }

    #[test]
    fn test_resonance_boosts_only_near_the_cutoff() {
        // Resonance lives in a narrow band. Far above the cutoff the response
        // must still collapse, otherwise the "boost" is just the old constant
        // offset reappearing.
        let peak_at = |freq: f32, resonance: f32| -> f32 {
            let mut f = AcidFilter::new(SAMPLE_RATE);
            f.set_cutoff(1000.0);
            f.set_resonance(resonance);
            let mut phase = 0.0f32;
            let step = core::f32::consts::TAU * freq / SAMPLE_RATE;
            let mut peak = 0.0f32;
            for _ in 0..100000 {
                let out = f.process_sample(libm::sinf(phase));
                phase += step;
                if phase >= core::f32::consts::TAU {
                    phase -= core::f32::consts::TAU;
                }
                peak = peak.max(out.abs());
            }
            peak
        };

        // Well above where the ladder's resonance lives: a 4-pole rolloff plus
        // a narrow resonant peak has to reach near-silence up here. If this
        // ever goes positive again, the feedback loop has gone back to
        // subtracting a constant.
        assert!(peak_at(10_000.0, 0.95) < 0.05, "far above cutoff");
        assert!(peak_at(18_000.0, 0.95) < 0.05, "far above cutoff");

        // The peak has to actually be there, not merely absent everywhere.
        let peak_low = peak_at(500.0, 0.95);
        let peak_mid = peak_at(1000.0, 0.95);
        let peak_high = peak_at(2000.0, 0.95);
        assert!(
            peak_high > peak_low && peak_high > peak_mid,
            "expected a peak above the cutoff: 500 Hz {peak_low}, \
             1000 Hz {peak_mid}, 2000 Hz {peak_high}"
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

        for i in 0..200000 {
            let input = libm::sinf((i as f32) * 0.05);
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
    fn test_stability_across_the_cutoff_range() {
        // The old feedback path went unstable as soon as the cutoff climbed,
        // because the loop ran at the same rate as the poles.
        for cutoff in [20.0, 100.0, 1000.0, 8000.0, 19_000.0] {
            for resonance in [0.0, 0.5, 0.9, 0.95] {
                let mut filter = AcidFilter::new(SAMPLE_RATE);
                filter.set_cutoff(cutoff);
                filter.set_resonance(resonance);

                for i in 0..50_000 {
                    let input = if i % 2 == 0 { 1.0 } else { -1.0 };
                    let output = filter.process_sample(input);
                    assert!(
                        output.is_finite() && output.abs() < 50.0,
                        "cutoff {cutoff} resonance {resonance} produced {output}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_module_trait() {
        // Drive two identically prepared filters with the same signal and
        // compare every sample. `reset` only clears the ladder state, so the
        // parameters still glide across calls; comparing a single call either
        // side of a `reset` would compare two different filter settings.
        let signal = |i: usize| libm::sinf(i as f32 * 0.037);

        let mut direct = AcidFilter::new(SAMPLE_RATE);
        direct.set_cutoff(1000.0);
        direct.set_resonance(0.7);

        let mut through_module = AcidFilter::new(SAMPLE_RATE);
        through_module.set_cutoff(1000.0);
        through_module.set_resonance(0.7);

        for i in 0..20_000 {
            let x = signal(i);
            assert_eq!(direct.process_sample(x), through_module.process(x));
        }
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

    #[test]
    fn test_cutoff_glides_instead_of_stepping() {
        let mut filter = AcidFilter::new(SAMPLE_RATE);
        filter.set_cutoff(800.0);
        filter.set_resonance(0.0);
        for _ in 0..200000 {
            filter.process_sample(0.0);
        }

        // Jump the parameter. The smoother target follows immediately, but the
        // value in use has to travel there over time.
        filter.set_cutoff(50.0);
        assert_eq!(filter.cutoff_smoother.target, 50.0);
        assert!(
            filter.cutoff_smoother.current > 700.0,
            "the cutoff in use should not jump, got {}",
            filter.cutoff_smoother.current
        );

        for _ in 0..200000 {
            filter.process_sample(0.0);
        }
        assert!(
            (filter.cutoff_smoother.current - 50.0).abs() < 1.0,
            "the cutoff should reach its target, got {}",
            filter.cutoff_smoother.current
        );
    }

    #[test]
    fn test_resonance_glides_instead_of_stepping() {
        let mut filter = AcidFilter::new(SAMPLE_RATE);
        filter.set_cutoff(1000.0);
        filter.set_resonance(0.0);
        for _ in 0..200000 {
            filter.process_sample(0.0);
        }

        filter.set_resonance(0.9);
        assert_eq!(filter.resonance_smoother.target, 0.9);
        assert!(filter.resonance_smoother.current < 0.05);

        for _ in 0..200000 {
            filter.process_sample(0.0);
        }
        assert!(
            (filter.resonance_smoother.current - 0.9).abs() < 0.01,
            "resonance should reach its target, got {}",
            filter.resonance_smoother.current
        );
    }
}
