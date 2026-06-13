#!/usr/bin/env bash
# grit-shared.sh — run grit against the fleet's SHARED backend with credentials
# injected by envctl (ADR-0010). Secrets are never exported: `secretctl run` injects
# them into grit's process env only.
#
#   scripts/grit-shared.sh claim --agent A --intent "…" auth.rs::login
#   scripts/grit-shared.sh done  --agent A
#
# Backend config (non-secret bucket/endpoint/region) is set once per repo via
# `grit config set-s3 …` / `grit config set-azure …` and lives in .grit/config.
# This wrapper supplies only the CREDENTIALS, at runtime, via envctl.
#
# READINESS: the envctl injection data-plane (`secretctl run`) is envctl Phase 8 and is
# not wired yet. Until it lands, this wrapper DEGRADES to plain local grit with a clear
# message — it never runs grit against a shared backend with no creds.
set -uo pipefail

PROVIDER="${GRIT_BACKEND_PROVIDER:-grit-backend}"

if [ "$#" -eq 0 ]; then
  echo "usage: grit-shared.sh <grit-args...>   (e.g. claim --agent A --intent x f.rs::sym)" >&2
  exit 2
fi

command -v grit >/dev/null 2>&1 || { echo "grit not on PATH" >&2; exit 1; }

# Probe whether envctl's injection data-plane is available (Phase 8).
injection_ready() {
  command -v secretctl >/dev/null 2>&1 || return 1
  # `secretctl run -- true` errors with the Phase-6/8 message while unwired.
  secretctl run -- true >/dev/null 2>&1
}

if injection_ready; then
  echo "[grit-shared] injecting '$PROVIDER' creds via envctl → shared backend"
  exec secretctl run --provider "$PROVIDER" -- grit "$@"
else
  echo "[grit-shared] envctl injection unavailable (Phase 8 not wired) — DEGRADING to local grit." >&2
  echo "[grit-shared] shared cross-repo coordination is pending envctl Phase 8 (ADR-0010)." >&2
  exec grit "$@"
fi
