//! Shared parameters between the main thread and the audio callback.
//!
//! The audio callback runs at interrupt priority; we cannot lock. Instead we
//! use `AtomicU32` slots holding bit-packed f32 values. Reads and writes are
//! individually atomic, and each parameter is independent so tearing across
//! parameters is not a concern.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::midi::MidiEvent;

pub struct SharedParams {
    // Filter cutoff bias, 0.0..1.0 → mapped to Hz in the audio callback.
    cutoff: AtomicU32,
    // Resonance, 0.0..1.0.
    resonance: AtomicU32,
    // Distortion drive, 1.0..20.0.
    drive: AtomicU32,
    // Decay time in seconds, 0.02..1.5.
    decay: AtomicU32,
}

impl SharedParams {
    pub const fn new() -> Self {
        Self {
            cutoff: AtomicU32::new(f32_to_bits(0.4)),
            resonance: AtomicU32::new(f32_to_bits(0.7)),
            drive: AtomicU32::new(f32_to_bits(4.0)),
            decay: AtomicU32::new(f32_to_bits(0.15)),
        }
    }

    pub fn set_cutoff(&self, v: f32) {
        self.cutoff.store(f32_to_bits(v), Ordering::Relaxed);
    }
    pub fn set_resonance(&self, v: f32) {
        self.resonance.store(f32_to_bits(v), Ordering::Relaxed);
    }
    pub fn set_drive(&self, v: f32) {
        self.drive.store(f32_to_bits(v), Ordering::Relaxed);
    }
    pub fn set_decay(&self, v: f32) {
        self.decay.store(f32_to_bits(v), Ordering::Relaxed);
    }

    pub fn cutoff(&self) -> f32 {
        f32_from_bits(self.cutoff.load(Ordering::Relaxed))
    }
    pub fn resonance(&self) -> f32 {
        f32_from_bits(self.resonance.load(Ordering::Relaxed))
    }
    pub fn drive(&self) -> f32 {
        f32_from_bits(self.drive.load(Ordering::Relaxed))
    }
    pub fn decay(&self) -> f32 {
        f32_from_bits(self.decay.load(Ordering::Relaxed))
    }

    /// Apply a MIDI CC to the shared parameter surface.
    /// Chosen so a standard controller (LinnStrument, launchpad, keystation)
    /// maps to something sensible out of the box.
    pub fn apply_cc(&self, ev: &MidiEvent) {
        if let MidiEvent::ControlChange { controller, value } = ev {
            let v = *value as f32 / 127.0;
            match controller {
                74 => self.set_cutoff(v),                    // "Sound Controller 5" — filter cutoff
                71 => self.set_resonance(v),                 // "Sound Controller 2" — resonance
                75 => self.set_drive(1.0 + v * 19.0),        // decay/drive alt
                72 => self.set_decay(0.02 + v * 1.48),       // release time
                _ => {}
            }
        }
    }
}

const fn f32_to_bits(x: f32) -> u32 {
    // const fn: no floating-point transmute on stable — use to_bits which is const-stable.
    x.to_bits()
}
fn f32_from_bits(x: u32) -> f32 {
    f32::from_bits(x)
}
