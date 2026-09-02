//! Generates `module-registry.json` from the module catalogue in `registry.rs`.
//!
//! This is a native helper run at build/release time to produce the static JSON
//! that ships alongside the wasm pkg. The React UI imports it directly, so the
//! palette stays in sync with the Rust module definitions without a separate TS
//! mirror.
//!
//! Usage: `cargo run -p web --bin gen_registry -- <out-path>`

use std::env;
use std::fs;
use std::path::Path;
use web::registry::registry_json;

fn main() {
    let args: Vec<String> = env::args().collect();
    let out = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("module-registry.json");
    if let Some(parent) = Path::new(out).parent() {
        fs::create_dir_all(parent).expect("create output dir");
    }
    fs::write(out, registry_json()).expect("write registry json");
    eprintln!("wrote {}", out);
}
