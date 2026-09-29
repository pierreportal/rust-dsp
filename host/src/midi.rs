use crate::params::{NoteEvent, Params};
use midir::{Ignore, MidiInput};
use std::sync::Arc;

fn midi_note_to_freq(note: u8) -> f32 {
    440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0)
}

pub struct MidiController {
    pub state: Arc<Params>,
}

impl MidiController {
    pub fn connect(&self, port: usize) -> Result<midir::MidiInputConnection<Arc<Params>>, String> {
        let mut midi_in = MidiInput::new("keyboard input").map_err(|e| e.to_string())?;
        midi_in.ignore(Ignore::None);
        let in_ports = midi_in.ports();
        if in_ports.is_empty() {
            return Err(String::from("No MIDI device detected."));
        }
        for (i, p) in in_ports.iter().enumerate() {
            println!("{}: {}", i, midi_in.port_name(p).unwrap());
        }
        let port = &in_ports[port];
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
            println!("Note On: {:?}", msg);
            let note = msg[1];
            let vel = msg[2];
            if vel > 0 {
                key_on(note, vel, midi_state);
            } else {
                key_off(note, midi_state);
            }
        }
        0x80 => {
            println!("Note Off: {:?}", msg);
            let note = msg[1];
            key_off(note, midi_state);
        }
        _ => {
            // Here we capture custom params
            // state.set_custom_params(...)
            println!("MIDI msg: {:?}", msg);
        }
    }
}

fn key_on(midi_note: u8, vel: u8, state: &Params) {
    let freq = midi_note_to_freq(midi_note);
    state.set_freq(freq);
    state.set_gate(1);
    state.set_vel(vel);
    state.set_midi(midi_note);
    state.push_note(NoteEvent::on(midi_note, vel));
}

fn key_off(midi_note: u8, state: &Params) {
    state.push_note(NoteEvent::off(midi_note));

    let active_midi_note = state.get_midi();
    if midi_note == active_midi_note {
        state.set_gate(0);
        state.set_vel(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chord_arrives_as_one_note_event_per_key() {
        let state = Params::new();

        handle_midi(&state, &[0x90, 60, 100]);
        handle_midi(&state, &[0x90, 64, 90]);
        handle_midi(&state, &[0x90, 67, 80]);
        handle_midi(&state, &[0x80, 64, 0]);

        assert_eq!(state.pop_note(), Some(NoteEvent::on(60, 100)));
        assert_eq!(state.pop_note(), Some(NoteEvent::on(64, 90)));
        assert_eq!(state.pop_note(), Some(NoteEvent::on(67, 80)));
        assert_eq!(state.pop_note(), Some(NoteEvent::off(64)));
        assert_eq!(state.pop_note(), None);
    }

    #[test]
    fn note_on_with_zero_velocity_releases_the_key() {
        let state = Params::new();

        handle_midi(&state, &[0x90, 60, 100]);
        handle_midi(&state, &[0x90, 60, 0]);

        assert_eq!(state.pop_note(), Some(NoteEvent::on(60, 100)));
        assert_eq!(state.pop_note(), Some(NoteEvent::off(60)));
    }

    #[test]
    fn releasing_a_chord_key_keeps_the_other_notes_pressed() {
        let state = Params::new();

        handle_midi(&state, &[0x90, 60, 100]);
        handle_midi(&state, &[0x90, 67, 100]);
        handle_midi(&state, &[0x80, 60, 0]);

        assert_eq!(state.pop_note(), Some(NoteEvent::on(60, 100)));
        assert_eq!(state.pop_note(), Some(NoteEvent::on(67, 100)));
        assert_eq!(state.pop_note(), Some(NoteEvent::off(60)));

        while state.pop_note().is_some() {}

        assert_eq!(state.get_gate(), 1);
    }
}
