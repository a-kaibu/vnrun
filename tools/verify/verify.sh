#!/usr/bin/env bash
# Checks vnrun against the trials registered in vnbox (targets.tsv: id, engine, title, URL, from
# vnbox's results/batch/recheck_all.tsv and results/verified.tsv). Each title is downloaded,
# unpacked, served with vnrun-devserver and driven in headless Chromium (tools/shot.js); the
# screenshots and logs go to $VNRUN_TRIALS/_out/ and are judged by eye into status.tsv, from which
# matrix.py writes docs/verification.md.
#
#   tools/verify/verify.sh boot <id>          download, boot to the title screen, delete the game
#   tools/verify/verify.sh all [engine]       boot every title not yet in $VNRUN_TRIALS/_out/ran.tsv
#   tools/verify/verify.sh run <id> <steps>   download (kept) and run tools/shot.js steps
#
# Needs: ./build.sh done (dist/), cargo build --release -p vnrun-devserver -p vnrun-prep,
# unar, 7z, convmv. Fonts for RealLive go in web/public/fonts/ (docs/protocol.md).
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
T="${VNRUN_TRIALS:-$ROOT/target/trials}"
OUT="$T/_out"
PORT="${VNRUN_PORT:-8102}"
mkdir -p "$T/_dl" "$OUT"

field() { awk -F'\t' -v i="$1" -v n="$2" '$1 == i { print $n }' "$HERE/targets.tsv"; }

# Unpacks the title into $T/<id> (once). Archive names are often Shift_JIS.
fetch() {
  local id=$1 url d f
  url=$(field "$id" 4)
  d="$T/$id"
  [[ -d $d ]] && return 0
  f="$T/_dl/$id.${url##*.}"
  [[ -s $f ]] || curl -sSL --retry 3 --max-time 1800 -o "$f" "$url" || return 1
  mkdir -p "$d.tmp"
  unar -q -f -e cp932 -o "$d.tmp" "$f" > /dev/null 2>&1 || 7z x -y -mcp=932 -o"$d.tmp" "$f" > /dev/null 2>&1
  # Archives inside the archive.
  find "$d.tmp" -maxdepth 3 -type f \( -iname '*.zip' -o -iname '*.lzh' -o -iname '*.7z' -o -iname '*.rar' \) |
    head -3 | while IFS= read -r n; do unar -q -f -e cp932 -o "$(dirname "$n")" "$n" > /dev/null 2>&1; done
  convmv -f cp932 -t utf8 -r --notest "$d.tmp" > /dev/null 2>&1
  # Some archives hold the same folder under both encodings; merge what convmv skipped.
  find "$d.tmp" -depth -mindepth 1 | while IFS= read -r p; do
    local b u t
    b=$(basename "$p")
    printf '%s' "$b" | iconv -f utf-8 -t utf-8 > /dev/null 2>&1 && continue
    u=$(printf '%s' "$b" | iconv -f cp932 -t utf-8 2> /dev/null) || continue
    t="$(dirname "$p")/$u"
    if [[ -d $p ]]; then mkdir -p "$t" && cp -rn "$p"/. "$t"/ 2> /dev/null; rm -rf "$p"; else mv -n "$p" "$t"; fi
  done
  mv "$d.tmp" "$d"
  rm -f "$f"
}

# The game root: the shallowest folder holding the engine's marker file.
game_root() {
  local m
  case $2 in
    siglus) m=(-iname scene.pck) ;;
    reallive) m=(-iname seen.txt) ;;
    bgi) m=(-iname sysgrp.arc -o -iname system.arc) ;;
    softpal) m=(-iname data.pac) ;;
    artemis) m=(-iname '*.pfs') ;;
    kirikiri) m=(-iname '*.xp3') ;;
    yuris) m=(-iname ysbin.ypf -o -iname yscfg.dat -o -ipath '*/pac/*.ypf') ;;
    nexas) m=(-iname script.pac) ;;
    advhd) m=(-iname rio.arc) ;;
    tmrhiro) m=(-iname srp.pac -o -iname mg_data.mbl) ;;
    livemaker) m=(-iname live.dll -o -iname game.ext) ;;
    catsystem2) m=(-iname scene.int) ;;
    entis) m=(-iname '*.csx' -o -iname '*.noa') ;;
    system4) m=(-iname '*.ain') ;;
    qlie) m=(-ipath '*/gamedata/data0.pack') ;;
    escude) m=(-iname script.bin) ;;
    interheart) m=(-iname data.fpk) ;;
    anim) m=(-iname '*_sce.dat') ;;
    cyberworks) m=(-iname arc01.dat) ;;
    lilim) m=(-iname scr.aos) ;;
    silkys) m=(-iname layer.arc) ;;
    shiina) m=(-iname '*.war') ;;
    nscripter) m=(-iname nscript.dat -o -iname 0.txt -o -iname 00.txt -o -iname '*.nsa' -o -iname '*.sar') ;;
    html) m=(-iname package.json -o -iname index.html -o -iname package.nw -o -iname nw.pak -o -iname app.asar) ;;
  esac
  # Older YU-RIS releases keep everything in pac/*.ypf; the game root is the folder above pac/.
  # Entis GLS keeps its archives in Data/ beside the exe and the .csx script.
  local up='/pac$'
  [[ $2 == entis ]] && up='/data$'
  [[ $2 == qlie ]] && up='/gamedata$'
  find "$1" -type f \( "${m[@]}" \) -printf '%d %h\n' 2> /dev/null | sort -n | head -1 | cut -d' ' -f2- | sed "s#$up##I"
}

# Serves the title and drives it; prints the game root, or nothing when it has none.
drive() {
  local id=$1 steps=$2 eng root srv
  eng=$(field "$id" 2)
  root=$(game_root "$T/$id" "$eng")
  if [[ -z $root ]]; then
    find "$T/$id" | head -80 > "$OUT/${id}_files.txt"
    return 1
  fi
  "$ROOT/target/release/vnrun-prep" "$root" > "$OUT/${id}_prep.log" 2>&1
  "$ROOT/target/release/vnrun-devserver" --dist "$ROOT/dist" --game "$root" --saves "$T/_saves/$id" \
    --addr "127.0.0.1:$PORT" --engine "$eng" > "$OUT/${id}_server.log" 2>&1 &
  srv=$!
  sleep 1
  timeout 900 node "$ROOT/tools/shot.js" "http://127.0.0.1:$PORT/" "$OUT/$id" "$steps"
  kill "$srv" 2> /dev/null
  wait "$srv" 2> /dev/null
}

BOOT_STEPS="wait:45,shot:boot,wait:60,shot:title,at:640;400,wait:20,shot:click"

boot() {
  local id=$1 r
  if ! fetch "$id"; then
    r=DLFAIL
  elif drive "$id" "$BOOT_STEPS" > /dev/null 2>&1; then
    r=RAN
  else
    r=NOROOT
  fi
  printf '%s\t%s\t%s\n' "$id" "$(field "$id" 2)" "$r" >> "$OUT/ran.tsv"
  rm -rf "${T:?}/$id"
}

case ${1:-} in
  boot) boot "$2" ;;
  all)
    touch "$OUT/ran.tsv"
    awk -F'\t' -v e="${2:-}" 'e == "" || $2 == e { print $1 }' "$HERE/targets.tsv" | while read -r id; do
      grep -q "^$id	" "$OUT/ran.tsv" && continue
      # Wait for disk: a few trials unpack to several GB.
      while (($(df --output=avail -k "$T" | tail -1) < 5000000)); do sleep 30; done
      boot "$id"
    done
    ;;
  run) fetch "$2" && drive "$2" "$3" ;;
  *)
    sed -n '2,13p' "$0"
    exit 2
    ;;
esac
