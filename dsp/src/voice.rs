use crate::env::Env;
use crate::filter::Filter;
use crate::osc::Osc;

pub struct Voice {
    pub osc: Osc,
    pub env: Env,
    pub filter: Filter,
    pub freq: f32,
}

impl Voice {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            osc: Osc {
                phase: 0.0,
                freq: 440.0,
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
            freq: 440.0,
        }
    }

    pub fn next(&mut self) -> f32 {
        self.osc.freq = self.freq;

        let sig = self.osc.next();
        let env = self.env.next();

        let filtered = self.filter.process(sig * env);

        filtered
    }
}
