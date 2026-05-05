use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SupportedStreamConfig};
use dsp::voice::Voice;
use std::io::{self, Read};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

struct Params {
    freq: AtomicU32,
}
fn f32_to_atomic(f: f32) -> u32 {
    f.to_bits()
}
fn atomic_to_f32(u: u32) -> f32 {
    f32::from_bits(u)
}

fn stream_audio(
    device: Device,
    mut voice: Voice,
    params: Arc<Params>,
    config: SupportedStreamConfig,
) {
    let params_clone = params.clone();

    let stream = device
        .build_output_stream(
            &config.into(),
            move |data: &mut [f32], _| {
                for sample in data.iter_mut() {
                    let freq = atomic_to_f32(params_clone.freq.load(Ordering::Relaxed));
                    voice.freq_smoother.set_target(freq);
                    *sample = voice.next() * 0.2;
                }
            },
            |err| eprintln!("audio error: {}", err),
            None,
        )
        .unwrap();
    stream.play().unwrap();
    println!("Synth running... press Ctrl+C to exit");

    loop {
        let mut input = [0u8; 1];
        io::stdin().read(&mut input).unwrap();

        let freq = match input[0] {
            b'a' => 110.0,
            b's' => 220.0,
            b'd' => 440.0,
            b'f' => 880.0,
            _ => continue,
        };

        params.freq.store(f32_to_atomic(freq), Ordering::Relaxed);
    }
}

fn main() {
    let host = cpal::default_host();
    let device = host.default_output_device().unwrap();
    let config = device.default_output_config().unwrap();
    let sample_rate = config.sample_rate() as f32;

    let mut voice = Voice::new(sample_rate);

    let params = Arc::new(Params {
        freq: AtomicU32::new(f32_to_atomic(110.0)),
    });

    {
        let freq = atomic_to_f32(params.freq.load(Ordering::Relaxed));
        voice.freq_smoother.set_target(freq);
    }
    stream_audio(device, voice, params, config);
}
