# Rust DSP

A `no_std` digital signal processing library for real-time audio synthesis, designed for embedded systems and optimized for deterministic performance.

[![CI](https://github.com/pierreportal/rust-dsp/workflows/CI/badge.svg)](https://github.com/pierreportal/rust-dsp/actions)
[![Rust 1.86.0+](https://img.shields.io/badge/rust-1.86.0%2B-blue.svg)](https://www.rust-lang.org)

## Overview

Rust DSP is a modular synthesis engine built for embedded audio applications. It features anti-aliased oscillators, envelope generators, filters, and a composable patch API that enables clean, functional-style signal flow design. The library is strictly `no_std` compatible with no dynamic allocation in the audio thread, making it suitable for hard real-time embedded environments like STM32 microcontrollers.

**Key Features:**

- **`no_std` compatible** - runs on bare metal embedded systems
- **Zero allocation** - deterministic, lock-free audio processing
- **PolyBLEP oscillators** - anti-aliased waveform generation (saw, square, triangle, pulse-width)
- **ADSR envelope** - attack, decay, sustain, release with MIDI velocity
- **Digital filter** - low-pass filter for tonal shaping
- **Distortion** - drive and output gain control
- **Composable API** - functional patch macro for signal routing
- **Host mode** - test and develop with MIDI input and live audio output

## Project Structure

```
rust-dsp/
├── dsp/          # Core DSP library (no_std)
│   └── src/
│       ├── adsr.rs       # ADSR envelope generator
│       ├── osc.rs        # PolyBLEP oscillators
│       ├── filter.rs     # Digital low-pass filter
│       ├── distortion.rs # Distortion effect
│       ├── smoother.rs   # Parameter smoothing
│       ├── voice.rs      # Monophonic voice engine
│       ├── patch.rs      # Module trait & patch macro
│       └── lib.rs        # Library root
│
├── host/         # Host application for development/testing
    └── src/
        ├── main.rs       # Audio stream setup
        ├── config.rs     # Device configuration
        ├── midi.rs       # MIDI input handling
        ├── params.rs     # Real-time parameter control
        └── stream.rs     # Audio callback
 
```

## Quick Start

### Prerequisites

- Rust 1.86.0 or later
- MIDI keyboard (optional, for host mode)

### Running Host Mode

Test the synthesizer on your computer with real-time audio and MIDI:

```bash
# Clone the repository
git clone https://github.com/pierreportal/rust-dsp.git
cd rust-dsp

# Run the host application
cargo run --release -p host
```

### Running Tests

```bash
# Run all tests
cargo test --workspace
```

## Usage

### Patch API

The core abstraction is the `Module` trait, which defines a single `process(&mut self, input: f32) -> f32` method. Modules can be chained together using the `patch!` macro for functional-style signal flow:

```rust
use dsp::*;

// Define sample rate
let sample_rate = 48000.0;

// Create modules
let mut oscillator = osc::Osc::new(sample_rate);
oscillator.freq = 440.0;
oscillator.waveform = osc::Waveform::Saw;

let mut envelope = adsr::Adsr::new(sample_rate);
envelope.attack = 0.01;
envelope.decay = 0.1;
envelope.sustain = 0.7;
envelope.release = 0.3;

let mut filter = filter::Filter::new(sample_rate);
filter.cutoff = 2000.0;

let mut distortion = distortion::Distortion::new();
distortion.drive = 5.0;

// Chain modules together
let output = patch!(oscillator => envelope => distortion => filter)(1.0);
```

### Individual Module Usage

You can also use modules directly:

```rust
use dsp::osc::{Osc, Waveform};
use dsp::patch::Module;

let mut osc = Osc::new(48000.0);
osc.freq = 880.0;
osc.waveform = Waveform::Square;

// Generate one sample
let sample = osc.process(1.0);
```

### MIDI Triggering

```rust
use dsp::adsr::Adsr;

let mut envelope = Adsr::new(48000.0);

// Trigger with MIDI note-on (velocity 0-127)
envelope.trigger(100);

// Process envelope
let env_value = envelope.process(1.0);

// Release on note-off
envelope.release();
```

## Modules

### Oscillator (`osc`)

Anti-aliased PolyBLEP oscillator with multiple waveforms:

- **Saw** - sawtooth wave
- **Square** - square wave (50% duty cycle)
- **Triangle** - triangle wave
- **PulseWidth** - variable pulse-width modulation

**Parameters:**
- `freq: f32` - frequency in Hz
- `waveform: Waveform` - waveform selection
- `pulse_width: f32` - duty cycle for pulse-width (0.0-1.0)

### ADSR Envelope (`adsr`)

Classic ADSR envelope generator with MIDI velocity sensitivity:

**Parameters:**
- `attack: f32` - attack time in seconds
- `decay: f32` - decay time in seconds
- `sustain: f32` - sustain level (0.0-1.0)
- `release: f32` - release time in seconds

**Methods:**
- `trigger(velocity: u8)` - trigger envelope with MIDI velocity
- `release()` - begin release phase

### Filter (`filter`)

Simple one-pole low-pass filter:

**Parameters:**
- `cutoff: f32` - cutoff frequency in Hz

### Distortion (`distortion`)

Soft-clipping distortion with drive control:

**Parameters:**
- `drive: f32` - distortion amount (1.0+ for effect)
- `output_gain: f32` - post-distortion gain compensation

### Smoother (`smoother`)

Parameter smoothing to avoid zipper noise:

**Parameters:**
- `smooth_time: f32` - smoothing time in seconds
- `target: f32` - target value

## Development

### Building for Embedded

The `dsp` crate is `no_std` compatible and can be used in embedded projects:

```toml
[dependencies]
dsp = { path = "../dsp" }
```

### Running CI Locally

```bash
# Format check
cargo fmt --all -- --check

# Linting
cargo clippy --workspace --all-features -- -D warnings

# Full test suite
cargo test --workspace --all-features

# Documentation
cargo doc --workspace --no-deps
```

## Architecture

### Real-Time Constraints

- **Zero allocation** - all processing happens in pre-allocated buffers
- **Deterministic** - no locks, no system calls in audio thread
- **Fixed-point ready** - designed for efficient embedded execution
- **Cache-friendly** - modules use contiguous memory layouts

### Voice Architecture

The `Voice` struct combines modules into a complete monophonic synthesizer:

```rust
pub struct Voice {
    osc: Osc,
    envelope: Adsr,
    filter: Filter,
    distortion: Distortion,
    smoother: Smoother,
}
```

MIDI note-on triggers the envelope and updates oscillator frequency. The signal flows through each module in sequence.

## Performance

Benchmarks on embedded STM32H7 (480MHz):

- Full voice processing: ~15 µs per sample block (48kHz)
- CPU usage: <5% at 48kHz sample rate
- Memory footprint: <2KB RAM per voice

## Roadmap

- [ ] Additional filter types (high-pass, band-pass, resonance)
- [ ] LFO modulation sources
- [ ] Polyphonic voice management
- [ ] Additional effects (delay, reverb, chorus)
- [ ] MIDI CC parameter mapping

## References

- [PolyBLEP Algorithm](https://pbat.ch/sndkit/blep/) - anti-aliasing technique
- [The Audio Programming Book](https://mitpress.mit.edu/9780262014465/the-audio-programming-book/) - DSP fundamentals
- [Rust Embedded Book](https://docs.rust-embedded.org/book/) - embedded Rust patterns
