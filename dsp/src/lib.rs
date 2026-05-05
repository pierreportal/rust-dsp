#![cfg_attr(not(feature = "std"), no_std)]
mod distortion;
mod env;
mod filter;
mod osc;
mod smoother;

pub mod voice;

use crate::{distortion::Distortion, osc::Waveform, smoother::Smoother};
use voice::Voice;

#[repr(C)]
pub struct VoiceWrapper {
    voice: Voice,
}

#[unsafe(no_mangle)]
pub extern "C" fn voice_init(v: &mut VoiceWrapper, sample_rate: f32) {
    v.voice = Voice {
        osc: osc::Osc {
            phase: 0.0,
            freq: 440.0,
            waveform: Waveform::Triangle,
            sample_rate,
        },
        env: env::Env {
            value: 0.0,
            decay: 0.5,
            active: false,
            sample_rate,
        },
        filter: filter::Filter {
            cutoff: 1000.0,
            z: 0.0,
            sample_rate,
        },
        distortion: Distortion {
            output_gain: 1.0,
            drive: 1.0,
        },
        freq_smoother: Smoother::new(440.0, 0.001),
    };
}

#[unsafe(no_mangle)]
pub extern "C" fn voice_process(v: &mut VoiceWrapper) -> f32 {
    v.voice.next()
}

#[unsafe(no_mangle)]
pub extern "C" fn note_on(v: &mut VoiceWrapper, freq: f32) {
    v.voice.freq_smoother.set_target(freq);
    v.voice.env.trigger();
}

#[unsafe(no_mangle)]
pub extern "C" fn set_cutoff(v: &mut VoiceWrapper, cutoff: f32) {
    v.voice.filter.cutoff = cutoff;
}
