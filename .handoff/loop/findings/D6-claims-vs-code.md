# D6 · claims-vs-code — skeptical doc↔code reconciliation (`hf` Continuity Ledger Kernel)

Scope: reconcile the project's OWN claims (PRD, NORTH-STAR, ADRs, INTEGRATION docs, CLAUDE.md
change-history, AGENTS.md) against the actual code. Each row is a falsifiable statement with
`file:line` and a confidence. Target root: `/home/drdave/Desktop/meta/handoff`.

Verdict up front: the **shipped verbs are real and wired** (no documented verb is silently
half-implemented in the live CLI usage string), BUT the PRD is an **aspirational spec that the
code only partially realizes**. Three classes of overclaim exist: (a) PRD-documented verbs/maps
that were never built (`hf index`, `hf plan`, the `.handoff/maps/*` artifacts), (b) the PRD's
whole crate/architecture/lint contract that the code does not follow, and (c) the
rusty-idd (`crates/*`) integration documented in `INTEGRATION-RUSTY-IDD.md` that is unbuilt — the
crates are orphaned workspace members with zero wiring into `hf`. The drift sentinel is the
subtlest case: it runs **10 checks but a different 10** than PRD §12.3 documents.

Legend: **HIGH** = read both the claim text and the implementing/absent code directly.

---

## A. The drift sentinel — "10 checks" is a count match, not a content match (the headline suspect)

PRD §12.3 (`docs/Continuity_Ledger_Kernel_PRD.md:461-476`) enumerates **10** named drift-sentinel
checks. `gates::detect()` (`hf/src/gates.rs:165-342`) is the implementation. Mapping each:

| PRD §12.3 check | Implemented? | Evidence |
|---|---|---|
| 1. Is current task still active? | **NO** — no drift item emitted for "claimed task no longer active"; `in_progress` is only a *filter* for other checks, never itself a flag | `gates.rs:168-173,287,310` |
| 2. Objective hash changed? | YES | `gates.rs:197-204` (`!c.objective`) |
| 3. Path scope changed? | YES | `gates.rs:205-212` |
| 4. Acceptance changed? | YES | `gates.rs:213-220` |
| 5. Repo constraints changed? | YES (constraint surface) | `gates.rs:221-230` |
| 6. Edit outside path scope? | YES | `gates.rs:257-283` |
| 7. Tests map to acceptance? | YES (acceptance↔test gap) | `gates.rs:287-297` |
| 8. New work contradicts a decision record? | **NO** — there is no contradiction check; the only nearby logic is the *undocumented*-decision advisory (PRD check 9), not contradiction | absent; closest is `gates.rs:324-339` |
| 9. Undocumented architecture changes? | **PARTIAL** — implemented but **advisory / non-blocking** (`undocumented_decisions` is excluded from `clean()`), whereas the PRD frames the sentinel as gating | `gates.rs:140-141,160-163,324-339` |
| 10. Handoff state updated after material changes? | **NO** — no check that `.handoff` views/packet were refreshed after edits | absent |

**CLAIM-A1** (HIGH): The drift sentinel does **not** implement PRD §12.3 checks #1 (task-active),
#8 (contradicts-a-decision-record), or #10 (handoff-state-updated). `gates.rs:165-342` contains no
code for any of the three. → overclaim relative to PRD §12.3.

**CLAIM-A2** (HIGH): The sentinel instead adds **three checks the PRD §12.3 list does not contain**:
northstar-revision drift (`gates.rs:231-240`), dependency-unsatisfied (`gates.rs:308-322`), and
missing-test-evidence (`gates.rs:298-306`). So the "10 checks" headline (CLAUDE.md history,
HFTASK-0046/0047 "8→10 checks") is a *count* coincidence, not the documented set. The substitution
is defensible (northstar/dependency are stronger continuity guards) but it is **undocumented drift
between PRD §12.3 and the code** — the PRD was never amended.

**CLAIM-A3** (MEDIUM): PRD §12.5 lists "stale packet contradicts Git/ledger" and "parallel write
lease overlap" as **hard fails**. Neither is enforced *inside* `hf drift`; they live in other
surfaces (state-precedence/`reconcile`, and the in-ledger lease CAS at claim). So `hf drift` alone
does not discharge the full §12.5 hard-fail list. Evidence of absence: `gates.rs:165-342` has no
packet-vs-git or lease-overlap branch; lease overlap is handled at claim (`hf/src/main.rs:509`,
`ledger try_acquire_lease`). Confidence MEDIUM because the *system* covers them elsewhere; only the
single-verb claim is the overclaim.

---

## B. PRD §9 Command Contract — two documented verbs were never built

PRD §9 (`docs/Continuity_Ledger_Kernel_PRD.md:318-338`) is a 17-row command contract. Compared
against the live dispatch (`hf/src/main.rs:3219-3572`) and usage string (`main.rs:3570`):

**CLAIM-B1** (HIGH): **`hf index`** (PRD `:323` "Generate maps and navigation docs") is **unbuilt**.
`grep 'Some("index")|cmd_index'` over `hf/src/main.rs` → 0 matches. The dispatch has no `index`
arm and the usage string omits it.

**CLAIM-B2** (HIGH): **`hf plan`** (PRD `:327` "Create or refresh task DAG") is **unbuilt**. 0
matches for `Some("plan")|cmd_plan`; absent from dispatch and usage. (The DAG it would build is
instead computed on the fly by `routing::next_safe`, `hf/src/routing.rs` — so the capability
half-exists but the documented verb/refresh-artifact does not.)

**CLAIM-B3** (HIGH): The `.handoff/maps/{repo-map,test-map,owner-map,dependency-map}.json`
artifacts the PRD layout promises (`docs/...PRD.md:289-293`) **do not exist** — `ls .handoff/maps`
→ "No such file or directory". Consistent with `hf index` (their generator) being unbuilt.

**CLAIM-B4** (MEDIUM): **`hf start`** (PRD `:330` "Create branch and worktree for active claim") is
not a top-level verb (0 matches for `Some("start")`); the capability was **relocated** to
`hf session start` (`main.rs:3434` → `session::cmd_session`). Documented-as-`hf start`,
shipped-as-`hf session start` — a naming drift, not a missing capability.

**CLAIM-B5** (MEDIUM): **`hf mcp serve`** (PRD `:338`) is not a `hf` subcommand; MCP is a
**separate `hf-mcp` binary** (`hf/src/bin/hf-mcp.rs`; `grep 'mcp serve'` → 0). Capability present,
invocation contract differs from the PRD.

Counter-balance (not an overclaim): every verb in the live usage string `main.rs:3570` has a real
dispatch arm and handler — I found **no** verb that is advertised in the CLI but unwired. The
overclaims are confined to the PRD, which advertises more than the CLI does.

---

## C. PRD §7.2 / §7.3 / §8 architecture & lint contract — not followed

**CLAIM-C1** (HIGH): PRD §7.2 (`PRD.md:204-238`) specifies a **12-crate** workspace
(`crates/handoff-core … handoff-drift, handoff-test, handoff-mcp, xtask`), `resolver = "3"`,
`edition = "2024"`, `rust-version = "1.96"`. Actual root `Cargo.toml`: `resolver = "2"`,
`edition = "2021"`, and members `["work-order","ledger","hf","crates/cli","crates/core",
"crates/runner","crates/spec","crates/tui"]`. **None** of the 12 `handoff-*` crates exist; the
functional decomposition was collapsed into `work-order`/`ledger`/`hf`. → the §7.2/§8 layout
contract is aspirational, not realized.

**CLAIM-C2** (HIGH): PRD §7.2 mandates `[workspace.lints]` with `unsafe_code = "forbid"`,
`unwrap_used = "deny"`, `expect_used = "deny"`, `panic = "deny"`, `todo = "deny"`,
`unimplemented = "deny"`. Root `Cargo.toml` has **no `[workspace.lints]` block at all**
(`grep 'lints|unwrap_used|unsafe_code' Cargo.toml` → 0). The code in fact uses `.unwrap()` freely
(e.g. `gates.rs:413,542`), which the documented `unwrap_used = "deny"` policy would reject. → the
lint-discipline contract is unbuilt.

**CLAIM-C3** (HIGH, doc-superseded, NOT an overclaim): PRD §7.3/§11.3 recommend
`rusqlite`/`sqlx` SQLite + WAL for the ledger. The ledger is now pure-Rust **redb**
(`ledger/Cargo.toml:6,14,33`). This is a *deliberate, documented* supersession by **ADR-0017**, so
it is correct doc-vs-code evolution, not drift — flagged only so the synthesizer doesn't double-count
the PRD's SQLite language as "current."

---

## D. rusty-idd (`crates/*`) — documented integration is unbuilt; crates are orphaned

**CLAIM-D1** (HIGH): `docs/INTEGRATION-RUSTY-IDD.md` documents a "COPY + REFERENCE" plan: copy
rusty-idd into **`crates/intent-analysis/`** (`:5,19-23`), add an **`hf index --intent-aware`**
subcommand that spawns rusty-idd as a subprocess and merges JSON into `capsule.json`
(`:25-38`), and asserts "existing **hf index** works unchanged" (`:53`). **Every load-bearing
element is unbuilt**: (i) the crate is named `crates/{cli,core,runner,spec,tui}`, **not**
`intent-analysis` (`ls crates/`); (ii) `hf index` does not exist at all (CLAIM-B1), so it cannot
"work unchanged"; (iii) no `--intent-aware` flag and no capsule-merge code exist.

**CLAIM-D2** (HIGH): The rusty-idd crates are **orphaned** from the `hf` continuity path. They are
workspace members but `grep` for `intent-analysis|crates/core|crates/cli|rusty-idd|idd` across
`hf/Cargo.toml`, `ledger/Cargo.toml`, `work-order/Cargo.toml` → **0 matches**, and
`grep 'intent_analysis|rusty_idd|idd::'` across `hf/src/` → **0 matches**. `crates/core/src/lib.rs:1-8`
self-describes as an independent "Intent Driven Development … dependency-light toolkit … avoids
network calls and provider-specific SDKs." So the codemap's claim that `crates/*` is "NOT on the
hf continuity path" is **confirmed**: they compile alongside `hf` but are not invoked by it. The
recent commit "deploy rusty-idd thin-adapter control plane (fleet canary)" imported the code without
wiring it to the kernel. → the documented hf↔rusty-idd integration is a **plan, not a feature**.

---

## E. Smaller doc↔code mismatches (lower stakes, flagged for completeness)

**CLAIM-E1** (MEDIUM): PRD §13 (`PRD.md:535`) specifies the capsule's `next_command` as
`"hf claim --next"`. The portable initializer sets it to `"hf resume"`
(`init_capsule`, asserted at `hf/src/main.rs:3675`). Minor contract drift in the capsule schema's
default value.

**CLAIM-E2** (LOW→MEDIUM): PRD §14 (`PRD.md:549-571`) requires the handoff packet to carry **17
named sections** and a `handoff.packet.v2` machine summary. I did not line-by-line verify
`render_packet_md` emits all 17 (codemap notes it is cited but not fully read,
`codemap.md:184`). Raise to HIGH by reading the packet renderer and diffing its emitted headings
against PRD §14's list — a concrete follow-up for the verifier.

**CLAIM-E3** (LOW): NORTH-STAR invariant "**No trust in docs over observed runtime**"
(`NORTH-STAR.md:54`) is itself an argument for this dimension: the kernel's own doctrine says
runtime beats docs, yet the PRD (a doc) overstates the runtime in classes A–D. Not a code bug — a
self-consistency observation that *supports* trusting the code over the PRD where they disagree.

---

## Overclaim / unbuilt ledger (the actionable list)

| # | Claimed in | Reality | Severity |
|---|---|---|---|
| 1 | PRD §9 — `hf index` | verb absent; `.handoff/maps/*` never generated | overbuilt-claim / **unbuilt** |
| 2 | PRD §9 — `hf plan` | verb absent (DAG computed ad-hoc in `routing.rs`) | **unbuilt** |
| 3 | PRD §12.3 — checks #1/#8/#10 | not implemented in `gates::detect()` | **unbuilt subset** |
| 4 | PRD §12.3 — "the 10 checks" | a *different* 10 implemented (northstar/dep/evidence swapped in) | **doc drift** |
| 5 | PRD §12.3 #9 / §12.5 | undocumented-arch check is advisory, not blocking | partial |
| 6 | PRD §7.2 — 12 `handoff-*` crates, resolver 3, edition 2024 | 3 real crates + rusty-idd; resolver 2; edition 2021 | **unbuilt architecture** |
| 7 | PRD §7.2 — `[workspace.lints]` (unwrap/panic/unsafe deny) | no lints block; `.unwrap()` used in tree | **unbuilt** |
| 8 | `INTEGRATION-RUSTY-IDD.md` — `crates/intent-analysis` + `hf index --intent-aware` + capsule merge | none built; crates orphaned, zero wiring | **unbuilt** |
| 9 | PRD §9 — `hf start`, `hf mcp serve` | shipped as `hf session start`, separate `hf-mcp` binary | naming drift |
| 10 | PRD §13 — capsule `next_command = hf claim --next` | code sets `hf resume` | minor drift |

Not-overclaims (verified consistent / legitimately superseded): every live-usage verb is wired
(no phantom CLI verb); the SQLite→redb change is ADR-0017-sanctioned; the 5-surface intent-lock
drift, lease CAS, witness chain, and contract-proof gate match their ADRs (cross-checked against
codemap §5–§6, not re-derived here).

## Open questions for the verifier

1. Confirm CLAIM-E2 by reading `render_packet_md` and diffing emitted headings vs PRD §14's 17.
2. Confirm there is **no** hidden `index`/`plan` capability reachable via an alias or `xtask`
   (I checked dispatch + usage; a Makefile/script alias would be a softer "exists").
3. Decide the disposition the kernel's own doctrine implies: amend the PRD to match the code
   (preferred, per `NORTH-STAR.md:54`) vs. build the missing §9/§12.3 items.
