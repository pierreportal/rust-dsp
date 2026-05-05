use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use dsp::voice::Voice;

fn main() {
    let host = cpal::default_host();
    let device = host.default_output_device().unwrap();

    let config = device.default_output_config().unwrap();

    let sample_rate = config.sample_rate() as f32;

    let mut voice = Voice::new(sample_rate);
    voice.freq = 220.0;

    let stream = device
        .build_output_stream(
            &config.into(),
            move |data: &mut [f32], _| {
                for sample in data.iter_mut() {
                    *sample = voice.next();
                }
            },
            |err| eprintln!("audio error: {}", err),
            None,
        )
        .unwrap();

    stream.play().unwrap();

    println!("Synth running... press Ctrl+C to exit");

    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
