#![cfg_attr(not(feature = "std"), no_std)]

mod env;
mod filter;
mod osc;
//mod panic;
pub mod voice;

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
        freq: 440.0,
    };
}

#[unsafe(no_mangle)]
pub extern "C" fn voice_process(v: &mut VoiceWrapper) -> f32 {
    v.voice.next()
}

#[unsafe(no_mangle)]
pub extern "C" fn note_on(v: &mut VoiceWrapper, freq: f32) {
    v.voice.freq = freq;
    v.voice.env.trigger();
}

#[unsafe(no_mangle)]
pub extern "C" fn set_cutoff(v: &mut VoiceWrapper, cutoff: f32) {
    v.voice.filter.cutoff = cutoff;
}
