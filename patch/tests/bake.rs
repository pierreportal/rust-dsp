use patch::bake::{bake, BakeError, MAX_BAKED_NODE_ID, MAX_NODES, MAX_PAYLOAD_BYTES};
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
    // An unrecognised module is dropped and the cable into it goes with it, but the
    // rest of the instrument still builds. The sine and the Out are what make the
    // patch playable, so the point of the fixture is the repair, not the graph.
    let j = json!({
        "v": 1,
        "n": [[0, 255, 0.0, 0.0, {}], [1, 10, 0.0, 0.0, {}], [2, 8, 0.0, 0.0, {}]],
        "e": [[0,0,1,0], [1,0,2,0]],
    })
    .to_string();
    let res = bake(&j);
    assert!(
        res.is_ok(),
        "decode repairs unknown kinds; baking accepts repaired valid patch"
    );
    let baked = res.unwrap();
    assert!(
        !baked.warnings.is_empty(),
        "dropping a module the registry does not know should be reported"
    );
    assert_eq!(baked.node_count(), 2, "the unknown module should be gone");
}

#[test]
fn bake_rejects_a_patch_whose_output_nothing_feeds() {
    // An Out module with no cable into it renders silence on every note. Shipping
    // that as a paid plugin is the exact failure the baking gate exists to prevent,
    // so it is refused at build time rather than discovered by a customer.
    let j = json!({
        "v": 1,
        "n": [[0, 10, 0.0, 0.0, {}], [1, 8, 0.0, 0.0, {}]],
        "e": [],
    })
    .to_string();
    assert!(matches!(bake(&j), Err(BakeError::NoAudioPath)));
}

#[test]
fn bake_rejects_an_output_fed_only_by_control_voltage() {
    // Nothing here can make a sound: a gate and an envelope into an Out is a
    // constant, not an instrument.
    let j = json!({
        "v": 1,
        "n": [[0, 9, 0.0, 0.0, {}], [1, 1, 0.0, 0.0, {}], [2, 8, 0.0, 0.0, {}]],
        "e": [[0,0,1,0], [1,0,2,0]],
    })
    .to_string();
    assert!(matches!(bake(&j), Err(BakeError::NoAudioPath)));
}

#[test]
fn bake_accepts_a_signal_that_reaches_the_output_through_a_chain() {
    // The reachability walk has to follow the whole chain, not just look at what is
    // plugged into the Out.
    let j = json!({
        "v": 1,
        "n": [
            [0, 10, 0.0, 0.0, {}],  // SineOsc
            [1, 2, 0.0, 0.0, {}],   // Filter
            [2, 3, 0.0, 0.0, {}],   // Distortion
            [3, 4, 0.0, 0.0, {}],   // Vca
            [4, 8, 0.0, 0.0, {}],   // Out
        ],
        "e": [[0,0,1,0], [1,0,2,0], [2,0,3,0], [3,0,4,0]],
    })
    .to_string();
    assert!(bake(&j).is_ok());
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
