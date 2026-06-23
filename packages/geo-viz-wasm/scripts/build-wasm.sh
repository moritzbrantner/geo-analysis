#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
RUSTFLAGS='--cfg getrandom_backend="wasm_js"' wasm-pack build "$ROOT_DIR/crates/geo-viz-wasm" --target web --out-dir "$ROOT_DIR/packages/geo-viz-wasm/pkg"
