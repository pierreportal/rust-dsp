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
// mod test_seq;

use core::cell::RefCell;
use cortex_m::interrupt::Mutex;
use cortex_m_rt::entry;

use daisy::audio;
use daisy::hal;
use hal::pac::{self, interrupt};
use hal::prelude::*;

use crate::acid::AcidBass;
use crate::gpio::Controls;
use crate::midi::{MidiEvent};
use crate::params::SharedParams;
// use crate::test_seq::TestSequencer;

const SAMPLE_RATE: f32 = 48_000.0;

static AUDIO_INTERFACE: Mutex<RefCell<Option<audio::Interface>>> = Mutex::new(RefCell::new(None));
static SYNTH: Mutex<RefCell<Option<AcidBass>>> = Mutex::new(RefCell::new(None));
static CONTROLS: Mutex<RefCell<Option<Controls>>> = Mutex::new(RefCell::new(None));
static SHARED: SharedParams = SharedParams::new();
// --------------------------------------------------------------------------

#[entry]
fn main() -> ! {
    let mut cp = cortex_m::Peripherals::take().unwrap();
    let dp = pac::Peripherals::take().unwrap();

    cp.SCB.enable_icache();
    cp.SCB.enable_dcache(&mut cp.CPUID);

    let board = daisy::Board::take().unwrap();
    let ccdr = daisy::board_freeze_clocks!(board, dp);
    let pins = daisy::board_split_gpios!(board, ccdr, dp);
    let audio_interface = daisy::board_split_audio!(ccdr, pins);

    let mut controls: Option<Controls> = None;

    let mut delay = hal::delay::Delay::new(cp.SYST, ccdr.clocks);
    let cv_pitch = pins.GPIO.PIN_15.into_analog();
    let cv_gate = pins.GPIO.PIN_16.into_pull_down_input();
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
        cv_gate,
    ));

    // ---- Move ownership into globals so the ISR can reach it -----------
    let audio_interface = audio_interface.spawn().unwrap();
    let synth = AcidBass::new(SAMPLE_RATE);
    cortex_m::interrupt::free(|cs| {
        AUDIO_INTERFACE.borrow(cs).replace(Some(audio_interface));
        if let Some(c) = controls {
            CONTROLS.borrow(cs).replace(Some(c));
        }
        SYNTH.borrow(cs).replace(Some(synth));
    });

    loop {
        cortex_m::asm::wfi();
    }
}

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

        let mut ctl_ref = CONTROLS.borrow(cs).borrow_mut();
        if let Some(ctl) = ctl_ref.as_mut() {
            ctl.poll_and_apply(&SHARED, |ev| match ev {
                MidiEvent::NoteOn { note, velocity } => synth.note_on(note, velocity),
                MidiEvent::NoteOff { note } => synth.note_off(note),
                _ => {}
            });
        }
        synth.voice.env.decay = SHARED.decay();
        synth.voice.env.release = SHARED.decay().min(0.6);
        synth.voice.distortion.drive = SHARED.drive();
        synth.voice.filter.set_resonance(SHARED.resonance());
        let _ = SHARED.cutoff();

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
