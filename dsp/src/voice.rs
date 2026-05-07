use crate::adsr::{Adsr, EnvState};
use crate::distortion::Distortion;
use crate::filter::Filter;
use crate::osc::{Osc, Waveform};
use crate::patch;
use crate::patch::Module;
use crate::smoother::Smoother;

pub struct Voice {
    pub osc: Osc,
    pub env: Adsr,
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
                waveform: Waveform::Saw,
                pulse_width: 0.7,
                sample_rate,
            },
            env: Adsr {
                attack: 0.5,
                sustain: 1.0,
                release: 0.5,
                velocity: 1.0,
                state: EnvState::Idle,
                value: 0.0,
                decay: 0.1,
                sample_rate,
            },
            filter: Filter {
                cutoff: 2000.0,
                z: 0.0,
                sample_rate,
            },
            distortion: Distortion {
                drive: 10.0,
                output_gain: 1.0,
            },
            freq_smoother: Smoother::new(440.0, 0.005),
        }
    }

    pub fn next(&mut self) -> f32 {
        self.osc.freq = self.freq_smoother.next();

        let input_gain = 1.0;

        patch!(self.osc => self.env => self.distortion => self.filter)(input_gain)
    }
}
