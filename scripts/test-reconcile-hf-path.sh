#!/usr/bin/env bash
# test-reconcile-hf-path.sh — self-contained functional test for reconcile_hf_path
# (HFTASK-0096, ADR-0006). Drives the real helper from handoff-lib.sh against a synthetic PATH
# and asserts behavior, then emits a libtest-compatible `test result:` summary line so
# `hf test` COUNT-verifies it (the tests-ran>0 gate, HFTASK-0045/0063) instead of trusting a
# bare exit code — closing the fail-open gap that a `bash -n` syntax check alone leaves (U3
# fail-closed doctrine: a verdict must not rest on exit-0 / an absence of failure).
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/handoff-lib.sh
. "$SCRIPT_DIR/handoff-lib.sh"

PASS=0
FAIL=0
FAILED=()
ok()  { echo "test reconcile::$1 ... ok"; PASS=$((PASS + 1)); }
bad() {
  echo "test reconcile::$1 ... FAILED"
  echo "  $2"
  FAIL=$((FAIL + 1))
  FAILED+=("$1")
}

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Build a fresh canonical + a stale shadow copy under an isolated layout. Echoes the layout root.
layout() {
  local root="$1"
  mkdir -p "$root/cargo/bin" "$root/local/bin"
  printf '#!/bin/sh\necho FRESH\n' >"$root/cargo/bin/hf"
  chmod +x "$root/cargo/bin/hf"
  printf '#!/bin/sh\necho STALE\n' >"$root/local/bin/hf"
  chmod +x "$root/local/bin/hf"
}

# 1) A stale shadow earlier on PATH is converged to a symlink -> canonical.
layout "$TMP/a"
PATH="$TMP/a/local/bin:$TMP/a/cargo/bin:$PATH" reconcile_hf_path "$TMP/a/cargo/bin/hf" 0 >/dev/null 2>&1
if [ -L "$TMP/a/local/bin/hf" ] &&
  [ "$(_realpath "$TMP/a/local/bin/hf")" = "$(_realpath "$TMP/a/cargo/bin/hf")" ] &&
  [ "$(PATH="$TMP/a/local/bin:$TMP/a/cargo/bin:$PATH" hf)" = "FRESH" ]; then
  ok converges_shadow_to_symlink
else
  bad converges_shadow_to_symlink "local/bin/hf is not a symlink to canonical (or hf still serves STALE)"
fi

# 2) Idempotent: a second run mutates nothing and emits no output.
out="$(PATH="$TMP/a/local/bin:$TMP/a/cargo/bin:$PATH" reconcile_hf_path "$TMP/a/cargo/bin/hf" 0 2>&1)"
if [ -z "$out" ] && [ -L "$TMP/a/local/bin/hf" ]; then
  ok idempotent_second_run
else
  bad idempotent_second_run "re-run emitted output or changed the symlink: '${out}'"
fi

# 3) --dry-run mutates nothing.
layout "$TMP/b"
PATH="$TMP/b/local/bin:$TMP/b/cargo/bin:$PATH" reconcile_hf_path "$TMP/b/cargo/bin/hf" 1 >/dev/null 2>&1
if [ ! -L "$TMP/b/local/bin/hf" ]; then
  ok dry_run_mutates_nothing
else
  bad dry_run_mutates_nothing "dry-run created a symlink"
fi

# 4) Canonical first on PATH: nothing shadows it, so the shadow after it is left untouched.
layout "$TMP/c"
out="$(PATH="$TMP/c/cargo/bin:$TMP/c/local/bin:$PATH" reconcile_hf_path "$TMP/c/cargo/bin/hf" 0 2>&1)"
if [ -z "$out" ] && [ ! -L "$TMP/c/local/bin/hf" ]; then
  ok canonical_first_is_noop
else
  bad canonical_first_is_noop "touched a shadow that does not shadow canonical: '${out}'"
fi

TOTAL=$((PASS + FAIL))
echo
if [ "$FAIL" -eq 0 ]; then
  echo "test result: ok. $PASS passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;"
  echo "[test-reconcile-hf-path] PASS ($PASS/$TOTAL)"
  exit 0
else
  echo "test result: FAILED. $PASS passed; $FAIL failed; 0 ignored; 0 measured; 0 filtered out;"
  echo "[test-reconcile-hf-path] FAIL (${FAILED[*]})"
  exit 1
fi
