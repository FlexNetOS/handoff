# FLEET_GUIDE.md — the handoff continuity fleet

How every repo in the FlexNetOS meta workspace joins the **Continuity Ledger
Kernel** (`hf` + `.handoff`), keeps its state in sync, and builds its own agent
harness on top. Read this once; it is the contract.

> **One sentence:** the repo is the source of truth; `hf resume` tells any fresh
> agent exactly where things stand and what to do next — no chat archaeology.

---

## 1. The model (read first)

### Two planes
| Plane | Lives in | Holds | Tooling |
|-------|----------|-------|---------|
| **Planning** | git-kb (`.kb/`) | why / what / next: tasks, specs, context docs | `git kb …`, `/kb-board` |
| **Execution** | `.handoff/` + the FLEET ledger | who / when / proof: claims, leases, checkpoints, witnessed events | `hf …` |

Work crosses the seam **one way**: mint a card from a kb task (`hf task mint
--from-kb`), then write progress back (`hf sync`). kb is **never** read back into
the ledger as execution truth (ADR-0003).

### Two ledgers, one per orchestration home (ADR-0004 §3 — settled)
| Ledger | Path | Purpose |
|--------|------|---------|
| **FLEET** | `meta/.handoff/ledger.db` | every member repo's witnessed events (run `hf` from `meta/`) |
| **KERNEL** | `meta/handoff/.handoff/ledger.db` | the handoff kernel's own self-development |

**A per-repo `.handoff/` is git-committed TEXT ONLY — never a `ledger.db`, never
binary state.** Your repo's events live in the FLEET ledger; your repo's packet is
compiled centrally by `hf fleet render`. The beads lesson: binary DB never in git,
text is the git-visible state. `hf fleet status` flags any stray per-repo ledger as
a policy-P7 violation.

### State precedence (settle every conflict with this)
```
Git > ledger (FLEET or KERNEL) > tasks/*.task.json > active.md > packets/latest.md
```
Cards and packets are **derived views** — regenerate them (`hf checkpoint
--sync-cards`, `hf fleet render`), never hand-edit.

---

## 2. Tiers — what your repo needs (policy P7)

| Tier | Who | Required `.handoff/` contents |
|------|-----|------------------------------|
| **A** canon | kernel-adjacent | `context/capsule.json` + `tasks/` + `packets/` + `README.md` (+ optional `hooks/`, `policies/`, `skills/` if it runs an autonomous loop) |
| **B** FlexNetOS tools | most repos here | same as A |
| **C** forks | upstream forks | `context/capsule.json` + `README.md` only — one commit, merge-safe, no CI forcing |
| **D** hubs / docs | catalogs | same as C |

`context/capsule.json` (`handoff.context_capsule.v1`) is **always required** — its
fields (`project_name`, `role`, `plane`, `northstar`, `next_command`) let any agent
landing in your repo learn its place in one read.

---

## 3. Set up `.handoff` in your repo (rollout)

Done for you by the fleet steward, but here is what it does (and how to do it by
hand). **Do not run `hf init`/`hf seed` in a member repo** — those create a
forbidden per-repo `ledger.db` and seed the kernel's own backlog.

```bash
cd <your-repo>
mkdir -p .handoff/context .handoff/tasks .handoff/packets
# 1. capsule (REQUIRED) — describe your repo
cat > .handoff/context/capsule.json <<'JSON'
{
  "schema": "handoff.context_capsule.v1",
  "project_name": "<repo> (<one-line what it is>)",
  "role": "<e.g. ops | tool | library>",
  "plane": "<e.g. execution | planning | env-control>",
  "northstar": "<the repo's guiding goal>",
  "next_command": "hf resume"
}
JSON
# 2. README — the one-screen contract
cat > .handoff/README.md <<'MD'
# .handoff (Tier-A, git-text-only)
Per ADR-0004 §3: text only, no ledger.db. Events live in the FLEET ledger
(meta/.handoff). Packets compiled by `hf fleet render <repo>`.
MD
# 3. commit (git-text only; the FLEET ledger holds events)
git add .handoff && git commit -m "chore: add Tier-A .handoff (git-text-only, ADR-0004 §3)"
```

Then confirm the fleet sees you:
```bash
cd .. && hf fleet status            # your repo should show .handoff = yes
```

---

## 4. Daily use — the `hf` verbs

Run kernel verbs from the **kernel repo or meta root**; member events go to the
FLEET ledger when you run from `meta/`.

| Verb | What it does |
|------|--------------|
| `hf resume [--json\|--compact]` | rehydrate: project / done / remaining / next-safe / next command |
| `hf status [--json]` | full task board from ledger truth |
| `hf intake --bundle FILE [--vibe TEXT] [--intent FILE] [--scope a,b]` | front door: parse a prompt_hub `SwarmBundle` → deterministically synthesize one gate-passing `handoff.task.v1` card per role (no LLM); each shares `correlation_id = workflow_id` |
| `hf dispatch <workflow_id> [--next]` | claim/activate the synthesized orders by `correlation_id` (witnessed `claim` path); `--next` claims only the first |
| `hf claim <ID>` | reserve the weave lease + record the claim (no edit without a claim) |
| `hf checkpoint <ID> [note] [--sync-cards]` | witness progress; `--sync-cards` re-renders cards from ledger truth |
| `hf sync [--auto] [--dry-run]` | repair `.meta.yaml`/`.gitignore` registration + one-way `ledger→.kb` mirror |
| `hf drift [--json]` | detect intent-lock drift + out-of-scope edits — **hard-fails on drift** |
| `hf policy check-claim\|check-edit\|check-handoff [--json]` | enforce the lifecycle gates (deny-without-claim, scope, protected files) |
| `hf fleet status [--json]` | the fleet board: every member's capsule/cards joined with the FLEET ledger |
| `hf fleet render <member>` | compile a member's packet from the FLEET ledger + its capsule/cards |
| `hf ship <ID> [--base BR]` | open the PR (auto-merge gated on green CI + review) |
| `hf review verdict <ID> <PR> approve\|deny [--by WHO]` | record the witnessed gate verdict (NOT a GitHub merge) |
| `hf handoff` | render the packet — the next-session prompt |
| `hf session start\|end [--recycle]` | worktree-isolated loop session (uses the meta worktree engine) |

### The rhythm
1. **Start** → `hf resume` (the SessionStart hook does this automatically).
2. **Claim** → `hf claim <ID>` before editing (no edit without a claim).
3. **Work** in your repo's scope (out-of-scope writes are blocked by `hf policy check-edit`).
4. **Witness** → `hf checkpoint <ID> "<what landed, what's verified, next step>"`.
5. **Sync** → `hf sync` (push progress to `.kb`).
6. **Hand off** → `hf handoff` (the SessionEnd hook does this). The packet IS the next prompt.

---

## 4b. Parallel work with grit (zero merge conflicts)

Many sessions/repos run at once. **`grit`** (ADR-0009) is the fleet's parallel-agent
coordination layer: AST-symbol-level locks + per-agent worktrees + serialized
conflict-free merge. It composes with handoff — handoff locks the **task**, grit
locks the **code symbols**:

| Lock | Tool | Granularity |
|------|------|-------------|
| task | `hf claim <TASK>` | `HFTASK-####` (weave lease + ledger) |
| code | `grit claim <symbols>` | `file::function` (AST) |

**The cycle for any parallel code change:**
```bash
hf claim <TASK>                 # task lease + ledger transition
grit plan                       # declare intent, get free-symbol suggestions
grit claim <file::symbol> …     # lock the functions/types you'll edit
   # … work in the grit worktree (.grit/worktrees/agent-N) — full isolation …
grit done                       # auto-commit → rebase on main → serialized merge → release
hf checkpoint <TASK> "<note>"   # witness the landed work
```
- **grit is the worktree standard** — parallel code work happens in a grit worktree
  (`grit session` / `grit worktree`), not an ad-hoc `git worktree`.
- Different symbols in the **same file** never conflict; `grit done` merges under a
  file lock (no `index.lock` races, no discarded work).
- **Setup:** `grit init` (local SQLite backend, zero-config) — done for every repo by
  `scripts/fleet-rollout.sh`. `.grit/` is binary state and is **gitignored** (same
  rule as the ledger).
- **Cross-repo coordination (shared backend, ADR-0010):** grit's S3/R2/Azure backend
  with credentials injected by envctl (never exported) — run grit via
  `scripts/grit-shared.sh <grit-args>`, which wraps `secretctl run --provider
  grit-backend -- grit …`. **Status: ready but BLOCKED on envctl Phase 8** (the
  `secretctl run` injection data-plane is unbuilt); until then the wrapper degrades to
  local grit and fleet coordination is within-repo only.
- `grit status` shows current locks; `grit symbols` lists what can be claimed.

---

## 5. The lifecycle hooks (autonomous substrate)

`.handoff/hooks/hooks.toml` (`handoff.hooks.v1`) fires `hf` on agent events so the
loop runs with no human in the loop. `fail_mode = block` = a hard gate:

| Event | Command | Mode |
|-------|---------|------|
| SessionStart | `hf resume --compact` | warn |
| PreSessionStart | `hf session preflight --json` | block |
| TaskClaim | `hf policy check-claim --json` | block |
| PreEdit | `hf policy check-edit --json` | block |
| PostEdit | `hf checkpoint --auto --changed-files` | warn |
| PreHandoff | `hf drift --json && hf policy check-handoff --json` | block |
| SessionStop | `hf checkpoint --auto && hf handoff` | warn |
| PostMerge | `hf sync --auto` | warn |

At the Claude Code layer, wire `.claude/settings.json` SessionStart →
`.handoff/hooks/loop-entry.sh` (auto-invokes the loop when a safe task exists) and
SessionEnd → `.handoff/hooks/session-end.sh` (checkpoint + handoff). See the kernel
repo for reference scripts.

---

## 6. Build your own harness (agents + skills)

The kernel ships a reference harness in `handoff/.claude/`. To give *your* repo an
agent team that drives its own loop, run the **harness** meta-skill
(`/harness:harness`) and describe your domain. It creates:

- `.claude/agents/*.md` — the expert roles (who).
- `.claude/skills/*/SKILL.md` — the procedures (how), including an **orchestrator**
  skill that forms the team and runs the cycle.
- a `CLAUDE.md` pointer (trigger rules + change history).

The kernel's own harness is the worked example to copy:

| Agent | Role |
|-------|------|
| `continuity-navigator` | orient + reconcile drift, pick next safe task |
| `kernel-researcher` | mandatory web + codebase research before any decision/ADR |
| `kernel-implementer` | claim → build in scope → witness |
| `kernel-verifier` | drive the binary + cross-boundary QA |
| `code-omniscient-gatekeeper` | witnessed verdict, scope-law, fail-closed (replaces human approval) |
| `fleet-steward` | per-repo `.handoff` rollout/maintenance (git-text-only) |
| `meta-sync-steward` | keep the repo in sync with loop_lib/meta_git_lib, meta_cli, `.kb` |

Skills: `handoff-loop` (orchestrator), `drift-reconcile`, `kernel-research`,
`kernel-verify`, `gatekeeper-review`, `fleet-handoff`, `meta-kb-sync`.

**Eject the kernel harness into your repo** by copying `handoff/.claude/` and
adapting the agent/skill names to your domain, or run `/harness:harness` to generate
a fresh one. Keep the same disciplines: state precedence, witnessed verdicts,
no-edit-without-claim, derived-views-never-hand-edited.

### Gate autonomy
The gatekeeper issues **witnessed verdicts** (`hf review verdict`) that replace
human approval for agent-decidable work — but it is scope-bounded and fail-closed,
and genuine owner walls still escalate: creating/pushing repos, org/infra changes,
irreversible operations, and **merging a protected trunk** (agents never hold the
merge token — a human or an Environment-gated job merges).

---

## 7. Conventions (meta-repo discipline)

- Each repo is an **independent git repo** — use `meta git` / `meta exec` for
  cross-repo operations, never raw loops.
- Register a new repo in `meta/.meta.yaml` + `meta/.gitignore` (`hf sync` repairs
  this idempotently — grep-guarded, never blind-append).
- Snapshot before destructive ops: `meta git snapshot create <name>`.
- Secrets come from **envctl injection** (`envctl run -- <tool>`), never raw
  `export` — envctl holds and auto-injects them.

---

## 8. Quick reference

```bash
hf resume                       # where am I? what's next?
hf intake --bundle b.json       # SwarmBundle -> synthesized task cards (front door)
hf dispatch <workflow_id>       # claim the synthesized cards by correlation_id
hf fleet status                 # whole-fleet board
hf fleet render <repo>          # compile a repo's packet
hf claim <ID> && <work>         # claim, then edit in scope
hf checkpoint <ID> "<note>"     # witness progress
hf drift                        # am I in scope / intent-locked?
hf sync                         # mirror progress to .kb + repair registration
hf handoff                      # render the next-session packet
```

Cold-start any repo: read `.handoff/context/capsule.json` + `.handoff/README.md`,
then run `hf resume`. That is the whole onboarding.
