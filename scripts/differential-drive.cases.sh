#!/usr/bin/env bash
# differential-drive.cases.sh — handoff's OWN live differential cases.
#
# Sourced by differential-drive.sh (uses the `drive` helper). These are NOT unit tests: they
# drive the REAL built `hf` binary end-to-end and diff its actual output against kernel
# CLI-contract invariants (the relay-#134 method). Cases are state-INDEPENDENT (the usage
# contract prints regardless of ledger state) so they are robust on a fresh CI checkout.
#
# This file is handoff-specific and is NOT part of the fleet-deployed bundle — every repo
# authors its own cases; without this file the harness fails closed (forcing adoption).

# Locate the REAL binary to drive. Prefer THIS checkout's build (the code under test) over a
# globally-installed `hf` on PATH, which may be a stale/other-branch version — a differential
# drive of a repo must verify the repo's OWN binary deterministically. Build it if absent.
_HF=""
if [ -x "target/debug/hf" ]; then
  _HF="target/debug/hf"
elif [ -x "target/release/hf" ]; then
  _HF="target/release/hf"
elif cargo build -p hf --quiet >/dev/null 2>&1 && [ -x "target/debug/hf" ]; then
  _HF="target/debug/hf"
elif command -v hf >/dev/null 2>&1; then
  _HF="hf"
fi

if [ -z "$_HF" ]; then
  # Nothing to drive — record a failing case so the harness reports FAIL-CLOSED (cases-run>0
  # with a real failure), never a silent zero-case pass.
  drive "hf binary is buildable + runnable" "false" ""
  return 0 2>/dev/null || true
fi

# CLI-contract invariants — drive the real binary, diff a stable substring of its usage output.
# Each guards a verb the kernel must expose; a dropped/renamed verb fails the live drive even
# when every unit test stays green (exactly the relay-#134 gap).
drive "usage exposes 'claim'"   "$_HF 2>&1 || true" "claim"
drive "usage exposes 'ship'"    "$_HF 2>&1 || true" "ship"
drive "usage exposes 'promote'" "$_HF 2>&1 || true" "promote"
drive "usage exposes 'drift'"   "$_HF 2>&1 || true" "drift"
drive "usage exposes 'handoff'" "$_HF 2>&1 || true" "handoff"
