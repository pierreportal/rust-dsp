//! Shared runtime patch-graph engine.
//!
//! This crate is the reason a patch behaves identically in the browser, the
//! desktop app, the Ableton plugin and the daisyseed firmware: there is exactly
//! one implementation of the synth, and every host embeds it.
//!
//! - [`graph::GraphEngine`] renders a single copy of a patch. One engine, one
//!   pitch: it is a *voice*.
//! - [`poly::PolyGraph`] is the polyphonic voice pool: `VOICES` identical graph
//!   copies, one per held note, with oldest-first stealing.
//! - [`registry`] describes the module catalogue (kinds, ports, params) so a
//!   host UI can draw its palette straight from the Rust definitions.
//!
//! Nothing here knows about wasm, audio devices or windowing. Hosts own that.
//!
//! Still `std`-only because the engine leans on `VecDeque`/`powf` and has
//! unchecked indexing on the audio thread. Both are tracked for cleanup before
//! the plugin and firmware targets.
pub mod graph;
pub mod poly;
pub mod registry;

pub use graph::{GraphEngine, Kind};
pub use poly::{PolyGraph, VOICES};
