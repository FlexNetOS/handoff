---
name: kernel-verify
description: "Evidence-capture protocol for verifying the hf binary and the ledger/cards/packet/git boundaries before a gatekeeper verdict. ALWAYS use to 'prove it works', drive the hf CLI, or before marking any kernel task done. Runtime observation + cross-boundary comparison only — do NOT substitute cargo test alone (tests are CI's job, not surface evidence)."
---

# kernel-verify — drive the binary, compare the boundaries

Verification is **runtime observation at the surface a user touches** plus
**cross-boundary comparison** of the artifacts that must agree. Reading the code or
re-running CI proves nothing about the surface. Run incrementally — after each
module — not once at the end.

## Protocol

1. **Establish the change.** `git diff @{u}.. --stat` (or `gh pr diff <n>`) in
   `handoff/`. The diff is ground truth; the claim is an assertion about it. Note
   mismatches — they're findings.
2. **Build the real artifact.** `cargo build -p hf` (+ affected `ledger`/`work-order`).
   Build output is setup, not evidence — don't quote it as proof.
3. **Run the contract tests.** `cargo test` across the workspace. Report failures
   only; a green run is one line.
4. **Drive the documented surface, happy path first.** Run the exact `hf`
   invocation the card/claim names. Capture stdout+stderr **separately**
   (`cmd >out 2>err`) and the exit code **unpiped** — `cmd | head` clobbers `$?`
   (verified gotcha: use `cmd >f; echo $?` or `${PIPESTATUS[0]}`).
5. **Probe around the claim** (≥1 probe or the verdict is a happy-path replay):
   - flags: empty value, given twice, conflicting pair, typo'd (does the error NAME the flag?)
   - args: missing required, extra positional, `--` handling
   - state: run twice (idempotent?), different CWD (path resolution), missing/locked
     ledger file
   - env: keyless `--help` must still exit 0; for envctl-injected tools verify the
     `envctl run -- <tool> …` shape, never raw exports
   - exit codes: usage error = 2, runtime error = 1, success = 0
6. **Cross-boundary QA** (compare shapes, don't existence-check). Read both sides at
   once and confirm agreement:
   - ledger events ↔ rendered cards (`tasks/*.task.json`)
   - cards ↔ `active.md` `Done X/Y` ↔ `packets/latest.md`
   - all of the above ↔ git history (merged ships)
   - card body ↔ its `intent_lock` (blake3)
   - witness chain verifies end-to-end (`hf resume --json` count / kernel verify path)
7. **Report inline** (the /verify format):
   - **Verdict:** PASS | FAIL | BLOCKED | SKIP — no partial pass; ambiguous output
     is FAIL with the raw capture attached.
   - **Steps:** one line per action on the RUNNING binary → quoted output. Mark
     probes 🔍 — a probe that held is still a finding ("🔍 empty --from → clean
     error, exit 2").
   - **Findings:** anything that made you pause — unhelpful errors, odd defaults,
     doc/behavior drift, boundary mismatch, slowness. Pre-existing breakage is a
     finding, not noise.
8. **Witness it.** `hf checkpoint "<verdict>: <surface> — <one-line evidence>"` so
   the verification survives the session. Output `_workspace/04_verify_<TASKID>.md`.

## Isolation

Shared state (the live ledger, sockets, tmux) → isolate: `mktemp -d` homes,
`tmux -L <name>`. Never drive against the live `handoff/.handoff/ledger.db` that
another session owns — copy it or use a temp home. The rtk hook rewrites bare
`grep`/`curl`; use `rtk proxy grep` or tool alternatives inside probes.

## House truths (save the rediscovery)

- `hf` verbs run from `handoff/`; ledger is `handoff/.handoff/ledger.db`
  (`sqlite3` CLI absent — read via `python3` sqlite3).
- Keyless `--help` is the regression probe for envctl-injected tools (e.g. `teri`
  historically died on missing `LLM_API_KEY` before arg-parse).
- Async (CI, auto-merge) → Monitor/poll, never `sleep`.
- `cargo test` green is necessary but NOT sufficient — the surface drive + boundary
  cross-check is what makes the verdict real.
