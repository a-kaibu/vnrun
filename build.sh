#!/usr/bin/env bash
# Builds the player into dist/: the page (web/) and every engine (engines/*/build.sh).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
mkdir -p "$ROOT/dist"
for b in "$ROOT"/engines/*/build.sh; do
  "$b"
done
cp -r "$ROOT/web/." "$ROOT/dist/"
