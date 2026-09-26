//! Runtime patch-graph engine for the web modular synth.
//!
//! Unlike the `patch!` macro (compile-time, fixed chain), this builds a
//! mutable node graph that can be rewired from the UI at runtime. Each node
//! has typed input/output ports; edges route output ports to input ports.
//! The graph is processed sample-by-sample in topological order each block.
use dsp::acid_filter::AcidFilter;
use dsp::adsr::Adsr;
use dsp::distortion::Distortion;
use dsp::osc::{Osc, Waveform};
use dsp::patch::Module;
use dsp::svf::Svf;
use std::collections::VecDeque;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Osc = 0,
    Adsr = 1,
    Filter = 2,
    Distortion = 3,
    Vca = 4,
    Mixer = 5,
    AcidFilter = 6,
    Constant = 7,
    Out = 8,
    Midi = 9,
    SineOsc = 10,
    SawOsc = 11,
    SquareOsc = 12,
}

impl Kind {
    fn from_u8(k: u32) -> Option<Kind> {
        match k {
            0 => Some(Kind::Osc),
            1 => Some(Kind::Adsr),
            2 => Some(Kind::Filter),
            3 => Some(Kind::Distortion),
            4 => Some(Kind::Vca),
            5 => Some(Kind::Mixer),
            6 => Some(Kind::AcidFilter),
            7 => Some(Kind::Constant),
            8 => Some(Kind::Out),
            9 => Some(Kind::Midi),
            10 => Some(Kind::SineOsc),
            11 => Some(Kind::SawOsc),
            12 => Some(Kind::SquareOsc),
            _ => None,
        }
    }

    pub fn inputs(self) -> &'static [&'static str] {
        match self {
            Kind::Osc => &["freq cv"],
            Kind::Adsr => &["gate"],
            Kind::Filter => &["signal", "cutoff cv"],
            Kind::Distortion => &["signal"],
            Kind::Vca => &["signal", "gain cv"],
            Kind::Mixer => &["a", "b", "c", "d"],
            Kind::AcidFilter => &["signal", "cutoff cv", "resonance cv"],
            Kind::Constant => &[],
            Kind::Out => &["signal"],
            Kind::Midi => &[],
            Kind::SineOsc => &["freq cv"],
            Kind::SawOsc => &["freq cv"],
            Kind::SquareOsc => &["freq cv"],
        }
    }

    pub fn outputs(self) -> &'static [&'static str] {
        match self {
            Kind::Osc => &["signal"],
            Kind::Adsr => &["env"],
            Kind::Filter => &["signal"],
            Kind::Distortion => &["signal"],
            Kind::Vca => &["signal"],
            Kind::Mixer => &["out"],
            Kind::AcidFilter => &["signal"],
            Kind::Constant => &["value"],
            Kind::Out => &[],
            Kind::Midi => &["gate", "pitch cv"],
            Kind::SineOsc => &["signal"],
            Kind::SawOsc => &["signal"],
            Kind::SquareOsc => &["signal"],
        }
    }
}

/// All tunable parameters. Only the fields relevant to a node's kind are used.
#[derive(Clone, Copy)]
pub struct Params {
    pub freq: f32, // osc base freq (Hz)
    pub waveform: u8,
    pub pulse_width: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub cutoff: f32, // filter base cutoff (Hz)
    pub resonance: f32,
    pub drive: f32,
    pub value: f32, // constant node output
}

impl Params {
    fn default_for(kind: Kind) -> Params {
        let base = Params {
            freq: 220.0,
            waveform: 1, // saw
            pulse_width: 0.5,
            attack: 0.01,
            decay: 0.15,
            sustain: 0.7,
            release: 0.3,
            cutoff: 1200.0,
            resonance: 0.2,
            drive: 4.0,
            value: 0.5,
        };
        match kind {
            Kind::Midi => Params { value: 0.0, ..base },
            _ => base,
        }
    }
}

/// Stateful DSP engines. Stateless kinds (Vca, Mixer, Constant, Out, Midi) use None.
pub enum NodeDsp {
    Osc(Osc),
    Adsr(Adsr),
    Filter(Svf),
    AcidFilter(AcidFilter),
    Distortion(Distortion),
    None,
}

pub struct Node {
    pub kind: Kind,
    pub params: Params,
    pub dsp: NodeDsp,
    pub gated: bool,     // ADSR gate tracking
    pub midi_note: u8,   // last MIDI note
    pub midi_gate: bool, // MIDI gate on/off
}

impl Node {
    fn new(kind: Kind, sample_rate: f32) -> Self {
        let params = Params::default_for(kind);
        let dsp = match kind {
            Kind::Osc => {
                let mut o = Osc::new(waveform_from_u8(params.waveform), params.freq, sample_rate);
                o.pulse_width = params.pulse_width;
                NodeDsp::Osc(o)
            }
            Kind::SineOsc => {
                let mut o = Osc::new(Waveform::Sine, params.freq, sample_rate);
                o.pulse_width = params.pulse_width;
                NodeDsp::Osc(o)
            }
            Kind::SawOsc => {
                let mut o = Osc::new(Waveform::Saw, params.freq, sample_rate);
                o.pulse_width = params.pulse_width;
                NodeDsp::Osc(o)
            }
            Kind::SquareOsc => {
                let mut o = Osc::new(Waveform::Square, params.freq, sample_rate);
                o.pulse_width = params.pulse_width;
                NodeDsp::Osc(o)
            }
            Kind::Adsr => {
                let mut e = Adsr::new(sample_rate);
                e.attack = params.attack;
                e.decay = params.decay;
                e.sustain = params.sustain;
                e.release = params.release;
                NodeDsp::Adsr(e)
            }
            Kind::Filter => {
                let mut f = Svf::new(sample_rate);
                f.set_cutoff(params.cutoff);
                f.set_resonance(params.resonance);
                NodeDsp::Filter(f)
            }
            Kind::AcidFilter => {
                let mut f = AcidFilter::new(sample_rate);
                f.set_cutoff(params.cutoff);
                f.set_resonance(params.resonance);
                NodeDsp::AcidFilter(f)
            }
            Kind::Distortion => {
                let mut d = Distortion::new();
                d.drive = params.drive;
                NodeDsp::Distortion(d)
            }
            _ => NodeDsp::None,
        };
        Self {
            kind,
            params,
            dsp,
            gated: false,
            midi_note: 69,
            midi_gate: false,
        }
    }

    /// Compute this node's outputs for one sample. `inputs` is indexed by
    /// input port; `out` is the node's output port slice (sized to outputs()).
    /// Returns true if this node is the audio sink (Out).
    fn process(&mut self, inputs: &[f32], out: &mut [f32]) -> bool {
        match self.kind {
            Kind::SquareOsc | Kind::SawOsc | Kind::SineOsc | Kind::Osc => {
                if let NodeDsp::Osc(o) = &mut self.dsp {
                    let cv = inputs.first().copied().unwrap_or(0.0);
                    o.freq = self.params.freq * (2.0f32).powf(cv);
                    out[0] = o.next_sample();
                }
                false
            }

            Kind::Adsr => {
                if let NodeDsp::Adsr(e) = &mut self.dsp {
                    let gate = inputs.first().copied().unwrap_or(0.0);
                    let on = gate > 0.5;
                    if on && !self.gated {
                        e.trigger(127);
                        self.gated = true;
                    } else if !on && self.gated {
                        e.release();
                        self.gated = false;
                    }
                    out[0] = e.next_sample();
                }
                false
            }
            Kind::Filter => {
                if let NodeDsp::Filter(f) = &mut self.dsp {
                    let sig = inputs.first().copied().unwrap_or(0.0);
                    let cv = inputs.get(1).copied().unwrap_or(0.0);
                    let cutoff = self.params.cutoff * (2.0f32).powf(cv);
                    f.set_cutoff(cutoff);
                    out[0] = f.process(sig);
                }
                false
            }
            Kind::AcidFilter => {
                if let NodeDsp::AcidFilter(f) = &mut self.dsp {
                    let sig = inputs.first().copied().unwrap_or(0.0);
                    let cv = inputs.get(1).copied().unwrap_or(0.0);
                    let cutoff = self.params.cutoff * (2.0f32).powf(cv);
                    f.set_cutoff(cutoff);
                    out[0] = f.process(sig);
                }
                false
            }
            Kind::Distortion => {
                if let NodeDsp::Distortion(d) = &mut self.dsp {
                    let sig = inputs.first().copied().unwrap_or(0.0);
                    out[0] = d.process(sig);
                }
                false
            }
            Kind::Vca => {
                let sig = inputs.first().copied().unwrap_or(0.0);
                let gain = inputs.get(1).copied().unwrap_or(1.0).max(0.0);
                out[0] = sig * gain;
                false
            }
            Kind::Mixer => {
                let a = inputs.first().copied().unwrap_or(0.0);
                let b = inputs.get(1).copied().unwrap_or(0.0);
                let c = inputs.get(2).copied().unwrap_or(0.0);
                let d = inputs.get(3).copied().unwrap_or(0.0);

                // Scale by the number of connected inputs so summing multiple
                // sources doesn't immediately push the signal past full scale.
                let mut n = 0u32;
                let mut sum = 0.0f32;
                for v in [a, b, c, d] {
                    sum += v;
                    n += 1;
                }
                out[0] = if n > 0 { sum / n as f32 } else { 0.0 };
                false
            }
            Kind::Constant => {
                out[0] = self.params.value;
                false
            }
            Kind::Out => true,
            Kind::Midi => {
                out[0] = if self.midi_gate { 1.0 } else { 0.0 };
                out[1] = (self.midi_note as f32 - 69.0) / 12.0;
                false
            }
        }
    }

    pub fn set_param(&mut self, name: &str, v: f32) {
        match self.kind {
            Kind::Osc => match name {
                "freq" => self.params.freq = v,
                "waveform" => {
                    self.params.waveform = v as u8;
                    if let NodeDsp::Osc(o) = &mut self.dsp {
                        o.waveform = waveform_from_u8(v as u8);
                    }
                }
                "pulseWidth" => {
                    self.params.pulse_width = v;
                    if let NodeDsp::Osc(o) = &mut self.dsp {
                        o.pulse_width = v;
                    }
                }
                _ => {}
            },
            Kind::SineOsc | Kind::SawOsc | Kind::SquareOsc => match name {
                "freq" => self.params.freq = v,
                _ => {}
            },
            Kind::Adsr => {
                if let NodeDsp::Adsr(e) = &mut self.dsp {
                    match name {
                        "attack" => e.attack = v,
                        "decay" => e.decay = v,
                        "sustain" => e.sustain = v,
                        "release" => e.release = v,
                        _ => {}
                    }
                }
            }
            Kind::Filter => match name {
                "cutoff" => self.params.cutoff = v,
                "resonance" => {
                    self.params.resonance = v;
                    if let NodeDsp::Filter(f) = &mut self.dsp {
                        f.set_resonance(v);
                    }
                }
                _ => {}
            },
            Kind::AcidFilter => match name {
                "cutoff" => self.params.cutoff = v,
                "resonance" => {
                    self.params.resonance = v;
                    if let NodeDsp::AcidFilter(f) = &mut self.dsp {
                        f.set_resonance(v);
                    }
                }
                _ => {}
            },
            Kind::Distortion => {
                if name == "drive" {
                    self.params.drive = v;
                    if let NodeDsp::Distortion(d) = &mut self.dsp {
                        d.drive = v;
                    }
                }
            }
            Kind::Constant => {
                if name == "value" {
                    self.params.value = v;
                }
            }
            _ => {}
        }
    }
}

fn waveform_from_u8(w: u8) -> Waveform {
    match w {
        0 => Waveform::Sine,
        1 => Waveform::Saw,
        2 => Waveform::Triangle,
        3 => Waveform::Square,
        _ => Waveform::PulseWidth,
    }
}

pub struct GraphEngine {
    sample_rate: f32,
    nodes: Vec<Option<Node>>,
    edges: Vec<(u32, u32, u32, u32)>, // from, from_port, to, to_port
    // Rebuilt on mutation (dense, allocation-free to read in process):
    order: Vec<u32>,                          // node ids in topological order
    input_sources: Vec<Vec<Vec<(u32, u32)>>>, // [id][input_port] -> sources
    current_out: Vec<Vec<f32>>,               // [id][output_port]
    out_ids: Vec<u32>,                        // ids of Out nodes (sinks)
}

/// Master soft-clip + output headroom.
///
/// The raw graph can sum signals that exceed full scale (a saw at ~±1.0 times
/// an envelope peaking at 1.0, a mixer with several inputs, multiple Out
/// sinks). A naive hard clamp near ±1.0 flattens the waveform tops into harsh,
/// harmonically rich "digital" distortion. Instead we apply a soft-knee tanh
/// limiter so overloads saturate smoothly, and we scale the signal to leave
/// headroom (the limiter's linear region sits well below 0 dBFS).
const MASTER_HEADROOM: f32 = 0.5; // nominal level ≤ ±0.5 → well under full scale
const LIMITER_DRIVE: f32 = 2.2; // gently raises the soft-knee saturation point

#[inline]
fn master_limiter(x: f32) -> f32 {
    let y = (x * MASTER_HEADROOM) as f64;
    // tanh(x) ≈ x for small x (linear pass-through for clean signals) and
    // saturates smoothly toward ±1 as the input grows.
    (libm::tanh(y * LIMITER_DRIVE as f64) / libm::tanh(LIMITER_DRIVE as f64)) as f32
}

impl GraphEngine {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            nodes: Vec::new(),
            edges: Vec::new(),
            order: Vec::new(),
            input_sources: Vec::new(),
            current_out: Vec::new(),
            out_ids: Vec::new(),
        }
    }

    fn ensure(&mut self, id: u32) {
        let need = id as usize + 1;
        if self.nodes.len() < need {
            self.nodes.resize_with(need, || None);
            self.input_sources.resize_with(need, Vec::new);
            self.current_out.resize_with(need, Vec::new);
        }
    }

    pub fn add_node(&mut self, id: u32, kind_u32: u32) -> bool {
        let kind = match Kind::from_u8(kind_u32) {
            Some(k) => k,
            None => return false,
        };
        self.ensure(id);
        let node = Node::new(kind, self.sample_rate);
        self.current_out[id as usize] = vec![0.0; kind.outputs().len()];
        self.nodes[id as usize] = Some(node);
        self.rebuild();
        true
    }

    pub fn remove_node(&mut self, id: u32) {
        let i = id as usize;
        if i >= self.nodes.len() || self.nodes[i].is_none() {
            return;
        }
        self.nodes[i] = None;
        self.current_out[i] = Vec::new();
        self.edges.retain(|&(f, _, t, _)| f != id && t != id);
        self.rebuild();
    }

    pub fn connect(&mut self, from: u32, from_port: u32, to: u32, to_port: u32) -> bool {
        if from == to {
            return false; // no self-loops
        }
        self.edges.push((from, from_port, to, to_port));
        if self.rebuild() {
            true
        } else {
            // cycle: revert
            self.edges.pop();
            self.rebuild();
            false
        }
    }

    pub fn disconnect(&mut self, from: u32, from_port: u32, to: u32, to_port: u32) {
        self.edges.retain(|&e| e != (from, from_port, to, to_port));
        self.rebuild();
    }

    pub fn set_param(&mut self, id: u32, name: &str, value: f32) {
        if let Some(node) = self.nodes.get_mut(id as usize).and_then(|n| n.as_mut()) {
            node.set_param(name, value);
        }
    }

    pub fn note_on(&mut self, id: u32, note: u8, _vel: u8) {
        if let Some(node) = self.nodes.get_mut(id as usize).and_then(|n| n.as_mut()) {
            if node.kind == Kind::Midi {
                node.midi_note = note;
                node.midi_gate = true;
            }
        }
    }

    pub fn note_off(&mut self, id: u32) {
        if let Some(node) = self.nodes.get_mut(id as usize).and_then(|n| n.as_mut()) {
            if node.kind == Kind::Midi {
                node.midi_gate = false;
            }
        }
    }

    /// Rebuild dense structures + topological order. Returns false on cycle.
    fn rebuild(&mut self) -> bool {
        let n = self.nodes.len();
        // input_sources sized per active node's input port count.
        let mut input_sources: Vec<Vec<Vec<(u32, u32)>>> = (0..n)
            .map(|i| match &self.nodes[i] {
                Some(node) => vec![Vec::new(); node.kind.inputs().len()],
                None => Vec::new(),
            })
            .collect();
        for &(f, fp, t, tp) in &self.edges {
            if let Some(port_vec) = input_sources
                .get_mut(t as usize)
                .and_then(|slot| slot.get_mut(tp as usize))
            {
                port_vec.push((f, fp));
            }
        }
        self.input_sources = input_sources;

        // Topological sort (Kahn) over active nodes.
        let mut indeg = vec![0u32; n];
        let mut adj: Vec<Vec<u32>> = vec![Vec::new(); n];
        let mut active: Vec<u32> = Vec::new();
        for (i, node) in self.nodes.iter().enumerate() {
            if node.is_some() {
                active.push(i as u32);
            }
        }
        for &(f, _, t, _) in &self.edges {
            if self.nodes[f as usize].is_some() && self.nodes[t as usize].is_some() {
                adj[f as usize].push(t);
                indeg[t as usize] += 1;
            }
        }
        let mut q: VecDeque<u32> = active
            .iter()
            .copied()
            .filter(|&i| indeg[i as usize] == 0)
            .collect();
        let mut order = Vec::with_capacity(active.len());
        while let Some(i) = q.pop_front() {
            order.push(i);
            for &j in &adj[i as usize] {
                indeg[j as usize] -= 1;
                if indeg[j as usize] == 0 {
                    q.push_back(j);
                }
            }
        }
        if order.len() != active.len() {
            self.order = Vec::new(); // cycle: refuse to process
            return false;
        }
        self.order = order;

        // Collect Out node ids.
        self.out_ids = self
            .order
            .iter()
            .copied()
            .filter(|&i| {
                self.nodes[i as usize]
                    .as_ref()
                    .map(|n| n.kind == Kind::Out)
                    .unwrap_or(false)
            })
            .collect();

        // Ensure current_out sized for any newly added outputs.
        for &i in &self.order {
            if let Some(node) = &self.nodes[i as usize] {
                let need = node.kind.outputs().len();
                if self.current_out[i as usize].len() != need {
                    self.current_out[i as usize] = vec![0.0; need];
                }
            }
        }
        true
    }

    /// Render `out.len()` samples into `out`. If there are multiple Out nodes,
    /// their signals are summed, then run through the master soft-clip limiter.
    pub fn process(&mut self, out: &mut [f32]) {
        if self.order.is_empty() {
            for s in out.iter_mut() {
                *s = 0.0;
            }
            return;
        }
        // Borrow fields disjointly so the loop body can read sources + write
        // outputs without aliasing issues.
        let order = self.order.clone();
        let input_sources = &self.input_sources;
        let current_out = &mut self.current_out;
        let nodes = &mut self.nodes;
        let out_ids = &self.out_ids;

        for sample in out.iter_mut() {
            *sample = 0.0;
            for &id in &order {
                let node = nodes[id as usize].as_mut().unwrap();
                let srcs = &input_sources[id as usize];
                let n_in = srcs.len();
                // Read inputs (sum of connected output ports).
                let mut inp = [0.0f32; 4];
                for p in 0..n_in {
                    let mut sum = 0.0;
                    for &(s_id, s_port) in &srcs[p] {
                        sum += current_out[s_id as usize][s_port as usize];
                    }
                    inp[p] = sum;
                }
                let is_out = node.process(&inp[..n_in], &mut current_out[id as usize]);
                if is_out {
                    *sample += inp[0];
                }
            }
            for &oid in out_ids {
                // already summed via is_out path; nothing more
                let _ = oid;
            }
            *sample = master_limiter(*sample);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48000.0;

    fn default_patch(g: &mut GraphEngine) {
        g.add_node(0, Kind::Midi as u32);
        g.add_node(1, Kind::Osc as u32);
        g.add_node(2, Kind::Adsr as u32);
        g.add_node(3, Kind::Vca as u32);
        g.add_node(4, Kind::Out as u32);
        g.connect(0, 1, 1, 0); // midi pitch -> osc freq cv
        g.connect(0, 0, 2, 0); // midi gate -> adsr gate
        g.connect(1, 0, 3, 0); // osc -> vca signal
        g.connect(2, 0, 3, 1); // env -> vca gain
        g.connect(3, 0, 4, 0); // vca -> out
    }

    #[test]
    fn default_patch_gain_is_bounded() {
        let mut g = GraphEngine::new(SR);
        default_patch(&mut g);
        g.note_on(0, 60, 127);

        let mut buf = vec![0.0f32; SR as usize];
        g.process(&mut buf);

        let peak = buf.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
        assert!(
            peak.is_finite() && peak <= 1.0,
            "peak {} must be bounded by the master limiter",
            peak
        );
        assert!(
            peak >= 0.3,
            "peak {} too low — the note should be clearly audible",
            peak
        );
        assert!(
            buf.iter().skip(SR as usize / 2).all(|s| s.is_finite()),
            "output must stay finite"
        );
    }

    #[test]
    fn repeated_note_on_does_not_retrigger_env() {
        let mut g = GraphEngine::new(SR);
        g.add_node(0, Kind::Midi as u32);
        g.add_node(1, Kind::Adsr as u32);
        g.add_node(2, Kind::Out as u32);
        g.connect(0, 0, 1, 0); // gate -> adsr
        g.connect(1, 0, 2, 0); // env -> out

        g.note_on(0, 60, 127);

        let mut buf1 = vec![0.0f32; SR as usize];
        g.process(&mut buf1);

        // Repeated note_ons while the gate is still high must not retrigger.
        g.note_on(0, 60, 127);
        g.note_on(0, 60, 127);
        let mut buf2 = vec![0.0f32; SR as usize];
        g.process(&mut buf2);

        let min1 = buf1
            .iter()
            .skip(SR as usize * 3 / 4)
            .cloned()
            .fold(f32::MAX, f32::min);
        let min2 = buf2
            .iter()
            .skip(SR as usize / 4)
            .cloned()
            .fold(f32::MAX, f32::min);
        assert!(
            min1 > 0.4 && min2 > 0.4,
            "env collapsed (min1={min1}, min2={min2}) — repeated note_on retriggered it"
        );

        // Releasing then re-triggering SHOULD retrigger (normal).
        g.note_off(0);
        // release defaults to 0.3s; wait well past it.
        let mut rel = vec![0.0f32; (SR * 2.0) as usize];
        g.process(&mut rel);
        assert!(rel
            .iter()
            .rev()
            .take(SR as usize / 10)
            .all(|s| s.abs() < 0.01));
        g.note_on(0, 60, 127);
        let mut buf3 = vec![0.0f32; SR as usize];
        g.process(&mut buf3);
        assert!(buf3.iter().take(1024).any(|&s| s > 0.2));
    }

    #[test]
    fn mixer_scales_summed_inputs() {
        let mut g = GraphEngine::new(SR);
        g.add_node(0, Kind::Constant as u32);
        g.add_node(1, Kind::Constant as u32);
        g.add_node(2, Kind::Constant as u32);
        g.add_node(3, Kind::Mixer as u32);
        g.add_node(4, Kind::Out as u32);
        g.set_param(0, "value", 1.0);
        g.set_param(1, "value", 1.0);
        g.set_param(2, "value", 1.0);
        g.connect(0, 0, 3, 0);
        g.connect(1, 0, 3, 1);
        g.connect(2, 0, 3, 2);
        g.connect(3, 0, 4, 0);

        let mut buf = vec![0.0f32; 64];
        g.process(&mut buf);
        // 1+1+1 averaged → 1.0 (then the master soft-limiter maps it ≈0.82).
        let v = buf[0];
        assert!(
            (v - master_limiter(1.0)).abs() < 1e-3,
            "mixer output unexpected: {v}"
        );
    }
}
