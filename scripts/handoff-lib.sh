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

# Resolve a path to its canonical form (following symlinks). realpath > readlink -f > as-is.
_realpath() {
  local p="$1"
  if command -v realpath >/dev/null 2>&1; then realpath "$p" 2>/dev/null
  elif command -v readlink >/dev/null 2>&1; then readlink -f "$p" 2>/dev/null
  else echo "$p"; fi
}

# Converge stale shadow `hf` copies on PATH to a symlink into the canonical build
# (ADR-0006 / HFTASK-0096). After `cargo install --path hf` the fresh binary lands in the cargo
# bin, but a real COPY of `hf` earlier on PATH (e.g. ~/.local/bin/hf) keeps shadowing it — so
# `hf` serves the OLD binary and freshly-added verbs report "unknown command" until a manual cp,
# silently defeating the rung-0 staleness automation. This walks PATH in order and, for every
# `hf` that resolves BEFORE the canonical one and differs from it, replaces the shadow with a
# symlink -> canonical (never a churning copy). Idempotent (a shadow already symlinked to
# canonical is left untouched), fail-soft + LOUD (warns, never aborts) when a dir/file isn't
# writable. Args: $1 = canonical hf path (the freshly-built/installed binary); $2 = DRY (1 = show
# only). Returns 0 always.
reconcile_hf_path() {
  local canonical="$1" dry="${2:-0}"
  [ -n "$canonical" ] && [ -e "$canonical" ] || return 0
  local canon_real
  canon_real="$(_realpath "$canonical")"
  [ -n "$canon_real" ] || return 0
  local oldifs="$IFS" dir target target_real seen_canon=0
  IFS=:
  for dir in $PATH; do
    [ -n "$dir" ] || continue
    target="$dir/hf"
    [ -e "$target" ] || [ -L "$target" ] || continue
    target_real="$(_realpath "$target")"
    if [ "$target_real" = "$canon_real" ]; then
      seen_canon=1            # canonical (or a symlink to it) — reached it; stop converging
      continue
    fi
    [ "$seen_canon" = 0 ] || continue   # only shadows that resolve BEFORE canonical matter
    if [ "$dry" = 1 ]; then
      echo "    DRY: ln -sf '$canonical' '$target'  (stale shadow: $target_real)"
    elif [ -w "$dir" ] && { [ ! -e "$target" ] || [ -w "$target" ]; }; then
      if ln -sf "$canonical" "$target" 2>/dev/null; then
        echo "[init] reconciled stale shadow hf: $target -> $canonical (ADR-0006)"
      else
        echo "[init] WARNING: could not symlink $target -> $canonical (left as-is)"
      fi
    else
      echo "[init] WARNING: stale shadow hf at $target shadows the fresh $canonical but is not writable — fix PATH or run: ln -sf '$canonical' '$target'"
    fi
  done
  IFS="$oldifs"
  return 0
}

# Does this repo's .gitignore already ignore PATH? (git check-ignore is the truth, the
# same predicate `hf fleet status` uses.)
_gi_ignored() { git -C "$1" check-ignore -q "$2" 2>/dev/null; }

# Ensure the BINARY-CACHE residency + migration-artifact guards. Returns 0 if it ADDED any
# missing guard, 1 if all were already present (idempotent). ADR-0018 D1 (HFTASK-0067): the
# binary ledger (+ rvf sidecar) is a gitignored LOCAL CACHE — the committed truth is the
# `.handoff/ledger.events.jsonl` text export. This guard does NOT ignore the rendered views
# (packets/, active.md, deliveries/), which are now committed.
ensure_ledger_guard() {
  local dir="$1" changed=0 need_header=1
  _add() {
    local path="$1"
    _gi_ignored "$dir" "$path" && return
    if [ "$need_header" = 1 ]; then
      {
        echo ""
        echo "# handoff continuity: binary ledger cache + migration artifacts are gitignored"
        echo "# (committed truth = .handoff/ledger.events.jsonl — ADR-0018 D1 / HFTASK-0067)"
      } >> "$dir/.gitignore"
      need_header=0
    fi
    echo "$path" >> "$dir/.gitignore"
    changed=1
  }
  _add ".handoff/**/ledger.db"
  _add ".handoff/**/*.db-wal"
  _add ".handoff/**/*.db-shm"
  _add ".handoff/**/*.rvf"          # RVF vector sidecar — binary cache (ADR-0018 D1)
  _add ".handoff/**/*.rvf.lock"
  _add ".handoff/**/*.sqlite.bak"   # redb-cutover migration backup (HFTASK-0053)
  _add ".handoff/**/*.redb.tmp"     # redb-cutover migration temp (HFTASK-0053)
  # Older fleet rollouts also wrote a nested `.handoff/.gitignore` with `!ledger.db` to
  # force-track the binary SQLite ledger. A parent `.gitignore` cannot override that nested
  # negation, so append a later nested rule that restores the ADR-0018 D1 model: binary ledger
  # caches are ignored, and `.handoff/ledger.events.jsonl` is the committed truth.
  if [ -f "$dir/.handoff/.gitignore" ] && ! _gi_ignored "$dir" ".handoff/ledger.db"; then
    {
      echo ""
      echo "# handoff continuity: binary ledger cache is gitignored"
      echo "# (committed truth = ledger.events.jsonl — ADR-0018 D1 / HFTASK-0067)"
      echo "ledger.db"
      echo "ledger.db-wal"
      echo "ledger.db-shm"
    } >> "$dir/.handoff/.gitignore"
    changed=1
  fi
  return $((1 - changed))
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
