pub struct Distortion {
    pub drive: f32,
    pub output_gain: f32,
}

impl Distortion {
    pub fn process(&self, input: f32) -> f32 {
        let x = input * self.drive;

        let y = x.tanh();
        y * self.output_gain
    }
}
