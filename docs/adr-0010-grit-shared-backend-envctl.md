# ADR-0010 — envctl-backed shared grit backend for cross-repo coordination

**Status:** accepted, **implementation BLOCKED on envctl Phase 8** (2026-06-13) ·
**Owner:** handoff kernel (orchestration plane) · **Derived from:** owner directive
2026-06-13 ("build the shared grit backend"), ADR-0009 (grit adoption), envctl secrets
charter (ADR-0007 + owner intent: "envctl holds the secrets and auto-injects them when
a tool needs them"), grit 0.3.0 backend surface.

## Context

ADR-0009 adopted grit with the **local SQLite** backend — which coordinates parallel
agents **within a single repo** only (the registry is `<repo>/.grit/registry.db`).
The fleet's real need is **cross-repo** coordination: many sessions across many repos
working at once (verified live in the weave heartbeats). That requires a **shared**
lock store.

grit already supports shared backends: `grit config set-s3` (AWS / R2 / GCS / MinIO,
atomic locking via conditional PUT) and `grit config set-azure` (atomic
`If-None-Match: *` + Event Grid events). The backend **config** (bucket / endpoint /
region) is non-secret and lives in `.grit/config`; the **credentials**
(`AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY`, or `AZURE_STORAGE_*`) are read by grit
**from the environment** (grit's own docs say `export AWS_ACCESS_KEY_ID=…`).

Per the owner's charter, secrets are **never exported** — envctl holds them and
auto-injects them into a child process on demand (`secretctl run -- <cmd>`, "injected
into the child only").

## Decision

The fleet's shared grit backend is **grit's S3/Azure backend with credentials supplied
by envctl injection** — never raw `export`:

```
secretctl run --provider grit-backend -- grit <claim|done|status|…>
```

1. **Config (non-secret), committed-once per repo:** `grit config set-s3 --bucket
   <fleet-bucket> --endpoint <…> --region <…>` (or `set-azure`). Stored in
   `.grit/config` (which is under the gitignored `.grit/`).
2. **Credentials (secret), injected at runtime:** envctl owns a `grit-backend`
   provider/relay holding `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY` (or
   `AZURE_STORAGE_ACCOUNT`/`AZURE_STORAGE_ACCESS_KEY`). `secretctl run` injects them
   into grit's process env only — they never touch the shell, a file, or git.
3. **Wrapper:** `scripts/grit-shared.sh` runs any grit command under `secretctl run`
   with the grit-backend provider. It detects when injection is unavailable and
   degrades with a clear message rather than running grit with no creds.
4. **Local stays the default;** the shared backend is opt-in per repo (or fleet-wide
   once Phase 8 lands). Within-repo parallelism keeps working on local SQLite.

## BLOCKER — envctl Phase 8 (data-plane)

**`secretctl run` is not implemented yet:** it returns
`Error: 'env-ctl run' is not wired in Phase 6 (data-plane is Phase 8)` (verified live
2026-06-13). The injection data-plane this ADR depends on is envctl's **Phase 8**.
Until it lands:
- the shared backend **cannot be activated** (grit would have no creds);
- `scripts/grit-shared.sh` is shipped **ready** but degrades to a clear "shared
  backend pending envctl Phase 8 — using local backend" message;
- the fleet runs on **per-repo local SQLite** (within-repo coordination only).

This is an honest hard dependency, not a workaround: cross-repo coordination needs a
shared store, a shared store needs creds, and creds come only from the (unbuilt)
injection data-plane. Forcing it any other way (raw `export`) violates the secrets
charter.

## Research

- **grit backends** (grit 0.3.0 `grit config --help` + `src/db/azure_store.rs`,
  `src/cli/mod.rs:1262 cmd_config_set_s3`, read 2026-06-13): S3 (`--bucket/--endpoint/
  --region`, creds from `AWS_*` env), Azure (azure_storage crate, `AZURE_STORAGE_*`
  env / connection string), both with atomic locking + events; config persisted in
  `.grit/config`.
- **envctl injection** (`secretctl run --help` + live probe, 2026-06-13): `secretctl
  run [--provider/--relay/--ephemeral] -- <cmd>` "runs a command with relay credentials
  injected into the child only" — but the data-plane is **Phase 8, not wired** (probe
  returned the Phase-6 error). secrets-engine `inject.rs` owns the per-provider env
  mapping; `secretctl run` "stays dumb."
- **Cross-validation:** grit reads creds from env exactly where envctl injects them →
  the seam is clean once Phase 8 exists. No grit change needed; no handoff change
  beyond the wrapper. The local→shared move is pure config + injection.

## Cross-References

- **ADR-0009** — grit adoption (local backend); this ADR is its cross-repo upgrade.
- **ADR-0007 / envctl secrets charter** — secrets via injection, never `export`.
- **envctl Phase 8** (data-plane / `secretctl run`) — the hard dependency; this ADR
  activates when it lands.
- **FLEET_GUIDE §4b** — adds the shared-backend note (pending Phase 8).
- `scripts/grit-shared.sh` — the ready, degrading wrapper.

## Consequences

- The design + wrapper are ready now; flipping to cross-repo coordination is a config
  + provider-registration step once envctl Phase 8 ships.
- Until then, fleet coordination is within-repo (local SQLite) — correct and safe, just
  not cross-repo.
- Follow-up when Phase 8 lands: register the `grit-backend` provider in envctl, run
  `grit config set-s3/set-azure` per repo, and switch the harness to invoke grit via
  `scripts/grit-shared.sh`.
