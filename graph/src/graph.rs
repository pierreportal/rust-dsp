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
    CC = 13,
}

impl Kind {
    /// The wire/native kind code for a module, or `None` if the host asked for
    /// a kind this build does not have. Hosts use this to validate an incoming
    /// patch without pattern-matching every variant themselves.
    pub fn from_u8(k: u32) -> Option<Kind> {
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
            13 => Some(Kind::CC),
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
            Kind::CC => &[],
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
            Kind::CC => &["value"],
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
    /// Which MIDI controller a `CC` node reads.
    pub cc: f32,
    /// Bipolar modulation depth in octaves for a `CC` node, so its output lines
    /// up with the `2^cv` convention the other CV inputs already use.
    pub depth: f32,
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
            cc: 74.0,
            depth: 2.0,
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
    /// Last controller value seen by a `CC` node, already centred to -1..=1 so
    /// that an unpatched controller sits at zero (no modulation) rather than at
    /// full negative depth.
    pub cc_bipolar: f32,
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
            cc_bipolar: 0.0,
        }
    }

    /// Compute this node's outputs for one sample. `inputs` is indexed by
    /// input port; `out` is the node's output port slice (sized to outputs()).
    /// `connected` is a bitmask of input ports that have a cable attached, so
    /// a node can tell "patched but silent" from "not patched".
    /// Returns true if this node is the audio sink (Out).
    fn process(&mut self, inputs: &[f32], out: &mut [f32], connected: u8) -> bool {
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
                // Average only the ports that actually have something patched
                // into them, so a three-way mix stays at unity rather than
                // being attenuated by the idle fourth input. `connected` is
                // required: a connected-but-silent source still counts, which a
                // "is it non-zero" test could not tell apart from no cable.
                let mut n = 0u32;
                let mut sum = 0.0f32;
                for (p, &input) in inputs.iter().enumerate() {
                    if connected & (1 << p) == 0 {
                        continue;
                    }
                    sum += input;
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
            Kind::CC => {
                // Bipolar, scaled in octaves: the same `2^cv` convention the
                // pitch and cutoff CV inputs already expect, so a controller
                // sweeps symmetrically instead of only ever brightening.
                out[0] = self.cc_bipolar * self.params.depth;
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
            Kind::SineOsc | Kind::SawOsc | Kind::SquareOsc if name == "freq" => {
                self.params.freq = v
            }
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
            Kind::CC => match name {
                "cc" => self.params.cc = v,
                "depth" => self.params.depth = v,
                _ => {}
            },
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
    input_mask: Vec<u8>,                      // [id] bitmask of patched input ports
    current_out: Vec<Vec<f32>>,               // [id][output_port]
    out_ids: Vec<u32>,                        // ids of Out nodes (sinks)
}

/// Soft-knee safety clipper.
///
/// The raw graph can legitimately exceed full scale (a saw at ~±1.0 times an
/// envelope peaking at 1.0, a mixer summing several sources, multiple Out
/// sinks). A naive hard clamp near ±1.0 flattens the waveform tops into harsh,
/// harmonically rich "digital" distortion.
///
/// Below `KNEE` the signal is returned bit-for-bit unchanged, so an ordinary
/// patch is completely transparent and a pure sine stays pure. Above `KNEE` the
/// remaining headroom is bent smoothly onto ±1.0.
///
/// This is a *clipper*, not a compressor: it has no envelope follower and no
/// gain reduction over time, so a loud sustained tone is still squashed rather
/// than ducked. It catches peaks; it does not control dynamics.
const KNEE: f32 = 0.8;

#[inline]
pub fn master_limiter(x: f32) -> f32 {
    let a = x.abs();
    if a <= KNEE {
        return x;
    }
    // tanh(0) = 0 gives a slope of exactly 1 at the knee, so the join is
    // C1-continuous; the second derivative is also 0 there, so it is C2 as
    // well and the knee is inaudible.
    let headroom = 1.0 - KNEE;
    let y = KNEE + headroom * libm::tanh(((a - KNEE) / headroom) as f64) as f32;
    if x < 0.0 {
        -y
    } else {
        y
    }
}

impl GraphEngine {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            nodes: Vec::new(),
            edges: Vec::new(),
            order: Vec::new(),
            input_sources: Vec::new(),
            input_mask: Vec::new(),
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

    /// Gate every Midi node in this graph with a note-on.
    ///
    /// A `Midi` node holds a single note/gate pair, so one engine can only ever
    /// play one pitch. The poly voice pool calls this on a single voice to give
    /// that voice its own pitch, which is why a patch with several Midi nodes
    /// still moves in lockstep rather than splitting the pitch across them.
    pub fn note_on_all(&mut self, note: u8) {
        for node in self.nodes.iter_mut().flatten() {
            if node.kind == Kind::Midi {
                node.midi_note = note;
                node.midi_gate = true;
            }
        }
    }

    /// Release every Midi node in this graph.
    pub fn note_off_all(&mut self) {
        for node in self.nodes.iter_mut().flatten() {
            if node.kind == Kind::Midi {
                node.midi_gate = false;
            }
        }
    }

    /// Feed a controller value to every `CC` node tuned to that controller.
    ///
    /// Unlike notes, this is broadcast to *all* matching modules rather than
    /// routed to one voice: a controller is a modulation source, not something
    /// a single voice owns, so two `CC` nodes reading CC 74 both move and the
    /// value survives a voice being stolen. The value is centred to -1..=1
    /// here so `Node::process` only has to apply depth.
    pub fn set_cc(&mut self, cc: u8, value: u8) {
        for node in self.nodes.iter_mut().flatten() {
            if node.kind == Kind::CC && node.params.cc.round() as i32 == cc as i32 {
                node.cc_bipolar = (value as f32 / 127.0) * 2.0 - 1.0;
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
        let mut input_mask = vec![0u8; n];
        for &(f, fp, t, tp) in &self.edges {
            if let Some(port_vec) = input_sources
                .get_mut(t as usize)
                .and_then(|slot| slot.get_mut(tp as usize))
            {
                port_vec.push((f, fp));
                input_mask[t as usize] |= 1 << tp;
            }
        }
        self.input_sources = input_sources;
        self.input_mask = input_mask;

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
        self.render(out);
        for s in out.iter_mut() {
            *s = master_limiter(*s);
        }
    }

    /// Render `out.len()` samples into `out` with no output limiting.
    ///
    /// The poly voice pool needs this: it sums several independent copies of the
    /// graph and then clips the *mix* once. Letting `process` limit each voice
    /// first would squash every voice against full scale on its own, which
    /// changes the sound and makes the mix depend on how the voices are spread.
    pub fn process_raw(&mut self, out: &mut [f32]) {
        self.render(out);
    }

    /// Render one block, summing every Out node, without limiting.
    fn render(&mut self, out: &mut [f32]) {
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
        let input_mask = &self.input_mask;
        let current_out = &mut self.current_out;
        let nodes = &mut self.nodes;

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
                let is_out = node.process(
                    &inp[..n_in],
                    &mut current_out[id as usize],
                    input_mask[id as usize],
                );
                if is_out {
                    *sample += inp[0];
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48000.0;

    /// Magnitude of `freq` in `buf` (Goertzel). With `buf` one second long at
    /// SR, bins land on whole Hz, so a tone on an exact bin has no leakage.
    fn goertzel(buf: &[f32], freq: f32) -> f64 {
        let n = buf.len() as f64;
        let w = 2.0 * std::f64::consts::PI * freq as f64 / SR as f64;
        let coeff = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for &x in buf {
            let s0 = x as f64 + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        (s1 * s1 + s2 * s2 - coeff * s1 * s2).sqrt() / n * 2.0
    }

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
    fn master_limiter_is_bit_transparent_below_the_knee() {
        // The safety clipper must not colour ordinary levels. Anything at or
        // below the knee has to come back out exactly as it went in.
        for i in 0..=800 {
            let x = i as f32 / 1000.0;
            assert_eq!(master_limiter(x), x, "positive {x} was altered");
            assert_eq!(master_limiter(-x), -x, "negative {x} was altered");
        }
    }

    #[test]
    fn master_limiter_stays_bounded_and_monotonic() {
        let mut prev = -1.0f32;
        for i in 0..=4000 {
            let x = -2.0 + i as f32 * 0.001;
            let y = master_limiter(x);
            assert!(y.abs() <= 1.0, "{x} -> {y} exceeds full scale");
            assert!(y >= prev, "not monotonic at {x}: {y} < {prev}");
            assert!(y.is_finite());
            prev = y;
        }
    }

    #[test]
    fn master_limiter_knee_has_unit_slope() {
        // tanh'(0) = 1 means the curve leaves the linear region with the same
        // slope it arrived with, so the knee introduces no step in gain.
        let d = 1e-4;
        let below = (master_limiter(KNEE) - master_limiter(KNEE - d)) / d;
        let above = (master_limiter(KNEE + d) - master_limiter(KNEE)) / d;
        assert!(
            (below - 1.0).abs() < 1e-3 && (above - 1.0).abs() < 1e-3,
            "knee slope discontinuous: below={below} above={above}"
        );
    }

    #[test]
    fn master_limiter_never_boosts() {
        for i in 0..=2000 {
            let x = i as f32 * 0.001;
            assert!(master_limiter(x) <= x, "clipper boosted {x}");
        }
    }

    #[test]
    fn held_sine_is_not_coloured_by_the_output_stage() {
        // Regression: the output stage used to be an unconditional tanh
        // saturator, which put a 3rd harmonic around -32 dBc on a held sine and
        // read as "crushed". Drive the default patch with a note whose pitch CV
        // is zero so the oscillator sits exactly on a 1 Hz bin, let the
        // envelope reach sustain, and require the harmonics to stay at the f32
        // noise floor.
        const F0: f32 = 131.0;
        let mut g = GraphEngine::new(SR);
        g.add_node(0, Kind::Midi as u32);
        g.add_node(1, Kind::SineOsc as u32);
        g.add_node(2, Kind::Adsr as u32);
        g.add_node(3, Kind::Vca as u32);
        g.add_node(4, Kind::Out as u32);
        g.set_param(1, "freq", F0);
        g.set_param(2, "attack", 0.001);
        g.set_param(2, "decay", 0.001);
        g.connect(0, 1, 1, 0);
        g.connect(0, 0, 2, 0);
        g.connect(1, 0, 3, 0);
        g.connect(2, 0, 3, 1);
        g.connect(3, 0, 4, 0);
        g.note_on(0, 69, 127); // A4 -> pitch cv 0 -> freq stays F0

        let mut buf = vec![0.0f32; SR as usize * 2];
        g.process(&mut buf);
        let sustain = &buf[SR as usize..];

        let fundamental = goertzel(sustain, F0);
        assert!(fundamental > 0.5, "note not audible: {fundamental}");
        for k in 2..=5 {
            let db = 20.0 * (goertzel(sustain, F0 * k as f32) / fundamental).log10();
            assert!(
                db < -90.0,
                "harmonic {k} at {db:.1} dBc - the output stage is colouring the signal"
            );
        }

        // A sine at sustain level must keep its own peak, i.e. nothing is
        // saturating it.
        let peak = sustain.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
        assert!(
            (peak - 0.7).abs() < 0.01,
            "sustain peak {peak} should be the 0.7 envelope level"
        );
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

    /// A CC module is a modulation source: its output must sit at zero until a
    /// controller moves, then swing symmetrically so it can brighten *and*
    /// darken a destination relative to the knob's own value.
    ///
    /// These assertions read the raw graph output rather than the mixed output:
    /// the master soft-clipper caps at full scale, so +2 octaves would arrive
    /// clamped to ~1.0 and the symmetry being tested would be destroyed.
    #[test]
    fn cc_node_is_centred_until_a_controller_moves() {
        let mut g = GraphEngine::new(SR);
        g.add_node(0, Kind::CC as u32);
        g.add_node(1, Kind::Mixer as u32);
        g.add_node(2, Kind::Out as u32);
        g.set_param(0, "depth", 2.0);
        g.connect(0, 0, 1, 0);
        g.connect(1, 0, 2, 0);

        // No controller message yet: no modulation.
        let mut rest = vec![0.0f32; 64];
        g.process_raw(&mut rest);
        assert!(
            rest.iter().all(|&s| s.abs() < 1e-7),
            "unpatched controller should not modulate"
        );

        // Full up -> +depth octaves, full down -> -depth octaves.
        g.set_cc(74, 127);
        let mut up = vec![0.0f32; 64];
        g.process_raw(&mut up);
        assert!(
            (up[0] - 2.0).abs() < 1e-6,
            "CC 127 should be +2 octaves, got {}",
            up[0]
        );

        g.set_cc(74, 0);
        let mut down = vec![0.0f32; 64];
        g.process_raw(&mut down);
        assert!(
            (down[0] + 2.0).abs() < 1e-6,
            "CC 0 should be -2 octaves, got {}",
            down[0]
        );

        // Centre of the controller travel is no modulation.
        g.set_cc(74, 64);
        let mut centre = vec![0.0f32; 64];
        g.process_raw(&mut centre);
        assert!(centre[0].abs() < 0.05, "mid CC should be near zero");

        // Depth scales the swing and can be closed entirely.
        g.set_param(0, "depth", 0.0);
        let mut closed = vec![0.0f32; 64];
        g.process_raw(&mut closed);
        assert!(closed[0].abs() < 1e-7, "depth 0 should mute modulation");
    }

    /// A controller must only reach the modules tuned to its own number, so two
    /// CC modules can read two different controllers without interfering.
    #[test]
    fn cc_nodes_only_respond_to_their_own_controller() {
        let mut g = GraphEngine::new(SR);
        g.add_node(0, Kind::CC as u32); // CC 74
        g.add_node(1, Kind::CC as u32); // CC 71
        g.add_node(2, Kind::Mixer as u32);
        g.add_node(3, Kind::Out as u32);
        g.set_param(0, "cc", 74.0);
        g.set_param(0, "depth", 1.0);
        g.set_param(1, "cc", 71.0);
        g.set_param(1, "depth", 1.0);
        g.connect(0, 0, 2, 0);
        g.connect(1, 0, 2, 1);
        g.connect(2, 0, 3, 0);

        g.set_cc(74, 127);
        g.set_cc(71, 0);

        // Both inputs are patched, so the mixer averages them: (+1 + -1) / 2.
        let mut buf = vec![0.0f32; 64];
        g.process(&mut buf);
        assert!(
            buf[0].abs() < 1e-6,
            "the two controllers should cancel, got {}",
            buf[0]
        );

        // Moving only CC 71 must move the mix.
        g.set_cc(71, 127);
        let mut buf2 = vec![0.0f32; 64];
        g.process(&mut buf2);
        assert!(buf2[0] > 0.9, "CC 71 had no effect: {}", buf2[0]);
    }
}
