#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
NAME="rusty-idd-core"
OUT="/mnt/data/${NAME}-v2.zip"
cd "$(dirname "$ROOT")"
zip -r "$OUT" "$(basename "$ROOT")" \
  -x "$(basename "$ROOT")/target/*" \
  -x "$(basename "$ROOT")/.git/*"
echo "$OUT"
