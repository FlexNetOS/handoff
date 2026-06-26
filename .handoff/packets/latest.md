# Handoff Packet (latest) — handoff.packet.v2

## 1. North Star
KERNEL DOCTRINE — build a local-first, auditable, reversible, model-native agentic OS where every agent action increases verified capability without corrupting the baseline: Integrity · Reversibility · Capability Gain (no promotion without all three). CECCA/NOA is the executive kernel; the Gold World is the protected baseline; failures compress into evidence. Authoritative: NORTH-STAR.md · keystone docs/adr-0001-flexnetos-autopilot-keystone.md. FLEET VISION (the why): NO HUMAN IN THE LOOP — multi-provider autopilot; user directs, system builds/operates; NEEDS-HUMAN is a scaffold replaced by a model with the human's skillset; end-state = single-person conglomerate. See ../NORTH-STAR.md · ../ARCHITECTURE-TRUTH.md · ../RUVECTOR-RUNBOOK.md

## 2. State Precedence
Git > .handoff/ledger.db > tasks/*.task.json > active.md > this packet.

## 3. Progress
Done: 89/96.  Tamper-evident events verified: 433.

## 0. Next Action / Direction
- **Next safe task:** HFTASK-0087 — Automation rung 3: hf fleet sync — REMEDIATE detected drift, not just report it
- **Next command:** `hf checkpoint HFTASK-0087`
- **Why it is next:** resume the in-progress task (status Claimed) before starting any new work.
- **Cycle / context budget:** context — wrap at ~50% of the context window (cycle_flush=4 caps a runaway cycle); this session is at cycle 0/4.
- **Ready to ship:** no (`hf ship` once the cycle is full / context budget hit).
- **Blocking walls:** none.

## 4. Remaining (next safe first)
- [P2] **HFTASK-0087** — Automation rung 3: hf fleet sync — REMEDIATE detected drift, not just report it
- [P3] **HFTASK-0088** — Automation rung 4: new-repo auto-onboarding (member present, no conformant .handoff)
- [P2] **HFTASK-0089** — Automation rung 5: worktree/branch reap as a hook, not agent memory
- [P1] **HFTASK-0091** — Fleet sync remediates legacy SQLite member ledgers instead of skipping rollup
- [P1] **HFTASK-0092** — hf sync KB mirror is isolated from ambient dirty meta worktrees
- [P2] **HFTASK-0093** — Fleet-member schema checks resolve canonical kernel schemas without local schema files
- [P3] **HFTASK-0094** — hf test zero-test failures include actionable runner/filter diagnostics

## 5. Next Best Task
**HFTASK-0087** — Automation rung 3: hf fleet sync — REMEDIATE detected drift, not just report it
  objective: `hf fleet status` detects non-conformant members (missing ledger guard, no committed ledger.events.jsonl, stale binary) but only REPORTS — a human must then run handoff-loop-init. Add `hf fleet sync` (and/or `hf fleet status --fix`) that, for each member the status sweep flags, runs the idempotent loop-init deploy bits (gitignore guard, hooks, diff-drive workflow, rules) + the binary staleness rebuild (HFTASK-0085), turning detection into remediation. Fail-closed per member (one member's failure never aborts the sweep; report it). Schedulable from a meta-level cron workflow so the fleet self-heals hands-off. Depends on HFTASK-0085.

## 6. Resume Commands
```bash
hf resume
hf claim HFTASK-0087
```

## 7. Machine Summary
```json
{
  "done": [
    "HFTASK-0001",
    "HFTASK-0002",
    "HFTASK-0003",
    "HFTASK-0004",
    "HFTASK-0005",
    "HFTASK-0006",
    "HFTASK-0007",
    "HFTASK-0008",
    "HFTASK-0009",
    "HFTASK-0010",
    "HFTASK-0011",
    "HFTASK-0012",
    "HFTASK-0013",
    "HFTASK-0014",
    "HFTASK-0015",
    "HFTASK-0016",
    "HFTASK-0017",
    "HFTASK-0018",
    "HFTASK-0019",
    "HFTASK-0020",
    "HFTASK-0021",
    "HFTASK-0022",
    "HFTASK-0026",
    "HFTASK-0027",
    "HFTASK-0028",
    "HFTASK-0029",
    "HFTASK-0030",
    "HFTASK-0031",
    "HFTASK-0032",
    "HFTASK-0033",
    "HFTASK-0034",
    "HFTASK-0035",
    "HFTASK-0036",
    "HFTASK-0037",
    "HFTASK-0038",
    "HFTASK-0039",
    "HFTASK-0040",
    "HFTASK-0041",
    "HFTASK-0042",
    "HFTASK-0043",
    "HFTASK-0044",
    "HFTASK-0045",
    "HFTASK-0046",
    "HFTASK-0047",
    "HFTASK-0048",
    "HFTASK-0049",
    "HFTASK-0050",
    "HFTASK-0051",
    "HFTASK-0052",
    "HFTASK-0053",
    "HFTASK-0054",
    "HFTASK-0055",
    "HFTASK-0056",
    "HFTASK-0057",
    "HFTASK-0058",
    "HFTASK-0059",
    "HFTASK-0060",
    "HFTASK-0061",
    "HFTASK-0062",
    "HFTASK-0063",
    "HFTASK-0064",
    "HFTASK-0065",
    "HFTASK-0066",
    "HFTASK-0067",
    "HFTASK-0068",
    "HFTASK-0069",
    "HFTASK-0070",
    "HFTASK-0071",
    "HFTASK-0072",
    "HFTASK-0073",
    "HFTASK-0074",
    "HFTASK-0075",
    "HFTASK-0076",
    "HFTASK-0077",
    "HFTASK-0078",
    "HFTASK-0079",
    "HFTASK-0080",
    "HFTASK-0081",
    "HFTASK-0082",
    "HFTASK-0083",
    "HFTASK-0084",
    "HFTASK-0085",
    "HFTASK-0086",
    "HFTASK-0090",
    "KBTASK-FLEET-HANDOFF-ROLLOUT",
    "TASK-0001",
    "TASK-0002",
    "TASK-0003",
    "TASK-0004"
  ],
  "next_command": "hf claim HFTASK-0087",
  "next_task_id": "HFTASK-0087",
  "project": "handoff (Continuity Ledger Kernel)",
  "remaining": [
    "HFTASK-0087",
    "HFTASK-0088",
    "HFTASK-0089",
    "HFTASK-0091",
    "HFTASK-0092",
    "HFTASK-0093",
    "HFTASK-0094"
  ],
  "schema": "handoff.packet.v2",
  "tasks_total": 96,
  "witnessed_events_verified": 433
}
```

## Contract Proof (ADR-0011 — ruvector-verified/Lean)
Active task **HFTASK-0087** — AgentContract PROVEN via ruvector-verified (5 obligation(s)).
- ✓ `intent:objective` (Eq.refl proof-term #0)
- ✓ `intent:path_scope` (Eq.refl proof-term #1)
- ✓ `intent:acceptance` (Eq.refl proof-term #2)
- ✓ `intent:constraint` (Eq.refl proof-term #3)
- ✓ `intent:northstar` (Eq.refl proof-term #4)
5 proof-term(s) · proof-hash `81782f7e9e455c98` · binding `0x6f7e11d1c30d8a98` · verifier `0x00010000` (lean-agentic 0.1.0).
