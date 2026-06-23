# TASK-0002 Decision — Windows-safe envctl provider in promote-verify

## Context

`promote-verify.yml` runs after develop/master promotion and mirrors the repo's full verification
surface. Its Windows jobs cloned `FlexNetOS/envctl` directly, unlike `ci.yml` and
`ai-gatekeeper.yml`. The envctl checkout contains filenames with colons, which are invalid on
Windows and can fail before Cargo reaches the handoff code under test.

## Decision

Use the same Windows-only manifest stub for the optional `envctl-secrets-engine` path dependency
that `ci.yml` and `ai-gatekeeper.yml` already use. Non-Windows jobs still clone the real envctl
repository. This preserves Linux/macOS coverage while making the Windows verification path test the
handoff workspace instead of failing on an optional dependency checkout shape.

## No-downgrade check

The stub is limited to Windows and to the optional secrets-engine manifest needed for dependency
resolution; default and no-default handoff builds do not compile the optional secrets feature in
these jobs. The real envctl provider remains covered on non-Windows jobs.
