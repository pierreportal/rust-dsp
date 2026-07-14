use crate::patch::Module;

pub struct LPF {
    pub cutoff: f32,
    pub alpha: f32,
    pub z: f32,
    pub sample_rate: f32,
}

impl LPF {
    pub fn new(sample_rate: f32, cutoff: f32) -> Self {
        Self {
            sample_rate,
            cutoff,
            alpha: 0.0,
            z: 0.0,
        }
    }

    pub fn set_cutoff(&mut self, cutoff: f32) {
        self.cutoff = cutoff;
    }
}

impl Module for LPF {
    fn process(&mut self, input: f32) -> f32 {
        let rc = 1.0 / (2.0 * core::f32::consts::PI * self.cutoff);
        let dt = 1.0 / self.sample_rate;

        self.alpha = dt / (rc + dt);

        self.z += self.alpha * (input - self.z);
        self.z
    }
}
