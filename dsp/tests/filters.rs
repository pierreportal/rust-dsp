use dsp::acid_filter::AcidFilter;
use dsp::filter::Filter;
use dsp::patch::Module;

/// 44.1 kHz, the rate the rest of the crate's tests assume.
const SAMPLE_RATE: f32 = 44100.0;

/// Analysis window length, in samples.
const N: usize = 16384;

/// Long enough for the slowest pole used here to settle.
const SETTLE: usize = 200_000;

const TAU: f32 = core::f32::consts::TAU;

/// Measure the amplitude of one frequency once `process` has settled.
///
/// The requested frequency is snapped so a whole number of periods fits in
/// the analysis window, which removes spectral leakage. Phase is accumulated
/// with explicit wrapping: evaluating `sin(w * n)` at large `n` loses f32
/// precision in the argument and silently corrupts the result.
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

/// The frequency 3 dB below the passband.
///
/// A geometric sweep brackets the crossing first, then bisection refines it.
/// The bracket matters because these filters do not all reach unity gain in
/// their passband, so a bisection that assumes a -3 dB drop at the search
/// bounds converges on the wrong number.
fn corner(make: &dyn Fn() -> Box<dyn FnMut(f32) -> f32>) -> f32 {
    let mut probes: Vec<(f32, f32)> = Vec::new();
    let mut hz = 5.0f32;
    while hz < SAMPLE_RATE * 0.49 {
        let mut q = make();
        let gain = db(amplitude_at(&mut q, hz));
        probes.push((hz, gain));
        hz *= 1.08;
    }

    // Narrow the bracket from the first adjacent pair that straddles -3 dB.
    let mut below: Option<(f32, f32)> = None;
    for pair in probes.windows(2) {
        let (f0, d0) = pair[0];
        let (f1, d1) = pair[1];
        if d0 > -3.0103 && d1 <= -3.0103 {
            let (mut lo, mut hi) = (f0, f1);
            for _ in 0..38 {
                let mid = (lo + hi) * 0.5;
                let mut q = make();
                if db(amplitude_at(&mut q, mid)) > -3.0103 {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            below = Some((lo, hi));
            break;
        }
    }

    below.map(|(lo, hi)| (lo + hi) * 0.5).unwrap_or_else(|| {
        panic!(
            "no -3 dB crossing in the sweep; passband start was {:.2} dB",
            probes[0].1
        )
    })
}

/// Hold the input at 1.0 until the output stops moving.
fn settled_dc<F: FnMut(f32) -> f32>(mut process: F) -> f32 {
    let mut out = 0.0;
    for _ in 0..500_000 {
        out = process(1.0);
    }
    out
}

/// Proves the measurement harness is sound before trusting its readings.
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

mod filter_rs {
    use super::*;

    /// `filter.rs` uses `coefficient = 1 - exp(-2*pi*fc/fs)`, the standard
    /// one-pole mapping. The measured -3 dB point tracks the requested cutoff
    /// closely across the musical range.
    #[test]
    fn cutoff_matches_the_measured_corner() {
        for cutoff in [100.0f32, 500.0, 1000.0, 4000.0] {
            let corner = corner(&|| {
                let mut f = Filter::new(SAMPLE_RATE);
                f.set_cutoff(cutoff);
                Box::new(move |s| f.process_sample(s))
            });
            let ratio = corner / cutoff;
            assert!(
                (0.98..=1.05).contains(&ratio),
                "cutoff {cutoff} measured a corner at {corner:.1} Hz (ratio {ratio:.3})"
            );
        }
    }

    /// The naive exponential approximation is only accurate while the pole
    /// stays well away from z = 1, so it drifts at very high cutoffs.
    #[test]
    fn corner_drifts_high_at_extreme_cutoffs() {
        for cutoff in [8000.0f32, 12000.0] {
            let corner = corner(&|| {
                let mut f = Filter::new(SAMPLE_RATE);
                f.set_cutoff(cutoff);
                Box::new(move |s| f.process_sample(s))
            });
            assert!(
                corner > cutoff,
                "cutoff {cutoff} measured a corner at {corner:.1} Hz; \
                 expected the approximation to read high"
            );
        }
    }

    #[test]
    fn passes_dc_at_unity() {
        for cutoff in [100.0f32, 1000.0, 10000.0] {
            let mut f = Filter::new(SAMPLE_RATE);
            f.set_cutoff(cutoff);
            let out = settled_dc(|s| f.process_sample(s));
            assert!(
                (out - 1.0).abs() < 1e-3,
                "DC gain at cutoff {cutoff} was {out}, expected 1.0"
            );
        }
    }

    #[test]
    fn clamps_cutoff_to_the_documented_range() {
        let mut f = Filter::new(SAMPLE_RATE);

        f.set_cutoff(100_000.0);
        assert_eq!(f.cutoff, SAMPLE_RATE * 0.45);

        f.set_cutoff(0.0);
        assert_eq!(f.cutoff, 1.0);

        f.set_cutoff(-50.0);
        assert_eq!(f.cutoff, 1.0);
    }

    #[test]
    fn stays_finite_at_the_maximum_clamped_cutoff() {
        let mut f = Filter::new(SAMPLE_RATE);
        f.set_cutoff(100_000.0);
        for i in 0..100_000 {
            let out = f.process_sample(libm::sinf(i as f32 * 0.05));
            assert!(out.is_finite(), "went non-finite at sample {i}");
        }
    }

    #[test]
    fn attenuates_content_above_the_cutoff() {
        let mut low = Filter::new(SAMPLE_RATE);
        low.set_cutoff(1000.0);
        let below = amplitude_at(&mut |s| low.process_sample(s), 100.0);

        let mut high = Filter::new(SAMPLE_RATE);
        high.set_cutoff(1000.0);
        let above = amplitude_at(&mut |s| high.process_sample(s), 10000.0);

        assert!((below - 1.0).abs() < 0.02, "passband gain was {below}");
        assert!(above < 0.2, "10 kHz passed through at {above}");
    }

    /// The smoother exists to stop a cutoff sweep from clicking, so it has to
    /// be advanced by processing and reach the target it was given.
    #[test]
    fn cutoff_changes_glide() {
        let mut f = Filter::new(SAMPLE_RATE);
        f.set_cutoff(8000.0);
        for _ in 0..50_000 {
            f.process_sample(0.0);
        }
        assert!((f.cutoff_smoother.current - 8000.0).abs() < 1.0);

        f.set_cutoff(100.0);
        assert_eq!(f.cutoff_smoother.target, 100.0);
        assert!(
            f.cutoff_smoother.current > 7000.0,
            "the cutoff in use jumped to {}",
            f.cutoff_smoother.current
        );

        for _ in 0..50_000 {
            f.process_sample(0.0);
        }
        assert!(
            (f.cutoff_smoother.current - 100.0).abs() < 1.0,
            "the cutoff should reach its target, got {}",
            f.cutoff_smoother.current
        );
    }

    #[test]
    fn module_trait_delegates_to_process_sample() {
        let mut a = Filter::new(SAMPLE_RATE);
        let mut b = Filter::new(SAMPLE_RATE);
        for i in 0..1000 {
            let x = libm::sinf(i as f32 * 0.01);
            assert_eq!(a.process_sample(x), Module::process(&mut b, x));
        }
    }
}

mod acid_filter_rs {
    use super::*;

    /// One running `AcidFilter` at a fixed cutoff, for frequency-response work.
    ///
    /// Building the filter inside the returned closure would reset its state on
    /// every single sample, leaving the measurement stuck at the transient
    /// response instead of the steady-state one.
    fn cascade(cutoff: f32) -> Box<dyn FnMut(f32) -> f32> {
        let mut f = AcidFilter::new(SAMPLE_RATE);
        f.set_cutoff(cutoff);
        f.set_resonance(0.0);
        Box::new(move |s| f.process_sample(s))
    }

    /// The four sections sit above the requested cutoff so the *cascade's*
    /// -3 dB point lands on the parameter rather than on each section's own
    /// corner.
    #[test]
    fn measured_corner_matches_requested_cutoff() {
        for cutoff in [100.0f32, 200.0, 500.0, 1000.0, 4000.0, 8000.0] {
            let corner = corner(&|| cascade(cutoff));
            let ratio = corner / cutoff;
            assert!(
                (0.90..=1.10).contains(&ratio),
                "cutoff {cutoff} measured a corner at {corner:.1} Hz (ratio {ratio:.3})"
            );
        }
    }

    #[test]
    fn passes_dc_at_unity_when_resonance_is_zero() {
        for cutoff in [100.0f32, 1000.0, 8000.0] {
            let mut f = AcidFilter::new(SAMPLE_RATE);
            f.set_cutoff(cutoff);
            f.set_resonance(0.0);
            let out = settled_dc(|s| f.process_sample(s));
            assert!(
                (out - 1.0).abs() < 1e-3,
                "DC gain at cutoff {cutoff} was {out}, expected 1.0"
            );
        }
    }

    /// The decisive one. `process_sample` used to compute
    /// `let x = input - self.feedback;` with `feedback = resonance * 4.0` as a
    /// constant, so the parameter subtracted a fixed offset and silence
    /// produced sound. The feedback term has to be a multiple of the output
    /// signal, which means silence has to stay silent.
    #[test]
    fn silence_stays_silent_at_every_resonance() {
        for res in [0.25f32, 0.5, 0.75, 0.9, 0.95] {
            let mut f = AcidFilter::new(SAMPLE_RATE);
            f.set_cutoff(1000.0);
            f.set_resonance(res);

            let mut peak = 0.0f32;
            for _ in 0..50_000 {
                peak = peak.max(f.process_sample(0.0).abs());
            }

            assert!(
                peak == 0.0,
                "at resonance {res} the filter produced {peak} from pure silence"
            );
        }
    }

    /// The `1 + feedback` input scaling exists to keep DC at unity as the
    /// feedback loop deepens. Without it the loop would settle at
    /// `1/(1 + feedback)`, which is a 13.6 dB dip at maximum resonance.
    #[test]
    fn passes_dc_at_unity_at_every_resonance() {
        for res in [0.0f32, 0.25, 0.5, 0.75, 0.9, 0.95] {
            let mut f = AcidFilter::new(SAMPLE_RATE);
            f.set_cutoff(1000.0);
            f.set_resonance(res);
            let out = settled_dc(|s| f.process_sample(s));
            assert!(
                (out - 1.0).abs() < 0.01,
                "DC gain at resonance {res} was {out}, expected 1.0"
            );
        }
    }

    /// A working ladder resonance reinforces the signal in a band around the
    /// cutoff. The old constant-offset version produced a "peak" that was flat
    /// across the whole spectrum, so this checks that the boost is both real
    /// and band-limited.
    #[test]
    fn resonance_boosts_a_narrow_band_around_the_cutoff() {
        let gain = |freq: f32, res: f32| -> f32 {
            let mut f = AcidFilter::new(SAMPLE_RATE);
            f.set_cutoff(1000.0);
            f.set_resonance(res);
            amplitude_at(&mut |s| f.process_sample(s), freq)
        };

        let flat = gain(1000.0, 0.0);
        let resonant = gain(1000.0, 0.9);
        assert!(
            resonant > flat * 1.5,
            "resonance should lift the response at the cutoff: \
             flat {flat}, resonant {resonant}"
        );

        // Up at the very top of the band the response has to collapse back to
        // nothing, otherwise this is the old constant offset in disguise.
        for freq in [10_000.0f32, 18_000.0] {
            let above = gain(freq, 0.95);
            assert!(
                above < 0.05,
                "{freq} Hz passed at {above}, expected a 4-pole rolloff to silence"
            );
        }
    }

    #[test]
    fn resonance_is_clamped_to_the_documented_range() {
        let mut f = AcidFilter::new(SAMPLE_RATE);
        f.set_resonance(5.0);
        assert_eq!(f.resonance, 0.95);
        f.set_resonance(-3.0);
        assert_eq!(f.resonance, 0.0);
    }

    #[test]
    fn clamps_cutoff_to_the_documented_range() {
        let mut f = AcidFilter::new(SAMPLE_RATE);
        f.set_cutoff(100_000.0);
        assert_eq!(f.cutoff, SAMPLE_RATE * 0.45);
        f.set_cutoff(1.0);
        assert_eq!(f.cutoff, 20.0);
    }

    #[test]
    fn stays_finite_at_the_maximum_clamped_cutoff() {
        let mut f = AcidFilter::new(SAMPLE_RATE);
        f.set_cutoff(100_000.0);
        f.set_resonance(0.95);
        for i in 0..200_000 {
            let out = f.process_sample(libm::sinf(i as f32 * 0.05));
            assert!(out.is_finite(), "went non-finite at sample {i}");
        }
    }

    #[test]
    fn reset_clears_the_cascade_state() {
        let mut f = AcidFilter::new(SAMPLE_RATE);
        f.set_cutoff(1000.0);
        f.set_resonance(0.0);
        for _ in 0..1000 {
            f.process_sample(1.0);
        }
        f.reset();
        assert_eq!(f.process_sample(0.0), 0.0);
    }

    /// Both smoothers have to be advanced by processing and driven by their
    /// setters.
    #[test]
    fn parameter_changes_glide() {
        let mut f = AcidFilter::new(SAMPLE_RATE);
        f.set_cutoff(8000.0);
        f.set_resonance(0.0);
        for _ in 0..200_000 {
            f.process_sample(0.0);
        }
        assert!((f.cutoff_smoother.current - 8000.0).abs() < 1.0);
        assert!((f.resonance_smoother.current - 0.0).abs() < 0.01);

        f.set_cutoff(100.0);
        f.set_resonance(0.9);
        assert_eq!(f.cutoff_smoother.target, 100.0);
        assert_eq!(f.resonance_smoother.target, 0.9);
        assert!(
            f.cutoff_smoother.current > 7000.0,
            "the cutoff in use jumped to {}",
            f.cutoff_smoother.current
        );
        assert!(
            f.resonance_smoother.current < 0.05,
            "resonance jumped to {}",
            f.resonance_smoother.current
        );

        for _ in 0..200_000 {
            f.process_sample(0.0);
        }
        assert!(
            (f.cutoff_smoother.current - 100.0).abs() < 1.0,
            "cutoff should reach 100.0, got {}",
            f.cutoff_smoother.current
        );
        assert!(
            (f.resonance_smoother.current - 0.9).abs() < 0.01,
            "resonance should reach 0.9, got {}",
            f.resonance_smoother.current
        );
    }
}
