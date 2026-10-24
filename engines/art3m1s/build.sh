#!/usr/bin/env bash
# Builds art3m1s-core (third_party/art3m1s-core, Artemis) with its browser host
# (engines/art3m1s/host) for the browser into web/public/engines/art3m1s/. Needs Emscripten (emcc on
# PATH, or EMSDK set); art3m1s-core embeds Lua 5.1, which is C.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC="$ROOT/third_party/art3m1s-core"
HOST="$ROOT/engines/art3m1s/host"
OUT="$ROOT/web/public/engines/art3m1s"

if [[ ! -f "$SRC/Cargo.toml" ]]; then
  git -C "$ROOT" submodule update --init third_party/art3m1s-core
fi
# Our fixes for the browser build (patches/art3m1s-core/*.patch), applied to a clean checkout.
git -C "$SRC" checkout -q -- .
for p in "$ROOT"/patches/art3m1s-core/*.patch; do
  [[ -e "$p" ]] && git -C "$SRC" apply "$p"
done
if ! command -v emcc > /dev/null; then
  # shellcheck disable=SC1091
  source "${EMSDK:?install Emscripten or set EMSDK}/emsdk_env.sh" > /dev/null
fi
rustup target add wasm32-unknown-emscripten

EXPORTS="_main,_vn_start,_vn_error,_vn_frame,_vn_pixels,_vn_exit_requested,_vn_mouse,_vn_button,_vn_key,_vn_poll_events,_vn_out,_vn_read,_vn_sound_finished,_vn_video_finished,_vn_dialog,_vn_lifecycle,_malloc,_free"
LINK=(
  -sMODULARIZE=1 -sEXPORT_ES6=1 -sEXPORT_NAME=createArt3m1s -sENVIRONMENT=web
  -sALLOW_MEMORY_GROWTH=1 -sMAXIMUM_MEMORY=4GB -sSTACK_SIZE=8MB
  -sMIN_WEBGL_VERSION=2 -sMAX_WEBGL_VERSION=2 -sGL_ENABLE_GET_PROC_ADDRESS=1
  "-sEXPORTED_FUNCTIONS=$EXPORTS"
  -sEXPORTED_RUNTIME_METHODS=FS,HEAPU8,UTF8ToString,stringToNewUTF8
  "--js-library=$HOST/library.js"
  # mlua builds Lua as C++ for Emscripten, so Lua errors are C++ exceptions.
  -lc++ -lc++abi
)
# Link flags go to the final binary only (RUSTFLAGS would also reach the cdylib dependencies).
ARGS=()
for flag in "${LINK[@]}"; do ARGS+=(-C "link-arg=$flag"); done
export CARGO_TARGET_DIR="$SRC/target"
# Rust on Emscripten unwinds with wasm exceptions, and both kinds cannot be linked together.
# lua-src builds Lua as C++ with -fexceptions (JavaScript exceptions); this compiler wrapper
# switches that to wasm exceptions.
WRAP="$CARGO_TARGET_DIR/emxx-wasm-eh"
mkdir -p "$CARGO_TARGET_DIR"
cat > "$WRAP" << 'WRAPPER'
#!/usr/bin/env bash
args=()
for a in "$@"; do [[ "$a" == -fexceptions ]] && a=-fwasm-exceptions; args+=("$a"); done
exec em++ "${args[@]}"
WRAPPER
chmod +x "$WRAP"
export CXX_wasm32_unknown_emscripten="$WRAP"
cargo rustc --manifest-path "$HOST/Cargo.toml" --release --target wasm32-unknown-emscripten --bin art3m1s -- "${ARGS[@]}"

BUILT="$CARGO_TARGET_DIR/wasm32-unknown-emscripten/release"
rm -rf "$OUT"
mkdir -p "$OUT"
cp "$BUILT/art3m1s.js" "$BUILT/art3m1s.wasm" "$OUT/"
git -C "$SRC" rev-parse HEAD > "$OUT/REVISION"
