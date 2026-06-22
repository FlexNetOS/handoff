# Handoff Packet (latest) — handoff.packet.v2

## 1. North Star
KERNEL DOCTRINE — build a local-first, auditable, reversible, model-native agentic OS where every agent action increases verified capability without corrupting the baseline: Integrity · Reversibility · Capability Gain (no promotion without all three). CECCA/NOA is the executive kernel; the Gold World is the protected baseline; failures compress into evidence. Authoritative: NORTH-STAR.md · keystone docs/adr-0001-flexnetos-autopilot-keystone.md. FLEET VISION (the why): NO HUMAN IN THE LOOP — multi-provider autopilot; user directs, system builds/operates; NEEDS-HUMAN is a scaffold replaced by a model with the human's skillset; end-state = single-person conglomerate. See ../NORTH-STAR.md · ../ARCHITECTURE-TRUTH.md · ../RUVECTOR-RUNBOOK.md

## 2. State Precedence
Git > .handoff/ledger.db > tasks/*.task.json > active.md > this packet.

## 3. Progress
Done: 65/75.  Tamper-evident events verified: 182.

## 4. Remaining (next safe first)
- [P1] **HFTASK-0067** — ADR-0018 D1: commit ALL dotfiles/dirs — reverse the .handoff residency-ignore + invert fleet P7
- [P2] **HFTASK-0069** — ADR-0018 D2: central pre/post hook contract + deployable canonical bundle
- [P2] **HFTASK-0070** — ADR-0018 D5: handoff-central format + cross-fleet deploy for session-relay-resume/-wrap-up
- [P2] **HFTASK-0071** — ADR-0018 D4: more direction from handoff (next-action in hf resume + packet)
- [P2] **HFTASK-0072** — ADR-0018 D7: full adoption of meta/.kb/AGENTS.md (init full .kb + create-first + two-way seam)
- [P2] **HFTASK-0073** — ADR-0018 D8: deeper grit + GitHub grounding (default grit cycle + gatekeeper-as-required-check)
- [P3] **HFTASK-0074** — ADR-0018 D9: real .idea integration + use (run configs, Qodana advisory CI)
- [P1] **HFTASK-0075** — ADR-0018 D10: worktree per task batch, reaped on verified PR merge
- [P1] **HFTASK-0076** — ADR-0018 D11: all PRs->develop; hands-off develop->trunk auto-promotion + master/main reconcile
- [P2] **HFTASK-0077** — ADR-0018 D6: update .claude/rules/* + meta rules to the full-auto model + fleet deploy

## 5. Next Best Task
**HFTASK-0067** — ADR-0018 D1: commit ALL dotfiles/dirs — reverse the .handoff residency-ignore + invert fleet P7
  objective: ADR-0018 D1: moving forward every dotfile/dotdir is git-TRACKED (.handoff incl. ledger + rendered views, .idea, .claude, .github, .kb, .grit config). Stop `hf init`/`scripts/fleet-rollout.sh`/`scripts/handoff-lib.sh` from writing the `.handoff/**/ledger.db`(+wal/shm/rvf/active.md/locks/deliveries/packets) ignore block; REMOVE the existing blocks; ensure those paths are tracked instead. INVERT `hf fleet status` P7 (HFTASK-0034 git_tracks_handoff_db/ledger_guard_present): a tracked `.handoff/ledger.db` is now CONFORMANT; a missing ledger or a present ignore-guard is the VIOLATION. Decide + implement the binary ledger.db conflict story (worktree-isolated per batch HFTASK-0075 + FLEET rollup + serialized merge; binary-merge=ours-replay OR a deterministic text export beside it). Migration artifacts (*.sqlite.bak/*.redb.tmp) stay OUT-OF-TREE (already true, PR #114) — only durable state is committed. Roll the guard removal + P7 inversion ATOMICALLY. Supersedes the ignore half of ADR-0004 §3/§6 + ADR-0016/HFTASK-0035/0037/0048/0021/0066.

## 6. Resume Commands
```bash
hf resume
hf claim HFTASK-0067
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
    "HFTASK-0068",
    "KBTASK-FLEET-HANDOFF-ROLLOUT"
  ],
  "next_command": "hf claim HFTASK-0067",
  "next_task_id": "HFTASK-0067",
  "project": "handoff (Continuity Ledger Kernel)",
  "remaining": [
    "HFTASK-0067",
    "HFTASK-0069",
    "HFTASK-0070",
    "HFTASK-0071",
    "HFTASK-0072",
    "HFTASK-0073",
    "HFTASK-0074",
    "HFTASK-0075",
    "HFTASK-0076",
    "HFTASK-0077"
  ],
  "schema": "handoff.packet.v2",
  "tasks_total": 75,
  "witnessed_events_verified": 182
}
```

## Contract Proof (ADR-0011 — ruvector-verified/Lean)
Active task **HFTASK-0067** — AgentContract PROVEN via ruvector-verified (5 obligation(s)).
- ✓ `intent:objective` (Eq.refl proof-term #0)
- ✓ `intent:path_scope` (Eq.refl proof-term #1)
- ✓ `intent:acceptance` (Eq.refl proof-term #2)
- ✓ `intent:constraint` (Eq.refl proof-term #3)
- ✓ `intent:northstar` (Eq.refl proof-term #4)
5 proof-term(s) · proof-hash `81782f7e9e455c98` · binding `0xdfea9b16c04ee238` · verifier `0x00010000` (lean-agentic 0.1.0).
