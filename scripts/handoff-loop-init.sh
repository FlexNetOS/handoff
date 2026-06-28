#!/usr/bin/env bash
# handoff-loop-init.sh — ALL-IN-ONE .handoff upgrade + sync + deploy, one command.
#
# Brings any repo's continuity layer fully current from a single invocation:
#   1. ensure the `hf` binary on PATH is the pure-Rust redb build (HFTASK-0053, no-C)
#   2. init-or-upgrade the repo's .handoff (portable `hf init` — repo self-identifies)
#   3. install the .gitignore residency + migration-artifact guards
#   4. migrate a legacy SQLite ledger -> redb (out-of-tree backup), ONLY if quiescent
#   5. deploy the auto-loop hooks (SessionStart loop-entry + SessionEnd safety net)
#      + the generic live differential-drive action workflow + harness (HFTASK-0078, relay-#134)
#      + canonical `.claude/rules/*` full-auto rules (ADR-0018 D6 / HFTASK-0077)
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
elif [ -n "$KERNEL_HOME" ] && _is_kernel_home "$KERNEL_HOME"; then
  # HFTASK-0085 (automation rung 0/1): the build-version stamp lets us detect a binary that is
  # BEHIND the kernel source and auto-rebuild — previously a stale-but-working redb hf was
  # silently kept and a human had to pass --build-hf. Compare installed stamp vs kernel HEAD.
  installed_commit="$("$HF" version --json 2>/dev/null | grep '"commit"' | sed -E 's/.*"commit"[^"]*"([^"]+)".*/\1/')"
  kernel_commit="$(git -C "$KERNEL_HOME" rev-parse --short HEAD 2>/dev/null)"
  if [ -n "$kernel_commit" ] && [ -n "$installed_commit" ] && [ "$installed_commit" != "unknown" ] && [ "$installed_commit" != "$kernel_commit" ]; then
    say "hf stamp '$installed_commit' is behind kernel HEAD '$kernel_commit' — will rebuild (HFTASK-0085)"; need_build=1
  fi
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

# ── Phase 0b: reconcile stale shadow `hf` copies on PATH (HFTASK-0096, ADR-0006) ──────
# `cargo install` lands the fresh binary in the cargo bin, but a COPY of `hf` earlier on PATH
# (e.g. ~/.local/bin/hf) keeps shadowing it — so `hf` serves the OLD binary and new verbs read
# as "unknown command" until a manual cp. Converge such shadows to a symlink into the build.
canonical_hf="${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$HOME/.cargo}}/bin/hf"
[ -x "$canonical_hf" ] || canonical_hf="$KERNEL_HOME/target/release/hf"
[ -x "$canonical_hf" ] || canonical_hf="$(command -v hf 2>/dev/null || echo "")"
if [ -n "$canonical_hf" ] && [ -e "$canonical_hf" ]; then
  reconcile_hf_path "$canonical_hf" "$DRY"
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

INIT=0 GUARD=0 MIGRATED=0 DEFERRED=0 HOOKED=0 DIFFDRIVE=0 RELAY=0 RULES=0 OK=0 FAIL=0

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

# Deploy the GENERIC live differential-drive action workflow + harness (HFTASK-0078, relay-#134).
# Ships ONLY the repo-agnostic reusable workflow + the harness — NOT handoff's own cases file or
# its handoff-local CI caller (each repo authors its own scripts/differential-drive.cases.sh; the
# harness fails closed until it does). Idempotent (plain copy). Returns 0 if it deployed.
deploy_diff_drive() {
  local dir="$1"
  # Canonical sources: the kernel checkout this script lives in (handoff dev or ejected-with-kernel).
  local wf_src="$KERNEL_HOME/.github/workflows/differential-drive.yml"
  [ -f "$wf_src" ] || wf_src="$SCRIPT_DIR/../.github/workflows/differential-drive.yml"
  local sh_src="$KERNEL_HOME/scripts/differential-drive.sh"
  [ -f "$sh_src" ] || sh_src="$SCRIPT_DIR/differential-drive.sh"
  if [ ! -f "$wf_src" ] || [ ! -f "$sh_src" ]; then
    say "  no differential-drive sources reachable — skipping (HFTASK-0078)"
    return 1
  fi
  if [ "$DRY" = 1 ]; then
    echo "    DRY: deploy differential-drive.yml + scripts/differential-drive.sh -> $dir"
    return 0
  fi
  mkdir -p "$dir/.github/workflows" "$dir/scripts"
  cp "$wf_src" "$dir/.github/workflows/differential-drive.yml"
  cp "$sh_src" "$dir/scripts/differential-drive.sh"
  chmod +x "$dir/scripts/differential-drive.sh" 2>/dev/null || true
  return 0
}

# Deploy the canonical session-relay skills (ADR-0018 D5, HFTASK-0070): handoff owns the canonical
# format/templates for session-relay-resume / session-relay-wrap-up — rendered from the witnessed
# `hf` ledger/packet, NEVER hand-authored prose — and pushes them + ENFORCES byte-consistency to
# every fleet member. Canonical source = handoff's own committed templates (dev checkout or the
# vendored copy when ejected). Idempotent (plain overwrite); fails closed if no source is reachable.
# Returns 0 if it deployed.
deploy_session_relay() {
  local dir="$1"
  # Canonical source: the kernel checkout this script lives in (handoff dev or ejected-with-kernel),
  # else the templates vendored beside this script under the plugin (HFTASK-0065 self-contained).
  local src="$KERNEL_HOME/.claude/skills"
  [ -d "$src/session-relay-resume" ] || src="$SCRIPT_DIR/../.claude/skills"
  [ -d "$src/session-relay-resume" ] || src="$SCRIPT_DIR/skills"
  if [ ! -d "$src/session-relay-resume" ] || [ ! -d "$src/session-relay-wrap-up" ]; then
    say "  no session-relay sources reachable — skipping (HFTASK-0070)"
    return 1
  fi
  if [ "$DRY" = 1 ]; then
    echo "    DRY: deploy session-relay-{resume,wrap-up} skills -> $dir/.claude/skills"
    return 0
  fi
  local skill
  for skill in session-relay-resume session-relay-wrap-up; do
    mkdir -p "$dir/.claude/skills/$skill"
    # Byte-consistency ENFORCEMENT (HFTASK-0067 model): report drift, then overwrite with canonical.
    if [ -f "$dir/.claude/skills/$skill/SKILL.md" ] \
       && ! cmp -s "$src/$skill/SKILL.md" "$dir/.claude/skills/$skill/SKILL.md"; then
      say "  session-relay drift in $skill — re-deploying canonical (byte-consistency)"
    fi
    cp "$src/$skill/SKILL.md" "$dir/.claude/skills/$skill/SKILL.md"
    if [ -d "$src/$skill/scripts" ]; then
      mkdir -p "$dir/.claude/skills/$skill/scripts"
      cp -r "$src/$skill/scripts/." "$dir/.claude/skills/$skill/scripts/"
    fi
  done
  return 0
}

# Deploy canonical `.claude/rules/*` (ADR-0018 D6, HFTASK-0077). These rules are the
# fleet-facing operating contract for full-auto work: committed durable dotfiles, full `.kb`
# adoption, worktree-per-batch, context-budget wrap, grit+GitHub grounding, and
# designated-agent-as-reviewer. Byte-consistency is enforced the same way session-relay is:
# report drift, then overwrite with canonical source. Dry-run prints the intended copy only.
deploy_rules() {
  local dir="$1"
  local src="$KERNEL_HOME/.claude/rules"
  [ -d "$src" ] || src="$SCRIPT_DIR/../.claude/rules"
  [ -d "$src" ] || src="$SCRIPT_DIR/rules"
  if [ ! -d "$src" ]; then
    say "  no canonical .claude/rules sources reachable — skipping (HFTASK-0077)"
    return 1
  fi
  if [ "$DRY" = 1 ]; then
    echo "    DRY: deploy canonical .claude/rules/* -> $dir/.claude/rules"
    return 0
  fi
  mkdir -p "$dir/.claude/rules"
  local f base
  shopt -s nullglob
  for f in "$src"/*.md; do
    base="$(basename "$f")"
    if [ -f "$dir/.claude/rules/$base" ] && ! cmp -s "$f" "$dir/.claude/rules/$base"; then
      say "  .claude/rules drift in $base — re-deploying canonical (byte-consistency)"
    fi
    cp "$f" "$dir/.claude/rules/$base"
  done
  shopt -u nullglob
  return 0
}

for dir in "${TARGETS[@]}"; do
  git -C "$dir" rev-parse --is-inside-work-tree >/dev/null 2>&1 \
    || { say "skip $(basename "$dir") (not a git repo)"; continue; }
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

  # (3) guards. ADR-0018 D1 flipped rendered views (active.md/packets/deliveries) to
  # committed durable text; only binary ledger caches + migration artifacts are ignored.
  gl=0
  if [ "$DRY" = 1 ]; then echo "    DRY: ensure ledger-cache + migration-artifact guards"; else
    ensure_ledger_guard "$dir" && gl=1
  fi
  [ "$gl" = 1 ] && { GUARD=$((GUARD+1)); say "  guards updated (ledger=$gl)"; }
  # If an older rollout committed binary ledger caches, remove them from the git index after
  # the guard is present while leaving the local files on disk for migration/import. Without this
  # `git check-ignore` keeps reporting the tracked path as unignored and fleet sync can never
  # clear the tracked_ledger / ledger_guard flags (prompt_hub exposed this P7 wedge).
  if [ "$DRY" = 1 ]; then
    echo "    DRY: git rm --cached --ignore-unmatch .handoff ledger cache files"
  else
    git -C "$dir" rm --cached --ignore-unmatch \
      .handoff/ledger.db .handoff/ledger.db-wal .handoff/ledger.db-shm \
      .handoff/**/ledger.db .handoff/**/*.db-wal .handoff/**/*.db-shm \
      >/dev/null 2>&1 || true
  fi

  # (4) ledger migration (fail-closed on quiescence)
  if [ "$NO_MIGRATE" = 0 ]; then
    legacy=""
    for db in "$dir"/.handoff/**/ledger.db "$dir"/.handoff/ledger.db; do
      [ -f "$db" ] && ledger_is_legacy_sqlite "$db" && { legacy="$db"; break; }
    done
    if [ -n "$legacy" ]; then
      if repo_quiescent "$dir"; then
        say "  legacy SQLite ledger detected — migrating to redb"
        legacy_abs="$(cd "$(dirname "$legacy")" && pwd)/$(basename "$legacy")"
        if [ -n "$KERNEL_HOME" ] && _is_kernel_home "$KERNEL_HOME"; then
          migrate_cmd="cd \"$KERNEL_HOME\" && cargo run -q -p hf --features legacy-sqlite --bin hf -- migrate \"$legacy_abs\""
        else
          migrate_cmd="cd \"$dir\" && \"$HF\" migrate \"$legacy_abs\""
        fi
        if [ "$DRY" = 1 ]; then echo "    DRY: $migrate_cmd"; else
          eval "$migrate_cmd" && { MIGRATED=$((MIGRATED+1)); say "  hf migrate ✓"; } \
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

  # (5b) differential-drive action workflow + harness (HFTASK-0078)
  deploy_diff_drive "$dir" && { DIFFDRIVE=$((DIFFDRIVE+1)); say "  differential-drive workflow + harness deployed"; }

  # (5c) canonical session-relay skills, byte-enforced (HFTASK-0070, ADR-0018 D5)
  deploy_session_relay "$dir" && { RELAY=$((RELAY+1)); say "  session-relay skills deployed (byte-enforced)"; }

  # (5d) canonical full-auto rule files, byte-enforced (HFTASK-0077, ADR-0018 D6)
  deploy_rules "$dir" && { RULES=$((RULES+1)); say "  .claude/rules deployed (byte-enforced)"; }

  # (6) verify + render
  if [ "$DRY" = 0 ]; then
    ( cd "$dir" && "$HF" resume >/dev/null 2>&1 ) || true
    ( cd "$dir" && "$HF" export >/dev/null 2>&1 ) && {
      git -C "$dir" add -f .handoff/ledger.events.jsonl >/dev/null 2>&1 || true
      say "  ledger.events.jsonl exported + staged"
    } || say "  hf export: see 'hf export'"
    ( cd "$dir" && "$HF" drift >/dev/null 2>&1 ) && say "  hf drift clean ✓" || say "  hf drift: see 'hf drift'"
  fi

  # (commit)
  if [ "$DO_COMMIT" = 1 ] && [ "$DRY" = 0 ]; then
    git -C "$dir" add .handoff .gitignore .claude/settings.json .github/workflows/differential-drive.yml scripts/differential-drive.sh .claude/skills/session-relay-resume .claude/skills/session-relay-wrap-up .claude/rules 2>/dev/null
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
echo "[init] targets=$OK init=$INIT guarded=$GUARD migrated=$MIGRATED deferred(busy)=$DEFERRED hooked=$HOOKED diffdrive=$DIFFDRIVE relay=$RELAY rules=$RULES failed=$FAIL"
[ "$DEFERRED" -gt 0 ] && echo "[init] $DEFERRED repo(s) had a live loop — re-run when idle to migrate their ledgers."
exit 0
