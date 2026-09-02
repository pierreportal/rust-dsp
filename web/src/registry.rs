//! Runtime module registry: the single source of truth for the Node module
//! catalogue that the web UI discovers at runtime.
//!
//! The React client used to hardcode kind codes, port layouts and parameter
//! specs in a TS file (`nodeSpec.ts`) that had to be manually kept in sync
//! with this crate. Instead, that metadata is now produced here — straight
//! from the `Kind`/`Params` definitions in `graph.rs` — and serialized to
//! JSON via the `wasm_bindgen` export `registry_json_js()`. The UI fetches it
//! once (bundled in the release artifact), so adding a module on the Rust
//! side is all that's needed for it to appear in the palette.

use crate::graph::Kind;

/// UI-facing parameter descriptor (mirrors the previous `ParamSpec` shape so
/// the React controls can render unchanged).
pub struct ParamSpec {
    pub name: &'static str,
    pub label: &'static str,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub default: f32,
}

/// A colour for each module kind, as served to the UI palette/canvas.
fn color(kind: Kind) -> &'static str {
    match kind {
        Kind::Osc => "#3b82f6",
        Kind::Adsr => "#a855f7",
        Kind::Filter => "#06b6d4",
        Kind::Distortion => "#f97316",
        Kind::Vca => "#22c55e",
        Kind::Mixer => "#eab308",
        Kind::Constant => "#64748b",
        Kind::Out => "#ef4444",
        Kind::Midi => "#ec4899",
    }
}

/// The palette ordering for the UI palette.
fn palette_order() -> &'static [Kind] {
    &[
        Kind::Midi,
        Kind::Osc,
        Kind::Adsr,
        Kind::Filter,
        Kind::Distortion,
        Kind::Vca,
        Kind::Mixer,
        Kind::Constant,
        Kind::Out,
    ]
}

fn osc_params() -> &'static [ParamSpec] {
    &[
        ParamSpec { name: "freq", label: "freq (Hz)", min: 20.0, max: 4000.0, step: 1.0, default: 220.0 },
        ParamSpec { name: "waveform", label: "wave (0-4)", min: 0.0, max: 4.0, step: 1.0, default: 1.0 },
        ParamSpec { name: "pulseWidth", label: "pulse width", min: 0.05, max: 0.95, step: 0.01, default: 0.5 },
    ]
}

fn adsr_params() -> &'static [ParamSpec] {
    &[
        ParamSpec { name: "attack", label: "attack (s)", min: 0.001, max: 2.0, step: 0.001, default: 0.01 },
        ParamSpec { name: "decay", label: "decay (s)", min: 0.001, max: 2.0, step: 0.001, default: 0.15 },
        ParamSpec { name: "sustain", label: "sustain", min: 0.0, max: 1.0, step: 0.01, default: 0.7 },
        ParamSpec { name: "release", label: "release (s)", min: 0.001, max: 3.0, step: 0.001, default: 0.3 },
    ]
}

fn filter_params() -> &'static [ParamSpec] {
    &[
        ParamSpec { name: "cutoff", label: "cutoff (Hz)", min: 40.0, max: 12000.0, step: 1.0, default: 1200.0 },
        ParamSpec { name: "resonance", label: "resonance", min: 0.05, max: 1.0, step: 0.01, default: 0.2 },
    ]
}

fn distortion_params() -> &'static [ParamSpec] {
    &[ParamSpec { name: "drive", label: "drive", min: 1.0, max: 30.0, step: 0.1, default: 4.0 }]
}

const EMPTY: &[ParamSpec] = &[];

fn params(kind: Kind) -> &'static [ParamSpec] {
    match kind {
        Kind::Osc => osc_params(),
        Kind::Adsr => adsr_params(),
        Kind::Filter => filter_params(),
        Kind::Distortion => distortion_params(),
        Kind::Vca => EMPTY,
        Kind::Mixer => EMPTY,
        Kind::Constant => &[
            ParamSpec { name: "value", label: "value", min: -2.0, max: 2.0, step: 0.01, default: 0.5 },
        ],
        Kind::Out => EMPTY,
        Kind::Midi => EMPTY,
    }
}

fn label(kind: Kind) -> &'static str {
    match kind {
        Kind::Osc => "Oscillator",
        Kind::Adsr => "ADSR",
        Kind::Filter => "Filter (SVF)",
        Kind::Distortion => "Distortion",
        Kind::Vca => "VCA",
        Kind::Mixer => "Mixer",
        Kind::Constant => "Constant",
        Kind::Out => "Output",
        Kind::Midi => "MIDI / CV",
    }
}

/// JSON document describing every module in the catalogue. One object per kind
/// keyed by kind code, plus a top-level `order` array for palette ordering:
///
/// ```json
/// {
///   "order": [9, 0, 1, ...],
///   "modules": {
///     "0": { "kind": 0, "label": "...", "color": "...",
///            "inputs": ["freq cv"], "outputs": ["signal"],
///            "params": [{ "name": "...", "label": "...", "min": 0, "max": 1, "step": 0.1, "default": 0.5 }] }
///   }
/// }
/// ```
pub fn registry_json() -> String {
    let mut out = String::from("{\"order\":[");
    let order = palette_order();
    for (i, &k) in order.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&(k as u32).to_string());
    }
    out.push_str("],\"modules\":{");
    let mut first = true;
    for &k in order {
        if !first {
            out.push(',');
        }
        first = false;
        let inputs = k.inputs().iter().map(|s| format!("\"{s}\"")).collect::<Vec<_>>().join(",");
        let outputs = k.outputs().iter().map(|s| format!("\"{s}\"")).collect::<Vec<_>>().join(",");
        let ps = params(k);
        let mut pj = String::new();
        for (j, p) in ps.iter().enumerate() {
            if j > 0 {
                pj.push(',');
            }
            pj.push_str(&format!(
                "{{\"name\":\"{}\",\"label\":\"{}\",\"min\":{},\"max\":{},\"step\":{},\"default\":{}}}",
                p.name, p.label, p.min, p.max, p.step, p.default
            ));
        }
        out.push_str(&format!(
            "\"{}\":{{\"kind\":{},\"label\":\"{}\",\"color\":\"{}\",\"inputs\":[{}],\"outputs\":[{}],\"params\":[{}]}}",
            k as u32, k as u32, label(k), color(k), inputs, outputs, pj
        ));
    }
    out.push_str("}}");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_valid_json() {
        let s = registry_json();
        let _val: serde_json::Value =
            serde_json::from_str(&s).expect("registry_json must be valid JSON");
    }

    #[test]
    fn every_palette_kind_appears_in_modules() {
        let s = registry_json();
        let val: serde_json::Value = serde_json::from_str(&s).unwrap();
        let modules = val["modules"].as_object().unwrap();
        let order = val["order"].as_array().unwrap();
        assert_eq!(modules.len(), order.len(), "every module in order must exist");
        for k in order {
            let key = k.as_u64().unwrap().to_string();
            assert!(modules.contains_key(&key), "module {key} missing from modules");
        }
    }
}
