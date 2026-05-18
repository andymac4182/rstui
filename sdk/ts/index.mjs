// @rstui-acp/plugin-sdk — runtime (ESM JS; types in index.d.ts).
//
// A TS plugin is a process speaking JSON-RPC 2.0 — the same wire as native
// Rust plugins. Transports (chosen by bridge(), precedence top→bottom —
// mirrors the Rust SDK's serve_auto):
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

// Plugin → host method per action `type` (matches the Rust SDK proto).
const ACTION_METHOD = {
  register_command: "commands/register",
  register_keybinding: "ui/registerKeybinding",
  set_status: "ui/setStatus",
  footer: "ui/footer",
  panel: "ui/panel",
  note: "ui/note",
  log: "ui/log",
  ask_user: "ui/askUser",
  modal: "ui/modal",
};

// The shared JSON-RPC plugin protocol: transport-agnostic. A transport
// supplies `writeLine(str)` + `close()` and feeds inbound lines to feed().
//
// Hot path: the queue carries the **event object** (not a re-serialized
// JSON string), and `emitObj`/`nextObj` let definePlugin skip a redundant
// stringify+parse on every inbound and outbound message — the only JSON
// work per round-trip is now the one decode in feed() and the one encode
// in emitObj(). The legacy string API (feed/emit/next) is retained so the
// injected V8-host bridge contract is unchanged. See OPTIMISATION.md.
function makeBridgeCore({ writeLine, closeTransport }) {
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
    const method = ACTION_METHOD[a.type];
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
            result: { ok: true, apiVersion: "1" },
          }),
        );
      }
      if (msg.params && typeof msg.params === "object") {
        push(msg.params);
        if (msg.params.type === "shutdown") finish();
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
function makeStdioBridge() {
  const p = globalThis.process;
  const core = makeBridgeCore({
    writeLine: (s) => p.stdout.write(`${s}\n`),
    closeTransport: () => rl.close(),
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
function makeStdioLpBridge() {
  const p = globalThis.process;
  const core = makeBridgeCore({
    writeLine: (s) => p.stdout.write(lpEncode(s)),
    closeTransport: () => {
      try {
        p.stdin.destroy();
      } catch {}
    },
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
async function makeUdsBridge(path, lp) {
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
async function makeWsBridge(port) {
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
function makeShmBridge(ShmChannel, path) {
  const ch = ShmChannel.open(path);
  const core = makeBridgeCore({
    writeLine: (s) => ch.send(Buffer.from(s, "utf8")),
    closeTransport: () => {
      try {
        globalThis.process?.exit?.(0);
      } catch {}
    },
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

async function bridge() {
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
    if (ShmChannel) return makeShmBridge(ShmChannel, shm);
    globalThis.process?.stderr?.write?.(
      "rstui plugin SDK: --shm requested but @rstui-acp/plugin-shm-native " +
        "is not installed; falling back to uds/stdio (no latency loss — " +
        "shm is ≈ stdio for Node anyway, see ADR 0019)\n",
    );
  }
  if (uds) return makeUdsBridge(uds, lp);
  if (wsPort) return makeWsBridge(wsPort);
  const haveStdio = globalThis.process?.stdin && globalThis.process?.stdout;
  if (lp && haveStdio) return makeStdioLpBridge();
  if (haveStdio) return makeStdioBridge(); // plain process over stdio
  throw new Error(
    "rstui plugin SDK: no host bridge / stdio / --ws / --uds — run as a process or via the V8 host.",
  );
}

export async function definePlugin(handlers) {
  const b = await bridge();
  let nextId = 1;
  /** id -> resolve fn for in-flight modal()/askUser() */
  const pending = new Map();

  // Fast path: hand the action object straight to the core (one encode).
  // Fallback keeps the injected V8-host's string `emit` contract.
  const emit = b.emitObj
    ? (action) => b.emitObj(action)
    : (action) => b.emit(JSON.stringify(action));
  // Inbound: nextObj yields the event object directly (no stringify→parse
  // bounce); the injected host still returns a JSON string, so parse that.
  const readEvent = b.nextObj
    ? () => b.nextObj()
    : async () => {
        const raw = await b.next();
        if (raw === null || raw === undefined) return null;
        if (typeof raw !== "string") return raw;
        try {
          return JSON.parse(raw);
        } catch {
          return undefined; // skip a malformed frame, keep pumping
        }
      };

  const host = {
    registerCommand: (name, description) =>
      emit({ type: "register_command", name, description }),
    registerKeybinding: (keys, command, description) =>
      emit({ type: "register_keybinding", keys, command, description }),
    setStatus: (key, value) => emit({ type: "set_status", key, value }),
    footer: (segments) => emit({ type: "footer", segments }),
    panel: (title, body) => emit({ type: "panel", title, body }),
    note: (text) => emit({ type: "note", text }),
    log: (text) => emit({ type: "log", text }),
    emit,
    modal(title, body, buttons) {
      const id = nextId++;
      emit({ type: "modal", id, title, body, buttons });
      return new Promise((resolve) => {
        pending.set(`modal:${id}`, (e) =>
          resolve(e.cancelled ? null : e.button),
        );
      });
    },
    askUser({ question, context = "", options = [], allowFreeform = false }) {
      const id = nextId++;
      emit({
        type: "ask_user",
        id,
        question,
        context,
        options,
        allow_freeform: allowFreeform,
      });
      return new Promise((resolve) => {
        pending.set(`ask:${id}`, (e) =>
          resolve({
            selections: e.selections,
            text: e.text,
            cancelled: e.cancelled,
          }),
        );
      });
    },
  };

  for (;;) {
    const ev = await readEvent();
    if (ev === null) break; // end of stream
    if (ev === undefined || typeof ev !== "object") continue; // skip junk

    if (ev.type === "modal_response" && pending.has(`modal:${ev.id}`)) {
      pending.get(`modal:${ev.id}`)(ev);
      pending.delete(`modal:${ev.id}`);
      continue;
    }
    if (ev.type === "ask_response" && pending.has(`ask:${ev.id}`)) {
      pending.get(`ask:${ev.id}`)(ev);
      pending.delete(`ask:${ev.id}`);
      continue;
    }

    if (ev.type === "shutdown") {
      try {
        await handlers.onShutdown?.(host);
      } catch (err) {
        host.log(`plugin onShutdown error: ${err?.message ?? err}`);
      }
      break;
    }

    // Dispatch WITHOUT blocking the pump: a handler may `await
    // host.modal()`/`askUser()`, whose answer is a later event.
    const run = async () => {
      switch (ev.type) {
        case "init":
          return handlers.onInit?.(
            { apiVersion: ev.api_version, client: ev.client, cwd: ev.cwd },
            host,
          );
        case "session_start":
          return handlers.onSessionStart?.(ev.agent, host);
        case "user_prompt":
          return handlers.onPrompt?.(ev.text, host);
        case "turn_ended":
          return handlers.onTurnEnded?.(ev.stop_reason, host);
        case "command":
          return handlers.onCommand?.(ev.name, ev.args, host);
        case "refresh":
          return handlers.onTick?.(host);
        default:
          return undefined;
      }
    };
    void run().catch((err) =>
      host.log(`plugin handler error: ${err?.message ?? err}`),
    );
  }
  b.close?.();
}

export default { definePlugin };
