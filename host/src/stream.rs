use crate::params::Params;
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Device, SupportedStreamConfig};
use dsp::voice::Voice;
use std::sync::Arc;

pub fn stream_audio(
    device: Device,
    mut voice: Voice,
    config: SupportedStreamConfig,
    midi_state: Arc<Params>,
) {
    let stream = device
        .build_output_stream(
            &config.into(),
            move |data: &mut [f32], _| {
                for sample in data.iter_mut() {
                    let (freq, gate, vel) = midi_state.get_params();
                    voice.freq_smoother.set_target(freq);
                    if gate == 1 {
                        voice.env.trigger(vel);
                    } else {
                        voice.env.release();
                    }
                    *sample = voice.next() * 0.2;
                }
            },
            |err| eprintln!("audio error: {}", err),
            None,
        )
        .unwrap();

    stream.play().unwrap();

    println!("Synth running! Press ^C to quit.");

    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
