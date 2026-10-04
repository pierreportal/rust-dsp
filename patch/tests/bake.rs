use patch::bake::{bake, BakeError, MAX_BAKED_NODE_ID, MAX_EDGES, MAX_NODES, MAX_PAYLOAD_BYTES};
use patch::default_patch;
use serde_json::json;

fn trivial_patch_json() -> String {
    // A patch with one sine and one out, connected.
    json!({
        "v": 1,
        "n": [
            [0, 10, 0.0, 0.0, {}], // SineOsc
            [1, 8,  0.0, 0.0, {}], // Out
        ],
        "e": [[0, 0, 1, 0]],
    })
    .to_string()
}

#[test]
fn bake_accepts_a_trivial_valid_patch() {
    let baked = bake(&trivial_patch_json()).unwrap();
    assert_eq!(baked.node_count(), 2);
    assert!(baked.warnings.is_empty());
}

#[test]
fn bake_rejects_empty_payload() {
    let res = bake("");
    assert!(matches!(res, Err(BakeError::NoNodes)));
}

#[test]
fn bake_tolerates_unknown_module_kind_with_repair() {
    let j = json!({
        "v": 1,
        "n": [[0, 255, 0.0, 0.0, {}], [1, 8, 0.0, 0.0, {}]],
        "e": [[0,0,1,0]],
    })
    .to_string();
    let res = bake(&j);
    assert!(
        res.is_ok(),
        "decode repairs unknown kinds; baking accepts repaired valid patch"
    );
}

#[test]
fn bake_rejects_too_many_nodes() {
    let mut nodes = Vec::new();
    // build MAX_NODES + 1
    let mut id = 0u32;
    while nodes.len() <= MAX_NODES {
        // alternate between a simple source and out? easier: add sources until
        // just before cap, then add out
        nodes.push(json!([id, 10, 0.0, 0.0, {}]));
        id += 1;
    }
    // replace last with out and ensure count > MAX_NODES? simpler: push out
    // but easier to construct explicitly
    let mut jnodes = Vec::new();
    for i in 0..=(MAX_NODES as u32) {
        jnodes.push(json!([i, 10, 0.0, 0.0, {}]));
    }
    let j = json!({"v": 1, "n": jnodes, "e": []}).to_string();
    let res = bake(&j);
    assert!(matches!(res, Err(BakeError::TooManyNodes { .. })));
}

#[test]
fn bake_rejects_node_id_too_large() {
    let j = json!({
        "v": 1,
        "n": [
            [MAX_BAKED_NODE_ID + 1, 10, 0.0, 0.0, {}],
            [0, 8, 0.0, 0.0, {}],
        ],
        "e": [],
    })
    .to_string();
    let res = bake(&j);
    assert!(matches!(res, Err(BakeError::NodeIdTooLarge { .. })));
}

#[test]
fn bake_rejects_deep_cable_chains_without_sink() {
    // chain of sources feeding each other, no Out -> NoSink or cycle? no
    // connections form a line but nothing feeds out
    let mut jnodes = Vec::new();
    for i in 0..10u32 {
        jnodes.push(json!([i, 10, 0.0, 0.0, {}]));
    }
    let mut jedges = Vec::new();
    for i in 0..9u32 {
        jedges.push(json!([i, 0, i + 1, 0]));
    }
    let j = json!({"v": 1, "n": jnodes, "e": jedges}).to_string();
    let res = bake(&j);
    assert!(res.is_err());
}

#[test]
fn bake_accepts_deep_chain_with_sink() {
    let mut jnodes = Vec::new();
    for i in 0..20u32 {
        jnodes.push(json!([i, 10, 0.0, 0.0, {}]));
    }
    jnodes.push(json!([20, 8, 0.0, 0.0, {}]));
    let mut jedges = Vec::new();
    for i in 0..20u32 {
        jedges.push(json!([i, 0, i + 1, 0]));
    }
    let j = json!({"v": 1, "n": jnodes, "e": jedges}).to_string();
    let res = bake(&j);
    assert!(res.is_ok());
}

#[test]
fn bake_accepts_patch_with_unusual_floats() {
    let j = json!({
        "v": 1,
        "n": [
            [0, 10, 0.0, 0.0, {"freq": "NaN"}],
            [1, 8, 0.0, 0.0, {}],
        ],
        "e": [[0,0,1,0]],
    })
    .to_string();
    let baked = bake(&j);
    assert!(baked.is_ok());
}

#[test]
fn bake_rejects_payload_too_large() {
    let big = "x".repeat(MAX_PAYLOAD_BYTES + 1);
    let res = bake(&big);
    assert!(matches!(res, Err(BakeError::TooLarge { .. })));
}

#[test]
fn bake_accepts_max_sized_payload() {
    let base = trivial_patch_json();
    let mut pad = base.clone();
    while pad.len() < MAX_PAYLOAD_BYTES {
        pad.push(' ');
    }
    if pad.len() > MAX_PAYLOAD_BYTES {
        pad.truncate(MAX_PAYLOAD_BYTES);
    }
    let res = bake(&pad);
    assert!(res.is_ok());
}

#[test]
fn bake_requires_out_node() {
    let j = json!({
        "v": 1,
        "n": [[0, 10, 0.0, 0.0, {}]],
        "e": [],
    })
    .to_string();
    let res = bake(&j);
    assert!(matches!(res, Err(BakeError::NoSink)));
}

#[test]
fn bake_repairs_cycles_into_valid_graph() {
    let j = json!({
        "v": 1,
        "n": [
            [0, 10, 0.0, 0.0, {}],
            [1, 4,  0.0, 0.0, {}],
            [2, 8,  0.0, 0.0, {}],
        ],
        "e": [[0,0,1,0],[1,0,0,0],[1,0,2,0]],
    })
    .to_string();
    let res = bake(&j);
    assert!(
        res.is_ok(),
        "a cycle-containing patch must bake to a repaired valid graph"
    );
    assert!(!res.as_ref().unwrap().warnings.is_empty());
}
