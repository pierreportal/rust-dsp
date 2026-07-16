//! Acid-bass firmware for Daisy Pod (rev 7 = Daisy Seed 1.2 core).
//!
//! Signal path
//!
//! ```text
//!   MIDI UART bytes ─► MidiParser ─┐
//!                                  ├─► AcidBass  ─► PCM3060 codec (SAI1 + DMA1)
//!   CV / gate / pots ─► gpio::Controls ─┘  (owned by DMA ISR)
//! ```
//!
//! One control source is active per firmware build. Flip
//! [`CONTROL_SOURCE`] to pick between:
//!
//! * [`ControlSource::TestSequencer`] — internal melody, no external input.
//! * [`ControlSource::Midi`]          — 31.25 kbaud MIDI on USART1 RX.
//! * [`ControlSource::Cv`]            — eurorack CV/gate + potentiometers on ADC1.
//!
//! Both MIDI and CV code paths are built either way; the unused peripheral
//! just isn't initialised, so there's no runtime cost for the unused mode.
//!
//! Flashing (Pod rev 7)
//!
//! ```bash
//! cargo build --release
//! cargo objcopy --release -- -O binary acid-bass.bin
//! # then hold BOOT + tap RESET on the Pod to enter DFU, then:
//! dfu-util -a 0 -s 0x08000000:leave -D acid-bass.bin
//! # or drop the .bin onto https://electro-smith.github.io/Programmer/
//! ```

#![no_std]
#![no_main]

use panic_halt as _;

mod acid;
mod gpio;
mod midi;
mod params;
mod test_seq;

use core::cell::RefCell;
use cortex_m::interrupt::Mutex;
use cortex_m_rt::entry;

use daisy::audio;
use daisy::hal;
use hal::pac::{self, interrupt};
use hal::prelude::*;

use crate::acid::AcidBass;
use crate::gpio::Controls;
use crate::midi::{MidiEvent, MidiParser};
use crate::params::SharedParams;
use crate::test_seq::TestSequencer;

/// The sample rate the daisy BSP configures the PCM3060 to run at.
const SAMPLE_RATE: f32 = 48_000.0;

/// Compile-time selection of where note + parameter data comes from.
/// Only one is active per firmware image — the others are still compiled
/// but their peripherals are not initialised.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ControlSource {
    /// Internal step sequencer. No external hardware needed.
    TestSequencer,
    /// MIDI on USART1 RX. Original synth mode.
    Midi,
    /// Eurorack module mode — 1V/oct pitch CV + gate + four pots.
    Cv,
}

/// Flip this to change how the synth is driven. Rebuild to switch modes.
const CONTROL_SOURCE: ControlSource = ControlSource::Cv;

// ---- Shared state between main and the audio ISR -------------------------

static AUDIO_INTERFACE: Mutex<RefCell<Option<audio::Interface>>> = Mutex::new(RefCell::new(None));

/// The MIDI UART receiver, owned by the audio ISR.
static MIDI_RX: Mutex<RefCell<Option<MidiRx>>> = Mutex::new(RefCell::new(None));

/// The synth engine itself. Also owned by the audio ISR.
static SYNTH: Mutex<RefCell<Option<AcidBass>>> = Mutex::new(RefCell::new(None));

/// Internal melody generator used when [`ControlSource::TestSequencer`] is on.
static TEST_SEQ: Mutex<RefCell<Option<TestSequencer>>> = Mutex::new(RefCell::new(None));

/// CV / gate / potentiometer surface, owned by the ISR when
/// [`ControlSource::Cv`] is active.
static CONTROLS: Mutex<RefCell<Option<Controls>>> = Mutex::new(RefCell::new(None));

/// Parameter surface shared between whichever control source is active and
/// the audio callback. Written by the ISR when a CC or pot changes, read
/// every block when applying to the voice.
static SHARED: SharedParams = SharedParams::new();

/// The concrete UART1 receiver type — spelled out so it can live in a static.
type MidiRx = hal::serial::Rx<pac::USART1>;

// --------------------------------------------------------------------------

#[entry]
fn main() -> ! {
    let mut cp = cortex_m::Peripherals::take().unwrap();
    let dp = pac::Peripherals::take().unwrap();

    // Caches — big audio-perf win. Data cache is safe here because the daisy
    // BSP does its own cache management around the audio DMA buffers.
    cp.SCB.enable_icache();
    cp.SCB.enable_dcache(&mut cp.CPUID);

    let board = daisy::Board::take().unwrap();

    let ccdr = daisy::board_freeze_clocks!(board, dp);
    let pins = daisy::board_split_gpios!(board, ccdr, dp);
    let audio_interface = daisy::board_split_audio!(ccdr, pins);

    // ---- Control-source-specific peripheral bring-up -----------------
    let mut midi_rx: Option<MidiRx> = None;
    let mut controls: Option<Controls> = None;

    match CONTROL_SOURCE {
        ControlSource::Midi => {
            // MIDI UART: USART1 RX on PIN_14 (PB7).
            //
            // Per the Pod rev 7 pinout, the 3.5 mm TRS MIDI IN jack is
            // wired to Seed PIN_14 = PB7 = USART1_RX. We init the
            // peripheral in RX-only mode (`NoTx`) because PIN_13 (PB6,
            // USART1_TX) is claimed by the Pod's encoder click switch —
            // configuring it as UART TX would fight that input. 31 250
            // baud, 8N1 — the MIDI spec.
            use hal::serial::NoTx;
            let rx = pins.GPIO.PIN_14.into_alternate::<7>(); // AF7 = USART1
            let serial = dp
                .USART1
                .serial(
                    (NoTx, rx),
                    31_250.bps(),
                    ccdr.peripheral.USART1,
                    &ccdr.clocks,
                )
                .unwrap();
            let (_tx, mut rx) = serial.split();
            rx.listen();
            midi_rx = Some(rx);
        }
        ControlSource::Cv => {
            // Six analog inputs on ADC1 + one digital gate. See gpio.rs
            // for the pin-to-role map. The ADC calibration path wants a
            // real DelayUs implementation — the SysTick delay is fine.
            let mut delay = hal::delay::Delay::new(cp.SYST, ccdr.clocks);
            let cv_pitch = pins.GPIO.PIN_15.into_analog();
            let gate = pins.GPIO.PIN_16.into_pull_down_input();
            let pot_cutoff = pins.GPIO.PIN_17.into_analog();
            let pot_resonance = pins.GPIO.PIN_18.into_analog();
            let pot_drive = pins.GPIO.PIN_19.into_analog();
            let pot_decay = pins.GPIO.PIN_20.into_analog();
            controls = Some(Controls::new(
                dp.ADC1,
                ccdr.peripheral.ADC12,
                &ccdr.clocks,
                &mut delay,
                cv_pitch,
                pot_cutoff,
                pot_resonance,
                pot_drive,
                pot_decay,
                gate,
            ));
        }
        ControlSource::TestSequencer => {}
    }

    // ---- Move ownership into globals so the ISR can reach it -----------
    let audio_interface = audio_interface.spawn().unwrap();
    let synth = AcidBass::new(SAMPLE_RATE);
    let test_seq = TestSequencer::new(SAMPLE_RATE);
    cortex_m::interrupt::free(|cs| {
        AUDIO_INTERFACE.borrow(cs).replace(Some(audio_interface));
        if let Some(rx) = midi_rx {
            MIDI_RX.borrow(cs).replace(Some(rx));
        }
        if let Some(c) = controls {
            CONTROLS.borrow(cs).replace(Some(c));
        }
        SYNTH.borrow(cs).replace(Some(synth));
        TEST_SEQ.borrow(cs).replace(Some(test_seq));
    });

    // No user code in the main loop — everything happens in the audio ISR.
    // We use `wfi` so the CPU sleeps between interrupts to save power.
    loop {
        cortex_m::asm::wfi();
    }
}

/// Audio DMA interrupt. Called every half-buffer (~333 µs at 48 kHz / 32-frame
/// blocks). We do three things:
///
/// 1. Drain any bytes available on the MIDI UART and feed the parser.
/// 2. Apply the latest CC-driven parameters to the voice.
/// 3. Run the DSP for one block and write it into the codec buffer.
#[interrupt]
fn DMA1_STR1() {
    cortex_m::interrupt::free(|cs| {
        let mut audio_ref = AUDIO_INTERFACE.borrow(cs).borrow_mut();
        let audio_interface = match audio_ref.as_mut() {
            Some(a) => a,
            None => return,
        };

        let mut synth_ref = SYNTH.borrow(cs).borrow_mut();
        let synth = match synth_ref.as_mut() {
            Some(s) => s,
            None => return,
        };

        // --- 1. Ingest events from the active control source ---
        match CONTROL_SOURCE {
            ControlSource::TestSequencer => {
                let mut seq_ref = TEST_SEQ.borrow(cs).borrow_mut();
                if let Some(seq) = seq_ref.as_mut() {
                    // Advance one block worth of samples so the tempo is right.
                    seq.tick(audio::BLOCK_LENGTH, |ev| match ev {
                        MidiEvent::NoteOn { note, velocity } => synth.note_on(note, velocity),
                        MidiEvent::NoteOff { note } => synth.note_off(note),
                        _ => {}
                    });
                }
            }
            ControlSource::Midi => {
                let mut rx_ref = MIDI_RX.borrow(cs).borrow_mut();
                if let Some(rx) = rx_ref.as_mut() {
                    // Kept across ISR invocations so running-status survives.
                    static mut PARSER: MidiParser = MidiParser::new();
                    // SAFETY: only touched inside `cortex_m::interrupt::free`.
                    let parser = unsafe { &mut *core::ptr::addr_of_mut!(PARSER) };

                    // Drain up to 8 bytes per ISR to bound worst-case work.
                    for _ in 0..8 {
                        match rx.read() {
                            Ok(byte) => {
                                if let Some(ev) = parser.push(byte) {
                                    match ev {
                                        MidiEvent::NoteOn { note, velocity } => {
                                            synth.note_on(note, velocity);
                                        }
                                        MidiEvent::NoteOff { note } => {
                                            synth.note_off(note);
                                        }
                                        MidiEvent::ControlChange { .. } => {
                                            SHARED.apply_cc(&ev);
                                        }
                                        MidiEvent::PitchBend { .. } => {}
                                    }
                                }
                            }
                            Err(nb::Error::WouldBlock) => break,
                            Err(nb::Error::Other(_)) => break, // framing / overrun — drop
                        }
                    }
                }
            }
            ControlSource::Cv => {
                let mut ctl_ref = CONTROLS.borrow(cs).borrow_mut();
                if let Some(ctl) = ctl_ref.as_mut() {
                    ctl.poll_and_apply(&SHARED, |ev| match ev {
                        MidiEvent::NoteOn { note, velocity } => synth.note_on(note, velocity),
                        MidiEvent::NoteOff { note } => synth.note_off(note),
                        _ => {}
                    });
                }
            }
        }

        // --- 2. Apply shared params ---
        //
        // NOTE: cutoff (CC 74) is captured into `SHARED` but not yet applied,
        // because `Voice::self_update` overwrites the cutoff-smoother target
        // every sample from its internal LFO. Wiring cutoff bias needs a
        // small change to `dsp::voice::Voice` (a `cutoff_bias` field added to
        // its formula). See README for the plan.
        synth.voice.env.decay = SHARED.decay();
        synth.voice.env.release = SHARED.decay().min(0.6);
        synth.voice.distortion.drive = SHARED.drive();
        synth.voice.filter.set_resonance(SHARED.resonance());
        let _ = SHARED.cutoff(); // silence dead-code warning until wired up

        // --- 3. DSP block ---
        audio_interface
            .handle_interrupt_dma1_str1(|block| {
                for frame in block.iter_mut() {
                    let s = synth.next_sample();
                    *frame = (s, s);
                }
            })
            .ok();
    });
}
