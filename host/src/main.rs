mod config;
mod midi;
mod params;
mod stream;
use config::define_host;
use dsp::voice::Voice;
use stream::stream_audio;

fn main() {
    let (device, config, sample_rate) = define_host();
    let voice = Voice::new(sample_rate);
    stream_audio(device, voice, config);
}
