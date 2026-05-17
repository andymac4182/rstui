// End-to-end smoke test for the V8 host + TypeScript SDK over the real
// JSON-RPC 2.0 wire. No deps, no secure-exec, no network: runs the host in
// dev-fallback mode against the sample plugin and drives the full
// handshake / command / modal round-trip / shutdown. `npm test` or
// `node sdk/v8-host/test/smoke.mjs`. Exits non-zero on any failure.

import { spawn } from "node:child_process";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const host = resolve(here, "../host.mjs");
const plugin = resolve(here, "../../examples/clock.plugin.mjs");

const child = spawn(process.execPath, [host, plugin], {
  stdio: ["pipe", "pipe", "inherit"],
});

const lines = [];
let buf = "";
child.stdout.on("data", (d) => {
  buf += d.toString();
  let i;
  while ((i = buf.indexOf("\n")) >= 0) {
    const l = buf.slice(0, i).trim();
    buf = buf.slice(i + 1);
    if (l) lines.push(JSON.parse(l));
  }
});

const send = (o) => child.stdin.write(`${JSON.stringify(o)}\n`);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function waitFor(pred, what, timeoutMs = 8000) {
  const start = Date.now();
  for (;;) {
    const hit = lines.find(pred);
    if (hit) return hit;
    if (Date.now() - start > timeoutMs) {
      throw new Error(`timeout waiting for ${what}; got ${JSON.stringify(lines)}`);
    }
    await sleep(25);
  }
}

const fail = (e) => {
  console.error(`SMOKE FAIL: ${e?.message ?? e}`);
  child.kill();
  process.exit(1);
};

try {
  // 1. handshake
  send({
    jsonrpc: "2.0",
    id: 1,
    method: "initialize",
    params: { type: "init", api_version: "1", client: "smoke", cwd: "/tmp" },
  });
  const ack = await waitFor(
    (m) => m.id === 1 && m.result && m.result.ok === true,
    "initialize ack",
  );
  assert.equal(ack.jsonrpc, "2.0");

  // init handlers: command + keybinding + footer registered
  await waitFor(
    (m) => m.method === "commands/register" && m.params.name === "clock",
    "commands/register clock",
  );
  await waitFor(
    (m) =>
      m.method === "ui/registerKeybinding" && m.params.keys === "ctrl+l",
    "ui/registerKeybinding ctrl+l",
  );
  await waitFor((m) => m.method === "ui/footer", "ui/footer");

  // 2. tick refreshes the footer
  send({ jsonrpc: "2.0", method: "tick", params: { type: "refresh" } });

  // 3. invoke /clock -> plugin opens a modal and awaits the answer
  send({
    jsonrpc: "2.0",
    method: "command/invoke",
    params: { type: "command", name: "clock", args: "" },
  });
  const modal = await waitFor(
    (m) => m.method === "ui/modal" && m.params.title === "Clock",
    "ui/modal Clock",
  );
  assert.deepEqual(modal.params.buttons, ["Copy", "Close"]);

  // 4. answer the modal -> plugin resolves its await and emits a note
  send({
    jsonrpc: "2.0",
    method: "modal/response",
    params: {
      type: "modal_response",
      id: modal.params.id,
      button: "Copy",
      cancelled: false,
    },
  });
  await waitFor(
    (m) => m.method === "ui/note" && /copied/.test(m.params.text),
    "ui/note copied",
  );

  // 5. shutdown
  send({ jsonrpc: "2.0", method: "shutdown", params: { type: "shutdown" } });
  await new Promise((r) => child.on("exit", r));

  console.log(`SMOKE OK — ${lines.length} JSON-RPC messages exchanged`);
  process.exit(0);
} catch (e) {
  fail(e);
}
