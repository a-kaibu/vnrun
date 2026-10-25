#!/usr/bin/env bash
# Builds KiriKiri SDL2 (third_party/krkrsdl2, a port of KiriKiri Z; MIT and the licenses of its
# parts) for the browser into web/public/engines/krkr/. Needs Emscripten (emcc on PATH, or EMSDK
# set), CMake and Ninja. Frames of the window surface go to the page (library.js), which shows
# them with WebGPU.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC="$ROOT/third_party/krkrsdl2"
OUT="$ROOT/web/public/engines/krkr"
WORK="$ROOT/target/krkr"

if [[ ! -f "$SRC/CMakeLists.txt" ]]; then
  git -C "$ROOT" submodule update --init --recursive third_party/krkrsdl2
fi
# Our fixes for the browser build (patches/krkrsdl2/*.patch), applied to a clean checkout.
git -C "$SRC" checkout -q -- .
for p in "$ROOT"/patches/krkrsdl2/*.patch; do
  [[ -e "$p" ]] && git -C "$SRC" apply "$p"
done
# And for its KiriKiri Z (a submodule of it), patches/krkrz/*.patch.
git -C "$SRC/external/krkrz" checkout -q -- . && git -C "$SRC/external/krkrz" clean -fdq
for p in "$ROOT"/patches/krkrz/*.patch; do
  [[ -e "$p" ]] && git -C "$SRC/external/krkrz" apply "$p"
done
# stb_vorbis (public domain or MIT) for its Ogg Vorbis decoder (patches/krkrz/0003).
cp "$ROOT/engines/krkr/stb_vorbis.c" "$SRC/external/krkrz/sound/"
if ! command -v emcc > /dev/null; then
  # shellcheck disable=SC1091
  source "${EMSDK:?install Emscripten or set EMSDK}/emsdk_env.sh" > /dev/null
fi

LDFLAGS=(
  -sEXPORT_ES6=1 -sEXPORT_NAME=createKrkr -sENVIRONMENT=web -sINVOKE_RUN=0 -sEXIT_RUNTIME=0
  "-sEXPORTED_FUNCTIONS=_main,_vnrun_mouse,_vnrun_wheel,_emscripten_syncfs_is_finished"
  -sEXPORTED_RUNTIME_METHODS=callMain,FS,HEAPU8
  "--js-library=$ROOT/engines/krkr/library.js"
)
emcmake cmake -S "$SRC" -B "$WORK" -G Ninja -DCMAKE_BUILD_TYPE=Release \
  -DOPTION_ENABLE_EXTERNAL_PLUGINS=OFF \
  "-DKRKRSDL2_EXTRA_LDFLAGS=$(IFS=';'; echo "${LDFLAGS[*]}")" > /dev/null
cmake --build "$WORK"
rm -rf "$OUT"
mkdir -p "$OUT"
cp "$WORK/krkrsdl2.js" "$OUT/krkr.js"
cp "$WORK/krkrsdl2.wasm" "$OUT/"
# KiriKiri SDL2's stand-ins for what it lacks (menus, the KAGParser plugin, a default font),
# from the KAG3 it ships (KAG3 license: free to modify and distribute), and ours for common
# plugins (polyfill/plugins). The page runs them before the game's startup.tjs.
cp -r "$ROOT/engines/krkr/polyfill" "$OUT/"
git -C "$SRC" rev-parse HEAD > "$OUT/REVISION"
