#!/usr/bin/env bash
# Builds xsystem4 (third_party/xsystem4, AliceSoft System4, GPL-2.0-or-later) for the browser into
# web/public/engines/system4/. Needs Emscripten (emcc on PATH, or EMSDK set), python3, flex and
# bison. The engine runs its VM under Asyncify and draws with GLES 3 (WebGL2) on a canvas that is
# never shown; each frame is read back and handed to the page (library.js), which shows it with
# WebGPU. libffi, libsndfile and libturbojpeg are replaced by the small shims in shim/.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HERE="$ROOT/engines/system4"
SRC="$ROOT/third_party/xsystem4"
SYS4="$SRC/subprojects/libsys4"
OUT="$ROOT/web/public/engines/system4"
WORK="$ROOT/target/system4"
CGLM_VERSION=v0.9.2
WEBP_VERSION=v1.4.0

if [[ ! -f "$SRC/meson.build" || ! -f "$SYS4/meson.build" ]]; then
  git -C "$ROOT" submodule update --init --recursive third_party/xsystem4
fi
# Our changes for the browser build (patches/xsystem4/*.patch), applied to a clean checkout.
git -C "$SRC" checkout -q -- .
for p in "$ROOT"/patches/xsystem4/*.patch; do
  [[ -e "$p" ]] && git -C "$SRC" apply "$p"
done
if ! command -v emcc > /dev/null; then
  # shellcheck disable=SC1091
  source "${EMSDK:?install Emscripten or set EMSDK}/emsdk_env.sh" > /dev/null
fi
mkdir -p "$WORK/gen" "$WORK/obj"

# cglm (headers only) and libwebp.
if [[ ! -d "$WORK/cglm/include" ]]; then
  git -c advice.detachedHead=false clone -q --depth 1 --branch "$CGLM_VERSION" https://github.com/recp/cglm "$WORK/cglm"
fi
if [[ ! -d "$WORK/libwebp/src" ]]; then
  git -c advice.detachedHead=false clone -q --depth 1 --branch "$WEBP_VERSION" \
    https://chromium.googlesource.com/webm/libwebp "$WORK/libwebp"
fi
if [[ ! -f "$WORK/libwebp.a" ]]; then
  objs=()
  for c in "$WORK"/libwebp/src/{dec,dsp,enc,utils}/*.c "$WORK"/libwebp/sharpyuv/*.c; do
    o="$WORK/obj/webp_$(basename "$(dirname "$c")")_$(basename "${c%.c}").o"
    emcc -O2 -I"$WORK/libwebp" -I"$WORK/libwebp/src" -c "$c" -o "$o"
    objs+=("$o")
  done
  emar rcs "$WORK/libwebp.a" "${objs[@]}"
fi

# Generated sources: version.h and libsys4's .ini parser.
sed "s/@VCS_TAG@/$(git -C "$SRC" rev-parse --short HEAD)/" "$SRC/src/version.h.in" > "$WORK/gen/version.h"
flex -o "$WORK/gen/ini_lexer.yy.c" "$SYS4/src/ini_lexer.l"
bison --defines="$WORK/gen/ini_parser.tab.h" --output="$WORK/gen/ini_parser.tab.c" "$SYS4/src/ini_parser.y"

PORTS=(-sUSE_SDL=2 -sUSE_FREETYPE=1 -sUSE_ZLIB=1 -sUSE_LIBPNG=1 -sUSE_OGG=1 -sUSE_VORBIS=1)
CFLAGS=(
  -O2 -std=gnu11 -D_DEFAULT_SOURCE -DRELEASE -DUSE_GLES '-DXSYS4_DATA_DIR="/xsys4"'
  -Wno-unused-parameter -Wno-deprecated-non-prototype -Wno-incompatible-function-pointer-types
  -I"$SRC/include" -I"$SYS4/include" -I"$SYS4/src" -I"$WORK/cglm/include" -I"$WORK/libwebp/src"
  -I"$HERE/shim" -I"$WORK/gen" "${PORTS[@]}"
)

# Source lists as in upstream's meson.build files.
list() { sed -n "/^$2 = \[/,/^\]/p" "$1" | grep -o "'[^']*\.c'" | tr -d "'"; }
SOURCES=()
while read -r f; do SOURCES+=("$SYS4/$f"); done < <(list "$SYS4/meson.build" system4)
SOURCES+=("$WORK/gen/ini_lexer.yy.c" "$WORK/gen/ini_parser.tab.c")
HLL=()
while read -r f; do
  case $f in
    hll/*) HLL+=("$SRC/src/$f") ;;
    *) SOURCES+=("$SRC/src/$f") ;;
  esac
done < <(list "$SRC/src/meson.build" xsystem4)
SOURCES+=("$SRC/src/movie_plmpeg.c" "$HERE/shim/ffi.c" "$HERE/shim/sndfile.c")

# The HLL libraries are compiled through their generated call wrappers (shim/ffi.h).
python3 "$HERE/gen_hll.py" "$WORK/gen/hll" "${HLL[@]}" -- emcc "${CFLAGS[@]}"
SOURCES+=("$WORK"/gen/hll/*.c)

# Whether object $1 is older than anything its .d file lists.
stale() {
  [[ -f $1 && -f ${1%.o}.d ]] || return 0
  local dep
  for dep in $(sed -e 's/\\$//' -e 's/^[^:]*://' "${1%.o}.d"); do
    [[ $dep -nt $1 ]] && return 0
  done
  return 1
}
OBJS=()
for f in "${SOURCES[@]}"; do
  rel="${f#"$ROOT"/}"
  o="$WORK/obj/$(printf '%s' "${rel%.c}" | tr '/' '_').o"
  stale "$o" && emcc "${CFLAGS[@]}" -MMD -MF "${o%.o}.d" -c "$f" -o "$o"
  OBJS+=("$o")
done
emcc -O2 -isystem "$(dirname "$(command -v emcc)")/third_party" -I"$HERE/shim" \
  -c "$HERE/shim/turbojpeg.c" -o "$WORK/obj/turbojpeg.o"
OBJS+=("$WORK/obj/turbojpeg.o")

rm -rf "$OUT"
mkdir -p "$OUT"
emcc -O2 "${OBJS[@]}" "$WORK/libwebp.a" "${PORTS[@]}" \
  -sMIN_WEBGL_VERSION=2 -sMAX_WEBGL_VERSION=2 -sFULL_ES3=1 \
  -sASYNCIFY -sASYNCIFY_STACK_SIZE=65536 -sALLOW_MEMORY_GROWTH=1 -sSTACK_SIZE=4MB \
  -sMODULARIZE=1 -sEXPORT_ES6=1 -sEXPORT_NAME=createSystem4 -sENVIRONMENT=web -sINVOKE_RUN=0 \
  -sFORCE_FILESYSTEM=1 --profiling-funcs \
  "-sEXPORTED_FUNCTIONS=_main,_vnrun_mouse,_vnrun_wheel" \
  -sEXPORTED_RUNTIME_METHODS=callMain,FS,HEAPU8 \
  --embed-file "$SRC/shaders@/xsys4/shaders" \
  "--js-library=$HERE/library.js" \
  -o "$OUT/system4.js"
# The default font for text outside the game's .fnl (VL Gothic, its license beside it).
mkdir -p "$OUT/fonts"
cp "$SRC/fonts/VL-Gothic-Regular.ttf" "$SRC/fonts/VL-Gothic-Regular.license" "$OUT/fonts/"
git -C "$SRC" rev-parse HEAD > "$OUT/REVISION"
