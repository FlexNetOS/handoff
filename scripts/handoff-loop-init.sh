#!/usr/bin/env bash
# handoff-loop-init.sh — ALL-IN-ONE .handoff upgrade + sync + deploy, one command.
#
# Brings any repo's continuity layer fully current from a single invocation:
#   1. ensure the `hf` binary on PATH is the pure-Rust redb build (HFTASK-0053, no-C)
#   2. init-or-upgrade the repo's .handoff (portable `hf init` — repo self-identifies)
#   3. install the .gitignore residency + migration-artifact guards
#   4. migrate a legacy SQLite ledger -> redb (out-of-tree backup), ONLY if quiescent
#   5. deploy the auto-loop hooks (SessionStart loop-entry + SessionEnd safety net)
#   6. verify conformance (hf drift + hf fleet status) and render the resume packet
#
# Idempotent and FAIL-CLOSED on the one dangerous step (ledger migration): a repo that
# isn't provably quiescent is reported as deferred, never migrated underneath a live loop.
#
# Usage:
#   scripts/handoff-loop-init.sh [TARGET ...] [flags]
#     TARGET        repo path(s); default = current git repo toplevel
#     --fleet       every present .meta.yaml member (skips non-quiescent for migration)
#     --commit      git add+commit the git-text (.handoff, .gitignore, .claude/settings.json)
#     --push        git push (implies --commit)
#     --no-migrate  skip the ledger migration step entirely
#     --no-hooks    skip auto-loop hook deployment
#     --build-hf    force-rebuild+install the redb hf from the kernel even if PATH hf is fine
#     --dry-run     print what would happen; mutate nothing
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Resolve the handoff KERNEL source root (used only for the optional `cargo install` rebuild of
# the redb hf) + the META_ROOT (used for `--fleet` member discovery). This script runs in TWO
# homes: (a) the handoff dev checkout (`meta/handoff/scripts/`) where `SCRIPT_DIR/..` IS the
# kernel; (b) VENDORED under the harness plugin (`.../skills/handoff-loop-init/scripts/`) where
# `SCRIPT_DIR/..` is NOT a kernel. Detect robustly so the same script works ejected (HFTASK-0065).
_find_meta_root() {  # walk up from $1 for a dir with .meta.yaml + a handoff/ member
  local d="$1"
  while [ -n "$d" ] && [ "$d" != / ]; do
    [ -f "$d/.meta.yaml" ] && { echo "$d"; return 0; }
    d="$(dirname "$d")"
  done
  return 1
}
_is_kernel_home() {  # a handoff kernel source root has the hf crate + the keystone ADR
  [ -f "$1/hf/Cargo.toml" ] && [ -f "$1/docs/adr-0001-flexnetos-autopilot-keystone.md" ]
}
KERNEL_HOME=""
if _is_kernel_home "$(cd "$SCRIPT_DIR/.." && pwd)"; then
  KERNEL_HOME="$(cd "$SCRIPT_DIR/.." && pwd)"          # (a) handoff dev checkout
fi
META_ROOT="$(_find_meta_root "$(pwd)" || _find_meta_root "$SCRIPT_DIR" || echo "")"
# Ejected case: no kernel beside the script — find one via the meta root (for the rebuild path only;
# absent that, Phase 0 falls back to PATH hf and degrades gracefully instead of rebuilding).
if [ -z "$KERNEL_HOME" ] && [ -n "$META_ROOT" ] && _is_kernel_home "$META_ROOT/handoff"; then
  KERNEL_HOME="$META_ROOT/handoff"
fi
[ -z "$META_ROOT" ] && [ -n "$KERNEL_HOME" ] && META_ROOT="$(cd "$KERNEL_HOME/.." && pwd)"
export HANDOFF_KERNEL_HOME="$KERNEL_HOME"
# shellcheck source=scripts/handoff-lib.sh
. "$SCRIPT_DIR/handoff-lib.sh"

DO_FLEET=0 DO_COMMIT=0 DO_PUSH=0 NO_MIGRATE=0 NO_HOOKS=0 BUILD_HF=0 DRY=0
TARGETS=()
for a in "$@"; do
  case "$a" in
    --fleet)      DO_FLEET=1 ;;
    --commit)     DO_COMMIT=1 ;;
    --push)       DO_COMMIT=1; DO_PUSH=1 ;;
    --no-migrate) NO_MIGRATE=1 ;;
    --no-hooks)   NO_HOOKS=1 ;;
    --build-hf)   BUILD_HF=1 ;;
    --dry-run)    DRY=1 ;;
    --*)          echo "unknown flag $a"; exit 2 ;;
    *)            TARGETS+=("$a") ;;
  esac
done

say() { echo "[init] $*"; }
run() { if [ "$DRY" = 1 ]; then echo "    DRY: $*"; else eval "$@"; fi; }

# ── Phase 0: ensure the redb hf binary ──────────────────────────────────────────────
HF="$(hf_bin)"
need_build=0
if [ -z "$HF" ]; then
  say "no hf on PATH or in kernel target — will build"; need_build=1
elif [ "$BUILD_HF" = 1 ]; then
  need_build=1
elif command -v ldd >/dev/null 2>&1 && ldd "$(command -v hf 2>/dev/null || echo /nonexistent)" 2>/dev/null | grep -qi sqlite; then
  say "PATH hf links libsqlite (pre-redb build) — will rebuild the no-C redb binary"; need_build=1
fi
if [ "$need_build" = 1 ]; then
  if [ -z "$KERNEL_HOME" ] || ! _is_kernel_home "$KERNEL_HOME"; then
    # Ejected (vendored under the plugin) with no handoff kernel source reachable: we cannot
    # rebuild hf here. Fail-closed with a NEEDS-HUMAN instruction rather than cd-ing nowhere.
    if [ -n "$HF" ] && [ "$BUILD_HF" = 0 ]; then
      say "WARNING: hf present but may be pre-redb, and no kernel source to rebuild from — proceeding with the existing hf (install the redb hf from meta/handoff to silence this)"
    else
      cat >&2 <<MSG
[init] NEEDS-HUMAN: a redb \`hf\` is required but is not on PATH and no handoff kernel source
       is reachable to build it from. Install it from meta/handoff:
         ( cd <…>/meta/handoff && cargo install --path hf --locked --force )
       then re-run this command.
MSG
      exit 2
    fi
  elif [ "$DRY" = 1 ]; then
    echo "    DRY: (cd $KERNEL_HOME && cargo install --path hf --locked --force)"
  else
    say "building+installing redb hf from $KERNEL_HOME (this may take a minute)…"
    ( cd "$KERNEL_HOME" && cargo install --path hf --locked --force ) || { echo "[init] FATAL: hf build failed"; exit 1; }
  fi
  HF="$(hf_bin)"
fi
[ -z "$HF" ] && HF="hf"
say "hf binary: $HF"
if command -v ldd >/dev/null 2>&1; then
  if ldd "$(command -v "$HF" 2>/dev/null || echo "$HF")" 2>/dev/null | grep -qi sqlite; then
    say "WARNING: hf still links libsqlite — re-run with --build-hf"
  else
    say "hf is C-free (no libsqlite) ✓"
  fi
fi

# ── Resolve targets ─────────────────────────────────────────────────────────────────
fleet_members() {
  [ -f "$META_ROOT/.meta.yaml" ] || return 0
  awk '
    /^[^[:space:]]/ { inproj = ($0 ~ /^projects:/) }
    inproj && /^  [A-Za-z0-9_-]+:[[:space:]]*$/ { gsub(/[ :]/,""); print }
  ' "$META_ROOT/.meta.yaml"
}
if [ "$DO_FLEET" = 1 ]; then
  while IFS= read -r m; do [ -n "$m" ] && [ -d "$META_ROOT/$m/.git" ] && TARGETS+=("$META_ROOT/$m"); done < <(fleet_members)
fi
if [ ${#TARGETS[@]} -eq 0 ]; then
  cur="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
  TARGETS+=("$cur")
fi

INIT=0 GUARD=0 MIGRATED=0 DEFERRED=0 HOOKED=0 OK=0 FAIL=0

deploy_hooks() {
  local dir="$1"
  mkdir -p "$dir/.handoff/hooks" "$dir/.claude"
  # Hook source: the kernel's live hooks when run from a handoff checkout; else the copies
  # vendored beside this script (so the skill stays self-contained when ejected, HFTASK-0065).
  local hooks_src="$KERNEL_HOME/.handoff/hooks"
  [ -d "$hooks_src" ] || hooks_src="$SCRIPT_DIR/hooks"
  local f had=0
  for f in loop-entry.sh session-end.sh hooks.toml; do
    [ -f "$hooks_src/$f" ] || continue
    had=1
    if [ "$DRY" = 1 ]; then echo "    DRY: cp hooks/$f -> $dir/.handoff/hooks/"; else
      cp "$hooks_src/$f" "$dir/.handoff/hooks/$f"; fi
  done
  # No hook sources anywhere (ejected without vendored hooks): do NOT wire settings.json to
  # files that don't exist — skip fail-closed rather than create dangling hook references.
  if [ "$had" = 0 ]; then
    say "  no hook sources found (neither \$KERNEL_HOME/.handoff/hooks nor vendored) — skipping hook wiring"
    return 0
  fi
  # Merge SessionStart/SessionEnd wiring into .claude/settings.json (preserve existing keys).
  if [ "$DRY" = 1 ]; then echo "    DRY: merge SessionStart/SessionEnd into $dir/.claude/settings.json"; return 0; fi
  python3 - "$dir/.claude/settings.json" <<'PY'
import json, os, sys
p = sys.argv[1]
data = {}
if os.path.exists(p):
    try:
        with open(p) as fh: data = json.load(fh)
    except Exception:
        data = {}
hooks = data.setdefault("hooks", {})
def ensure(event, script):
    entries = hooks.setdefault(event, [])
    blob = json.dumps(entries)
    if script in blob:
        return
    entries.append({"hooks": [{"type": "command",
        "command": 'bash "$CLAUDE_PROJECT_DIR/.handoff/hooks/%s"' % script}]})
ensure("SessionStart", "loop-entry.sh")
ensure("SessionEnd", "session-end.sh")
with open(p, "w") as fh:
    json.dump(data, fh, indent=2); fh.write("\n")
PY
}

for dir in "${TARGETS[@]}"; do
  [ -d "$dir/.git" ] || { say "skip $(basename "$dir") (not a git repo)"; continue; }
  name="$(basename "$dir")"
  say "── $name ($dir)"

  # (2) init-or-upgrade .handoff
  if [ -d "$dir/.handoff" ]; then
    say "  .handoff present — upgrading guards/hooks (preserving capsule/cards)"
  else
    say "  no .handoff — portable hf init"
    if [ "$DRY" = 1 ]; then echo "    DRY: (cd $dir && $HF init)"; else
      ( cd "$dir" && "$HF" init >/dev/null 2>&1 ) && { INIT=$((INIT+1)); say "  hf init ✓"; } \
        || { say "  hf init FAILED"; FAIL=$((FAIL+1)); continue; }
    fi
  fi

  # (3) guards
  gl=0; ga=0
  if [ "$DRY" = 1 ]; then echo "    DRY: ensure ledger + active.md + migration-artifact guards"; else
    ensure_ledger_guard "$dir"   && gl=1
    ensure_active_md_guard "$dir" && ga=1
  fi
  [ "$gl" = 1 ] || [ "$ga" = 1 ] && { GUARD=$((GUARD+1)); say "  guards updated (ledger=$gl active=$ga)"; }

  # (4) ledger migration (fail-closed on quiescence)
  if [ "$NO_MIGRATE" = 0 ]; then
    legacy=""
    for db in "$dir"/.handoff/**/ledger.db "$dir"/.handoff/ledger.db; do
      [ -f "$db" ] && ledger_is_legacy_sqlite "$db" && { legacy="$db"; break; }
    done
    if [ -n "$legacy" ]; then
      if repo_quiescent "$dir"; then
        say "  legacy SQLite ledger detected — migrating to redb"
        if [ "$DRY" = 1 ]; then echo "    DRY: (cd $dir && $HF migrate)"; else
          ( cd "$dir" && "$HF" migrate ) && { MIGRATED=$((MIGRATED+1)); say "  hf migrate ✓"; } \
            || { say "  hf migrate FAILED"; FAIL=$((FAIL+1)); }
        fi
      else
        DEFERRED=$((DEFERRED+1))
        say "  legacy ledger but repo NOT quiescent — DEFERRED (run when its loop is idle)"
      fi
    fi
  fi

  # (5) hooks
  if [ "$NO_HOOKS" = 0 ]; then
    deploy_hooks "$dir" && { HOOKED=$((HOOKED+1)); say "  auto-loop hooks deployed"; }
  fi

  # (6) verify + render
  if [ "$DRY" = 0 ]; then
    ( cd "$dir" && "$HF" resume >/dev/null 2>&1 ) || true
    ( cd "$dir" && "$HF" drift >/dev/null 2>&1 ) && say "  hf drift clean ✓" || say "  hf drift: see 'hf drift'"
  fi

  # (commit)
  if [ "$DO_COMMIT" = 1 ] && [ "$DRY" = 0 ]; then
    git -C "$dir" add .handoff .gitignore .claude/settings.json 2>/dev/null
    if git -C "$dir" diff --cached --quiet 2>/dev/null; then
      say "  nothing to commit"
    elif git -C "$dir" commit -q -m "chore: handoff-loop-init — .handoff upgrade + guards + auto-loop hooks"; then
      say "  committed"
      [ "$DO_PUSH" = 1 ] && { git -C "$dir" push -q 2>/dev/null && say "  pushed" || say "  push FAILED"; }
    fi
  fi
  OK=$((OK+1))
done

echo "---"
echo "[init] targets=$OK init=$INIT guarded=$GUARD migrated=$MIGRATED deferred(busy)=$DEFERRED hooked=$HOOKED failed=$FAIL"
[ "$DEFERRED" -gt 0 ] && echo "[init] $DEFERRED repo(s) had a live loop — re-run when idle to migrate their ledgers."
exit 0
