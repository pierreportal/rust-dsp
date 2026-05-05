use crate::distortion::Distortion;
use crate::env::Env;
use crate::filter::Filter;
use crate::osc::{Osc, Waveform};
use crate::smoother::Smoother;

pub struct Voice {
    pub osc: Osc,
    pub env: Env,
    pub filter: Filter,
    pub distortion: Distortion,
    pub freq_smoother: Smoother,
}

impl Voice {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            osc: Osc {
                phase: 0.0,
                freq: 440.0,
                waveform: Waveform::Triangle,
                sample_rate,
            },
            env: Env {
                value: 1.0,
                decay: 0.01,
                active: true,
                sample_rate,
            },
            filter: Filter {
                cutoff: 2000.0,
                z: 0.0,
                sample_rate,
            },
            distortion: Distortion {
                drive: 50.0,
                output_gain: 1.0,
            },
            freq_smoother: Smoother::new(440.0, 0.001),
        }
    }

    pub fn next(&mut self) -> f32 {
        let freq = self.freq_smoother.next();
        self.osc.freq = freq;

        let mut sig = self.osc.next();
        sig *= self.env.next();
        sig = self.distortion.process(sig);
        sig = self.filter.process(sig);
        sig
    }
}
