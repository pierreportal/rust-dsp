mod config;
mod control;
mod midi;
mod params;
mod stream;
use config::define_host;

use params::Params;
use std::sync::Arc;

use control::{Control, Next};
use dsp::{
    adsr::Adsr,
    distortion::Distortion,
    osc::{Osc, Waveform},
    patch,
    patch::Module,
    svf::Svf,
};
use std::f32;
use stream::stream_audio;

struct Voice {
    osc: Osc,
    env: Adsr,
    filter: Svf,
    distortion: Distortion,
}

impl Voice {
    fn new(sample_rate: f32) -> Self {
        Self {
            osc: Osc::new(Waveform::Saw, 220.0, sample_rate),
            env: Adsr::new(sample_rate),
            filter: Svf::new(sample_rate),
            distortion: Distortion::new(),
        }
    }
}

impl Next for Voice {
    fn update(&mut self) {
        self.osc.freq = self.osc.freq_smoother.next_sample();

        let cutoff = self.filter.cutoff_smoother.next_sample();
        self.filter.set_cutoff(cutoff);

        let resonance = self.filter.resonance_smoother.next_sample();
        self.filter.set_resonance(resonance);
    }

    fn patch(&mut self) -> f32 {
        patch!(self.osc =>  self.env => self.filter => self.distortion)(1.0)
    }
}

impl Control for Voice {
    fn next_sample(&mut self) -> f32 {
        self.update();
        self.patch()
    }
    fn set_freq(&mut self, freq: f32) {
        self.osc.freq_smoother.set_target(freq);
    }
    fn note_on(&mut self, vel: u8) {
        self.env.trigger(vel);
    }
    fn note_off(&mut self) {
        self.env.release();
    }
    fn set_float_param(&mut self, key: u8, value: f32) {
        match key {
            77 => self.filter.cutoff_smoother.set_target(value),
            65 => self.filter.resonance_smoother.set_target(value),
            _ => {}
        }
    }
}

fn main() {
    let (device, config, sample_rate) = define_host();

    let mut voice = Voice::new(sample_rate);

    voice.osc.freq_smoother.set_coeff(0.0005);

    let voice_params = Arc::new(Params::new());

    stream_audio::<Voice>(device, voice_params, voice, config);
}
