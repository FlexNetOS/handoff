#!/usr/bin/env node
/**
 * ruvocal-mcp-bridge
 *
 * Adapts the handoff `hf-mcp` stdio MCP server to the HTTP bridge protocol that
 * RuVocal's `mcp-bridge/` already speaks. RuVocal can be pointed at this bridge
 * via `MCP_SERVERS` (see `servers.json.example`) and then call:
 *
 *   - hf_prompt_hub     : turn a vibe request into a handoff task + dispatch
 *   - hf_status         : surface loop state
 *   - hf_delivery_get   : surface delivery for a correlation_id
 *   - hf_delivery_list  : list recent deliveries
 *
 * This is a spike/proof-of-concept under `handoff/spike/` (HFTASK-0022).
 */

import express from "express";
import { spawn } from "child_process";
import { dirname, join } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));

const PORT = Number(process.env.PORT || 3001);
const HF_MCP_EXE =
  process.env.HF_MCP_EXE || join(__dirname, "..", "..", "target", "debug", "hf-mcp");

let pending = Promise.resolve();

class McpStdioClient {
  constructor(exe) {
    this.exe = exe;
    this.proc = null;
    this.buffer = "";
    this.started = false;
  }

  start() {
    if (this.proc) return;
    this.proc = spawn(this.exe, [], {
      stdio: ["pipe", "pipe", "pipe"],
      env: process.env,
    });

    this.proc.on("error", (err) => {
      console.error(`[mcp-bridge] hf-mcp spawn error: ${err.message}`);
    });
    this.proc.on("exit", (code) => {
      console.error(`[mcp-bridge] hf-mcp exited with code ${code}`);
      this.proc = null;
      this.started = false;
    });
    this.proc.stderr.on("data", (chunk) => {
      // hf-mcp logs diagnostics to stderr; surface them for debugging.
      process.stderr.write(`[hf-mcp] ${chunk}`);
    });

    this.started = true;
  }

  async request(req) {
    this.start();
    return (pending = pending.then(() => this._send(req)));
  }

  _send(req) {
    return new Promise((resolve, reject) => {
      const line = JSON.stringify(req) + "\n";
      let timer;

      const onData = (chunk) => {
        this.buffer += chunk.toString("utf8");
        const lines = this.buffer.split("\n");
        this.buffer = lines.pop(); // keep partial line
        for (const l of lines) {
          if (!l.trim()) continue;
          try {
            const msg = JSON.parse(l);
            if (msg.id === req.id) {
              cleanup();
              resolve(msg);
              return;
            }
          } catch (err) {
            // ignore stray non-JSON lines
          }
        }
      };

      const cleanup = () => {
        clearTimeout(timer);
        this.proc.stdout.off("data", onData);
      };

      timer = setTimeout(() => {
        cleanup();
        reject(new Error("hf-mcp request timed out"));
      }, 30000);

      this.proc.stdout.on("data", onData);
      this.proc.stdin.write(line, (err) => {
        if (err) {
          cleanup();
          reject(err);
        }
      });
    });
  }
}

const client = new McpStdioClient(HF_MCP_EXE);

export const app = express();
app.use(express.json());

app.get("/health", (_req, res) => {
  res.json({ ok: true, hf_mcp: HF_MCP_EXE });
});

app.post("/mcp", async (req, res) => {
  try {
    const { method, params, id } = req.body;
    const response = await client.request({ jsonrpc: "2.0", id, method, params });
    res.json(response);
  } catch (err) {
    console.error(`[mcp-bridge] /mcp error: ${err.message}`);
    res.status(500).json({
      jsonrpc: "2.0",
      id: req.body?.id ?? null,
      error: { code: -32603, message: err.message },
    });
  }
});

export function start(port = PORT) {
  return new Promise((resolve) => {
    const server = app.listen(port, () => {
      const addr = server.address();
      console.log(`[mcp-bridge] listening on http://localhost:${addr.port}`);
      console.log(`[mcp-bridge] hf-mcp exe: ${HF_MCP_EXE}`);
      resolve(server);
    });
  });
}

// Start the server only when this file is the entry point (not when imported by tests).
if (import.meta.url === `file://${process.argv[1]}`) {
  start();
}
