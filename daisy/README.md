# acid-bass — Daisy Pod (rev 7) firmware

A MIDI-controlled 303-style acid bass instrument built on the workspace's
`dsp` crate. Targets the Daisy Pod rev 7 and is designed to be flashed over
USB using the Daisy Web Programmer, `dfu-util`, or `cargo embed`.

## What it does

- One monophonic voice: PolyBLEP saw → SVF LP (LFO-modulated) → soft-clip
  distortion → ADSR.
- 303 voicing: fast attack, no sustain, resonant filter, drive.
- **Slide** (legato portamento) when a new note starts while the previous
  one is still held.
- **Accent** when MIDI velocity ≥ 100 — boosts drive for that note.
- CC-mapped parameters (see the MIDI map below).

## Prerequisites

```bash
rustup target add thumbv7em-none-eabihf
cargo install cargo-binutils
rustup component add llvm-tools-preview

# For flashing over USB DFU:
brew install dfu-util
# Or, for probe-rs (ST-Link / DAP-Link):
cargo install probe-rs --features cli
```

## Build

From the workspace root:

```bash
cargo build --release -p acid-bass
```

Produces `target/thumbv7em-none-eabihf/release/acid-bass`.

## Flash over USB (Pod rev 7)

1. Hold **BOOT** on the Daisy, tap **RESET**, release **BOOT** — the board
   enumerates as a DFU device.
2. Convert to a raw binary and flash:

```bash
cargo objcopy --release -p acid-bass -- -O binary acid-bass.bin
dfu-util -a 0 -s 0x08000000:leave -D acid-bass.bin
```

Alternatively, drop `acid-bass.bin` onto the
[Daisy Web Programmer](https://electro-smith.github.io/Programmer/).

## MIDI map

| Message              | Effect                                    |
| -------------------- | ----------------------------------------- |
| Note On (vel < 100)  | Play note                                 |
| Note On (vel ≥ 100)  | Play note **with accent**                 |
| Note On while held   | Slide (legato) — no envelope retrigger    |
| Note Off             | Release envelope                          |
| CC 74                | Filter cutoff bias                        |
| CC 71                | Resonance                                 |
| CC 75                | Distortion drive                          |
| CC 72                | Decay / release time                      |
| Pitch bend           | Wired but not yet applied to `osc.freq`   |

## Architecture

```text
  MIDI UART bytes ──► MidiParser ──┐
                                   ├─► lock-free SPSC queue ─┐
  CCs ──► SharedParams (atomic) ───┘                          ▼
                                                       audio callback
                                                              │
                                                    AcidBass::next_sample
                                                              │
                                                     Codec DMA (stereo)
```

- `src/acid.rs` — the `AcidBass` preset around `dsp::voice::Voice`.
- `src/midi.rs` — streaming, running-status MIDI parser (no allocation).
- `src/params.rs` — atomic parameter surface shared between threads.
- `src/main.rs` — board init, audio-callback spawn, foreground loop.

## Hardware notes

Verified against the **Daisy Pod rev 7 databrief (v1.1, MAY/14/2026)**:

- Pod rev 7 uses a **Daisy Seed 1.2** — PCM3060 codec via I2C2, audio via
  SAI1 + DMA1. `Cargo.toml` uses `daisy = { features = ["seed_1_2"] }`
  accordingly.
- MIDI IN (3.5 mm TRS jack) → **Seed D14 = PB7 = USART1 RX**. Initialized
  RX-only (`NoTx`) because Seed D13 (PB6 / USART1 TX) is claimed by the
  Pod's encoder click switch (ENC_CLICK).
- Memory layout comes from the `daisy` crate's own `memory.x` — no custom
  linker script needed.
- Sample rate is 48 kHz (BSP default). Enable the `sampling_rate_96khz`
  feature on the `daisy` dep if you want 96 kHz.

### Pod-specific pins reference

| Pod control       | Seed pin      | STM32 pin | Notes                        |
| ----------------- | ------------- | --------- | ---------------------------- |
| MIDI IN           | D14           | PB7       | USART1 RX (used)             |
| POT 1             | D21 / A6      | PA7       | ADC 3                        |
| POT 2             | D15 / A0      | PA0       | ADC 10                       |
| SW 1              | D27           | PC4       | tactile, pull-up             |
| SW 2              | D28           | PA2       | tactile, pull-up             |
| ENC A             | D26           | PA2 (see note) | rotary encoder A       |
| ENC B             | D25           | PA0       | rotary encoder B             |
| ENC CLICK         | D13           | PB6       | encoder push — do NOT reuse  |
| LED 1 R/G/B       | D20 / D19 / D18 | -       | RGB LED 1                    |
| LED 2 R/G/B       | D17 / D24 / D23 | -       | RGB LED 2                    |

(STM32 pins from the daisy 0.10 pins.rs — cross-check against the Pod rev
7 pinout diagram before wiring anything unusual.)

## What is NOT wired yet

I built this without hardware in the loop; the DSP core, MIDI parser, and
board bring-up all compile clean against the pinned `daisy 0.10` +
`stm32h7xx-hal 0.16` versions, but a few things still need real-hardware
verification and one small `dsp` change to feel "finished":

- **CC 74 (cutoff bias)** — captured into `SharedParams` but not yet
  applied. `dsp::voice::Voice::self_update` currently overwrites the
  filter cutoff every sample from the internal LFO, which would clobber
  any bias we set from the outside. To wire this properly, add a
  `pub cutoff_bias: f32` field to `Voice` and mix it into the cutoff
  formula in `self_update`. Trivial patch, but touches `dsp/`.
- **Pitch bend** — parsed but ignored.
- **Pod knobs / encoder / switches / LEDs** — the Pod exposes POT_1
  (D21/A6), POT_2 (D15/A0), a clicked encoder (ENC_A=D26, ENC_B=D25,
  ENC_CLICK=D13), two tactile switches (D27, D28), and two RGB LEDs
  (D18–D20, D23–D24). Reading them requires bringing up the STM32 ADC and
  GPIO input handles — none of that is wired yet. MIDI CCs are currently
  the only parameter source. See the pin table above.
- **Panic handler** — currently `panic-halt`. Swap for `panic-probe` +
  `defmt` when you're debugging with a probe.

## Development on host

The DSP core, MIDI parser, and parameter surface are all `no_std` and
have host-runnable unit tests in the `dsp` crate. Those live in the
workspace and run with a plain:

```bash
cargo test --workspace
```

## Roadmap

- Add `cutoff_bias` to `dsp::voice::Voice` and wire CC 74 to it.
- Apply pitch bend to `osc.freq`.
- Add a dedicated filter envelope so accent boosts cutoff sweep, not just drive.
- Read Pod knobs / encoders via the on-board ADC / MCP23017 for hands-on control.
- Optional MIDI thru on UART TX.
