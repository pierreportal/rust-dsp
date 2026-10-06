//! The Coarse patch format, version 1.
//!
//! A patch is the *whole* instrument: a set of nodes (id, kind, position,
//! parameters) and a set of cables (which output port of which node feeds which
//! input port of which node). It is the only thing shared between the browser,
//! the desktop app, the plugin and the firmware, which is what makes a shared
//! link portable by construction.
//!
//! On the wire it is a small JSON document, base64url-encoded into the `p`
//! query parameter of a share link:
//!
//! ```text
//! #p=eyJ2IjoxLCJuIjpbWzAsMTAsMCwwLHsiZnJlcSI6NDQwfV1dfQ
//! {"v":1,"n":[[0,10,0,0,{"freq":440}]],"e":[]}
//! ```
//!
//! This mirrors `rust_dsp_web/src/patch/patchCodec.ts`; the two are expected to
//! stay byte-compatible, and `tests::matches_the_typescript_encoding` pins the
//! shape against the same fixtures the browser uses.
//!
//! MIDI mappings are ordinary `Controller` nodes and ordinary cables, so adding
//! a new modulation source needs no new patch version.

use graph::{registry, Kind};
use serde_json::Value;
use std::collections::HashMap;

/// The only patch version this build understands.
pub const PATCH_VERSION: u32 = 1;

/// Whether decoded parameters are clamped to the module registry's declared
/// range. Matches the browser codec, so the two hosts repair a bad value the
/// same way.
const CLAMP_PARAMS: bool = true;

/// Largest node id accepted, to keep a hostile link from allocating wildly.
pub const MAX_NODE_ID: u32 = 1 << 20;

/// One module in the patch.
#[derive(Clone, Debug, PartialEq)]
pub struct PatchNode {
    pub id: u32,
    pub kind: u32,
    pub x: f32,
    pub y: f32,
    pub params: HashMap<String, f32>,
}

/// One cable: `source`'s `source_port` output feeds `target`'s `target_port` input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatchEdge {
    pub source: u32,
    pub source_port: u32,
    pub target: u32,
    pub target_port: u32,
}

/// A complete instrument.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Patch {
    pub nodes: Vec<PatchNode>,
    pub edges: Vec<PatchEdge>,
}

/// Outcome of decoding, including anything repaired along the way.
#[derive(Clone, Debug, Default)]
pub struct Decoded {
    pub patch: Patch,
    /// Non-fatal problems: unknown kinds, dropped cables, cycles, out-of-range
    /// parameters. Surfaced in the UI so a bad link never fails silently.
    pub warnings: Vec<String>,
}

fn decode_base64url(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    // The TS side emits unpadded base64url; accept padding too.
    let mut owned = s.replace('-', "+").replace('_', "/");
    while !owned.len().is_multiple_of(4) {
        owned.push('=');
    }
    base64::engine::general_purpose::STANDARD.decode(owned).ok()
}

/// Read a number as the `f32` the engine actually stores.
///
/// serde_json parses into `f64`, and widening `0.15f32` to `f64` and back is
/// lossy: it lands on 0.15000000596046448. Left unchecked that drift rewrites
/// every parameter of a shared link and changes its hash, so every decoded
/// number is rounded back through `f32` here, once, at the boundary.
fn finite(v: Option<&Value>) -> Option<f32> {
    match v {
        Some(Value::Number(n)) => n
            .as_f64()
            .map(|f| f as f32) // round-trip through the engine's own precision
            .filter(|f| f.is_finite()),
        _ => None,
    }
}

/// Fill a node's parameters from the module registry: every parameter the module
/// declares gets a value, using the wire value where present and valid and the
/// declared default otherwise.
///
/// This is what makes a patch forward and backward tolerant: a link saved before
/// a module grew a parameter still loads, and one naming a parameter that no
/// longer exists loses only that value.
///
/// `clamp` bounds are a decision, not an accident: it must match the browser's
/// codec exactly. A shared link is a hash, so if the two hosts disagree about
/// whether `-1234.5` is a legal cutoff, the same link stops round-tripping and
/// every save from one app silently changes the patch in the other.
fn resolve_params(
    kind: u32,
    raw: Option<&Value>,
    warnings: &mut Vec<String>,
    clamp: bool,
) -> HashMap<String, f32> {
    let mut out = HashMap::new();
    let kind = match Kind::from_u8(kind) {
        Some(k) => k,
        None => return out,
    };
    let specs = registry::params(kind);
    let provided = match raw {
        Some(Value::Object(map)) => Some(map),
        _ => None,
    };
    if let Some(map) = provided {
        // An empty object is simply "no overrides", not a mistake; only complain
        // when a value was actually sent for a module that takes none.
        if !map.is_empty() && specs.is_empty() {
            warnings.push(format!(
                "module kind {} takes no parameters, but the patch supplied some",
                kind as u8
            ));
        }
    }
    for spec in specs {
        let given = provided.and_then(|m| finite(m.get(spec.name)));
        let mut value = given.unwrap_or(spec.default);
        if clamp && value < spec.min {
            value = spec.min;
            warnings.push(format!(
                "parameter {} clamped to its minimum ({})",
                spec.name, spec.min
            ));
        } else if clamp && value > spec.max {
            value = spec.max;
            warnings.push(format!(
                "parameter {} clamped to its maximum ({})",
                spec.name, spec.max
            ));
        }
        out.insert(spec.name.to_string(), value);
    }
    if let Some(map) = provided {
        for key in map.keys() {
            if !specs.iter().any(|s| s.name == key) {
                warnings.push(format!("ignored unknown parameter \"{key}\""));
            }
        }
    }
    out
}

/// Decode a `#p=` patch code.
///
/// Every stage is defensive: an unreadable document yields an empty patch with a
/// warning rather than an error, because a half-loaded instrument the user can
/// see and fix is better than a dialog.
pub fn decode(code: &str) -> Decoded {
    let raw = code.trim();
    if raw.is_empty() {
        return Decoded {
            patch: Patch::default(),
            warnings: vec!["empty link".to_string()],
        };
    }

    let bytes = match decode_base64url(raw) {
        Some(b) => b,
        None => {
            return Decoded {
                patch: Patch::default(),
                warnings: vec!["link is not valid base64url".to_string()],
            }
        }
    };
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(doc) => decode_document(&doc),
        Err(e) => Decoded {
            patch: Patch::default(),
            warnings: vec![format!("link is not valid JSON: {e}")],
        },
    }
}

/// Validate and repair an already-parsed patch document.
fn decode_document(doc: &Value) -> Decoded {
    let mut warnings = Vec::new();
    let version = doc.get("v").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    if version != PATCH_VERSION {
        return Decoded {
            patch: Patch::default(),
            warnings: vec![format!("unsupported patch version {version}")],
        };
    }

    let mut patch = Patch::default();
    let empty = Vec::new();
    let raw_nodes = doc.get("n").and_then(|v| v.as_array()).unwrap_or(&empty);

    let mut seen_ids = std::collections::HashSet::new();
    for entry in raw_nodes {
        let arr = match entry.as_array() {
            Some(a) if a.len() >= 5 => a,
            _ => {
                warnings.push("skipped a malformed node entry".to_string());
                continue;
            }
        };
        let id = match arr[0].as_u64() {
            Some(i) if i < MAX_NODE_ID as u64 => i as u32,
            _ => {
                warnings.push("skipped a node with an invalid id".to_string());
                continue;
            }
        };
        if !seen_ids.insert(id) {
            warnings.push(format!("skipped duplicate node id {id}"));
            continue;
        }
        let kind = match arr[1].as_u64() {
            Some(k) if Kind::from_u8(k as u32).is_some() => k as u32,
            Some(k) => {
                warnings.push(format!("skipped unknown module kind {k} (id {id})"));
                continue;
            }
            None => {
                warnings.push(format!("skipped node {id} with an invalid kind"));
                continue;
            }
        };
        let x = finite(Some(&arr[2])).unwrap_or(0.0);
        let y = finite(Some(&arr[3])).unwrap_or(0.0);
        let params = resolve_params(kind, Some(&arr[4]), &mut warnings, CLAMP_PARAMS);
        patch.nodes.push(PatchNode {
            id,
            kind,
            x,
            y,
            params,
        });
    }

    // Cables are validated against the surviving nodes and the module registry,
    // so a link that references a dropped node cannot produce a dangling edge.
    let known: HashMap<u32, Kind> = patch
        .nodes
        .iter()
        .map(|n| (n.id, Kind::from_u8(n.kind).expect("checked above")))
        .collect();
    let raw_edges = doc.get("e").and_then(|v| v.as_array()).unwrap_or(&empty);
    for entry in raw_edges {
        let arr = match entry.as_array() {
            Some(a) if a.len() >= 4 => a,
            _ => {
                warnings.push("skipped a malformed cable".to_string());
                continue;
            }
        };
        let nums: Vec<Option<u64>> = arr.iter().take(4).map(|v| v.as_u64()).collect();
        let (source, source_port, target, target_port) = match nums[..] {
            [Some(a), Some(b), Some(c), Some(d)] => (a as u32, b as u32, c as u32, d as u32),
            _ => {
                warnings.push("skipped a cable with invalid endpoints".to_string());
                continue;
            }
        };
        let (from, to) = match (known.get(&source), known.get(&target)) {
            (Some(f), Some(t)) => (*f, *t),
            _ => {
                warnings.push(format!(
                    "dropped cable {source}:{source_port} -> {target}:{target_port} (missing endpoint)"
                ));
                continue;
            }
        };
        if source_port as usize >= from.outputs().len() {
            warnings.push(format!(
                "dropped cable from unknown output port {source_port}"
            ));
            continue;
        }
        if target_port as usize >= to.inputs().len() {
            warnings.push(format!(
                "dropped cable into unknown input port {target_port}"
            ));
            continue;
        }
        patch.edges.push(PatchEdge {
            source,
            source_port,
            target,
            target_port,
        });
    }

    patch.edges = drop_cycles(patch.edges, &mut warnings);
    Decoded { patch, warnings }
}

/// Remove cables that close a feedback loop, which the engine cannot render.
///
/// A signal graph is a *directed* acyclic graph, so this has to test
/// reachability in the signal direction rather than plain connectivity: two
/// modules both feeding one VCA is ordinary summing, not a feedback loop. An
/// edge `s -> t` is rejected only when `t` can already reach `s`, which is
/// exactly the case the engine's topological sort would choke on.
fn drop_cycles(edges: Vec<PatchEdge>, warnings: &mut Vec<String>) -> Vec<PatchEdge> {
    // Adjacency of the cables accepted so far.
    let mut out: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut kept = Vec::with_capacity(edges.len());

    for edge in edges {
        if reaches(&out, edge.target, edge.source) {
            warnings.push(format!(
                "dropped cable {}:{} -> {}:{} to break a feedback loop",
                edge.source, edge.source_port, edge.target, edge.target_port
            ));
            continue;
        }
        out.entry(edge.source).or_default().push(edge.target);
        kept.push(edge);
    }
    kept
}

/// Is `target` reachable from `start` following `out` edges?
fn reaches(out: &HashMap<u32, Vec<u32>>, start: u32, target: u32) -> bool {
    if start == target {
        return true;
    }
    let mut stack = vec![start];
    let mut seen = std::collections::HashSet::new();
    seen.insert(start);
    while let Some(node) = stack.pop() {
        for &next in out.get(&node).map(|v| v.as_slice()).unwrap_or(&[]) {
            if next == target {
                return true;
            }
            if seen.insert(next) {
                stack.push(next);
            }
        }
    }
    false
}

/// Serialise a node's parameters in the module registry's declaration order.
///
/// Order is part of the wire format, not a cosmetic detail: a share code gets
/// compared and hashed, so a host that emits the same values in a different
/// order produces a different code and makes every save look like an edit.
/// `params` is a `HashMap`, so without this the output order is whatever the
/// hash function happened to produce, varying between runs of the same binary.
/// The browser writes registry order, so this does too; a parameter the registry
/// no longer declares is appended in sorted order so encoding stays
/// deterministic for a hand-built patch.
fn ordered_params(kind: u32, params: &HashMap<String, f32>) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    if let Some(kind) = Kind::from_u8(kind) {
        for spec in registry::params(kind) {
            if let Some(value) = params.get(spec.name) {
                out.insert(spec.name.to_string(), json_f32(*value));
            }
        }
    }
    let mut extra: Vec<(&String, &f32)> = params
        .iter()
        .filter(|(name, _)| !out.contains_key(*name))
        .collect();
    extra.sort_by(|a, b| a.0.cmp(b.0));
    for (name, value) in extra {
        out.insert(name.clone(), json_f32(*value));
    }
    out
}

/// Encode a patch back into a share code. Round-trips through [`decode`].
pub fn encode(patch: &Patch) -> String {
    use base64::Engine;
    let nodes: Vec<Value> = patch
        .nodes
        .iter()
        .map(|n| {
            serde_json::json!([
                n.id,
                n.kind,
                json_f32(n.x),
                json_f32(n.y),
                ordered_params(n.kind, &n.params)
            ])
        })
        .collect();
    let edges: Vec<Value> = patch
        .edges
        .iter()
        .map(|e| serde_json::json!([e.source, e.source_port, e.target, e.target_port]))
        .collect();
    let doc = serde_json::json!({ "v": PATCH_VERSION, "n": nodes, "e": edges });
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(&doc).unwrap())
}

/// Decode a patch from the readable JSON form, as written by [`to_json`].
///
/// Shares [`decode`]'s repair behaviour, so a hand-edited or exported file gets
/// the same validation as a shared link: unknown modules dropped, cables
/// repaired, parameters filled in from the registry.
pub fn from_json(text: &str) -> Decoded {
    match serde_json::from_str::<Value>(text) {
        Ok(doc) => decode_document(&doc),
        Err(e) => Decoded {
            patch: Patch::default(),
            warnings: vec![format!("not valid patch JSON: {e}")],
        },
    }
}

/// The patch as readable JSON, for saving to a file or inspecting in a repo.
pub fn to_json(patch: &Patch) -> String {
    let nodes: Vec<Value> = patch
        .nodes
        .iter()
        .map(|n| {
            serde_json::json!([
                n.id,
                n.kind,
                json_f32(n.x),
                json_f32(n.y),
                ordered_params(n.kind, &n.params)
            ])
        })
        .collect();
    let edges: Vec<Value> = patch
        .edges
        .iter()
        .map(|e| serde_json::json!([e.source, e.source_port, e.target, e.target_port]))
        .collect();
    serde_json::json!({ "v": PATCH_VERSION, "n": nodes, "e": edges }).to_string()
}

/// Whole numbers are emitted without a decimal point so a port/param of 2 does
/// not become 2.0 in the shared link; the TS codec does the same.
fn json_f32(v: f32) -> Value {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        Value::from(v as i64)
    } else {
        // Widen through the shortest decimal that round-trips as `f32`.
        //
        // serde_json stores numbers as `f64`, so handing it an `f32` widens
        // 0.01f32 to 0.009999999776482582 and prints every digit of it. A share
        // code is hashed and compared, so those extra digits change the code
        // and make identical patches look different. The browser's numbers are
        // already `f64` and print as "0.01"; formatting the `f32` directly
        // yields the same digits, which is also what the engine stores.
        match serde_json::from_str(&format!("{v}")) {
            Ok(value) => value,
            // Only reachable for NaN and the infinities, which decoding
            // rejects anyway; there is no shorter spelling of them to use.
            Err(_) => Value::from(v),
        }
    }
}

/// Pull the `p=` code out of a share URL or a bare code.
///
/// Accepts a full `...#p=CODE`, a `?p=CODE` link, or the code on its own, so
/// pasting whatever the user copied off the share button always works.
pub fn code_from_link(input: &str) -> Option<&str> {
    let trimmed = input.trim();
    for marker in ["#p=", "?p=", "p="] {
        if let Some(idx) = trimmed.find(marker) {
            let rest = &trimmed[idx + marker.len()..];
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
                .unwrap_or(rest.len());
            let code = &rest[..end];
            if !code.is_empty() {
                return Some(code);
            }
        }
    }
    // A bare code, which is what "copy the code" gives you. Only accept it if it
    // really looks like one token of base64url, so a sentence of prose does not
    // get treated as a patch.
    let is_bare_code = !trimmed.is_empty()
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    is_bare_code.then_some(trimmed)
}

impl Patch {
    /// Every `(node id, parameter, value)` in the patch, for applying it to an
    /// engine. Iteration order is unspecified; callers that need determinism
    /// should sort nodes by id first.
    pub fn params_by_id(&self) -> Vec<(u32, String, f32)> {
        self.nodes
            .iter()
            .flat_map(|n| {
                n.params
                    .iter()
                    .map(move |(name, value)| (n.id, name.clone(), *value))
            })
            .collect()
    }

    /// A node by id.
    pub fn node(&self, id: u32) -> Option<&PatchNode> {
        self.nodes.iter().find(|n| n.id == id)
    }
}

/// The patch a fresh install starts with: a playable one-oscillator voice.
///
/// It is a real graph rather than a special case in the engine, so the very
/// first thing a user hears is the same code path as any shared link.
pub fn default_patch() -> Patch {
    let node = |id: u32, kind: Kind, x: f32, y: f32, params: &[(&str, f32)]| PatchNode {
        id,
        kind: kind as u32,
        x,
        y,
        params: params.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
    };
    Patch {
        nodes: vec![
            node(0, Kind::Midi, 0.0, 0.0, &[]),
            node(1, Kind::SineOsc, 240.0, 0.0, &[("freq", 220.0)]),
            node(
                2,
                Kind::Adsr,
                0.0,
                180.0,
                // Spelled out rather than left to the registry defaults, so the
                // shipped patch is stable even if those defaults ever change.
                &[
                    ("attack", 0.01),
                    ("decay", 0.15),
                    ("sustain", 0.7),
                    ("release", 0.3),
                ],
            ),
            node(3, Kind::Vca, 240.0, 180.0, &[]),
            node(4, Kind::Out, 480.0, 90.0, &[]),
        ],
        edges: vec![
            // Midi pitch -> oscillator freq cv
            PatchEdge {
                source: 0,
                source_port: 1,
                target: 1,
                target_port: 0,
            },
            // Midi gate -> envelope
            PatchEdge {
                source: 0,
                source_port: 0,
                target: 2,
                target_port: 0,
            },
            // Oscillator -> VCA signal
            PatchEdge {
                source: 1,
                source_port: 0,
                target: 3,
                target_port: 0,
            },
            // Envelope -> VCA cv
            PatchEdge {
                source: 2,
                source_port: 0,
                target: 3,
                target_port: 1,
            },
            // VCA -> Out
            PatchEdge {
                source: 3,
                source_port: 0,
                target: 4,
                target_port: 0,
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A link produced by the TypeScript codec, kept here so the two encoders
    /// cannot drift apart without a test failing.
    const TS_FIXTURE: &str = "eyJ2IjoxLCJuIjpbWzAsMTAsMCwwLHsiZnJlcSI6NDQwfV1dLCJlIjpbXX0";

    #[test]
    fn matches_the_typescript_encoding() {
        let mut patch = Patch::default();
        patch.nodes.push(PatchNode {
            id: 0,
            kind: 10,
            x: 0.0,
            y: 0.0,
            params: [("freq".to_string(), 440.0)].into_iter().collect(),
        });
        // Byte-identical to the TypeScript codec's output for the same patch,
        // which is what makes a link built on either side interchangeable.
        assert_eq!(encode(&patch), TS_FIXTURE);

        let decoded = decode(TS_FIXTURE);
        assert!(
            decoded.warnings.is_empty(),
            "warnings: {:?}",
            decoded.warnings
        );
        assert_eq!(decoded.patch.nodes.len(), 1);
        assert_eq!(decoded.patch.nodes[0].id, 0);
        assert_eq!(decoded.patch.nodes[0].kind, 10);
        assert_eq!(
            decoded.patch.nodes[0].params.get("freq").copied(),
            Some(440.0)
        );
    }

    /// Fractional parameters survive the f32 -> f64 -> text trip with the digits
    /// a person wrote, not the binary tail of the approximation.
    ///
    /// serde_json formats numbers as `f64`, so `0.01f32` used to come out as
    /// 0.009999999776482582. A share code is hashed and compared, so those extra
    /// digits made the same patch encode differently in each host and turned
    /// every load-save into a spurious change.
    #[test]
    fn fractional_parameters_encode_with_the_digits_they_were_given() {
        let mut patch = Patch::default();
        patch.nodes.push(PatchNode {
            id: 0,
            kind: 1,
            x: 0.0,
            y: 0.0,
            params: [
                ("attack".to_string(), 0.01),
                ("decay".to_string(), 0.15),
                ("sustain".to_string(), 0.7),
                ("release".to_string(), 0.3),
            ]
            .into_iter()
            .collect(),
        });

        let json = to_json(&patch);
        assert!(
            json.contains(r#""attack":0.01"#)
                && json.contains(r#""decay":0.15"#)
                && json.contains(r#""sustain":0.7"#)
                && json.contains(r#""release":0.3"#),
            "expected the written digits in {json}"
        );

        let decoded = decode(&encode(&patch));
        assert_eq!(decoded.patch.nodes[0].params["attack"], 0.01);
        assert_eq!(decoded.patch.nodes[0].params["decay"], 0.15);
    }

    /// Encoding is a pure function of the patch, not of hash iteration order.
    ///
    /// `params` is a `HashMap`, so serialising it directly emitted keys in
    /// whatever order the hash function produced, which Rust randomises per
    /// process. The same patch could encode to two different share codes in two
    /// runs of the same binary; the browser always writes registry order.
    #[test]
    fn encoding_is_deterministic_across_runs() {
        let mut patch = Patch::default();
        patch.nodes.push(PatchNode {
            id: 0,
            kind: 13,
            x: 0.0,
            y: 0.0,
            // Alphabetically ordered, so only the registry's declaration order
            // can produce this exact string.
            params: [("depth".to_string(), 2.0), ("cc".to_string(), 74.0)]
                .into_iter()
                .collect(),
        });

        assert_eq!(
            encode(&patch),
            // {"v":1,"n":[[0,13,0,0,{"cc":74,"depth":2}]],"e":[]}
            "eyJ2IjoxLCJuIjpbWzAsMTMsMCwwLHsiY2MiOjc0LCJkZXB0aCI6Mn1dXSwiZSI6W119",
        );
        assert_eq!(
            to_json(&patch),
            r#"{"v":1,"n":[[0,13,0,0,{"cc":74,"depth":2}]],"e":[]}"#
        );
    }

    /// Links produced by the running web app (`encodePatch` in
    /// `rust_dsp_web/src/patch/patchCodec.ts`), including its real hash shape.
    ///
    /// Captured rather than hand-written so the fixture cannot drift: these are
    /// verbatim outputs of the TypeScript codec.
    const BROWSER_LINKS: &[(&str, &str)] = &[
        // Single Sine module.
        (
            "eyJ2IjoxLCJuIjpbWzAsMTAsMCwwLHsiZnJlcSI6NDQwfV1dLCJlIjpbXX0",
            r#"{"v":1,"n":[[0,10,0,0,{"freq":440}]],"e":[]}"#,
        ),
        // A Controller node patched into an oscillator's freq cv: the CC
        // mapping, expressed purely as a node and a cable.
        (
            "eyJ2IjoxLCJuIjpbWzAsMTMsMCwwLHsiY2MiOjc0LCJkZXB0aCI6Mn1dLFsxLDEwLDI0MCwwLHsiZnJlcSI6MjIwfV1dLCJlIjpbWzAsMCwxLDBdXX0",
            r#"{"v":1,"n":[[0,13,0,0,{"cc":74,"depth":2}],[1,10,240,0,{"freq":220}]],"e":[[0,0,1,0]]}"#,
        ),
    ];

    #[test]
    fn a_link_from_the_web_app_decodes_here() {
        for (code, expected_json) in BROWSER_LINKS {
            let decoded = decode(code);
            assert!(
                decoded.warnings.is_empty(),
                "browser link {code} produced warnings: {:?}",
                decoded.warnings
            );
            // Re-encoding must reproduce the browser's bytes exactly, or a patch
            // shared from the web would change the moment it is saved here.
            assert_eq!(
                &encode(&decoded.patch),
                code,
                "re-encoding a browser link changed it (expected {expected_json})"
            );
        }
    }

    #[test]
    fn a_controller_mapping_in_a_link_survives_into_a_working_engine() {
        // The end-to-end promise: a mapping authored in the browser arrives in
        // the desktop app already wired up, with no extra mapping table.
        let (code, _) = BROWSER_LINKS[1];
        let decoded = decode(code);
        let controller = decoded.patch.node(0).expect("controller node");
        assert_eq!(controller.kind, Kind::CC as u32);
        assert_eq!(controller.params.get("cc").copied(), Some(74.0));
        assert_eq!(controller.params.get("depth").copied(), Some(2.0));
        assert_eq!(decoded.patch.edges.len(), 1);
        assert_eq!(decoded.patch.edges[0].target_port, 0);
    }

    #[test]
    fn a_default_patch_decodes_to_itself() {
        let patch = default_patch();
        let decoded = decode(&encode(&patch));
        assert!(
            decoded.warnings.is_empty(),
            "warnings: {:?}",
            decoded.warnings
        );
        assert_eq!(decoded.patch, patch);
    }

    #[test]
    fn missing_parameters_are_filled_from_the_registry() {
        // A patch saved before the envelope grew `sustain` still loads, and
        // picks up the declared default rather than a silent zero.
        let code = encode(&Patch {
            nodes: vec![PatchNode {
                id: 0,
                kind: 1,
                x: 0.0,
                y: 0.0,
                params: HashMap::new(),
            }],
            edges: vec![],
        });
        let decoded = decode(&code);
        let params = &decoded.patch.nodes[0].params;
        assert_eq!(params.get("attack").copied(), Some(0.01));
        assert_eq!(params.get("sustain").copied(), Some(0.7));
        assert_eq!(params.get("release").copied(), Some(0.3));
    }

    #[test]
    fn out_of_range_parameters_are_clamped_not_rejected() {
        let code = encode(&Patch {
            nodes: vec![PatchNode {
                id: 0,
                kind: 13,
                x: 0.0,
                y: 0.0,
                // cc above its declared maximum of 127
                params: [("cc".to_string(), 4000.0)].into_iter().collect(),
            }],
            edges: vec![],
        });
        let decoded = decode(&code);
        // Clamped to the registry's declared maximum for this parameter.
        assert_eq!(
            decoded.patch.nodes[0].params.get("cc").copied(),
            Some(127.0)
        );
        assert!(!decoded.warnings.is_empty(), "clamp should be reported");
    }

    #[test]
    fn a_dangling_cable_is_dropped_but_the_nodes_survive() {
        let code = encode(&Patch {
            nodes: vec![PatchNode {
                id: 0,
                kind: 8,
                x: 0.0,
                y: 0.0,
                params: HashMap::new(),
            }],
            edges: vec![PatchEdge {
                source: 0,
                source_port: 0,
                target: 99,
                target_port: 0,
            }],
        });
        let decoded = decode(&code);
        assert_eq!(decoded.patch.nodes.len(), 1);
        assert!(decoded.patch.edges.is_empty());
        assert!(decoded
            .warnings
            .iter()
            .any(|w| w.contains("missing endpoint")));
    }

    #[test]
    fn a_feedback_loop_is_broken_rather_than_rejected() {
        // Two Mixers feeding each other's inputs: the second cable closes the
        // loop and must be the one dropped, leaving a playable patch.
        let mut nodes = Vec::new();
        for id in [0u32, 1] {
            nodes.push(PatchNode {
                id,
                kind: 5,
                x: 0.0,
                y: 0.0,
                params: HashMap::new(),
            });
        }
        let code = encode(&Patch {
            nodes,
            edges: vec![
                PatchEdge {
                    source: 0,
                    source_port: 0,
                    target: 1,
                    target_port: 0,
                },
                PatchEdge {
                    source: 1,
                    source_port: 0,
                    target: 0,
                    target_port: 0,
                },
            ],
        });
        let decoded = decode(&code);
        assert_eq!(decoded.patch.nodes.len(), 2, "both nodes should survive");
        assert_eq!(decoded.patch.edges.len(), 1, "one cable should be dropped");
        assert!(decoded.warnings.iter().any(|w| w.contains("feedback loop")));
    }

    #[test]
    fn a_cable_into_a_nonexistent_port_is_dropped() {
        // SineOsc has one input ("freq cv"), so port 7 cannot exist.
        let code = encode(&Patch {
            nodes: vec![
                PatchNode {
                    id: 0,
                    kind: 10,
                    x: 0.0,
                    y: 0.0,
                    params: HashMap::new(),
                },
                PatchNode {
                    id: 1,
                    kind: 10,
                    x: 0.0,
                    y: 0.0,
                    params: HashMap::new(),
                },
            ],
            edges: vec![PatchEdge {
                source: 0,
                source_port: 0,
                target: 1,
                target_port: 7,
            }],
        });
        let decoded = decode(&code);
        assert!(decoded.patch.edges.is_empty());
        assert!(decoded.warnings.iter().any(|w| w.contains("input port")));
    }

    #[test]
    fn a_cable_from_a_terminal_module_is_dropped() {
        // Out has no outputs at all, so nothing can be patched out of it.
        let code = encode(&Patch {
            nodes: vec![
                PatchNode {
                    id: 0,
                    kind: 8,
                    x: 0.0,
                    y: 0.0,
                    params: HashMap::new(),
                },
                PatchNode {
                    id: 1,
                    kind: 10,
                    x: 0.0,
                    y: 0.0,
                    params: HashMap::new(),
                },
            ],
            edges: vec![PatchEdge {
                source: 0,
                source_port: 0,
                target: 1,
                target_port: 0,
            }],
        });
        let decoded = decode(&code);
        assert!(decoded.patch.edges.is_empty());
        assert!(decoded.warnings.iter().any(|w| w.contains("output port")));
    }

    #[test]
    fn an_unknown_module_kind_is_skipped_with_a_warning() {
        let code = encode(&Patch {
            nodes: vec![
                PatchNode {
                    id: 0,
                    kind: 10,
                    x: 0.0,
                    y: 0.0,
                    params: HashMap::new(),
                },
                PatchNode {
                    id: 1,
                    kind: 240,
                    x: 0.0,
                    y: 0.0,
                    params: HashMap::new(),
                },
            ],
            edges: vec![],
        });
        let decoded = decode(&code);
        assert_eq!(decoded.patch.nodes.len(), 1);
        assert!(decoded
            .warnings
            .iter()
            .any(|w| w.contains("unknown module kind")));
    }

    #[test]
    fn a_hostile_link_yields_an_empty_patch_instead_of_panicking() {
        for code in [
            "",
            "not base64!!!",
            "eyJ2Ijo5OSwibiI6W119",                        // version 99
            "eyJ2IjoxLCJuIjoibm90IGFuIGFycmF5In0",         // n is not an array
            "eyJ2IjoxLCJuIjpbWzAsMTAsMCwwLDNdLCJlIjpbXX0", // node missing params
            "eyJ2IjoxLCJuIjpbWzAsMTAsMCwwLHtdfSwibiI6WzBdLCJlIjpbXX0", // params not an object
        ] {
            let decoded = decode(code);
            assert!(
                decoded.patch.nodes.is_empty(),
                "expected no nodes from {code:?}, got {:?}",
                decoded.patch.nodes
            );
        }
    }

    #[test]
    fn a_share_link_is_recognised_in_every_form_the_ui_offers() {
        let code = "eyJ2IjoxfQ";
        assert_eq!(
            code_from_link(&format!("https://coarse.app/?p={code}#x")),
            Some(code)
        );
        assert_eq!(code_from_link(&format!("https://coarse.app/#{code}")), None);
        assert_eq!(code_from_link(&format!("?p={code}")), Some(code));
        assert_eq!(code_from_link(code), Some(code));
        assert_eq!(code_from_link("   "), None);
    }

    #[test]
    fn a_saved_json_file_reloads_through_the_same_repairs() {
        let patch = default_patch();
        let json = to_json(&patch);
        // Readable enough to hand-edit or commit to a repo.
        assert!(json.contains("\"v\":1"), "{json}");
        let reloaded = from_json(&json);
        assert!(reloaded.warnings.is_empty(), "{:?}", reloaded.warnings);
        assert_eq!(reloaded.patch, patch);

        // A file gets the same validation as a link: a bad module is dropped and
        // reported rather than silently kept.
        let broken = from_json(r#"{"v":1,"n":[[0,99,0,0,{}]],"e":[]}"#);
        assert!(broken.patch.nodes.is_empty());
        assert!(!broken.warnings.is_empty());

        // And unreadable input is a warning, not a panic.
        let nonsense = from_json("{ not json");
        assert!(nonsense.patch.nodes.is_empty());
        assert!(nonsense.warnings[0].contains("not valid patch JSON"));
    }

    #[test]
    fn a_controller_node_survives_the_round_trip() {
        // The mapping is just a node and a cable, so it needs no patch version.
        let mut patch = Patch::default();
        patch.nodes.push(PatchNode {
            id: 0,
            kind: 13,
            x: 0.0,
            y: 0.0,
            params: [("cc".to_string(), 74.0), ("depth".to_string(), 2.5)]
                .into_iter()
                .collect(),
        });
        patch.nodes.push(PatchNode {
            id: 1,
            kind: 10,
            x: 0.0,
            y: 0.0,
            params: [("freq".to_string(), 220.0)].into_iter().collect(),
        });
        patch.edges.push(PatchEdge {
            source: 0,
            source_port: 0,
            target: 1,
            target_port: 0,
        });

        let decoded = decode(&encode(&patch));
        assert!(
            decoded.warnings.is_empty(),
            "warnings: {:?}",
            decoded.warnings
        );
        assert_eq!(decoded.patch, patch);
    }
}

pub mod bake;
pub use bake::{
    apply_patch, bake, BakeError, BakedPatch, MAX_BAKED_NODE_ID, MAX_EDGES, MAX_NODES,
    MAX_PAYLOAD_BYTES,
};
