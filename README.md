# Rust-DSP Embedded Mono Synth (STM32 Daisy Seed)

## Embedded Audio DSP System

### Overview
A Rust-based DSP engine for embedded system. The project explores embedded audio synthesis, real-time constraints, and multi-platform compiling.

### DSP Design
The synthesis engine is built around a **[PolyBLEP](https://pbat.ch/sndkit/blep/) oscillator** to eliminate aliasing artifacts, an amplitude envelope for dynamic articulation, and a digital low-pass filter for tonal shaping and other basic synth modules.
The system is optimized for deterministic real-time execution with no dynamic allocation in the audio thread.

- Real-time audio processing at 48kHz on embedded STM32 hardware - Rust-based DSP core
- PolyBLEP oscillator for anti-aliased waveform generation 
- MIDI-controlled synthesis with hardware parameter mapping - Strict no-allocation, deterministic audio callback design

### Interaction Model (debug and host mode)
The instrument supports MIDI note input and real-time parameter control via hardware knobs.
Notes trigger a monophonic voice engine with frequency mapping and envelope shaping.

### Host mode features
Host mode allows you to test your synth voice using a MIDI keyboard and design the chain of modules. In host mode you can use:
- MIDI control
- Patch API
- Live audio stream

### Patch API macro
```rs
let signal: f32 = patch!(oscillator => envelope => filter)(input_gain);
```

Example usage:

```rs
let mut oscillator = Osc {
    phase: 0.0,
    freq: 440.0,
    waveform: Waveform::Saw,
    pulse_width: 0.5,
    sample_rate: device_sample_rate,
};

let mut envelope = Adsr {
    attack: 0.5,
    sustain: 1.0,
    release: 0.5,
    velocity: 1.0,
    state: EnvState::Idle,
    value: 0.0,
    decay: 0.1,
    sample_rate: device_sample_rate,
};

let mut filter = Filter {
    cutoff: 2000.0,
    z: 0.0,
    sample_rate,
};

let mut distortion = Distortion {
    drive: 10.0,
    output_gain: 1.0,
};

patch!(oscillator => envelope => distortion => filter)(1.0);
```
