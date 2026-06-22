---
id: 019eedcb-fb9b-7d13-ab82-25b1cffb86dc
slug: context/extensible/tech
title: "Tech Context"
type: context
status: draft
priority: medium
---

## Stack
- Rust workspace (`hf`, `ledger`, `work-order`); pure-Rust **redb** ACID event store (no bundled C, ADR-0017) + native RVF v2 vector overlay.
- `ruvector-verified` (Lean AgentContract proof at `hf handoff`); `cognitum-gate-tilezero` (witnessed policy).
- git-kb 0.2.10 (planning plane `.kb` + AST code intelligence); weave (A2A); grit (symbol locks + worktrees); envctl (secrets).

## Dev setup
- `cargo build -p hf`; `./target/debug/hf` is the local binary.
- CI gate (mirror locally before push): `cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --all --check` + `cargo test`. `--all-targets` lints test code (the PR #30 lesson).
- Merge flow: branch off `develop` → PR `--base develop` → `hf promote` (develop→trunk ff). NEVER PR `master` (runner-cap stall).

## Constraints
- No-C trust boundary; fail-closed continuity-gating paths; scope-bounded edits (path_scope + intent_lock).