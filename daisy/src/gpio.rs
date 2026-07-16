//! CV / gate / potentiometer control surface — the eurorack front panel.
//!
//! Six ADC1 channels + one digital pin drive the synth when the firmware is
//! built in [`ControlSource::Cv`](crate::ControlSource::Cv) mode:
//!
//! | Pod pin | STM pin | ADC1 ch | Role                                     |
//! | ------- | ------- | ------- | ---------------------------------------- |
//! | PIN_15  | PC0     | 10      | CV IN — 1V/oct pitch                     |
//! | PIN_16  | PA3     | —       | GATE IN (digital, pull-down)             |
//! | PIN_17  | PB1     | 5       | POT — filter cutoff                      |
//! | PIN_18  | PA7     | 7       | POT — resonance                          |
//! | PIN_19  | PA6     | 3       | POT — distortion drive                   |
//! | PIN_20  | PC1     | 11      | POT — envelope decay                     |
//!
//! ## Front-panel signal conditioning
//!
//! The MCU tolerates 0–3.3 V only. Anything from a eurorack bus (±5 V or
//! ±10 V CV, 0/+5 V gates, ±5 V audio) has to be attenuated / level-shifted
//! by op-amp circuitry on the module PCB before it reaches these pins.
//! The [`CV_BASE_NOTE`] / [`CV_VOLTS_PER_MCU_FULLSCALE`] constants below
//! model that scaling — tune them to whatever your front-end delivers.
//!
//! ## Rate
//!
//! `poll_and_apply` runs from the audio DMA ISR every ~333 µs (32-sample
//! block at 48 kHz). One blocking `OneShot` conversion per channel costs
//! ~8 µs at f_adc = 4 MHz with 32.5-cycle sample time → ~48 µs for all six
//! channels — well inside the budget. If you need more headroom, switch to
//! round-robin (one channel per ISR).

use daisy::hal;
use hal::adc::{Adc, Enabled, Resolution};
use hal::gpio::{gpioa, gpiob, gpioc, Analog, Input};
use hal::pac::ADC1;
use hal::prelude::*;

use crate::acid::ACCENT_THRESHOLD;
use crate::midi::MidiEvent;
use crate::params::SharedParams;

// ---- Front-panel scaling constants ---------------------------------------

/// MIDI note produced when the pitch-CV input is at 0 V (ADC reads 0).
/// C1 = 24. Adjust to whatever your front-end pins as the low end.
const CV_BASE_NOTE: f32 = 24.0;

/// How many octaves the full ADC range (0..=0xFFFF at 16-bit) spans.
/// With an op-amp scaler mapping ±5 V eurorack CV into 0..3.3 V on the MCU
/// pin, a common choice is 5 octaves across the full ADC range.
const CV_OCTAVES_PER_MCU_FULLSCALE: f32 = 5.0;

/// One-pole smoothing coefficient applied to the pitch CV each block, to
/// hide the ~few-LSB jitter on the ADC without introducing audible glide.
/// (Actual musical portamento is handled by `AcidBass::note_on`.)
const PITCH_SMOOTH: f32 = 0.25;

/// Gate threshold expressed as a fraction of ADC full-scale (unused when
/// the gate is wired as a pure digital input, kept here for reference in
/// case you want to switch to sampled-analog gate detection).
#[allow(dead_code)]
const GATE_THRESHOLD_FRAC: f32 = 0.5;

/// Default velocity for CV-triggered notes — accent is provided by a
/// dedicated CV/gate line in richer designs; here we treat every gate rise
/// as a normal-velocity hit unless you want to expose a switch/CV that
/// pushes velocity above [`ACCENT_THRESHOLD`].
const DEFAULT_VELOCITY: u8 = 96;

/// ADC full-scale value at 16-bit resolution.
const ADC_FS: f32 = 65_535.0;

// ---- Pin type aliases (matches BSP `Analog` state at split) ---------------

pub type CvPitchPin = gpioc::PC0<Analog>;
pub type PotCutoffPin = gpiob::PB1<Analog>;
pub type PotResonancePin = gpioa::PA7<Analog>;
pub type PotDrivePin = gpioa::PA6<Analog>;
pub type PotDecayPin = gpioc::PC1<Analog>;
pub type GatePin = gpioa::PA3<Input>;

// ---- Controls -------------------------------------------------------------

pub struct Controls {
    adc: Adc<ADC1, Enabled>,

    cv_pitch: CvPitchPin,
    pot_cutoff: PotCutoffPin,
    pot_resonance: PotResonancePin,
    pot_drive: PotDrivePin,
    pot_decay: PotDecayPin,
    gate: GatePin,

    /// Smoothed pitch CV, in ADC counts (0..=ADC_FS).
    pitch_smoothed: f32,
    /// Gate level captured on the previous poll — for rising/falling edges.
    gate_prev: bool,
    /// Note currently held (captured on the last rising edge) so we release
    /// the matching note on the falling edge.
    held_note: Option<u8>,
}

impl Controls {
    /// Consumes the ADC12 register block + peripheral rec, boots ADC1, and
    /// re-typestates the pins for use as ADC/GPIO inputs. `delay` is used
    /// only during ADC power-up calibration.
    pub fn new(
        adc1: hal::stm32::ADC1,
        prec: hal::rcc::rec::Adc12,
        clocks: &hal::rcc::CoreClocks,
        delay: &mut impl hal::hal::blocking::delay::DelayUs<u8>,
        cv_pitch: CvPitchPin,
        pot_cutoff: PotCutoffPin,
        pot_resonance: PotResonancePin,
        pot_drive: PotDrivePin,
        pot_decay: PotDecayPin,
        gate: GatePin,
    ) -> Self {
        let mut adc = Adc::adc1(adc1, 4.MHz(), delay, prec, clocks).enable();
        adc.set_resolution(Resolution::SixteenBit);

        Self {
            adc,
            cv_pitch,
            pot_cutoff,
            pot_resonance,
            pot_drive,
            pot_decay,
            gate,
            pitch_smoothed: 0.0,
            gate_prev: false,
            held_note: None,
        }
    }

    /// Sample every input once, write pot values into `SharedParams`, and
    /// emit note-on / note-off through `on_event` for gate edges.
    pub fn poll_and_apply<F>(&mut self, params: &SharedParams, mut on_event: F)
    where
        F: FnMut(MidiEvent),
    {
        // --- Pots → filter/env parameters -----------------------------
        let cutoff = read_norm(&mut self.adc, &mut self.pot_cutoff);
        let resonance = read_norm(&mut self.adc, &mut self.pot_resonance);
        let drive = read_norm(&mut self.adc, &mut self.pot_drive);
        let decay = read_norm(&mut self.adc, &mut self.pot_decay);

        // Same mapping as `SharedParams::apply_cc` — keeps the CV path and
        // the MIDI path producing identical param ranges.
        params.set_cutoff(cutoff);
        params.set_resonance(resonance);
        params.set_drive(1.0 + drive * 19.0);
        params.set_decay(0.02 + decay * 1.48);

        // --- Pitch CV → MIDI note (with light smoothing) ---------------
        let raw_pitch: u32 = self.adc.read(&mut self.cv_pitch).unwrap();
        self.pitch_smoothed += PITCH_SMOOTH * (raw_pitch as f32 - self.pitch_smoothed);
        let note = cv_to_note(self.pitch_smoothed / ADC_FS);

        // --- Gate edges → note events ---------------------------------
        let gate_now = self.gate.is_high();
        match (self.gate_prev, gate_now) {
            (false, true) => {
                // Rising edge — capture the note now (so pitch is locked
                // to the sample of gate onset, avoiding a wobbly attack if
                // the CV is slewing) and fire note-on.
                on_event(MidiEvent::NoteOn {
                    note,
                    velocity: DEFAULT_VELOCITY,
                });
                self.held_note = Some(note);
            }
            (true, false) => {
                // Falling edge — release whatever note we were holding.
                if let Some(n) = self.held_note.take() {
                    on_event(MidiEvent::NoteOff { note: n });
                }
            }
            (true, true) => {
                // Gate stays high — if CV has moved to a new note, retrig
                // as legato (AcidBass turns this into a slide because the
                // previous note is still on).
                if let Some(prev) = self.held_note {
                    if note != prev {
                        on_event(MidiEvent::NoteOn {
                            note,
                            velocity: DEFAULT_VELOCITY,
                        });
                        self.held_note = Some(note);
                    }
                }
            }
            (false, false) => {}
        }
        self.gate_prev = gate_now;

        // Silence the accent-threshold unused warning when no CV-driven
        // accent input is wired — reference the constant so future edits
        // can compute velocity from a separate CV in the DEFAULT_VELOCITY
        // spot above.
        let _ = ACCENT_THRESHOLD;
    }
}

// --------------------------------------------------------------------------

/// Read a channel and normalise to 0.0..=1.0 (ADC full-scale = 1.0).
fn read_norm<P>(adc: &mut Adc<ADC1, Enabled>, pin: &mut P) -> f32
where
    P: embedded_hal::adc::Channel<ADC1, ID = u8>,
    Adc<ADC1, Enabled>: embedded_hal::adc::OneShot<ADC1, u32, P>,
{
    // OneShot::read is infallible on this HAL (returns `nb::Result<_, ()>`
    // that never actually errors); unwrap is safe.
    let raw: u32 =
        embedded_hal::adc::OneShot::read(adc, pin).unwrap_or(0);
    (raw as f32 / ADC_FS).clamp(0.0, 1.0)
}

/// Turn a normalised ADC reading (0.0..=1.0) into a MIDI note number.
///
/// Assumes the front-end op-amp maps CV → 0..3.3 V such that the ADC's
/// full range corresponds to [`CV_OCTAVES_PER_MCU_FULLSCALE`] octaves of
/// musical pitch, with 0 V mapping to [`CV_BASE_NOTE`].
fn cv_to_note(norm: f32) -> u8 {
    let semitones = norm * CV_OCTAVES_PER_MCU_FULLSCALE * 12.0;
    let n = CV_BASE_NOTE + semitones;
    // Clamp so we never index outside MIDI's 0..=127 range.
    let clamped = n.clamp(0.0, 127.0);
    clamped as u8
}

// `embedded-hal` traits are pulled in transitively via stm32h7xx-hal. Re-use
// its re-export so we don't have to add a direct dep.
use hal::hal as embedded_hal;

#[cfg(test)]
mod tests {
    // No `#[cfg(target_arch)]` gate needed — the pure helpers below don't
    // touch any HAL types, so they build on host too.
    use super::cv_to_note;
    use super::{CV_BASE_NOTE, CV_OCTAVES_PER_MCU_FULLSCALE};

    #[test]
    fn zero_cv_is_base_note() {
        assert_eq!(cv_to_note(0.0), CV_BASE_NOTE as u8);
    }

    #[test]
    fn full_scale_is_five_octaves_up() {
        let expected = CV_BASE_NOTE as u8 + (CV_OCTAVES_PER_MCU_FULLSCALE as u8) * 12;
        assert_eq!(cv_to_note(1.0), expected);
    }

    #[test]
    fn one_octave_is_twelve_semitones() {
        let a = cv_to_note(0.0);
        let b = cv_to_note(1.0 / CV_OCTAVES_PER_MCU_FULLSCALE);
        assert_eq!(b - a, 12);
    }
}
