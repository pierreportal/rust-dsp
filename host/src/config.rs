use cpal::{
    traits::{DeviceTrait, HostTrait},
    Device, SupportedStreamConfig,
};

pub fn define_host() -> (Device, SupportedStreamConfig, f32) {
    let host = cpal::default_host();
    let device = host.default_output_device().unwrap();
    let config = device.default_output_config().unwrap();
    let sample_rate = config.sample_rate() as f32;
    (device, config, sample_rate)
}
