# DR1 · crate-decomposition-state — findings

**Question:** Is the `hf`→`handoff-*` crate decomposition (ADR-0019 D5 #4 / HFTASK-0081/0083)
genuinely behavior-preserving, and what coupling/risk remains that an in-flight fleet change
(DR2 / HFTASK-0087 `hf fleet sync`) should know about?

**Verdict: behavior-preserving = QUALIFIED-YES.** The decomposition is a pure path-alias move
(zero behavioral wrappers), the workspace builds clean as one unit, and lint policy is uniformly
inherited. The one nuance: the cartographer's "routing duplication" is **not** a duplication — it
is two disjoint, both-live modules with confusingly similar names. Residual coupling is a clean
DAG with `handoff-core` as the hub; `handoff-fleet` is a low-level node (clean deps) but **not a
sink** — `handoff-route` (and transitively `handoff-gatekeeper`) depend on it, which constrains
the fleet-sync change.

---

## 1. Alias mechanism — pure renames, no behavioral wrappers (HIGH)

**Claim 1.1 — every crate-root alias is a plain Rust `use … as …` / `pub(crate) use …::{…}` path
rename; no shim files, no wrapper functions.** Confirmed by direct read of the alias block:
- `pub(crate) use handoff_policy::{branch, policy};` — `hf/src/main.rs:31`
- `use handoff_schema as schema;` — `:34`
- `use handoff_lease as lease;` — `:37`
- `use handoff_hooks as hooks;` — `:40`
- `use handoff_index as index;` — `:42`
- `use handoff_fleet as fleet;` — `:44`
- `use handoff_drift as gates;` — `:46`
- `use handoff_route as route;` — `:48`
- `use handoff_gatekeeper as gatekeeper;` + `use handoff_gatekeeper::GhPrView;` — `:51-52`
- `use handoff_intake as intake;` — `:54`
- `pub(crate) use handoff_core::{HF, … must_witness, next_safe, now_ns, pretty_json, … status_of, tasks_dir};` — `hf/src/main.rs:64-68`

These are name bindings only — there is **no function body** in the alias block, so a call like
`fleet::cmd_fleet_status(...)` resolves directly to `handoff_fleet::cmd_fleet_status` with
identical signature/semantics. The old in-tree files (`hf/src/{fleet,gates,hooks,schema,…}.rs`)
are deleted (no shim layer). **This is the load-bearing behavior-preservation claim and it holds:
a rename cannot change behavior.** Confidence HIGH (the alias block is the entire mechanism and was
read in full).

**Claim 1.2 — the alias names exactly reconstruct the pre-split `crate::…` paths the feature
modules already used.** e.g. `route::route_for_task` is called 8× at `hf/src/main.rs:861,978,993,
1054,1159,1198,1654,1919`; `fleet::cmd_fleet_status` at `:3798`; `gates::cmd_policy_check` at
`:3795`. All resolve through the aliases unchanged. The drift-audit alias deliberately maps the
NEW crate `handoff_drift` onto the OLD name `gates` (`:46`) precisely so `gates::…` call sites
need no edit. Confidence HIGH.

---

## 2. handoff-core as the shared leaf hub (HIGH)

**Claim 2.1 — the shared continuity primitives were lifted verbatim into `handoff-core` and are
`pub`.** All present in `handoff-core/src/lib.rs`: `HF` (`:19`), `now_ns` (`:23`), `tasks_dir`
(`:31`), `capsule_field` (`:42`), `current_northstar_revision` (`:50`), `ledger_path` (`:58`),
`current_statuses` (`:72`), `status_of` (`:89`), `run_out` (`:99`), `must_witness` (`:119`),
`pretty_json` (`:134`), `parse_card_file` (`:156`), `scan_card_conformance` (`:173`), `load_tasks`
(`:197`), `save_task` (`:216`), `save_task_in` (`:222`), `load_task_in` (`:230`), `next_safe`
(`:240`). `hf` re-exports these `pub(crate)` (`hf/src/main.rs:64-68`) so `crate::HF`,
`crate::ledger_path`, etc. are unchanged. Confidence HIGH.

**Claim 2.2 — `handoff-core` is the dependency hub, not a god-crate; its own deps are minimal.**
`handoff-core/Cargo.toml` deps = `ledger`, `work-order`, `handoff-schema`, `serde`, `serde_json`.
The single `handoff-*`→`handoff-*` edge here is `handoff-core → handoff-schema` (so `load_tasks`
can validate-on-load through the same path), explicitly documented no-cycle (handoff-schema deps
only `work-order` + `jsonschema`). Confidence HIGH.

**Claim 2.3 — `handoff-fleet` depends on `handoff-core` and uses `handoff_core::pretty_json`.**
`handoff-fleet/Cargo.toml:9` = `handoff-core = { path = "../handoff-core" }` (full deps:
handoff-core, ledger, work-order, serde_json). The JSON board output calls
`handoff_core::pretty_json(&out)` at `handoff-fleet/src/lib.rs:397`. Confidence HIGH.

---

## 3. Residual coupling — the "routing duplication" is NOT a duplication (HIGH)

**Claim 3.1 — `hf/src/routing.rs` (`routing::`) and the `handoff-route` crate (`route::`) are two
distinct, both-live modules with disjoint symbol sets. Neither is dead.**
- Local `mod routing` (`hf/src/routing.rs`) = the RuVector Thompson-bandit **task value-picker**:
  `bucket_of` (`:21`), `RoutingDecision` (`:70`), `route_with_history` (`:81`). Imports
  `ruvector_domain_expansion::transfer`. **Live:** called as `routing::bucket_of` /
  `routing::History` / `routing::route_with_history` at `hf/src/main.rs:752,756,758,838`
  (the `hf claim --batch` value-routing path).
- `handoff-route` crate (`route::`) = the ADR-0004 §3 **ledger-residency router**: `local_home`
  (`:39`), `fleet_home` (`:45`), `route_for_task` (`:62`). **Live:** `route::route_for_task` called
  8× at `hf/src/main.rs:861…1919` (per-task ledger/tasks-dir resolution).

The two share only a similar name; their symbols, deps, and purposes do not overlap. **Risk class:
cognitive/naming collision, not code duplication or dead code.** A future reader (or a careless
refactor) could conflate `routing::` and `route::`. Confidence HIGH (read both files; grepped all
call sites).

**Claim 3.2 — the inter-crate dependency graph is a clean acyclic DAG; `handoff-core` is the hub
and the only `handoff-*`→`handoff-*` edges beyond it are few.** From the `[dependencies]` of each
crate manifest:
- Pure leaves (no handoff/ledger/work-order dep): `handoff-policy` (serde+toml),
  `handoff-lease` (none), `handoff-test-support` (none).
- `handoff-schema` → work-order; `handoff-secrets` → envctl-secrets-engine (external).
- `handoff-core` → ledger, work-order, **handoff-schema**.
- `handoff-hooks`/`handoff-index`/`handoff-fleet`/`handoff-drift`/`handoff-intake` → handoff-core
  (+ ledger/work-order as needed).
- `handoff-route` → handoff-core, **handoff-fleet**.
- `handoff-gatekeeper` → handoff-core, **handoff-policy**, **handoff-route**, ledger (+ optional
  handoff-secrets).

No cycles. Confidence HIGH (read every `handoff-*/Cargo.toml`).

**Claim 3.3 (the coupling that matters for DR2) — `handoff-fleet` has clean downward deps but is
NOT a sink: `handoff-route` depends on it, and `handoff-gatekeeper` depends on it transitively.**
`handoff-route/Cargo.toml` deps `handoff-fleet`, and `route_for_task`/`fleet_home` call
`fleet::find_meta_root` (`handoff-route/src/lib.rs:28,46`). `handoff-gatekeeper/Cargo.toml` deps
`handoff-route`. **Implication:** any new dependency added to `handoff-fleet` (e.g. to locate or
run `scripts/handoff-loop-init.sh`) propagates at compile time into `handoff-route` and
`handoff-gatekeeper`. Confidence HIGH.

---

## 4. Workspace health — builds as one unit, lints uniformly inherited (HIGH)

**Claim 4.1 — the workspace is one unit of 21 members and `cargo check --workspace` passes
post-split.** `Cargo.toml:3` lists 21 members (8→21: the 13 new `handoff-*` crates + work-order,
ledger, hf, and the 5 `crates/*` rusty-idd members). I ran `cargo check --workspace` → **exit 0**
("0 errors"). The single emitted warning is an `unused_imports` in an unrelated `vault`/secrets
path-dep crate (`use vault::store::{… CertRow …}`), **not** in any `handoff-*` decomposition crate.
Confidence HIGH (ran the build).

**Claim 4.2 — the CI clippy gate (`clippy --workspace --all-targets -D warnings`) is uniformly
enforced because EVERY member declares `[lints] workspace = true`.** Confirmed for all 13
`handoff-*` crates **and** `hf` (grep of each `Cargo.toml`). The workspace lint policy
(`Cargo.toml:33-43`) denies `unsafe_code`, the clippy `all` group, and the HFTASK-0080
error-handling trio `unwrap_used`/`expect_used`/`panic`. So `handoff-fleet` inherits all of these:
**new fleet-sync code in that crate may not use a bare `.unwrap()`/`.expect()`/`panic!` in
production** (test code is exempted via `#![cfg_attr(test, allow(...))]`, e.g.
`handoff-fleet/src/lib.rs` mirrors `handoff-core/src/lib.rs:2`). Confidence HIGH.

**Caveat (MEDIUM):** I verified `cargo check`, not the full `clippy --all-targets -D warnings` CI
gate, and did not run `cargo test --workspace`. The decomposition compiles and type-checks clean;
a lint-only or test regression cannot be 100% excluded from static reading alone. To raise to HIGH,
run `cargo clippy --workspace --all-targets -- -D warnings` + `cargo test --workspace` (the kernel's
own CI gate, per `handoff/CLAUDE.md`).

---

## 5. Constraints the decomposition imposes on the fleet-sync change (HIGH)

**Claim 5.1 — `handoff-fleet` cannot call `hf`-main helpers; `cmd_fleet_sync` may only use
`handoff-core` (+ ledger/work-order/serde_json).** `hf` depends on `handoff-fleet`
(`hf/src/main.rs:44`), so `handoff-fleet` depending back on `hf` would be a build cycle. The fleet
crate's deps are exactly handoff-core/ledger/work-order/serde_json. Therefore any helper
`cmd_fleet_sync` needs — witnessing, JSON output, subprocess shell-out, ledger path — must come
from `handoff-core`, which already exposes them: `must_witness` (`handoff-core/src/lib.rs:119`),
`pretty_json` (`:134`), `run_out` (`:99`), `ledger_path` (`:58`). This is a *constraint satisfied
by design* — the very reason those helpers were lifted into `handoff-core` (its module doc,
`:117-118`, says feature crates witness "without depending back on the `hf` binary crate").
Confidence HIGH.

**Claim 5.2 — spawning a subprocess is NOT a new invariant for `handoff-fleet`, but mutating member
repos IS.** The crate already shells to `git` via `std::process::Command` 4× in production
(`handoff-fleet/src/lib.rs:157,173,199,220` — all read-only `git ls-files`/`check-ignore`), so
`Command` is in-bounds. Today the status sweep `cmd_fleet_status`/`collect_rows` is **pure-read**;
the only production `fs::write` is `render_member_packet` writing a derived packet
(`:563-565`). `cmd_fleet_sync` driving `handoff-loop-init.sh` would make the crate a **mutator of
member repos** (deploys guards/hooks) — a new behavioral class for this crate, and it must locate
the bash driver (a path coupling absent from the 4 deps). Confidence HIGH.

**Claim 5.3 — adding deps to `handoff-fleet` has blast radius into `handoff-route` +
`handoff-gatekeeper`.** Per Claim 3.3, prefer satisfying `cmd_fleet_sync` from the existing 4 deps
+ `std::process` (no new crate dep) to avoid pushing weight onto the routing/gatekeeper layer.
Confidence HIGH.

**Claim 5.4 (open, for DR2 not DR1) — witness/fail-closed decision is unsettled.** `cmd_fleet_status`
witnesses nothing; loop-init deploys emit no ledger event. Whether a `sync` action SHOULD witness
(kernel doctrine: every transition witnessed) is a DR2 design question. `must_witness` is available
in-crate if the answer is yes. Confidence — flagged as open, not a DR1 claim.

---

## Cross-dimension hooks for the synthesizer
- **DR2 (fleet-sync-blast-radius):** Claims 3.3, 5.1–5.3 are direct inputs — the dep cycle ban, the
  handoff-core-only helper constraint, and the route/gatekeeper blast radius from new fleet deps.
- **D4 (fleet-rollup):** the fleet logic moved file but not behavior; D4's `fleet.rs:290/515`
  citations are now `handoff-fleet/src/lib.rs` (cmd_fleet_status `:298`, render `:523`).
- **D5 (agent-loop-model):** the bandit router (`hf/src/routing.rs`) stayed an in-tree `hf` module;
  the ledger-residency router became the `handoff-route` crate — D5's routing citations split.

## Open questions / what would raise confidence
1. Run `cargo clippy --workspace --all-targets -- -D warnings` + `cargo test --workspace` to
   upgrade Claim 4.1/4.2 from "compiles clean" to "CI-gate clean" (the §4 MEDIUM caveat).
2. Confirm no other repo/script still references the deleted `hf/src/fleet.rs` etc. paths (the
   codemap-delta §3 flagged a STALE git-kb index edge to `hf/src/fleet.rs:290`; that is index
   staleness, not a live reference, but worth a grep sweep).
