# Building a TB-303 Acid Bass Voice with `patch!`

This tutorial walks you through building a synthesizer voice that sounds
exactly like a classic **Roland TB-303 acid bass**. We reuse the existing
modules where they fit, build two new ones where the 303 demands something
the library doesn't yet have, and wire it all together with the `patch!`
macro.

By the end you'll have:

- a new 4-pole resonant low-pass filter (`acid_filter`),
- a new decay-only filter envelope (`acid_env`),
- a rewritten `Voice` that reproduces the signature 303 behavior (squelch,
  accent, slide),
- and a test that renders a real acid bass line to a `.wav` file you can
  listen to.

All the code lives under `dsp/`. Every step compiles and has unit tests.

---

## Step 0 — What makes a 303 sound like a 303?

Before writing any code, understand the anatomy. The TB-303 is worth
reproducing *precisely* because nearly every one of its quirks is what makes
it recognizable:

| 303 characteristic | Musical result |
|---|---|
| **Single VCO** (sawtooth or square) | aggressive harmonic-rich raw tone |
| **4-pole (24 dB/oct) low-pass filter** | steep rolloff, cute and round |
| **Resonant ladder filter with feedback** | the "squelch" and self-oscillation |
| **Decay-only filter envelope** (no attack, no sustain) | the per-note "wow" sweep |
| **Accent** — opens the sweep wider + louder on accented notes | rhythmic squelching groove |
| **Slide (portamento)** without envelope retrigger | the smooth, legato acid lines |
| **Post-filter overdrive** | the gnarly, aggressive acid timbre |

The single most important element is the **resonant low-pass filter with a
decay envelope sweeping its cutoff**. That sweeping filter is the whole
identity of the sound. So our plan is:

1. Reuse the existing `Osc`, `Adsr`, and `Distortion` modules.
2. Build a new `AcidFilter` (4-pole resonant low-pass) — the existing `Svf`
   is only 2-pole, so it can't give the 24 dB/oct character.
3. Build a new `AcidEnv` (decay-only filter envelope) — the existing `Adsr`
   has an attack and a sustain stage, which the 303's filter envelope
   explicitly does *not* have.
4. Rewrite `Voice` to combine them and add accent + slide.
5. Wire the audio path with `patch!` and render a WAV to hear it.

---

## Step 1 — Understand the `patch!` macro

The `patch!` macro is the connective tissue of the library. It is defined in
`dsp/src/patch.rs`:

```rust
pub trait Module {
    fn process(&mut self, input: f32) -> f32;
}

#[macro_export]
macro_rules! patch {
    ($($module:expr)=>+ $(,)?) => {{
        move |mut input| {
            $(
                input = $module.process(input);
            )+
            input
        }
    }};
}
```

### How it works

It expands into a closure that takes an `input` sample and threads it through
every module in order, left-to-right, separated by `=>`:

```text
patch!(a => b => c)(1.0)
    expands to  move |mut input| { input = a.process(input); input = b.process(input); input = c.process(input); input }
```

So the signal flows `input → a → b → c → output`.

### Key convention: most modules multiply by `input`

Look at how the existing modules implement `Module`:

- `Osc::process(input)` returns `next_sample() * input` — it *generates* a
  waveform and scales it by whatever came in.
- `Adsr::process(input)` returns `env_value * input` — it *generates* its
  envelope and multiplies it over the incoming signal.
- `Distortion::process(input)` returns `tanh(input * drive) * gain` — it
  *transforms* the incoming signal.
- `Filter::process(input)` returns a filtered `input` — it *transforms* the
  incoming signal.

The initial input you pass to the closure (conventionally `1.0`) is a
master gain / enable. Because `Osc` and `Adsr` are *generators* that scale
the signal by their internal value, putting them in the chain means:

```text
patch!(osc => env => filter)(1.0)
       saw * 1.0  →  env_value * (saw)  →  filter(env_value * saw)
```

That single line is a full voice. This is exactly the pattern the existing
`Voice` (in `dsp/src/voice.rs`) uses:

```rust
patch!(self.osc => self.env => self.filter => self.distortion)(1.0)
```

We keep this same idiom for the 303.

---

## Step 2 — Create the 4-pole resonant filter: `dsp/src/acid_filter.rs`

The heart of the 303. We need a *4-pole* resonant low-pass. Rather than a
single biquad, we **cascade four one-pole low-pass stages** to get the steep
24 dB/oct rolloff, and we feed the final stage's output back into the input
to create resonance — this is a digital cousin of the analogue diode ladder.

### The one-pole stage

A single-pole low-pass with coefficient `g` (derived from the cutoff) is:

```
y[n] = y[n-1] + g * (x[n] - y[n-1])
```

Cascade four of them. The bilinear-style mapping from cutoff to `g` is:

```
g = tan(pi * cutoff / sample_rate) / (1 + tan(pi * cutoff / sample_rate))
```

For cutoff well below Nyquist this is stable, and it approaches the ideal
24 dB/oct rolloff.

### Resonance

Feedback is the magic. We subtract `feedback * s4` (the last stage's output)
from the input, where `feedback = resonance * 4.0`:

```
track = input - feedback * s4
s1 += g * (track - s1)
s2 += g * (s1 - s2)
s3 += g * (s2 - s3)
s4 += g * (s3 - s4)
```

With `resonance` near `1.0`, `feedback` approaches `4.0` and the filter goes
into borderline **self-oscillation** — the screaming acid squelch. We clamp
`resonance` to `[0.0, 0.95]` so it never blows up.

### Full module

Create `dsp/src/acid_filter.rs`:

```rust
use crate::patch::Module;
use crate::smoother::Smoother;
use core::f32::consts::PI;
use libm::tanf;

pub struct AcidFilter {
    pub sample_rate: f32,
    pub cutoff: f32,
    pub resonance: f32,
    pub cutoff_smoother: Smoother,
    pub resonance_smoother: Smoother,

    g: f32,
    feedback: f32,
    s1: f32,
    s2: f32,
    s3: f32,
    s4: f32,
}

impl AcidFilter {
    pub fn new(sample_rate: f32) -> Self {
        let mut filter = Self {
            sample_rate,
            cutoff: 300.0,
            resonance: 0.5,
            cutoff_smoother: Smoother::new(300.0, 0.0005),
            resonance_smoother: Smoother::new(0.5, 0.0005),
            g: 0.0,
            feedback: 0.0,
            s1: 0.0, s2: 0.0, s3: 0.0, s4: 0.0,
        };
        filter.update();
        filter
    }

    pub fn set_cutoff(&mut self, cutoff: f32) {
        self.cutoff = cutoff.clamp(20.0, self.sample_rate * 0.45);
        self.update();
    }

    pub fn set_resonance(&mut self, resonance: f32) {
        self.resonance = resonance.clamp(0.0, 0.95);
        self.update();
    }

    fn update(&mut self) {
        let t = tanf(PI * self.cutoff / self.sample_rate);
        self.g = t / (1.0 + t);
        self.feedback = self.resonance * 4.0;
    }

    pub fn reset(&mut self) {
        self.s1 = 0.0; self.s2 = 0.0; self.s3 = 0.0; self.s4 = 0.0;
    }

    pub fn process_sample(&mut self, input: f32) -> f32 {
        let x = input - self.feedback * self.s4;
        self.s1 += self.g * (x - self.s1);
        self.s2 += self.g * (self.s1 - self.s2);
        self.s3 += self.g * (self.s2 - self.s3);
        self.s4 += self.g * (self.s3 - self.s4);
        self.s4
    }
}

impl Module for AcidFilter {
    fn process(&mut self, input: f32) -> f32 {
        self.process_sample(input)
    }
}
```

Implements `Module` so it slots into `patch!`. Keep in mind the audio loop
sweeps cutoff *between* samples (via `set_cutoff`), so the filter needs its
internal states (`s1..s4`) to persist — that's why they're struct fields.

The included unit tests confirm: DC passes through at unity, high
frequencies get strongly attenuated at low cutoff, resonance boosts the
signal near cutoff, and the filter stays bounded even at 0.95 resonance.

---

## Step 3 — Create the decay-only filter envelope: `dsp/src/acid_env.rs`

Now the second new module: the 303's filter envelope. It does **not** have
an attack ramp or a sustain level. On a note it snaps instantly to `1.0`
and decays exponentially toward zero. It is what sweeps the filter cutoff.

Create `dsp/src/acid_env.rs`:

```rust
use crate::patch::Module;
use libm::{expf, logf};

#[derive(Clone, Copy)]
pub struct AcidEnv {
    pub value: f32,
    pub decay: f32,   // seconds to fall to ~1% of peak
    pub accent: f32,  // 0.0..1.0, scales the sweep depth
    pub sample_rate: f32,
}

impl AcidEnv {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            value: 0.0,
            decay: 0.3,
            accent: 0.5,
            sample_rate,
        }
    }

    pub fn trigger(&mut self) {
        self.value = 1.0; // no attack: snap straight to peak
    }

    pub fn release(&mut self) {
        self.value = 0.0; // a 303 kills the sweep when the note ends
    }

    pub fn is_idle(&self) -> bool {
        self.value <= 0.0
    }

    pub fn next_sample(&mut self) -> f32 {
        // Choose tau so the value reaches ~1% of peak after `decay` seconds.
        let tau = self.decay / logf(100.0);
        let decay_rate = expf(-1.0 / (tau * self.sample_rate).max(1.0));
        self.value *= decay_rate;
        if self.value < 1e-5 {
            self.value = 0.0;
        }
        self.value
    }

    /// Envelope level scaled by the accent amount, for modulating cutoff.
    pub fn mod_amount(&mut self) -> f32 {
        self.value * self.accent
    }
}

impl Module for AcidEnv {
    fn process(&mut self, input: f32) -> f32 {
        self.next_sample() * input
    }
}
```

Notes:

- The `accent` field (0.0…1.0) scales how far the sweep opens. A value of
  `1.0` gives the biggest, most open "wow"; `0.0` leaves the filter closed.
- We keep this envelope **out of the audio `patch!` chain** and instead use
  it on the control path to modulate `filter.cutoff`. That is how a 303
  works: the envelope controls the *filter*, not the volume.
- It still implements `Module` (multiplying by input) so it *can* be used in
  a patch if you ever want it in the audio path, but its real role here is
  as a control signal.

---

## Step 4 — Register the new modules: `dsp/src/lib.rs`

Add the two new module declarations to `dsp/src/lib.rs` so they're part of
the crate and re-exported:

```rust
#![no_std]

pub mod acid_env;
pub mod acid_filter;
pub mod adsr;
pub mod distortion;
pub mod filter;
pub mod osc;
pub mod patch;
pub mod smoother;
pub mod svf;
pub mod voice;
```

---

## Step 5 — Rewrite the voice: `dsp/src/voice.rs`

Now assemble the modules. The `Voice` struct keeps the audio modules and two
smoothers, plus the 303-specific parameters:

```rust
use crate::acid_env::AcidEnv;
use crate::acid_filter::AcidFilter;
use crate::adsr::Adsr;
use crate::distortion::Distortion;
use crate::osc::{Osc, Waveform};
use crate::patch;
use crate::patch::Module;
use crate::smoother::Smoother;

pub struct Voice {
    //---audio modules (the patch! chain)---
    pub osc: Osc,
    pub filter: AcidFilter,
    pub amp_env: Adsr,
    pub distortion: Distortion,
    //---filter envelope (control path)---
    pub filter_env: AcidEnv,
    //---smoothers---
    pub freq_smoother: Smoother,
    pub accent_smoother: Smoother,
    //---303 parameters---
    pub base_cutoff: f32,
    pub env_depth: f32,
    pub slide: bool,
    pub slide_rate: f32,
    pub sample_rate: f32,
}
```

### Constructor

```rust
const INITIAL_FREQ: f32 = 120.0;
const INITIAL_CUTOFF: f32 = 300.0;
const DEFAULT_RESONANCE: f32 = 0.7;

impl Voice {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            osc: Osc::new(Waveform::Saw, INITIAL_FREQ, sample_rate),
            filter: AcidFilter::new(sample_rate),
            amp_env: Adsr::new(sample_rate),
            distortion: Distortion::new(),
            filter_env: AcidEnv::new(sample_rate),
            freq_smoother: Smoother::new(INITIAL_FREQ, 0.00005),
            accent_smoother: Smoother::new(0.5, 0.001),
            base_cutoff: INITIAL_CUTOFF,
            env_depth: 2000.0,
            slide: false,
            slide_rate: 0.01,
            sample_rate,
        }
    }

    pub fn init_acid_timbre(&mut self) {
        self.osc.waveform = Waveform::Saw;
        self.filter.set_resonance(DEFAULT_RESONANCE);

        // 303 amplitude envelope: near-zero attack, decay, no sustain.
        self.amp_env.attack = 0.001;
        self.amp_env.decay = 0.25;
        self.amp_env.sustain = 0.0;
        self.amp_env.release = 0.1;

        // Medium filter sweep.
        self.filter_env.decay = 0.25;
    }
}
```

### The control path: `self_update`

Runs once per sample, before the audio path. Three jobs:

```rust
fn self_update(&mut self) {
    // 1. Frequency, with optional portamento (slide).
    let target = self.freq_smoother.next_sample();
    if self.slide {
        self.osc.freq += (target - self.osc.freq) * self.slide_rate;
    } else {
        self.osc.freq = target;
    }

    // 2. Advance the filter envelope and sweep the cutoff.
    //    accent scales the sweep; env_depth sets the sweep range.
    let env = self.filter_env.next_sample();
    let accent = self.accent_smoother.next_sample();
    let sweep = env * self.env_depth * (0.3 + 0.7 * accent);
    let cutoff = (self.base_cutoff + sweep).clamp(20.0, self.sample_rate * 0.45);
    self.filter.set_cutoff(cutoff);

    // 3. Resonance.
    let resonance = self.filter.resonance_smoother.next_sample();
    self.filter.set_resonance(resonance);
}
```

Step 2 is the essence of the acid sound: each note, the envelope snaps the
cutoff up to `base_cutoff + env_depth` and it decays back down. Higher
`accent` opens it wider (`0.3 + 0.7*accent` ranges 0.3→1.0).

### The audio path: `patch!`

```rust
pub fn next_sample(&mut self) -> f32 {
    self.self_update();
    patch!(self.osc => self.filter => self.distortion => self.amp_env)(1.0)
}
```

Reading the chain: the oscillator generates a saw, the 4-pole resonant
filter shapes it (cutoff already swept by `self_update`), then distortion
overdrives the *timbre*. The amplitude ADSR runs **last**, so it gates the
final level. That ordering matters: putting the distortion after the
amplitude envelope would let `tanh` re-flatten the already-shaped signal into
a constant buzz. Overdrive the timbre first, then shape the volume — the
classic acid signal path. `1.0` is the master gain / enable.

### Note control

```rust
pub fn note_on(&mut self, freq: f32, vel: u8, accent: f32) {
    self.freq_smoother.set_target(freq);
    self.accent_smoother.set_target(accent.clamp(0.0, 1.0));
    self.filter_env.trigger();
    self.amp_env.trigger(vel);
}

pub fn note_off(&mut self) {
    self.filter_env.release();
    self.amp_env.release();
}
```

---

## Step 6 — Hear it: render a WAV

Add `hound` as a dev-dependency in `dsp/Cargo.toml`:

```toml
[dev-dependencies]
hound = "3"
```

Then create `dsp/tests/acid_303.rs`, which plays a 16-step acid pattern with
accents and slides and writes `dsp/target/acid_303.wav`:

```rust
use dsp::voice::Voice;

const SAMPLE_RATE: u32 = 44100;
const SR: f32 = SAMPLE_RATE as f32;
const BPM: f32 = 140.0;
const STEP_SECS: f32 = 60.0 / BPM / 4.0; // sixteenth note

fn midi_to_freq(note: u8) -> f32 {
    440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0)
}

#[test]
fn render_acid_303() {
    let mut voice = Voice::new(SR);
    voice.osc.waveform = dsp::osc::Waveform::Saw;
    voice.filter.set_resonance(0.8);

    voice.amp_env.attack = 0.001;
    voice.amp_env.decay = 0.2;
    voice.amp_env.sustain = 0.0;
    voice.amp_env.release = 0.05;

    voice.filter_env.decay = 0.3;
    voice.base_cutoff = 220.0;
    voice.env_depth = 2200.0;

    voice.distortion.drive = 1.2;
    voice.distortion.output_gain = 1.0;

    // 16-step pattern: (midi, accent, slide_into)
    let e2 = 40u8; // low E (one octave up from E1 so it's easy to hear)
    let (g, a, b, c) = (43u8, 45u8, 47u8, 48u8);
    let pattern: [(u8, f32, bool); 16] = [
        (e2, 1.0, false),
        (e2, 0.5, false),
        (g, 0.5, false),
        (a, 0.8, false),
        (b, 0.4, false),
        (c, 0.7, false),
        (b, 0.4, false),
        (a, 1.0, true),  // accent + slide into next
        (g, 0.5, false),
        (g, 0.4, false),
        (a, 0.8, false),
        (b, 0.4, false),
        (c, 0.7, false),
        (b, 0.4, false),
        (a, 0.6, false),
        (e2, 1.0, true), // accent + slide back to low E
    ];

    let total_steps = 32usize;
    let total_samples = (STEP_SECS * total_steps as f32 * SR) as usize;
    let step_samples = (STEP_SECS * SR) as usize;
    let mut samples = vec![0.0_f32; total_samples];
    let mut cursor = 0usize;

    for step in 0..total_steps {
        let (note, accent, slide) = pattern[step % 16];
        voice.slide = slide;
        if !slide {
            voice.freq_smoother.set_target(midi_to_freq(note));
            voice.accent_smoother.set_target(accent);
            voice.filter_env.trigger();
            voice.amp_env.trigger(100);
        } else {
            voice.freq_smoother.set_target(midi_to_freq(note));
        }
        for _ in 0..step_samples {
            if cursor < total_samples {
                samples[cursor] = voice.next_sample();
                cursor += 1;
            }
        }
    }

    // Write the WAV and assert it's audible.
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target").join("acid_303.wav");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    for &s in &samples {
        let clamped = s.clamp(-1.0, 1.0);
        writer.write_sample((clamped * i16::MAX as f32) as i16).unwrap();
    }
    writer.finalize().unwrap();

    let mut peak = 0.0_f32;
    for &s in &samples {
        assert!(s.is_finite());
        peak = peak.max(s.abs());
    }
    assert!(peak > 0.05, "expected audible output, peak={peak}");
    println!("Wrote {} (peak={peak:.2})", path.display());
}
```

Run it:

```bash
cargo test -p dsp --test acid_303 -- --nocapture
```

Open `dsp/target/acid_303.wav` in any audio player and you'll hear the
squelchy, sweeping acid bass — the accent notes pop open the filter and the
slides glide between pitches without retriggering the envelope, exactly like
a real 303.

---

## Step 7 — Verify the whole thing

```bash
# All unit tests for the new modules + the voice:
cargo test -p dsp --lib -- acid_env acid_filter voice

# The WAV render:
cargo test -p dsp --test acid_303 -- --nocapture

# The whole workspace still builds:
cargo build --workspace

# Clean code style:
cargo fmt -p dsp --check
cargo clippy -p dsp --all-targets
```

(Note: `dsp/src/adsr.rs` contains two pre-existing tests — `test_release_reaches_idle`
and `test_full_envelope_cycle` — that fail because the ADSR's release is an
exponential decay that never quite reaches zero within the window. These
failures exist on the original, unmodified code and are not introduced by
this tutorial.)

---

## Tuning to taste

These parameters map directly to making the sound more or less "acid":

| Parameter | Effect | Typical range |
|---|---|---|
| `filter.resonance` | squelch / self-oscillation | 0.5 → 0.95 (higher = more screaming) |
| `filter_env.decay` | length of each sweep | 0.1 s (short stab) → 0.5 s (long wash) |
| `base_cutoff` | starting cutoff of each note | 100–400 Hz |
| `env_depth` | how far the sweep opens | 500–4000 Hz |
| `amp_env.decay` | pluck length | 0.05 → 0.5 s |
| `distortion.drive` | aggression | 0.5 (mild) → 2.0 (hot); past ~3 it hard-squares |
| `accent` (per note) | how much each note "pops" | 0.0–1.0 |
| `osc.waveform` | Saw vs Square (both classic 303) | `Saw` / `Square` |
| `slide_rate` | glide speed | higher = slower glide |

With a saw, resonance ~0.8, a fast filter sweep, and a touch of drive, you
have the definitive 303 acid bass.
