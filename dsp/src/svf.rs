use crate::smoother::Smoother;
use core::f32::consts::PI;
use libm::{powf, tanf};

/// Q at `resonance` 0.0: a Butterworth response, with no resonant peak.
const Q_FLAT: f32 = core::f32::consts::FRAC_1_SQRT_2;

/// Q at `resonance` 1.0: loud and musical, still short of self-oscillation.
const Q_RESONANT: f32 = 20.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterMode {
    LowPass,
    HighPass,
    BandPass,
    Notch,
}

/// A two-pole state-variable filter in Zavalishin's topology-preserving
/// transform (TPT) form.
///
/// TPT solves the filter's difference equations implicitly, so the response
/// stays stable for any cutoff below Nyquist. The previous implementation used
/// the older Chamberlin structure, which is only stable while the integrator
/// coefficient `tan(w)` stays under roughly 1.4; at 44.1 kHz that meant it
/// began producing NaN at about 14 kHz even though `set_cutoff` accepted up to
/// 19.8 kHz.
///
/// `cutoff` is the -3 dB point. `resonance` maps geometrically onto Q, from
/// `Q_FLAT` (no peak) to `Q_RESONANT`.
#[derive(Clone, Copy)]
pub struct Svf {
    pub sample_rate: f32,
    pub cutoff: f32,
    pub resonance: f32,
    pub cutoff_smoother: Smoother,
    pub resonance_smoother: Smoother,

    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
    last_cutoff: f32,
    last_resonance: f32,
    low: f32,
    high: f32,
    band: f32,
    notch: f32,
    ic1eq: f32,
    ic2eq: f32,
    mode: FilterMode,
}

impl Svf {
    pub fn new(sample_rate: f32) -> Self {
        let cutoff = 300.0;
        let resonance = 0.0;

        Self {
            sample_rate,
            cutoff,
            resonance,
            cutoff_smoother: Smoother::new(cutoff, 0.0005),
            resonance_smoother: Smoother::new(resonance, 0.0005),
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            k: 0.0,
            // NaN never compares equal to itself, so the first call always
            // takes the branch that computes the coefficients.
            last_cutoff: f32::NAN,
            last_resonance: f32::NAN,
            low: 0.0,
            high: 0.0,
            band: 0.0,
            notch: 0.0,
            ic1eq: 0.0,
            ic2eq: 0.0,
            mode: FilterMode::LowPass,
        }
    }

    pub fn set_cutoff(&mut self, cutoff: f32) {
        self.cutoff = cutoff.clamp(20.0, self.sample_rate * 0.45);
        self.cutoff_smoother.set_target(self.cutoff);
    }

    pub fn set_resonance(&mut self, resonance: f32) {
        self.resonance = resonance.clamp(0.0, 1.0);
        self.resonance_smoother.set_target(self.resonance);
    }

    pub fn set_mode(&mut self, mode: FilterMode) {
        self.mode = mode;
    }

    /// Geometric interpolation from [`Q_FLAT`] to [`Q_RESONANT`].
    ///
    /// A linear ramp would spend most of its travel in the inaudible Q < 2
    /// region and leave almost nothing for the top of the range, where the
    /// filter actually sings.
    fn q_for_resonance(resonance: f32) -> f32 {
        Q_FLAT * powf(Q_RESONANT / Q_FLAT, resonance.clamp(0.0, 1.0))
    }

    /// TPT coefficients for a cutoff and Q.
    ///
    /// The cutoff is prewarped with `tan(w)`, so the digital corner lands on
    /// the requested frequency rather than drifting low the way the plain
    /// `tan` coefficient did.
    fn update(&mut self, cutoff: f32, resonance: f32) {
        // Once the smoothers reach their targets they stop moving, so most
        // samples repeat the previous coefficients. Rebuilding them would cost
        // a `tanf` and a `powf` per sample for no change in the response.
        if cutoff == self.last_cutoff && resonance == self.last_resonance {
            return;
        }
        self.last_cutoff = cutoff;
        self.last_resonance = resonance;

        let g = tanf(PI * cutoff / self.sample_rate);
        self.k = 1.0 / Self::q_for_resonance(resonance);

        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    pub fn reset(&mut self) {
        self.low = 0.0;
        self.high = 0.0;
        self.band = 0.0;
        self.notch = 0.0;
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let cutoff = self.cutoff_smoother.next_sample();
        let resonance = self.resonance_smoother.next_sample();
        self.update(cutoff, resonance);

        let v3 = input - self.ic2eq;
        let v1 = self.a1 * self.ic1eq + self.a2 * v3;
        let v2 = self.ic2eq + self.a2 * self.ic1eq + self.a3 * v3;

        // Delay-free feedback: each integrator's input is available before it
        // is used, so nothing aliases or rings the way a direct-form topology
        // does up near Nyquist.
        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;

        self.low = v2;
        self.band = v1;
        self.high = input - self.k * v1 - v2;
        // The notch is the sum of the low-pass and high-pass outputs. Writing
        // it as `input - k * band` instead looks equivalent but is not: that
        // form is missing the second integrator's contribution, and it leaves
        // a shallow dip at the centre rather than a real null.
        self.notch = self.low + self.high;

        match self.mode {
            FilterMode::LowPass => self.low,
            FilterMode::HighPass => self.high,
            FilterMode::BandPass => self.band,
            FilterMode::Notch => self.notch,
        }
    }
}

impl crate::patch::Module for Svf {
    fn process(&mut self, input: f32) -> f32 {
        self.process(input)
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::Module;
    use std::boxed::Box;

    const SAMPLE_RATE: f32 = 44100.0;

    /// Analysis window length, in samples.
    ///
    /// Every measurement projects onto a whole number of periods inside this
    /// window, which removes spectral leakage and makes the result exact.
    const N: usize = 16384;

    /// Long enough for the slowest pole used in these tests to settle.
    const SETTLE: usize = 200_000;

    const TAU: f32 = core::f32::consts::TAU;

    /// Measure the amplitude of one frequency once `process` has settled.
    ///
    /// The requested frequency is snapped so a whole number of periods fits in
    /// the analysis window. Phase is accumulated with explicit wrapping:
    /// evaluating `sin(w * n)` at large `n` loses f32 precision in the
    /// argument and silently corrupts the result.
    fn amplitude_at<F: FnMut(f32) -> f32>(process: &mut F, freq: f32) -> f32 {
        let cycles = (libm::roundf(freq * N as f32 / SAMPLE_RATE) as usize).max(4);
        let step = TAU * cycles as f32 / N as f32;

        let mut phase = 0.0f32;
        let mut buf = [0.0f32; N];

        for _ in 0..SETTLE {
            process(libm::sinf(phase));
            phase += step;
            if phase >= TAU {
                phase -= TAU;
            }
        }
        for v in buf.iter_mut() {
            *v = process(libm::sinf(phase));
            phase += step;
            if phase >= TAU {
                phase -= TAU;
            }
        }

        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, &x) in buf.iter().enumerate() {
            let a = step * i as f32;
            re += (x as f64) * (libm::cosf(a) as f64);
            im -= (x as f64) * (libm::sinf(a) as f64);
        }
        (2.0 * libm::sqrt(re * re + im * im) / N as f64) as f32
    }

    fn db(x: f32) -> f32 {
        20.0 * libm::log10f(x.abs().max(1e-12))
    }

    /// Binary-search the frequency 3 dB below unity.
    ///
    /// These filters sit at unity in their passband, so a bisection bounded by
    /// the search window converges on the real crossing.
    fn lowpass_corner(make: &dyn Fn() -> Box<dyn FnMut(f32) -> f32>) -> f32 {
        let (mut lo, mut hi) = (2.0f32, SAMPLE_RATE * 0.49);
        for _ in 0..38 {
            let mid = (lo + hi) * 0.5;
            let mut q = make();
            if db(amplitude_at(&mut q, mid)) > -3.0103 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (lo + hi) * 0.5
    }

    /// Sweep for the strongest band-pass response and where it sits.
    fn bandpass_peak(resonance: f32) -> (f32, f32) {
        let (mut best, mut best_hz) = (0.0f32, 0.0f32);
        let mut hz = 20.0f32;
        while hz < SAMPLE_RATE * 0.45 {
            let mut f = Svf::new(SAMPLE_RATE);
            f.set_cutoff(1000.0);
            f.set_resonance(resonance);
            f.set_mode(FilterMode::BandPass);
            let g = amplitude_at(&mut |s| f.process(s), hz);
            if g > best {
                best = g;
                best_hz = hz;
            }
            hz *= 1.03;
        }
        (best, best_hz)
    }

    /// Sweep for the deepest notch and where it sits.
    ///
    /// The search is confined to a window around the cutoff: at full resonance
    /// the null is only tens of hertz wide, far narrower than a grid coarse
    /// enough to cover the whole spectrum would have to be.
    fn notch_null(resonance: f32) -> (f32, f32) {
        let (mut worst, mut worst_hz) = (f32::MAX, 0.0f32);
        let mut hz = 700.0f32;
        while hz < 1300.0 {
            let mut f = Svf::new(SAMPLE_RATE);
            f.set_cutoff(1000.0);
            f.set_resonance(resonance);
            f.set_mode(FilterMode::Notch);
            let g = amplitude_at(&mut |s| f.process(s), hz);
            if g < worst {
                worst = g;
                worst_hz = hz;
            }
            hz *= 1.004;
        }
        (worst, worst_hz)
    }

    fn settled_dc<F: FnMut(f32) -> f32>(mut process: F) -> f32 {
        let mut out = 0.0;
        for _ in 0..400_000 {
            out = process(1.0);
        }
        out
    }

    /// A deterministic noise source, so a failure can be reproduced exactly.
    fn noise_source() -> impl FnMut() -> f32 {
        let mut seed = 12345u32;
        move || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / 8_388_608.0 - 1.0
        }
    }

    /// A unity-gain path, proving the harness itself is sound before trusting
    /// anything it reports about the filter.
    #[test]
    fn harness_reports_unity_gain_as_unity() {
        for freq in [50.0f32, 440.0, 1000.0, 5000.0, 10000.0] {
            let g = amplitude_at(&mut |s| s, freq);
            assert!(
                (g - 1.0).abs() < 1e-4,
                "harness is broken: {freq} Hz reported {g}, expected 1.0"
            );
        }
    }

    #[test]
    fn new_has_expected_defaults() {
        let filter = Svf::new(SAMPLE_RATE);
        assert_eq!(filter.sample_rate, SAMPLE_RATE);
        assert_eq!(filter.cutoff, 300.0);
        assert_eq!(filter.resonance, 0.0);
        assert!(matches!(filter.mode, FilterMode::LowPass));
    }

    /// The smoothers have to start on the values their fields report, or the
    /// first `process` call would glide away from the documented defaults.
    #[test]
    fn smoothers_start_on_the_parameter_values() {
        let filter = Svf::new(SAMPLE_RATE);
        assert_eq!(filter.cutoff_smoother.current, filter.cutoff);
        assert_eq!(filter.cutoff_smoother.target, filter.cutoff);
        assert_eq!(filter.resonance_smoother.current, filter.resonance);
        assert_eq!(filter.resonance_smoother.target, filter.resonance);
    }

    /// The default resonance has to be a value the setter accepts, otherwise
    /// `new()` and `set_resonance` disagree about the valid domain.
    #[test]
    fn default_resonance_is_inside_the_setters_range() {
        let mut filter = Svf::new(SAMPLE_RATE);
        filter.set_resonance(filter.resonance);
        assert_eq!(filter.resonance, 0.0);
    }

    #[test]
    fn lowpass_passes_dc_unchanged() {
        for cutoff in [100.0f32, 1000.0, 8000.0] {
            let mut filter = Svf::new(SAMPLE_RATE);
            filter.set_cutoff(cutoff);
            let out = settled_dc(|s| filter.process(s));
            assert!(
                (out - 1.0).abs() < 1e-3,
                "DC gain at cutoff {cutoff} was {out}, expected 1.0"
            );
        }
    }

    /// The DC-gain invariant across every mode.
    ///
    /// A notch rejects its centre frequency, not DC: it is the sum of the
    /// low-pass and high-pass outputs, so DC must pass through at unity.
    #[test]
    fn dc_behaves_correctly_in_every_mode() {
        for mode in [
            FilterMode::LowPass,
            FilterMode::HighPass,
            FilterMode::BandPass,
            FilterMode::Notch,
        ] {
            let mut filter = Svf::new(SAMPLE_RATE);
            filter.set_cutoff(1000.0);
            filter.set_mode(mode);
            let out = settled_dc(|s| filter.process(s));

            let passes_dc = matches!(mode, FilterMode::LowPass | FilterMode::Notch);
            if passes_dc {
                assert!((out - 1.0).abs() < 1e-3, "{mode:?} DC gain {out}");
            } else {
                assert!(out.abs() < 0.05, "{mode:?} passed DC at {out}");
            }
        }
    }

    /// At zero resonance the response is Butterworth: the band-pass peak sits at
    /// `Q_FLAT` and the low-pass never rises above unity anywhere.
    #[test]
    fn zero_resonance_has_no_peak() {
        let (bandpass, _) = bandpass_peak(0.0);
        assert!(
            (bandpass - Q_FLAT).abs() < 0.05,
            "flat response should peak at Q_FLAT {Q_FLAT}, got {bandpass}"
        );

        let mut worst = 0.0f32;
        let mut hz = 20.0f32;
        while hz < SAMPLE_RATE * 0.45 {
            let mut f = Svf::new(SAMPLE_RATE);
            f.set_cutoff(1000.0);
            f.set_resonance(0.0);
            f.set_mode(FilterMode::LowPass);
            let g = amplitude_at(&mut |s| f.process(s), hz);
            if g > worst {
                worst = g;
            }
            hz *= 1.03;
        }
        assert!(
            worst < 1.001,
            "a flat low-pass rose to {worst}; there should be no resonant peak"
        );
    }

    #[test]
    fn resonance_increases_the_bandpass_peak() {
        let mut previous = 0.0;
        for res in [0.05f32, 0.2, 0.4, 0.6, 0.8, 1.0] {
            let (best, best_hz) = bandpass_peak(res);
            assert!(
                best > previous,
                "resonance {res} gave peak {best} at {best_hz:.0} Hz, \
                 not above the previous {previous}"
            );
            previous = best;
        }
    }

    /// A band-pass's peak height is its Q, so the mapping is measured directly
    /// rather than assumed from the coefficients.
    #[test]
    fn full_resonance_reaches_the_advertised_q() {
        let (peak, _) = bandpass_peak(1.0);
        assert!(
            (peak - Q_RESONANT).abs() / Q_RESONANT < 0.1,
            "full-resonance band-pass peak was {peak}, expected Q ~ {Q_RESONANT}"
        );
    }

    /// The peak has to sit on the cutoff, at every resonance.
    #[test]
    fn the_resonant_peak_tracks_the_cutoff() {
        for res in [0.3f32, 0.6, 1.0] {
            let (_, peak_hz) = bandpass_peak(res);
            assert!(
                (900.0..1100.0).contains(&peak_hz),
                "resonance {res} put the peak at {peak_hz:.0} Hz, expected near the 1000 Hz cutoff"
            );
        }
    }

    #[test]
    fn resonance_and_cutoff_are_clamped() {
        let mut filter = Svf::new(SAMPLE_RATE);

        filter.set_resonance(5.0);
        assert_eq!(filter.resonance, 1.0);
        filter.set_resonance(-3.0);
        assert_eq!(filter.resonance, 0.0);

        filter.set_cutoff(100_000.0);
        assert_eq!(filter.cutoff, SAMPLE_RATE * 0.45);
        filter.set_cutoff(5.0);
        assert_eq!(filter.cutoff, 20.0);
    }

    /// `set_cutoff` advertises the whole range up to 0.45 * sample_rate, and
    /// every part of it has to be usable. The Chamberlin topology this replaced
    /// produced NaN from about 14 kHz upward.
    #[test]
    fn stays_finite_across_the_whole_cutoff_range() {
        for cutoff in [
            20.0f32,
            100.0,
            1000.0,
            5000.0,
            10_000.0,
            14_100.0,
            19_000.0,
            SAMPLE_RATE * 0.45,
        ] {
            for resonance in [0.0f32, 0.5, 1.0] {
                let mut filter = Svf::new(SAMPLE_RATE);
                filter.set_cutoff(cutoff);
                filter.set_resonance(resonance);
                for mode in [
                    FilterMode::LowPass,
                    FilterMode::HighPass,
                    FilterMode::BandPass,
                    FilterMode::Notch,
                ] {
                    filter.set_mode(mode);
                    let mut noise = noise_source();
                    for _ in 0..100_000 {
                        let out = filter.process(noise());
                        assert!(
                            out.is_finite() && out.abs() < 10.0,
                            "cutoff {cutoff} resonance {resonance} {mode:?} produced {out}"
                        );
                    }
                }
            }
        }
    }

    /// The measured -3 dB point has to sit on the requested cutoff.
    #[test]
    fn measured_corner_matches_requested_cutoff() {
        for cutoff in [100.0f32, 500.0, 1000.0, 4000.0, 8000.0] {
            let corner = lowpass_corner(&|| {
                let mut f = Svf::new(SAMPLE_RATE);
                f.set_cutoff(cutoff);
                f.set_resonance(0.0);
                Box::new(move |s| f.process(s))
            });
            let ratio = corner / cutoff;
            assert!(
                (0.95..=1.05).contains(&ratio),
                "cutoff {cutoff} measured a corner at {corner} Hz (ratio {ratio:.3})"
            );
        }
    }

    /// A notch has to reject its centre frequency, and that null belongs on the
    /// cutoff.
    #[test]
    fn notch_rejects_its_centre_frequency() {
        for resonance in [0.3f32, 0.6, 1.0] {
            let (depth, null_hz) = notch_null(resonance);
            assert!(
                depth < 0.2,
                "resonance {resonance} left a notch only {depth} deep"
            );
            assert!(
                (900.0..1100.0).contains(&null_hz),
                "resonance {resonance} put the null at {null_hz:.0} Hz, expected near the 1000 Hz cutoff"
            );
        }
    }

    /// The notch null and the band-pass peak must land on the same frequency,
    /// since both are governed by the same centre frequency.
    #[test]
    fn notch_null_and_bandpass_peak_agree() {
        for res in [0.3f32, 0.6, 1.0] {
            let (_, notch_hz) = notch_null(res);
            let (_, peak_hz) = bandpass_peak(res);
            assert!(
                (notch_hz - peak_hz).abs() / notch_hz < 0.05,
                "at resonance {res} the notch null was at {notch_hz:.0} Hz but \
                 the band-pass peak at {peak_hz:.0} Hz; they should coincide"
            );
        }
    }

    #[test]
    fn reset_clears_all_state() {
        let mut filter = Svf::new(SAMPLE_RATE);
        filter.process(1.0);
        assert!(filter.low != 0.0 || filter.band != 0.0);

        filter.reset();
        assert_eq!(filter.low, 0.0);
        assert_eq!(filter.high, 0.0);
        assert_eq!(filter.band, 0.0);
        assert_eq!(filter.notch, 0.0);
        assert_eq!(filter.ic1eq, 0.0);
        assert_eq!(filter.ic2eq, 0.0);
    }

    #[test]
    fn module_trait_delegates_to_process() {
        let mut inherent = Svf::new(SAMPLE_RATE);
        let mut via_trait = Svf::new(SAMPLE_RATE);
        for i in 0..1000 {
            let x = libm::sinf(i as f32 * 0.01);
            // `Svf::process` has an inherent method of the same name, so the
            // trait version has to be named explicitly to be reachable.
            assert_eq!(inherent.process(x), Module::process(&mut via_trait, x));
        }
    }

    /// The smoothers exist to stop parameter jumps from clicking, so `process`
    /// has to advance them.
    #[test]
    fn smoothers_advance_during_processing() {
        let mut filter = Svf::new(SAMPLE_RATE);
        filter.set_cutoff(4000.0);
        filter.set_resonance(0.8);

        let before_cutoff = filter.cutoff_smoother.current;
        let before_resonance = filter.resonance_smoother.current;
        for i in 0..100_000 {
            filter.process(libm::sinf(i as f32 * 0.01));
        }

        assert!(
            filter.cutoff_smoother.current > before_cutoff,
            "cutoff smoother did not advance"
        );
        assert!(
            filter.resonance_smoother.current > before_resonance,
            "resonance smoother did not advance"
        );
    }

    /// A jump in the target must not jump the value in use.
    #[test]
    fn parameter_changes_glide() {
        let mut filter = Svf::new(SAMPLE_RATE);
        filter.set_cutoff(8000.0);
        filter.set_resonance(0.0);
        for _ in 0..200_000 {
            filter.process(0.0);
        }

        filter.set_cutoff(100.0);
        filter.set_resonance(1.0);
        assert_eq!(filter.cutoff_smoother.target, 100.0);
        assert_eq!(filter.resonance_smoother.target, 1.0);
        assert!(
            filter.cutoff_smoother.current > 7000.0,
            "cutoff in use jumped to {}",
            filter.cutoff_smoother.current
        );
        assert!(
            filter.resonance_smoother.current < 0.05,
            "resonance in use jumped to {}",
            filter.resonance_smoother.current
        );

        for _ in 0..200_000 {
            filter.process(0.0);
        }
        assert!(
            (filter.cutoff_smoother.current - 100.0).abs() < 1.0,
            "cutoff should reach 100.0, got {}",
            filter.cutoff_smoother.current
        );
        assert!(
            (filter.resonance_smoother.current - 1.0).abs() < 0.01,
            "resonance should reach 1.0, got {}",
            filter.resonance_smoother.current
        );
    }
}
