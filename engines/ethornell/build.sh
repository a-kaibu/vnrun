#!/usr/bin/env bash
# Builds ethornell-rs (third_party/ethornell-rs, BGI/Ethornell) for the browser into
# web/public/engines/ethornell/: the wasm-bindgen module and the upstream audio worklet. The worker
# that runs it is ours (web/engines/ethornell.worker.js) because it reads files over HTTP.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC="$ROOT/third_party/ethornell-rs"
OUT="$ROOT/web/public/engines/ethornell"

if [[ ! -f "$SRC/Cargo.toml" ]]; then
  git -C "$ROOT" submodule update --init third_party/ethornell-rs
fi
# Our fixes for the browser build (patches/ethornell-rs/*.patch), applied to a clean checkout.
git -C "$SRC" checkout -q -- .
for p in "$ROOT"/patches/ethornell-rs/*.patch; do
  [[ -e "$p" ]] && git -C "$SRC" apply "$p"
done
# ethornell-rs pins this version in its Cargo.lock; the CLI must match it.
WASM_BINDGEN_VERSION="$(awk '/^name = "wasm-bindgen"$/ {getline; gsub(/[^0-9.]/, ""); print; exit}' "$SRC/Cargo.lock")"
# Each engine pins its own version, so each version gets its own install.
BINDGEN_ROOT="$ROOT/target/tools/wasm-bindgen-$WASM_BINDGEN_VERSION"
if [[ ! -x "$BINDGEN_ROOT/bin/wasm-bindgen" ]]; then
  cargo install --root "$BINDGEN_ROOT" wasm-bindgen-cli --version "$WASM_BINDGEN_VERSION" --locked
fi
wasm_bindgen() { "$BINDGEN_ROOT/bin/wasm-bindgen" "$@"; }
rustup target add wasm32-unknown-unknown

cargo build --manifest-path "$SRC/Cargo.toml" --release --locked -p ethornell-web --target wasm32-unknown-unknown
rm -rf "$OUT"
mkdir -p "$OUT"
wasm_bindgen "$SRC/target/wasm32-unknown-unknown/release/ethornell_web.wasm" --target web --no-typescript --out-dir "$OUT"
cp "$SRC/platform/wasm/audio-worklet.js" "$OUT/"
git -C "$SRC" rev-parse HEAD > "$OUT/REVISION"
