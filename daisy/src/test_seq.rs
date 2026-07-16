//! Internal test sequencer — pretends to be a MIDI keyboard playing a short
//! looping melody so we can flash the board, plug earphones in, and confirm
//! audio + synth are alive without needing a real MIDI controller.
//!
//! Flip [`crate::USE_TEST_SEQUENCER`] to `false` to disable and use MIDI IN.

use crate::midi::MidiEvent;

/// One step of the melody: (MIDI note, velocity, duration in 16th-notes).
#[derive(Clone, Copy)]
struct Step {
    note: u8,
    velocity: u8,
    /// Length of the step in sixteenth-notes. 1 = one 16th, 2 = eighth, 4 = quarter.
    len_16ths: u8,
}

/// Bass-line: two bars of A-minor-pentatonic 303-flavour.
/// Notes: A2=45, C3=48, D3=50, E3=52, G3=55, A3=57.
const MELODY: &[Step] = &[
    // bar 1
    Step { note: 45, velocity: 110, len_16ths: 2 }, // A2
    Step { note: 45, velocity: 90,  len_16ths: 2 },
    Step { note: 52, velocity: 100, len_16ths: 2 }, // E3
    Step { note: 48, velocity: 115, len_16ths: 2 }, // C3 (accent)
    Step { note: 55, velocity: 100, len_16ths: 2 }, // G3
    Step { note: 45, velocity: 90,  len_16ths: 2 },
    Step { note: 57, velocity: 120, len_16ths: 2 }, // A3 (accent)
    Step { note: 55, velocity: 100, len_16ths: 2 },
    // bar 2
    Step { note: 45, velocity: 110, len_16ths: 2 },
    Step { note: 50, velocity: 100, len_16ths: 2 }, // D3
    Step { note: 48, velocity: 90,  len_16ths: 2 },
    Step { note: 45, velocity: 115, len_16ths: 2 },
    Step { note: 52, velocity: 100, len_16ths: 4 }, // E3, longer
    Step { note: 55, velocity: 100, len_16ths: 2 },
    Step { note: 45, velocity: 100, len_16ths: 2 },
];

/// Tempo — 120 BPM feels right for a bass-line. One 16th note at 120 BPM = 125 ms.
const BPM: f32 = 120.0;

/// Gate length as a fraction of the step length (0..1). Shorter = staccato, more 303.
const GATE_FRACTION: f32 = 0.85;

pub struct TestSequencer {
    /// Samples per sixteenth-note.
    samples_per_16th: u32,
    /// Which step in `MELODY` we're currently on.
    step_idx: usize,
    /// Samples elapsed inside the current step.
    step_sample: u32,
    /// Samples we should hold the note for before emitting NoteOff.
    gate_samples: u32,
    /// True once we've emitted NoteOn for the current step (waiting for gate end).
    note_on_emitted: bool,
    /// True once we've emitted NoteOff for the current step.
    note_off_emitted: bool,
}

impl TestSequencer {
    pub fn new(sample_rate: f32) -> Self {
        let samples_per_16th = ((sample_rate * 60.0) / (BPM * 4.0)) as u32;
        Self {
            samples_per_16th,
            step_idx: 0,
            step_sample: 0,
            gate_samples: 0,
            note_on_emitted: false,
            note_off_emitted: false,
        }
    }

    /// Advance the sequencer by `n_samples` (one audio block). Emits any
    /// note-on / note-off events that fall inside the window via `sink`.
    pub fn tick(&mut self, n_samples: usize, mut sink: impl FnMut(MidiEvent)) {
        let n = n_samples as u32;

        // Fire NoteOn at the very start of the step if we haven't yet.
        if !self.note_on_emitted {
            let step = MELODY[self.step_idx];
            let step_samples = self.samples_per_16th * step.len_16ths as u32;
            self.gate_samples = ((step_samples as f32) * GATE_FRACTION) as u32;
            sink(MidiEvent::NoteOn {
                note: step.note,
                velocity: step.velocity,
            });
            self.note_on_emitted = true;
        }

        // Advance the counter. If we cross the gate boundary, emit NoteOff.
        // If we cross the step boundary, move to the next step.
        let step = MELODY[self.step_idx];
        let step_samples = self.samples_per_16th * step.len_16ths as u32;

        let new_sample = self.step_sample + n;

        if !self.note_off_emitted && new_sample >= self.gate_samples {
            sink(MidiEvent::NoteOff { note: step.note });
            self.note_off_emitted = true;
        }

        if new_sample >= step_samples {
            // Advance to next step, keep the overflow so tempo stays exact.
            let overflow = new_sample - step_samples;
            self.step_idx = (self.step_idx + 1) % MELODY.len();
            self.step_sample = overflow;
            self.note_on_emitted = false;
            self.note_off_emitted = false;
        } else {
            self.step_sample = new_sample;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advances_through_all_steps_and_loops() {
        let mut seq = TestSequencer::new(48_000.0);
        let mut note_ons = 0usize;
        let mut note_offs = 0usize;
        // Simulate 5 seconds — plenty for the loop to wrap.
        let block = 32;
        for _ in 0..(48_000 * 5 / block) {
            seq.tick(block, |ev| match ev {
                MidiEvent::NoteOn { .. } => note_ons += 1,
                MidiEvent::NoteOff { .. } => note_offs += 1,
                _ => {}
            });
        }
        assert!(note_ons >= MELODY.len(), "expected at least one full pass, got {}", note_ons);
        assert_eq!(note_ons, note_offs, "every note-on should be paired with a note-off");
    }
}
