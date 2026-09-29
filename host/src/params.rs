use std::sync::atomic::{AtomicU32, AtomicU8, AtomicUsize, Ordering};

const NOTE_QUEUE_SIZE: usize = 256;
const NOTE_ON_FLAG: u32 = 1 << 15;
const NOTE_FIELD_MASK: u32 = 0x7F;

#[inline]
fn f32_to_atomic(f: f32) -> u32 {
    f.to_bits()
}

#[inline]
fn atomic_to_f32(u: u32) -> f32 {
    f32::from_bits(u)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NoteKind {
    On,
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoteEvent {
    pub note: u8,
    pub vel: u8,
    pub kind: NoteKind,
}

impl NoteEvent {
    pub fn on(note: u8, vel: u8) -> Self {
        Self {
            note,
            vel,
            kind: NoteKind::On,
        }
    }

    pub fn off(note: u8) -> Self {
        Self {
            note,
            vel: 0,
            kind: NoteKind::Off,
        }
    }

    fn pack(self) -> u32 {
        let note = ((self.note as u32) & NOTE_FIELD_MASK) << 8;
        let vel = (self.vel as u32) & NOTE_FIELD_MASK;
        let flag = if self.kind == NoteKind::On {
            NOTE_ON_FLAG
        } else {
            0
        };
        note | vel | flag
    }

    fn unpack(bits: u32) -> Self {
        let note = ((bits >> 8) & NOTE_FIELD_MASK) as u8;
        let vel = (bits & NOTE_FIELD_MASK) as u8;
        let kind = if (bits & NOTE_ON_FLAG) != 0 {
            NoteKind::On
        } else {
            NoteKind::Off
        };
        Self { note, vel, kind }
    }
}

#[derive(Debug)]
struct NoteQueue {
    slots: [AtomicU32; NOTE_QUEUE_SIZE],
    read: AtomicUsize,
    write: AtomicUsize,
}

impl NoteQueue {
    fn new() -> Self {
        Self {
            slots: [const { AtomicU32::new(0) }; NOTE_QUEUE_SIZE],
            read: AtomicUsize::new(0),
            write: AtomicUsize::new(0),
        }
    }

    fn push(&self, event: NoteEvent) {
        let write = self.write.load(Ordering::Relaxed);
        let read = self.read.load(Ordering::Acquire);

        if write.wrapping_sub(read) >= NOTE_QUEUE_SIZE {
            return;
        }

        self.slots[write % NOTE_QUEUE_SIZE].store(event.pack(), Ordering::Relaxed);
        self.write.store(write.wrapping_add(1), Ordering::Release);
    }

    fn pop(&self) -> Option<NoteEvent> {
        let read = self.read.load(Ordering::Relaxed);
        let write = self.write.load(Ordering::Acquire);

        if read == write {
            return None;
        }

        let event = NoteEvent::unpack(self.slots[read % NOTE_QUEUE_SIZE].load(Ordering::Relaxed));
        self.read.store(read.wrapping_add(1), Ordering::Release);

        Some(event)
    }
}

#[derive(Debug)]
pub struct Params {
    pub midi: AtomicU8,
    pub freq: AtomicU32,
    pub gate: AtomicU8,
    pub vel: AtomicU8,
    notes: NoteQueue,
}

#[allow(unused)]
impl Params {
    pub fn new() -> Self {
        Self {
            midi: AtomicU8::new(46),
            freq: AtomicU32::new(f32_to_atomic(110.0)),
            gate: AtomicU8::new(0),
            vel: AtomicU8::new(0),
            notes: NoteQueue::new(),
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
    pub fn get_midi(&self) -> u8 {
        self.midi.load(Ordering::Relaxed)
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
    pub fn set_midi(&self, midi: u8) {
        self.midi.store(midi, Ordering::Relaxed);
    }
    pub fn get_params(&self) -> (f32, u8, u8) {
        (self.get_freq(), self.get_gate(), self.get_vel())
    }
    pub fn push_note(&self, event: NoteEvent) {
        self.notes.push(event);
    }
    pub fn pop_note(&self) -> Option<NoteEvent> {
        self.notes.pop()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_event_round_trip() {
        let on = NoteEvent::on(60, 100);
        assert_eq!(on.note, 60);
        assert_eq!(on.vel, 100);
        assert_eq!(on.kind, NoteKind::On);

        let off = NoteEvent::off(60);
        assert_eq!(off.note, 60);
        assert_eq!(off.vel, 0);
        assert_eq!(off.kind, NoteKind::Off);

        assert_eq!(NoteEvent::unpack(on.pack()), on);
        assert_eq!(NoteEvent::unpack(off.pack()), off);
    }

    #[test]
    fn queue_preserves_order_of_a_chord() {
        let params = Params::new();
        assert!(params.pop_note().is_none());

        params.push_note(NoteEvent::on(60, 100));
        params.push_note(NoteEvent::on(64, 90));
        params.push_note(NoteEvent::on(67, 80));
        params.push_note(NoteEvent::off(64));

        assert_eq!(params.pop_note(), Some(NoteEvent::on(60, 100)));
        assert_eq!(params.pop_note(), Some(NoteEvent::on(64, 90)));
        assert_eq!(params.pop_note(), Some(NoteEvent::on(67, 80)));
        assert_eq!(params.pop_note(), Some(NoteEvent::off(64)));
        assert!(params.pop_note().is_none());
    }

    #[test]
    fn queue_drops_events_when_full() {
        let params = Params::new();

        for i in 0..NOTE_QUEUE_SIZE {
            params.push_note(NoteEvent::on(60, (i % 127 + 1) as u8));
        }
        params.push_note(NoteEvent::on(72, 127));

        let mut drained = 0;
        while params.pop_note().is_some() {
            drained += 1;
        }

        assert_eq!(drained, NOTE_QUEUE_SIZE);
    }
}
