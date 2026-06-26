# Verdicts — D4 · Fleet-Rollup (adversarial verification)

Date: 2026-06-25
Verifier pass: refute-first. Cited lines re-read against target root
`/home/drdave/Desktop/meta/handoff`. Tally at bottom.

---

## PRIORITY (a) — Rollup provenance re-derivation + gate

**D4-13 / D4-14 — CONFIRMED.** `verify_rollup_provenance` (`ledger/src/v1.rs:853-892`)
iterates EVENTS, and for every row with `origin_repo` set, recomputes
`hash_action(&r.event_type, &r.work_order_id, &r.payload_json)` (`:882`) and byte-compares
`r.origin_action_hash == Some(recomputed)` (`:883`). `hash_action` (`ledger/src/v1.rs:291-297`)
is exactly `SHA3-256(event_type ‖ work_order_id ‖ payload)` — three `update()` calls, no
`prev_hash`/`ts_ns`/`seq`, confirming the D4-14 content-only nuance. Mismatch (or `None`
origin hash) increments `prov.mismatched` (`:887`). The faithfulness gate
`RollupProvenance::is_faithful` (`:281-283`) is `mismatched == 0`, and `cmd_fleet_status`
fires a WARNING on `!is_faithful()` (`hf/src/fleet.rs:335-343`).

Refutation attempts that FAILED:
- *"It's a tautology — compares two copies of the same stored hash."* REFUTED: rollup stores
  `action_hash` and `origin_action_hash` to the same value (`v1.rs:772,776`), but the verifier
  recomputes from the stored *content* (`event_type/work_order_id/payload_json`) and compares to
  the stored `origin_action_hash`. A central-row payload tampered without re-stamping the origin
  hash is caught. Genuine re-derivation, not a self-comparison.
- *"NULL origin hash fails open."* REFUTED: `None != Some(recomputed)` ⇒ counted as `mismatched`
  (comment `v1.rs:270-272` is explicit). Fail-closed.

QUALIFIER (already disclosed by the analyst, not a refutation): the broken-provenance result is
surfaced as a WARNING, not a process-exit/hard-fail. Claim D4-15 states this; consistent.

## PRIORITY (b) — P7 residency inversion (git-decided, not filesystem)

**D4-17 / D4-18 / D4-19 — CONFIRMED.** `collect_rows` (`hf/src/fleet.rs:220-257`):
- `jsonl_export_missing = present && has_handoff && local_ledger_on_disk(repo) &&
  !git_tracks_jsonl_export(repo)` (`:232-235`). The verdict half (`git_tracks_jsonl_export`,
  `:164-177`) is `git ls-files -- .handoff/ledger.events.jsonl`, not a filesystem read; only the
  *trigger* `local_ledger_on_disk` (`:181-183`) uses `.is_file()`, which is correct (the policy
  only applies when a local binary ledger exists).
- `tracked_ledger = present && git_tracks_handoff_db(repo)` (`:230`); `git_tracks_handoff_db`
  (`:148-160`) greps `git ls-files -- .handoff` for `.db`/`.db-wal`/`.db-shm`. A gitignored-on-disk
  `.db` is NOT flagged. Inversion holds: tracked binary banned, on-disk gitignored legitimate.
- Guards: `ledger_guard_present` (`:190-202`) = `git check-ignore -q .handoff/ledger.db`;
  `walshm_guard_present` (`:208-218`) checks `-wal`/`-shm`. Warnings wired `:309-332`.

Refutation attempt that FAILED: *"Residency decided by disk presence."* REFUTED — every verdict
predicate calls `git ls-files` / `git check-ignore`; presence is only the precondition gate.

## PRIORITY (c) — capsule.json / PILOT.toml NOT read by fleet.rs

**GAP D4-G1 — CONFIRMED.** `capsule_field` (`hf/src/fleet.rs:71-72`) reads
`repo.join(".handoff/context/capsule.json")` where `repo = root.join(member)` — the sibling
repo's OWN capsule, not the `.handoff/fleet/<member>/capsule.json` snapshot store. `grep "fleet/"
hf/src/fleet.rs` returns nothing. The on-disk `.handoff/fleet/` store is unread by the kernel.

**GAP D4-G2 — CONFIRMED.** `grep -rn "PILOT" hf/src/` finds zero references (the single
`hf/src/main.rs:2824` hit is an unrelated comment about a 4-agent fleet sweep). No Rust parses or
enforces `PILOT.toml`; pilot scope is a process/skill control, not a kernel control. `hf sync`
Part C rolls up any member with a local ledger regardless of PILOT scope (`hf/src/sync.rs:100-104`,
loops all `members`, only filter is "has a local `.handoff/ledger.db`").

---

## Remaining material claims

**D4-1 / D4-2 / D4-3 (daemonless, git-transport, hand-rolled YAML) — CONFIRMED.**
`cmd_fleet_status` (`fleet.rs:290+`) is straight-line filesystem/git/ledger I/O, no spawn/thread.
`part_c_rollup` is a one-shot `for member in &members` loop (`sync.rs:100`). Member discovery and
`parse_members` are dependency-light. (Daemonlessness verified by absence of any spawn/loop-forever
in the read path.)

**D4-5 (ledger-over-card precedence operative) — CONFIRMED.** `member_status_of`
(`fleet.rs:561-567`) returns the ledger-replay status where `k == card.id`, else `card.status`.
Drives `done`/`remaining` in `compose_member_packet` (`:578-585`). Operative, not prose.

**D4-7 (append-with-provenance re-chaining, chains never merged) — CONFIRMED.** `rollup_from`
(`v1.rs:730-803`): central tail read INSIDE the write tx (`:753`), fresh `next_seq = tail_seq+1`
(`:766`), `prev_hash` chained onto central tail (`:773,783`), insert `:778`. Doc `:729` "Chains
are NEVER merged."

**D4-9 (idempotent/incremental, two backstops, single tx) — CONFIRMED.** ORIGIN_INDEX
skip-and-count (`v1.rs:759-761`); SYNC_CURSOR advanced in the SAME `begin_write` tx (`:790-792`),
two-phase commit on (`:744`). Cursor read in Part C (`sync.rs:121-124`).

**D4-10 (events_after self-contained, seq order) — CONFIRMED.** `events_after`
(`v1.rs:688-698`): `range((after_seq+1)..)`, big-endian seq order, returns full `EventRow`
incl. `action_hash` (`body_to_row` `:701-710`).

**D4-11 (central never rolled into itself; git-text-only skipped) — CONFIRMED.** `sync.rs:105-110`
canonicalize-and-skip-self; `:102-104` skip members without a local ledger.

**D4-12 (provenance triple stamped at rollup) — CONFIRMED.** `rollup_from` stamps
`origin_repo`/`origin_seq`/`origin_action_hash` (`v1.rs:774-776`), `origin_action_hash` =
recomputed `action_hash` (byte-identical to source).

**D4-15 (three independent integrity layers + broken-bridge warning) — CONFIRMED.**
(i) `fleet_ledger_stats` → `verify_witness_chain` (`fleet.rs:260-274`; chain `v1.rs:829-846`);
(ii) `per_repo_chain_stats` (`fleet.rs:127-141`); (iii) `fleet_provenance` →
`verify_rollup_provenance` (`fleet.rs:279-288`). Broken bridge → WARNING (`:335-343`), not swallowed.

**D4-21 (member packet from FLEET ledger + member capsule; North Star not hardcoded) — QUALIFIED.**
Substantively CONFIRMED: `compose_member_packet` renders precedence/progress/remaining from the
FLEET replay + member cards, and the North Star comes from the member capsule with a neutral
fallback (`fleet.rs:526`, `:589`), avoiding the ADR-0006 hardcode bug. QUALIFIER: the cite says
"member's `capsule.json`" but the exact path read is `.handoff/context/capsule.json` (`fleet.rs:72`)
— the claim's subpath is imprecise though the substance (capsule-driven, not hardcoded) holds.

**D4-4 / D4-6 / D4-8 / D4-16 / D4-20 — CONFIRMED** (precedence prose at `fleet.rs:590`;
git-truth residency at `:148-202`; Part C feed at `sync.rs:69-152` + verb wiring;
`load_member_tasks` loud parse; ADR-0004 §6 policy text). Read and consistent with cites.

---

## TALLY (D4)

- CONFIRMED: 19  (D4-1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20 minus the one qualified
  = D4-1..20 except D4-21; plus gaps G1,G2 confirmed-as-gaps)
- QUALIFIED: 1  (D4-21 — imprecise capsule subpath; substance holds)
- REFUTED: 0
- INCONCLUSIVE: 0
- Gaps confirmed accurate (not claims): D4-G1, D4-G2 (both CONFIRMED as real gaps).

Net for synthesis: all 20 lettered claims flow forward (19 CONFIRMED + 1 QUALIFIED).
No claim REFUTED. The two disclosed seams (provenance = content-identity not chain-position;
fleet/PILOT not kernel-read) are accurate and should be carried into the report as caveats.
