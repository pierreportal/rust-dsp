pub struct Smoother {
    pub current: f32,
    pub target: f32,
    pub coeff: f32,
}

impl Smoother {
    pub fn new(initial: f32, coeff: f32) -> Self {
        Self {
            current: initial,
            target: initial,
            coeff,
        }
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    pub fn next(&mut self) -> f32 {
        self.current += (self.target - self.current) * self.coeff;
        self.current
    }
}
