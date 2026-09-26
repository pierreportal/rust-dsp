use crate::patch::Module;
use libm::{expf, logf};

/// A decay-only envelope in the style of the TB-303's filter envelope.
///
/// Unlike a full ADSR, the 303 filter envelope has:
///   - NO attack  (it snaps instantly to its maximum level on note-on)
///   - NO sustain (it immediately starts falling after the peak)
///   - ONLY a decay that falls exponentially toward zero
///
/// The envelope level is typically used to sweep a filter's cutoff: on each
/// note it jumps to `level` and then decays, which is what produces the
/// trademark "wow" acid squelch. The `accent` amount determines how deep the
/// sweep opens on a given note.
#[derive(Clone, Copy)]
pub struct AcidEnv {
    pub value: f32,
    pub decay: f32,
    pub accent: f32,
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

    /// Snap the envelope to its peak level. Called on every note-on.
    pub fn trigger(&mut self) {
        self.value = 1.0;
    }

    /// Instantly kill the envelope. Called on note-off; a 303 stops the
    /// filter sweep as soon as the note ends.
    pub fn release(&mut self) {
        self.value = 0.0;
    }

    pub fn is_idle(&self) -> bool {
        self.value <= 0.0
    }

    /// Advance one sample. The value falls exponentially toward zero.
    /// The falloff constant is derived from the desired decay time.
    pub fn next_sample(&mut self) -> f32 {
        // `per-sample multiplier` that reaches ~1% of the peak after `decay`
        // seconds. exp(-t / tau): choose tau = decay / ln(100) so that after
        // `decay` seconds the value is 1% of its peak.
        let tau = self.decay / logf(100.0);
        let decay_rate = expf(-1.0 / (tau * self.sample_rate).max(1.0));

        self.value *= decay_rate;
        if self.value < 1e-5 {
            self.value = 0.0;
        }

        self.value
    }

    /// The envelope signal scaled by the accent amount. This is how the
    /// envelope is applied to modulate the filter cutoff.
    pub fn mod_amount(&mut self) -> f32 {
        self.value * self.accent
    }
}

impl Module for AcidEnv {
    fn process(&mut self, input: f32) -> f32 {
        self.next_sample() * input
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f32 = 44100.0;

    #[test]
    fn test_new() {
        let env = AcidEnv::new(SAMPLE_RATE);
        assert_eq!(env.value, 0.0);
        assert_eq!(env.decay, 0.3);
        assert_eq!(env.accent, 0.5);
    }

    #[test]
    fn test_trigger_snaps_to_peak() {
        let mut env = AcidEnv::new(SAMPLE_RATE);
        env.trigger();
        assert!((env.value - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_no_attack_phase() {
        // The very first sample after trigger should already be at max,
        // demonstrating there is no attack ramp.
        let mut env = AcidEnv::new(SAMPLE_RATE);
        env.trigger();
        let first = env.next_sample();
        assert!(first > 0.99);
    }

    #[test]
    fn test_decay_monotonic() {
        let mut env = AcidEnv::new(SAMPLE_RATE);
        env.decay = 0.2;
        env.trigger();

        let mut last = env.next_sample();
        for _ in 0..1000 {
            let next = env.next_sample();
            assert!(next <= last + 1e-9, "envelope should decay monotonically");
            last = next;
        }
    }

    #[test]
    fn test_decay_time_semantics() {
        // After ~`decay` seconds the value should be well below the peak.
        let mut env = AcidEnv::new(SAMPLE_RATE);
        env.decay = 0.1;
        env.trigger();

        let samples = (env.decay * SAMPLE_RATE) as usize;
        let mut value = 1.0;
        for _ in 0..samples {
            value = env.next_sample();
        }
        assert!(
            value < 0.05,
            "expected near-zero after decay time, got {}",
            value
        );
    }

    #[test]
    fn test_longer_decay_last_longer() {
        let short_env_val = |decay: f32| -> f32 {
            let mut env = AcidEnv::new(SAMPLE_RATE);
            env.decay = decay;
            env.trigger();
            for _ in 0..1000 {
                env.next_sample();
            }
            env.value
        };

        let short = short_env_val(0.05);
        let long = short_env_val(0.5);
        assert!(
            long > short,
            "longer decay should hold value longer: short={short} long={long}"
        );
    }

    #[test]
    fn test_accent_scales_mod_amount() {
        let mut env = AcidEnv::new(SAMPLE_RATE);
        env.decay = 1.0; // slow decay so it barely falls over a few samples
        env.trigger();

        env.accent = 1.0;
        let full = env.mod_amount();

        env.accent = 0.3;
        let partial = env.mod_amount();

        // With the same raw value, a smaller accent gives a smaller amount.
        assert!(
            partial < full,
            "accent should scale the modulation amount: partial={partial} full={full}"
        );
    }

    #[test]
    fn test_release_instantly_kills() {
        let mut env = AcidEnv::new(SAMPLE_RATE);
        env.decay = 1.0;
        env.trigger();
        env.next_sample();
        assert!(env.value > 0.9);

        env.release();
        assert_eq!(env.value, 0.0);
        assert!(env.is_idle());
    }

    #[test]
    fn test_eventually_idle() {
        let mut env = AcidEnv::new(SAMPLE_RATE);
        env.decay = 0.1;
        env.trigger();
        for _ in 0..(0.5 * SAMPLE_RATE) as usize {
            env.next_sample();
        }
        assert!(env.is_idle());
    }

    #[test]
    fn test_module_trait() {
        let mut env = AcidEnv::new(SAMPLE_RATE);
        env.trigger();
        // Right after trigger, process(1.0) ~= 1.0 (peak * input).
        let out = env.process(1.0);
        assert!(out > 0.9 && out <= 1.0);
    }
}
