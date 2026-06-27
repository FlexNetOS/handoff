# Codemap DELTA — 2026-06-26 (crate decomposition + fleet-surface re-map)

Builds on `./codemap.md` (prior MAP pass). Target root: `/home/drdave/Desktop/meta/handoff`.
Code intel: git-kb refreshed to 2,582 symbols / 614 files / 24,692 call sites — but see the
**index-degradation note** in §3 (cross-crate edges + per-file `symbols` lookups are stale after
the move; this delta falls back to direct source inspection per the code-intelligence rule).

## 1. What changed since the prior map

The prior map described `hf` as a **monolith** (`hf/src/main.rs` ~4,348 lines + ~10 local `mod`s
including `fleet.rs`, `gates.rs`, `hooks.rs`, `schema.rs`, `gatekeeper.rs`, `intake.rs`, `branch.rs`).
Per **ADR-0019 D5 #4 / HFTASK-0081 / HFTASK-0083**, the monolith was **decomposed into a 12-crate
workspace of `handoff-*` library crates**. `hf` is now a thin CLI shell that re-aliases those crates.
Behavior is preserved by **crate-root `use … as …` aliases** (not in-tree shim files — the old
`hf/src/fleet.rs` etc. are *deleted*). `hf/src/main.rs` is now 4,679 lines (grew slightly: held the
backlog seed for HFTASK-0085/0086/0087 + the dispatch).

## 2. Workspace member-list delta

`Cargo.toml:3` now lists **21 members** (was 8). New + unchanged:

| Member | Status | Role | Evidence |
|---|---|---|---|
| `work-order` | unchanged | `handoff.task.v1` envelope; now also owns `PrioStr`, `Priority` | `Cargo.toml:3`; `hf/src/main.rs:58` |
| `ledger` | unchanged | redb + RVF witness substrate | `Cargo.toml:3` |
| `handoff-core` | **NEW** | leaf continuity primitives: `HF`, `now_ns`, `ledger_path`, `tasks_dir`, `run_out`, `current_statuses`, `status_of`, `parse_card_file`, `load_tasks`, `next_safe`, `pretty_json`, `must_witness`, `scan_card_conformance` | `handoff-core/src/lib.rs:19-240`; `hf/src/main.rs:64-68` |
| `handoff-policy` | **NEW** | branch/remote/merge policy engine — owns `branch` + `policy` submodules | `handoff-policy/src/{branch.rs,policy.rs,lib.rs}`; `hf/src/main.rs:31` |
| `handoff-schema` | **NEW** | `handoff.task.v1` runtime validator (owns the `jsonschema` dep) | `handoff-schema/src/lib.rs`; `hf/src/main.rs:34` |
| `handoff-lease` | **NEW** | weave-coordinated claim-lease bridge (`Leaser`/`ClaimGate`/`WeaveCli`, HFTASK-0048) | `handoff-lease/src/lib.rs`; `hf/src/main.rs:37,56` |
| `handoff-hooks` | **NEW** | typed lifecycle-hook contract (14 events; `cmd_hook_list`/`run`) | `handoff-hooks/src/lib.rs`; `hf/src/main.rs:40` |
| `handoff-index` | **NEW** | index/plan maps | `handoff-index/src/lib.rs`; `hf/src/main.rs:42` |
| `handoff-fleet` | **NEW** | fleet rollup aggregation + member packet render (this delta's focus) | `handoff-fleet/src/lib.rs`; `hf/src/main.rs:44` |
| `handoff-drift` | **NEW** | drift-audit + policy-check engine (former `gates.rs`) | `handoff-drift/src/lib.rs`; `hf/src/main.rs:46` |
| `handoff-route` | **NEW** | ledger-routing module (note: distinct from the local `hf/src/routing.rs`) | `handoff-route/src/lib.rs`; `hf/src/main.rs:48` |
| `handoff-test-support` | **NEW** | shared cwd-serialization test mutex (dev-dep) | `handoff-test-support/src/lib.rs`; `hf/Cargo.toml` dev-deps |
| `handoff-secrets` | **NEW** | envctl secrets merge-gate (optional, `secrets` feature) | `handoff-secrets/src/lib.rs`; `hf/Cargo.toml` |
| `handoff-gatekeeper` | **NEW** | AI gatekeeper + `GhPrView` GitHub-PR type | `handoff-gatekeeper/src/lib.rs`; `hf/src/main.rs:51-52` |
| `handoff-intake` | **NEW** | front-door intake/dispatch verbs | `handoff-intake/src/lib.rs`; `hf/src/main.rs:54` |
| `hf` | thinned | CLI shell + remaining local `mod`s | `hf/Cargo.toml`; `hf/src/main.rs` |
| `crates/{cli,core,runner,spec,tui}` | unchanged | rusty-idd IDD toolkit (NOT on the continuity path) | `Cargo.toml:3` |

**Modules that MOVED out of `hf/src/` (old file → new crate, alias used in `hf`):**

| Old hf module | New crate | Alias (`hf/src/main.rs`) |
|---|---|---|
| `fleet.rs` | `handoff-fleet` | `use handoff_fleet as fleet;` :44 |
| `gates.rs` | `handoff-drift` | `use handoff_drift as gates;` :46 |
| `hooks.rs` | `handoff-hooks` | `use handoff_hooks as hooks;` :40 |
| `schema.rs` | `handoff-schema` | `use handoff_schema as schema;` :34 |
| `gatekeeper.rs` | `handoff-gatekeeper` | `use handoff_gatekeeper as gatekeeper;` :51 |
| `intake.rs` | `handoff-intake` | `use handoff_intake as intake;` :54 |
| `branch.rs` + policy | `handoff-policy` | `pub(crate) use handoff_policy::{branch, policy};` :31 |
| `lease.rs` | `handoff-lease` | `use handoff_lease as lease;` :37 |
| `index.rs` | `handoff-index` | `use handoff_index as index;` :42 |
| ledger-routing | `handoff-route` | `use handoff_route as route;` :48 |
| shared helpers | `handoff-core` | `pub(crate) use handoff_core::{…};` :64-68 |

**Still local `mod` in `hf/src/` (NOT moved):** `cognitum`, `contract`, `delivery`, `durability`,
`kb`, `prompt_hub`, `routing`, `session`, `sync` — `hf/src/main.rs:15-23`. (Note `routing` stays
local AND `handoff-route` exists as `route`; the local `routing.rs` is the `next_safe`/bandit task
picker, `handoff-route` is the ledger-ops crate.)

**The re-export "shim" mechanism (load-bearing for DR1):** there are **no shim files**. The crate-root
aliases (`use handoff_X as Y;` / `pub(crate) use handoff_policy::{branch, policy};` /
`pub(crate) use handoff_core::{…};`) create `crate::Y` names, so every pre-existing
`crate::fleet::…` / `fleet::…` / `crate::branch::…` / `crate::HF` / `crate::ledger_path` path across
`main.rs` and its sibling modules (`session`, `kb`, `sync`, `contract`…) resolves **unchanged**. The
comments at `hf/src/main.rs:28-30,61-63` state this is "behavior-preserving" — a **claim to verify**
(DR1).

## 3. FLEET surface re-map (priority — for the in-flight `hf fleet sync` change)

All in `handoff-fleet/src/lib.rs` unless noted. **`hf fleet` dispatch** lives in `hf/src/main.rs`.

### Public API (what `hf` calls)
| Symbol | Loc | Signature / role |
|---|---|---|
| `find_meta_root` | `:37` | `pub fn -> Option<PathBuf>` — walk up for `.meta.yaml` |
| `parse_members` | `:53` | `pub fn(&str) -> Vec<String>` — YAML-free `projects:` 2-space-key parse |
| `cmd_fleet_status` | `:298` | `pub fn(json: bool)` — the status board entry |
| `render_member_packet` | `:523` | `pub fn(&Path, &str) -> Result<PathBuf,String>` — render `<member>/.handoff/packets/latest.md` |

### Internal (private to the crate — DR2 coupling point)
| Symbol | Loc | Role |
|---|---|---|
| `struct Row` | `:95` | per-member row; **fields are private** |
| `Row.jsonl_export_missing` | `:106` | local ledger on disk but `ledger.events.jsonl` not git-tracked (ADR-0018 D1 primary P7 gate) |
| `Row.tracked_ledger` | `:110` | a git-TRACKED binary `.db` under `.handoff` (banned) |
| `Row.ledger_guard_missing` | `:113` | `.gitignore` lacks `.handoff/**/ledger.db` guard |
| `Row.walshm_guard_missing` | `:116` | `.gitignore` lacks `*.db-wal`/`*.db-shm` guard |
| `Row.per_repo_chain` | `:123` | `Option<PerRepoChain{events,witnessed}>` standalone witness verify |
| `collect_rows` | `:228` | `fn(&Path,&[String]) -> Vec<Row>` — the per-member sweep; **pure read** (filesystem + `git ls-files`/`check-ignore`), writes nothing |
| `git_tracks_handoff_db` | `:156` | `git ls-files -- .handoff` → any `*.db*` tracked |
| `git_tracks_jsonl_export` | `:172` | `git ls-files -- .handoff/ledger.events.jsonl` |
| `local_ledger_on_disk` | `:189` | `.handoff/ledger.db` is_file |
| `ledger_guard_present` | `:198` | `git check-ignore -q .handoff/ledger.db` |
| `walshm_guard_present` | `:216` | `git check-ignore -q` on the two sidecars |
| `fleet_ledger_stats` | `:268` | central ledger events + witness count |
| `fleet_provenance` | `:287` | `verify_rollup_provenance()` over the FLEET ledger |
| `per_repo_chain_stats` | `:135` | open `<repo>/.handoff/ledger.db`, verify chain standalone |
| `load_member_tasks` | `:497` | LOUD card load via `handoff_core::parse_card_file` (fail-open fix) |

### Call map (caller → callee, with file:line)
- **`hf` dispatch → fleet** (`hf/src/main.rs`):
  - `main` :3797 arm `Some("fleet") if args.get(1)=="status"` → **`fleet::cmd_fleet_status`** call at `hf/src/main.rs:3798`.
  - `main` :3800 arm `Some("fleet") if args.get(1)=="render"` → `fleet::find_meta_root` at `:3807`, then `fleet::render_member_packet` at `:3808`.
- **`cmd_fleet_status` (`:298`) callees:** `find_meta_root` :299, `parse_members` :304, `collect_rows` :305,
  `fleet_ledger_stats` :306, `fleet_provenance` :309, `handoff_core::pretty_json` :397.
- **`collect_rows` (:228) callees:** `git_tracks_handoff_db` :238, `local_ledger_on_disk`+`git_tracks_jsonl_export` :240-243,
  `ledger_guard_present` :244, `walshm_guard_present` :245-248, `count_cards`/`capsule_field` :253-256, `per_repo_chain_stats` :261.
- **`render_member_packet` (:523) callees:** `load_member_tasks` :497, `compose_member_packet` (unit-tested helper),
  + `ledger::Ledger` open for the FLEET ledger.
- **`cmd_fleet_status` callers:** only the dispatch arm at `hf/src/main.rs:3798` (single caller).

### Index-degradation note (honest gap)
`git kb code symbols handoff-fleet/src/lib.rs` returns **"No symbols found"** (the refreshed index
does not resolve symbols by the new crate path), and `git kb code callers cmd_fleet_status` returns a
**STALE** entry for the now-deleted `hf/src/fleet.rs:290` (carrying the `main.rs:3798` call edge)
*alongside* the real `handoff-fleet/src/lib.rs:298` entry marked **"(no callers)"** — i.e. the
cross-crate call edge is unresolved post-move. All call sites above were therefore confirmed by direct
grep/read, not the call graph. **Re-index by the new crate paths before trusting git-kb for fleet.**

## 4. Confirmation: the DR2 change is already a seeded backlog card

`hf fleet sync` is **HFTASK-0087** ("Automation rung 3: REMEDIATE detected drift, not just report"),
seeded in `cmd_seed` at `hf/src/main.rs:3100-3111`, **depends on HFTASK-0085** (`hf --version` staleness,
`:3087`) and is followed by HFTASK-0088 (auto-onboard members absent from `.handoff`, `:3110`). The card
text mandates: reuse the status sweep's flagged members, run the idempotent `handoff-loop-init.sh`
deploy bits per member, **fail-closed per member** (one member's failure never aborts the sweep),
schedulable from a meta cron. So DR2 maps an in-flight, already-specified change.
