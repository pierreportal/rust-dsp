mod config;
mod control;
mod midi;
mod params;
mod poly;
mod stream;
use config::define_host;
use control::{Control, Next};
use dsp::{
    adsr::Adsr,
    distortion::Distortion,
    osc::{Osc, Waveform},
    patch,
    patch::Module,
    svf::Svf,
};
use params::Params;
use std::f32;
use std::sync::Arc;
use stream::{stream_audio, stream_poly_audio};

use crate::poly::PolyphonicVoice;

#[derive(Clone, Copy)]
struct Voice {
    osc: Osc,
    env: Adsr,
    filter: Svf,
    distortion: Distortion,
    filter_env: Adsr,
}

impl Voice {
    fn new(sample_rate: f32) -> Self {
        Self {
            osc: Osc::new(Waveform::Saw, 220.0, sample_rate),
            env: Adsr::new(sample_rate),
            filter: Svf::new(sample_rate),
            distortion: Distortion::new(),
            filter_env: Adsr::new(sample_rate),
        }
    }
}

impl Next for Voice {
    fn update(&mut self) {
        self.osc.freq = self.osc.freq_smoother.next_sample();
        let cutoff = self.filter.cutoff_smoother.next_sample();
        let filter_env_value = self.filter_env.next_sample();
        self.filter.set_cutoff(cutoff + filter_env_value * 300.0);
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
        // self.osc.freq_smoother.set_target(freq);
        self.env.trigger(vel);
        self.filter_env.trigger(vel);
    }

    fn note_off(&mut self) {
        self.env.release();
        self.filter_env.release();
    }
    fn set_float_param(&mut self, key: u8, value: f32) {
        match key {
            77 => self.filter.cutoff_smoother.set_target(value),
            65 => self.filter.resonance_smoother.set_target(value),
            _ => {}
        }
    }
}

fn base_voice(sample_rate: f32) -> Voice {
    let mut voice = Voice::new(sample_rate);

    voice.env.attack = 0.0;
    voice.env.release = 0.0;

    voice.filter_env.attack = 0.1;
    voice.filter_env.decay = 1.0;
    voice.filter_env.sustain = 0.0;
    voice.filter_env.release = 0.2;

    voice
}

const PHASE_STRIDE: f32 = 0.618_034;
const POOL_ATTACK_SECONDS: f32 = 0.002;
const POOL_RELEASE_SECONDS: f32 = 0.005;

fn mono_voice(sample_rate: f32) -> Voice {
    let mut voice = base_voice(sample_rate);
    voice.osc.freq_smoother.set_coeff(0.0005);
    voice
}

fn poly_voice(sample_rate: f32) -> Voice {
    let mut voice = base_voice(sample_rate);
    voice.osc.freq_smoother.set_coeff(1.0);
    voice.env.attack = POOL_ATTACK_SECONDS;
    voice.env.release = POOL_RELEASE_SECONDS;
    voice
}

fn poly_voices(sample_rate: f32) -> PolyphonicVoice<Voice> {
    PolyphonicVoice::with_voices(sample_rate, |index| {
        let mut voice = poly_voice(sample_rate);
        voice.osc.phase = (PHASE_STRIDE * index as f32) % 1.0;
        voice
    })
}

fn main() {
    let (device, config, sample_rate) = define_host();

    let voice_params = Arc::new(Params::new());

    if std::env::args().any(|arg| arg == "--mono") {
        stream_audio::<Voice>(device, voice_params, mono_voice(sample_rate), config);
    } else {
        let poly = poly_voices(sample_rate);
        stream_poly_audio::<Voice>(device, voice_params, poly, config);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pitch_after_one_sample(voice: &mut Voice, freq: f32) -> f32 {
        voice.osc.freq_smoother.set_target(freq);
        voice.osc.freq_smoother.next_sample()
    }

    #[test]
    fn poly_voices_play_their_own_pitch_immediately() {
        let mut voice = poly_voice(44100.0);

        assert!((pitch_after_one_sample(&mut voice, 261.63) - 261.63).abs() < 1e-6);
        assert!((pitch_after_one_sample(&mut voice, 880.0) - 880.0).abs() < 1e-6);
    }

    #[test]
    fn mono_voice_glides_to_the_played_pitch() {
        let mut voice = mono_voice(44100.0);

        assert!(pitch_after_one_sample(&mut voice, 880.0) < 300.0);
    }
}
