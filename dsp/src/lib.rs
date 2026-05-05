#![cfg_attr(not(feature = "std"), no_std)]
mod adsr;
mod distortion;
mod filter;
mod osc;
mod smoother;

pub mod voice;
use voice::Voice;

#[repr(C)]
pub struct VoiceWrapper {
    voice: Voice,
}

#[unsafe(no_mangle)]
pub extern "C" fn voice_init(v: &mut VoiceWrapper, sample_rate: f32) {
    v.voice = Voice::new(sample_rate)
}

#[unsafe(no_mangle)]
pub extern "C" fn voice_process(v: &mut VoiceWrapper) -> f32 {
    v.voice.next()
}

#[unsafe(no_mangle)]
pub extern "C" fn note_on(v: &mut VoiceWrapper, freq: f32) {
    v.voice.freq_smoother.set_target(freq);
    v.voice.env.trigger(1);
}

#[unsafe(no_mangle)]
pub extern "C" fn set_cutoff(v: &mut VoiceWrapper, cutoff: f32) {
    v.voice.filter.cutoff = cutoff;
}
