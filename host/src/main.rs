mod config;
mod control;
mod midi;
mod params;
mod stream;
use config::define_host;
use control::{Control, Next};
use dsp::{
    adsr::Adsr,
    distortion::Distortion,
    osc::{Osc, Waveform},
    patch,
    patch::Module,
    smoother::Smoother,
    svf::Svf,
};
use std::f32;
use stream::stream_audio;

struct Voice {
    osc: Osc,
    env: Adsr,
    filter: Svf,
    distortion: Distortion,
    freq_smoother: Smoother,
    filter_cutoff_smoother: Smoother,
    filter_resonance_smoother: Smoother,
}

impl Voice {
    fn new(sample_rate: f32) -> Self {
        Self {
            osc: Osc::new(Waveform::Saw, 220.0, sample_rate),
            env: Adsr::new(sample_rate),
            filter: Svf::new(sample_rate),
            distortion: Distortion::new(),
            freq_smoother: Smoother::new(220.0, 0.00005),
            filter_cutoff_smoother: Smoother::new(2000.0, 0.0005),
            filter_resonance_smoother: Smoother::new(2000.0, 0.0005),
        }
    }
}

impl Next for Voice {
    fn update(&mut self) {
        self.osc.freq = self.freq_smoother.next_sample();

        let cutoff = self.filter_cutoff_smoother.next_sample();
        self.filter.set_cutoff(cutoff);

        let resonance = self.filter_resonance_smoother.next_sample();
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
        self.freq_smoother.set_target(freq);
    }
    fn note_on(&mut self, vel: u8) {
        self.env.trigger(vel);
    }
    fn note_off(&mut self) {
        self.env.release();
    }
    fn set_float_param(&mut self, key: u8, value: f32) {
        match key {
            77 => self.filter_cutoff_smoother.set_target(value),
            65 => self.filter_resonance_smoother.set_target(value),
            _ => {}
        }
    }
}

fn main() {
    let (device, config, sample_rate) = define_host();
    let voice = Voice::new(sample_rate);
    stream_audio::<Voice>(device, voice, config);
}
