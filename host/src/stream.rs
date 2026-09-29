use crate::control::Next;
use crate::midi::MidiController;
use crate::params::{NoteKind, Params};
use crate::poly::PolyphonicVoice;
use crate::Control;
use std::sync::Arc;

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Device, SupportedStreamConfig};

const MASTER_GAIN: f32 = 0.2;

pub fn stream_audio<T>(
    device: Device,
    params: Arc<Params>,
    mut voice: T,
    config: SupportedStreamConfig,
) where
    T: Control + Copy + Next + Send + 'static,
{
    let controller = MidiController {
        state: params.clone(),
    };
    let _connection = controller.connect(0);
    let mut prev_gate = 0;

    play(device, config, move |data| {
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

            *sample = voice.next_sample() * MASTER_GAIN;
        }
    });
}

pub fn stream_poly_audio<T>(
    device: Device,
    params: Arc<Params>,
    mut poly: PolyphonicVoice<T>,
    config: SupportedStreamConfig,
) where
    T: Control + Copy + Send + 'static,
{
    let controller = MidiController {
        state: params.clone(),
    };
    let _connection = controller.connect(0);

    play(device, config, move |data| {
        for sample in data.iter_mut() {
            while let Some(event) = params.pop_note() {
                match event.kind {
                    NoteKind::On => poly.key_on(event.note, event.vel),
                    NoteKind::Off => poly.key_off(event.note),
                }
            }

            *sample = poly.next_sample() * MASTER_GAIN;
        }
    });
}

fn play<F>(device: Device, config: SupportedStreamConfig, mut render: F)
where
    F: FnMut(&mut [f32]) + Send + 'static,
{
    println!(
        "\nSynth running! {} Hz, {:?}. Press ^C to quit.",
        config.sample_rate(),
        config.buffer_size()
    );

    let stream = device
        .build_output_stream(
            &config.into(),
            move |data: &mut [f32], _| render(data),
            |err| eprintln!("audio error: {}", err),
            None,
        )
        .unwrap();

    stream.play().unwrap();

    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
