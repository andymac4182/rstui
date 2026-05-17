// Parity test: every TypeScript plugin emits the same key actions as its
// Rust counterpart (mirrors crates/rstui-acp-client/tests/plugins.rs).
//   RUNTIME=node|bun TRANSPORT=stdio|ws node sdk/v8-host/test/plugins-ts.mjs
import { startPlugin } from "../../bench/lib.mjs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const dir = resolve(here, "../../plugins");
const runtime = process.env.RUNTIME || "node";
const transport = process.env.TRANSPORT || "stdio";
const cmd = runtime === "bun" ? process.env.BUN || "bun" : process.execPath;
let port = 41000 + Math.floor(Math.random() * 4000);

const init = {
  jsonrpc: "2.0",
  id: 1,
  method: "initialize",
  params: { type: "init", api_version: "1", client: "parity", cwd: here },
};
const N = (method, params) => ({ jsonrpc: "2.0", method, params });

async function run(file, steps, checks) {
  const p = await startPlugin({
    cmd,
    args: [resolve(dir, file)],
    transport,
    wsPort: port++,
  });
  p.send(init);
  await p.waitFor((m) => m.id === 1 && m.result?.ok === true, `${file} ack`);
  for (const s of steps) {
    if (s.wait) await p.waitFor(s.wait, `${file}:${s.label}`);
    if (s.send) p.send(s.send);
  }
  await new Promise((r) => setTimeout(r, 150));
  for (const c of checks) {
    if (!p.lines.some(c.pred))
      throw new Error(`${file}: missing ${c.label}\n${JSON.stringify(p.lines)}`);
  }
  p.send(N("shutdown", { type: "shutdown" }));
  await new Promise((r) => setTimeout(r, 80));
  p.kill();
}

const has = (method, extra = () => true) => (m) =>
  m.method === method && extra(m.params || {});

try {
  await run("powerline.plugin.mjs", [], [
    { label: "ui/footer", pred: has("ui/footer") },
    { label: "ui/log", pred: has("ui/log") },
  ]);
  await run(
    "session.plugin.mjs",
    [
      { send: N("command/invoke", { type: "command", name: "session", args: "" }) },
      { wait: has("ui/modal"), label: "modal" },
      {
        send: N("modal/response", {
          type: "modal_response",
          id: 1,
          button: "Reset",
          cancelled: false,
        }),
      },
    ],
    [
      { label: "register session", pred: has("commands/register", (p) => p.name === "session") },
      { label: "ui/footer", pred: has("ui/footer") },
      { label: "ui/modal", pred: has("ui/modal") },
      { label: "reset note", pred: has("ui/note", (p) => /reset/.test(p.text)) },
    ],
  );
  await run(
    "fortune.plugin.mjs",
    [{ send: N("session/turnEnded", { type: "turn_ended", stop_reason: "EndTurn" }) }],
    [
      { label: "register fortune", pred: has("commands/register", (p) => p.name === "fortune") },
      { label: "keybinding", pred: has("ui/registerKeybinding", (p) => p.keys === "ctrl+y") },
      { label: "ui/note 🥠", pred: has("ui/note", (p) => /🥠/.test(p.text)) },
      { label: "ui/panel", pred: has("ui/panel", (p) => p.title === "Fortune") },
    ],
  );
  await run(
    "pomodoro.plugin.mjs",
    [
      { send: N("command/invoke", { type: "command", name: "pomodoro", args: "1" }) },
      { send: N("tick", { type: "refresh" }) },
    ],
    [
      { label: "register pomodoro", pred: has("commands/register", (p) => p.name === "pomodoro") },
      { label: "started note", pred: has("ui/note", (p) => /pomodoro started/.test(p.text)) },
      { label: "ui/footer", pred: has("ui/footer") },
    ],
  );
  await run(
    "btw.plugin.mjs",
    [{ send: N("command/invoke", { type: "command", name: "btw", args: "remember milk" }) }],
    [
      { label: "register btw", pred: has("commands/register", (p) => p.name === "btw") },
      { label: "noted", pred: has("ui/note", (p) => /noted privately/.test(p.text)) },
      { label: "status", pred: has("ui/setStatus", (p) => p.key === "btw") },
      { label: "panel", pred: has("ui/panel", (p) => p.title === "BTW notes") },
    ],
  );
  await run(
    "ask-user.plugin.mjs",
    [
      { send: N("command/invoke", { type: "command", name: "ask", args: "Ship it?" }) },
      { wait: has("ui/askUser"), label: "askUser" },
      {
        send: N("askUser/response", {
          type: "ask_response",
          id: 1,
          selections: ["Yes, continue"],
          text: "",
          cancelled: false,
        }),
      },
    ],
    [
      { label: "register ask", pred: has("commands/register", (p) => p.name === "ask") },
      { label: "ui/askUser", pred: has("ui/askUser") },
      { label: "answer note", pred: has("ui/note", (p) => /ask-user →/.test(p.text)) },
    ],
  );
  await run(
    "git.plugin.mjs",
    [{ send: N("command/invoke", { type: "command", name: "git", args: "" }) }],
    [
      { label: "register git", pred: has("commands/register", (p) => p.name === "git") },
      { label: "ui/footer", pred: has("ui/footer") },
      { label: "ui/panel Git", pred: has("ui/panel", (p) => p.title === "Git") },
    ],
  );
  await run(
    "history.plugin.mjs",
    [
      { send: N("session/prompt", { type: "user_prompt", text: "hello agent" }) },
      { send: N("command/invoke", { type: "command", name: "history", args: "" }) },
    ],
    [
      { label: "register history", pred: has("commands/register", (p) => p.name === "history") },
      { label: "status", pred: has("ui/setStatus", (p) => p.key === "history") },
      { label: "panel", pred: has("ui/panel", (p) => p.title === "Prompt history") },
    ],
  );
  console.log(`PARITY OK — 8/8 TS plugins  (${runtime} / ${transport})`);
  process.exit(0);
} catch (e) {
  console.error(`PARITY FAIL (${runtime}/${transport}): ${e?.message ?? e}`);
  process.exit(1);
}
