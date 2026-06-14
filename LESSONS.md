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
