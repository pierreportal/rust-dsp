mod midi;
mod params;
mod stream;
use cpal::{
    Device, SupportedStreamConfig,
    traits::{DeviceTrait, HostTrait},
};
use dsp::voice::Voice;
use stream::stream_audio;

fn define_host() -> (Device, SupportedStreamConfig, f32) {
    let host = cpal::default_host();
    let device = host.default_output_device().unwrap();
    let config = device.default_output_config().unwrap();
    let sample_rate = config.sample_rate() as f32;
    (device, config, sample_rate)
}

fn main() {
    let (device, config, sample_rate) = define_host();

    let voice = Voice::new(sample_rate);

    stream_audio(device, voice, config);
}
