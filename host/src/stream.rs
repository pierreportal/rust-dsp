use crate::control::Next;
use crate::midi::MidiController;
use crate::params::Params;
use crate::Control;
use std::sync::Arc;

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Device, SupportedStreamConfig};

pub fn stream_audio<T>(
    device: Device,
    params: Arc<Params>,
    mut voice: T,
    config: SupportedStreamConfig,
) where
    T: Control + Next + Send + 'static,
{
    let controller = MidiController {
        state: params.clone(),
    };
    let _connection = controller.connect(0);
    let mut prev_gate = 0;
    let stream = device
        .build_output_stream(
            &config.into(),
            move |data: &mut [f32], _| {
                for sample in data.iter_mut() {
                    let freq = params.get_freq();
                    let gate = params.get_gate();
                    let vel = params.get_vel();

                    voice.set_freq(freq);

                    if gate == 1 && prev_gate == 0 {
                        voice.note_on(vel);
                    } else if gate == 0 && prev_gate == 1 {
                        voice.note_off();
                    }
                    prev_gate = gate;

                    *sample = voice.next_sample() * 0.2;
                }
            },
            |err| eprintln!("audio error: {}", err),
            None,
        )
        .unwrap();

    stream.play().unwrap();

    println!("\nSynth running! Press ^C to quit.");

    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
