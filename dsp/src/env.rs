pub struct Env {
    pub value: f32,
    pub decay: f32,
    pub active: bool,
    pub sample_rate: f32,
}

impl Env {
    pub fn trigger(&mut self) {
        self.value = 1.0;
        self.active = true;
    }

    pub fn next(&mut self) -> f32 {
        if self.active {
            self.value -= self.decay / self.sample_rate;

            if self.value <= 0.0 {
                self.value = 0.0;
                self.active = false;
            }
        }

        self.value
    }
}
