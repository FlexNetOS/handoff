---
name: handoff-loop-init
description: >-
  All-in-one initializer that upgrades, syncs, and deploys the .handoff continuity
  layer to any repo (or the whole fleet) from a single command. Use when onboarding a
  repo to the handoff kernel, bringing a stale .handoff current, deploying the redb
  (no-C) hf binary + auto-loop hooks, or migrating a legacy SQLite ledger to redb.
  Invoked as /handoff-loop-init (the pending /harness:handoff-loop-init).
---

# handoff-loop-init — one command, fully-deployed continuity

This is the **init half** of the handoff loop: where `handoff-loop` *advances* a repo
that already has `.handoff`, `handoff-loop-init` **gets a repo to that state** — idempotent,
fail-closed, and from a single invocation. It wraps the existing primitives (`hf init`,
`hf migrate`, the fleet guards, the auto-loop hooks) so the user never has to chain them.

The heavy lifting lives in `scripts/handoff-loop-init.sh` (sourcing `scripts/handoff-lib.sh`).
This skill decides scope, runs the script, and reports.

## When to use

- Onboard a new repo to the continuity kernel (`.handoff` does not exist yet).
- Bring an existing but stale `.handoff` current (new guards, new hooks, redb cutover).
- Roll the upgrade across the whole fleet (`--fleet`).
- Deploy the pure-Rust **redb** `hf` (HFTASK-0053, no-C trust boundary) to PATH.

## What the command does (idempotent, in order)

1. **Ensure the redb `hf` binary** — if PATH `hf` is missing or still links `libsqlite`
   (a pre-redb build), rebuild+install it from the kernel (`cargo install --path hf`).
   A current C-free `hf` is left untouched.
2. **Init-or-upgrade `.handoff`** — `hf init` (portable: the repo self-identifies from its
   git toplevel, neutral `(seed me)` northstar, never the kernel's identity). Existing
   capsule/cards/README are preserved.
3. **Guards** — add the `.gitignore` residency guards (`.handoff/**/ledger.db` + wal/shm,
   `active.md`) **plus** the redb-cutover migration-artifact guards (`*.sqlite.bak`,
   `*.redb.tmp`) so derived/transient state never churns git or trips `hf drift`.
4. **Migrate the ledger (fail-closed)** — if the local `ledger.db` is still legacy SQLite
   (`SQLite format 3` magic), run `hf migrate` (writes an out-of-tree backup) — **only if
   the repo is provably quiescent**. A repo with a live loop (running hf/cargo/grit, an
   active grit worktree, a held lease lock, or a ledger written in the last 120 s) is
   **deferred and reported**, never migrated underneath a running session.
5. **Deploy auto-loop hooks** — copy `loop-entry.sh` / `session-end.sh` / `hooks.toml` into
   the repo's `.handoff/hooks/` and merge `SessionStart`/`SessionEnd` wiring into
   `.claude/settings.json` (existing keys preserved).
6. **Verify + render** — `hf resume` (render packet), `hf drift` (conformance), report.

## How to run

```bash
# current repo (default)
bash scripts/handoff-loop-init.sh

# a specific repo
bash scripts/handoff-loop-init.sh /path/to/repo --commit

# the whole fleet (skips non-quiescent repos for the migration step only)
bash scripts/handoff-loop-init.sh --fleet --commit
```

Flags: `--commit` / `--push` (stage+commit the git-text), `--no-migrate`, `--no-hooks`,
`--build-hf` (force the redb rebuild), `--dry-run` (mutate nothing — always offer this
first for a fleet run).

## Discipline

- **Never live-deploy across repos with running loops.** `--fleet` is safe for the additive
  steps (init/guards/hooks) but the migration step self-defers on any non-quiescent repo.
  Prefer `--dry-run` first on a fleet run and report the plan.
- **Idempotent.** Re-running is a no-op where state is already current; safe to repeat.
- **Fail-closed.** The only data-mutating step (ledger migration) refuses to run unless it
  can positively prove quiescence; uncertainty ⇒ defer, never migrate.
- This is a *project skill* (`/handoff-loop-init`). The `harness:` plugin packaging
  (`/harness:handoff-loop-init`) is the remaining "proper harness setup" — same script,
  re-homed under the harness plugin namespace.
