---
name: grit-coordination
description: "The full grit feature set for parallel-agent code coordination: claim with read/write --mode, --queue/--wait, heartbeat (TTL), assign, status/watch, gc, and the envctl-backed shared backend for cross-repo locks. ALWAYS use when an agent will edit code that another session might touch, when coordinating parallel agents, or when setting up cross-repo symbol coordination. Do NOT use for task-level continuity (that's hf claim) or per-repo .handoff rollout (that's fleet-handoff)."
---

# grit-coordination — claim symbols, never collide, merge clean

grit (ADR-0009/0010) is the code-coordination plane: AST-symbol locks + per-agent
worktrees + serialized conflict-free merge. handoff locks the **task** (`hf claim`);
grit locks the **code** (`grit claim`). Use grit's *working* primitives — init, claim,
done, status, queue, heartbeat, assign, gc, watch. (NB: `grit session start` is broken
in grit 0.3.0 — `git checkout -b grit/<n> --`, empty base — do not use it; `hf session`
already grit-inits the session worktree instead.)

## The cycle (per parallel code change)

```bash
hf claim <TASK>                                   # task lease (continuity)
grit plan --agent <id> --intent "<what>"          # suggestions: free symbols in scope
grit claim --agent <id> --intent "<what>" \
   --mode write f.rs::sym  [--ttl 600] [--queue] [--wait 120]
   # work in .grit/worktrees/<id> — full isolation
grit heartbeat --agent <id> --ttl 600             # refresh before TTL expires on long work
grit done --agent <id>                            # auto-commit → rebase → serialized merge → release
hf checkpoint <TASK> "<note>"                      # witness the landed work
```

## The full feature set (use the right one)

| Need | Verb | Notes |
|------|------|-------|
| lock exact symbols | `grit claim --agent A --intent x --mode write f::sym …` | write = exclusive (default); **read = shared** (many readers, no writer) |
| can't pick symbols | `grit assign --agent A --intent x --file f.rs` | auto-pick + claim a free symbol in a file |
| blocked, don't fail | `grit claim … --queue` | FIFO; auto-granted on release. `grit queue list/cancel` |
| blocked, wait | `grit claim … --wait <secs>` | retries with backoff until granted or timeout |
| long work | `grit heartbeat --agent A --ttl <secs>` | refresh before the TTL (default 600s) expires, or the lock is reclaimed |
| see state | `grit status` (locks) · `grit symbols [-f file]` (claimable) · `grit watch` (live events: socket local / `--poll` S3) |
| finish | `grit done --agent A` | merges the agent's worktree, releases all its locks |
| cleanup | `grit gc` | garbage-collect expired locks |

## Mode discipline (read vs write)

- **`--mode write`** (default): exclusive — for editing a symbol. Another writer is
  blocked/queued; readers are blocked.
- **`--mode read`**: shared — for *reading/analyzing* a symbol you must not have change
  under you (e.g. a caller you're refactoring against). Multiple readers coexist; a
  writer waits. Use read locks when blast-radius analysis needs a stable view, so you
  don't over-serialize.

## TTL + heartbeat (don't lose a lock mid-work)

Default lock TTL is 600s. Long edits MUST `grit heartbeat` before expiry or the lock is
reclaimed (a 50-agent run reclaims aggressively). The handoff claim TTL (`CLAIM_TTL_SECS
=3600`) is the outer task bound; grit's per-symbol TTL is the inner code bound — keep
the heartbeat cadence under the grit TTL.

## Shared backend for CROSS-REPO coordination (ADR-0010)

Local SQLite coordinates agents **within one repo**. Cross-repo symbol coordination
needs a **shared** backend (S3/R2/Azure). Credentials come from **envctl injection,
never `export`** (secrets charter):

```bash
grit config set-s3 --bucket <fleet-bucket> --endpoint <…> --region <…>   # non-secret, once per repo
scripts/grit-shared.sh claim --agent A --intent x f::sym                 # runs grit under envctl injection
```
`scripts/grit-shared.sh` wraps `secretctl run --provider grit-backend -- grit …`.
**Status: BLOCKED on envctl Phase 8** (`secretctl run` data-plane unbuilt) — the wrapper
degrades to local grit with a clear message until Phase 8 lands. Until then, cross-repo
coordination is *not* available; within-repo is.

## Known grit bugs (degrade around them, report upstream)

- `grit session start` → broken (empty base). Use init/claim/done.
- `grit claim <unknown-symbol>` → leaks `FOREIGN KEY constraint failed`; pre-check with
  `grit symbols`.
- `grit status` may mark a fresh lock `[EXPIRED]` yet still enforce it.

## Why this over raw git

At 10–50 parallel agents, raw git loses 70–90% of work to file-level merge conflicts.
grit's symbol locks + serialized merge lose 0%. The ceremony (two claims: task + code)
is encoded in the harness so agents do it automatically.
