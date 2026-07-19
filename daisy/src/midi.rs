//! Minimal streaming MIDI parser — `no_std`, no allocation, running-status aware.
//!
//! Only the messages the acid bass cares about are emitted. Everything else
//! (SysEx, MTC, aftertouch, etc.) is silently consumed so the parser stays in
//! sync but doesn't waste cycles decoding what we ignore.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiEvent {
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8 },
}

/// State for a running-status streaming parser.
#[allow(dead_code)]
pub struct MidiParser {
    status: u8,
    data1: u8,
    have_data1: bool,
    in_sysex: bool,
}

#[allow(dead_code)]
impl MidiParser {
    pub const fn new() -> Self {
        Self {
            status: 0,
            data1: 0,
            have_data1: false,
            in_sysex: false,
        }
    }

    /// Feed one raw byte; returns Some(event) whenever a complete message is decoded.
    pub fn push(&mut self, byte: u8) -> Option<MidiEvent> {
        // Real-time messages (0xF8..=0xFF) can appear anywhere and don't disturb state.
        if byte >= 0xF8 {
            return None;
        }

        if self.in_sysex {
            if byte == 0xF7 {
                self.in_sysex = false;
            }
            return None;
        }

        if byte & 0x80 != 0 {
            // Status byte
            if byte == 0xF0 {
                self.in_sysex = true;
                return None;
            }
            // System common (non-sysex) — reset running status.
            if byte >= 0xF1 && byte <= 0xF7 {
                self.status = 0;
                self.have_data1 = false;
                return None;
            }
            self.status = byte;
            self.have_data1 = false;
            return None;
        }

        // Data byte — needs a running status.
        if self.status == 0 {
            return None;
        }

        let cmd = self.status & 0xF0;
        match cmd {
            0x80 | 0x90 | 0xB0 | 0xE0 => {
                // 2-data-byte messages
                if !self.have_data1 {
                    self.data1 = byte;
                    self.have_data1 = true;
                    None
                } else {
                    let d1 = self.data1;
                    let d2 = byte;
                    self.have_data1 = false; // ready for next running-status message
                    match cmd {
                        0x90 => {
                            if d2 == 0 {
                                Some(MidiEvent::NoteOff { note: d1 })
                            } else {
                                Some(MidiEvent::NoteOn {
                                    note: d1,
                                    velocity: d2,
                                })
                            }
                        }
                        0x80 => Some(MidiEvent::NoteOff { note: d1 }),

                        _ => None,
                    }
                }
            }
            0xC0 | 0xD0 => {
                // 1-data-byte messages (program change, channel pressure) — ignored.
                None
            }
            _ => None,
        }
    }
}

impl Default for MidiParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(p: &mut MidiParser, bytes: &[u8]) -> EventBuf {
        let mut out = EventBuf::new();
        for b in bytes {
            if let Some(ev) = p.push(*b) {
                out.push(ev);
            }
        }
        out
    }

    // Tiny fixed-capacity vec so tests stay no_std-friendly.
    struct EventBuf {
        buf: [Option<MidiEvent>; 8],
        len: usize,
    }
    impl EventBuf {
        fn new() -> Self {
            Self {
                buf: [None; 8],
                len: 0,
            }
        }
        fn push(&mut self, ev: MidiEvent) {
            self.buf[self.len] = Some(ev);
            self.len += 1;
        }
        fn get(&self, i: usize) -> MidiEvent {
            self.buf[i].unwrap()
        }
    }

    #[test]
    fn parses_note_on() {
        let mut p = MidiParser::new();
        let ev = feed(&mut p, &[0x90, 60, 100]);
        assert_eq!(ev.len, 1);
        assert_eq!(
            ev.get(0),
            MidiEvent::NoteOn {
                note: 60,
                velocity: 100
            }
        );
    }

    #[test]
    fn note_on_with_velocity_zero_is_note_off() {
        let mut p = MidiParser::new();
        let ev = feed(&mut p, &[0x90, 60, 0]);
        assert_eq!(ev.len, 1);
        assert_eq!(ev.get(0), MidiEvent::NoteOff { note: 60 });
    }

    #[test]
    fn running_status_reuses_last_status() {
        let mut p = MidiParser::new();
        let ev = feed(&mut p, &[0x90, 60, 100, 62, 120, 64, 90]);
        assert_eq!(ev.len, 3);
        assert_eq!(
            ev.get(0),
            MidiEvent::NoteOn {
                note: 60,
                velocity: 100
            }
        );
        assert_eq!(
            ev.get(1),
            MidiEvent::NoteOn {
                note: 62,
                velocity: 120
            }
        );
        assert_eq!(
            ev.get(2),
            MidiEvent::NoteOn {
                note: 64,
                velocity: 90
            }
        );
    }

    #[test]
    fn realtime_bytes_do_not_break_running_status() {
        let mut p = MidiParser::new();
        // Clock (0xF8) injected between data bytes.
        let ev = feed(&mut p, &[0x90, 60, 0xF8, 100, 0xF8, 62, 120]);
        assert_eq!(ev.len, 2);
    }

    #[test]
    fn pitch_bend_center_is_zero() {
        let mut p = MidiParser::new();
        let ev = feed(&mut p, &[0xE0, 0x00, 0x40]);
        assert_eq!(ev.len, 1);
        assert_eq!(ev.get(0), MidiEvent::PitchBend { value: 0 });
    }

    #[test]
    fn control_change() {
        let mut p = MidiParser::new();
        let ev = feed(&mut p, &[0xB0, 74, 64]);
        assert_eq!(ev.len, 1);
        assert_eq!(
            ev.get(0),
            MidiEvent::ControlChange {
                controller: 74,
                value: 64
            }
        );
    }

    #[test]
    fn sysex_is_swallowed() {
        let mut p = MidiParser::new();
        let ev = feed(&mut p, &[0xF0, 0x7E, 0x7F, 0x06, 0x01, 0xF7, 0x90, 60, 100]);
        assert_eq!(ev.len, 1);
        assert_eq!(
            ev.get(0),
            MidiEvent::NoteOn {
                note: 60,
                velocity: 100
            }
        );
    }
}
