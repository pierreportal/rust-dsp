use dsp::osc::{Osc, Waveform};
use rustfft::{num_complex::Complex, FftPlanner};

const SAMPLE_RATE: f32 = 44_100.0;
const FFT_SIZE: usize = 65_536;

/// Frequency chosen so that it corresponds exactly to an FFT bin.
///
/// This dramatically reduces spectral leakage and makes the comparison
/// between naive and PolyBLEP oscillators much more meaningful.
fn coherent_frequency(bin: usize) -> f32 {
    bin as f32 * SAMPLE_RATE / FFT_SIZE as f32
}

fn render_oscillator(waveform: Waveform, frequency: f32, pulse_width: f32) -> Vec<f32> {
    let mut osc = Osc::new(waveform, frequency, SAMPLE_RATE);

    osc.pulse_width = pulse_width;

    (0..FFT_SIZE).map(|_| osc.next_sample()).collect()
}

/// Naive reference implementation.
///
/// This deliberately does NOT use PolyBLEP.
///
/// It lets us compare:
///
///     naive oscillator
///              vs
///     current PolyBLEP oscillator
///
/// using exactly the same phase progression.
fn render_naive(waveform: Waveform, frequency: f32, pulse_width: f32) -> Vec<f32> {
    let dt = frequency / SAMPLE_RATE;

    let mut phase = 0.0;

    (0..FFT_SIZE)
        .map(|_| {
            phase += dt;

            if phase >= 1.0 {
                phase -= 1.0;
            }

            match waveform {
                Waveform::Sine => libm::sinf(phase * core::f32::consts::TAU),

                Waveform::Saw => 2.0 * phase - 1.0,

                Waveform::Triangle => 2.0 * ((2.0 * phase - 1.0).abs() - 0.5),

                Waveform::Square => {
                    if phase < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }

                Waveform::PulseWidth => {
                    if phase < pulse_width {
                        1.0
                    } else {
                        -1.0
                    }
                }
            }
        })
        .collect()
}

fn fft_magnitude(samples: &[f32]) -> Vec<f32> {
    let mut planner = FftPlanner::<f32>::new();

    let fft = planner.plan_fft_forward(samples.len());

    let mut buffer: Vec<Complex<f32>> = samples
        .iter()
        .map(|&sample| Complex::new(sample, 0.0))
        .collect();

    fft.process(&mut buffer);

    let scale = 2.0 / samples.len() as f32;

    buffer
        .iter()
        .take(samples.len() / 2 + 1)
        .map(|value| value.norm() * scale)
        .collect()
}

/// Return the FFT bin corresponding to a harmonic.
fn harmonic_bin(fundamental_bin: usize, harmonic: usize) -> usize {
    fundamental_bin * harmonic
}

/// Find the strongest spectral component which is NOT an
/// expected harmonic below Nyquist.
///
/// For an ideal periodic oscillator, energy should occur at
/// harmonic frequencies.
///
/// Anything else is a strong candidate for aliasing / unwanted
/// spectral energy.
fn strongest_non_harmonic_bin(magnitude: &[f32], fundamental_bin: usize) -> (usize, f32) {
    let nyquist_bin = magnitude.len() - 1;

    let mut masked = vec![true; magnitude.len()];

    // DC is not interesting for this test.
    masked[0] = false;

    // Remove all legitimate harmonics below Nyquist.
    let mut harmonic = 1;

    loop {
        let bin = harmonic_bin(fundamental_bin, harmonic);

        if bin > nyquist_bin {
            break;
        }

        // Ignore a couple of bins around each harmonic.
        //
        // This gives the FFT a little numerical tolerance.
        let start = bin.saturating_sub(2);
        let end = (bin + 2).min(nyquist_bin);

        for index in start..=end {
            masked[index] = false;
        }

        harmonic += 1;
    }

    let mut best_bin = 0;
    let mut best_magnitude = 0.0;

    for index in 1..=nyquist_bin {
        if masked[index] && magnitude[index] > best_magnitude {
            best_bin = index;
            best_magnitude = magnitude[index];
        }
    }

    (best_bin, best_magnitude)
}

fn db(amplitude: f32) -> f32 {
    20.0 * amplitude.max(1e-15).log10()
}

/// Measure the strongest non-harmonic spectral component.
fn alias_spur_db(samples: &[f32], fundamental_bin: usize) -> f32 {
    let magnitude = fft_magnitude(samples);

    let (_, amplitude) = strongest_non_harmonic_bin(&magnitude, fundamental_bin);

    db(amplitude)
}

#[test]
fn polyblep_saw_reduces_alias_spurs() {
    let fundamental_bin = 7424;
    let frequency = coherent_frequency(fundamental_bin);

    let naive = render_naive(Waveform::Saw, frequency, 0.5);

    let polyblep = render_oscillator(Waveform::Saw, frequency, 0.5);

    let naive_spur = alias_spur_db(&naive, fundamental_bin);

    let polyblep_spur = alias_spur_db(&polyblep, fundamental_bin);

    let improvement = naive_spur - polyblep_spur;

    println!(
        "\nSaw @ {:.2} Hz\n\
         naive:    {:.2} dB\n\
         PolyBLEP: {:.2} dB\n\
         improve:  {:.2} dB",
        frequency, naive_spur, polyblep_spur, improvement,
    );

    assert!(
        improvement > 8.0,
        "PolyBLEP only improved alias rejection by {:.2} dB",
        improvement
    );
}

#[test]
fn polyblep_square_reduces_alias_spurs() {
    let fundamental_bin = 10496;
    let frequency = coherent_frequency(fundamental_bin);

    let naive = render_naive(Waveform::Square, frequency, 0.5);

    let polyblep = render_oscillator(Waveform::Square, frequency, 0.5);

    let naive_spur = alias_spur_db(&naive, fundamental_bin);

    let polyblep_spur = alias_spur_db(&polyblep, fundamental_bin);

    let improvement = naive_spur - polyblep_spur;

    println!(
        "\nSquare @ {:.2} Hz\n\
         naive:    {:.2} dB\n\
         PolyBLEP: {:.2} dB\n\
         improve:  {:.2} dB",
        frequency, naive_spur, polyblep_spur, improvement,
    );

    assert!(
        improvement > 8.0,
        "PolyBLEP only improved alias rejection by {:.2} dB",
        improvement
    );
}

#[test]
fn polyblep_pwm_reduces_alias_spurs() {
    let fundamental_bin = 10496;
    let frequency = coherent_frequency(fundamental_bin);

    let naive = render_naive(Waveform::PulseWidth, frequency, 0.30);

    let polyblep = render_oscillator(Waveform::PulseWidth, frequency, 0.30);

    let naive_spur = alias_spur_db(&naive, fundamental_bin);

    let polyblep_spur = alias_spur_db(&polyblep, fundamental_bin);

    let improvement = naive_spur - polyblep_spur;

    println!(
        "\nPWM @ {:.2} Hz, width=0.30\n\
         naive:    {:.2} dB\n\
         PolyBLEP: {:.2} dB\n\
         improve:  {:.2} dB",
        frequency, naive_spur, polyblep_spur, improvement,
    );

    assert!(
        improvement > 8.0,
        "PolyBLEP only improved alias rejection by {:.2} dB",
        improvement
    );
}

#[test]
fn sine_does_not_need_blep() {
    let fundamental_bin = 7424;
    let frequency = coherent_frequency(fundamental_bin);

    let naive = render_naive(Waveform::Sine, frequency, 0.5);

    let oscillator = render_oscillator(Waveform::Sine, frequency, 0.5);

    let naive_magnitude = fft_magnitude(&naive);
    let oscillator_magnitude = fft_magnitude(&oscillator);

    let fundamental = fundamental_bin;

    let difference = (naive_magnitude[fundamental] - oscillator_magnitude[fundamental]).abs();

    assert!(
        difference < 1e-3,
        "Sine fundamental changed unexpectedly: {:.6}",
        difference
    );
}

#[test]
fn oscillator_does_not_produce_nan_or_inf() {
    let waveforms = [
        Waveform::Sine,
        Waveform::Saw,
        Waveform::Triangle,
        Waveform::Square,
        Waveform::PulseWidth,
    ];

    let frequencies = [
        20.0, 100.0, 440.0, 1000.0, 5000.0, 10000.0, 18000.0, 22050.0, 30000.0,
    ];

    for waveform in waveforms {
        for frequency in frequencies {
            let mut osc = Osc::new(waveform, frequency, SAMPLE_RATE);

            osc.pulse_width = 0.3;

            for _ in 0..FFT_SIZE {
                let sample = osc.next_sample();

                assert!(
                    sample.is_finite(),
                    "Non-finite output for {:?} @ {} Hz",
                    waveform,
                    frequency
                );
            }
        }
    }
}
