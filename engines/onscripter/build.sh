#!/usr/bin/env bash
# Builds ONScripter-Yuri (third_party/onscripter-yuri, NScripter, GPL-2.0) for the browser into
# web/public/engines/onscripter/. Needs Emscripten (emcc on PATH, or EMSDK set). The engine runs
# its own SDL loop under Asyncify; frames go to the page (library.js), which shows them with
# WebGPU.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC="$ROOT/third_party/onscripter-yuri"
OUT="$ROOT/web/public/engines/onscripter"
WORK="$ROOT/target/onscripter"
LUA_VERSION=5.4.4

if [[ ! -f "$SRC/CMakeLists.txt" ]]; then
  git -C "$ROOT" submodule update --init third_party/onscripter-yuri
fi
# Our fixes for the browser build (patches/onscripter-yuri/*.patch), applied to a clean checkout.
git -C "$SRC" checkout -q -- .
for p in "$ROOT"/patches/onscripter-yuri/*.patch; do
  [[ -e "$p" ]] && git -C "$SRC" apply "$p"
done
if ! command -v emcc > /dev/null; then
  # shellcheck disable=SC1091
  source "${EMSDK:?install Emscripten or set EMSDK}/emsdk_env.sh" > /dev/null
fi

PORTS=(
  -sUSE_SDL=2 -sUSE_SDL_TTF=2 -sUSE_BZIP2=1
  -sUSE_SDL_IMAGE=2 "-sSDL2_IMAGE_FORMATS=[bmp,png,jpg]"
  -sUSE_SDL_MIXER=2 "-sSDL2_MIXER_FORMATS=[ogg,mp3,mid]" -sUSE_OGG=1 -sUSE_VORBIS=1 -sUSE_MPG123=1
)
CFLAGS=(-O2 -Wno-invalid-source-encoding -Wno-deprecated-declarations -I"$WORK/lua/src")

# Lua (ONScripter's lua command), from its GitHub mirror.
if [[ ! -f "$WORK/lua/liblua.a" ]]; then
  rm -rf "$WORK/lua"
  mkdir -p "$WORK/lua"
  git -c advice.detachedHead=false clone -q --depth 1 --branch "v$LUA_VERSION" https://github.com/lua/lua "$WORK/lua"
  mkdir -p "$WORK/lua/src"
  mv "$WORK/lua"/*.[ch] "$WORK/lua/src/"
  # lua.hpp ships in release tarballs only.
  printf 'extern "C" {\n#include "lua.h"\n#include "lualib.h"\n#include "lauxlib.h"\n}\n' > "$WORK/lua/src/lua.hpp"
  objs=()
  for c in "$WORK/lua/src"/*.c; do
    case "$(basename "$c")" in lua.c | luac.c | onelua.c | ltests.c) continue ;; esac
    emcc -O2 -DLUA_COMPAT_5_3 -c "$c" -o "${c%.c}.o"
    objs+=("${c%.c}.o")
  done
  emar rcs "$WORK/lua/liblua.a" "${objs[@]}"
fi

# Sources as listed in upstream's CMakeLists.txt (ONSYURI_CODE).
SOURCES=()
while read -r f; do SOURCES+=("$SRC/$f"); done < <(
  sed -n '/^set(ONSYURI_CODE/,/^)/p' "$SRC/CMakeLists.txt" | grep -o 'src/[^ ]*\.cpp'
)
mkdir -p "$WORK/obj"
OBJS=()
for f in "${SOURCES[@]}"; do
  o="$WORK/obj/$(basename "${f%.cpp}").o"
  em++ -std=c++11 "${CFLAGS[@]}" "${PORTS[@]}" \
    -DUSE_LUA -DUSE_BUILTIN_LAYER_EFFECTS -DENABLE_1BYTE_CHAR -DWEB -DLINUX \
    -c "$f" -o "$o"
  OBJS+=("$o")
done

rm -rf "$OUT"
mkdir -p "$OUT"
em++ -O2 "${OBJS[@]}" "$WORK/lua/liblua.a" "${PORTS[@]}" \
  -sASYNCIFY -sALLOW_MEMORY_GROWTH=1 -sSTACK_SIZE=4MB \
  -sMODULARIZE=1 -sEXPORT_ES6=1 -sEXPORT_NAME=createOnscripter -sENVIRONMENT=web -sINVOKE_RUN=0 \
  -sFORCE_FILESYSTEM=1 \
  "-sEXPORTED_FUNCTIONS=_main,_vnrun_mouse,_vnrun_wheel" \
  -sEXPORTED_RUNTIME_METHODS=callMain,FS,HEAPU8,UTF8ToString \
  "--js-library=$ROOT/engines/onscripter/library.js" \
  -o "$OUT/onscripter.js"
git -C "$SRC" rev-parse HEAD > "$OUT/REVISION"
