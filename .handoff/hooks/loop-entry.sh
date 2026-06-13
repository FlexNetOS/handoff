#!/usr/bin/env bash
# SessionStart hook — rehydrate kernel state and auto-invoke the handoff loop.
#
# Wired from handoff/.claude/settings.json (project layer). The kernel's own
# contract (.handoff/hooks/hooks.toml) names SessionStart as the loop entry point;
# this script is the executable realization of that for the Claude Code harness.
#
# Behavior: print the compact resume packet (becomes session context), then — only
# when the ledger has a safe next task — emit a directive telling Claude to run the
# `handoff-loop` skill. No work queued → no directive (don't force a loop with
# nothing to do).
set -uo pipefail
cd "${CLAUDE_PROJECT_DIR:-.}" 2>/dev/null || exit 0

# Locate the hf binary (PATH, then debug/release build). Never hard-fail a hook.
HF=""
if command -v hf >/dev/null 2>&1; then HF="hf"
elif [ -x target/debug/hf ];   then HF="./target/debug/hf"
elif [ -x target/release/hf ]; then HF="./target/release/hf"
fi
if [ -z "$HF" ]; then
  echo "[handoff] hf not built — run 'cargo build -p hf' to enable the loop."
  exit 0
fi

# Rehydrate (compact packet → session context).
"$HF" resume --compact 2>/dev/null || true

# Detect a safe next task from ledger truth (resume --json).
NEXT="$("$HF" resume --json 2>/dev/null | python3 -c '
import sys, json
try:
    d = json.load(sys.stdin)
    t = d.get("next_task_id") or ""
    if (not t) and isinstance(d.get("next_command"), str) and d["next_command"].startswith("hf claim "):
        t = d["next_command"].split()[-1]
    print("" if t in ("", "done") else t)
except Exception:
    print("")
' 2>/dev/null || true)"

if [ -n "$NEXT" ]; then
  cat <<EOF

[handoff-loop] Ledger has a safe next task: ${NEXT}.
ACTION: invoke the \`handoff-loop\` skill to continue the autonomous Continuity Kernel
loop (reconcile drift → research → implement → verify → gatekeeper verdict → ship →
handoff), one witnessed task per cycle. The packet above is rendered state and may be
stale — the loop re-derives truth from Git > ledger > cards first.
Skip ONLY if the user's first request is a one-off question unrelated to the kernel.
EOF
fi
exit 0
