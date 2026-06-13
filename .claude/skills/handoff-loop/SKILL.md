---
name: handoff-loop
description: "Drives the autonomous Continuity Ledger Kernel loop for the handoff repo: reconcile drift → pick next safe task → research → implement → verify → gatekeeper verdict → ship → handoff, one task per cycle, witnessed. ALWAYS use to run, resume, or continue the kernel loop, to organize/advance the HFTASK backlog, to roll out .handoff across the fleet, or for follow-ups: 'run the loop', 'continue', 'resume', 'pick up where it left off', 're-run', 'do the next task', 'redo only the <phase>', 'based on the previous cycle'. Do NOT use for a single ad-hoc git commit or a one-off question (answer directly)."
---

# Handoff Loop — the Continuity Kernel orchestrator

Coordinates the `handoff` agent team to advance the Continuity Ledger Kernel one
witnessed task per cycle, with full code knowledge, mandatory research, an
autonomous code-omniscient gate, and per-repo `.handoff` fleet control. The repo
is the source of truth; this loop never trusts chat history or a stale packet.

## Execution Mode: Hybrid

| Phase | Mode | Reason |
|-------|------|--------|
| Phase 2 (orient + reconcile) | Sub-agent | Single navigator establishes truth; no team chatter needed |
| Phase 2b (conduct + pull next-best task) | Sub-agent | systems-orchestrator sequences cross-system work + selects the task (hybrid) |
| Phase 3 (research + implement + verify + gate) | Agent team | Tight feedback loop: researcher ↔ implementer ↔ verifier ↔ gatekeeper |
| Phase 4 (cross-workspace coherence: fleet + meta-sync) | Sub-agent | fleet-steward + meta-sync-steward run independent sweeps |
| Phase 4b (doc sync) | Sub-agent | doc-updater regenerates derived views + syncs prose to the change |

## Agent Composition

| Member | Agent type | Role | Skill | Output |
|--------|-----------|------|-------|--------|
| continuity-navigator | continuity-navigator | orient + reconcile drift, pick next safe task | drift-reconcile | `_workspace/01_navigator_truth.md` |
| kernel-researcher | kernel-researcher | deep web+code research + cross-ref | kernel-research | `_workspace/02_research_<ID>.md` |
| kernel-implementer | kernel-implementer | claim + build in scope | drift-reconcile, kernel-verify | `_workspace/03_impl_<ID>.md` |
| kernel-verifier | kernel-verifier | drive the binary + boundary QA | kernel-verify | `_workspace/04_verify_<ID>.md` |
| code-omniscient-gatekeeper | code-omniscient-gatekeeper | witnessed verdict, scope law | gatekeeper-review | `_workspace/05_verdict_<ID>.md` |
| fleet-steward | fleet-steward | repo-per-.handoff rollout/maintenance | fleet-handoff | `_workspace/06_fleet_<scope>.md` |
| meta-sync-steward | meta-sync-steward | sync handoff ⟷ loop_lib/meta_git_lib, meta_cli/conventions, .kb seam | meta-kb-sync | `_workspace/07_metasync_<scope>.md` |
| systems-orchestrator | systems-orchestrator | conduct the integrated systems; pull next-best task (hybrid) | systems-conduct, grit-coordination | `_workspace/08_systems_<scope>.md` |
| doc-updater | doc-updater | sync docs to the change; regenerate derived views | doc-sync | `_workspace/09_docsync_<scope>.md` |

All Agent/TeamCreate calls use `model: "opus"`.

**Task selection (hybrid back-fill policy, owner-decided 2026-06-13):** a repo session
ORIENTS to its next safe task but auto-claims ONLY tasks flagged `ready: true` in their
capsule/card; everything else waits for `systems-orchestrator` to pull the highest-value
task across the fleet under the cycle/budget gate. Never auto-backfill every repo.

## State precedence (settle every conflict)

**Git > `.handoff/ledger.db` > `tasks/*.task.json` > `active.md` > `packets/latest.md`.**
Cards and packets are *derived* — regenerate them (`hf checkpoint --sync-cards`,
`hf handoff`); never hand-edit. This is what keeps "Done 0/22" from lying after a
ship.

## ICM persistent memory (mandatory — `icm-memory` skill)

ICM is the cross-session memory the owner requires. **Recall** relevant memory at
Phase 2 (orient) before selecting/deciding; **store** at Phase 5 (and immediately on
any error-resolved / decision / preference / completion trigger — before responding).
The ledger records what happened; ICM records what was learned and decided. See the
`icm-memory` skill for the recall queries + store triggers/topics.

## Conventions

- **SSH is the git default.** Every `.meta.yaml` remote is `git@github.com:FlexNetOS/…`
  — clones, `git remote`, and `gh`/`meta git` operations assume SSH auth. Never write an
  `https://github.com/…` remote; it will fail the workspace's auth.

## Workflow

### Phase 0: Context check (follow-up support)

1. Check whether `_workspace/` exists in `handoff/`.
2. Decide mode:
   - **Absent** → initial run; create `_workspace/`.
   - **Present + user asks for a partial re-run** (e.g. "redo only the verify",
     "re-research HFTASK-0013") → re-invoke only that agent, overwrite only its
     artifact.
   - **Present + new cycle** → archive `_workspace/` to `_workspace_prev/`, recreate.
3. On partial re-run, pass the prior artifact paths into the agent prompt so it
   improves rather than restarts.

### Phase 1: Preflight

1. Confirm cwd is `handoff/`. Read `.handoff/policy.toml` for loop params
   (`cycle_flush`, base/trunk branch, `permission_gate`).
2. Run the session preflight discipline: clean tree, base synced with origin.
   Work happens in a **grit worktree** (ADR-0009), not an ad-hoc `git worktree`:
   `hf claim <TASK>` (task lease) → `grit plan` → `grit claim <file::symbol>` (AST
   locks) → work in `.grit/worktrees/agent-N` → `grit done` (rebase+merge under file
   lock). Different symbols in the same file never collide, so cycles run truly
   parallel with zero discarded work. (`.grit/` is gitignored binary state.)
3. Note the cycle budget: ship after `cycle_flush` tasks (default 4), then hand off.

### Phase 2: Orient + reconcile  — **Execution mode: Sub-agent**

Invoke `continuity-navigator` (model: opus). It runs `hf resume`, applies the
precedence ladder, **re-renders any stale derived views**, and writes
`_workspace/01_navigator_truth.md` with the ledger-verified backlog and the single
next safe task (`hf claim <ID>`). If it emits a P0 finding (broken witness chain,
ledger unreadable) → STOP and surface it; do not pick a task. **First**, `icm recall`
relevant prior decisions/errors/preferences for the selected task (icm-memory skill) so
the cycle doesn't re-litigate settled architecture or repeat a resolved error.

### Phase 2b: Conduct + select  — **Execution mode: Sub-agent**

Invoke `systems-orchestrator` (model: opus) when the task spans more than one system or
when running autonomously across the fleet. It reads `hf fleet status`, sequences the
cross-system steps (the `systems-conduct` canonical order), and **selects the task by
the hybrid policy**: auto-take only `ready`-flagged tasks; otherwise pull the
highest-value task across the fleet under the cycle/budget gate. Writes
`_workspace/08_systems_<scope>.md`. For a single-repo, single-system cycle this phase
is a no-op (the navigator's next-safe task stands).

### Phase 3: Advance one task  — **Execution mode: Agent team**

1. `TeamCreate(team_name: "handoff-cycle", members: [kernel-researcher,
   kernel-implementer, kernel-verifier, code-omniscient-gatekeeper])`, all opus.
2. `TaskCreate` the cycle's tasks with dependencies:
   - research `<ID>` → (assignee: kernel-researcher)
   - implement `<ID>` (depends_on: research) → kernel-implementer
   - verify `<ID>` (depends_on: implement) → kernel-verifier
   - verdict `<ID>` (depends_on: verify) → code-omniscient-gatekeeper
3. Members self-coordinate via SendMessage (researcher → implementer approach;
   implementer → verifier ready signal; verifier → gatekeeper evidence; gatekeeper
   → implementer on deny). The leader monitors with TaskGet and intervenes on idle.
4. **Producer–Reviewer cap:** gatekeeper deny → implementer fixes only the missing
   evidence → re-verify → re-verdict. Max 3 bounces; then escalate the task.
5. On gatekeeper **approve**: ship per policy — `hf done <ID> --pr <N>` /
   `hf ship <ID> --base master`, then `hf checkpoint --sync-cards` so views match
   truth. The gatekeeper's `hf review verdict` is the recorded approval (autonomous:
   no human stop), but genuine owner walls (NEEDS-HUMAN: physical/account/
   irreversible/scope-expanding) still escalate.
6. `TeamDelete` at cycle end.

**.kb seam through the cycle (ADR-0003, one-way authority).** The planning plane
(git-kb) and execution plane (`.handoff`) stay bound automatically — the loop must
honor it, not bypass it (see the `meta-kb-sync` skill):
- **mint (in):** fleet/pickup-able work is planned in kb first, then minted —
  `hf task mint --from-kb <slug>` stamps `kb_ref` + `correlation_id`.
- **write-back (out):** `hf claim` flips the kb task to `active`; `hf checkpoint`/
  `hf handoff` append a progress line; terminal `done` flips it to `completed` with
  evidence. **kb is never read back into the ledger** — planning informs, never
  overrides, execution truth.

### Phase 4: Cross-workspace coherence (when in scope)  — **Execution mode: Sub-agent**

handoff is a meta member, not an island. Two independent sweeps (invoke as needed,
in parallel):
- **Fleet** — when the task is a fleet rollout item or on an explicit fleet request,
  invoke `fleet-steward` (opus) for a per-repo `.handoff` sweep →
  `_workspace/06_fleet_<scope>.md`.
- **Meta-sync** — when the task touches the loop/worktree engine (`loop_lib`/
  `meta_git_lib`), host CLI/conventions (`meta_cli`, `.meta.yaml`), or the `.kb`
  seam, invoke `meta-sync-steward` (opus) → `_workspace/07_metasync_<scope>.md`. It
  enforces *sync-don't-reimplement* (e.g. `hf session` should depend on
  meta_git_lib, HFTASK-0007) and the one-way `.kb` mirror (HFTASK-0011).

Any change that modifies a sibling repo, `.meta.yaml`/`.gitignore`, or `.kb` goes
back through the gatekeeper for a witnessed verdict before landing.

### Phase 4b: Doc sync  — **Execution mode: Sub-agent**

After the cycle's change lands (and before/with handoff), invoke `doc-updater` (opus):
sync prose docs to the change (FLEET_GUIDE/AGENTS/READMEs verb tables, "planned"→done),
add the CLAUDE.md change-history row, and **regenerate derived views** (`hf checkpoint
--sync-cards`, `hf handoff`, `hf fleet render <repo>`) — never hand-edit them. Writes
`_workspace/09_docsync_<scope>.md` with any doc↔code mismatch as a finding.

### Phase 5: Cycle close + loop

1. Increment the cycle counter. If `< cycle_flush` and more safe tasks remain →
   return to Phase 2 for the next task.
2. At `cycle_flush` (or no safe task, or budget hit): `hf checkpoint` →
   `hf handoff` (re-render the packet) → report. The rendered packet IS the
   next-session prompt.
3. Preserve `_workspace/` (audit trail). Report a per-cycle summary: task shipped,
   verdict, drift reconciled, next safe task.
4. **`icm store`** the cycle's outcome on its trigger (completion → `context-handoff`;
   any decision → `decisions-handoff`; resolved error → `errors-resolved`) before
   handing off — ICM is the cross-session memory (icm-memory skill).

## Data flow

```
[Leader]
  └─ Phase 2: continuity-navigator ─→ 01_navigator_truth.md (next task ID)
       └─ Phase 3 team:
            researcher ─SendMessage→ implementer ─→ verifier ─→ gatekeeper
               │02_research      │03_impl       │04_verify     │05_verdict
               └──────────── all preserved in _workspace/ ─────────┘
                                     ↓ approve
                              hf ship + hf checkpoint --sync-cards
  └─ Phase 4 (opt): fleet-steward ─→ 06_fleet_*.md ─┐
                    meta-sync-steward ─→ 07_metasync_*.md ─┴─→ gatekeeper
  └─ Phase 5: hf handoff (render packet) + kb write-back → loop or stop
```

## Data transfer protocol

- **Task-based** (TaskCreate/Update) for cycle coordination + dependencies.
- **Message-based** (SendMessage) for real-time researcher↔implementer↔verifier↔gatekeeper.
- **File-based** under `_workspace/` (`{phase}_{agent}_{artifact}.md`) for every
  artifact — the audit trail. Use absolute paths rooted at `handoff/_workspace/`.
- The **ledger** (`hf` verbs) is the durable, witnessed transfer that survives the
  session; `_workspace/` is per-run scratch.

## Error handling

| Situation | Strategy |
|-----------|----------|
| Navigator P0 (corrupt ledger/witness chain) | STOP the cycle, escalate as owner wall |
| A member stalls/idles | Leader pings via SendMessage; restart or reassign |
| Gatekeeper deny ×3 on same task | Stop that task, checkpoint partial, escalate; move to next safe task |
| Two classifier denials on a surface | Stop, escalate verbatim, never route around (scope law) |
| Missing `hf` verb (drift/sync/policy) | Fall back to manual reconciliation; log the gap as a kernel task |
| Conflicting evidence | Apply state precedence, re-verify live, keep both sources in the finding |
| Budget/cycle_flush hit mid-task | Checkpoint partial, `hf handoff`, resume next session — never leave tree dirty |

## Test scenarios

### Happy path
1. User: "run the handoff loop." `_workspace/` absent → initial run.
2. Phase 2: navigator finds packet says Done 0/22 but git shows 0007/0009/0011/0020
   merged → reconciles, re-renders cards/packet, picks next safe task (e.g.
   HFTASK-0005 `hf drift` gate).
3. Phase 3 team: researcher dossiers it, implementer builds in scope + checkpoints,
   verifier drives `hf drift --json` + boundary QA → PASS, gatekeeper witnesses
   approve.
4. `hf ship` + `hf checkpoint --sync-cards`; cycle counter = 1 (< 4) → next task.
5. After 4 tasks: `hf handoff` renders the packet; loop reports and stops.
6. Expected: `_workspace/01..05` artifacts present, packet matches ledger truth.

### Error path
1. Phase 3 gatekeeper denies the verify evidence (acceptance criterion unproven).
2. Sends missing-evidence list to implementer; implementer adds the test, re-verify.
3. Second verdict: still missing the cross-boundary check → bounce #2.
4. Third pass PASS → approve → ship. (If a third deny occurred → escalate the task,
   checkpoint partial, move on.)
5. Report notes the bounce count and final verdict.
