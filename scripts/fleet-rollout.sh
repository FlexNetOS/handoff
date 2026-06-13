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

GENERATED=0; SKIPPED=0; COMMITTED=0; PUSHED=0; FAILED=0
if [ ${#ONLY[@]} -gt 0 ]; then
  TARGETS=("${ONLY[@]}")
else
  mapfile -t TARGETS < <(members)
fi

for repo in "${TARGETS[@]}"; do
  [ -z "$repo" ] && continue
  dir="$META_ROOT/$repo"
  [ -d "$dir/.git" ] || { echo "skip $repo (not cloned)"; continue; }
  if [ -d "$dir/.handoff" ]; then echo "skip $repo (.handoff exists)"; SKIPPED=$((SKIPPED+1)); continue; fi

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
# .handoff (git-text-only, ADR-0004 §3)

Continuity layer for \`${repo}\`. **Text only — no \`ledger.db\`, no binary state.**
Witnessed events live in the FLEET ledger (\`meta/.handoff/ledger.db\`); this repo's
packet is compiled centrally by \`hf fleet render ${repo}\`. See \`meta/handoff/FLEET_GUIDE.md\`.

Cold start: read \`context/capsule.json\`, then run \`hf resume\`.
MD
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
    if git -C "$dir" add .handoff && \
       git -C "$dir" commit -q -m "chore: add Tier .handoff (git-text-only, ADR-0004 §3)" ; then
      COMMITTED=$((COMMITTED+1))
      if [ "$DO_PUSH" = 1 ]; then
        if git -C "$dir" push -q 2>/dev/null; then PUSHED=$((PUSHED+1)); else echo "  push FAILED $repo"; FAILED=$((FAILED+1)); fi
      fi
    else echo "  commit FAILED $repo"; FAILED=$((FAILED+1)); fi
  fi
done

echo "---"
echo "generated=$GENERATED skipped(existing)=$SKIPPED committed=$COMMITTED pushed=$PUSHED failed=$FAILED"
