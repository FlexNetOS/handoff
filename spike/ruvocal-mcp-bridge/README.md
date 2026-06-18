# ruvocal-mcp-bridge

Spike adapter that exposes the handoff `hf-mcp` MCP server over HTTP so that
RuVocal (`meta/RuVector/ui`) can use it as an MCP tool source.

## Why this exists

HFTASK-0022 makes RuVocal the real human front door for the handoff loop. The
loop has three seams:

1. **Input** — `hf_prompt_hub` turns a natural-language vibe request into a
   `handoff.task.v1` card and optionally dispatches the first safe order.
2. **State** — `hf_status` returns the live loop read-model.
3. **Delivery** — `hf_delivery_get` / `hf_delivery_list` round-trip the outcome
   of a merged PR by `correlation_id`.

All three already live in `hf-mcp` (HFTASK-0019/0020/0021). This bridge simply
adapts `hf-mcp`'s stdio transport to the HTTP `/mcp` endpoint RuVocal already
consumes.

## Run the bridge

```bash
cd handoff/spike/ruvocal-mcp-bridge
npm install
npm start
```

The server listens on `http://localhost:3001` by default. Point the `HF_MCP_EXE`
environment variable at a built `hf-mcp` binary if it is not at
`../../target/debug/hf-mcp`.

## Wire RuVocal to it

Add the bridge to RuVocal's `MCP_SERVERS` environment variable (array of
`{ name, url }` objects):

```bash
export MCP_SERVERS='[{"name":"hf","url":"http://localhost:3001/mcp"}]'
```

RuVocal's chat UI will list the `hf_*` tools, and an autopilot loop can call
`hf_prompt_hub` to start work, then poll `hf_status` and `hf_delivery_get` to
surface progress and results back in chat.

## Test

```bash
npm test
```

The test boots the bridge, sends `initialize` + `tools/list`, and asserts that
`hf_prompt_hub` is advertised.

## Scope note

This is intentionally a spike under `handoff/spike/`. It does not modify
`meta/RuVector/ui/ruvocal` directly; that repo remains the consumer.
