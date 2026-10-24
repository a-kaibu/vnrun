#!/usr/bin/env bash
# Fills Emscripten's port cache from git for ports whose source Emscripten downloads as a GitHub
# "archive" (zip/tarball of a tag). Use it where those downloads are refused (some proxies allow
# git and release assets but not archives). Each port is cloned at the tag the installed
# Emscripten wants, into the directory it expects, and marked as unpacked.
#
#   tools/emscripten-ports.sh [port...]   (default: the ports the ONScripter build uses)
set -euo pipefail

if ! command -v emcc > /dev/null; then
  # shellcheck disable=SC1091
  source "${EMSDK:?install Emscripten or set EMSDK}/emsdk_env.sh" > /dev/null
fi
EM="$(dirname "$(readlink -f "$(command -v emcc)")")"
PORTS_DIR="$(em-config CACHE)/ports"
[[ $# -gt 0 ]] || set -- zlib sdl2 sdl2_image sdl2_ttf sdl2_mixer freetype bzip2

for name in "$@"; do
  # The port's URL and unpacked directory, from Emscripten's own port definition.
  read -r url subdir < <(python3 - "$EM/tools/ports" "$name" <<'EOF'
import re, sys
src = open(f"{sys.argv[1]}/{sys.argv[2]}.py").read()
# Only the module's one-line constants are needed (port files import Emscripten's tools).
env = {}
for k, v in re.findall(r"^([A-Z_]+) = (.+?)(?:\s*#.*)?$", src, re.M):
    try:
        env[k] = eval(v, {}, env)
    except SyntaxError:
        pass
url = eval(re.search(r"fetch_project\('[^']+', (f?'[^']+')", src).group(1), {}, env)
subdir = eval(re.search(r"get_dir\('[^']+', ([^)]+)\)", src).group(1), {}, env)
print(url, subdir)
EOF
  )
  dest="$PORTS_DIR/$name"
  if [[ -f "$dest/.emscripten_url" && "$(cat "$dest/.emscripten_url")" == "$url" ]]; then
    continue
  fi
  # .../<owner>/<repo>/archive/[refs/tags/]<ref>.<ext>
  repo="$(sed -E 's#(https://github.com/[^/]+/[^/]+)/archive/.*#\1#' <<< "$url")"
  ref="$(sed -E 's#.*/archive/(refs/tags/)?##; s#\.(zip|tar\.gz)$##' <<< "$url")"
  echo "port $name: $repo @ $ref"
  rm -rf "$dest"
  git -c advice.detachedHead=false clone -q --depth 1 --branch "$ref" "$repo" "$dest/$subdir"
  echo "$url" > "$dest/.emscripten_url"
done
