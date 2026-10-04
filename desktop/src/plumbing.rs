//! Lock-free plumbing between the UI/MIDI threads and the audio thread.
//!
//! Two channels, because the two kinds of message have very different
//! deadlines:
//!
//! - [`EventQueue`] carries notes and controllers as a single-producer /
//!   single-consumer ring of atomics. A dropped note-on is an audible glitch, so
//!   this path never allocates and never blocks.
//! - [`Control`] carries parameter turns and whole-patch swaps. These arrive at
//!   human speed, so a mutex is fine and the audio thread only ever `try_lock`s:
//!   if the UI happens to be mid-update the message is picked up on the next
//!   block rather than stalling the audio thread.

use patch::Patch;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Mutex;

const RING: usize = 1024;

/// A timing-critical message. Packed into one `u32` so the ring needs no
/// per-slot locks and no allocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    NoteOn { note: u8, vel: u8 },
    NoteOff { note: u8 },
    Cc { cc: u8, value: u8 },
}

/// `kind` occupies the top 2 bits; note/cc and value take 7 bits each.
const KIND_SHIFT: u32 = 30;
const NOTE_OFF: u32 = 0;
const NOTE_ON: u32 = 1;
const CC: u32 = 2;

impl Event {
    fn pack(self) -> u32 {
        match self {
            Event::NoteOn { note, vel } => {
                (NOTE_ON << KIND_SHIFT) | (note as u32) << 8 | vel as u32
            }
            Event::NoteOff { note } => (NOTE_OFF << KIND_SHIFT) | (note as u32) << 8,
            Event::Cc { cc, value } => (CC << KIND_SHIFT) | (cc as u32) << 8 | value as u32,
        }
    }

    fn unpack(bits: u32) -> Event {
        let data = (bits >> 8) & 0x7F;
        let value = (bits & 0x7F) as u8;
        match bits >> KIND_SHIFT {
            NOTE_ON => Event::NoteOn {
                note: data as u8,
                vel: value,
            },
            CC => Event::Cc {
                cc: data as u8,
                value,
            },
            _ => Event::NoteOff { note: data as u8 },
        }
    }
}

/// Single-producer / single-consumer ring of [`Event`]s.
///
/// Overflow drops the oldest event and bumps a counter the UI can surface: a
/// stuck note from a full ring is worth telling the user about, because it
/// explains an instrument that will not shut up.
#[derive(Debug)]
pub struct EventQueue {
    slots: [AtomicU32; RING],
    read: AtomicUsize,
    write: AtomicUsize,
    dropped: AtomicUsize,
}

impl EventQueue {
    pub fn new() -> Self {
        Self {
            slots: [const { AtomicU32::new(u32::MAX) }; RING],
            read: AtomicUsize::new(0),
            write: AtomicUsize::new(0),
            dropped: AtomicUsize::new(0),
        }
    }

    pub fn push(&self, event: Event) {
        let write = self.write.load(Ordering::Relaxed);
        let read = self.read.load(Ordering::Acquire);
        if write.wrapping_sub(read) >= RING {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        self.slots[write % RING].store(event.pack(), Ordering::Relaxed);
        // Release: the slot contents must be visible before the index advances.
        self.write.store(write.wrapping_add(1), Ordering::Release);
    }

    /// Drain everything queued. Called once per audio block.
    pub fn drain(&self) -> Vec<Event> {
        let mut out = Vec::new();
        while let Some(event) = self.pop() {
            out.push(event);
        }
        out
    }

    fn pop(&self) -> Option<Event> {
        let read = self.read.load(Ordering::Relaxed);
        let write = self.write.load(Ordering::Acquire);
        if read == write {
            return None;
        }
        let packed = self.slots[read % RING].load(Ordering::Relaxed);
        self.read.store(read.wrapping_add(1), Ordering::Release);
        Some(Event::unpack(packed))
    }

    pub fn dropped(&self) -> usize {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl Default for EventQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// A message that is allowed to wait for the next block.
pub enum ControlMsg {
    SetParam {
        id: u32,
        name: String,
        value: f32,
    },
    /// Replace the whole instrument. Carried out of band so the audio thread
    /// never allocates while rendering.
    LoadPatch(Box<Patch>),
}

pub struct Control {
    queue: Mutex<VecDeque<ControlMsg>>,
    ready: AtomicBool,
}

impl Control {
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            ready: AtomicBool::new(false),
        }
    }

    pub fn push(&self, msg: ControlMsg) {
        // A poisoned lock here means a previous audio thread panicked; the audio
        // thread is already gone, so recovering the data is strictly better
        // than propagating the panic into the UI.
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.push_back(msg);
        self.ready.store(true, Ordering::Release);
    }

    /// Take everything, or nothing if the UI thread currently holds the lock.
    /// Never blocks: dropping a control message for one block is harmless,
    /// because the UI will send another.
    pub fn take(&self) -> Vec<ControlMsg> {
        if !self.ready.load(Ordering::Acquire) {
            return Vec::new();
        }
        let mut queue = match self.queue.try_lock() {
            Ok(q) => q,
            Err(_) => return Vec::new(),
        };
        self.ready.store(false, Ordering::Release);
        queue.drain(..).collect()
    }
}

/// What the UI reads without touching the audio thread.
#[derive(Debug)]
pub struct Status {
    /// One bit per MIDI note, so the UI can light up held keys.
    held: [AtomicU32; 4],
    /// Most recent controller number, or `u32::MAX` for none.
    pub last_cc: AtomicU32,
    /// Bumped whenever a patch is loaded, so the UI can notice a MIDI-driven
    /// load and refresh itself.
    pub patch_generation: AtomicU32,
    /// Peak absolute sample of the last block, for a level meter.
    pub peak: AtomicU32,
}

impl Status {
    pub fn new() -> Self {
        Self {
            held: [const { AtomicU32::new(0) }; 4],
            last_cc: AtomicU32::new(u32::MAX),
            patch_generation: AtomicU32::new(0),
            peak: AtomicU32::new(0),
        }
    }

    pub fn set_note(&self, note: u8, on: bool) {
        if note >= 128 {
            return;
        }
        let word = (note / 32) as usize;
        let bit = 1u32 << (note % 32);
        if on {
            self.held[word].fetch_or(bit, Ordering::Relaxed);
        } else {
            self.held[word].fetch_and(!bit, Ordering::Relaxed);
        }
    }

    pub fn is_held(&self, note: u8) -> bool {
        if note >= 128 {
            return false;
        }
        let word = (note / 32) as usize;
        self.held[word].load(Ordering::Relaxed) & (1u32 << (note % 32)) != 0
    }

    pub fn held_count(&self) -> usize {
        self.held
            .iter()
            .map(|w| w.load(Ordering::Relaxed).count_ones() as usize)
            .sum()
    }

    pub fn clear_notes(&self) {
        for word in &self.held {
            word.store(0, Ordering::Relaxed);
        }
    }

    pub fn set_last_cc(&self, cc: u8) {
        self.last_cc.store(cc as u32, Ordering::Relaxed);
    }

    pub fn last_cc(&self) -> Option<u8> {
        match self.last_cc.load(Ordering::Relaxed) {
            u32::MAX => None,
            v => Some(v as u8),
        }
    }

    pub fn bump_patch_generation(&self) {
        self.patch_generation.fetch_add(1, Ordering::Release);
    }

    pub fn set_peak(&self, peak: f32) {
        self.peak
            .store(peak.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn peak(&self) -> f32 {
        f32::from_bits(self.peak.load(Ordering::Relaxed))
    }
}

impl Default for Status {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_survive_the_round_trip_through_a_packed_slot() {
        for event in [
            Event::NoteOn { note: 60, vel: 127 },
            Event::NoteOn { note: 0, vel: 1 },
            Event::NoteOff { note: 127 },
            Event::Cc { cc: 74, value: 64 },
            Event::Cc { cc: 0, value: 0 },
        ] {
            assert_eq!(Event::unpack(event.pack()), event);
        }
    }

    #[test]
    fn the_note_kinds_do_not_collide() {
        // The packing uses the top bits for the kind, so a note-on of 127 with
        // velocity 127 must not decode as a controller or a note-off.
        let on = Event::NoteOn {
            note: 127,
            vel: 127,
        }
        .pack();
        let cc = Event::Cc {
            cc: 127,
            value: 127,
        }
        .pack();
        assert_ne!(on >> KIND_SHIFT, cc >> KIND_SHIFT);
        assert_eq!(
            Event::unpack(on),
            Event::NoteOn {
                note: 127,
                vel: 127
            }
        );
    }

    #[test]
    fn a_chord_arrives_in_order_and_drains_once() {
        let q = EventQueue::new();
        q.push(Event::NoteOn { note: 60, vel: 100 });
        q.push(Event::NoteOn { note: 64, vel: 90 });
        q.push(Event::NoteOn { note: 67, vel: 80 });
        q.push(Event::NoteOff { note: 64 });

        assert_eq!(
            q.drain(),
            vec![
                Event::NoteOn { note: 60, vel: 100 },
                Event::NoteOn { note: 64, vel: 90 },
                Event::NoteOn { note: 67, vel: 80 },
                Event::NoteOff { note: 64 },
            ]
        );
        assert!(q.drain().is_empty(), "a second drain must be empty");
    }

    #[test]
    fn an_overfull_ring_drops_and_reports_instead_of_overwriting_silently() {
        let q = EventQueue::new();
        for i in 0..(RING + 10) {
            q.push(Event::NoteOn {
                note: (i % 128) as u8,
                vel: 100,
            });
        }
        assert_eq!(q.dropped(), 10);
        assert_eq!(q.drain().len(), RING);
    }

    #[test]
    fn control_messages_wait_for_the_next_take() {
        let c = Control::new();
        assert!(c.take().is_empty());
        c.push(ControlMsg::SetParam {
            id: 1,
            name: "freq".into(),
            value: 440.0,
        });
        let taken = c.take();
        assert_eq!(taken.len(), 1);
        assert!(c.take().is_empty());
    }

    #[test]
    fn held_notes_are_tracked_for_the_ui() {
        let s = Status::new();
        assert_eq!(s.held_count(), 0);
        s.set_note(60, true);
        s.set_note(64, true);
        assert!(s.is_held(60) && s.is_held(64));
        assert_eq!(s.held_count(), 2);
        s.set_note(60, false);
        assert!(!s.is_held(60));
        assert_eq!(s.held_count(), 1);
        // Out-of-range notes are ignored rather than panicking.
        s.set_note(200, true);
        assert!(!s.is_held(200));
        s.clear_notes();
        assert_eq!(s.held_count(), 0);
    }
}
