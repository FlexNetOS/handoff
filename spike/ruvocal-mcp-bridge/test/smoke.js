import assert from "node:assert";
import { start } from "../index.js";

async function main() {
  const server = await start(0);
  const { port } = server.address();
  const baseUrl = `http://localhost:${port}`;

  async function rpc(id, method, params) {
    const res = await fetch(`${baseUrl}/mcp`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id, method, params }),
    });
    assert.strictEqual(res.status, 200);
    return res.json();
  }

  const init = await rpc(1, "initialize", {});
  assert.strictEqual(init.jsonrpc, "2.0");
  assert.strictEqual(init.id, 1);
  assert.ok(init.result?.serverInfo?.name);

  const tools = await rpc(2, "tools/list");
  assert.ok(Array.isArray(tools.result?.tools));
  const names = tools.result.tools.map((t) => t.name);
  assert.ok(names.includes("hf_prompt_hub"), `expected hf_prompt_hub in ${names.join(", ")}`);
  assert.ok(names.includes("hf_status"));
  assert.ok(names.includes("hf_delivery_get"));

  console.log("smoke: ok");
  server.close();
  process.exit(0);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
