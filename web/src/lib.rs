//! WebAssembly bindings for the shared Coarse patch graph.
//!
//! Compiled with `wasm-pack build --target web`. The `Graph` type is driven
//! from an AudioWorklet processor (see `web/ui/public/graph-processor.js`);
//! the React UI mirrors its node/edge state into the worklet via messages.
//!
//! There is deliberately no engine code here: the graph lives in the `graph`
//! crate so the desktop app, the Ableton plugin and the firmware all run the
//! exact same synth. This crate only translates between wasm-bindgen and that
//! shared API.
use graph::PolyGraph;
use wasm_bindgen::prelude::*;

/// Serialized module catalogue (kind codes, ports, params). The web UI fetches
/// this to render the palette, so new modules added in Rust appear there
/// without any TS-side changes.
#[wasm_bindgen]
pub fn registry_json_js() -> String {
    graph::registry::registry_json()
}

/// A polyphonic patch graph: a fixed pool of `VOICES` identical graph copies,
/// one per held note. Structural mutations are broadcast to every voice; a
/// note-on is routed to a single voice so each note has its own pitch.
#[wasm_bindgen]
pub struct Graph {
    eng: PolyGraph,
}

#[wasm_bindgen]
impl Graph {
    #[wasm_bindgen(constructor)]
    pub fn new(sample_rate: f32) -> Graph {
        Graph {
            eng: PolyGraph::new(sample_rate),
        }
    }

    /// Add a node of `kind` at the given (UI-assigned) numeric id.
    /// Returns false on unknown kind.
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

    /// Play a note: allocated to one voice, which drives every Midi node in
    /// that voice's copy of the graph.
    pub fn note_on(&mut self, note: u8, vel: u8) {
        self.eng.note_on(note, vel);
    }

    /// Release the voice (if any) currently holding `note`.
    pub fn note_off(&mut self, note: u8) {
        self.eng.note_off(note);
    }

    /// Render `out.len()` samples into `out`. Called once per audio block.
    pub fn process(&mut self, out: &mut [f32]) {
        self.eng.process(out);
    }

    /// Feed a MIDI controller value (0-127) to every Controller module tuned to
    /// `cc`. Controllers are patchable sources, so the UI routes these
    /// unconditionally rather than picking a voice.
    pub fn set_cc(&mut self, cc: u8, value: u8) {
        self.eng.set_cc(cc, value);
    }
}