# ADR-0016 — Canonical `.handoff` durability policy (commit-vs-ignore, kernel-shipped)

- **Status:** Accepted
- **Date:** 2026-06-21
- **Task:** HFTASK-0058
- **Depends on:** HFTASK-0035 (`.handoff/**/ledger.db` residency guard), HFTASK-0034 (P7
  fleet gate), HFTASK-0037 (`active.md` derived-view ignore)
- **Pillar:** Integrity · Reversibility (NORTH-STAR: *no promotion without Integrity ·
  Reversibility · Capability Gain*) — durable continuity truth must be provably committed.

## Context

`.handoff/` is the continuity source-of-truth **everywhere** the `hf` kernel is used — the
handoff repo, every fleet member, and every consumer harness (rust-port, feature-forge, …).
But the rule for *which* `.handoff` subpaths are **durable** (must be committed) versus
**regenerable** (must be gitignored, rebuilt by the kernel) lived only **implicitly** in the
handoff repo's own `.gitignore`. It was never shipped, so every consumer hand-rolled its own
`.gitignore` and got it wrong.

The sharpest, found 2026-06-21 while verifying the rust-port harness end-to-end (harness_hub#49):

1. **The dir-form swallow (fatal).** A consumer that ignores a continuity directory in
   **dir-form** — a bare `.handoff/` or `.claude/` with no `/*` — makes Git refuse to
   re-include **anything** beneath it. `!`-negations **cannot** rescue a path whose parent
   directory is excluded (only contents-form `.handoff/*` + negations can). So durable
   `.handoff/tasks/*.json`, `.handoff/decisions/*.md`, and loop `*.md` ledgers are **silently
   swallowed**: `git add` stages zero, continuity dies on the next clean checkout, and
   nothing detects it.
2. **Over-matching.** A `.handoff/**/*.db`-shaped pattern can over-match and catch a
   consumer's durable `.md`.
3. **No guard.** These bugs are invisible from reading code — they only surface by asking Git
   on a real tree (`git check-ignore`). There was no fail-closed detector.

The rust-port harness mitigated this **per-consumer** (its `eject.sh` detects dir-form ignores
and probes representative paths). That is a band-aid on one consumer. The fix belongs at the
**source** — the kernel.

## Decision

The kernel owns and **ships** a single canonical durability policy.

### 1. Taxonomy (`hf/src/durability.rs`)

| Class | Subpaths | Rule |
|------|----------|------|
| **DURABLE** | `tasks/`, `decisions/`, `context/` (capsule), loop `*.md` ledgers, `hooks/`, `policy.toml`, `README.md` | committed; MUST stay un-ignored |
| **REGENERABLE** | `*.db` / `*.db-wal` / `*.db-shm` / `*.rvf`, `packets/`, `workspaces/`, `locks/`, `deliveries/`, `active.md` | gitignored; rebuilt by `hf` from the authoritative ledger |

### 2. Kernel-shipped, contents-form `.gitignore` fragment

`durability::CANONICAL_GITIGNORE_FRAGMENT` is emitted by `hf gitignore` (no args). Every rule
targets a **specific regenerable subpath** — there is deliberately **no** bare `.handoff/` /
`.claude/` line. Consumers inherit it via `hf gitignore --repair` instead of hand-rolling:

- `hf gitignore` — print the canonical fragment to stdout.
- `hf gitignore --repair` / `--write` — strip any dir-form `.handoff/`/`.claude/` swallow and
  append the fragment, idempotently and non-destructively (all other lines preserved).

### 3. Fail-closed swallow-guard (`hf gitignore --check`, wired into `hf doctor`)

`durability::swallow_report` asks Git's own ignore engine (`git check-ignore`) whether any
representative **durable** probe path is ignored, and scans `.gitignore` for dir-form
continuity excludes. A swallow is **fatal** (exit 1), exactly like a broken witness chain:

- `hf doctor` adds a `durability:` line and folds the result into overall health (DEGRADED +
  exit 1 on a swallow).
- `hf gitignore --check` is the standalone gate for hooks/CI.

It also reports (as a non-fatal warning) any **regenerable** probe that escaped the ignore set
(under-ignoring), so the guard is a strict superset of "durable truth is committable" without
loosening the regenerable-ignore set.

## Consequences

- **No-downgrade:** this only strengthens continuity. The existing regenerable-ignore set
  (`ledger.db`/WAL/SHM/`rvf`, `packets/`, `workspaces/`, `locks/`, `deliveries/`, `active.md`)
  is unchanged — the canonical fragment reproduces it and a test asserts no regression.
- **Detection moved into Git's engine.** The guard reasons via `git check-ignore` on real
  probe paths, not by pattern-guessing, so it catches swallows that are invisible to code review.
- **Consumers inherit, not reinvent.** `fleet-rollout.sh::ensure_ledger_guard` and the rust-port
  eject mitigation are now subsumed by one kernel verb; follow-up wires them to call
  `hf gitignore --repair` so there is a single source of the policy.
- **Aligns with the commit-dotfiles policy:** removing a dir-form `.claude/` swallow also
  re-includes durable IDE/agent dotfiles, consistent with treating tracked dotfiles as durable
  project state.

## Alternatives considered

- *Per-consumer `.gitignore` linting* (the rust-port band-aid) — rejected: it fixes one
  consumer and drifts; the policy must live once, at the kernel.
- *A bare `.handoff/` + `!`-negations* — rejected: Git cannot re-include past an excluded
  parent directory, so this is precisely the swallow.
- *Pattern-matching the `.gitignore` in Rust* — rejected for detection: re-implementing Git's
  ignore semantics is error-prone; `git check-ignore` is authoritative.
