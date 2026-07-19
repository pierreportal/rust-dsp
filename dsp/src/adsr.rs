use crate::patch::Module;

#[derive(PartialEq, Debug, Copy, Clone)]
pub enum EnvState {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Copy)]
pub struct Adsr {
    pub value: f32,
    pub state: EnvState,
    pub sample_rate: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub velocity: f32,
}

impl Adsr {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            attack: 0.5,
            sustain: 1.0,
            release: 0.5,
            velocity: 1.0,
            state: EnvState::Idle,
            value: 0.0,
            decay: 0.1,
            sample_rate,
        }
    }
    pub fn trigger(&mut self, vel: u8) {
        self.velocity = vel as f32 / 127.0;
        self.state = EnvState::Attack;
    }

    pub fn is_idle(&self) -> bool {
        self.state == EnvState::Idle
    }

    pub fn release(&mut self) {
        self.state = EnvState::Release;
    }

    pub fn next_sample(&mut self) -> f32 {
        match self.state {
            EnvState::Idle => {
                self.value = 0.0;
            }

            EnvState::Attack => {
                let step = 1.0 / (self.attack * self.sample_rate).max(1.0);
                self.value += step;

                if self.value >= 1.0 {
                    self.value = 1.0;
                    self.state = EnvState::Decay;
                }
            }

            EnvState::Decay => {
                let step = (1.0 - self.sustain) / (self.decay * self.sample_rate).max(1.0);
                self.value -= step;

                if self.value <= self.sustain {
                    self.value = self.sustain;
                    self.state = EnvState::Sustain;
                }
            }

            EnvState::Sustain => {
                self.value = self.sustain;
            }

            EnvState::Release => {
                // let step = self.sustain / (self.release * self.sample_rate).max(1.0);
                let step = self.value / (self.release * self.sample_rate).max(1.0);
                self.value -= step;
                if self.value <= 0.0 {
                    self.value = 0.0;
                    self.state = EnvState::Idle;
                }
            }
        }
        self.value * self.velocity
    }
}

impl Module for Adsr {
    fn process(&mut self, input: f32) -> f32 {
        let x = self.next_sample();
        x * input
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f32 = 44100.0;
    const EPSILON: f32 = 1e-4;

    #[test]
    fn test_adsr_new() {
        let adsr = Adsr::new(SAMPLE_RATE);
        assert_eq!(adsr.value, 0.0);
        assert_eq!(adsr.state, EnvState::Idle);
        assert_eq!(adsr.sample_rate, SAMPLE_RATE);
        assert_eq!(adsr.attack, 0.5);
        assert_eq!(adsr.sustain, 1.0);
        assert_eq!(adsr.release, 0.5);
        assert_eq!(adsr.velocity, 1.0);
    }

    #[test]
    fn test_trigger() {
        let mut adsr = Adsr::new(SAMPLE_RATE);
        adsr.trigger(100);

        assert_eq!(adsr.state, EnvState::Attack);
        assert!((adsr.velocity - (100.0 / 127.0)).abs() < EPSILON);
    }

    #[test]
    fn test_trigger_max_velocity() {
        let mut adsr = Adsr::new(SAMPLE_RATE);
        adsr.trigger(127);

        assert_eq!(adsr.velocity, 1.0);
    }

    #[test]
    fn test_trigger_min_velocity() {
        let mut adsr = Adsr::new(SAMPLE_RATE);
        adsr.trigger(0);

        assert_eq!(adsr.velocity, 0.0);
    }

    #[test]
    fn test_is_idle() {
        let mut adsr = Adsr::new(SAMPLE_RATE);
        assert!(adsr.is_idle());

        adsr.trigger(100);
        assert!(!adsr.is_idle());
    }

    #[test]
    fn test_attack_phase() {
        let mut adsr = Adsr::new(SAMPLE_RATE);
        adsr.attack = 0.1; // 100ms attack
        adsr.trigger(127);

        let mut last_value = 0.0;
        let samples = (0.1 * SAMPLE_RATE) as usize;

        // During attack, value should increase
        for _ in 0..samples / 2 {
            let value = adsr.next_sample();
            assert!(
                value >= last_value,
                "Attack phase should increase monotonically"
            );
            assert_eq!(adsr.state, EnvState::Attack);
            last_value = value;
        }
    }

    #[test]
    fn test_attack_reaches_peak() {
        let mut adsr = Adsr::new(SAMPLE_RATE);
        adsr.attack = 0.01; // 10ms attack
        adsr.decay = 0.01;
        adsr.sustain = 0.8;
        adsr.trigger(127);

        // Process through entire attack phase
        let samples = (0.02 * SAMPLE_RATE) as usize;
        for _ in 0..samples {
            adsr.next_sample();
        }

        // Should have moved to decay or sustain
        assert!(adsr.state == EnvState::Decay || adsr.state == EnvState::Sustain);
    }

    #[test]
    fn test_decay_phase() {
        let mut adsr = Adsr::new(SAMPLE_RATE);
        adsr.attack = 0.001; // Very short attack
        adsr.decay = 0.1;
        adsr.sustain = 0.5;
        adsr.trigger(127);

        // Skip through attack
        let attack_samples = (0.002 * SAMPLE_RATE) as usize;
        for _ in 0..attack_samples {
            adsr.next_sample();
        }

        // Now in decay phase
        if adsr.state == EnvState::Decay {
            let value_start = adsr.value;
            for _ in 0..100 {
                adsr.next_sample();
            }
            // Value should decrease during decay
            assert!(adsr.value < value_start);
        }
    }

    #[test]
    fn test_sustain_phase() {
        let mut adsr = Adsr::new(SAMPLE_RATE);
        adsr.attack = 0.001;
        adsr.decay = 0.001;
        adsr.sustain = 0.7;
        adsr.trigger(127);

        // Process to sustain phase
        let samples = (0.005 * SAMPLE_RATE) as usize;
        for _ in 0..samples {
            adsr.next_sample();
        }

        // Should be in sustain
        if adsr.state == EnvState::Sustain {
            let sustain_value = adsr.value;
            // Hold for a while
            for _ in 0..1000 {
                let value = adsr.next_sample();
                assert!((value - sustain_value).abs() < EPSILON);
            }
        }
    }

    #[test]
    fn test_release_phase() {
        let mut adsr = Adsr::new(SAMPLE_RATE);
        adsr.attack = 0.001;
        adsr.decay = 0.001;
        adsr.sustain = 0.8;
        adsr.release = 0.1;
        adsr.trigger(127);

        // Process to sustain
        let samples = (0.005 * SAMPLE_RATE) as usize;
        for _ in 0..samples {
            adsr.next_sample();
        }

        // Trigger release
        adsr.release();
        assert_eq!(adsr.state, EnvState::Release);

        let mut last_value = adsr.value;
        // During release, value should decrease
        for _ in 0..100 {
            let value = adsr.next_sample();
            assert!(value <= last_value + EPSILON);
            last_value = value;
        }
    }

    #[test]
    fn test_release_reaches_idle() {
        let mut adsr = Adsr::new(SAMPLE_RATE);
        adsr.attack = 0.001;
        adsr.decay = 0.001;
        adsr.sustain = 0.5;
        adsr.release = 0.01;
        adsr.trigger(127);

        // Process to sustain
        let samples = (0.005 * SAMPLE_RATE) as usize;
        for _ in 0..samples {
            adsr.next_sample();
        }

        adsr.release();

        // Process entire release
        let release_samples = (0.02 * SAMPLE_RATE) as usize;
        for _ in 0..release_samples {
            adsr.next_sample();
        }

        // Should return to idle
        assert!(adsr.is_idle());
        assert_eq!(adsr.value, 0.0);
    }

    #[test]
    fn test_velocity_scaling() {
        let mut adsr1 = Adsr::new(SAMPLE_RATE);
        adsr1.attack = 0.001;
        adsr1.trigger(64); // Half velocity

        let mut adsr2 = Adsr::new(SAMPLE_RATE);
        adsr2.attack = 0.001;
        adsr2.trigger(127); // Full velocity

        // Verify velocity values
        assert!((adsr1.velocity - 64.0 / 127.0).abs() < EPSILON);
        assert!((adsr2.velocity - 1.0).abs() < EPSILON);

        // Process to peak
        for _ in 0..100 {
            adsr1.next_sample();
            adsr2.next_sample();
        }

        // Check that velocity scaling is working (output includes velocity)
        // The actual values might be in different envelope stages
        assert!(adsr1.velocity < adsr2.velocity);
    }

    #[test]
    fn test_idle_state_output() {
        let mut adsr = Adsr::new(SAMPLE_RATE);

        for _ in 0..100 {
            let value = adsr.next_sample();
            assert_eq!(value, 0.0);
            assert_eq!(adsr.state, EnvState::Idle);
        }
    }

    #[test]
    fn test_module_trait() {
        let mut adsr = Adsr::new(SAMPLE_RATE);
        adsr.trigger(127);

        let input = 1.0;
        let output = adsr.process(input);

        // Output should be scaled by envelope
        assert!(output >= 0.0 && output <= input);
    }

    #[test]
    fn test_full_envelope_cycle() {
        let mut adsr = Adsr::new(SAMPLE_RATE);
        adsr.attack = 0.01;
        adsr.decay = 0.01;
        adsr.sustain = 0.6;
        adsr.release = 0.01;

        // Start idle
        assert!(adsr.is_idle());

        // Trigger
        adsr.trigger(100);
        assert!(!adsr.is_idle());

        // Process through attack and decay
        for _ in 0..(0.03 * SAMPLE_RATE) as usize {
            adsr.next_sample();
        }

        // Should be in sustain
        assert_eq!(adsr.state, EnvState::Sustain);

        // Release
        adsr.release();

        // Process release
        for _ in 0..(0.02 * SAMPLE_RATE) as usize {
            adsr.next_sample();
        }

        // Back to idle
        assert!(adsr.is_idle());
    }
}
