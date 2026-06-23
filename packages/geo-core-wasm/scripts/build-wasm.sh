#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
wasm-pack build "$ROOT_DIR/crates/geo-core-wasm" --target web --out-dir "$ROOT_DIR/packages/geo-core-wasm/pkg"
