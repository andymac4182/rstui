// Correctness smoke across every host × transport: spawn the fortune
// plugin, handshake, invoke its command, assert a ui/note comes back.
// Proves the new uds / *-lp framing + the object fast-path round-trip on
// both the Rust and the TS SDKs. Run: node sdk/bench/verify.mjs
import { startPlugin } from "./lib.mjs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import { existsSync } from "node:fs";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const tsFortune = resolve(here, "../plugins/fortune.plugin.mjs");
const rustBin = resolve(
  process.env.CARGO_TARGET_DIR || resolve(repo, "target"),
  "release/rstui-acp-plugin-fortune",
);
const NODE = process.execPath;
const BUN = process.env.BUN || `${process.env.HOME}/.bun/bin/bun`;

const impls = [];
if (existsSync(rustBin)) impls.push({ name: "rust", cmd: rustBin, args: [] });
impls.push({ name: "node", cmd: NODE, args: [tsFortune] });
if (existsSync(BUN)) impls.push({ name: "bun", cmd: BUN, args: [tsFortune] });

const TRANSPORTS = ["stdio", "stdio-lp", "uds", "uds-lp", "ws"];
const INIT = {
  jsonrpc: "2.0",
  id: 1,
  method: "initialize",
  params: { type: "init", api_version: "1", client: "verify", cwd: here },
};
const CMD = {
  jsonrpc: "2.0",
  method: "command/invoke",
  params: { type: "command", name: "fortune", args: "" },
};

let port = 49100;
let fails = 0;
for (const impl of impls) {
  for (const transport of TRANSPORTS) {
    try {
      const p = await startPlugin({ ...impl, transport, wsPort: port++ });
      p.send(INIT);
      await p.waitFor((m) => m.id === 1 && m.result?.ok === true, "ack", 8000);
      p.send(CMD);
      await p.waitFor((m) => m.method === "ui/note", "note", 8000);
      p.send({
        jsonrpc: "2.0",
        method: "shutdown",
        params: { type: "shutdown" },
      });
      await new Promise((r) => setTimeout(r, 30));
      p.kill();
      console.log(`ok   ${impl.name}/${transport}`);
    } catch (e) {
      fails++;
      console.log(`FAIL ${impl.name}/${transport}: ${e.message}`);
    }
  }
}
console.log(fails ? `\n${fails} failure(s)` : "\nall transports ok");
process.exit(fails ? 1 : 0);
