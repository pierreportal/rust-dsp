use crate::patch::Module;

pub enum Waveform {
    Saw,
    Triangle,
    Square,
    PulseWidth,
}

impl Waveform {
    fn saw(phase: f32) -> f32 {
        2.0 * phase - 1.0
    }
    fn triangle(phase: f32) -> f32 {
        // TODO: Needs to be improved
        2.0 * ((2.0 * phase - 1.0).abs() - 0.5)
    }
    fn square(phase: f32, width: f32) -> f32 {
        if phase < width {
            1.0
        } else {
            -1.0
        }
    }
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

pub struct Osc {
    pub phase: f32,
    pub freq: f32,
    pub sample_rate: f32,
    pub waveform: Waveform,
    pub pulse_width: f32,
}

impl Osc {
    pub fn next(&mut self) -> f32 {
        let dt = self.freq / self.sample_rate;

        let mut value = match self.waveform {
            Waveform::Saw => Waveform::saw(self.phase),
            Waveform::Triangle => Waveform::triangle(self.phase),
            Waveform::Square => Waveform::square(self.phase, 0.5),
            Waveform::PulseWidth => Waveform::square(self.phase, self.pulse_width),
        };

        value -= poly_blep(self.phase, dt);

        self.phase += dt;

        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        value
    }
}

impl Module for Osc {
    fn process(&mut self, input: f32) -> f32 {
        let x = self.next();
        x * input
    }
}
