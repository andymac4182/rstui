// Proves a TS plugin is a first-class *process*: run the sample directly
// (`node clock.plugin.mjs`) with NO V8 host — the SDK's built-in stdio
// bridge speaks JSON-RPC 2.0 itself. Also exercises the recommended
// zero-dep hardening (Node Permission Model) when RUN_HARDENED=1. No deps.
// Run: node sdk/v8-host/test/standalone.mjs   (exit non-zero on failure).

import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const plugin = resolve(here, "../../examples/clock.plugin.mjs");

const args = [];
if (process.env.RUN_HARDENED === "1") {
  // Node's built-in capability sandbox — no extra dependency.
  args.push("--permission", `--allow-fs-read=${resolve(here, "../..")}`);
}
args.push(plugin);

const child = spawn(process.execPath, args, {
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
async function waitFor(pred, what, t = 8000) {
  const s = Date.now();
  for (;;) {
    const h = lines.find(pred);
    if (h) return h;
    if (Date.now() - s > t)
      throw new Error(`timeout: ${what}; got ${JSON.stringify(lines)}`);
    await sleep(25);
  }
}

try {
  send({
    jsonrpc: "2.0",
    id: 1,
    method: "initialize",
    params: { type: "init", api_version: "1", client: "standalone", cwd: "/tmp" },
  });
  await waitFor((m) => m.id === 1 && m.result?.ok === true, "init ack");
  await waitFor(
    (m) => m.method === "commands/register" && m.params.name === "clock",
    "commands/register",
  );
  send({
    jsonrpc: "2.0",
    method: "command/invoke",
    params: { type: "command", name: "clock", args: "" },
  });
  const modal = await waitFor((m) => m.method === "ui/modal", "ui/modal");
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
    "ui/note",
  );
  send({ jsonrpc: "2.0", method: "shutdown", params: { type: "shutdown" } });
  await new Promise((r) => child.on("exit", r));
  console.log(
    `STANDALONE OK${process.env.RUN_HARDENED === "1" ? " (hardened)" : ""} — ${lines.length} messages, no V8 host`,
  );
  process.exit(0);
} catch (e) {
  console.error(`STANDALONE FAIL: ${e?.message ?? e}`);
  child.kill();
  process.exit(1);
}
