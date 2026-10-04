//! Turning an untrusted patch into an engine.
//!
//! `decode` is deliberately tolerant: it repairs bad values and collects
//! warnings, because a shared link has to load in whatever app opens it and the
//! two codecs must agree exactly or links stop round-tripping. That tolerance is
//! right for a *link* and wrong for a *baked binary*.
//!
//! This module is the stricter gate in front of it, for the case where the patch
//! becomes the instrument. The goal from the milestone plan is a hostile patch
//! producing a valid but boring plugin -- never a crash, a hang, or a 200 MB
//! binary -- and a broken graph failing at build time rather than shipping as a
//! silent plugin.
//!
//! Nothing here changes what `decode` accepts. A patch the browser accepts still
//! bakes here, with warnings, or this returns a `BakeError` explaining which
//! limit it hit.
//!
//! # Why the limits are here and not in the codec
//!
//! `MAX_NODE_ID` in the codec bounds ids at 2^20. That is the right tolerance for
//! a link, but it is not a memory bound: `GraphEngine::ensure` sizes its node
//! table from the *id*, not the node count, so one node with id 1_048_575 costs a
//! million table entries in the plugin. Since the decoder cannot be tightened
//! without breaking cross-codec agreement, the tighter bound lives here.

use crate::{decode, Decoded, Patch, PatchEdge};
use graph::{Kind, PolyGraph};

/// Most modules a baked patch may contain.
///
/// A synth patch is tens of modules. This leaves generous headroom for the
/// largest presets anyone actually builds, while keeping the node table, the
/// per-voice engine copies and the build-time validation all bounded.
pub const MAX_NODES: usize = 512;

/// Highest node id a baked patch may use.
///
/// The codec accepts up to `MAX_NODE_ID`, but `GraphEngine` sizes its table from
/// the id, so this is what actually bounds memory. It is far below the codec's
/// own limit on purpose: baking is allowed to be stricter than sharing.
pub const MAX_BAKED_NODE_ID: u32 = 4096;

/// Most cables a baked patch may contain.
///
/// Bounds the topological sort and the per-node input tables. A chain this long
/// would be pathological; a real patch has tens.
pub const MAX_EDGES: usize = 2048;

/// Largest patch payload accepted, in bytes.
///
/// The patch is embedded in the binary, so this is roughly the binary's growth.
/// 1 MB of JSON is roughly 10x any real patch; the plan calls for testing 10 KB
/// and 1 MB payloads, and 1 MB sits just under this cap.
pub const MAX_PAYLOAD_BYTES: usize = 1 << 20;

/// Why a patch cannot be baked into a plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BakeError {
    /// The payload exceeded `MAX_PAYLOAD_BYTES`, so it is not worth parsing.
    TooLarge { bytes: usize, limit: usize },
    /// The JSON did not decode at all.
    Undecodable { detail: String },
    /// No modules survived decoding.
    NoNodes,
    /// More modules than `MAX_NODES`.
    TooManyNodes { count: usize, limit: usize },
    /// A node id beyond `MAX_BAKED_NODE_ID`, which would blow up the engine's
    /// table since it is sized from the id.
    NodeIdTooLarge { id: u32, limit: u32 },
    /// More cables than `MAX_EDGES`.
    TooManyEdges { count: usize, limit: usize },
    /// The graph cannot make sound: no `Out` module, so it renders silence.
    ///
    /// Not fatal for a link -- someone might be mid-edit -- but a binary that is
    /// silent on every note is broken, and shipping it silently is exactly what
    /// this gate exists to prevent.
    NoSink,
    /// A cable points at a module that is not in the patch.
    DanglingEdge { edge: PatchEdge, missing_node: u32 },
    /// A cable references a port that does not exist on an endpoint module.
    BadPort { edge: PatchEdge, detail: String },
    /// The cables form a cycle, so there is no valid render order.
    Cycle,
    /// The graph decodes and validates, but no module produced an output at all.
    NoAudioPath,
}

impl std::fmt::Display for BakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BakeError::TooLarge { bytes, limit } => write!(
                f,
                "patch is {bytes} bytes, over the {limit} byte baking limit"
            ),
            BakeError::Undecodable { detail } => write!(f, "patch does not decode: {detail}"),
            BakeError::NoNodes => write!(f, "patch contains no modules"),
            BakeError::TooManyNodes { count, limit } => {
                write!(f, "patch has {count} modules, over the {limit} limit")
            }
            BakeError::NodeIdTooLarge { id, limit } => write!(
                f,
                "node id {id} is over the baking limit of {limit}; \
                 ids size the engine's table, so this would allocate {id} slots"
            ),
            BakeError::TooManyEdges { count, limit } => {
                write!(f, "patch has {count} cables, over the {limit} limit")
            }
            BakeError::NoSink => write!(
                f,
                "patch has no Out module, so the plugin would render silence"
            ),
            BakeError::DanglingEdge { edge, missing_node } => write!(
                f,
                "cable {}:{} -> {}:{} refers to missing module {missing_node}",
                edge.source, edge.source_port, edge.target, edge.target_port
            ),
            BakeError::BadPort { edge, detail } => write!(
                f,
                "cable {}:{} -> {}:{} is invalid: {detail}",
                edge.source, edge.source_port, edge.target, edge.target_port
            ),
            BakeError::Cycle => write!(f, "cables form a cycle, so there is no render order"),
            BakeError::NoAudioPath => write!(f, "patch cannot make sound"),
        }
    }
}

impl std::error::Error for BakeError {}

/// A patch that passed baking: decoded, bounded, and known to render.
#[derive(Clone, Debug, Default)]
pub struct BakedPatch {
    pub patch: Patch,
    /// Repairs the decoder made, kept so the plugin can show them.
    ///
    /// A baked instrument that quietly dropped a cable should be able to say so.
    pub warnings: Vec<String>,
}

impl BakedPatch {
    /// Module count, the number the plan's sanity bound applies to.
    pub fn node_count(&self) -> usize {
        self.patch.nodes.len()
    }
}

/// Decode a patch and hold it to the baking limits.
///
/// The size check happens before parsing, so a hostile payload cannot make the
/// build process do the work of reading it.
pub fn bake(json: &str) -> Result<BakedPatch, BakeError> {
    if json.len() > MAX_PAYLOAD_BYTES {
        return Err(BakeError::TooLarge {
            bytes: json.len(),
            limit: MAX_PAYLOAD_BYTES,
        });
    }

    let decoded = decode_json(json);
    validate(&decoded.patch)?;

    Ok(BakedPatch {
        patch: decoded.patch,
        warnings: decoded.warnings,
    })
}

/// `decode` returns a `Decoded` with empty warnings rather than an error for
/// malformed JSON, so a total failure here looks like a patch with no modules.
/// Distinguish the two by checking whether anything was decoded at all.
fn decode_json(json: &str) -> Decoded {
    let decoded = crate::from_json(json);
    if decoded.patch.nodes.is_empty() && decoded.warnings.is_empty() {
        // Nothing at all was produced, so nothing was recognisable.
        return Decoded {
            patch: Patch::default(),
            warnings: vec![format!(
                "patch did not contain a recognisable v{} patch",
                crate::PATCH_VERSION
            )],
        };
    }
    decoded
}

/// Check a decoded patch against everything a baked binary needs.
fn validate(patch: &Patch) -> Result<(), BakeError> {
    if patch.nodes.is_empty() {
        return Err(BakeError::NoNodes);
    }
    if patch.nodes.len() > MAX_NODES {
        return Err(BakeError::TooManyNodes {
            count: patch.nodes.len(),
            limit: MAX_NODES,
        });
    }
    if patch.edges.len() > MAX_EDGES {
        return Err(BakeError::TooManyEdges {
            count: patch.edges.len(),
            limit: MAX_EDGES,
        });
    }
    for node in &patch.nodes {
        if node.id > MAX_BAKED_NODE_ID {
            return Err(BakeError::NodeIdTooLarge {
                id: node.id,
                limit: MAX_BAKED_NODE_ID,
            });
        }
    }

    // Index the modules so cable checks are not quadratic on a large patch.
    let mut kinds: Vec<(u32, Kind)> = Vec::with_capacity(patch.nodes.len());
    let mut has_sink = false;
    for node in &patch.nodes {
        // `decode` drops modules with an unknown kind, so anything left should be
        // convertible. Treat a failure as fatal rather than skipping, so a codec
        // and registry disagreement is loud instead of silently shrinking a
        // shipped instrument.
        let kind = Kind::from_u8(node.kind).ok_or(BakeError::NoAudioPath)?;
        if kind == Kind::Out {
            has_sink = true;
        }
        kinds.push((node.id, kind));
    }
    if !has_sink {
        return Err(BakeError::NoSink);
    }

    let kind_of = |id: u32| kinds.iter().find(|(n, _)| *n == id).map(|(_, k)| *k);

    for edge in &patch.edges {
        let from = kind_of(edge.source).ok_or(BakeError::DanglingEdge {
            edge: *edge,
            missing_node: edge.source,
        })?;
        let to = kind_of(edge.target).ok_or(BakeError::DanglingEdge {
            edge: *edge,
            missing_node: edge.target,
        })?;
        if (edge.source_port as usize) >= from.outputs().len() {
            return Err(BakeError::BadPort {
                edge: *edge,
                detail: format!(
                    "source has {} outputs, port {} does not exist",
                    from.outputs().len(),
                    edge.source_port
                ),
            });
        }
        if (edge.target_port as usize) >= to.inputs().len() {
            return Err(BakeError::BadPort {
                edge: *edge,
                detail: format!(
                    "target takes {} inputs, port {} does not exist",
                    to.inputs().len(),
                    edge.target_port
                ),
            });
        }
    }

    if has_cycle(patch, &kinds) {
        return Err(BakeError::Cycle);
    }

    Ok(())
}

/// Kahn's algorithm over the cables. Mirrors `GraphEngine::rebuild`, so a patch
/// accepted here is one the engine can order.
fn has_cycle(patch: &Patch, kinds: &[(u32, Kind)]) -> bool {
    let mut indeg: Vec<usize> = kinds.iter().map(|_| 0).collect();
    let mut adj: Vec<Vec<usize>> = kinds.iter().map(|_| Vec::new()).collect();
    let index_of = |id: u32| kinds.iter().position(|(n, _)| *n == id);

    let mut edges = Vec::new();
    for edge in &patch.edges {
        let (Some(f), Some(t)) = (index_of(edge.source), index_of(edge.target)) else {
            continue;
        };
        if f == t {
            return true;
        }
        adj[f].push(t);
        indeg[t] += 1;
        edges.push(f);
    }
    let _ = edges;

    // Seed with every module nothing feeds.
    let mut queue: Vec<usize> = (0..kinds.len()).filter(|i| indeg[*i] == 0).collect();
    let mut visited = 0usize;
    while let Some(n) = queue.pop() {
        visited += 1;
        for &next in &adj[n] {
            indeg[next] -= 1;
            if indeg[next] == 0 {
                queue.push(next);
            }
        }
    }
    visited != kinds.len()
}

/// Build a voice pool from a patch.
///
/// Shared by every host, so the desktop app and the plugin cannot drift on how a
/// patch becomes an engine. `patch` depends on `graph`, so this lives here rather
/// than in a host: a plugin must not depend on the desktop app to get its synth.
///
/// Any cable the engine refuses is reported rather than printed, so a caller can
/// surface it.
pub fn apply_patch(voice: &mut PolyGraph, patch: &Patch, sample_rate: f32) -> Vec<String> {
    let mut fresh = PolyGraph::new(sample_rate);
    let mut problems = Vec::new();

    // Sorted by id so the engine's node table is filled in a deterministic order,
    // which keeps a rebuilt binary's layout stable.
    let mut nodes: Vec<&crate::PatchNode> = patch.nodes.iter().collect();
    nodes.sort_by_key(|n| n.id);
    for node in &nodes {
        if !fresh.add_node(node.id, node.kind) {
            problems.push(format!(
                "skipped module {}: unknown kind {}",
                node.id, node.kind
            ));
        }
    }

    for (id, name, value) in patch.params_by_id() {
        fresh.set_param(id, &name, value);
    }

    for edge in &patch.edges {
        if !fresh.connect(edge.source, edge.source_port, edge.target, edge.target_port) {
            problems.push(format!(
                "skipped cable {}:{} -> {}:{}",
                edge.source, edge.source_port, edge.target, edge.target_port
            ));
        }
    }

    *voice = fresh;
    problems
}
