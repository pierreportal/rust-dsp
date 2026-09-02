//! WebAssembly bindings for the rust-dsp modular patch graph.
//!
//! Compiled with `wasm-pack build --target web`. The `Graph` type is driven
//! from an AudioWorklet processor (see `web/ui/public/graph-processor.js`);
//! the React UI mirrors its node/edge state into the worklet via messages.
mod graph;

use graph::GraphEngine;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Graph {
    eng: GraphEngine,
}

#[wasm_bindgen]
impl Graph {
    #[wasm_bindgen(constructor)]
    pub fn new(sample_rate: f32) -> Graph {
        Graph {
            eng: GraphEngine::new(sample_rate),
        }
    }

    /// Add a node of `kind` at the given (UI-assigned) numeric id.
    /// Kind codes: 0 Osc, 1 Adsr, 2 Filter, 3 Distortion, 4 Vca, 5 Mixer,
    /// 7 Constant, 8 Out, 9 Midi. Returns false on unknown kind.
    pub fn add_node(&mut self, id: u32, kind: u32) -> bool {
        self.eng.add_node(id, kind)
    }

    pub fn remove_node(&mut self, id: u32) {
        self.eng.remove_node(id);
    }

    /// Connect an output port to an input port. Returns false on a cycle
    /// (the edge is not added).
    pub fn connect(&mut self, from: u32, from_port: u32, to: u32, to_port: u32) -> bool {
        self.eng.connect(from, from_port, to, to_port)
    }

    pub fn disconnect(&mut self, from: u32, from_port: u32, to: u32, to_port: u32) {
        self.eng.disconnect(from, from_port, to, to_port);
    }

    pub fn set_param(&mut self, id: u32, name: &str, value: f32) {
        self.eng.set_param(id, name, value);
    }

    /// Drive a Midi node (id) with a note-on.
    pub fn note_on(&mut self, id: u32, note: u8, vel: u8) {
        self.eng.note_on(id, note, vel);
    }

    pub fn note_off(&mut self, id: u32) {
        self.eng.note_off(id);
    }

    /// Render `out.len()` samples into `out`. Called once per audio block.
    pub fn process(&mut self, out: &mut [f32]) {
        self.eng.process(out);
    }
}
