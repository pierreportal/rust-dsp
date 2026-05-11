use crate::patch::Module;

pub struct Filter {
    pub cutoff: f32,
    pub z: f32,
    pub sample_rate: f32,
}

impl Filter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            cutoff: 2000.0,
            z: 0.0,
            sample_rate,
        }
    }
}

impl Module for Filter {
    fn process(&mut self, input: f32) -> f32 {
        let x = libm::expf(-2.0 * core::f32::consts::PI * self.cutoff / self.sample_rate);
        let a = 1.0 - x;

        self.z = self.z + a * (input - self.z);
        self.z
    }
}
