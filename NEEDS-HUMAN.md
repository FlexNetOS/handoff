# NEEDS-HUMAN — HFTASK-0001

## Blocked step: create + push FlexNetOS/handoff GitHub repo

The local portion of HFTASK-0001 is **done and committed** (rename to
Continuity Ledger Kernel, drop Ark/V1/V2, PRD file renamed, `cargo test`
green, initial commit `06432b5`).

The final objective item — creating the GitHub repo and pushing — was
**denied by the Claude Code permission classifier**. It treats creating a new
external GitHub repo and pushing the whole working tree as an outward-facing
action requiring explicit human approval. This is a genuine human wall, not a
transient/retryable failure, so it is not auto-retried.

### To unblock, run this yourself (gh is already authed as `drdave-flexnetos`):

```bash
cd ~/Desktop/meta/handoff
gh repo create FlexNetOS/handoff --private --source=. --remote=origin \
  --description "Continuity Ledger Kernel — Rust-native, repo-local handoff kernel for AI coding agents (hf CLI)" \
  --push
```

Or run `! gh repo create ...` directly in the Claude Code prompt to keep it in-session.

### After the repo exists (per ~/Desktop/meta/CLAUDE.md — new crates):
- Add `handoff` to `~/Desktop/meta/.meta.yaml` projects.
- Add the `handoff/` dir to `~/Desktop/meta/.gitignore` (child repos are not part of the parent).

Once pushed, HFTASK-0001 is fully satisfied; next safe task is HFTASK-0002
(wire weave leases into `hf claim`).
