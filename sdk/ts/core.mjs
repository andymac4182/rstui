// rstui plugin framework — app-agnostic transport core (ESM JS).
// ADR 0021: the JS twin of the Rust `rstui-plugin-core` crate. No app
// vocabulary lives here — `bridge(proto)` is generic over a small
// `proto = { actionMethod, initializeResult, isShutdown }`. The ACP layer
// (./index.mjs, definePlugin) injects ACP's `proto`; another app injects
// its own. Identical JSON-RPC 2.0 wire as the native Rust plugins.
//
// Transports (chosen by bridge(), precedence top→bottom — mirrors the
// Rust SDK's serve_auto):
//   * injected `globalThis.__rstuiHost`  — running under the V8 host.
//   * `--shm <path>` / RSTUI_PLUGIN_SHM — shared memory via the OPTIONAL
//     native addon (@rstui-acp/plugin-shm-native); probed, with graceful
//     fallback when absent. Offered for parity, NOT speed: ≈ Node-stdio
//     latency (the Node event loop is the floor — ADR 0019).
//   * `--uds <path>` / RSTUI_PLUGIN_UDS — a Unix-domain-socket server (no
//     TCP/IP stack, no port — the lowest-overhead local socket).
//   * `--ws <port>` / RSTUI_PLUGIN_WS=<port> — a dependency-free RFC 6455
//     WebSocket server (node:net + node:crypto; works in Node and Bun).
//   * `--lp` / RSTUI_PLUGIN_LP — length-prefixed binary framing (u32 BE
//     length + JSON bytes; no newline scan) over stdio/uds.
//   * otherwise — newline-delimited JSON-RPC over stdio.
// See sdk/RUNTIME_DECISION.md for why this (not secure-exec) is the path,
// and sdk/bench/OPTIMISATION.md for the transport/overhead analysis.

// The shared JSON-RPC plugin core: transport-agnostic AND app-agnostic.
// A transport supplies `writeLine(str)` + `close()` and feeds inbound
// lines to feed(); `proto` supplies the only app-specific bits:
//   * actionMethod    — { action.type → JSON-RPC method } map (outbound)
//   * initializeResult — the `result` payload for the `initialize` ack
//   * isShutdown(params) — true when an inbound event ends the loop
//
// Hot path: the queue carries the **event object** (not a re-serialized
// JSON string), and `emitObj`/`nextObj` let definePlugin skip a redundant
// stringify+parse on every inbound and outbound message — the only JSON
// work per round-trip is now the one decode in feed() and the one encode
// in emitObj(). The legacy string API (feed/emit/next) is retained so the
// injected V8-host bridge contract is unchanged. See OPTIMISATION.md.
function makeBridgeCore({ writeLine, closeTransport, proto }) {
  const queue = [];
  let waiting = null;
  let done = false;
  const push = (obj) => {
    if (waiting) {
      const w = waiting;
      waiting = null;
      w(obj);
    } else {
      queue.push(obj);
    }
  };
  const finish = () => {
    done = true;
    if (waiting) {
      const w = waiting;
      waiting = null;
      w(null);
    }
  };
  const emitObj = (a) => {
    const method = proto.actionMethod[a.type];
    if (!method) return;
    writeLine(JSON.stringify({ jsonrpc: "2.0", method, params: a }));
  };
  const nextObj = () => {
    if (queue.length > 0) return Promise.resolve(queue.shift());
    if (done) return Promise.resolve(null);
    return new Promise((res) => {
      waiting = res;
    });
  };
  return {
    finish,
    feed(line) {
      // A binary/socket framer hands us an exact JSON string; only the
      // newline path can carry stray whitespace, so trim lazily.
      let msg;
      try {
        msg = JSON.parse(line);
      } catch {
        const s = String(line).trim();
        if (!s) return;
        try {
          msg = JSON.parse(s);
        } catch {
          return;
        }
      }
      if (msg.method === undefined) return; // a response, not for us
      if (msg.method === "initialize" && msg.id !== undefined) {
        writeLine(
          JSON.stringify({
            jsonrpc: "2.0",
            id: msg.id,
            result: proto.initializeResult,
          }),
        );
      }
      if (msg.params && typeof msg.params === "object") {
        push(msg.params);
        if (proto.isShutdown(msg.params)) finish();
      }
    },
    emitObj,
    emit(actionJson) {
      let a;
      try {
        a = JSON.parse(actionJson);
      } catch {
        return;
      }
      emitObj(a);
    },
    nextObj,
    next() {
      // Legacy string contract (kept for any external caller / parity with
      // the injected host): serialize on demand. definePlugin uses nextObj.
      return nextObj().then((o) => (o == null ? null : JSON.stringify(o)));
    },
    close() {
      try {
        closeTransport();
      } catch {
        /* already closed */
      }
      globalThis.process?.exit?.(0);
    },
  };
}

// ---- length-prefixed binary framing (shared by stdio-lp + uds) ---------
// One message = u32 big-endian byte length + raw JSON bytes. No newline
// scan, no per-line string concat — exact reads straight off the socket.
function lpEncode(s) {
  const body = Buffer.from(s, "utf8");
  const head = Buffer.allocUnsafe(4);
  head.writeUInt32BE(body.length, 0);
  return Buffer.concat([head, body]);
}
function makeLpDecoder(onMsg) {
  let buf = Buffer.alloc(0);
  let start = 0; // read cursor; advance through whole frames without reslice
  return (chunk) => {
    if (start === buf.length) {
      buf = chunk; // steady state: previous chunk fully consumed — adopt
      start = 0;
    } else if (start > 0) {
      buf = Buffer.concat([buf.subarray(start), chunk]); // splice tail only
      start = 0;
    } else {
      buf = Buffer.concat([buf, chunk]);
    }
    for (;;) {
      if (buf.length - start < 4) return;
      const n = buf.readUInt32BE(start);
      if (buf.length - start < 4 + n) return;
      const json = buf.toString("utf8", start + 4, start + 4 + n);
      start += 4 + n;
      onMsg(json);
    }
  };
}
function makeNlDecoder(onMsg) {
  let buf = "";
  return (chunk) => {
    buf += chunk;
    let i;
    while ((i = buf.indexOf("\n")) >= 0) {
      const l = buf.slice(0, i);
      buf = buf.slice(i + 1);
      if (l) onMsg(l);
    }
  };
}

// Newline-delimited JSON-RPC over stdio (the default).
function makeStdioBridge(proto) {
  const p = globalThis.process;
  const core = makeBridgeCore({
    writeLine: (s) => p.stdout.write(`${s}\n`),
    closeTransport: () => rl.close(),
    proto,
  });
  // node:readline reliably drains stdin incl. bytes a launcher buffered
  // before this listener attached (real launchers send `initialize`
  // immediately). getBuiltinModule keeps this synchronous + import-free.
  const rl = p
    .getBuiltinModule("node:readline")
    .createInterface({ input: p.stdin });
  rl.on("line", (l) => core.feed(l));
  rl.on("close", core.finish);
  return core;
}

// Length-prefixed JSON-RPC over stdio (binary framing, no newline scan).
// stdin stays in Buffer mode (no setEncoding) so frames split exactly.
function makeStdioLpBridge(proto) {
  const p = globalThis.process;
  const core = makeBridgeCore({
    writeLine: (s) => p.stdout.write(lpEncode(s)),
    closeTransport: () => {
      try {
        p.stdin.destroy();
      } catch {}
    },
    proto,
  });
  const dec = makeLpDecoder((m) => core.feed(m));
  p.stdin.on("data", dec);
  p.stdin.on("end", core.finish);
  p.stdin.on("close", core.finish);
  return core;
}

// Unix-domain-socket *server* (mirrors the Rust serve_unix): bind a
// filesystem path, accept one client, frame newline or length-prefixed.
// No TCP/IP stack, no port — the lowest-overhead local socket.
async function makeUdsBridge(path, lp, proto) {
  const net = await import("node:net");
  const fs = await import("node:fs");
  let sock = null;
  let server = null;
  const core = makeBridgeCore({
    writeLine: (s) => {
      if (sock) sock.write(lp ? lpEncode(s) : `${s}\n`);
    },
    closeTransport: () => {
      try {
        sock?.end();
      } catch {}
      try {
        server?.close();
      } catch {}
      try {
        fs.unlinkSync(path);
      } catch {}
    },
    proto,
  });
  try {
    fs.unlinkSync(path); // bind fails if a stale socket file exists
  } catch {}
  await new Promise((resolve, reject) => {
    server = net.createServer((s) => {
      if (sock) {
        s.destroy();
        return;
      }
      sock = s;
      if (lp) {
        const dec = makeLpDecoder((m) => core.feed(m));
        s.on("data", dec);
      } else {
        s.setEncoding("utf8");
        const dec = makeNlDecoder((l) => core.feed(l));
        s.on("data", dec);
      }
      s.on("close", core.finish);
      s.on("error", core.finish);
    });
    server.on("error", reject);
    server.listen(path, () => resolve());
  });
  return core;
}

// Dependency-free RFC 6455 WebSocket *server* (mirrors the Rust
// WsTransport): bind, accept one client, frame JSON-RPC text messages.
async function makeWsBridge(port, proto) {
  const net = await import("node:net");
  const crypto = await import("node:crypto");
  const GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
  let sock = null;
  let server = null;

  const core = makeBridgeCore({
    writeLine: (s) => {
      if (sock) sock.write(encodeFrame(0x1, Buffer.from(s, "utf8")));
    },
    closeTransport: () => {
      try {
        sock?.end();
      } catch {}
      server?.close();
    },
    proto,
  });

  function encodeFrame(opcode, payload) {
    const n = payload.length;
    let header;
    if (n < 126) header = Buffer.from([0x80 | opcode, n]);
    else if (n <= 0xffff) {
      header = Buffer.from([0x80 | opcode, 126, n >> 8, n & 0xff]);
    } else {
      header = Buffer.alloc(10);
      header[0] = 0x80 | opcode;
      header[1] = 127;
      header.writeBigUInt64BE(BigInt(n), 2);
    }
    return Buffer.concat([header, payload]);
  }

  await new Promise((resolve) => {
    server = net.createServer((s) => {
      if (sock) {
        s.destroy();
        return;
      }
      sock = s;
      let buf = Buffer.alloc(0);
      let upgraded = false;
      const frags = [];
      s.on("data", (chunk) => {
        buf = Buffer.concat([buf, chunk]);
        if (!upgraded) {
          const i = buf.indexOf("\r\n\r\n");
          if (i < 0) return;
          const head = buf.slice(0, i).toString("utf8");
          buf = buf.slice(i + 4);
          const key = /sec-websocket-key:\s*(.+)/i.exec(head)?.[1]?.trim();
          const accept = crypto
            .createHash("sha1")
            .update(key + GUID)
            .digest("base64");
          s.write(
            "HTTP/1.1 101 Switching Protocols\r\n" +
              "Upgrade: websocket\r\nConnection: Upgrade\r\n" +
              `Sec-WebSocket-Accept: ${accept}\r\n\r\n`,
          );
          upgraded = true;
        }
        // Parse as many complete frames as are buffered.
        for (;;) {
          if (buf.length < 2) break;
          const fin = (buf[0] & 0x80) !== 0;
          const opcode = buf[0] & 0x0f;
          const masked = (buf[1] & 0x80) !== 0;
          let len = buf[1] & 0x7f;
          let off = 2;
          if (len === 126) {
            if (buf.length < 4) break;
            len = buf.readUInt16BE(2);
            off = 4;
          } else if (len === 127) {
            if (buf.length < 10) break;
            len = Number(buf.readBigUInt64BE(2));
            off = 10;
          }
          const need = off + (masked ? 4 : 0) + len;
          if (buf.length < need) break;
          let mask = null;
          if (masked) {
            mask = buf.slice(off, off + 4);
            off += 4;
          }
          const payload = Buffer.from(buf.slice(off, off + len));
          if (mask) for (let k = 0; k < payload.length; k++) payload[k] ^= mask[k % 4];
          buf = buf.slice(need);
          if (opcode === 0x8) {
            core.finish();
            return;
          }
          if (opcode === 0x9) {
            s.write(encodeFrame(0xa, payload));
            continue;
          }
          if (opcode === 0xa) continue;
          frags.push(payload);
          if (fin) {
            core.feed(Buffer.concat(frags).toString("utf8"));
            frags.length = 0;
          }
        }
      });
      s.on("close", core.finish);
      s.on("error", core.finish);
    });
    server.listen(port, "127.0.0.1", () => resolve());
  });
  return core;
}

// Probe the OPTIONAL native shared-memory addon (ADR 0019). The core SDK
// stays dependency-free: this is a try/catch dynamic import, so when the
// addon is not installed `bridge()` simply falls back to uds/stdio.
// Resolution order: explicit env module → the published optional package
// → the in-repo dev loader (sibling of this file).
async function loadShmAddon() {
  const env = globalThis.process?.env ?? {};
  const cands = [
    env.RSTUI_SHM_NATIVE_MODULE,
    "@rstui-acp/plugin-shm-native",
    new URL("../shm-native/index.mjs", import.meta.url).href,
  ];
  for (const c of cands) {
    if (!c) continue;
    try {
      const m = await import(c);
      if (m?.ShmChannel) return m.ShmChannel;
    } catch {
      /* not present — try next */
    }
  }
  return null;
}

// Shared-memory bridge (ADR 0019): the plugin attaches to the host's
// segment; framing IS the ring (one whole JSON-RPC message per
// tryRecv()), so messages are fed straight to the core. Adaptive poll —
// hot (setImmediate, ≈ one event-loop tick) for a short window after
// activity, a 1 ms timer when idle (≈0 % CPU). NOTE: this is ≈ Node-stdio
// latency, not the Rust sub-µs — the Node event loop is the floor, by
// design (ADR 0019). It is offered for parity/optionality, not speed.
function makeShmBridge(ShmChannel, path, proto) {
  const ch = ShmChannel.open(path);
  const core = makeBridgeCore({
    writeLine: (s) => ch.send(Buffer.from(s, "utf8")),
    closeTransport: () => {
      try {
        globalThis.process?.exit?.(0);
      } catch {}
    },
    proto,
  });
  let hotUntil = 0;
  const pump = () => {
    if (ch.isClosed()) {
      core.finish();
      return;
    }
    let did = false;
    for (;;) {
      const msg = ch.tryRecv();
      if (!msg) break;
      core.feed(msg.toString("utf8"));
      did = true;
    }
    if (did) hotUntil = Date.now() + 4;
    if (Date.now() < hotUntil) setImmediate(pump);
    else setTimeout(pump, 1);
  };
  pump();
  return core;
}

export async function bridge(proto) {
  const injected = globalThis.__rstuiHost;
  if (
    injected &&
    typeof injected.next === "function" &&
    typeof injected.emit === "function"
  ) {
    return injected; // running under the V8 host
  }
  const argv = globalThis.process?.argv ?? [];
  const env = globalThis.process?.env ?? {};
  const argVal = (f) => {
    const i = argv.indexOf(f);
    return i >= 0 ? argv[i + 1] : undefined;
  };
  const shm = argVal("--shm") || env.RSTUI_PLUGIN_SHM;
  const uds = argVal("--uds") || env.RSTUI_PLUGIN_UDS;
  const wsPort = Number(argVal("--ws")) || Number(env.RSTUI_PLUGIN_WS) || 0;
  const lp =
    argv.includes("--lp") ||
    (!!env.RSTUI_PLUGIN_LP && env.RSTUI_PLUGIN_LP !== "0");
  // Precedence (mirrors the Rust serve_auto): shm → uds → ws → stdio.
  // shm needs the OPTIONAL native addon; if it is absent we log one line
  // and fall through (graceful — the core SDK is dependency-free).
  if (shm) {
    const ShmChannel = await loadShmAddon();
    if (ShmChannel) return makeShmBridge(ShmChannel, shm, proto);
    globalThis.process?.stderr?.write?.(
      "rstui plugin SDK: --shm requested but @rstui-acp/plugin-shm-native " +
        "is not installed; falling back to uds/stdio (no latency loss — " +
        "shm is ≈ stdio for Node anyway, see ADR 0019)\n",
    );
  }
  if (uds) return makeUdsBridge(uds, lp, proto);
  if (wsPort) return makeWsBridge(wsPort, proto);
  const haveStdio = globalThis.process?.stdin && globalThis.process?.stdout;
  if (lp && haveStdio) return makeStdioLpBridge(proto);
  if (haveStdio) return makeStdioBridge(proto); // plain process over stdio
  throw new Error(
    "rstui plugin SDK: no host bridge / stdio / --ws / --uds — run as a process or via the V8 host.",
  );
}


// App-agnostic exports. `bridge(proto)` is exported inline above.
export { makeBridgeCore };
