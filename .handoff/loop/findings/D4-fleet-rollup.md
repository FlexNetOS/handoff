# D4 · Fleet-Rollup — findings

Dimension question: How does fleet aggregation work without daemons? (git-as-transport;
state precedence Git > ledger > cards; P7 residency policy + ADR-0018 D1 inversion;
rollup-provenance integrity.)

Target root: `/home/drdave/Desktop/meta/handoff` (read via the `code-research` worktree).
All `file:line` cites are against the worktree copy, identical paths under the target root.

Verdict: Fleet aggregation is a **daemonless, pull-on-invocation, git-as-transport rollup**:
sibling repos' gitignored per-repo ledgers are re-appended (not merged) into one central FLEET
ledger with cryptographic provenance, and `hf fleet status` is the read-side integrity gate that
re-verifies three independent layers. The design is real, wired, and unit-tested. Two honest
caveats: provenance proves *content* identity (not chain *position*), and the on-disk
`.handoff/fleet/<member>/` capsule store is NOT what the Rust reads.

---

## CLAIMS

### A. Daemonless / git-as-transport

- **CLAIM D4-1 (high):** Fleet aggregation runs **only on command invocation** — there is no
  background process, scheduler, or service. `cmd_fleet_status` does synchronous filesystem +
  git + ledger reads and returns. Evidence: `hf/src/fleet.rs:290-479` (entire fn is straight-line
  I/O, no spawn/thread/loop-forever). Verb dispatch is a one-shot `match`: `hf/src/main.rs:3485-3486`
  (`Some("fleet") if … "status" => fleet::cmd_fleet_status(...)`).

- **CLAIM D4-2 (high):** "Git is the sync transport — no daemons" is the stated and implemented
  model: members are enumerated from the meta root's `.meta.yaml`, and each repo's *committed*
  git-text `.handoff` (capsule + cards) is read from the working tree that `meta git update`
  already pulled. Evidence: module doc `hf/src/fleet.rs:1-6`; ADR-0004 §4 `docs/adr-0004-fleet-handoff-rollout.md:67-71`
  ("Git is the sync transport — no daemons, no new services; `meta git update` pulls fleet state
  naturally").

- **CLAIM D4-3 (high):** Member discovery walks up from cwd to the dir holding `.meta.yaml`
  (`find_meta_root`, `hf/src/fleet.rs:30-40`) then parses member names with a **hand-rolled YAML
  reader** — deliberately no YAML crate, to respect the no-C / dependency-light trust boundary
  (`parse_members`, `hf/src/fleet.rs:42-69`; rationale in the doc comment at `:42-45`). It reads
  only 2-space-indented bare `name:` keys under `projects:`. Confidence high; unit-tested at
  `hf/src/fleet.rs:617-638`.

### B. State precedence (Git > ledger > cards)

- **CLAIM D4-4 (high):** Precedence Git > (FLEET) ledger > cards is both documented and rendered
  into every member packet. Evidence: doc `hf/src/fleet.rs:5-6`; the rendered packet literally
  emits "## 2. State Precedence\nGit > FLEET ledger (meta/.handoff/ledger.db) > tasks/*.task.json >
  this packet." at `hf/src/fleet.rs:590`.

- **CLAIM D4-5 (high):** The ledger-over-card precedence is *operative*, not just prose: a member
  card's displayed status is the **ledger replay** value where the work-order id appears, falling
  back to the card's stored status only when the ledger is silent. Evidence: `member_status_of`
  `hf/src/fleet.rs:561-567` (`replay.iter().find(... == card.id) … .unwrap_or(card.status)`), fed
  by `replay_latest_status()` `ledger/src/v1.rs:806-825`.

- **CLAIM D4-6 (medium):** "Git first" is enforced for the *residency* questions by asking git, not
  the filesystem — `git ls-files` / `git check-ignore` decide tracked-ness and ignore-status
  (see P7 claims below). So git truth, not disk state, drives the P7 verdict.
  Evidence: `git_tracks_handoff_db` `hf/src/fleet.rs:148-160`, `ledger_guard_present` `:190-202`.

### C. Rollup mechanism (the actual aggregation feed)

- **CLAIM D4-7 (high):** Aggregation is **append-with-provenance re-chaining (CT / RFC 6962
  model)**, NOT a chain merge: each source event is re-appended onto the *central* tail, allocating
  a fresh central `seq` and re-chaining `prev_hash` onto the current central tail read inside the
  write tx. Evidence: `Ledger::rollup_from` `ledger/src/v1.rs:730-803` (doc `:712-729`; tail read
  `:753`, fresh seq `:766`, insert `:778`). "Chains are NEVER merged" `:729`.

- **CLAIM D4-8 (high):** The rollup feed is **`hf sync` Part C**, fired at session-end / post-merge
  (a one-shot pass, no daemon). For each member with a local `.handoff/ledger.db` it pulls
  `events_after(cursor)` and calls `central.rollup_from(member, &rows, now)`. Evidence:
  `part_c_rollup` `hf/src/sync.rs:69-152` (pull `:125`, rollup `:144`); verb wiring
  `hf/src/main.rs:3283-3301`; module doc `hf/src/sync.rs:13-18`.

- **CLAIM D4-9 (high):** Rollup is **idempotent and incremental** via two independent backstops:
  (1) per-origin high-water mark `SYNC_CURSOR: origin_repo → (last_seq, updated_ns)`
  (`ledger/src/v1.rs:137-138`, read `sync_cursor_get` `:450-454`, advanced inside the same rollup
  tx `:790-792`); and (2) a UNIQUE `ORIGIN_INDEX (origin_repo, origin_seq) → central_seq` that
  skips-and-counts an already-rolled row even if the cursor is wrong (`ledger/src/v1.rs:759-761`).
  Whole batch + cursor advance commit in ONE two-phase-commit tx (`:743-744`, `:797`). Tested:
  `hf/src/sync.rs:472-510` (idempotent re-run = 0 new; incremental = exactly the 2 new).

- **CLAIM D4-10 (high):** `events_after(after_seq)` returns self-contained rows ordered by `seq`
  (redb big-endian key range scan), so the central ledger never re-opens the source mid-transaction.
  Evidence: `ledger/src/v1.rs:688-698` (`range((after_seq+1)..)`, doc `:683-687`).

- **CLAIM D4-11 (high):** The central ledger is never rolled into itself: `part_c_rollup`
  canonicalizes each source path and skips it when it equals the central path. Evidence:
  `hf/src/sync.rs:105-110`. Git-text-only members (no `.handoff/ledger.db`) are silently skipped
  (`:102-104`).

### D. Rollup-provenance integrity (the verification half)

- **CLAIM D4-12 (high):** Every rolled-up central row carries provenance triple
  `(origin_repo, origin_seq, origin_action_hash)`, where `origin_action_hash` is recomputed at
  rollup time from the SAME inputs as the source and is therefore byte-identical to the source's
  own `action_hash`. Evidence: `EventBody` stamping in `rollup_from` `ledger/src/v1.rs:763-776`
  (`hash_action(...)` then `origin_action_hash: Some(action_hash)`).

- **CLAIM D4-13 (high):** `verify_rollup_provenance` re-derives each rolled row's action hash and
  byte-compares it to the stored `origin_action_hash`; `mismatched == 0` is the faithfulness gate.
  Evidence: `ledger/src/v1.rs:853-892` (recompute `:882`, compare `:883`), `RollupProvenance::is_faithful`
  `ledger/src/v1.rs:281-283`. Native (NULL-origin) local events are out of scope (`:862`).

- **CLAIM D4-14 (high):** The provenance hash is `SHA3-256(event_type ‖ work_order_id ‖
  payload_json)` — `hash_action`, `ledger/src/v1.rs:291-297`. **Nuance (medium):** it does NOT
  include `prev_hash`, `ts_ns`, or `seq`, so provenance proves the central row mirrors the *content*
  of a source event, but does NOT by itself prove the rolled row sits at the right *position* in the
  source chain. Chain-position integrity is covered separately by the per-repo witness chain
  (D4-15(ii)), so jointly the two checks are sound; in isolation provenance is a content-identity
  proof. This is a real, defensible design seam worth naming.

- **CLAIM D4-15 (high):** `hf fleet status` re-verifies **three independent integrity layers**:
  (i) the central witness chain (`fleet_ledger_stats` → `verify_witness_chain`, `hf/src/fleet.rs:260-274`;
  chain at `ledger/src/v1.rs:829-846`); (ii) each member's per-repo chain standalone
  (`per_repo_chain_stats`, `hf/src/fleet.rs:127-141`); (iii) rollup-provenance faithfulness
  (`fleet_provenance` → `verify_rollup_provenance`, `hf/src/fleet.rs:279-288`). A broken bridge is
  surfaced as a WARNING, not swallowed (`hf/src/fleet.rs:335-343`). Matches ADR-0004 §4
  (`docs/adr-0004-fleet-handoff-rollout.md:71-76`). Integration-tested at `hf/src/fleet.rs:662-715`.

- **CLAIM D4-16 (medium):** Member-card loading was hardened against a fail-OPEN bug: a member card
  that fails to read/parse no longer silently vanishes from the rollup — it routes through the
  kernel's LOUD, schema-validated `parse_card_file`. Evidence: `load_member_tasks`
  `hf/src/fleet.rs:489-510` (comment `:499-503` cites the card-#95 fail-open class).

### E. P7 residency policy + ADR-0018 D1 inversion

- **CLAIM D4-17 (high):** The P7 *primary* gate is **inverted** per ADR-0018 D1: a member that has a
  local binary ledger on disk MUST git-track its deterministic text export
  `.handoff/ledger.events.jsonl`; if that export is missing, it is a violation
  (`jsonl_export_missing`). Evidence: `hf/src/fleet.rs:232-235` (condition), `:309-314` (warning),
  `git_tracks_jsonl_export` `:164-177`, `local_ledger_on_disk` `:181-183`. Module doc `:8-18`.
  Tested `hf/src/fleet.rs:797-841` (`p7_inversion_requires_tracked_jsonl_export`).

- **CLAIM D4-18 (high):** A git-**TRACKED** binary `.db` under `.handoff` is still BANNED (the
  committed-binary-ledger violation). It is detected by asking git, not the filesystem:
  `git ls-files -- .handoff` and matching `.db` / `.db-wal` / `.db-shm`. A gitignored `.db` merely
  present on disk is LEGITIMATE. Evidence: `git_tracks_handoff_db` `hf/src/fleet.rs:148-160`;
  `tracked_ledger` `:230`; warning `:315-320`; module doc `:17-21`. Tested `:719-792`
  (force-tracking is the violation; gitignored-on-disk is not).

- **CLAIM D4-19 (high):** The `.gitignore` residency guard is required: `git check-ignore -q
  .handoff/ledger.db` must succeed (`ledger_guard_present`, `hf/src/fleet.rs:190-202`,
  `ledger_guard_missing` `:236`), and a second-tier guard requires the WAL/SHM sidecar patterns
  (`walshm_guard_present` `:208-218`, `:237-240`). Both surface as P7 warnings (`:321-332`).

- **CLAIM D4-20 (high):** ADR-0004 §6 / §3.3-rev is the governing policy text: a *git-committed*
  binary ledger is banned; a *gitignored* local `.handoff/ledger.db` is the legitimate per-repo
  source of record that rolls up to the central FLEET ledger. Evidence:
  `docs/adr-0004-fleet-handoff-rollout.md:39-66, 80-87`.

### F. Member packet rendering (the cross-repo board's per-member view)

- **CLAIM D4-21 (high):** `hf fleet render <member>` compiles a member's packet from the FLEET
  ledger + that member's git-text capsule/cards (there is no per-repo ledger to render from in the
  rollup model). The North Star comes from the member's `capsule.json`, never hardcoded —
  deliberately avoiding the ADR-0006 `cmd_handoff` portability bug. Evidence:
  `render_member_packet` `hf/src/fleet.rs:515-559`, `compose_member_packet` `:570-611`, verb wiring
  `hf/src/main.rs:3488-3496`, capsule-driven test `hf/src/fleet.rs:641-657`.

---

## GAPS / OPEN QUESTIONS

- **GAP D4-G1 (medium):** The on-disk `.handoff/fleet/<member>/capsule.json` store (21 dirs:
  Archon, ECC, RuVector, vox, weave, …) is **NOT read by `fleet.rs`** — verified: `grep "fleet/"
  hf/src/fleet.rs` returns nothing, and the rollup/render code reads `root.join(member)/.handoff`
  (the actual sibling repo dir), not `.handoff/fleet/`. That capsule snapshot store is maintained by
  the fleet-steward skill/agents (`.claude/agents/fleet-steward.md`, `.claude/skills/fleet-handoff/`),
  not the kernel. So "the fleet directory" and "what the rollup aggregates" are two different things
  — a real source of confusion worth flagging to the synthesizer.

- **GAP D4-G2 (medium):** `.handoff/fleet/PILOT.toml` restricts rollout scope to `flexnetos_runner`,
  but it is read by skills/agents only — **no Rust code parses or enforces it** (grep of `hf/`
  finds no `PILOT` reference). So the pilot gate is a *process* control, not a *kernel* control;
  `hf fleet status` / `hf sync` will roll up any member with a local ledger regardless of PILOT scope.

- **GAP D4-G3 (low):** `hf sync` Part C is the only writer of the central rollup and is wired to
  session-end / post-merge hooks; I did not runtime-confirm the hook actually fires Part C in
  practice (static reading only). A verifier could run `hf sync --dry-run` at the meta root and
  diff would-roll counts against `hf fleet status` event totals.

- **GAP D4-G4 (low):** Provenance content-vs-position seam (D4-14): worth a verifier check that a
  reordered-but-content-identical rollup is caught by *some* layer (it should be caught by the
  per-repo `origin_seq` UNIQUE index + the central witness chain, but I did not construct the
  adversarial case).

## CROSS-DIMENSION HOOKS

- Ledger substrate (witness chain, redb, `hash_action`, `EventBody` provenance fields) is shared
  with the ledger/contract dimension — `ledger/src/v1.rs`.
- `hf sync` Part B (kb mirror, one-way ledger→kb) and Part A (registration) overlap the kb-seam /
  meta-sync dimension — `hf/src/sync.rs:154-351`.
- P7 git-truth checks reuse the same `git ls-files`/`check-ignore` discipline as the durability /
  gitignore-repair dimension (`durability.rs`, `cmd_gitignore`).
