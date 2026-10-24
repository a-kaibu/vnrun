#!/usr/bin/env bash
# Builds sena-rs (third_party/sena-rs, Softpal / PAL) for the browser into dist/engines/sena/.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC="$ROOT/third_party/sena-rs"
OUT="$ROOT/dist/engines/sena"

if [[ ! -f "$SRC/Cargo.toml" ]]; then
  git -C "$ROOT" submodule update --init third_party/sena-rs
fi
# Our fixes for the browser build (patches/sena-rs/*.patch), applied to a clean checkout.
git -C "$SRC" checkout -q -- .
for p in "$ROOT"/patches/sena-rs/*.patch; do
  [[ -e "$p" ]] && git -C "$SRC" apply "$p"
done
# sena-rs pins this version in its Cargo.lock; the CLI must match it.
WASM_BINDGEN_VERSION="$(awk '/^name = "wasm-bindgen"$/ {getline; gsub(/[^0-9.]/, ""); print; exit}' "$SRC/Cargo.lock")"
# Each engine pins its own version, so each version gets its own install.
BINDGEN_ROOT="$ROOT/target/tools/wasm-bindgen-$WASM_BINDGEN_VERSION"
if [[ ! -x "$BINDGEN_ROOT/bin/wasm-bindgen" ]]; then
  cargo install --root "$BINDGEN_ROOT" wasm-bindgen-cli --version "$WASM_BINDGEN_VERSION" --locked
fi
wasm_bindgen() { "$BINDGEN_ROOT/bin/wasm-bindgen" "$@"; }
rustup target add wasm32-unknown-unknown

cargo build --manifest-path "$SRC/Cargo.toml" --release --locked -p pal-vm --lib --target wasm32-unknown-unknown
rm -rf "$OUT"
mkdir -p "$OUT"
wasm_bindgen "$SRC/target/wasm32-unknown-unknown/release/pal_vm.wasm" --target web --no-typescript --out-dir "$OUT"
git -C "$SRC" rev-parse HEAD > "$OUT/REVISION"
