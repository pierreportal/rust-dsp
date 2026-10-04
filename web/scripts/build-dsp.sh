#!/usr/bin/env bash
#
# Builds the WebAssembly bundle (web/pkg/) plus the module registry JSON
# (web/pkg/module-registry.json) that the React UI consumes.
#
# This is the single entry point for producing a DSP release artifact. It is
# called locally (`./scripts/build-dsp.sh`) and by the GitHub Actions release
# workflow. The UI repo fetches the resulting `pkg` archive + registry, so a
# new Rust module simply requires rebuilding with this script.
#
# The engine itself lives in the sibling `graph/` crate; this script builds
# only the wasm bindings around it plus the shared module catalogue.
#
# Usage: ./scripts/build-dsp.sh [--release]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG_DIR="$ROOT/pkg"

PROFILE="dev"
if [[ "${1:-}" == "--release" ]]; then
  PROFILE="release"
fi

echo "==> Building wasm pkg ($PROFILE) into $PKG_DIR"
wasm-pack build "$ROOT" --target web --$PROFILE

echo "==> Generating module-registry.json"
(cd "$ROOT/.." && cargo run --quiet -p graph --example gen_registry -- "$PKG_DIR/module-registry.json")

echo "==> Done:"
ls -lh "$PKG_DIR"
