# Branch reconciliation — 2026-06-21

Durable carry-forward record (url + SHA + verdict) for the stale/divergent branches
audited and reconciled on 2026-06-21. Per the owner rule — **carry stale refs forward with
url + SHA, never lose a ref; upgrade only, never downgrade** — every branch is recorded here
before it is pruned, so its head commit is recoverable from the SHA even after the ref is
removed.

## Context

The 2026-06-18 commit `ac5385f` ("Implement IntentLock 5-field… + all remaining tasks", via
Rust-Rover combining ~10 branches) imported the entire **rusty-idd** project as
`crates/{cli,core,runner,spec,tui}` with a broken workspace layout + monolithic ledger, which
broke `develop`. Recovery happened on `develop` (PRs #90/#69/#89/#95/#96 → `27cad6e`), leaving
several parallel recovery branches behind. An exhaustive hunk-by-hunk audit vs `develop`
classified each branch's content as **branch-ahead (value to merge)** or **develop-ahead
(superseded)**.

## Value recovered and MERGED (upgrade-only, fix-in-place)

| Source branch | Head SHA | Value | Merged |
|---|---|---|---|
| `wip/orphaned-runner-retry-on-failure` | `f67fd32` (→ brought current `43d33f0`) | `retry_on_failure` wired into the stall loop + test (develop had a **dead config field**) | **PR #97 → `683da35`** |
| `HFTASK-0014-foundation` | `8a3c7f3` | full intent-driven OpenSpec `schema.yaml` authoring instructions (develop had trimmed them to one-liners) | **PR #99 → `c7b6444`** |
| (this session) `feat/hftask-0058-durability-policy` | `e3d21a6` | HFTASK-0058 canonical `.handoff` durability policy + `hf gitignore` swallow-guard (ADR-0016) | **PR #98 → `96ba987`** |

## Confirmed develop-ahead / superseded — no unmerged value

Audited hunk-by-hunk; every delta is a pre-refactor/pre-revision version `develop` has since
superseded (recorded here so the refs are carried forward, then pruned):

| Branch | Head SHA | Why superseded |
|---|---|---|
| `fix/windows-ledger-path-and-promote-checkout` | `91d430b` | strict subset of `develop` (0 unique files, 0 unique ledger symbols); broken root workspace layout |
| `adr-0006-portability` | `8407cf5` | only unique file was derived `active.md`; carries pre-HFTASK-0047 3-field IntentLock + superseded §6 residency doctrine; ADR-0006 intentionally pruned on develop |
| `handoff-HFTASK-0031-ledger-provenance` | `1c7a97f` | HFTASK-0031 Done on develop; carries the **superseded** "no per-repo ledger.db" doctrine (develop has the revised §6 "gitignored per-repo ledger is legitimate") |
| `feat/hftask-0048-atomic-leases` | `103a515` | HFTASK-0048 Done on develop; same superseded residency doctrine |
| `copilot/fix-develop-workspace` | `27beaab` | subset of `develop` (monolith ledger + old bash test fixtures) |
| `copilot/fix-hftask-0014-manifest` | `181174c` | superseded gatekeeper/ledger (develop has the final HFTASK-0014 gatekeeper + busy-retry) |

## Spent-source branches (value extracted, safe to prune)

| Branch | Head SHA | Disposition |
|---|---|---|
| `wip/orphaned-runner-retry-on-failure` | `43d33f0` | value merged (#97) |
| `HFTASK-0014-foundation` | `8a3c7f3` | schema value merged (#99); remainder superseded |
| `feat/hftask-0058-durability-policy` | `e3d21a6` | merged (#98) |
| `feat/0014-schema-enrichment` | `e656a09` | merged (#99) |

All branches above are recoverable from their recorded SHA (`git fetch origin <sha>` /
`https://github.com/FlexNetOS/handoff/commit/<sha>`).
