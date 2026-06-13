# handoff — Continuity Ledger Kernel

The repo is the source of truth; chat history is not. First command in any session:
`hf resume`. Navigation order and hard rules live in `AGENTS.md`.

## Harness: Handoff Loop (Continuity Kernel Loop)

**Goal:** advance the kernel one witnessed task per cycle — reconcile drift → research →
implement → verify → autonomous code-omniscient gate → ship → handoff — and keep
`.handoff` control conforming across the repo and the fleet.

**Trigger:** for any work that runs/resumes/continues the kernel loop, advances the
HFTASK backlog, reconciles drift between rendered views and ledger/git truth, or rolls
out/maintains `.handoff` across the fleet, use the `handoff-loop` skill. It also handles
follow-ups ("continue", "resume", "re-run", "do the next task", "redo only the <phase>").
Simple questions may be answered directly.

**Autonomy:** the gatekeeper is autonomous (witnessed verdicts replace human approval,
HFTASK-0014) but scope-bounded and fail-closed; genuine owner walls (NEEDS-HUMAN:
physical/account/irreversible/scope-expanding) still escalate.

**Change history:**
| Date | Change | Target | Reason |
|------|--------|--------|--------|
| 2026-06-13 | hf session grit-enables its worktree + envctl-backed shared grit backend (design) | hf/src/session.rs (grit_enable on session worktree, HFTASK-0007); +docs/adr-0010-grit-shared-backend-envctl.md + scripts/grit-shared.sh (ready, degrading) + FLEET_GUIDE §4b shared-backend note | Owner: build hf session→grit delegation + the shared backend. session→grit landed (#23). Shared backend BLOCKED on envctl Phase 8 (`secretctl run` data-plane unbuilt) — shipped ready+degrading, honestly scoped. Found grit 0.3.0 `grit session start` broken (uses init/claim/done instead) |
| 2026-06-13 | Adopt grit as the fleet parallel-agent coordination layer | +docs/adr-0009-grit-parallel-coordination.md; fleet-rollout.sh runs `grit init` per repo (local backend, `.grit/` gitignored); FLEET_GUIDE §4b "Parallel work with grit"; kernel-implementer + handoff-loop adopt the `hf claim`→`grit claim`→grit-worktree→`grit done` cycle | Owner directive: worktrees are the proper way + implement grit fleet-wide. grit (AST-symbol locks + worktrees + serialized merge) = code-coordination plane; handoff = continuity plane. Built in a worktree per the standing preference |
| 2026-06-13 | Initial build | 6 agents (continuity-navigator, kernel-researcher, kernel-implementer, kernel-verifier, code-omniscient-gatekeeper, fleet-steward), 6 skills (handoff-loop orchestrator, drift-reconcile, kernel-research, kernel-verify, gatekeeper-review, fleet-handoff), CLAUDE.md pointer | Loop automation + whole-codebase control + mandatory research + repo-per-.handoff fleet control + close the drift/stale-view gap |
| 2026-06-13 | Add meta-sync surface | +agent meta-sync-steward, +skill meta-kb-sync, orchestrator Phase 4 → "cross-workspace coherence" (fleet + meta-sync), .kb seam wired into the per-task loop, drift-reconcile cross-links the seam | Keep handoff in sync with loop_lib + meta_git_lib (loop/worktree engine), meta_cli + org conventions, and the .kb planning↔execution seam (ADR-0003, one-way) — owner-flagged omission |
| 2026-06-13 | Wire loop auto-invoke + pilot scope | +.claude/settings.json (SessionStart→loop-entry.sh auto-invokes handoff-loop when ledger has a safe task; SessionEnd→session-end.sh checkpoint+handoff safety net), +.handoff/hooks/{loop-entry,session-end}.sh, +.handoff/fleet/PILOT.toml (rollout gated to flexnetos_runner only), fleet-handoff+fleet-steward honor the pilot gate | Auto-run the loop on session start; start fleet rollout with one clean test repo (flexnetos_runner) before fleet-wide (owner directive). Pilot rollout smoke-tested green: independent ledger + witness chain verified; surfaced ADR-0006 portability bug (cmd_handoff hardcodes handoff's northstar in the packet renderer) |
| 2026-06-13 | Reconcile fleet model to ADR-0004 §3 (envctl FINDING-0002) | fleet-handoff + fleet-steward + meta-kb-sync + drift-reconcile + continuity-navigator rewritten to two-ledger residency: FLEET=meta/.handoff/ledger.db, KERNEL=meta/handoff/.handoff/ledger.db, per-repo .handoff = git-text-only (NO ledger.db); aggregation via unbuilt `hf fleet status`. Removed forbidden flexnetos_runner/.handoff/ledger.db (kept git-text capsule/policy/README). PILOT.toml notes git-text-only rollout | My initial "one ledger per repo" model contradicted the settled ADR-0004 §3; envctl's committed FINDING-0002 + agenticOS "ledger-residency $META_ROOT only" gate are authoritative. Cross-boundary QA caught my own design error |
