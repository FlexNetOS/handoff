# ADR-0009 — grit as the fleet parallel-agent coordination layer

**Status:** accepted (2026-06-13) · **Owner:** handoff kernel (orchestration plane) ·
**Derived from:** owner directive 2026-06-13 ("worktree is the proper way … implement
grit here for everyone"), ADR-0004 (fleet `.handoff`), HFTASK-0002 (weave leases) +
HFTASK-0007 (`hf session` worktree engine), the live weave heartbeats showing multiple
sessions/repos picking up each other's tasks in parallel.

## Context

The fleet now runs **many concurrent agent sessions across many repos** (verified live:
the envctl agenticOS loop and the handoff loop interleave, picking up tasks from each
other). Raw git breaks under parallel agents: two agents editing different functions in
the **same file** produce conflicting line-level hunks, the merge fails, and the losing
agent's work is discarded. Measured: at 50 parallel agents on raw git, ~90% of work is
lost to conflicts.

handoff already provides **continuity** (ledger, resume, task cards) and **task-level**
coordination (weave leases on a task id via `hf claim`). It does **not** provide
**code-level** parallel-write safety: nothing stops two sessions editing overlapping
symbols, and `hf session` (HFTASK-0007) only wraps `meta git worktree`, with no
conflict-free merge.

`grit` (FlexNetOS/grit 0.3.0, already built + installed at `~/.local/bin/grit`) solves
exactly this: **AST-symbol-level locks + per-agent worktrees + serialized conflict-free
merge** — claim → work-in-worktree → done.

## Decision

Adopt **grit as the fleet's parallel-agent code-coordination layer**, complementary to
the handoff continuity kernel. Two planes, two lock granularities:

| Plane | Tool | Locks | Unit |
|-------|------|-------|------|
| **Continuity** | `hf` (handoff) | weave lease on a **task** | `HFTASK-####` |
| **Code coordination** | `grit` | AST locks on **symbols** + worktree | `file::function` |

**The standard cycle for any parallel code work:**
```
hf claim <TASK>          # task-level lease + ledger transition (continuity)
grit plan / grit claim <symbols>   # AST-symbol locks (code coordination)
   … work in the grit worktree (.grit/worktrees/agent-N) — full isolation …
grit done                # auto-commit → rebase on main → serialized merge → release
hf checkpoint <TASK> …   # witness the landed work (continuity)
```

1. **grit is the worktree standard** (answers the owner's "worktree is the proper
   way"). Parallel code work happens in a grit worktree (`grit session`/`grit
   worktree`), not an ad-hoc `git worktree`. `hf session` delegates its worktree to
   grit where grit is initialized; it falls back to `meta git worktree` otherwise.
2. **Symbol locks, not file locks.** Agents `grit claim` the specific functions/types
   they will edit; different symbols in the same file never conflict.
3. **Serialized conflict-free merge.** `grit done` rebases + merges under a file lock —
   no `index.lock` races, no discarded work.
4. **Backend.** Local SQLite WAL by default (per-repo, zero setup — coordinates agents
   *within* a repo). The **cross-repo / team** upgrade is a shared backend (S3 / R2 /
   Azure) whose credentials are supplied by **envctl injection** (`envctl run -- grit
   …`), never raw env — consistent with the secrets charter.
5. **Residency / git hygiene.** `.grit/` (registry.db, room.sock, worktrees/) is
   **gitignored** — binary coordination state never enters git, exactly as the handoff
   ledger is binary-out-of-git (ADR-0004 §3). grit init adds the ignore automatically.
6. **Fleet-wide rollout.** `grit init` (local backend) is added to the deterministic
   `scripts/fleet-rollout.sh`, so every member repo gets grit coordination alongside
   its git-text `.handoff`. This is the "for everyone" requirement.

## Research

- **grit mechanism** (FlexNetOS/grit 0.3.0 README + `--help`, read 2026-06-13):
  function-level AST locking via tree-sitter (13 languages incl. Rust:
  functions/structs/enums/traits/impls/types); CLAIM→WORK(worktree)→DONE(rebase+merge,
  file-locked); subcommands `init/claim/release/status/symbols/plan/done/watch/
  worktree/queue/gc/session/config/assign/heartbeat`; backends local-SQLite (default,
  WAL), Azure Blob (atomic `If-None-Match`, Event Grid events), S3/R2 (conditional PUT).
  Verified live: `grit init` in the handoff repo indexed 189 symbols / 258 deps and
  parsed the Rust kernel correctly; `.grit` auto-gitignored.
- **Measured efficacy** (README benchmark): raw git loses 70–90% of work at 10–50
  agents; grit loses 0% (50 agents on Azure: 54–76 merges, 0 conflicts, 6–24s).
- **Cross-validation with the existing model:** grit's worktree+serialized-merge is the
  same isolation discipline HFTASK-0007 reaches for via `meta_git_lib`; grit supersedes
  the ad-hoc path with conflict-free merge. grit's binary `.grit/` mirrors handoff's
  binary ledger residency rule (ADR-0004 §3) — both gitignored, text/state separated.

## Cross-References

- **ADR-0004 §3** — residency (binary state gitignored): `.grit/` follows the same rule.
- **HFTASK-0002** (weave leases) — task-level locks; grit adds the *symbol-level* tier.
- **HFTASK-0007** (`hf session` + `meta_git_lib`) — grit becomes the worktree backend;
  `hf session` delegates to it (follow-up: wire the delegation in `session.rs`).
- **envctl secrets charter** (ADR-0007 / owner intent) — shared grit backend creds via
  `envctl run -- grit`, never raw env.
- **FLEET_GUIDE.md** — adds the "Parallel work with grit" section; the harness
  `kernel-implementer` + `handoff-loop` adopt the claim→worktree→done cycle.

## Consequences

- Parallel agents across the fleet stop discarding each other's work; throughput scales
  with agent count instead of collapsing.
- Two claims per parallel code task (`hf claim` task + `grit claim` symbols) — slightly
  more ceremony, encoded in the harness so agents do it automatically.
- New follow-up tasks: (a) `hf session` → grit worktree delegation; (b) shared grit
  backend via envctl for true cross-repo symbol coordination; (c) `grit init` in the
  fleet rollout (landed with this ADR).
- `.grit/` is per-repo binary state — never commit it; the registry rebuilds from code.
