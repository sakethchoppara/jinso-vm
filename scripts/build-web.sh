#!/usr/bin/env bash
# Builds the browser playground into web/. Preview locally with:
#   python3 -m http.server -d web    (then open http://localhost:8000)
set -euo pipefail
cd "$(dirname "$0")/.."
rustup target add wasm32-unknown-unknown > /dev/null 2>&1 || true
cargo build --release --lib --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/lumen.wasm web/lumen.wasm
ls -lh web/lumen.wasm
