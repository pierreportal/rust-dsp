pub enum EnvState {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

pub struct Env {
    pub value: f32,
    pub state: EnvState,
    pub sample_rate: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub velocity: f32,
}

impl Env {
    pub fn trigger(&mut self, vel: u8) {
        self.velocity = vel as f32 / 127.0;
        self.state = EnvState::Attack;
    }

    pub fn release(&mut self) {
        self.state = EnvState::Release;
    }

    pub fn next(&mut self) -> f32 {
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
                let step = self.sustain / (self.release * self.sample_rate).max(1.0);
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
