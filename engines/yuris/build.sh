#!/usr/bin/env bash
# Builds our YU-RIS engine (crates/yuris, exported by crates/yuris-web) for the browser into
# web/public/engines/yuris/.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="$ROOT/web/public/engines/yuris"

# The CLI must match the wasm-bindgen crate in Cargo.lock.
WASM_BINDGEN_VERSION="$(awk '/^name = "wasm-bindgen"$/ {getline; gsub(/[^0-9.]/, ""); print; exit}' "$ROOT/Cargo.lock")"
BINDGEN_ROOT="$ROOT/target/tools/wasm-bindgen-$WASM_BINDGEN_VERSION"
if [[ ! -x "$BINDGEN_ROOT/bin/wasm-bindgen" ]]; then
  cargo install --root "$BINDGEN_ROOT" wasm-bindgen-cli --version "$WASM_BINDGEN_VERSION" --locked
fi
rustup target add wasm32-unknown-unknown

cargo build --manifest-path "$ROOT/Cargo.toml" --release --locked -p yuris-web --target wasm32-unknown-unknown
rm -rf "$OUT"
mkdir -p "$OUT"
"$BINDGEN_ROOT/bin/wasm-bindgen" "$ROOT/target/wasm32-unknown-unknown/release/yuris_web.wasm" --target web --no-typescript --out-dir "$OUT"
