// Smoke the sample plugin across RUNTIME × TRANSPORT (no deps).
//   RUNTIME=node|bun TRANSPORT=stdio|ws node sdk/v8-host/test/smoke-matrix.mjs
// Drives the full handshake / command / modal round-trip / shutdown.

import { startPlugin } from "../../bench/lib.mjs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const plugin = resolve(here, "../../examples/clock.plugin.mjs");
const runtime = process.env.RUNTIME || "node";
const transport = process.env.TRANSPORT || "stdio";
const cmd = runtime === "bun" ? process.env.BUN || "bun" : process.execPath;

try {
  const p = await startPlugin({
    cmd,
    args: [plugin],
    transport,
    wsPort: 39000 + Math.floor(Math.random() * 2000),
  });
  p.send({
    jsonrpc: "2.0",
    id: 1,
    method: "initialize",
    params: { type: "init", api_version: "1", client: "m", cwd: "/tmp" },
  });
  await p.waitFor((m) => m.id === 1 && m.result?.ok === true, "init ack");
  await p.waitFor(
    (m) => m.method === "commands/register" && m.params.name === "clock",
    "commands/register",
  );
  p.send({
    jsonrpc: "2.0",
    method: "command/invoke",
    params: { type: "command", name: "clock", args: "" },
  });
  const modal = await p.waitFor((m) => m.method === "ui/modal", "ui/modal");
  p.send({
    jsonrpc: "2.0",
    method: "modal/response",
    params: {
      type: "modal_response",
      id: modal.params.id,
      button: "Copy",
      cancelled: false,
    },
  });
  await p.waitFor(
    (m) => m.method === "ui/note" && /copied/.test(m.params.text),
    "ui/note",
  );
  p.send({ jsonrpc: "2.0", method: "shutdown", params: { type: "shutdown" } });
  await new Promise((r) => setTimeout(r, 200));
  p.kill();
  console.log(`OK  ${runtime} / ${transport}  (${p.lines.length} msgs)`);
  process.exit(0);
} catch (e) {
  console.error(`FAIL ${runtime} / ${transport}: ${e?.message ?? e}`);
  process.exit(1);
}
