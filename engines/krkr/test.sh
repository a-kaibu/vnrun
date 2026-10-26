#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
mkdir -p "$ROOT/target/krkr-tests"
"${CXX:-c++}" -std=c++14 -Wall -Wextra -Werror \
  "$ROOT/engines/krkr/tests/xp3_akabei.cpp" -o "$ROOT/target/krkr-tests/xp3_akabei"
"$ROOT/target/krkr-tests/xp3_akabei"

"${CXX:-c++}" -std=c++14 -Wall -Wextra -Werror \
  "$ROOT/engines/krkr/tests/xp3_cxdec.cpp" -o "$ROOT/target/krkr-tests/xp3_cxdec"
"$ROOT/target/krkr-tests/xp3_cxdec"
