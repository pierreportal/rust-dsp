use crate::params::Params;
use midir::{Ignore, MidiInput};
use std::sync::Arc;
use std::sync::atomic::Ordering;

fn midi_note_to_freq(note: u8) -> f32 {
    440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0)
}

pub struct MidiController {
    pub state: Arc<Params>,
}

impl MidiController {
    pub fn start_midi(&self) -> Result<midir::MidiInputConnection<Arc<Params>>, String> {
        let mut midi_in = MidiInput::new("keyboard input").map_err(|e| e.to_string())?;
        midi_in.ignore(Ignore::None);
        let in_ports = midi_in.ports();

        if in_ports.is_empty() {
            return Err(String::from("No MIDI device detected."));
        }

        for (i, p) in in_ports.iter().enumerate() {
            println!("{}: {}", i, midi_in.port_name(p).unwrap());
        }

        let port = &in_ports[0];
        let state_clone = self.state.clone();
        midi_in
            .connect(
                port,
                "midi-read",
                move |_, msg, state| handle_midi(state, msg),
                state_clone,
            )
            .map_err(|e| e.to_string())
    }
}

fn handle_midi(midi_state: &Params, msg: &[u8]) {
    let status = msg[0] & 0xF0;
    match status {
        0x90 => {
            let note = msg[1];
            let vel = msg[2];
            if vel > 0 {
                let freq = midi_note_to_freq(note);
                midi_state.freq.store(freq.to_bits(), Ordering::Relaxed);
                midi_state.gate.store(1, Ordering::Relaxed);
                midi_state.vel.store(vel, Ordering::Relaxed);
            } else {
                midi_state.gate.store(0, Ordering::Relaxed);
                midi_state.vel.store(0, Ordering::Relaxed);
            }
        }
        0x80 => {
            midi_state.gate.store(0, Ordering::Relaxed);
            midi_state.vel.store(0, Ordering::Relaxed);
        }
        _ => {}
    }
}
