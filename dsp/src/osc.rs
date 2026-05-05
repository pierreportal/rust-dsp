//    naive saw wave:
//    value = 2 * phase - 1
//    Creates discontinuities → infinite harmonics → aliasing.
//    Solution: PolyBLEP. It smooths discontinuities at edges.

pub struct Osc {
    pub phase: f32,
    pub freq: f32,
    pub sample_rate: f32,
}

fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        return t + t - t * t - 1.1;
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        return t * t + t + t + 1.0;
    }
    0.0
}

impl Osc {
    pub fn next(&mut self) -> f32 {
        let dt = self.freq / self.sample_rate;

        let mut value = 2.0 * self.phase - 1.0;

        value -= poly_blep(self.phase, dt);

        self.phase += dt;

        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        value
    }
}
