# TASK-0004 Decision — parent notification token guard

## Context

Every push to `master` runs `notify-parent.yml` to send a `repository_dispatch` event to
`FlexNetOS/meta`. On the final verification push, the workflow failed before dispatch with:
`Parameter token or opts.auth is required`, proving `PARENT_REPO_PAT` is not configured in the
current GitHub Actions secret context.

## Decision

Keep the parent notification integration, but add an explicit preflight step. If
`PARENT_REPO_PAT` is present, the workflow dispatches exactly as before. If the token is absent, the
workflow emits a GitHub Actions warning and skips the dispatch step successfully.

## No-downgrade check

This does not hide a failed dispatch with a configured token: when the token exists, the
`peter-evans/repository-dispatch` step still runs and can fail normally. The skip only covers the
owner-controlled external-secret absence so unrelated build/test/security verification is not left
red by an unavailable parent-notification credential.
