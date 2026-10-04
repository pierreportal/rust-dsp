//! Fixed-size polyphonic voice pool over the runtime patch graph.
//!
//! Polyphony is modelled the way the hardware/host build models it: the patch
//! graph *is* the voice. A `Midi` node inside one `GraphEngine` carries a single
//! note/gate pair, so a single engine can only ever sound one pitch. Rather than
//! reworking that, the pool keeps `VOICES` identical copies of the graph and
//! gives each held note its own copy.
//!
//! - Structural edits (add/remove/connect/param) are broadcast to every voice,
//!   so the copies can never disagree.
//! - Notes are *not* broadcast: a note-on is routed to one voice, so each voice
//!   has its own pitch and envelope. This is the whole point of the pool.
//!
//! Voice stealing, per-voice velocity, the smoothed mix gain and the
//! silent-voice skip all follow `host/src/poly.rs`, so the web build and the
//! native build behave the same way.
use crate::graph::{master_limiter, GraphEngine};
use dsp::smoother::Smoother;

/// Number of simultaneous voices. Matches the desktop/VST target.
pub const VOICES: usize = 16;

/// Seconds for the mix gain to glide when a note is added or released, so a
/// held chord does not step the output level.
const GAIN_SMOOTHING_SECONDS: f32 = 0.005;

/// Consecutive silent blocks before a non-held voice is parked.
const SILENCE_RUN: u16 = 64;

/// Peak below which a rendered block counts as silent.
const SILENCE_EPS: f32 = 1e-7;

struct Voice {
    eng: GraphEngine,
    /// MIDI note this voice currently holds, if any. `None` means the gate is
    /// up (or was never lowered) but no note is being held; the release tail
    /// may still be ringing.
    note: Option<u8>,
    /// Velocity as a linear gain. The graph's ADSR always triggers at full
    /// scale, so velocity is applied here at the voice's output.
    vel_gain: f32,
    /// Note-on order; lower is older, used to pick a steal victim.
    age: u32,
    /// Consecutive silent blocks seen while not held.
    silent: u16,
    /// A parked voice is skipped entirely until it is next allocated.
    idle: bool,
}

pub struct PolyGraph {
    voices: Vec<Voice>,
    /// Reused render target so the audio callback allocates nothing.
    scratch: Vec<f32>,
    gain: Smoother,
    next_age: u32,
}

impl PolyGraph {
    pub fn new(sample_rate: f32) -> Self {
        let voices = (0..VOICES)
            .map(|_| Voice {
                eng: GraphEngine::new(sample_rate),
                note: None,
                vel_gain: 1.0,
                age: 0,
                // Never played: park these immediately so an untouched patch
                // costs nothing.
                silent: SILENCE_RUN,
                idle: true,
            })
            .collect();
        Self {
            voices,
            scratch: Vec::new(),
            gain: Smoother::from_time(1.0, GAIN_SMOOTHING_SECONDS, sample_rate),
            next_age: 0,
        }
    }

    // ---- structural edits are broadcast to every voice ----

    pub fn add_node(&mut self, id: u32, kind: u32) -> bool {
        let mut added = false;
        for v in &mut self.voices {
            added |= v.eng.add_node(id, kind);
        }
        added
    }

    pub fn remove_node(&mut self, id: u32) {
        for v in &mut self.voices {
            v.eng.remove_node(id);
        }
    }

    pub fn connect(&mut self, from: u32, from_port: u32, to: u32, to_port: u32) -> bool {
        // Every voice applies the same edge to an identical graph, so they
        // cannot disagree on success. `|=` still reports failure correctly
        // because they all take the same branch.
        let mut ok = false;
        for v in &mut self.voices {
            ok |= v.eng.connect(from, from_port, to, to_port);
        }
        ok
    }

    pub fn disconnect(&mut self, from: u32, from_port: u32, to: u32, to_port: u32) {
        for v in &mut self.voices {
            v.eng.disconnect(from, from_port, to, to_port);
        }
    }

    pub fn set_param(&mut self, id: u32, name: &str, value: f32) {
        for v in &mut self.voices {
            v.eng.set_param(id, name, value);
        }
    }

    /// Feed a controller value to every voice. A controller modulates the patch
    /// as a whole, so every voice must see it — otherwise a sweep would only
    /// affect whichever voices happened to be allocated.
    pub fn set_cc(&mut self, cc: u8, value: u8) {
        for v in &mut self.voices {
            v.eng.set_cc(cc, value);
        }
    }

    // ---- note handling ----

    pub fn note_on(&mut self, note: u8, vel: u8) {
        if self.voices.is_empty() {
            return;
        }
        let vel_gain = vel as f32 / 127.0;

        // A note-on for a pitch that is already held retriggers that same voice
        // and does not allocate a second one, so holding a pitch twice cannot
        // stack voices. (The envelope does not retrigger because the gate never
        // drops — the graph treats a held gate as "still on".)
        if let Some(v) = self.voices.iter_mut().find(|v| v.note == Some(note)) {
            v.vel_gain = vel_gain;
            v.eng.note_on_all(note);
            return;
        }

        // Steal in priority order: a parked voice, then the oldest released
        // voice whose tail is still ringing, then the oldest held voice.
        let idx = self
            .voices
            .iter()
            .position(|v| v.idle)
            .or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| v.note.is_none())
                    .min_by_key(|(_, v)| v.age)
                    .map(|(i, _)| i)
            })
            .unwrap_or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, v)| v.age)
                    .map(|(i, _)| i)
                    .expect("voices is non-empty")
            });

        let age = self.next_age;
        self.next_age = self.next_age.wrapping_add(1);

        let v = &mut self.voices[idx];
        v.note = Some(note);
        v.vel_gain = vel_gain;
        v.age = age;
        v.silent = 0;
        v.idle = false;
        v.eng.note_on_all(note);

        self.update_gain_target();
    }

    pub fn note_off(&mut self, note: u8) {
        let mut changed = false;
        for v in &mut self.voices {
            if v.note == Some(note) {
                v.note = None;
                v.eng.note_off_all();
                changed = true;
            }
        }
        if changed {
            self.update_gain_target();
        }
    }

    fn held(&self) -> usize {
        self.voices.iter().filter(|v| v.note.is_some()).count()
    }

    fn update_gain_target(&mut self) {
        self.gain
            .set_target(1.0 / (self.held().max(1) as f32).sqrt());
    }

    /// Render the mix for one block: every un-parked voice into a shared
    /// scratch, velocity-scaled and summed, mix gain applied, then a single
    /// soft-clip on the summed signal.
    pub fn process(&mut self, out: &mut [f32]) {
        for s in out.iter_mut() {
            *s = 0.0;
        }
        // Destructure so each field is a distinct borrow: rendering a voice
        // needs `voices` (mut) and `scratch` (mut) at the same time.
        let Self {
            voices,
            scratch,
            gain,
            ..
        } = self;
        if scratch.len() != out.len() {
            scratch.resize(out.len(), 0.0);
        }

        for v in voices.iter_mut() {
            if v.idle {
                continue;
            }
            v.eng.process_raw(scratch);

            let vel_gain = v.vel_gain;
            let mut peak = 0.0f32;
            for (dst, &src) in out.iter_mut().zip(scratch.iter()) {
                *dst += src * vel_gain;
                peak = peak.max(src.abs());
            }

            if v.note.is_some() {
                v.silent = 0;
            } else if peak < SILENCE_EPS {
                v.silent = v.silent.saturating_add(1);
                if v.silent >= SILENCE_RUN {
                    v.idle = true;
                }
            } else {
                v.silent = 0;
            }
        }

        for s in out.iter_mut() {
            *s = master_limiter(*s * gain.next_sample());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48000.0;

    /// Goertzel magnitude of `freq` in `buf`; with `buf` one second long at SR
    /// the bins land on whole Hz so an on-bin tone does not leak.
    fn goertzel(buf: &[f32], freq: f32) -> f64 {
        let n = buf.len() as f64;
        let w = 2.0 * std::f64::consts::PI * freq as f64 / SR as f64;
        let coeff = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        let mut s0;
        for &x in buf {
            s0 = x as f64 + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        (s1 * s1 + s2 * s2 - coeff * s1 * s2).sqrt() / n * 2.0
    }

    /// Pitch the graph will actually produce for `note`, given a base of 440.
    fn played_hz(note: u8) -> f32 {
        440.0 * 2.0f32.powf((note as f32 - 69.0) / 12.0)
    }

    /// A three-partial-friendly voice: one sine per note so spectral checks are
    /// unambiguous. Sine base 440 keeps every played pitch an exact value.
    fn default_patch(g: &mut PolyGraph) {
        g.add_node(0, 9); // Midi
        g.add_node(1, 10); // SineOsc
        g.add_node(2, 1); // Adsr
        g.add_node(3, 4); // Vca
        g.add_node(4, 8); // Out
        g.set_param(1, "freq", 440.0);
        g.set_param(2, "attack", 0.001);
        g.set_param(2, "decay", 0.001);
        g.set_param(2, "sustain", 0.3);
        g.set_param(2, "release", 0.1);
        g.connect(0, 1, 1, 0); // pitch cv -> osc
        g.connect(0, 0, 2, 0); // gate -> adsr
        g.connect(1, 0, 3, 0); // osc -> vca signal
        g.connect(2, 0, 3, 1); // env -> vca gain
        g.connect(3, 0, 4, 0); // vca -> out
    }

    #[test]
    fn silent_until_a_note_is_played() {
        let mut g = PolyGraph::new(SR);
        default_patch(&mut g);
        let mut buf = vec![0.0f32; 128];
        g.process(&mut buf);
        assert!(
            buf.iter().all(|&s| s.abs() < 1e-7),
            "unpatched voice leaked"
        );
    }

    #[test]
    fn a_chord_sounds_every_held_pitch_at_once() {
        // The defining property of the pool: three held notes must each be
        // present in the mix, each from its own oscillator voice. A monophonic
        // engine would only ever show the last note played.
        let mut g = PolyGraph::new(SR);
        default_patch(&mut g);
        for note in [60u8, 64, 67] {
            g.note_on(note, 127);
        }

        let mut buf = vec![0.0f32; SR as usize * 2];
        g.process(&mut buf);
        let steady = &buf[SR as usize..];

        for note in [60u8, 64, 67] {
            let mag = goertzel(steady, played_hz(note));
            assert!(mag > 0.02, "note {note} missing from the chord: {mag}");
        }
    }

    #[test]
    fn note_off_releases_only_that_note() {
        let mut g = PolyGraph::new(SR);
        default_patch(&mut g);
        g.note_on(60, 127);
        g.note_on(64, 127);

        let mut warm = vec![0.0f32; 256];
        g.process(&mut warm);

        g.note_off(60);

        // Keep processing past 64's release; it should still be ringing.
        let mut tail = vec![0.0f32; SR as usize / 4];
        g.process(&mut tail);
        assert!(
            goertzel(&tail, played_hz(64)) > 0.01,
            "releasing one note silenced the other"
        );
    }

    #[test]
    fn releasing_an_unheld_note_is_a_no_op() {
        let mut g = PolyGraph::new(SR);
        default_patch(&mut g);
        g.note_on(60, 127);
        let mut warm = vec![0.0f32; 256];
        g.process(&mut warm);
        g.note_off(61); // never held
        let mut still = vec![0.0f32; 256];
        g.process(&mut still);
        assert!(goertzel(&still, played_hz(60)) > 0.01, "held note was cut");
    }

    #[test]
    fn repeating_a_held_note_does_not_stack_voices() {
        let mut g = PolyGraph::new(SR);
        default_patch(&mut g);
        g.note_on(60, 127);
        let mut warm = vec![0.0f32; 256];
        g.process(&mut warm);
        // Hammer the same pitch; it must reuse the one voice.
        for _ in 0..8 {
            g.note_on(60, 127);
        }
        let mut after = vec![0.0f32; SR as usize / 4];
        g.process(&mut after);
        let mag = goertzel(&after, played_hz(60));
        // One voice measures at the envelope's sustain level (~0.3 here); a
        // second stacked copy would roughly double it.
        assert!(
            mag > 0.15 && mag < 0.45,
            "held pitch stacked or dropped out: {mag}"
        );
    }

    #[test]
    fn stealing_keeps_more_than_a_pool_of_notes_bounded() {
        let mut g = PolyGraph::new(SR);
        default_patch(&mut g);
        // More simultaneous notes than there are voices.
        for note in 40..(40 + VOICES as u8 + 12) {
            g.note_on(note, 127);
        }
        let mut buf = vec![0.0f32; SR as usize];
        g.process(&mut buf);
        assert!(
            buf.iter().all(|s| s.is_finite() && s.abs() <= 1.0),
            "steal overflowed the limiter"
        );
        // And the newest note is definitely sounding.
        let newest = 40 + VOICES as u8 + 11;
        assert!(
            goertzel(&buf, played_hz(newest)) > 0.01,
            "newest note lost during stealing"
        );
    }

    #[test]
    fn velocity_scales_a_voice() {
        let mut g = PolyGraph::new(SR);
        default_patch(&mut g);
        g.note_on(60, 127);
        let mut loud = vec![0.0f32; SR as usize];
        g.process(&mut loud);
        g.note_off(60);

        let mut g2 = PolyGraph::new(SR);
        default_patch(&mut g2);
        g2.note_on(60, 32);
        let mut soft = vec![0.0f32; SR as usize];
        g2.process(&mut soft);

        let loud_mag = goertzel(&loud, played_hz(60));
        let soft_mag = goertzel(&soft, played_hz(60));
        assert!(
            soft_mag < loud_mag * 0.5,
            "velocity ignored (soft={soft_mag} loud={loud_mag})"
        );
    }
}
