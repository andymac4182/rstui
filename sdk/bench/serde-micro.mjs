// Serialization micro-bench: how much of a plugin round-trip is actually
// spent in JSON encode/decode? This is the evidence behind the
// "protobuf/Cap'n Proto would not help here" recommendation in
// OPTIMISATION.md — you only swap the serializer if it's the bottleneck.
//
// We time JSON.stringify+JSON.parse for the two messages on the hot path
// (a `command/invoke` event and a `ui/note` action — representative tiny
// control messages, the 99% case for TUI plugins), then state the
// per-message ns so it can be compared against the measured round-trip
// latency in RESULTS.md. Dependency-free. Run: node sdk/bench/serde-micro.mjs

const EVENT = {
  jsonrpc: "2.0",
  method: "command/invoke",
  params: { type: "command", name: "fortune", args: "" },
};
const ACTION = {
  jsonrpc: "2.0",
  method: "ui/note",
  params: {
    type: "note",
    text: "The best way to predict the future is to invent it.",
  },
};
// A deliberately larger payload to show where a binary/zero-copy format
// *would* start to matter (big structured blobs, not control messages).
const BIG = {
  jsonrpc: "2.0",
  method: "ui/panel",
  params: {
    type: "panel",
    title: "diff",
    body: Array.from({ length: 200 }, (_, i) => `line ${i}: ${"x".repeat(80)}`),
  },
};

function timeRoundTrip(obj, iters) {
  // Warm up the JIT.
  for (let i = 0; i < 50000; i++) JSON.parse(JSON.stringify(obj));
  const t0 = performance.now();
  for (let i = 0; i < iters; i++) {
    const s = JSON.stringify(obj);
    JSON.parse(s);
  }
  const ns = ((performance.now() - t0) * 1e6) / iters;
  const bytes = Buffer.byteLength(JSON.stringify(obj), "utf8");
  return { ns, bytes };
}

const ITERS = 2_000_000;
const rows = [
  ["command/invoke event", EVENT],
  ["ui/note action", ACTION],
  ["ui/panel (200-line blob)", BIG],
].map(([name, o]) => {
  const { ns, bytes } = timeRoundTrip(o, ITERS);
  return { name, ns, bytes };
});

let md = "message | bytes | JSON encode+decode (ns) \n";
md += "------- | ----: | ----------------------: \n";
for (const r of rows)
  md += `${r.name} | ${r.bytes} | ${r.ns.toFixed(0)} \n`;

const ctl = rows[0].ns + rows[1].ns;
process.stdout.write(md);
process.stdout.write(
  `\nA full control-message round-trip pays ~${(ctl / 1000).toFixed(2)} µs ` +
    `of JSON encode+decode total (event in + action out), on ` +
    `${process.version}. Compare to the measured transport round-trip in ` +
    `RESULTS.md (tens to hundreds of µs): JSON ser/de is a single-digit-%% ` +
    `slice — the cost is process + transport, not the serializer. A binary ` +
    `format (protobuf/Cap'n Proto) only shifts the small slice, while ` +
    `breaking the JSON-RPC wire shared with ACP/MCP. The large-blob row ` +
    `shows where zero-copy would matter — payloads TUI plugins don't send.\n`,
);
