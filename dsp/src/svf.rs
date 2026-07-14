use core::f32::consts::PI;
use libm::tanf;

#[derive(Debug, Clone, Copy)]
pub enum FilterMode {
    LowPass,
    HighPass,
    BandPass,
    Notch,
}

pub struct Svf {
    pub sample_rate: f32,

    pub cutoff: f32,
    pub resonance: f32,

    freq: f32,
    damp: f32,

    low: f32,
    high: f32,
    band: f32,
    notch: f32,

    mode: FilterMode,
}

impl Svf {
    pub fn new(sample_rate: f32) -> Self {
        let mut filter = Self {
            sample_rate,

            cutoff: 300.0,
            resonance: 0.0,

            freq: 0.0,
            damp: 0.0,

            low: 0.0,
            high: 0.0,
            band: 0.0,
            notch: 0.0,

            mode: FilterMode::LowPass,
        };

        filter.update();

        filter
    }

    pub fn set_cutoff(&mut self, cutoff: f32) {
        self.cutoff = cutoff.clamp(20.0, self.sample_rate * 0.45);

        self.update();
    }

    pub fn set_resonance(&mut self, resonance: f32) {
        self.resonance = resonance.clamp(0.0, 1.0);

        self.update();
    }

    pub fn set_mode(&mut self, mode: FilterMode) {
        self.mode = mode;
    }

    fn update(&mut self) {
        // frequency coefficient
        //
        // This controls how fast the filter reacts
        //
        let g = tanf(PI * self.cutoff / self.sample_rate);

        self.freq = g;

        // resonance damping
        //
        // higher resonance = less damping
        //
        self.damp = 1.0 - self.resonance * 0.9;
    }

    pub fn process(&mut self, input: f32) -> f32 {
        // feedback path
        let feedback = self.high * self.damp;

        let input = input - feedback;

        // first integrator
        self.band += self.freq * self.high;

        // second integrator
        self.low += self.freq * self.band;

        self.high = input - self.low - self.damp * self.band;

        self.notch = self.low + self.high;

        match self.mode {
            FilterMode::LowPass => self.low,

            FilterMode::HighPass => self.high,

            FilterMode::BandPass => self.band,

            FilterMode::Notch => self.notch,
        }
    }

    pub fn reset(&mut self) {
        self.low = 0.0;
        self.high = 0.0;
        self.band = 0.0;
        self.notch = 0.0;
    }
}
