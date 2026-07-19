//! Shared parameters between the main thread and the audio callback.
//!
//! The audio callback runs at interrupt priority; we cannot lock. Instead we
//! use `AtomicU32` slots holding bit-packed f32 values. Reads and writes are
//! individually atomic, and each parameter is independent so tearing across
//! parameters is not a concern.

use core::sync::atomic::{AtomicU32, Ordering};

pub struct SharedParams {
    param1: AtomicU32,
    param2: AtomicU32,
    param3: AtomicU32,
    // param4: AtomicU32,
}

impl SharedParams {
    pub const fn new() -> Self {
        Self {
            param1: AtomicU32::new(f32_to_bits(0.4)),
            param2: AtomicU32::new(f32_to_bits(0.7)),
            param3: AtomicU32::new(f32_to_bits(0.2)),
            // param4: AtomicU32::new(f32_to_bits(0.15)),
        }
    }

    pub fn set_param1(&self, v: f32) {
        self.param1.store(f32_to_bits(v), Ordering::Relaxed);
    }
    pub fn set_param2(&self, v: f32) {
        self.param2.store(f32_to_bits(v), Ordering::Relaxed);
    }
    pub fn set_param3(&self, v: f32) {
        self.param3.store(f32_to_bits(v), Ordering::Relaxed);
    }
    // pub fn set_param4(&self, v: f32) {
    //     self.param4.store(f32_to_bits(v), Ordering::Relaxed);
    // }

    pub fn read_param1(&self) -> f32 {
        f32_from_bits(self.param1.load(Ordering::Relaxed))
    }
    pub fn read_param2(&self) -> f32 {
        f32_from_bits(self.param2.load(Ordering::Relaxed))
    }
    pub fn read_param3(&self) -> f32 {
        f32_from_bits(self.param3.load(Ordering::Relaxed))
    }
    // pub fn decay(&self) -> f32 {
    //     f32_from_bits(self.param4.load(Ordering::Relaxed))
    // }
}

const fn f32_to_bits(x: f32) -> u32 {
    // const fn: no floating-point transmute on stable — use to_bits which is const-stable.
    x.to_bits()
}
fn f32_from_bits(x: u32) -> f32 {
    f32::from_bits(x)
}
