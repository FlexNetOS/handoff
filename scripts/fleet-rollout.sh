#!/usr/bin/env bash
# fleet-rollout.sh — deterministic git-text .handoff generator (ADR-0004 §3/§7).
#
# For each present .meta.yaml member lacking a .handoff/, generate the Tier-A/B
# git-text core (capsule.json + README.md) — NO ledger.db, NO binary state. Events
# live in the FLEET ledger (meta/.handoff); packets compile via `hf fleet render`.
# Idempotent: skips a repo that already has .handoff/. No agent creativity per repo
# (ADR-0004 §7 deterministic generator).
#
# Usage:
#   scripts/fleet-rollout.sh [--commit] [--push] [member ...]
#     (no flags)  generate files locally only (reversible; review then commit)
#     --commit    git add+commit the .handoff in each repo
#     --push      git push (implies --commit)
#     [member...] limit to these members (default: all present members w/o .handoff)
set -uo pipefail

META_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"   # handoff/scripts -> meta root
[ -f "$META_ROOT/.meta.yaml" ] || { echo "no .meta.yaml at $META_ROOT"; exit 1; }

DO_COMMIT=0; DO_PUSH=0; NO_GRIT=0; ONLY=()
for a in "$@"; do
  case "$a" in
    --commit) DO_COMMIT=1 ;;
    --push)   DO_COMMIT=1; DO_PUSH=1 ;;
    --no-grit) NO_GRIT=1 ;;
    --*) echo "unknown flag $a"; exit 2 ;;
    *) ONLY+=("$a") ;;
  esac
done

# Member names = 2-space-indented keys under projects: (same parse as hf fleet status).
members() {
  awk '
    /^[^[:space:]]/ { inproj = ($0 ~ /^projects:/) }
    inproj && /^  [A-Za-z0-9_-]+:[[:space:]]*$/ { gsub(/[ :]/,""); print }
  ' "$META_ROOT/.meta.yaml"
}

# Derive plane from a repo's tags line (best-effort, deterministic).
plane_for() {
  local repo="$1" tags
  tags="$(grep -A4 "^  ${repo}:" "$META_ROOT/.meta.yaml" | grep -m1 'tags:' || true)"
  case "$tags" in
    *env-control*|*secrets*) echo "env-control" ;;
    *planning*|*kb*)         echo "planning" ;;
    *orchestration*)         echo "orchestration" ;;
    *)                        echo "execution" ;;
  esac
}
role_for() {
  local repo="$1" tags
  tags="$(grep -A4 "^  ${repo}:" "$META_ROOT/.meta.yaml" | grep -m1 'tags:' || true)"
  tags="${tags#*tags:}"; tags="${tags//[\[\] ]/}"; tags="${tags%%,*}"; echo "${tags:-tool}"
}

# HFTASK-0066: the .gitignore residency guards live in ONE place — `scripts/handoff-lib.sh`
# (ADR-0004 §3.3/§6 rev, HFTASK-0035/0037; extended for the redb cutover HFTASK-0053 with the
# `*.sqlite.bak`/`*.redb.tmp` migration-artifact guards). This script previously kept its own
# copy of `ensure_ledger_guard`/`ensure_active_md_guard`, which drifted (it lacked the migration
# guards). Source the canonical lib so there is exactly one definition; the functions are
# signature-compatible drop-ins (each takes a dir, returns 0 if it ADDED a guard, 1 if present).
# shellcheck source=scripts/handoff-lib.sh
. "$(dirname "$0")/handoff-lib.sh"

GENERATED=0; SKIPPED=0; COMMITTED=0; PUSHED=0; FAILED=0; GUARDED=0; ACTIVE_GUARDED=0
if [ ${#ONLY[@]} -gt 0 ]; then
  TARGETS=("${ONLY[@]}")
else
  mapfile -t TARGETS < <(members)
fi

for repo in "${TARGETS[@]}"; do
  [ -z "$repo" ] && continue
  dir="$META_ROOT/$repo"
  [ -d "$dir/.git" ] || { echo "skip $repo (not cloned)"; continue; }
  # HFTASK-0035/0037: a repo that already has .handoff still needs the .gitignore guards
  # (back-fill). Ensure them (idempotent), commit just the .gitignore if requested, then skip
  # the rest of generation.
  if [ -d "$dir/.handoff" ]; then
    local ledger_changed=0 active_changed=0
    ensure_ledger_guard "$dir" && { ledger_changed=1; GUARDED=$((GUARDED+1)); }
    ensure_active_md_guard "$dir" && { active_changed=1; ACTIVE_GUARDED=$((ACTIVE_GUARDED+1)); }
    if [ "$ledger_changed" = 1 ] || [ "$active_changed" = 1 ]; then
      echo "guard added $repo (ledger=$ledger_changed active=$active_changed)"
      if [ "$DO_COMMIT" = 1 ]; then
        if git -C "$dir" add .gitignore && \
           git -C "$dir" commit -q -m "chore: gitignore handoff derived state (ADR-0004 §6 / HFTASK-0035 / HFTASK-0037)"; then
          COMMITTED=$((COMMITTED+1))
          [ "$DO_PUSH" = 1 ] && { git -C "$dir" push -q 2>/dev/null && PUSHED=$((PUSHED+1)) || { echo "  push FAILED $repo"; FAILED=$((FAILED+1)); }; }
        else echo "  guard commit FAILED $repo"; FAILED=$((FAILED+1)); fi
      fi
    else
      echo "skip $repo (.handoff exists, guards present)"; SKIPPED=$((SKIPPED+1))
    fi
    continue
  fi

  plane="$(plane_for "$repo")"; role="$(role_for "$repo")"; [ -z "$role" ] && role="tool"
  mkdir -p "$dir/.handoff/context" "$dir/.handoff/tasks" "$dir/.handoff/packets"
  cat > "$dir/.handoff/context/capsule.json" <<JSON
{
  "schema": "handoff.context_capsule.v1",
  "project_name": "${repo}",
  "role": "${role}",
  "plane": "${plane}",
  "northstar": "(seed me) the guiding goal for ${repo}",
  "next_command": "hf resume"
}
JSON
  cat > "$dir/.handoff/README.md" <<MD
# .handoff (ADR-0004 §3.3/§6 rev)

Continuity layer for \`${repo}\`. **Committed content is git-text only** (capsule, cards,
packets). Local derived views — \`ledger.db\`, \`active.md\`, and \`packets/latest.md\` — are
**gitignored**; the ledger is the per-repo source of record that rolls up into the FLEET
ledger at \`meta/.handoff/ledger.db\`. A *committed* binary ledger or derived view is banned.
This repo's packet compiles centrally via \`hf fleet render ${repo}\`. See
\`meta/handoff/FLEET_GUIDE.md\`.

Cold start: read \`context/capsule.json\`, then run \`hf resume\`.
MD
  # HFTASK-0035/0037: newly-seeded repos get the ledger and active.md .gitignore guards.
  ensure_ledger_guard "$dir" && GUARDED=$((GUARDED+1))
  ensure_active_md_guard "$dir" && ACTIVE_GUARDED=$((ACTIVE_GUARDED+1))
  GENERATED=$((GENERATED+1)); echo "generated $repo (role=$role plane=$plane)"

  # grit (ADR-0009): initialize the parallel-agent coordination layer per repo (local
  # SQLite backend, zero-setup). .grit/ is binary state — gitignored by grit init, so
  # it never enters git (same rule as the handoff ledger, ADR-0004 §3). Best-effort.
  if [ "$NO_GRIT" = 0 ] && command -v grit >/dev/null 2>&1 && [ ! -d "$dir/.grit" ]; then
    # `grit init` first — it creates ./.grit. `grit config set-local` REQUIRES ./.grit
    # to already exist (errors "Run grit init first"), so it must come AFTER init.
    # local is grit's default backend, so set-local is just an explicit confirmation.
    if (cd "$dir" && grit init >/dev/null 2>&1 && grit config set-local >/dev/null 2>&1); then
      echo "  grit initialized $repo"
    else
      echo "  grit init skipped $repo (non-fatal)"
    fi
  fi

  if [ "$DO_COMMIT" = 1 ]; then
    if git -C "$dir" add .handoff .gitignore && \
       git -C "$dir" commit -q -m "chore: add Tier .handoff + gitignore local ledger (ADR-0004 §3/§6)" ; then
      COMMITTED=$((COMMITTED+1))
      if [ "$DO_PUSH" = 1 ]; then
        if git -C "$dir" push -q 2>/dev/null; then PUSHED=$((PUSHED+1)); else echo "  push FAILED $repo"; FAILED=$((FAILED+1)); fi
      fi
    else echo "  commit FAILED $repo"; FAILED=$((FAILED+1)); fi
  fi
done

echo "---"
echo "generated=$GENERATED guarded(ledger .gitignore)=$GUARDED active-guarded=$ACTIVE_GUARDED skipped(existing)=$SKIPPED committed=$COMMITTED pushed=$PUSHED failed=$FAILED"
