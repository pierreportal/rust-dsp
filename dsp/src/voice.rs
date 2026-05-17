use crate::adsr::Adsr;
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
    pub filter_cutoff_smoother: Smoother,
}

impl Voice {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            osc: Osc::new(Waveform::PulseWidth, 220.0, sample_rate),
            env: Adsr::new(sample_rate),
            filter: Filter::new(sample_rate),
            distortion: Distortion::new(),
            freq_smoother: Smoother::new(440.0, 0.0005),
            filter_cutoff_smoother: Smoother::new(2000.0, 0.0005),
        }
    }

    fn self_update(&mut self) {
        self.osc.freq = self.freq_smoother.next();
        self.filter.cutoff = self.filter_cutoff_smoother.next();
    }

    pub fn next(&mut self) -> f32 {
        self.self_update();

        let input_sig = 1.0;

        patch!(self.osc => self.env => self.distortion => self.filter)(input_sig)
    }
}
