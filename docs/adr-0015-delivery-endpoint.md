# ADR-0015: Delivery / Output Endpoint for prompt_hub Round-Trip

## Status

Accepted — implements HFTASK-0021.

## Context

ADR-0001 §13/R14 describes the agentic pipeline as:

```text
prompt_hub(input) -> process -> delivery(output)
```

The front door (`hf intake`, HFTASK-0003) already converts a prompt_hub `SwarmBundle`
into one or more `WorkOrder`s, carrying `SwarmBundle.workflow_id` as the order's
`correlation_id`. The process side (claim / checkpoint / ship / review / done) was
implemented, but the output endpoint — closing the loop back to the originating
workflow — was missing.

RuVocal / prompt_hub need a deterministic way to know that a workflow's merged cycle
has produced a result, keyed by the same `correlation_id` that entered the front door.

## Decision

On `hf done <id> --pr <N>` (the post-merge terminal signal), after the `pr_merged`
ledger event is witnessed, emit a typed `handoff.delivery.v1` record:

- `correlation_id`: the originating `SwarmBundle.workflow_id` from the `WorkOrder`.
- `task_id`: the completed handoff task.
- `pr`: the merged PR reference.
- `status`: `"merged"`.
- `summary`: human-readable one-line result.
- `url`: best-effort PR URL.
- `timestamp_ns`: event time.

The record is:

1. **Witnessed** in the local ledger as a `delivery` event (authoritative source of truth).
2. **Cached** to `.handoff/deliveries/<correlation_id>.delivery.json` for easy front-door
   polling by prompt_hub / RuVocal.

Two query verbs are added:

- `hf delivery get <correlation_id> [--json]` — return the newest delivery for a workflow.
- `hf delivery list [--json]` — list all deliveries, newest first.

The MCP server (`hf-mcp`) exposes these as `hf_delivery_get` and `hf_delivery_list`
tools, so chat / rvAgent clients can query outcomes without shell access.

## Consequences

- **Round-trip closed**: prompt_hub can correlate an output delivery with its input
  workflow using the same `correlation_id`.
- **Ledger remains authoritative**: the artifact file is a convenience cache; the
  `delivery` ledger event is the tamper-evident record.
- **No network in core path**: delivery emission is local file/ledger IO, honoring
  `allows_network: false`.
- **Best-effort URL**: numeric PRs and `owner/repo#N` shapes are resolved to
  `github.com` URLs; already-absolute URLs pass through; unknown shapes return `None`.
- **MCP coverage extended**: the delivery endpoint is reachable via the T11 universal
  control seam.

## Related

- ADR-0001 §13/R14 (pipeline input/process/output)
- HFTASK-0003 (front-door intake)
- HFTASK-0019 (MCP server exposing `hf` verbs)
