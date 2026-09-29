use dsp::osc::{Osc, Waveform};
use std::fs::File;
use std::io::{BufWriter, Write};

const SAMPLE_RATE: f32 = 44_100.0;
const FFT_SIZE: usize = 65_536;

fn coherent_frequency(bin: usize) -> f32 {
    bin as f32 * SAMPLE_RATE / FFT_SIZE as f32
}

fn render(waveform: Waveform, frequency: f32, pulse_width: f32) -> Vec<f32> {
    let mut osc = Osc::new(waveform, frequency, SAMPLE_RATE);

    osc.pulse_width = pulse_width;

    (0..FFT_SIZE).map(|_| osc.next_sample()).collect()
}

fn naive_render(waveform: Waveform, frequency: f32, pulse_width: f32) -> Vec<f32> {
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

fn write_csv(path: &str, naive: &[f32], polyblep: &[f32]) -> std::io::Result<()> {
    let file = File::create(path)?;

    let mut writer = BufWriter::new(file);

    writeln!(writer, "sample,naive,polyblep")?;

    for i in 0..naive.len() {
        writeln!(writer, "{},{},{}", i, naive[i], polyblep[i])?;
    }

    Ok(())
}

fn main() -> std::io::Result<()> {
    let frequency = coherent_frequency(7424);

    println!("Oscillator analysis");

    println!("sample rate: {} Hz", SAMPLE_RATE);

    println!("frequency:   {:.2} Hz", frequency);

    for (name, waveform) in [
        ("saw", Waveform::Saw),
        ("square", Waveform::Square),
        ("pwm", Waveform::PulseWidth),
    ] {
        let naive = naive_render(waveform, frequency, 0.30);

        let polyblep = render(waveform, frequency, 0.30);

        let path = format!("oscillator_{name}.csv");

        write_csv(&path, &naive, &polyblep)?;

        println!("generated {}", path);
    }

    Ok(())
}
