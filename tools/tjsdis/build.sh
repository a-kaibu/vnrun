#!/bin/sh
# Builds tjsdis, a native disassembler for compiled TJS2 scripts (TJS2100 bytecode, as games
# ship their system/*.tjs): tools/tjsdis/tjsdis FILE... prints krkrz's own disassembly.
# Needs the krkrsdl2 submodule (its krkrz/tjs2 sources); SJIS conversion and regexps are stubbed.
set -e
cd "$(dirname "$0")"
K=../../third_party/krkrsdl2/external/krkrz
F="-O1 -w -std=gnu++14 -DKRKRZ_CASE_INSENSITIVE -I$K/tjs2 -I$K/external/onig/src -I../../third_party/krkrsdl2/src/core/sdl2 -I../../third_party/krkrsdl2/src/config"
mkdir -p obj
for f in $K/tjs2/*.cpp; do g++ -c $F -o obj/$(basename $f .cpp).o $f & done
wait
g++ $F main.cpp stubs.cpp obj/*.o -o tjsdis
