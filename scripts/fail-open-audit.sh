#!/usr/bin/env bash
# fail-open-audit.sh — ADVISORY detective control for the FAIL-OPEN class (lesson L7).
#
# The FAIL-OPEN class: a guard, loader, or evidence-check that, when it cannot confirm
# its precondition (missing card, empty ledger read, zero-match test, dead-lock retry
# exhausted), PROCEEDS as if satisfied instead of stopping. That inverts the kernel's
# founding promise (witnessed + fail-closed) — see AGENTS.md "Fail-closed law" and
# LESSONS.md L7–L10. The original instances: `load_tasks` silently dropping card #95,
# the `hf test` exit-0 rubber-stamp (PR #103), the orphaned `.rvf.lock` wedge.
#
# This script GREPS the kernel sources (hf/, ledger/, work-order/) for fail-open
# CANDIDATE patterns and LISTS them with file:line so a human/agent can audit each.
# It is a DETECTIVE control, not a preventive one:
#   - ADVISORY ONLY — it always exits 0 and NEVER blocks a build, push, or CI gate.
#   - Matches are CANDIDATES, not confirmed defects: many `if let Ok` / `.ok()?` /
#     `unwrap_or_default()` uses are perfectly fine OUTSIDE continuity-gating paths.
#     A human must judge whether each site is on a gating path (card/ledger/status/
#     completion-evidence/lock/policy) before treating it as a real fail-open.
#   - It is intentionally a lint, not a test: promoting it to a CI-gating check is the
#     escalation if a 4th FAIL-OPEN instance ever surfaces post-fix (LESSONS.md watch).
#
# Usage:  scripts/fail-open-audit.sh
# Output: grouped candidate sites by pattern; a trailing total. Exit code: always 0.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && { git rev-parse --show-toplevel 2>/dev/null || pwd; })"
cd "$ROOT" || exit 0

# Source trees that contain continuity-gating logic.
SRC_DIRS=(hf/src ledger/src work-order/src)
EXISTING=()
for d in "${SRC_DIRS[@]}"; do
  [ -d "$d" ] && EXISTING+=("$d")
done
if [ "${#EXISTING[@]}" -eq 0 ]; then
  echo "fail-open-audit: no kernel source dirs found under $ROOT — nothing to scan."
  exit 0
fi

# grep -rn is run RAW (rtk's grep hook clobbers -n/format flags; we want line numbers).
# Prefer a literal grep so the rtk hook does not rewrite it.
GREP() { command grep -rn --include='*.rs' "$@" "${EXISTING[@]}" 2>/dev/null; }

TOTAL=0
section() {
  local title="$1"; shift
  local note="$1"; shift
  local hits
  hits="$(GREP -E "$1" || true)"
  echo "── ${title}"
  echo "   why it can fail open: ${note}"
  if [ -n "$hits" ]; then
    # prefix each line for readability; count them
    local n
    n="$(printf '%s\n' "$hits" | grep -c .)"
    TOTAL=$((TOTAL + n))
    printf '%s\n' "$hits" | sed 's/^/   /'
  else
    echo "   (no candidate sites)"
  fi
  echo
}

echo "fail-open-audit (ADVISORY) — candidate FAIL-OPEN sites in: ${EXISTING[*]}"
echo "Each match is a CANDIDATE; a human must confirm it is on a continuity-gating path."
echo

# 1. `.ok()?` — swallows an error and short-circuits as if the value were simply absent.
section ".ok()? on a fallible read (swallows the Err, short-circuits as empty)" \
  "a failed card/ledger read becomes 'None' and the caller proceeds as if there was nothing to load." \
  '\.ok\(\)\?'

# 2. unwrap_or_default() — an empty default reported as truth (e.g. status derivation).
section "unwrap_or_default() feeding state/status" \
  "an error yields the type default (empty map/0/None) that is then reported as the real status." \
  'unwrap_or_default\(\)'

# 3. `if let Ok(...)` with no surfaced else — the Err branch is silently dropped.
section "if let Ok(...) { ... } (verify the Err case is surfaced, not silently skipped)" \
  "the missing 'else'/Err arm means a parse/read failure is silently skipped (how card #95 was dropped)." \
  'if let Ok\('

# 4. exit-0 / status(0) treated as pass — absence of failure read as evidence of success.
section "exit-code-0 / success(0) as pass (require a POSITIVE count, not just a clean exit)" \
  "a zero exit / empty result / zero rows is treated as 'it passed' rather than 'nothing was exercised'." \
  'success\(\)|code\(0\)|ExitStatus|\.status\(\)'

# 5. retry-then-give-up — exhausting a retry cap and proceeding (the stale-lock wedge).
section "retry/attempt loops (verify exhaustion FAILS LOUD, not silently proceeds)" \
  "a retry cap that, when exhausted, returns Ok/empty instead of surfacing the wall (the orphaned-lock wedge)." \
  'retr(y|ies)|max_attempts|attempts|backoff'

echo "fail-open-audit: ${TOTAL} candidate site(s). ADVISORY ONLY — exit 0, never blocks."
echo "Audit each against AGENTS.md 'Fail-closed law'; a confirmed gating-path fail-open must fail closed with a surfaced diagnostic."
exit 0
