#!/usr/bin/env bash
# SessionEnd hook — continuity safety net.
#
# If a session ends without the loop reaching its own checkpoint/handoff step,
# witness the current state and re-render the packet so the next session resumes
# from truth, not from a half-finished turn. Mirrors the kernel contract
# (.handoff/hooks/hooks.toml SessionStop: hf checkpoint --auto && hf handoff).
#
# Idempotent and best-effort: never block session teardown.
set -uo pipefail
cd "${CLAUDE_PROJECT_DIR:-.}" 2>/dev/null || exit 0

HF=""
if command -v hf >/dev/null 2>&1; then HF="hf"
elif [ -x target/debug/hf ];   then HF="./target/debug/hf"
elif [ -x target/release/hf ]; then HF="./target/release/hf"
fi
[ -z "$HF" ] && exit 0

# Witness whatever progress exists, then re-render the packet/active from ledger truth.
"$HF" checkpoint --auto --quiet 2>/dev/null || true
"$HF" handoff 2>/dev/null || true
exit 0
