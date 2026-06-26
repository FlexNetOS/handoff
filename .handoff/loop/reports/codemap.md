# Codemap — `hf` Continuity Ledger Kernel (handoff repo)

Target root: `/home/drdave/Desktop/meta/handoff`
Code intel: 1,830 symbols / 4,257 call edges / 520 files (Rust deep: 89 files, 1,820 symbols).
All file:line citations are against the target root unless noted.

## 1. What this system appears to be

A **local-first, auditable, reversible agentic continuity kernel**: a Rust CLI (`hf`) backed
by a witnessed, hash-chained, append-only event ledger. The repo (chat history is *not* truth) is
the source of record; every state transition (claim → checkpoint → ship → promote → handoff) is a
witnessed ledger event, and `hf handoff` renders a packet that **fails closed** unless the active
task's "agent contract" (an intent-lock hash) is formally proven. It also governs a **fleet** of
~37 sibling repos' `.handoff/` continuity layers via git-text rollup.

## 2. Crate / workspace layout

Rust workspace, `Cargo.toml:3`:
`members = ["work-order", "ledger", "hf", "crates/cli", "crates/core", "crates/runner", "crates/spec", "crates/tui"]`

| Crate | Path | Role | Key evidence |
|---|---|---|---|
| `work-order` | `work-order/` | The `handoff.task.v1` envelope: `WorkOrder`, `IntentLock`, `Status`, `compute_intent_lock` (blake3); `SwarmBundle → WorkOrder` seam. **No C, no path deps.** | `work-order/src/lib.rs:38,87,156`; `work-order/Cargo.toml` (serde, blake3, schemars) |
| `ledger` | `ledger/` | The operational-truth tier: pure-Rust **redb** ACID store + `rvf-crypto` witness chain; opt-in **v2** RVF semantic-recall overlay; deterministic JSONL export/import; legacy-SQLite→redb importer. | `ledger/src/lib.rs:1-39`; v1 store `ledger/src/v1.rs`; overlay `ledger/src/v2.rs`; `ledger/Cargo.toml` |
| `hf` | `hf/` | The CLI kernel: 2 bins (`hf`, `hf-mcp`), ~21 submodules, the verb surface, the contract gate, fleet rollup, session/loop model. | `hf/Cargo.toml`; `hf/src/main.rs` (4,348 lines) |
| `crates/*` (rusty-idd-{core,cli,runner,spec,tui}) | `crates/` | **Separate "Intent Driven Development" thin-adapter control plane** — dependency-light Rust toolkit for AI-assisted repo-unification; *not* part of the `hf` continuity path. | `crates/core/src/lib.rs:1-19` |

**RuVector sibling deps** (path deps to `../../RuVector/...`, cloned as sibling `RuVector/` in CI):
- `rvf-crypto` (witness chain), `rvf-runtime`/`rvf-index`/`rvf-types` (v2 recall) — `ledger/Cargo.toml`
- `ruvector-verified` (formal proof for the contract gate) — `hf/Cargo.toml`
- `ruvector-domain-expansion` (Thompson-bandit task routing) — `hf/Cargo.toml`
- `cognitum-gate-tilezero` (action governor, default feature) — `hf/Cargo.toml`
- `envctl/crates/secrets-engine` (optional merge-gate seam) — `hf/Cargo.toml`

## 3. Entry points

- **`hf` binary** — `hf/Cargo.toml [[bin]] name="hf" path="src/main.rs"`. `fn main()` at
  `hf/src/main.rs:3217`; dispatch is a positional `match args.first()` over verb strings,
  `hf/src/main.rs:3220-3559`. Global flags `--ledger PATH` / `HANDOFF_LEDGER` are stripped before
  dispatch (`apply_ledger_flag`, `ledger_path()` near `main.rs:87-97`).
- **`hf-mcp` binary** — `hf/src/bin/hf-mcp.rs`: an MCP (JSON-RPC over stdio, protocol `2024-11-05`)
  server that exposes the verbs (`hf_status`, `hf_claim`, `hf_ship`, …) as tools by shelling to the
  `hf` binary. Header `hf/src/bin/hf-mcp.rs:1-18`.

## 4. Verb surface (CLI external interface)

Dispatched at `hf/src/main.rs:3220-3559`; handler `fn cmd_*` locations in `hf/src/main.rs` unless noted:

| Verb | Handler | Purpose |
|---|---|---|
| `init` | `cmd_init` :421 | Portable `.handoff` init (repo self-identifies; ledger gitignore guard) |
| `seed` | `cmd_seed` :2699 | Kernel-only: seed the HFTASK backlog (cards + tight `test_commands`) |
| `status` | `cmd_status` :2255 | Live board recomputed from ledger replay (`--json`) |
| `claim` / `claim --batch` / claim-next | `cmd_claim` :509, `cmd_claim_with` :1010, `cmd_claim_batch` :952 | Atomic in-ledger + weave lease acquire; transitions Backlog→Active |
| `release` | `cmd_release` :1109 | Un-claim: free leases, transition back to Backlog |
| `reopen` | `cmd_reopen` :1174 | Re-open a Done/Review task with a reason |
| `lease` | `cmd_lease` :1224 | Show lease holders |
| `checkpoint` | `cmd_checkpoint` :1265 | Witnessed progress evidence (`--auto`/`--quiet`) |
| `done` | `cmd_done` :1322 | Fail-closed completion; auto `pr_merged` + auto `promote` |
| `test` | `cmd_test` :1765 | Run a card's `test_commands`; **fails closed on zero executed tests** |
| `ship` | `cmd_ship` :1927 | Open a PR (`--base`, branch/remote policy via `branch.rs`) |
| `promote` | `cmd_promote` :1567 | Hands-off develop→trunk ff via gh-api PATCH (runner-independent) |
| `handoff` | `cmd_handoff` :2570 | **Contract-proof gate** then render packet + `active.md` (fail-closed) |
| `resume` | `cmd_resume` :2644 | Render resume packet (Full/Json/Compact) from ledger |
| `drift` | `gates::cmd_drift` | Drift detection gate (intent-lock mismatch, out-of-scope writes) |
| `doctor` | `cmd_doctor` :595 | Fail-closed invariant sweep + stale-lock report |
| `reconcile` | `cmd_reconcile` :884 | Reconcile ledger vs cards/git |
| `export` / `import` | `cmd_export` :96 / `cmd_import` :122 | Deterministic JSONL ledger export / rebuild |
| `migrate` | `cmd_migrate` :753 | Legacy C-SQLite → redb (feature `legacy-sqlite`) |
| `sync` / `sync-cards` | `sync.rs` | Idempotent one-way maintenance passes (session-end/post-merge) |
| `task mint` | `kb::` | Mint a card from a git-kb task (planning→execution seam) |
| `intake` / `dispatch` | `intake.rs` | Front door: prompt_hub `SwarmBundle` → synthesized cards |
| `prompt-hub` | `prompt_hub.rs` | NL "vibe" → verifiable card |
| `review request` / `review verdict` | `cmd_review_request` :2118 / `cmd_review_verdict` :2066 | Witnessed PR review verdicts |
| `gatekeeper check` | `gatekeeper.rs` | Deterministic AI-gatekeeper status check |
| `policy gate` / `policy check-*` | `cognitum.rs` / `gates::cmd_policy_check` | Action governor (Permit/Defer/Deny); claim/edit/handoff gates |
| `secret gate-check` | `secrets.rs` | Optional merge-gate decision helper |
| `session start|end|reap` | `session::cmd_session` | Worktree-isolated loop sessions |
| `hook list|run` | `hooks.rs` | Typed lifecycle-hook contract (14 events) |
| `fleet status|render` | `fleet.rs:290,515` | Fleet rollup board / member packet render |
| `delivery get|list` | `delivery.rs` | Output endpoint |
| `gitignore` | `cmd_gitignore` :542 | Durability gitignore repair (`durability.rs`) |
| `schema` | `schema.rs` | Emit / validate the `handoff.task.v1` JSON schema |

## 5. Ledger / witness / RVF substrate (`ledger/`)

- **Authoritative store** is `v1` (module name kept; it is now **redb**, not SQLite) — `ledger/src/lib.rs:17-19`.
  `pub struct Ledger` at `ledger/src/v1.rs:231`; `Ledger::open` :400; `append` :482 (each write is a
  `begin_write()` tx that reads the tail inside the tx for chain integrity).
- **Witness chain / tamper-evidence**: `hash_action` (sha3) `ledger/src/v1.rs:291`; `verify_witness_chain`
  :829; serde of `[u8;32]` hashes :164-193.
- **Atomic lease CAS**: `try_acquire_lease` :538, `resolve_lease` state machine :313, `release_lease` :635,
  `lease_holder` :642, `LeaseOutcome` :301.
- **Rollup provenance**: `rollup_from` :730, `verify_rollup_provenance` :853, `RollupProvenance`/`is_faithful`
  :266-291, `RollupStat` :252 — the integrity proof behind `hf fleet status`.
- **Replay**: `all_events` :672, `events_after` :688, `replay_latest_status` :806; `EventRow` :238.
- **v2 overlay** (default feature) layers `rvf-runtime::RvfStore` for HNSW `query_by_intent` semantic
  recall over the redb store — `ledger/src/lib.rs:6-10,36-38`; `ledger/src/v2.rs`.
- **Text export** (committed truth, ADR-0018 D1): `export_jsonl` / `rebuild_from_jsonl`
  `ledger/src/export.rs`; binary `ledger.db`(+`.rvf`) is a gitignored rebuild cache.
- **Legacy import**: `migrate_sqlite_to_redb` + `file_is_legacy_sqlite` magic-byte guard (fail-closed)
  `ledger/src/migrate.rs`, `ledger/src/v1.rs:379`.

## 6. Contract-proof gate (`hf/src/contract.rs`)

The intent-lock IS the agent contract. `prove_contract` (`contract.rs:119`) discharges it through the
real `ruvector-verified` crate: re-derives the blake3 hashes via `WorkOrder::compute_intent_lock`
and full-string-compares to the recorded lock; on match mints an `Eq.refl` proof term + a tamper-evident
`ProofAttestation`. Obligations: objective / path_scope / acceptance integrity + completion-evidence
(≥1 witnessed checkpoint when status Review/Done) — `contract.rs:14-19`. **Wired fail-closed into
`cmd_handoff`** at `hf/src/main.rs:2578-2591` (exits before any packet write). Render: `render_proof_section`
`contract.rs:275`.

## 7. Fleet rollup (`hf/src/fleet.rs`)

`find_meta_root` :30 → enumerate members from `.meta.yaml` → read each repo's git-text `.handoff`
(capsule + cards) → join with FLEET ledger events. **Git is the sync transport, no daemons; precedence
Git > ledger > cards** (`fleet.rs:1-6`). P7 residency policy (ADR-0018 D1 inversion): committed
`.handoff/ledger.events.jsonl` is *required*; a git-tracked binary `.db` is banned; gitignore guard
required (`fleet.rs:8-21`). Entry: `cmd_fleet_status` :290, `render_member_packet` :515. Fleet members
live under `.handoff/fleet/<member>/` (~37 dirs).

## 8. Agent / loop model

- **Session isolation** (`hf/src/session.rs:1-9`): a session = fresh worktree off `origin/<base>` + a
  weave path-scope lease + witnessed `session_start`/`session_end` events; refuses to start on a drifted
  tree. Reuses `meta git worktree` (not a crate dep, to stay independently-cloneable), falls back to plain
  `git worktree`. `cmd_session` :205; reap-on-merge: `reap_decide` :64, `reap_open_session_if_merged` :614,
  `cmd_session_reap` :568; `preflight_decide` :79.
- **Task routing** (`hf/src/routing.rs:1-3`): `next_safe` picks the next task by topological order over
  deps (first backlog whose deps are Done); used by `cmd_handoff` (`main.rs:2597`) and the loop.
  RuVector Thompson-bandit value routing via `ruvector-domain-expansion`.
- **Action governance** (`hf/src/cognitum.rs`): `hf policy gate <action>` → Permit/Defer/Deny witnessed
  decision (default feature). Hard gates `hf policy check-{claim,edit,handoff}` + `hf drift` in
  `gates.rs` are what `.handoff/hooks/hooks.toml` fires (PreEdit/PreHandoff/TaskClaim), exit-nonzero on block.
- **Lifecycle hooks** (`hf/src/hooks.rs`): typed 14-event contract; `hf hook list/run`; deployed fleet-wide.
- **kb seam** (`hf/src/kb.rs`, ADR-0003): one-way planning(git-kb)→execution(.handoff); claim/checkpoint/
  done/release mirror progress back to kb.

## 9. `.handoff/` layout (on-disk control surface)

```
.handoff/
  ledger.events.jsonl   committed continuity truth (durable, ADR-0018 D1)
  ledger.db, .db.rvf    binary redb cache + RVF sidecar (gitignored, rebuildable)
  active.md             rendered "Next / Done N/M" view
  packets/latest.md     rendered resume packet
  tasks/HFTASK-*.task.json   work-order cards
  context/capsule.json  identity/northstar capsule (packet renders from this, ADR-0006)
  policy.toml / policies/    handoff.policy.v1 (remote/loop/merge/preflight/sync)
  decisions/  deliveries/  locks/  skills/  hooks/  loop/
  fleet/<member>/ + fleet/PILOT.toml   per-member fleet capsules
```

## 10. Build / run surface

- Build: `cargo build` (workspace); `hf` default features `["cognitum"]` (`hf/Cargo.toml`);
  ledger default `["v2"]`. `legacy-sqlite` is the only C-pulling feature (never default).
- CI gate (mirror locally): `cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --all --check`
  + `cargo test` (`.github/workflows/ci.yml`; documented in `CLAUDE.md`). RuVector cloned as sibling for path deps.
- Run: `hf resume` (session start), then the loop verbs. `Makefile`, `scripts/preflight.sh`,
  `scripts/handoff-loop-init.sh`, `scripts/differential-drive.sh` are the operational scripts.
- Specs/ADRs: `docs/Continuity_Ledger_Kernel_PRD.md`, `docs/adr-0001..0018-*.md`, `schemas/{task,packet,session}.schema.json`.

## 11. Dependency graph (internal)

```
work-order  ──(envelope/IntentLock)──▶ ledger ──▶ hf
                                         ▲          │
            rvf-crypto (witness) ────────┘          ├─▶ ruvector-verified  (contract proof)
            rvf-runtime/index/types (v2 recall) ────┤   ruvector-domain-expansion (routing)
                                                     ├─▶ cognitum-gate-tilezero  (action gate)
                                                     └─▶ envctl/secrets-engine   (optional)
crates/* (rusty-idd-*)  =  independent IDD adapter, NOT on the hf continuity path
```

## 12. Gaps / deferred (explicit, for honest coverage)

- `crates/*` (rusty-idd) read only at the `lib.rs` doc level — its runner/spec/tui internals are
  unmapped (separate control plane; low relevance to the kernel question).
- `ledger/src/v1.rs` (1,654 lines) and `v2.rs` (720) read at the public-API level; internal redb
  table layout / RVF overlay internals not deep-read.
- `hf/src/main.rs` (4,348 lines) mapped at the dispatch + key-handler level; many helper fns
  (`render_packet_md`, `load_tasks`, `current_statuses`, status helpers) cited but not line-by-line.
