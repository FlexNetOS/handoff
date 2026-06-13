# Phase 8 (envctl SERVER-MODE) — kickoff + handoff to the envctl loop

**Status:** scoped, handed off (2026-06-13) · **Owner of execution:** the envctl
agenticOS-consolidation loop (`envctl/.handoff/loop/`) · **Author:** handoff
orchestrator session.

> **Why this is a handoff, not an implementation.** Phase 8 is envctl's **security trust
> boundary** — the credential data-plane. It is governed by a THREAT-MODEL + an ordered
> audit spec, its remaining items are TLS/DPoP/replay crypto, and its own doc says it
> *"needs a design spike first."* Implementing it ad-hoc from a handoff session would be
> the exact unsafe shortcut this kernel exists to prevent. handoff's job was to define
> the **contract** (ADR-0010 + `scripts/grit-shared.sh`); execution belongs to envctl's
> loop with full security care.

## What Phase 8 unblocks (the dependency chain)

```
envctl Phase 8 (secretctl run data-plane: injection_template wired)
   └─ unblocks → handoff ADR-0010 shared grit backend (scripts/grit-shared.sh)
        └─ unblocks → cross-repo grit symbol coordination (vs within-repo only today)
   └─ unblocks → the secrets charter broadly: `envctl run -- <tool>` for every
                 meta tool that needs API keys (teri LLM_API_KEY, grit S3/Azure, …)
```
Until Phase 8 lands, `scripts/grit-shared.sh` degrades to local grit (verified), and
fleet coordination is within-repo only.

## Grounded scope (verified against the code + specs, 2026-06-13)

**The `secretctl run` data-plane is the handoff-relevant slice:**
- `crates/secretctl/src/main.rs` `Cmd::Run(_)` → hard stub:
  `"env-ctl run is not wired in Phase 6 (data-plane is Phase 8)"` (verified live).
- `crates/secrets-engine/src/inject.rs`:
  - `injection_template(provider, bearer, proxy, ca_pem_path) -> ResolvedInjection` → **`todo!()`**.
    This is the per-provider env-delta table across 3 `DataPlaneMode`s
    (`BaseUrlRepoint`, `HttpsProxyMitm`, `NativeSubtoken`). The real key MUST NOT enter
    the child env — only an ephemeral, peer-pid-bound bearer (HF-8).
  - `discover_profile(cwd, trusted_roots) -> Option<PathBuf>` → **`todo!()`** (fail-closed
    trust-root profile discovery, FS-S15).
- `crates/secretd/src/grpc.rs:340` → `injection: None  // not wired in Phase 6`.

**The broader Phase 8 (SERVER-MODE) remaining items — ordered by `docs/secrets/audits/
AUDIT-server-mode.md §5`** (NOT all needed for the data-plane, but they share the plane):
- **F2** — in-process TLS + DPoP/EKM listener ("the only thing that *serves*", now unblocked).
- **F6** — bounded jti replay store.
- **F5** — streaming-revocation tear-down.
- **F14** — `PresenceGate` egress-gate refactor.
- **F7–F9** — VPS Profile-B operator-authorizer gates.

Already landed (per `docs/HANDOFF-kasetto-env-and-phase8.md`): the `decide()` remote-binding
enforcement core, F12 (plane-bound row MAC), F15 (remote-client registry + mint).

## Execution plan (for the envctl loop — design-spike-first)

1. **Design spike** (REQUIRED before code): pick the data-plane mode for the local fleet
   case. For grit's S3/Azure backend the cleanest is **`NativeSubtoken`** (inject
   `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY` as an ephemeral, peer-pid-bound bearer) —
   no MITM proxy needed for the grit case. Confirm against THREAT-MODEL + AUDIT §5.
2. **Implement `injection_template`** for `NativeSubtoken` first (the grit/secrets-charter
   path), real-key-never-in-child invariant table-tested. Defer `HttpsProxyMitm` (needs
   the F2 TLS edge) and `BaseUrlRepoint` to their own slices.
3. **Implement `discover_profile`** (fail-closed, trusted-roots only, FS-S15).
4. **Wire `secretd` grpc** to return the injection template; **wire `Cmd::Run`** to
   resolve → spawn the child with the overlaid env (real key excluded).
5. **Verify** (envctl's own gates: `ci/gates/{no-c,shape,enable}.sh` + the secrets/engine
   test suites + `secretctl run -- env | grep <injected>` proves the seam).
6. On landing, **flip handoff**: `scripts/grit-shared.sh` stops degrading; register the
   `grit-backend` provider; `grit config set-s3` per repo → cross-repo coordination live.

## Handoff signal

- **Execution owner:** envctl loop — resume from `envctl/.handoff/loop/HANDOFF.md` and
  this kickoff; the authoritative ordered spec is `envctl/docs/secrets/audits/
  AUDIT-server-mode.md §5`.
- **handoff-side trigger to re-activate ADR-0010:** when `injection_template` lands and
  `secretctl run -- <cmd>` injects (no longer the Phase-6 stub), re-verify
  `scripts/grit-shared.sh` no longer degrades, then enable the shared backend fleet-wide.
- **Do NOT** implement the secrets data-plane from a non-envctl session — security
  boundary, design-spike-gated, owner/loop-owned.
