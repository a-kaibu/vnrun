#!/usr/bin/env bash
# Builds the player into dist/: every engine (engines/*/build.sh, into web/public/engines/),
# then the page (web/, with Vite+).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
for b in "$ROOT"/engines/*/build.sh; do
  "$b"
done
cd "$ROOT/web"
npm ci
npx vp build
