#!/usr/bin/env bash
# SessionEnd hook — continuity safety net.
#
# If a session ends without the loop reaching its own checkpoint/handoff step,
# witness the current state and re-render the packet so the next session resumes
# from truth, not from a half-finished turn. Mirrors the kernel contract
# (.handoff/hooks/hooks.toml SessionEnd: hf checkpoint --auto && hf handoff && hf export
# && hf sync --auto — the canonical event, renamed from SessionStop in HFTASK-0069).
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
# ADR-0018 D1 (HFTASK-0067): refresh the COMMITTED continuity truth — the deterministic
# `.handoff/ledger.events.jsonl` text export — from the (now-quiescent) binary ledger. Runs as a
# separate process at this commit point, NOT inside a mutating verb (redb is single-writer).
"$HF" export 2>/dev/null || true
# HFTASK-0052 / fleet auto-sync: roll every member's per-repo ledger into the central
# FLEET ledger. Best-effort and idempotent; degrades gracefully when no meta root exists.
"$HF" sync --auto 2>/dev/null || true
exit 0
