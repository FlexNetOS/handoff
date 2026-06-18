#!/usr/bin/env bash
# Project status line for Claude Code.
# Prints the current git branch and the next safe handoff task when hf is built.

set -uo pipefail
cd "${CLAUDE_PROJECT_DIR:-.}" 2>/dev/null || exit 0

BRANCH=$(git branch --show-current 2>/dev/null || echo "unknown")
printf "handoff [%s]" "$BRANCH"

HF=""
if command -v hf >/dev/null 2>&1; then HF="hf"
elif [ -x target/debug/hf ];   then HF="./target/debug/hf"
elif [ -x target/release/hf ]; then HF="./target/release/hf"
fi

if [ -n "$HF" ]; then
  NEXT=$("$HF" resume --json 2>/dev/null | python3 -c '
import json, sys
try:
    d = json.load(sys.stdin)
    print(d.get("next_task", ""))
except Exception:
    pass
' 2>/dev/null || true)
  if [ -n "$NEXT" ]; then
    printf " -> %s" "$NEXT"
  fi
fi

printf "\n"
