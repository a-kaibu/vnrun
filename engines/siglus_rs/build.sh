#!/usr/bin/env bash
# Builds siglus_rs (third_party/siglus_rs) for the browser into dist/engines/siglus_rs/.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC="$ROOT/third_party/siglus_rs"
OUT="$ROOT/dist/engines/siglus_rs"
# siglus_rs pins this version in its Cargo.lock; the CLI must match it.
WASM_BINDGEN_VERSION="$(awk '/^name = "wasm-bindgen"$/ {getline; gsub(/[^0-9.]/, ""); print; exit}' "$SRC/Cargo.lock")"

if [[ ! -f "$SRC/Cargo.toml" ]]; then
  git -C "$ROOT" submodule update --init third_party/siglus_rs
fi
# Our fixes for the browser build (patches/siglus_rs/*.patch), applied to a clean checkout.
git -C "$SRC" checkout -q -- .
for p in "$ROOT"/patches/siglus_rs/*.patch; do
  [[ -e "$p" ]] && git -C "$SRC" apply "$p"
done
if [[ "$(wasm-bindgen --version 2>/dev/null | awk '{print $2}')" != "$WASM_BINDGEN_VERSION" ]]; then
  cargo install -f wasm-bindgen-cli --version "$WASM_BINDGEN_VERSION" --locked
fi
rustup target add wasm32-unknown-unknown

cargo build --manifest-path "$SRC/Cargo.toml" --release -p game_launcher --target wasm32-unknown-unknown
rm -rf "$OUT"
mkdir -p "$OUT"
wasm-bindgen "$SRC/target/wasm32-unknown-unknown/release/game_launcher.wasm" --target web --out-dir "$OUT"
git -C "$SRC" rev-parse HEAD > "$OUT/REVISION"
