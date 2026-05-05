use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

#[inline]
fn f32_to_atomic(f: f32) -> u32 {
    f.to_bits()
}

#[inline]
fn atomic_to_f32(u: u32) -> f32 {
    f32::from_bits(u)
}

#[derive(Debug)]
pub struct Params {
    pub freq: AtomicU32,
    pub gate: AtomicU8,
    pub vel: AtomicU8,
}

#[allow(unused)]
impl Params {
    pub fn new() -> Self {
        Params {
            freq: AtomicU32::new(f32_to_atomic(110.0)),
            gate: AtomicU8::new(0),
            vel: AtomicU8::new(0),
        }
    }
    pub fn get_freq(&self) -> f32 {
        let freq = self.freq.load(Ordering::Relaxed);
        atomic_to_f32(freq)
    }
    pub fn get_gate(&self) -> u8 {
        self.gate.load(Ordering::Relaxed)
    }
    pub fn get_vel(&self) -> u8 {
        self.vel.load(Ordering::Relaxed)
    }
    pub fn set_freq(&self, freq: f32) {
        self.freq.store(f32_to_atomic(freq), Ordering::Relaxed);
    }
    pub fn set_gate(&self, gate: u8) {
        self.gate.store(gate, Ordering::Relaxed);
    }
    pub fn set_vel(&self, vel: u8) {
        self.vel.store(vel, Ordering::Relaxed);
    }
    pub fn get_params(&self) -> (f32, u8, u8) {
        (self.get_freq(), self.get_gate(), self.get_vel())
    }
}
