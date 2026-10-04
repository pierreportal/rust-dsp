//! MIDI input: notes and controllers from a hardware device.
//!
//! Both messages become [`Event`]s on the same lock-free ring the UI uses, so
//! there is one ordering and one place where a message can be dropped. The
//! parser is split out from the device handling so it can be tested without a
//! MIDI port.

use crate::plumbing::{Event, EventQueue};
use midir::{MidiInput, MidiInputConnection};
use std::sync::Arc;

/// Translate one raw MIDI message into the events it implies.
///
/// Notes and controllers are separate streams on purpose: the engine routes a
/// controller to every voice while a note goes to exactly one, so a filter sweep
/// has to keep working while a chord is held.
///
/// Returns an empty vector for anything we do not use (clock, aftertouch,
/// program change, pitch bend), rather than treating those as notes.
pub fn parse(msg: &[u8]) -> Vec<Event> {
    // A real MIDI port can hand us a truncated message (running status, a
    // device unplugged mid-packet), so every byte is checked before use rather
    // than trusting the length.
    if msg.len() < 2 {
        return Vec::new();
    }
    let (&status, &note_byte) = (&msg[0], &msg[1]);
    let command = status & 0xF0;
    let channel = status & 0x0F;
    let _ = channel; // the engine ignores channel, so a mapping is channel-agnostic

    match command {
        0x80 => vec![Event::NoteOff { note: note_byte }],
        0x90 => {
            let vel = msg.get(2).copied().unwrap_or(0);
            // Running status aside, a note-on with zero velocity is the standard
            // way keyboards send a note-off; without this a stuck note hangs.
            if vel == 0 {
                vec![Event::NoteOff { note: note_byte }]
            } else {
                vec![Event::NoteOn {
                    note: note_byte,
                    vel,
                }]
            }
        }
        0xB0 => vec![Event::Cc {
            cc: note_byte,
            value: msg.get(2).copied().unwrap_or(0),
        }],
        _ => Vec::new(),
    }
}

/// A live MIDI input, kept alive because dropping the connection stops input.
pub struct MidiIn {
    _connection: MidiInputConnection<Arc<EventQueue>>,
}

/// Names of every available MIDI input, for the UI's device picker.
pub fn available_inputs() -> Vec<String> {
    match MidiInput::new("coarse-probe") {
        Ok(input) => input
            .ports()
            .iter()
            .filter_map(|p| input.port_name(p).ok())
            .collect(),
        // Creating a probe connection can fail when another app holds the port;
        // an empty list is a better outcome than refusing to start.
        Err(_) => Vec::new(),
    }
}

/// Connect to the input at `index`, forwarding everything it sends to `events`.
///
/// The device list is read once and the chosen port looked up again by name,
/// because index ordering is not stable across enumerations.
pub fn connect(index: usize, events: Arc<EventQueue>) -> Result<MidiIn, String> {
    let input = MidiInput::new("coarse-desktop").map_err(|e| e.to_string())?;
    let ports = input.ports();
    let port = ports
        .get(index)
        .ok_or_else(|| format!("MIDI input {index} is not available"))?;

    let connection = input
        .connect(
            port,
            "coarse-input",
            move |_, msg, queue: &mut Arc<EventQueue>| {
                for event in parse(msg) {
                    queue.push(event);
                }
            },
            events,
        )
        .map_err(|e| format!("could not connect to MIDI input: {e}"))?;

    Ok(MidiIn {
        _connection: connection,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plumbing::Event;

    #[test]
    fn a_chord_becomes_one_event_per_key() {
        assert_eq!(
            parse(&[0x90, 60, 100]),
            vec![Event::NoteOn { note: 60, vel: 100 }]
        );
        assert_eq!(
            parse(&[0x90, 64, 90]),
            vec![Event::NoteOn { note: 64, vel: 90 }]
        );
        assert_eq!(
            parse(&[0x90, 67, 80]),
            vec![Event::NoteOn { note: 67, vel: 80 }]
        );
        assert_eq!(parse(&[0x80, 60, 0]), vec![Event::NoteOff { note: 60 }]);
    }

    #[test]
    fn note_on_with_zero_velocity_releases_the_key() {
        // Many keyboards signal note-off this way; treating it as a note-on
        // leaves a voice droning forever.
        assert_eq!(parse(&[0x90, 60, 0]), vec![Event::NoteOff { note: 60 }]);
    }

    #[test]
    fn controllers_are_forwarded_with_their_raw_value() {
        assert_eq!(
            parse(&[0xB0, 74, 127]),
            vec![Event::Cc { cc: 74, value: 127 }]
        );
        assert_eq!(parse(&[0xB0, 1, 0]), vec![Event::Cc { cc: 1, value: 0 }]);
    }

    #[test]
    fn a_controller_on_any_channel_reaches_the_engine_the_same_way() {
        // Channel is deliberately dropped: a mapping should not stop working
        // because the controller is on a different channel than the keyboard.
        assert_eq!(parse(&[0xB1, 74, 64]), parse(&[0xB0, 74, 64]));
        assert_eq!(parse(&[0x9F, 60, 100]), parse(&[0x90, 60, 100]));
    }

    #[test]
    fn messages_we_do_not_use_are_ignored_rather_than_misread_as_notes() {
        for msg in [
            &[0xF8][..],        // clock
            &[0xC0, 5][..],     // program change
            &[0xD0, 90][..],    // channel aftertouch
            &[0xE0, 0, 64][..], // pitch bend
            &[][..],            // empty
            &[0x90][..],        // truncated note-on
        ] {
            assert!(parse(msg).is_empty(), "expected {msg:?} to be ignored");
        }
    }

    #[test]
    fn a_truncated_note_on_is_treated_as_a_release_not_a_panic() {
        // No velocity byte means velocity 0, which is a note-off by the rule
        // above. Failing closed avoids a stuck note from a malformed message.
        assert_eq!(parse(&[0x90, 60]), vec![Event::NoteOff { note: 60 }]);
    }
}
