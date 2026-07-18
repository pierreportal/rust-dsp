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

// use crate::acid::ACCENT_THRESHOLD;
use crate::midi::MidiEvent;
use crate::params::SharedParams;

const CV_BASE_NOTE: f32 = 24.0;
const CV_OCTAVES_PER_MCU_FULLSCALE: f32 = 5.0;
const PITCH_SMOOTH: f32 = 0.25;

#[allow(dead_code)]
const GATE_THRESHOLD_FRAC: f32 = 0.5;

const DEFAULT_VELOCITY: u8 = 96;
const ADC_FS: f32 = 65_535.0;

// Gate & Pitch CV IN
pub type CvGatePin = gpioa::PA3<Input>;
pub type CvPitchPin = gpioc::PC0<Analog>;

// Pot parameters
pub type PotParam1Pin = gpiob::PB1<Analog>;
pub type PotParam2Pin = gpioa::PA7<Analog>;
pub type PotParam3Pin = gpioa::PA6<Analog>;
pub type PotParam4Pin = gpioc::PC1<Analog>;

// Cv parameters
pub type CvParam1Pin = gpioc::PC2<Analog>;

pub struct Controls {
    adc: Adc<ADC1, Enabled>,
    cv_pitch: CvPitchPin,
    pot_param1: PotParam1Pin,
    pot_param2: PotParam2Pin,
    pot_param3: PotParam3Pin,
    pot_param4: PotParam4Pin,
    cv_gate: CvGatePin,
    pitch_smoothed: f32,
    gate_prev: bool,
    held_note: Option<u8>,
}

impl Controls {
    pub fn new(
        adc1: hal::stm32::ADC1,
        prec: hal::rcc::rec::Adc12,
        clocks: &hal::rcc::CoreClocks,
        delay: &mut impl hal::hal::blocking::delay::DelayUs<u8>,
        cv_pitch: CvPitchPin,
        pot_param1: PotParam1Pin,
        pot_param2: PotParam2Pin,
        pot_param3: PotParam3Pin,
        pot_param4: PotParam4Pin,
        cv_gate: CvGatePin,
    ) -> Self {
        let mut adc = Adc::adc1(adc1, 4.MHz(), delay, prec, clocks).enable();
        adc.set_resolution(Resolution::SixteenBit);

        Self {
            adc,
            cv_pitch,
            pot_param1,
            pot_param2,
            pot_param3,
            pot_param4,
            cv_gate,
            pitch_smoothed: 0.0,
            gate_prev: false,
            held_note: None,
        }
    }

    pub fn poll_and_apply<F>(&mut self, params: &SharedParams, mut on_event: F)
    where
        F: FnMut(MidiEvent),
    {
        let value_param1 = read_norm(&mut self.adc, &mut self.pot_param1);
        let value_param2 = read_norm(&mut self.adc, &mut self.pot_param2);
        let value_param3 = read_norm(&mut self.adc, &mut self.pot_param3);
        let value_param4 = read_norm(&mut self.adc, &mut self.pot_param4);

        params.set_cutoff(value_param1);
        params.set_resonance(value_param2);
        params.set_drive(value_param3);
        params.set_decay(value_param4);

        // TODO: extract control out of gpio.rs
        let raw_pitch: u32 = self.adc.read(&mut self.cv_pitch).unwrap();
        self.pitch_smoothed += PITCH_SMOOTH * (raw_pitch as f32 - self.pitch_smoothed);
        let note = cv_to_note(self.pitch_smoothed / ADC_FS);

        let gate_now = self.cv_gate.is_high();
        match (self.gate_prev, gate_now) {
            (false, true) => {
                on_event(MidiEvent::NoteOn {
                    note,
                    velocity: DEFAULT_VELOCITY,
                });
                self.held_note = Some(note);
            }
            (true, false) => {
                if let Some(n) = self.held_note.take() {
                    on_event(MidiEvent::NoteOff { note: n });
                }
            }
            (true, true) => {
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

        // let _ = ACCENT_THRESHOLD;
    }
}

// --------------------------------------------------------------------------

fn read_norm<P>(adc: &mut Adc<ADC1, Enabled>, pin: &mut P) -> f32
where
    P: embedded_hal::adc::Channel<ADC1, ID = u8>,
    Adc<ADC1, Enabled>: embedded_hal::adc::OneShot<ADC1, u32, P>,
{
    let raw: u32 = embedded_hal::adc::OneShot::read(adc, pin).unwrap_or(0);
    (raw as f32 / ADC_FS).clamp(0.0, 1.0)
}

fn cv_to_note(norm: f32) -> u8 {
    let semitones = norm * CV_OCTAVES_PER_MCU_FULLSCALE * 12.0;
    let n = CV_BASE_NOTE + semitones;
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
