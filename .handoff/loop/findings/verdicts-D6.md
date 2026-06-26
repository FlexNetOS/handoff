# Verdicts — D6 (claims-vs-code overclaims)

Adversarial verification (assume-false, try-to-refute) of the analyst's D6 overclaim flags
against the live source at `/home/drdave/Desktop/meta/handoff`. Each verdict cites the
counter-evidence I sought. Refutation FAILED for every material overclaim → the analyst
was accurate, not over-accusing.

## 2026-06-25 — verifier pass

### A. Drift sentinel — "10 checks is a count, not content"

- **CLAIM-A1 (checks #1/#8/#10 unbuilt): CONFIRMED.** Read PRD §12.3 list
  (`PRD.md:465-476`) vs `gates::detect()` (`gates.rs:165-342`) in full. Tried to find the
  three: (#1 task-active) `in_progress` (`gates.rs:168-173`) is only a *filter* applied to
  checks 5/6/7 — no `drift.push` ever fires for "claimed task no longer active"; (#8
  contradicts-a-decision-record) the only nearby branch (`gates.rs:324-339`) is the
  *undocumented-decision* advisory, a different semantic — no contradiction check exists;
  (#10 handoff-state-updated) zero packet/view-refresh branch anywhere in the function.
  Refutation failed. **CONFIRMED.**

- **CLAIM-A2 (a different 10 — northstar/dependency/missing-evidence swapped in): CONFIRMED.**
  Source shows three checks absent from the PRD §12.3 list: northstar-revision drift
  (`gates.rs:231-240`), dependency-unsatisfied (`gates.rs:308-322`), missing-test-evidence
  (`gates.rs:298-306`). PRD §12.3 names none of them. The "8→10" CLAUDE.md history is a
  count match, not the documented set; PRD never amended. **CONFIRMED (doc drift).**

- **CLAIM-A3 (§12.5 hard-fails not enforced inside `hf drift`): CONFIRMED (QUALIFIED).**
  `gates.rs:165-342` has no stale-packet-vs-git or lease-overlap branch; lease overlap is a
  CAS at claim (`main.rs:509`, `ledger try_acquire_lease`), not in `detect()`. The single-verb
  claim holds; the *system* covers them elsewhere, so this is correctly scoped as MEDIUM.
  **CONFIRMED as QUALIFIED** (true of `hf drift` alone, not of the whole kernel).

### B. Command contract — two documented verbs never built

- **CLAIM-B1 (`hf index` unbuilt): CONFIRMED.** `grep 'Some("index")|cmd_index'` over
  `hf/src/main.rs` → 0. No dispatch arm; usage omits it. PRD §9 `:323` advertises it.
- **CLAIM-B2 (`hf plan` unbuilt): CONFIRMED.** `grep 'Some("plan")|cmd_plan'` → 0. PRD §9
  `:327` advertises it; DAG is computed ad-hoc in `routing.rs` instead.
- **CLAIM-B3 (`.handoff/maps/*` absent): CONFIRMED.** `ls .handoff/maps` → "No such file or
  directory". Consistent with their generator (`hf index`) being unbuilt.
- **CLAIM-B4 (`hf start` → `hf session start`): CONFIRMED as naming drift.** No top-level
  `Some("start")`; capability relocated to `Some("session") => session::cmd_session`
  (`main.rs:3434`). Capability present; PRD §9 `:330` contract differs. **QUALIFIED**
  (drift, not missing capability — analyst labeled it correctly).
- **CLAIM-B5 (`hf mcp serve` → separate `hf-mcp` binary): CONFIRMED as naming drift.**
  `grep 'mcp serve'` → 0; `hf/src/bin/hf-mcp.rs` exists. PRD §9 `:338` invocation contract
  differs; capability present. **QUALIFIED.**
- **Counter-balance verified:** I did not find any verb advertised in the live usage string
  that lacks a dispatch arm — the overclaims are confined to the PRD, exactly as the analyst
  stated. No false-accusation here.

### C. Architecture & lint contract

- **CLAIM-C1 (12 `handoff-*` crates, resolver 3, edition 2024 — not realized): CONFIRMED.**
  Root `Cargo.toml`: `resolver = "2"` (`:2`), `edition = "2021"` (`:6`), members
  `work-order, ledger, hf, crates/{cli,core,runner,spec,tui}` (`:3`). None of the 12
  `handoff-*` crates exist; 3 functional crates + 5 rusty-idd crates.
- **CLAIM-C2 (no `[workspace.lints]`; `.unwrap()` used): CONFIRMED.**
  `grep 'workspace.lints|unwrap_used|unsafe_code|expect_used' Cargo.toml` → 0. The mandated
  deny-block is absent; `gates.rs:413` and `:542` both call `.unwrap()` — which
  `unwrap_used = "deny"` would reject. Lint-discipline contract unbuilt.
- **CLAIM-C3 (redb supersedes PRD SQLite — NOT an overclaim): CONFIRMED as not-an-overclaim.**
  Correctly flagged as ADR-0017-sanctioned doc evolution, not drift. No double-count.

### D. rusty-idd integration

- **CLAIM-D1 (`crates/intent-analysis` + `hf index --intent-aware` + capsule merge — unbuilt):
  CONFIRMED.** `INTEGRATION-RUSTY-IDD.md:5` names `crates/intent-analysis/`; actual `ls crates/`
  → `cli core runner spec tui` (no `intent-analysis`). `:27` promises `hf index --intent-aware`
  but `hf index` doesn't exist (B1), so `:53` "existing hf index works unchanged" is vacuous.
  `grep '--intent-aware'` / capsule-merge code → 0. Every load-bearing element unbuilt.
- **CLAIM-D2 (crates orphaned from `hf`): CONFIRMED.** `grep intent-analysis|crates/core|
  crates/cli|rusty-idd|idd` across `hf/Cargo.toml`, `ledger/Cargo.toml`, `work-order/Cargo.toml`
  → 0. `grep intent_analysis|rusty_idd|idd::` across `hf/src/` → 0. `crates/core/src/lib.rs:1-8`
  self-describes as an independent "Intent Driven Development … dependency-light" toolkit. They
  compile alongside `hf` but `hf` never invokes them. **CONFIRMED — plan, not feature.**

### E. Smaller mismatches

- **CLAIM-E1 (capsule `next_command`): CONFIRMED.** PRD §13 `:535` = `"hf claim --next"`;
  `init_capsule` sets `"next_command": "hf resume"` (`main.rs:407`), asserted at
  `main.rs:3675` (`assert_eq!(member["next_command"], "hf resume")`). Minor schema-default drift.
- **CLAIM-E2 (17 packet sections in `render_packet_md`): INCONCLUSIVE.** Analyst self-flagged
  as unverified; I did not read the renderer line-by-line. Stays unconfirmed — does NOT flow to
  synthesis as fact. (Follow-up: diff `render_packet_md` headings vs PRD §14.)
- **CLAIM-E3 (NORTH-STAR self-consistency observation): N/A.** Not a code claim; a doctrine
  argument supporting trust-code-over-PRD. No verdict needed.

## Tally

- CONFIRMED: A1, A2, B1, B2, B3, C1, C2, C3(not-overclaim), D1, D2, E1 = **11**
- QUALIFIED: A3, B4, B5 = **3** (true as scoped; capability exists / system covers elsewhere)
- REFUTED: **0** (no false accusation found)
- INCONCLUSIVE: E2 = **1** (analyst-self-flagged; stays unconfirmed)
- N/A: E3

Every material overclaim the analyst flagged survives adversarial refutation. The PRD/INTEGRATION
docs genuinely over-advertise relative to the shipped CLI; the analyst did NOT over-accuse.
