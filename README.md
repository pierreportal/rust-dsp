# Rust-DSP Embedded Mono Synth (STM32 Daisy Seed)

## Embedded Audio DSP System

### Overview
A real-time monophonic synthesizer built on STM32 (Daisy Seed), featuring a Rust-based DSP engine integrated via a C++ libDaisy firmware layer. The project explores embedded audio synthesis, real-time constraints, and cross-language systems design.

### System Architecture
```
Daisy Seed (STM32H7) 
    ↓ 
libDaisy (C++)
    ↓ 
Audio Callback (ISR) 
    ↓ 
Rust DSP Engine
    ↓
Voice Processing (Oscillator, Envelope, Filter) 
    ↓ 
Audio Output
```
### DSP Design
The synthesis engine is built around a **[PolyBLEP](https://pbat.ch/sndkit/blep/) oscillator** to eliminate aliasing artifacts, an amplitude envelope for dynamic articulation, and a digital low-pass filter for tonal shaping. 
The system is optimized for deterministic real-time execution with no dynamic allocation in the audio thread.

### Engineering Highlights
- Real-time audio processing at 48kHz on embedded STM32 hardware - Rust-based DSP core with C++ hardware abstraction layer 
- PolyBLEP oscillator for anti-aliased waveform generation 
- MIDI-controlled synthesis with hardware parameter mapping - Strict no-allocation, deterministic audio callback design

### Interaction Model
The instrument supports MIDI note input and real-time parameter control via hardware knobs.
Notes trigger a monophonic voice engine with frequency mapping and envelope shaping.

