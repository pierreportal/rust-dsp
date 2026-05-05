mod midi;
mod params;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SupportedStreamConfig};
use dsp::voice::Voice;
use midi::MidiController;
use params::Params;
use std::sync::Arc;

fn stream_audio(
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

    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn main() {
    let host = cpal::default_host();
    let device = host.default_output_device().unwrap();
    let config = device.default_output_config().unwrap();
    let sample_rate = config.sample_rate() as f32;
    let params = Arc::new(Params::new());
    let controller = MidiController {
        state: params.clone(),
    };
    let voice = Voice::new(sample_rate);

    let _connection = controller.connect();
    stream_audio(device, voice, config, params);
}
