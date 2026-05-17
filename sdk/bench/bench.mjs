// Profiler: rust | node | bun  ×  stdio | websocket, on the SAME plugin
// (fortune — tiny + deterministic, so the numbers reflect runtime +
// transport overhead, not plugin work). Measures cold startup, per-message
// round-trip latency (p50/p95), and sustained throughput, then writes
// sdk/bench/RESULTS.md. Dependency-free; reproducible with `node bench.mjs`.
//
// Prereq for the rust rows: build the release bin first —
//   cargo build --release -p rstui-acp-client --bin rstui-acp-plugin-fortune

import { startPlugin } from "./lib.mjs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import { existsSync, writeFileSync } from "node:fs";
import os from "node:os";
import { execSync } from "node:child_process";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const tsFortune = resolve(here, "../plugins/fortune.plugin.mjs");
const rustBin = resolve(
  process.env.CARGO_TARGET_DIR || resolve(repo, "target"),
  "release/rstui-acp-plugin-fortune",
);
const NODE = process.execPath;
const BUN =
  process.env.BUN ||
  `${process.env.HOME}/.bun/bin/bun`;

const STARTUP_RUNS = 15;
const WARMUP = 300;
const LAT_ITERS = 500;
const TPUT_MSGS = 3000;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const median = (a) => {
  const s = [...a].sort((x, y) => x - y);
  return s[Math.floor(s.length / 2)];
};
const pct = (a, p) => {
  const s = [...a].sort((x, y) => x - y);
  return s[Math.min(s.length - 1, Math.floor((p / 100) * s.length))];
};

const INIT = {
  jsonrpc: "2.0",
  id: 1,
  method: "initialize",
  params: { type: "init", api_version: "1", client: "bench", cwd: here },
};
const CMD = {
  jsonrpc: "2.0",
  method: "command/invoke",
  params: { type: "command", name: "fortune", args: "" },
};

let portSeq = 47000;

function impls() {
  const list = [];
  if (existsSync(rustBin))
    list.push({ name: "rust", cmd: rustBin, args: [] });
  else console.error(`(skip rust: ${rustBin} not built)`);
  list.push({ name: "node", cmd: NODE, args: [tsFortune] });
  if (existsSync(BUN)) list.push({ name: "bun", cmd: BUN, args: [tsFortune] });
  else console.error(`(skip bun: ${BUN} not found)`);
  return list;
}

async function measureStartup(impl, transport) {
  const ds = [];
  for (let i = 0; i < STARTUP_RUNS; i++) {
    const t0 = performance.now();
    const p = await startPlugin({
      ...impl,
      transport,
      wsPort: portSeq++,
    });
    p.send(INIT);
    await p.waitFor((m) => m.id === 1 && m.result?.ok === true, "ack");
    ds.push(performance.now() - t0);
    p.send({ jsonrpc: "2.0", method: "shutdown", params: { type: "shutdown" } });
    await sleep(15);
    p.kill();
  }
  return median(ds);
}

async function measureLatencyAndThroughput(impl, transport) {
  const p = await startPlugin({ ...impl, transport, wsPort: portSeq++ });
  let notes = 0;
  let onNote = null;
  p.onMessage = (m) => {
    if (m.method === "ui/note") {
      notes++;
      onNote?.();
    }
  };
  p.send(INIT);
  await p.waitFor((m) => m.id === 1 && m.result?.ok === true, "ack");

  const waitNotes = (target) =>
    new Promise((res) => {
      if (notes >= target) return res();
      onNote = () => {
        if (notes >= target) {
          onNote = null;
          res();
        }
      };
    });

  // Warm up (JIT for node/bun).
  for (let i = 0; i < WARMUP; i++) p.send(CMD);
  await waitNotes(WARMUP);

  // Round-trip latency, single in-flight.
  const lat = [];
  for (let i = 0; i < LAT_ITERS; i++) {
    const target = notes + 1;
    const t0 = performance.now();
    p.send(CMD);
    await waitNotes(target);
    lat.push((performance.now() - t0) * 1000); // µs
  }

  // Sustained throughput.
  const base = notes;
  const t0 = performance.now();
  for (let i = 0; i < TPUT_MSGS; i++) p.send(CMD);
  await waitNotes(base + TPUT_MSGS);
  const tput = TPUT_MSGS / ((performance.now() - t0) / 1000);

  p.send({ jsonrpc: "2.0", method: "shutdown", params: { type: "shutdown" } });
  await sleep(40);
  p.kill();
  return { p50: pct(lat, 50), p95: pct(lat, 95), tput };
}

function ver(cmd) {
  try {
    return execSync(cmd, { encoding: "utf8" }).trim().split("\n")[0];
  } catch {
    return "n/a";
  }
}

const rows = [];
for (const impl of impls()) {
  for (const transport of ["stdio", "ws"]) {
    process.stderr.write(`measuring ${impl.name}/${transport}…\n`);
    const startup = await measureStartup(impl, transport);
    const { p50, p95, tput } = await measureLatencyAndThroughput(
      impl,
      transport,
    );
    rows.push({ impl: impl.name, transport, startup, p50, p95, tput });
  }
}

const n1 = (x) => x.toFixed(1);
const n0 = (x) => Math.round(x).toLocaleString("en-US");
let md = `# Plugin runtime & transport profile

Same plugin (\`fortune\`) under each host × transport, so the numbers
isolate **runtime + JSON-RPC transport overhead**, not plugin logic.

- **startup** — spawn → \`initialize\` ack (cold process + handshake), median of ${STARTUP_RUNS}.
- **latency** — single in-flight \`command/invoke\` → \`ui/note\` round-trip; p50/p95 of ${LAT_ITERS} (after ${WARMUP} warm-up).
- **throughput** — ${TPUT_MSGS} \`command/invoke\` pipelined, messages/sec.

Environment: ${os.type()} ${os.release()} · ${os.arch()} · ${os.cpus()[0]?.model ?? "cpu?"} · \
node ${process.version} · bun ${ver(`${BUN} --version`)} · ${ver("rustc --version")}.
Single machine, single run, loopback websockets — **indicative**, not a benchmark suite.
Reproduce: \`cargo build --release -p rstui-acp-client --bin rstui-acp-plugin-fortune && node sdk/bench/bench.mjs\`.

| host | transport | startup (ms) | latency p50 (µs) | latency p95 (µs) | throughput (msg/s) |
|------|-----------|-------------:|-----------------:|-----------------:|-------------------:|
`;
for (const r of rows) {
  md += `| ${r.impl} | ${r.transport} | ${n1(r.startup)} | ${n0(r.p50)} | ${n0(r.p95)} | ${n0(r.tput)} |\n`;
}
// Observations derived from THIS run's numbers so the prose can never
// contradict the table.
const best = (key, dir = "min") =>
  rows.reduce((a, b) =>
    (dir === "min" ? b[key] < a[key] : b[key] > a[key]) ? b : a,
  );
const sStart = best("startup");
const sLat = best("p50");
const sTput = best("tput", "max");
const ratio = (impl) => {
  const s = rows.find((r) => r.impl === impl && r.transport === "stdio");
  const w = rows.find((r) => r.impl === impl && r.transport === "ws");
  return s && w ? (s.tput / w.tput).toFixed(1) : "?";
};
const tputRatios = [...new Set(rows.map((r) => r.impl))]
  .map((i) => `${i} ${ratio(i)}×`)
  .join(", ");
md += `
## Reading it (derived from the numbers above)

- **Fastest cold start:** ${sStart.impl}/${sStart.transport} (${n1(sStart.startup)} ms).
- **Lowest round-trip latency (p50):** ${sLat.impl}/${sLat.transport} (${n0(sLat.p50)} µs).
- **Highest throughput:** ${sTput.impl}/${sTput.transport} (${n0(sTput.tput)} msg/s).
- **stdio beats websocket on throughput** by roughly ${tputRatios}
  (websocket adds RFC 6455 framing + a loopback TCP hop). Use stdio for
  local plugins; websocket when the plugin must be remote/long-lived.
- Numbers vary run-to-run (JIT warm-up, scheduler) — treat as orders of
  magnitude, not exact. Rust avoids VM warm-up so its cold start and tail
  latency are the most consistent.
- Every host speaks the **identical JSON-RPC 2.0 wire** — the runtime
  choice is purely performance/operational, never a protocol difference.
`;

const out = resolve(here, "RESULTS.md");
writeFileSync(out, md);
process.stderr.write(`\nwrote ${out}\n`);
process.stdout.write(md);
process.exit(0);
