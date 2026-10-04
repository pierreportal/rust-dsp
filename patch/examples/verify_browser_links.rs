//! Cross-check this crate's codec against the browser's TypeScript codec.
//!
//! `node /tmp/codec-xcheck.cjs` drives the real web app in a headless browser
//! and writes one `case -> { code, roundTrip }` entry per patch to
//! `/tmp/browser-links.json`. This example then decodes each `code` and
//! re-encodes it, and requires three things:
//!
//! 1. Rust's re-encode is byte-identical to the browser's re-encode of the same
//!    code, so a patch shared either way survives being opened and saved in the
//!    other host, and encoding is idempotent;
//! 2. every node's parameters are exactly the module registry's declared set,
//!    each within its declared range;
//! 3. repairs Rust reports are printed, since it is stricter than the browser
//!    about saying so.
//!
//! Comparing against the browser's *decoded* re-encode rather than its original
//! input matters: the browser clamps and default-fills during decode, so a raw
//! out-of-range value is not something either host would keep.
//!
//! Run: `node /tmp/codec-xcheck.cjs` then
//! `cargo run -p patch --example verify_browser_links`

use graph::{registry, Kind};
use patch::{decode, encode};
use std::collections::{HashMap, HashSet};

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/browser-links.json".into());
    let text = std::fs::read_to_string(&path).expect("run node /tmp/codec-xcheck.cjs first");
    let doc: serde_json::Value = serde_json::from_str(&text).expect("parse browser links json");

    let cases = doc.as_object().expect("object of cases");
    assert!(!cases.is_empty(), "no cases in {path}");
    let mut failures = Vec::new();
    let mut notes = Vec::new();

    for (name, case) in cases {
        let code = case["code"]
            .as_str()
            .unwrap_or_else(|| panic!("{name}: no code"));
        let browser_reencode = case["roundTrip"]
            .as_str()
            .unwrap_or_else(|| panic!("{name}: browser could not decode its own code"));

        let decoded = decode(code);
        // Rust reports a repair that the browser performs silently. That is a
        // deliberate difference in diagnostics, not in behaviour: the repaired
        // patch is compared byte for byte below, so a warning here can never
        // hide a disagreement. Note them so the gap stays visible.
        if !decoded.warnings.is_empty() {
            notes.push(format!(
                "{name}: repaired here, silently in the browser: {}",
                decoded.warnings.join("; ")
            ));
        }

        let ours = encode(&decoded.patch);
        if ours != browser_reencode {
            failures.push(format!(
                "{name}: Rust re-encode differs from the browser's\n  \
                 browser: {browser_reencode}\n  rust:    {ours}"
            ));
        }
        if encode(&decode(&ours).patch) != ours {
            failures.push(format!("{name}: re-encoding is not idempotent"));
        }

        // Parameters must be exactly what the module declares: a missing one
        // would be a silent gap, an extra one a stale name from a past version.
        for node in &decoded.patch.nodes {
            let specs = match Kind::from_u8(node.kind) {
                Some(kind) => registry::params(kind),
                None => {
                    failures.push(format!(
                        "{name}: node {} has unknown kind {}",
                        node.id, node.kind
                    ));
                    continue;
                }
            };
            let declared: HashSet<&str> = specs.iter().map(|s| s.name).collect();
            let got: HashSet<&str> = node.params.keys().map(String::as_str).collect();
            if declared != got {
                failures.push(format!(
                    "{name}: node {} (kind {}) has params {:?}, registry declares {:?}",
                    node.id, node.kind, got, declared
                ));
            }
            for spec in specs {
                let value = node.params[spec.name];
                if value < spec.min || value > spec.max {
                    failures.push(format!(
                        "{name}: node {} {} = {value} is outside {}..={}",
                        node.id, spec.name, spec.min, spec.max
                    ));
                }
            }
        }

        println!(
            "{name:<10} {:>2} nodes {:>2} cables  {}",
            decoded.patch.nodes.len(),
            decoded.patch.edges.len(),
            if ours == browser_reencode {
                "matches browser"
            } else {
                "DIFFERS"
            }
        );
    }

    // A Controller mapping must arrive already wired, with no mapping table:
    // that is the whole promise of sharing a link between the two hosts.
    let cc_code = cases
        .get("ccMap")
        .and_then(|c| c["code"].as_str())
        .expect("ccMap case");
    let patch = decode(cc_code).patch;
    let by_kind: HashMap<u32, &patch::PatchNode> =
        patch.nodes.iter().map(|n| (n.kind, n)).collect();
    let controller = by_kind.get(&13).expect("controller node, kind 13");
    assert_eq!(controller.params["cc"], 74.0);
    assert_eq!(controller.params["depth"], 2.0);
    assert_eq!(patch.edges.len(), 1, "the cable should have arrived too");
    println!(
        "\nCC {} at depth {} octaves patched into input port {} survived the crossing",
        controller.params["cc"], controller.params["depth"], patch.edges[0].target_port
    );

    for note in &notes {
        println!("note: {note}");
    }

    if failures.is_empty() {
        println!("\nPASS: Rust and the browser agree on every case");
        return;
    }
    eprintln!("\nFAIL:\n - {}", failures.join("\n - "));
    std::process::exit(1);
}
