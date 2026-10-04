//! The window: patch loading, per-module knobs, MIDI status and a playable
//! on-screen keyboard.
//!
//! The UI never touches the engine. It edits a [`Patch`] and sends control
//! messages, because the audio thread owns the only live voice pool and must not
//! wait on a window.

use crate::audio::{note_name, Engine};
use crate::midi;
use crate::plumbing::{Control, ControlMsg, Event, EventQueue, Status};
use egui::{Color32, RichText};
use graph::registry;
use graph::Kind;
use patch::{self, Patch};
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// Two octaves, starting at middle C: enough to play a chord and hear the
/// polyphony without needing hardware attached.
const FIRST_KEY: u8 = 60;
const KEY_COUNT: u8 = 25;

/// Which computer key plays which note, so the app is playable with no MIDI
/// device. Laid out like a two-row piano.
fn computer_key(note: u8) -> Option<char> {
    // 25 notes from middle C: a 12-note row plus a 13-note row.
    const ROW1: [char; 12] = ['a', 'w', 's', 'e', 'd', 'f', 't', 'g', 'y', 'h', 'u', 'j'];
    const ROW2: [char; 13] = [
        'k', 'o', 'l', 'p', '\'', 'z', 'm', ',', '.', '/', 'q', '1', '2',
    ];
    let i = (note - FIRST_KEY) as usize;
    match i {
        0..=11 => ROW1.get(i).copied(),
        _ => ROW2.get(i - 12).copied(),
    }
}

pub struct CoarseApp {
    engine: Arc<Engine>,
    events: Arc<EventQueue>,
    control: Arc<Control>,
    status: Arc<Status>,

    /// The patch as the user last set it. The audio thread has its own copy of
    /// the *voice pool*; this is the editable document.
    patch: Patch,
    warnings: Vec<String>,
    link_input: String,
    /// Set when a patch was loaded from the command line, to show it once.
    notice: Option<String>,
    midi_inputs: Vec<String>,
    midi_index: Option<usize>,
    midi_status: String,
    /// Notes switched on by the computer keyboard, tracked so a key release can
    /// turn them off.
    keyboard_held: Vec<u8>,
    /// Notes switched on by clicking a key. A click is momentary, so these are
    /// released again on the following frame.
    mouse_held: Vec<u8>,
    /// Live MIDI connection. Dropping it stops input, so it lives in the app.
    _midi: Option<midi::MidiIn>,
    last_seen_generation: u32,
}

impl CoarseApp {
    pub fn new(
        engine: Arc<Engine>,
        initial: Patch,
        warnings: Vec<String>,
        link: String,
        notice: Option<String>,
    ) -> Self {
        let events = Arc::clone(engine.events());
        let control = Arc::clone(engine.control());
        let status = Arc::clone(engine.status());
        let last_seen_generation = status.patch_generation.load(Ordering::Acquire);
        let midi_inputs = midi::available_inputs();
        let midi_status = if midi_inputs.is_empty() {
            "no MIDI devices found".to_string()
        } else {
            format!("{} device(s) available", midi_inputs.len())
        };
        Self {
            engine,
            events,
            control,
            status,
            patch: initial,
            warnings,
            link_input: link,
            notice,
            midi_inputs,
            midi_index: None,
            midi_status,
            keyboard_held: Vec::new(),
            mouse_held: Vec::new(),
            _midi: None,
            last_seen_generation,
        }
    }

    fn load(&mut self, code: &str) {
        let Some(code) = patch::code_from_link(code) else {
            self.notice = Some("that does not look like a patch link".to_string());
            return;
        };
        let decoded = patch::decode(code);
        if decoded.patch.nodes.is_empty() {
            self.notice = Some("nothing playable in that link".to_string());
            self.warnings = decoded.warnings;
            return;
        }
        self.warnings = decoded.warnings;
        self.patch = decoded.patch;
        self.link_input = patch::encode(&self.patch);
        self.notice = Some(format!(
            "loaded {} module(s) — paste a `#p=` link into the box above",
            self.patch.nodes.len()
        ));
        // Release anything the previous patch was holding, or a load mid-chord
        // leaves notes droning in the new one.
        for note in self.keyboard_held.drain(..) {
            self.events.push(Event::NoteOff { note });
        }
        self.control
            .push(ControlMsg::LoadPatch(Box::new(self.patch.clone())));
    }

    fn reset_to_default(&mut self) {
        self.warnings.clear();
        self.patch = patch::default_patch();
        self.link_input = patch::encode(&self.patch);
        self.notice = Some("reset to the default patch".to_string());
        self.control
            .push(ControlMsg::LoadPatch(Box::new(self.patch.clone())));
    }

    fn connect_midi(&mut self, index: usize) {
        match midi::connect(index, Arc::clone(&self.events)) {
            Ok(connection) => {
                // Held in the app so the connection lives as long as the window.
                self._midi = Some(connection);
                self.midi_index = Some(index);
                let name = self
                    .midi_inputs
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| format!("input {index}"));
                self.midi_status = format!("connected: {name}");
            }
            Err(e) => self.midi_status = e,
        }
    }

    fn keyboard_note(&mut self, note: u8, on: bool) {
        if on {
            if !self.keyboard_held.contains(&note) {
                self.keyboard_held.push(note);
            }
            self.events.push(Event::NoteOn { note, vel: 110 });
        } else {
            self.keyboard_held.retain(|&n| n != note);
            self.events.push(Event::NoteOff { note });
        }
    }
}

// `_midi` is declared after the impl so the field list above stays readable.
impl CoarseApp {
    fn ui_top(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Coarse");
            ui.label(
                RichText::new(format!("{:.0} Hz", self.engine.sample_rate()))
                    .small()
                    .weak(),
            );
            if let Some(note) = self.notice.clone() {
                ui.label(RichText::new(note).small().color(Color32::LIGHT_BLUE));
                self.notice = None;
            }
        });

        ui.horizontal(|ui| {
            ui.label("Link:");
            ui.add(
                egui::TextEdit::singleline(&mut self.link_input)
                    .hint_text("#p=… or a full share url")
                    .desired_width(360.0),
            );
            if ui.button("Load").clicked() {
                let link = self.link_input.clone();
                self.load(&link);
            }
            if ui.button("Reset").clicked() {
                self.reset_to_default();
            }
            if ui.button("Copy share code").clicked() {
                ui.ctx().copy_text(self.link_input.clone());
                self.notice = Some("share code copied".to_string());
            }
        });

        if !self.warnings.is_empty() {
            ui.colored_label(
                Color32::from_rgb(220, 160, 60),
                format!("{} repair(s) applied while loading", self.warnings.len()),
            );
            for warning in self.warnings.clone() {
                ui.small(warning);
            }
        }
    }

    fn ui_midi(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("MIDI:");
            if self.midi_inputs.is_empty() {
                ui.label(RichText::new(self.midi_status.clone()).weak());
            } else {
                egui::ComboBox::from_label("").show_ui(ui, |ui| {
                    for (i, name) in self.midi_inputs.clone().into_iter().enumerate() {
                        let selected = self.midi_index == Some(i);
                        if ui.selectable_label(selected, name).clicked() {
                            self.connect_midi(i);
                        }
                    }
                });
            }
            if let Some(cc) = self.status.last_cc() {
                ui.label(format!("last CC {cc}"));
            }
        });
    }

    fn ui_keyboard(&mut self, ui: &mut egui::Ui) {
        use egui::Key;

        // Release anything clicked last frame: a click is momentary, so there is
        // no release event to wait for.
        for note in self.mouse_held.drain(..) {
            self.events.push(Event::NoteOff { note });
        }

        // Latching keys: egui's keyboard events are edge-triggered, so a held
        // computer key must be re-asserted to sustain the note.
        let pressed: Vec<Key> = ui
            .input(|i| i.events.clone())
            .into_iter()
            .filter_map(|e| match e {
                egui::Event::Key {
                    key, pressed: true, ..
                } => Some(key),
                _ => None,
            })
            .collect();

        for note in FIRST_KEY..FIRST_KEY + KEY_COUNT {
            let wanted = computer_key(note)
                .map(|c| {
                    pressed
                        .iter()
                        .any(|k| key_char(*k).is_some_and(|kc| kc.eq_ignore_ascii_case(&c)))
                })
                .unwrap_or(false);
            let held = self.keyboard_held.contains(&note);
            if wanted && !held {
                self.keyboard_note(note, true);
            } else if !wanted && held {
                self.keyboard_note(note, false);
            }
        }

        ui.horizontal(|ui| {
            for note in FIRST_KEY..FIRST_KEY + KEY_COUNT {
                let held = self.status.is_held(note);
                let name = computer_key(note)
                    .map(|c| c.to_string())
                    .unwrap_or_default();
                let label = format!(
                    "{}\n{}",
                    if name.is_empty() { " " } else { &name },
                    note_name(note)
                );
                let text = RichText::new(label).small();
                let button = egui::Button::new(if held {
                    text.color(Color32::BLACK)
                } else {
                    text
                })
                .fill(if held {
                    Color32::from_rgb(120, 200, 255)
                } else {
                    Color32::from_rgb(60, 60, 70)
                })
                .min_size(egui::vec2(38.0, 44.0));
                let response = ui.add(button);
                if response.clicked() && !held {
                    self.events.push(Event::NoteOn { note, vel: 110 });
                    self.mouse_held.push(note);
                    // A click has no "release" event, so the note is turned off
                    // again at the top of the next frame.
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(120));
                }
            }
        });
        ui.label(
            RichText::new("Play with the computer keys above, or plug in a controller.")
                .small()
                .weak(),
        );
    }

    fn ui_modules(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("Patch");
            ui.label(
                RichText::new(format!(
                    "{} module(s), {} cable(s)",
                    self.patch.nodes.len(),
                    self.patch.edges.len()
                ))
                .small()
                .weak(),
            );
        });

        let mut nodes = self.patch.nodes.clone();
        nodes.sort_by_key(|n| n.id);
        for node in nodes {
            let Some(kind) = Kind::from_u8(node.kind) else {
                continue;
            };
            ui.horizontal(|ui| {
                ui.colored_label(
                    egui::Color32::from_hex(registry::color(kind)).unwrap_or(Color32::GRAY),
                    format!("{} #{}", registry::label(kind), node.id),
                );
                for spec in registry::params(kind) {
                    let current = node.params.get(spec.name).copied().unwrap_or(spec.default);
                    let mut value = current as f64;
                    ui.add(
                        egui::Slider::new(&mut value, spec.min as f64..=spec.max as f64)
                            .text(spec.label)
                            .step_by(spec.step as f64),
                    );
                    let value = value as f32;
                    if value != current {
                        // Optimistically update the document so the knob does not
                        // snap back, and tell the engine.
                        if let Some(p) = self.patch.nodes.iter_mut().find(|n| n.id == node.id) {
                            p.params.insert(spec.name.to_string(), value);
                        }
                        self.control.push(ControlMsg::SetParam {
                            id: node.id,
                            name: spec.name.to_string(),
                            value,
                        });
                    }
                }
            });
        }

        // The palette: what could be added next. Adding a module needs an id and
        // a cable to be useful, which is the web editor's job, so this is a
        // readout of the shared catalogue rather than a half-working editor.
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Available:").small().weak());
            for kind in registry::palette_order() {
                ui.colored_label(
                    egui::Color32::from_hex(registry::color(*kind)).unwrap_or(Color32::GRAY),
                    RichText::new(registry::label(*kind)).small(),
                );
            }
        });
    }
}

/// Whether a layout character can actually be produced by a key press, i.e.
/// whether [`key_char`] has a matching arm.
///
/// The layout uses a few punctuation keys (`'`, `,`, `.`, `/`) to fit 25 notes
/// onto the keyboard, so this cannot simply check for a letter.
#[cfg(test)]
fn key_char_name(target: char) -> Option<char> {
    ALL_KEYS
        .iter()
        .find_map(|k| key_char(*k).filter(|c| *c == target))
}

/// Every egui key the layout could use, for reachability checks.
#[cfg(test)]
const ALL_KEYS: &[egui::Key] = &[
    egui::Key::Num0,
    egui::Key::Num1,
    egui::Key::Num2,
    egui::Key::Num3,
    egui::Key::Num4,
    egui::Key::Num5,
    egui::Key::Num6,
    egui::Key::Num7,
    egui::Key::Num8,
    egui::Key::Num9,
    egui::Key::A,
    egui::Key::B,
    egui::Key::C,
    egui::Key::D,
    egui::Key::E,
    egui::Key::F,
    egui::Key::G,
    egui::Key::H,
    egui::Key::I,
    egui::Key::J,
    egui::Key::K,
    egui::Key::L,
    egui::Key::M,
    egui::Key::N,
    egui::Key::O,
    egui::Key::P,
    egui::Key::Q,
    egui::Key::R,
    egui::Key::S,
    egui::Key::T,
    egui::Key::U,
    egui::Key::V,
    egui::Key::W,
    egui::Key::X,
    egui::Key::Y,
    egui::Key::Z,
    egui::Key::Quote,
    egui::Key::Comma,
    egui::Key::Period,
    egui::Key::Slash,
    egui::Key::Semicolon,
];

/// egui key to its character, for matching the two-row typing layout.
fn key_char(key: egui::Key) -> Option<char> {
    use egui::Key::*;
    Some(match key {
        // egui calls the digits `Num0`..`Num9`.
        Num0 => '0',
        Num1 => '1',
        Num2 => '2',
        Num3 => '3',
        Num4 => '4',
        Num5 => '5',
        Num6 => '6',
        Num7 => '7',
        Num8 => '8',
        Num9 => '9',
        Quote => '\'',
        Comma => ',',
        Period => '.',
        Slash => '/',
        Semicolon => ';',
        A => 'a',
        B => 'b',
        C => 'c',
        D => 'd',
        E => 'e',
        F => 'f',
        G => 'g',
        H => 'h',
        I => 'i',
        J => 'j',
        K => 'k',
        L => 'l',
        M => 'm',
        N => 'n',
        O => 'o',
        P => 'p',
        Q => 'q',
        R => 'r',
        S => 's',
        T => 't',
        U => 'u',
        V => 'v',
        W => 'w',
        X => 'x',
        Y => 'y',
        Z => 'z',
        _ => return None,
    })
}

impl eframe::App for CoarseApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // A patch can also arrive from the command line after the window is up.
        let generation = self.status.patch_generation.load(Ordering::Acquire);
        if generation != self.last_seen_generation {
            self.last_seen_generation = generation;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            self.ui_top(ui);
            ui.add_space(4.0);
            self.ui_midi(ui);
            ui.add_space(4.0);
            self.ui_modules(ui);
            ui.add_space(8.0);
            self.ui_keyboard(ui);
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                let peak = self.status.peak();
                ui.label(RichText::new("level").small().weak());
                let _ = egui::ProgressBar::new(peak).desired_width(160.0);
                ui.label(
                    RichText::new(format!(
                        "{} voice(s) held{}",
                        self.status.held_count(),
                        match self.engine.dropped_events() {
                            0 => String::new(),
                            n => format!("  — {n} event(s) dropped, audio too busy"),
                        }
                    ))
                    .small(),
                );
            });
        });

        // Repaint continuously: held notes and the meter change without input.
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_computer_key_layout_covers_every_offered_note() {
        let mut seen = std::collections::HashSet::new();
        for note in FIRST_KEY..FIRST_KEY + KEY_COUNT {
            let key = computer_key(note).unwrap_or_else(|| panic!("note {note} has no key"));
            assert!(seen.insert(key), "key {key} is mapped twice");
            // Punctuation is fine as long as `key_char` can produce it.
            assert!(key_char_name(key).is_some(), "{key} is not reachable");
        }
    }

    #[test]
    fn the_key_rows_have_no_duplicates_within_themselves() {
        // The two-row piano layout is easy to get subtly wrong, and a clash
        // would make one note impossible to play.
        let mut all = Vec::new();
        for note in FIRST_KEY..FIRST_KEY + KEY_COUNT {
            if let Some(k) = computer_key(note) {
                all.push(k);
            }
        }
        let unique: std::collections::HashSet<_> = all.iter().collect();
        assert_eq!(unique.len(), all.len(), "duplicate keys: {all:?}");
    }
}
