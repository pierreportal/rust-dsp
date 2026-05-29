use crate::midi::MidiController;
use crate::params::Params;
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Device, SupportedStreamConfig};
use dsp::voice::Voice;
use std::sync::Arc;

pub fn stream_audio(device: Device, mut voice: Voice, config: SupportedStreamConfig) {
    let params = Arc::new(Params::new());
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
                    let (freq, gate, vel) = params.get_params();
                    voice.freq_smoother.set_target(freq);
                    // Edge detection: trigger only on rising edge, release only on falling edge
                    if gate == 1 && prev_gate == 0 {
                        voice.env.trigger(vel);
                    } else if gate == 0 && prev_gate == 1 {
                        voice.env.release();
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
