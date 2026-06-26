# Re-map + HFTASK-0087 (`hf fleet sync`) — decision-grade report

**Date:** 2026-06-26 · **Target:** `/home/drdave/Desktop/meta/handoff`
**Sources:** `codemap-delta-2026-06-26.md`, `DR1-crate-decomposition-state.md`,
`DR2-fleet-sync-blast-radius.md`, `verdicts-DR1-DR2.md` (verifier pass 2026-06-26).
**Evidence policy:** built only from CONFIRMED / QUALIFIED claims; each line traces to `file:line`.

---

## VERDICT (lead)

1. **The `hf` monolith decomposition is behavior-preserving (QUALIFIED-YES) and the workspace is
   green.** `hf` went from an 8-member workspace + ~10 in-tree `mod`s to a **21-member workspace**
   with **13 new `handoff-*` library crates**; the move is a **pure path-alias rename** (no shim
   files, no wrapper functions), `cargo check --workspace` exits 0, and the target crate
   `handoff-fleet` is **CI-gate clean at baseline** (build + clippy `--all-targets -D warnings` +
   test all exit 0, **5 tests ran**). *Confidence: HIGH.*

2. **HFTASK-0087 (`hf fleet sync`) is implementable exactly as specified, with one load-bearing
   refinement.** All four structural preconditions are confirmed on real source; **no Cargo change is
   needed**; the impacted files are just `handoff-fleet/src/lib.rs` + `hf/src/main.rs`. The single
   non-obvious constraint: **`handoff-loop-init.sh` cannot report per-member failure via its exit
   code** (it ends in unconditional `exit 0`), so the verb MUST judge remediation by **re-running
   `collect_rows` after** each member and computing its own exit code from the after-state.
   *Confidence: HIGH on structure; MEDIUM on two settled-by-design choices (ledger-absent witness
   policy; unresolved-but-no-error = failure).*

3. **One housekeeping finding (not a blocker):** the git-kb code index is **stale after the crate
   move** — cross-crate call edges are unresolved and a phantom edge to the **deleted**
   `hf/src/fleet.rs:290` still appears. Re-run `git kb code index` against the new crate paths before
   trusting the call graph for fleet work.

---

## 1. Re-map delta — what changed and why it's safe

**8 → 21 workspace members; 13 new `handoff-*` crates.** `Cargo.toml:3` now lists 21 members (was 8):
the 13 new `handoff-*` crates + `work-order`, `ledger`, `hf`, and the 5 unchanged `crates/*`
rusty-idd members (not on the continuity path). `hf` is now a thin CLI shell that re-aliases the
extracted crates (`codemap-delta §1-2`; `DR1 Claim 4.1`).

**The aliases are pure renames — behavior cannot change.** The entire decomposition mechanism is a
block of crate-root path aliases in `hf/src/main.rs`; there are **no shim files and no function
bodies** (`DR1 Claim 1.1`, HIGH):
- `use handoff_fleet as fleet;` — `hf/src/main.rs:44`
- `use handoff_drift as gates;` — `:46` (NEW crate mapped onto the OLD `gates::` name so call sites
  need no edit)
- `use handoff_hooks as hooks;` `:40`, `use handoff_schema as schema;` `:34`,
  `use handoff_lease as lease;` `:37`, `use handoff_index as index;` `:42`,
  `use handoff_route as route;` `:48`, `use handoff_gatekeeper as gatekeeper;` `:51`,
  `use handoff_intake as intake;` `:54`
- `pub(crate) use handoff_policy::{branch, policy};` `:31`
- `pub(crate) use handoff_core::{HF, … must_witness, … pretty_json, … status_of, tasks_dir};` `:64-68`

A call like `fleet::cmd_fleet_status(...)` resolves directly to `handoff_fleet::cmd_fleet_status`
with identical signature/semantics; the old in-tree `hf/src/{fleet,gates,hooks,…}.rs` files are
**deleted**, not shimmed (`DR1 Claim 1.1-1.2`). The dependency graph is a clean acyclic DAG with
`handoff-core` as the hub (`DR1 Claim 3.2`, every `handoff-*/Cargo.toml` read).

**Baseline is green (verifier ran the gates):**

| Gate | Command | Exit | Result |
|------|---------|------|--------|
| Build | `cargo build -p handoff-fleet` | 0 | clean (62 crates) |
| Clippy | `cargo clippy -p handoff-fleet --all-targets -- -D warnings` | 0 | "No issues found" |
| Test | `cargo test -p handoff-fleet` | 0 | **5 passed, 0 failed** (tests-ran = 5 > 0) |

(`verdicts §"DR1 open caveat — CLOSED"`.) Workspace-wide, `cargo check --workspace` = exit 0; the one
warning is an `unused_imports` in an unrelated `vault`/secrets path-dep, not in any `handoff-*` crate
(`DR1 Claim 4.1`). Every `handoff-*` crate **and** `hf` declares `[lints] workspace = true`, so the
CI clippy policy (denies `unsafe_code`, clippy `all`, and the HFTASK-0080 `unwrap_used`/`expect_used`/
`panic` trio) is uniformly inherited — **new fleet-sync production code may not use bare
`.unwrap()`/`.expect()`/`panic!`** (test code exempted via `#![cfg_attr(test, allow(...))]`)
(`DR1 Claim 4.2`).

**Index-degradation note (housekeeping).** `git kb code symbols handoff-fleet/src/lib.rs` →
"No symbols found"; `git kb code callers cmd_fleet_status` returns a **STALE** entry for the deleted
`hf/src/fleet.rs:290` alongside the real `handoff-fleet/src/lib.rs:298` marked "(no callers)" — the
cross-crate edge is unresolved post-move (`codemap-delta §3 index-degradation note`). **Action:**
re-run `git kb code index` against new crate paths; all call sites in this report were confirmed by
direct grep/read, not the call graph.

---

## 2. Fleet surface map (the change's blast surface)

All in `handoff-fleet/src/lib.rs` unless noted; `hf fleet` dispatch is in `hf/src/main.rs`.

**Public API (what `hf` calls):**
| Symbol | Loc | Role |
|---|---|---|
| `find_meta_root` | `:37` | `pub fn -> Option<PathBuf>` — walk up for `.meta.yaml` (= meta root) |
| `parse_members` | `:53` | `pub fn(&str) -> Vec<String>` — `projects:` 2-space-key parse |
| `cmd_fleet_status` | `:298` | `pub fn(json: bool)` — the status board entry, **sole public hook** for the read path |
| `render_member_packet` | `:523` | render `<member>/.handoff/packets/latest.md` |

**Call graph (confirmed by direct read, `codemap-delta §3` + `DR2 Q1`):**
```
hf::main dispatch (hf/src/main.rs:3797 "fleet"/"status") ──► fleet::cmd_fleet_status (:298)   [single caller :3798]
  cmd_fleet_status ──► find_meta_root (:299→:37)
                   ──► parse_members  (:304→:53)
                   ──► collect_rows   (:305→:228)        [returns Vec<Row>; single caller today]
                   ──► fleet_ledger_stats (:306→:268)
                   ──► fleet_provenance   (:309→:287)
                   ──► handoff_core::pretty_json (:397)
  collect_rows (:228) ──► git_tracks_handoff_db (:238→:156)
                      ──► local_ledger_on_disk + git_tracks_jsonl_export (:240-243→:189,:172)
                      ──► ledger_guard_present  (:244→:198)
                      ──► walshm_guard_present  (:245-248→:216)
                      ──► per_repo_chain_stats  (:261→:135)
```

**`Row` and ALL flag fields are private** (`DR2 Claim 1.1`, HIGH — read directly):
| Field | Loc | Meaning |
|---|---|---|
| `struct Row` | `:95` | no `pub` |
| `jsonl_export_missing` | `:106` | local ledger on disk but `ledger.events.jsonl` not git-tracked (ADR-0018 D1 primary P7 gate) |
| `tracked_ledger` | `:110` | a git-TRACKED binary `.db` under `.handoff` (banned) |
| `ledger_guard_missing` | `:113` | `.gitignore` lacks `.handoff/**/ledger.db` guard |
| `walshm_guard_missing` | `:116` | `.gitignore` lacks `*.db-wal`/`*.db-shm` guard |
| `per_repo_chain` | `:123` | `Option<PerRepoChain>` standalone witness verify |

Because Rust private items are visible throughout the defining module, a **new `pub fn cmd_fleet_sync`
in the same `lib.rs` reads `r.jsonl_export_missing` etc. and calls `collect_rows` with zero
visibility changes** (`DR2 Claim 1.2`).

---

## 3. HFTASK-0087 blast radius (the priority)

**Impacted files — both within `path_scope: ["spike/**","handoff/**"]` (`HFTASK-0087.task.json:8-11`):**

- **`handoff-fleet/src/lib.rs`** — NEW `pub fn cmd_fleet_sync(json, dry_run)` + private `flagged(&Row)`
  selector + a `handoff.fleet_sync.v1` JSON/text renderer + the `must_witness`/`Ledger::append`
  witness. Reuses `collect_rows` (`:228`), `find_meta_root` (`:37`), `parse_members` (`:53`),
  `fleet_ledger_stats` (`:268`). (`DR2 Blast-radius summary`.)
- **`hf/src/main.rs`** — NEW dispatch arm `Some("fleet") if args.get(1)=="sync"` (+ a `status --fix`
  alias) beside the existing `:3797-3822`; usage-string update. The `fleet::…` alias (`:44`) already
  routes; the HFTASK-0087 `test_commands` are already seeded in `cmd_seed` (`:3100-3111`).

**NO Cargo changes.** `handoff-fleet/Cargo.toml:9-11` already deps `handoff-core` + `ledger` +
`work-order` + `serde_json`; witnessing and spawning need no new crate (`DR2 Blast-radius summary`,
contradicting the codemap's "possibly" note). **Avoiding new deps is also required**: `handoff-fleet`
is **not a sink** — `handoff-route` deps it and `handoff-gatekeeper` deps it transitively, so any new
dep propagates into both at compile time (`DR1 Claim 3.3 / 5.3`).

**Baseline `handoff-fleet` is GREEN** (build / clippy `--all-targets` / 5-pass test all exit 0 —
§1 table), so any post-change regression is attributable to this change alone.

---

## 4. The four confirmed design facts

**(a) `Row`/flags are readable in-crate — do NOT pub-ify anything.** Put `cmd_fleet_sync` in
`handoff-fleet/src/lib.rs`; the only new public symbol `hf` needs is that entrypoint, mirroring how
`cmd_fleet_status` (`:298`) is the lone public hook for the read path. The selector is a private
helper. (`DR2 Q1 CONFIRMED`; `verdicts DR2-Q1 CONFIRMED`.)

**(b) `handoff-loop-init.sh` takes a single member-dir positional and deploys per-member.**
Arg loop `for a in "$@"` (`:64-76`): any non-`--` token → `TARGETS+=("$a")` (`:74`); `--fleet` is
opt-in (`:66`); unknown `--*` → `exit 2` (`:73`). If `TARGETS` is empty it defaults to
`git rev-parse --show-toplevel` (`:144-147`). Main loop `for dir in "${TARGETS[@]}"` (`:293`) runs
per-target: `ensure_ledger_guard` (`:314`), `deploy_hooks` (`:340`), `deploy_diff_drive` (`:344`),
`deploy_session_relay` (`:347`), `deploy_rules` (`:350`), then `hf resume`/`hf drift` (`:354-355`).
So `handoff-loop-init.sh <meta_root>/<member>` remediates exactly that one member — **`--fleet` is
not required and would over-reach** (`verdicts DR2-Q2 CONFIRMED`; `DR2 Claim 2.2-2.3`).

**(c) `handoff-fleet` MUST NOT depend on `hf` (cycle).** `hf/Cargo.toml:33` deps `handoff-fleet`;
`hf/src/main.rs:44` aliases it — a back-edge would be a build cycle. All helpers come from
`handoff-core` (which is exactly why `must_witness`/`pretty_json`/`run_out`/`ledger_path` were lifted
there; module doc `handoff-core/src/lib.rs:117-118`: feature crates witness "without depending back
on the `hf` binary crate"). (`verdicts DR1-constraint CONFIRMED`; `DR1 Claim 5.1`.)

**(d) Witness target = the FLEET ledger at `<meta_root>/.handoff/ledger.db`.** `find_meta_root`
returns the `.meta.yaml` dir = meta root (`:37-47`); `fleet_ledger_stats` resolves
`root.join(".handoff").join("ledger.db")` (`:269`). `must_witness` is `pub` and fail-closed
(`handoff-core/src/lib.rs:119`, `exit(1)` on failure); `Ledger::append(event_type, work_order_id,
payload, ts_ns)` is reachable via the existing `ledger` dep (`ledger/src/v1.rs:504`). No new
dependency. (`verdicts DR2-Q4 CONFIRMED`; `DR2 Claim 4.2-4.3`.)

---

## 5. THE load-bearing constraint (from the verifier — do not miss this)

`scripts/handoff-loop-init.sh` ends with an **unconditional `exit 0` (`:374`)**, and a per-member
deploy failure inside the target loop does **`FAIL=$((FAIL+1)); continue` WITHOUT changing the
process exit code** (`verdicts §Refutations`, QUALIFIED). Non-zero exits (`exit 2`) occur **only**
for *pre-loop* fail-closed conditions: unknown flag (`:73`), NEEDS-HUMAN no-kernel (`:113`). The
script even silently `continue`s past a non-git-repo target (`:294-295`).

**Consequence:** a Rust caller spawning the script per-member will **almost always see
`script_exit==0` regardless of internal failure.** Therefore:

> `cmd_fleet_sync` MUST judge each member's remediation by **re-running `collect_rows` AFTER** the
> spawn (the `after` flags + a `resolved` boolean), and **compute the verb's exit code from the
> after-state — NEVER inherit the script's exit code.** A still-flagged `after` row is a FAILURE
> even when `script_exit==0`.

Use `script_exit` only to detect the *pre-loop* fail-closed cases (script unreachable / NEEDS-HUMAN).
This strengthens (does not contradict) DR2's Risk 3 — the script cannot be trusted to report failure
via exit code, so the `after` re-check is the **only** reliable remediation signal. `--dry-run` is
safe: the script's `DRY=1` path echoes instead of executing the deploy / `cargo install` steps
(`:79,116,304,313,327`), so passing `--dry-run` through mutates nothing
(`verdicts §Note`; `DR2 Q3 DESIGN CONSEQUENCE`).

---

## 6. Implementation spec (build-ready)

### Signature & placement
```rust
// handoff-fleet/src/lib.rs  (same module as Row/collect_rows — no visibility changes)
pub fn cmd_fleet_sync(json: bool, dry_run: bool) -> i32   // returns process exit code

fn flagged(r: &Row) -> bool {
    r.jsonl_export_missing || r.tracked_ledger || r.ledger_guard_missing || r.walshm_guard_missing
}
```
Mirror the existing text-board `match` (`:459-470`) and warning filters (`:317-340`) for which flags
count. Keep `flagged` pure over `&Row` so the selection logic is unit-testable without spawning.

### Flow
1. `root = find_meta_root()` → if `None`, fail-closed (non-zero, explicit error). Script path =
   `root.join("handoff/scripts/handoff-loop-init.sh")`, honoring `HANDOFF_KERNEL_HOME` first
   (ejected-kernel case). **Existence-check before any spawn** — missing script ⇒ loud error +
   non-zero, **never a silent no-op** (`DR2 Risk 3`).
2. `members = parse_members(&read(root/.meta.yaml))`; `before = collect_rows(&root, &members)`.
3. **Flagged set** = members where `flagged(&row)`. If empty ⇒ clean fleet ⇒ emit empty report,
   **exit 0** (idempotence).
4. **Per flagged member** (sweep ALL; one member's failure never aborts the rest — collect into
   `failures[]`, per `HFTASK-0087.task.json` + `codemap-delta §4`):
   - spawn `handoff-loop-init.sh <root>/<member>` (append `--dry-run` when `dry_run`), via
     `std::process::Command` (already imported `:32`; the crate shells `git` read-only 6× today —
     `DR2 Claim 3.1`). **Capture child stdout/stderr** (do not inherit-and-detach — the report needs
     it; `DR2 Q3`).
   - record `script_exit` (used only for the pre-loop fail-closed cases).
   - **re-collect that member's Row** → `after` flags + `resolved = !flagged(after)` (THE §5
     constraint — never trust `script_exit==0`).
   - witness one `fleet_sync` event into the FLEET ledger via `handoff_core::must_witness` +
     `ledger::Ledger::append` **when the FLEET ledger is present**; **when absent, emit an explicit
     "ledger absent — NOT witnessed" line (loud degrade, fail-closed) — never a silent success**
     (`DR2 Risk 1`). `--dry-run` witnesses nothing.
5. **Exit code** computed from after-state: **0 iff every flagged member `resolved` (or none
   flagged); non-zero if any member is unresolved or errored** (`DR2 Q6 exit contract`).

### JSON shape (`handoff.fleet_sync.v1`, mirrors `handoff.fleet_status.v1`, printed via `handoff_core::pretty_json`)
```json
{
  "schema": "handoff.fleet_sync.v1",
  "meta_root": "<root>",
  "dry_run": true,
  "members": [
    { "name": "memberx",
      "flags_before": { "jsonl_export_missing": true, "tracked_ledger": false,
                        "ledger_guard_missing": true, "walshm_guard_missing": false },
      "action": "handoff-loop-init.sh <root>/memberx [--dry-run]",
      "script_exit": 0,
      "flags_after":  { "jsonl_export_missing": false, "tracked_ledger": false,
                        "ledger_guard_missing": false, "walshm_guard_missing": false },
      "resolved": true }
  ],
  "failures": [ { "name": "membery", "error": "script not found | unresolved after sync", "script_exit": 2 } ],
  "all_resolved": false
}
```

### Tests (card `test_commands` = `cargo test`, `HFTASK-0087.task.json:15` — unit-level satisfies acceptance)
Reuse the existing per-test isolated-repo idiom (`std::env::temp_dir().join(format!("hf-…-{pid}-{nanos}"))`
+ a local `git` closure; there is **no shared `mk_temp_repo()` helper** — copy-paste per test, per
`DR2 Claim 5.2`, e.g. `:727`/`:805`-style setup):
1. **Flagged-set selection** — temp meta-root with N members, some conformant / some flagged;
   assert `collect_rows` + `flagged()` picks exactly the non-conformant set.
2. **Fail-closed** — a member that stays flagged after sync ⇒ `resolved=false` ⇒ verb exit non-zero
   (proves §5: not gated on `script_exit`).
3. **Dry-run no-op** — `cmd_fleet_sync(json, dry_run=true)`; re-`collect_rows` unchanged (mutates
   nothing).
4. **Idempotence** — clean fleet ⇒ empty flagged set ⇒ empty report, exit 0.

The live shell-out (real script needs `HANDOFF_KERNEL_HOME`/the `hf` binary + writes files) is an
**integration test**, guarded/`#[ignore]` or driven by the differential-drive harness — NOT a unit
test (`DR2 Claim 5.3`).

---

## 7. Confidence + named gaps

**Overall confidence: HIGH** that the change is implementable exactly as specified, on a green
baseline, with no Cargo change. Every structural precondition (private-field access, single-member
script invocation, no-`hf`-dep cycle, FLEET-ledger witness target) is **CONFIRMED by the verifier on
direct source**, and the §5 exit-code constraint is a verified QUALIFICATION, not speculation.

**MEDIUM-confidence design choices (settle in the card, don't free-hand):**
- **Ledger-absent witness policy** — degrade-with-evidence (witness only when present, loud "NOT
  witnessed" line otherwise) vs. hard-fail when the FLEET ledger is absent. Recommend degrade-with-
  evidence; both honor fail-closed doctrine (`DR2 Risk 1 / Q4 DESIGN CONSEQUENCE`).
- **Unresolved-but-no-error = failure?** Recommend **yes** (detection→remediation is the card's
  contract) — drives the exit-code semantics in §6 (`DR2 Q6`).
- **Script-location override precedence** (`HANDOFF_KERNEL_HOME` first vs. `<meta_root>/handoff/...`
  path-join) depends on the ejected-harness story — confirmed MEDIUM by the verifier; existence-check
  before spawn makes either safe (`DR2 Q2 / Risk 3`).

**Gaps not closed by this pass:**
1. **Stale git-kb index** — re-run `git kb code index` against new crate paths; the phantom
   `hf/src/fleet.rs:290` edge will then clear. Until then, do not trust the call graph for fleet work.
2. **Full-workspace CI gate not re-run here** — the verifier ran `-p handoff-fleet` (green) and
   `cargo check --workspace` (exit 0), but **not** `cargo clippy --workspace --all-targets -- -D
   warnings` + `cargo test --workspace` for the whole tree. Run that exact kernel CI gate before
   shipping (the `--all-targets` test-code lint is the PR #30 trap).
3. **`handoff-fleet` becomes a member-repo MUTATOR** (first time a `handoff-*` library crate deploys
   into / `cargo install`s for arbitrary members, vs. the `hf` bin). Mitigated by dry-run-safe
   defaults, single-member spawns (no blanket `--fleet` from inside `hf`), captured child output, and
   per-member fail-closed — but it is a genuinely new behavioral class for the crate worth a
   gatekeeper note (`DR1 Claim 5.2`; `DR2 Risk 2`).
4. **Live shell-out is unverified by unit tests** — needs the guarded integration test / differential-
   drive harness to prove the real deploy actually flips an `after` flag.

---

*All claims above are CONFIRMED or QUALIFIED in `verdicts-DR1-DR2.md`. No claim used here was
REFUTED or INCONCLUSIVE; no material claim was refuted in the verifier pass.*
