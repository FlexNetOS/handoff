#!/usr/bin/env bash
# handoff-lib.sh — shared, sourceable helpers for the .handoff continuity layer.
#
# Canonical home for the .gitignore residency guards (ADR-0004 §3.3/§6 rev,
# HFTASK-0035/0037) plus the redb-cutover hygiene additions (HFTASK-0053 follow-up:
# migration artifacts `*.sqlite.bak` / `*.redb.tmp` must never churn git or trip
# `hf drift`'s deny_without_claim). `handoff-loop-init.sh` sources this; the canonical
# fleet generator `fleet-rollout.sh` keeps its own copies in sync (a tiny follow-up can
# converge them onto this lib).
#
# Pure functions, no top-level side effects — safe to `source`.
set -uo pipefail

# Locate the hf binary: PATH first, then the kernel build outputs. Echoes the binary
# (or empty). Never hard-fails.
hf_bin() {
  if command -v hf >/dev/null 2>&1; then echo "hf"; return 0; fi
  local kernel="${HANDOFF_KERNEL_HOME:-}"
  if [ -n "$kernel" ]; then
    [ -x "$kernel/target/release/hf" ] && { echo "$kernel/target/release/hf"; return 0; }
    [ -x "$kernel/target/debug/hf" ]   && { echo "$kernel/target/debug/hf"; return 0; }
  fi
  echo ""; return 1
}

# Does this repo's .gitignore already ignore PATH? (git check-ignore is the truth, the
# same predicate `hf fleet status` uses.)
_gi_ignored() { git -C "$1" check-ignore -q "$2" 2>/dev/null; }

# Ensure the ledger-residency + migration-artifact guards. Returns 0 if it ADDED any
# missing guard, 1 if all were already present (idempotent). Mirrors
# fleet-rollout.sh::ensure_ledger_guard and EXTENDS it with the redb backup/temp guard.
ensure_ledger_guard() {
  local dir="$1" changed=0 need_header=1
  _add() {
    local path="$1"
    _gi_ignored "$dir" "$path" && return
    if [ "$need_header" = 1 ]; then
      {
        echo ""
        echo "# handoff continuity: local ledger + migration artifacts are gitignored"
        echo "# (ADR-0004 §3.3/§6 rev, HFTASK-0035; redb cutover HFTASK-0053)"
      } >> "$dir/.gitignore"
      need_header=0
    fi
    echo "$path" >> "$dir/.gitignore"
    changed=1
  }
  _add ".handoff/**/ledger.db"
  _add ".handoff/**/*.db-wal"
  _add ".handoff/**/*.db-shm"
  _add ".handoff/**/*.sqlite.bak"   # redb-cutover migration backup (HFTASK-0053)
  _add ".handoff/**/*.redb.tmp"     # redb-cutover migration temp (HFTASK-0053)
  return $((1 - changed))
}

# Ensure the active.md derived-view guard (HFTASK-0037). 0 if added, 1 if already ignored.
ensure_active_md_guard() {
  local dir="$1"
  _gi_ignored "$dir" .handoff/active.md && return 1
  {
    echo ""
    echo "# handoff continuity: active.md is a local derived view (HFTASK-0037)"
    echo "/.handoff/active.md"
  } >> "$dir/.gitignore"
  return 0
}

# Is FILE a legacy SQLite ledger (magic "SQLite format 3\0")? Returns 0 if legacy.
# Mirrors ledger::file_is_legacy_sqlite (HFTASK-0053). Empty/missing/redb → non-zero.
ledger_is_legacy_sqlite() {
  local f="$1"
  [ -f "$f" ] || return 1
  # First 16 bytes == "SQLite format 3\0"
  local magic
  magic="$(head -c 16 "$f" 2>/dev/null | tr -d '\0')"
  [ "$magic" = "SQLite format 3" ]
}

# Best-effort, FAIL-CLOSED quiescence check for a repo before any ledger-mutating step
# (the dangerous part of deployment). Returns 0 (quiescent) only when we can positively
# rule out concurrent work; on ANY uncertainty returns 1 (busy) so we never migrate a
# repo with a live loop. Cheap signals only.
repo_quiescent() {
  local dir="$1"
  # 1) A live process whose cwd is inside this repo (hf/cargo/grit).
  if command -v lsof >/dev/null 2>&1; then
    if lsof -a -d cwd -- "$dir" 2>/dev/null | grep -qE '\b(hf|cargo|grit)\b'; then
      return 1
    fi
  fi
  # 2) An active grit agent worktree.
  if compgen -G "$dir/.grit/worktrees/agent-*" >/dev/null 2>&1; then
    return 1
  fi
  # 3) A held lease lock (HFTASK-0048 .handoff/locks/*.lock) whose holder PID is alive.
  local lk pid
  if compgen -G "$dir/.handoff/locks/*.lock" >/dev/null 2>&1; then
    for lk in "$dir"/.handoff/locks/*.lock; do
      pid="$(grep -oE '"pid"[: ]+[0-9]+' "$lk" 2>/dev/null | grep -oE '[0-9]+' | head -1)"
      if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then return 1; fi
    done
  fi
  # 4) Ledger written within the last 120s (a loop is probably mid-write).
  local db
  for db in "$dir"/.handoff/**/ledger.db "$dir"/.handoff/ledger.db; do
    [ -f "$db" ] || continue
    local age now mt
    now="$(date +%s 2>/dev/null || echo 0)"
    mt="$(stat -c %Y "$db" 2>/dev/null || echo 0)"
    age=$(( now - mt ))
    [ "$now" != 0 ] && [ "$mt" != 0 ] && [ "$age" -lt 120 ] && return 1
  done
  return 0
}
