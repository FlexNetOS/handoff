# Verdicts — D5 (agent/loop model)

**Verifier pass — 2026-06-25.** Adversarial refutation of the D5 claims against source +
live GitHub branch-protection. Target: `/home/drdave/Desktop/meta/handoff`.

Method: read cited source (cognitum.rs, gatekeeper.rs, routing.rs, gates wiring, hooks.toml,
cmd_handoff/cmd_done in main.rs), `git grep` for negative wiring claims, and a LIVE
`gh api repos/FlexNetOS/handoff/branches/develop/protection` check (the one claim static
reading could not settle).

---

## PRIORITY CLAIMS

### C12 — cognitum gate NOT wired into the loop's firing contract — **CONFIRMED**
Tried to refute by grepping every loop driver. `git grep "policy gate|cognitum"` over
`.handoff/hooks/*` (incl. `hooks.toml`), `scripts/*`, and `.claude/*` (all SKILL.md / agent
md) returns **zero** matches. `cognitum`/`policy gate` appears ONLY in: source (`cognitum.rs`,
the usage string main.rs:3570), ADRs/PRD/CLAUDE.md change-history, the HFTASK-0017 card, and
`.kb` docs. I read `cognitum.rs` in full: `cmd_policy_gate` is a real fail-closed verb
(exits 1 on defer/deny, witnesses `cognitum_decision`), but **nothing fires it**. The
deployed `hooks.toml` wires `check-claim`/`check-edit`/`check-handoff`/`drift`/`preflight`/
`checkpoint` — never `policy gate`. Counter-example search failed → claim holds.
**Verdict: CONFIRMED (negative result, grep-exhaustive across hooks+scripts+skills).**

### C23 — gatekeeper shallow + merge-gate degrades + "NOT a required check yet" — **QUALIFIED (one sub-claim REFUTED)**
Three sub-claims; the live check breaks the third:
- **(a) shallow (git grep, not AST): CONFIRMED.** `gatekeeper.rs:1-10` header admits
  "Full code-intelligence … is not yet wired"; `impact_scan` (`:57-83`) runs `git grep -l`
  on module-name tokens, not the call graph.
- **(b) merge-gate `None` does not hard-fail: CONFIRMED.** `verdict_from_signals`
  (`gatekeeper.rs:168-182`): `None` pushes an advisory reason but the `hard_fail` set is
  `{empty diff, test fail, protected-without-clearance, merge_gate==Some(false)}` — `None`
  excluded. Pinned by test `default_required_check_can_approve_when_merge_gate_is_unlinked`
  (`:337-346`).
- **(c) "NOT a GitHub branch-protection *required* check yet": REFUTED.** LIVE
  `gh api .../branches/develop/protection` → `required_status_checks.contexts` =
  `["Test (ubuntu-latest)","Test (macos-latest)","Clippy","Format","AI Gatekeeper"]`.
  **"AI Gatekeeper" IS a required status check on develop**, and a real workflow produces it:
  `.github/workflows/ai-gatekeeper.yml:22,60-62` runs the witnessed
  `cargo run -p hf -- gatekeeper check <pr> --task HFTASK-0073` as a required job named
  "AI Gatekeeper". So the analyst's "not yet a required check" is factually wrong.
- **Operative caveat that rescues the claim's *thrust*:** `enforce_admins.enabled = false`
  (live) and the repo `CLAUDE.md:42` documents the standing merge flow as
  `gh pr merge <n> --admin --squash` — **admin bypass** of the required checks. So the
  required check exists AND is routinely bypassed by the owner via `--admin`.
**Verdict: QUALIFIED.** (a)+(b) CONFIRMED; (c) REFUTED — the AI Gatekeeper is wired as a
required branch-protection check + a live CI job; it is nonetheless admin-bypassable
(`enforce_admins=false` + documented `--admin` flow).

### C8 — authoritative "next task" is topological `next_safe`, not the bandit — **CONFIRMED**
`next_safe` (main.rs:319-334) read verbatim: step 1 resumes an in-progress task
(Claimed/Checkpointed/Active/Review), step 2 picks the first Backlog card with all deps Done.
14 call sites incl. `cmd_handoff` (main.rs:2597) and BOTH policy gates (gates.rs:480,518),
`sync.rs:326`. The bandit is not among them. **Verdict: CONFIRMED.**

### C9 — bandit (`route_with_history`) scoped to `hf claim --batch` only — **CONFIRMED**
`git grep route_with_history` → exactly one non-test caller: main.rs:988, inside
`cmd_claim_batch`. Read main.rs:955-995: the batch path first resumes any in-progress task
(mirrors next_safe step 1), then builds `candidates` = Backlog with deps-all-Done (the SAME
predicate as next_safe step 2, main.rs:967-974), then routes among them. The bandit chooses
*among* topologically-safe tasks and never overrides dependency safety.
**Verdict: CONFIRMED.**

---

## CORROBORATED (read or cross-checked this pass)

- **C10 — bandit learns from ledger:** main.rs:986-988 `routing_history(&tasks)` feeds
  `route_with_history`; seed = witnessed-chain count (main.rs:981-984). Wiring CONFIRMED;
  the Bayesian-update internals (`routing.rs:55-66`) not re-derived line-by-line →
  treat learning-claim as **CONFIRMED (wiring) / plausible (math not re-run)**.
- **C11 — cognitum verb exists, fail-closed:** `cognitum.rs:102-164` read in full; exits 1
  on defer/deny, exit 2 without the feature, witnesses `cognitum_decision`. **CONFIRMED.**
- **C15/C16 — typed 14-event contract:** `CONTRACT_EVENTS` = 14 (counted, hooks.rs:32-47).
  **CONFIRMED** for the count; envelope/`unknown_events` internals not re-read → C16 detail
  **INCONCLUSIVE** (count CONFIRMED).
- **C17 — hooks.toml wires the gates:** `hooks.toml` read in full. TaskClaim→check-claim
  (block), PreEdit→check-edit (block), PreHandoff→`drift && check-handoff` (block),
  PreSessionStart→preflight (block), PostTest→drift (block), SessionEnd→
  `checkpoint && handoff && export && sync` (warn); PreCommand/PreTest are warn. Header:
  "how the loop runs with no human in the loop." **CONFIRMED** (and reconfirms C12 — no
  cognitum line present).
- **C21 — continuity verbs replace the human gate inside the kernel:** cmd_handoff
  (main.rs:2578-2591) proves the AgentContract via `contract::prove_contract` and
  `exit(1)` on `Err` BEFORE rendering any packet (fail-closed, ADR-0011). cmd_done
  (main.rs:1339-1373) blocks Done on a missing green `test_result` (`latest_test_passed`),
  then auto-witnesses `pr_merged`, calls `promote_develop_to_trunk` + `sync_develop_to_trunk`
  + `session::reap_open_session_if_merged`. No human-approval read in these paths.
  **CONFIRMED.**
- **C22 — gatekeeper witnessed Approve/Deny, fail-closed:** `cmd_gatekeeper_check`
  (gatekeeper.rs:199-299) gathers gh changed files + `cargo test --workspace` + impact scan
  + optional merge-gate, then `verdict_from_signals`; Deny `exit(1)`; witnesses
  `gatekeeper_judgment` (`:279`). **CONFIRMED.**

---

## NOT INDEPENDENTLY RE-VERIFIED THIS PASS (fail-closed → INCONCLUSIVE)

Source not opened this pass; claims are internally consistent and partly corroborated by the
files I did read, but per the fail-closed rule they are NOT upgraded to fact on this pass:
- **C1–C7** (session.rs: lease/preflight/worktree/grit/reap/cycle-counter) — INCONCLUSIVE.
  Note: C5/C6 reap behavior is corroborated *downstream* by cmd_done calling
  `reap_open_session_if_merged` (C21 path), consistent with "reap only on witnessed merge."
- **C13/C14** (gates.rs exit codes + 10-check drift) — INCONCLUSIVE on internals; the
  block-mode wiring that depends on these verbs exiting nonzero is CONFIRMED via hooks.toml.
- **C18** (.claude/settings.json auto-invoke) — partially corroborated: `loop-entry.sh`
  read (emits the handoff-loop directive only when `hf resume --json` yields a safe
  next_task); settings.json itself not re-read → INCONCLUSIVE on the exact wiring lines.
- **C19/C20** (kb.rs one-way seam + plane routing) — INCONCLUSIVE (not opened).
- **C24** (review_verdict is a record, not a blocking gate) — partially corroborated: no
  `review verdict` line appears in hooks.toml (consistent with "not loop-blocking"), but
  cmd_review_verdict body not read → INCONCLUSIVE.

---

## NET CORRECTION TO THE DIMENSION VERDICT

The D5 verdict line (findings :8-12) conflates two distinct things and is **partly wrong on
the gatekeeper**:
- **cognitum action governor:** genuinely NOT wired into the firing contract — CONFIRMED
  (C12). Accurate.
- **AI gatekeeper:** the claim that it is "not wired at the GitHub merge boundary" /
  "not a required check" is **REFUTED** — it IS a required status check on `develop`
  (live) and a real CI job (`ai-gatekeeper.yml`). The correct statement is: the gatekeeper
  is enforced at the merge boundary as a required check, **but it is admin-bypassable**
  (`enforce_admins=false` + the documented `gh pr merge --admin --squash` flow), and the
  check itself is shallow (git-grep impact, merge-gate degrades to non-blocking `None`).

So "no human in the loop at the GitHub boundary" is **structurally enforced (required check)
yet operationally bypassed by the owner via `--admin`** — not "absent / degrades to rely on
branch protection." Branch protection genuinely carries the required AI Gatekeeper check; the
hole is the admin bypass, not a missing check.

---

## TALLY (D5)
- CONFIRMED: C8, C9, C11, C12, C17, C21, C22  (+ C15 count, C10 wiring) = **9**
- QUALIFIED: C23 (sub-claims a,b confirmed; sub-claim c REFUTED)
- REFUTED: C23(c) "AI Gatekeeper is not a required check" (live counter-evidence)
- INCONCLUSIVE (not re-verified, fail-closed): C1–C7, C13, C14, C16(detail), C18, C19,
  C20, C24

Only CONFIRMED + the QUALIFIED-confirmed portions of C23 may flow to synthesis. The C23(c)
refutation MUST be carried into the report — the gatekeeper IS a required check (bypassable),
contradicting the analyst's "not yet."
