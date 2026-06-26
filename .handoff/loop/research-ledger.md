# Research Ledger — `hf` Continuity Ledger Kernel

Research question: a cited, decision-grade architecture + capability map of the hf kernel — entry
points, ledger/witness/RVF substrate, verb surface, contract-proof gate, fleet rollup, agent/loop model.

Structural map: `./reports/codemap.md`. Dimensions below are dependency-ordered (map → core → edges).
Status legend: `[ ]` todo · `[~]` in progress · `[x]` done · `[!]` blocked/gap.

## Dimensions

- [x] **D0 · map-and-entrypoints** — crate layout, `hf`/`hf-mcp` bins, dispatch surface.
  Start: `Cargo.toml:3`; `hf/src/main.rs:3217-3559` (main + dispatch); `hf/Cargo.toml`;
  `hf/src/bin/hf-mcp.rs:1-18`. (Completed in this MAP pass — codemap §2-§4.)

- [x] **D1 · ledger-witness-rvf-substrate** — Is the ledger genuinely append-only, hash-chained,
  tamper-evident, and ACID? Verify the redb-tx-per-append + read-tail-in-tx invariant; the witness
  chain (`hash_action`/`verify_witness_chain`); atomic lease CAS; rollup provenance; the v2 RVF
  recall overlay; and the no-C trust boundary (default graph links no `rusqlite`/`-sys`).
  Start: `ledger/src/lib.rs`; `ledger/src/v1.rs:231,291,400,482,538,730,829,853`; `ledger/src/v2.rs`;
  `ledger/src/export.rs`; `ledger/Cargo.toml` (features).

- [x] **D2 · verb-surface** — Enumerate + characterize the full verb set and the
  claim→checkpoint→ship→promote→handoff lifecycle. What each verb mutates in the ledger, which are
  fail-closed, the auto-chains (`done`→auto `pr_merged`+`promote`). Start: dispatch
  `hf/src/main.rs:3220-3559`; handlers `cmd_claim`:509, `cmd_checkpoint`:1265, `cmd_done`:1322,
  `cmd_test`:1765, `cmd_ship`:1927, `cmd_promote`:1567, `cmd_handoff`:2570, `cmd_status`:2255.

- [x] **D3 · contract-proof-gate** — Does `hf handoff` really fail closed on an unprovable contract?
  Trace `prove_contract` → `ruvector-verified` `Eq.refl` proof + `ProofAttestation`; the 4 obligations;
  the re-derivation faithfulness (reuses `compute_intent_lock`); the exit-before-write wiring.
  Start: `hf/src/contract.rs:14-19,119,275`; gate site `hf/src/main.rs:2578-2591`;
  `work-order/src/lib.rs:156,182` (IntentLock).

- [x] **D4 · fleet-rollup** — How does fleet aggregation work without daemons? Git-as-transport;
  state precedence Git>ledger>cards; P7 residency policy (ADR-0018 D1 inversion: JSONL required,
  tracked binary banned); rollup-provenance integrity. Start: `hf/src/fleet.rs:1-21,30,290,515`;
  `ledger/src/v1.rs:730,853`; `.handoff/fleet/`; `docs/adr-0004-fleet-handoff-rollout.md`.

- [x] **D5 · agent-loop-model** — The autonomous loop: session worktree isolation + weave lease,
  `next_safe` topological routing, RuVector bandit value-routing, cognitum action governance,
  the typed hook contract, and the kb planning↔execution seam. Is "no human in the loop" actually
  enforced (witnessed gatekeeper verdicts replacing human approval)? Start: `hf/src/session.rs:1-9,64,205,614`;
  `hf/src/routing.rs`; `hf/src/cognitum.rs`; `hf/src/gates.rs`; `hf/src/hooks.rs`; `hf/src/gatekeeper.rs`;
  `hf/src/kb.rs`; `docs/adr-0018-full-auto-agentic-operation.md`.

- [x] **D6 · claims-vs-code (skeptical doc check)** — Reconcile the project's own claims
  (NORTH-STAR, AGENTS.md, PRD, ADRs, CLAUDE.md change-history) against the code. Flag any
  designed-but-unbuilt or partially-implemented item (e.g. PRD §12 drift: `detect_drift` implements
  2 of 10 checks per the seed note at `hf/src/main.rs` cmd_seed; `crates/*` rusty-idd relationship).
  Start: `docs/Continuity_Ledger_Kernel_PRD.md`; `NORTH-STAR.md`; `AGENTS.md`; `hf/src/gates.rs:345`.

## Re-map delta (2026-06-26) — see `./reports/codemap-delta-2026-06-26.md`

Context: the `hf` monolith was decomposed (ADR-0019 D5 #4 / HFTASK-0081/0083) into 21 workspace
members incl. 13 new `handoff-*` crates; `fleet.rs` → `handoff-fleet`. Two fresh dimensions:

- [ ] **DR1 · crate-decomposition-state** — Is the decomposition **behavior-preserving**? Verify the
  crate-root alias mechanism (`use handoff_X as Y;` at `hf/src/main.rs:31-68`) actually keeps every
  pre-existing `crate::fleet::…` / `crate::branch::…` / `crate::HF` / `crate::ledger_path` path valid
  unchanged (no `pub use` shim files exist — the old `hf/src/{fleet,gates,hooks,…}.rs` are deleted).
  What COUPLING remains: which `handoff-*` crates still depend on each other vs. on `handoff-core`/
  `ledger`/`work-order` only; is `handoff-fleet` (deps: handoff-core+ledger+work-order+serde_json,
  `handoff-fleet/Cargo.toml`) a clean leaf? Does `cargo build --workspace` + the CI gate
  (`clippy --workspace --all-targets -D warnings`) still pass post-split? Note the routing duplication
  (local `hf/src/routing.rs` AND `handoff-route` as `route`).
  Start: `Cargo.toml:3`; `hf/src/main.rs:28-68`; each `handoff-*/Cargo.toml`; `handoff-core/src/lib.rs:19-240`.

- [ ] **DR2 · fleet-sync-blast-radius** — For adding `pub fn cmd_fleet_sync(json, dry_run)` to
  `handoff-fleet` + a `Some("fleet") if …=="sync"` dispatch arm in `hf/src/main.rs` (near the
  status/render arms at `:3797-3808`) that drives `scripts/handoff-loop-init.sh` per flagged member
  (= HFTASK-0087, seeded `hf/src/main.rs:3100-3111`, depends on HFTASK-0085):
  enumerate callers/callees/impacted files; what shares state with `collect_rows`; the risks.
  Key facts to verify/expand: (a) `cmd_fleet_sync` must reuse `collect_rows`/`Row` to find flagged
  members — but `Row` + its flag fields are **private** (`handoff-fleet/src/lib.rs:95-124`), so either
  the fn lives in-crate or `Row`/a flagged-members helper must be made `pub`. (b) `handoff-fleet`
  **already** shells to `git` via `std::process::Command` 6× (`:157,173,199,220,740,818`), so spawning
  a process is NOT a new invariant — BUT today the crate is **read-only** (`cmd_fleet_status` writes
  nothing); `cmd_fleet_sync` makes the fleet crate a **mutator** of member repos (deploy guards/hooks),
  and it must **locate** the bash script (`$HANDOFF_KERNEL_HOME`/repo-relative) — a new coupling absent
  from the crate's 4 deps. (c) Witness/provenance: loop-init deploys are git-text only and emit **no**
  ledger event — decide whether a sync action SHOULD be witnessed (kernel doctrine: every transition is
  witnessed) and how to avoid a fail-OPEN silent proceed; `cmd_fleet_status` witnesses nothing today.
  (d) Test surface: `handoff-fleet` has 5 `#[test]`s (`:625,648,670,727,805`) that build temp git repos
  and assert flag detection — a `cmd_fleet_sync` test needs a `--dry-run` unit path (real shell-out to
  `handoff-loop-init.sh` needs the `hf` binary + kernel home → integration test, not unit). (e) Impacted
  files: `handoff-fleet/src/lib.rs` (new fn + maybe pub-ify `Row`), `hf/src/main.rs` (dispatch arm +
  usage string + the HFTASK-0087 `test_commands` in `cmd_seed`), possibly `handoff-fleet/Cargo.toml`,
  `scripts/handoff-loop-init.sh` (the driver), HFTASK-0085 prerequisite.
  Start: `handoff-fleet/src/lib.rs:95-124,228-296,298,523`; `hf/src/main.rs:3797-3808,3100-3111`;
  `scripts/handoff-loop-init.sh`.
