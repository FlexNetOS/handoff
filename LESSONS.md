# Handoff Harness — Lessons Ledger

Durable, append-only memory of what each run taught the harness. Never truncate — recurrence
history is the whole point. One row per lesson: date · lesson (class) · evidence · recurrence ·
routed-to · status(noted|applied|proposed). Companion to the CLAUDE.md change history (which
records *applied* upgrades) and `.handoff/loop/evaluation.md` (per-run scorecard, scratch).

Status legend: **noted** (seen once, recorded; act on 2nd recurrence) · **proposed**
(upgrade written to `_workspace/proposed-upgrades.md`, awaiting owner ratification) · **applied**
(landed via PR + CLAUDE.md change-history row).

---

## 2026-06-13 — Phase E wrap-up retro (kernel loop + owner-directed architecture session)

Run shipped (all merged, witnessed): HFTASK-0003, 0026 (ledger kernel/fleet routing — fixed a
cwd-relative CONTAMINATION where envctl kb-tasks landed in handoff's kernel ledger), 0027 (hf
resume live count), 0028 (concurrent ledger-write serialization, BEGIN IMMEDIATE), 0029 (hf
hygiene — ship/seed/claim safety), 0030 (preflight CI-mirror), 0031 (rollup-provenance schema),
0032 (hf sync per-repo→central rollup). Plus: wired the kernel to its north-star (ADR-0006),
adopted the owner's two-level NORTH-STAR doctrine, authored meta NORTH-STAR v2, and REVISED
ADR-0004 §3 (per-repo gitignored ledger + central rollup, reversing the prior "no per-repo
ledger.db" policy).

| # | Lesson (class) | Evidence | Recurrence | Routed-to | Status |
|---|----------------|----------|------------|-----------|--------|
| L1 | **Search canon before synthesizing.** When the owner says "X is missing", GREP the meta root for existing canonical docs and query ICM memoir/memory BEFORE spinning up a research/synthesis workflow — the artifact often already exists at a higher level. | Owner said "handoff is missing the comprehensive vision/plan"; leader spun up a **15-agent** workflow to re-derive it — but NORTH-STAR.md, ARCHITECTURE-TRUTH.md, RUVECTOR-RUNBOOK.md + an icm memoir "system-architecture" already existed at meta root. Owner stopped the workflow ("search meta root… call icm memoir"). Wasted a large fan-out. | 1 (related: prompt_hub "copy the FULL structure, not the thin seed" — same search-before-act class) | orchestrator (handoff-loop skill) — add a "search canon / recall ICM" gate to the research/synthesize step | proposed |
| L2 | **A CLASS of unsafe hf-verb defaults: mutating/destructive defaults + missing --help/safety guards.** Every `hf` verb with side effects should guard `--help`/`-h` before execution and stage narrowly (never `git add -A`), fail non-zero on BLOCKED, and never clobber existing state on re-run. | `hf ship` did `git add -A` (swept scratch into PR #29); `hf seed` CLOBBERED done-card status→backlog on re-seed; `hf claim` exited 0 when BLOCKED; `hf sync --help` EXECUTED the rollup, mutating the real FLEET ledger during verification. (0029 fixed ship/seed/claim; 0032 fixed sync --help.) | 2+ (≥4 instances of the same class in one run) — **escalate now** | a standing **hf-verb-safety check** (script in `scripts/`, callable; OR a checklist criterion in kernel-verify / gatekeeper-review skills) | proposed |
| L3 | **verify/preflight must MIRROR each repo's actual CI invocation, not a fixed subset.** A subset gate that is *narrower than CI on the same dimension* (not just fewer dimensions) silently false-passes. | Local preflight ran `clippy --all-features`; CI ran `clippy --all-targets` — a test-code lint passed local gate, failed CI (PR #30). Fixed in 0030 (per-repo CI-mirror); kernel-verifier/implementer agent defs now mandate `--all-targets`. | 1 | confirm the generalization in kernel-verify skill + scripts/preflight (CI-mirror, per-repo) | proposed |
| L4 | **Verifiers driving mutating verbs MUST use isolated temp roots — never the real meta-root.** Isolation is the primary guard; verb-level `--help` safety is the backstop, not the reverse. | kernel-verifier ran `hf sync --help` against the REAL fleet ledger (20→427 events) — partly the --help bug (L2), partly insufficient isolation. Verifier later correctly used /tmp meta-roots for 0032. | 1 | kernel-verify skill — add an explicit "isolate the root before driving any mutating verb" criterion | proposed |
| L5 | **The loop handled concurrency well once ledger serialization (0028) landed.** Concurrent sessions writing the kernel ledger are safe with `BEGIN IMMEDIATE`. (Positive pattern — keep.) | A separate session worked HFTASK-0004 + authored a 28K ADR-0001 concurrently; 0028's serialization made the parallel ledger writes safe. | 1 | none (note the pattern; it works) | noted |
| L6 | **Stacked PRs: squash-merging a base branch deletes it and orphans the stack.** Prefer branching off master after the parent merges, or expect to cherry-pick onto fresh master. | Squash-merging a base branch deleted it, orphaning the stacked PR; had to cherry-pick onto fresh master. | 1 | session-relay / loop ship guidance — note the stacked-PR hazard | noted |

### Recurrence watch (act on next occurrence)
- **L1 / search-before-act** (this run: synthesis fan-out; prior: prompt_hub thin-seed copy) — if a
  third instance appears, the canon-search gate moves from *proposed* to *applied*.

---

## 2026-06-21 — Seeded-card hardening retro (HAND OFF; lightweight)

Run evaluated: PR #102 (`1b0503c`, scoped test_commands for HFTASK-0058/0059/0060) + PR #103
(`6b91ed1`, `hf test` fails closed on zero tests) + the surfaced-not-fixed orphaned RVF lock wedge.
**Headline:** every defect this run is one anti-pattern wearing different masks — **FAIL-OPEN**: a
guard/loader/evidence-check that, when its input is missing/empty/unrecognized, proceeds as if
satisfied instead of stopping. That inverts the kernel's founding promise (witnessed + fail-closed).
Full retro + routed upgrades + 5th target: `_workspace/10_evolution.md`.

| # | Lesson (class) | Evidence | Recurrence | Routed-to | Status |
|---|----------------|----------|------------|-----------|--------|
| L7 | **FAIL-OPEN class.** A guard/loader/evidence-check that can't confirm its precondition must STOP, not proceed. Banned in continuity-gating paths: silent `if let Ok(_)`/`.ok()?` on cards, `unwrap_or_default()` on ledger reads feeding status, "exit 0 ⇒ pass", "retry then quietly give up". | `load_tasks` swallowed card #95 (missing `intent_lock`) → invisible from `hf status` a whole session (`hf/src/main.rs:90-96`); `hf test` exit-0 rubber-stamp (PR #103); orphaned `.rvf.lock` wedge. Also-audited fail-open sites: `current_statuses()` `unwrap_or_default` (`main.rs:131-135`), `load_task_in().ok()?` (`main.rs:124-128`). | 1 NEW class, **≥3 distinct instances in one run ⇒ escalate now** (Phase 7-4) | AGENTS.md **fail-closed law** (U4) + `scripts/fail-open-audit.sh` advisory lint (U5) | **applied** (U4+U5 on `chore/fail-closed-doctrine`; kernel loud-load HFTASK-0057 PR #107 + doctor sweep HFTASK-0064 PR #108 shipped) |
| L8 | **Every witnessed PASS needs POSITIVE evidence.** A gate that passed because nothing failed (exit 0, empty result set, `None`/degraded runner, zero rows) is not evidence — require the count/artifact proving the criterion was exercised. | `cargo test <filter>` matching nothing exited 0 → witnessed PASS; PR #103 `parse_tests_ran` (Some(0)=FAIL, None=degrade-with-note). | 1 | kernel-verifier agent + kernel-verify skill: **assert tests-ran > 0** (U1); code-omniscient-gatekeeper + gatekeeper-review skill: reject absence-as-pass, surface degrade-notes (U3) | **applied** (U1+U3 on `chore/fail-closed-doctrine`; backed by `hf test`/`parse_tests_ran` PR #103/#106) |
| L9 | **A card that fails to load is a P0 surfacing, never a silent skip.** `hf status` must reflect every card on disk or fail loudly; drift can't flag what `load_tasks` already dropped. | #95 invisible until test_commands were hand-audited; delayed detection a full session. | 1 (instance of L7, tracked separately for its doctrine routing) | handoff-loop state-precedence doctrine + continuity-navigator + drift-reconcile (U2-doctrine); loud `load_tasks` code fix (U2-code = HFTASK-0057) | **applied** (U2-doctrine on `chore/fail-closed-doctrine`; U2-code shipped HFTASK-0057 PR #107) |
| L10 | **Liveness gap: orphaned-lock wedge.** A stale lock from a provably-dead holder wedges every `hf` call past the retry cap. Raising the cap is a fail-open band-aid (longer wait, same wedge); provably-dead reclaim is the fix. | persistent `.handoff/ledger.db.rvf.lock` returns RVF 0x0300 LockHeld past the HFTASK-0060 retry cap; manual `rm` required. | 1 | **5th target**: `hf doctor` fail-closed invariant sweep + stale-lock self-heal | **applied** (shipped: RVF reclaim HFTASK-0062 + `hf doctor` sweep HFTASK-0064 PR #108) |

### Recurrence watch (act on next occurrence)
- **FAIL-OPEN (L7)** already escalated this run (≥3 instances). If a 4th surfaces post-fix, the
  `fail-open-audit.sh` lint should be promoted from advisory to a CI-gating check.
- **L8 / absence-as-pass** — if any other gate (drift, fleet status, hook severity) is found
  accepting an empty/absent result as pass, the gatekeeper checklist item (U3) moves from skill-text
  to a structural pre-verdict assertion.

---

Run evaluated: `scripts/fail-open-audit.sh` full remediation pass (the U5 advisory lint, actioned
per its own footer: "Audit each against AGENTS.md 'Fail-closed law'"). The lint listed **115
candidate sites**; each was read at its call site and judged on-gating-path vs benign.

| # | Lesson (class) | Evidence | Recurrence | Routed-to | Status |
|---|----------------|----------|------------|-----------|--------|
| L11 | **Advisory fail-open lints must be actioned, not just shipped.** The U5 lint surfaced 115 candidates; 3 were REAL continuity-gating fail-opens still live after the U1–U5 doctrine pass, incl. one (`current_statuses()`) the doctrine had already *named* but left code-unfixed. The other 112 are benign (fail-*closed* direction, render fallbacks, correct run-helper success mapping, comments/seed-text) and stay as-is — over-fixing benign sites would be churn. | **R1** `fleet.rs::load_member_tasks` silently dropped unparseable member cards (the #95 class on the fleet path, never fixed by HFTASK-0057 which only covered kernel cards) → now reuses the loud `parse_card_file`. **R2** `current_statuses()` `unwrap_or_default` reported empty on a present-but-unreadable ledger to 20 callers incl. `claim`/`next_safe` → now distinguishes absent (quiet) vs present-failed (loud WARNING). **R3** session lifecycle + `cmd_release` un-claim used `if let Ok(mut led)=Ledger::open(){let _ = led.append()}`, silently losing the witness → now `witness_lifecycle()`/`open_ledger_or_exit` surface a lost witness loudly. | 3 instances (all L7 class), 0 new classes | code fixes only (no new doctrine needed — U4 law already covers them); verified live (corrupt-present ledger warns + absent quiet; broken member card warns + render still completes) | **applied** (this PR) |

### Recurrence watch (act on next occurrence)
- The lint stays **advisory**: post-remediation it should report only benign candidates. If a future
  run finds a NEW real fail-open the lint already listed (i.e. a candidate that became gating), that is
  the 4th-instance trigger to promote `fail-open-audit.sh` from advisory to a CI-gating check (L7 watch).
