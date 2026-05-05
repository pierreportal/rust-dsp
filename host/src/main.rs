mod midi;
mod params;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SupportedStreamConfig};
use dsp::voice::Voice;
use midi::{MidiController, MidiState};
use params::Params;
use std::sync::Arc;
use std::sync::atomic::Ordering;

fn stream_audio(
    device: Device,
    mut voice: Voice,
    params: Arc<Params>,
    config: SupportedStreamConfig,
    midi_state: Arc<MidiState>,
) {
    let params_clone = params.clone();
    let stream = device
        .build_output_stream(
            &config.into(),
            move |data: &mut [f32], _| {
                for sample in data.iter_mut() {
                    let freq = params_clone.get_freq();
                    let gate = params_clone.get_gate();
                    let vel = params_clone.get_vel();
                    voice.freq_smoother.set_target(freq);
                    if gate > 0 {
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
    println!("Synth running... press Ctrl+C to exit");

    loop {
        let input_freq = midi_state.freq.load(Ordering::Relaxed);
        let input_gate = midi_state.gate.load(Ordering::Relaxed);
        let input_vel = midi_state.vel.load(Ordering::Relaxed);
        params.set_ufreq(input_freq);
        params.set_gate(input_gate);
        params.set_vel(input_vel);
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn main() {
    let host = cpal::default_host();
    let device = host.default_output_device().unwrap();
    let config = device.default_output_config().unwrap();
    let sample_rate = config.sample_rate() as f32;
    let params = Arc::new(Params::new());

    let state = Arc::new(MidiState::new());
    let controller = MidiController {
        state: state.clone(),
    };

    let voice = Voice::new(sample_rate);

    match controller.start_midi() {
        Ok(_c) => stream_audio(device, voice, params, config, state),
        Err(e) => eprintln!("Failed to start MIDI: {}", e),
    }
}
