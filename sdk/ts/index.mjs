// @rstui-acp/plugin-sdk — runtime (ESM JS; types in index.d.ts).
//
// A TS plugin is a process speaking JSON-RPC 2.0 — the same wire as native
// Rust plugins. Transports (chosen by bridge()):
//   * injected `globalThis.__rstuiHost`  — running under the V8 host.
//   * `--ws <port>` / RSTUI_PLUGIN_WS=<port> — a dependency-free RFC 6455
//     WebSocket server (node:net + node:crypto; works in Node and Bun).
//   * otherwise — newline-delimited JSON-RPC over stdio.
// See sdk/RUNTIME_DECISION.md for why this (not secure-exec) is the path.

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
function makeBridgeCore({ writeLine, closeTransport }) {
  const queue = [];
  let waiting = null;
  let done = false;
  const push = (s) => {
    if (waiting) {
      const w = waiting;
      waiting = null;
      w(s);
    } else {
      queue.push(s);
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
  return {
    finish,
    feed(line) {
      const s = String(line).trim();
      if (!s) return;
      let msg;
      try {
        msg = JSON.parse(s);
      } catch {
        return;
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
        push(JSON.stringify(msg.params));
        if (msg.params.type === "shutdown") finish();
      }
    },
    emit(actionJson) {
      let a;
      try {
        a = JSON.parse(actionJson);
      } catch {
        return;
      }
      const method = ACTION_METHOD[a.type];
      if (!method) return;
      writeLine(JSON.stringify({ jsonrpc: "2.0", method, params: a }));
    },
    next() {
      if (queue.length > 0) return Promise.resolve(queue.shift());
      if (done) return Promise.resolve(null);
      return new Promise((res) => {
        waiting = res;
      });
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
  const wsIdx = argv.indexOf("--ws");
  const wsPort =
    (wsIdx >= 0 && Number(argv[wsIdx + 1])) ||
    Number(globalThis.process?.env?.RSTUI_PLUGIN_WS) ||
    0;
  if (wsPort) return makeWsBridge(wsPort);
  if (globalThis.process?.stdin && globalThis.process?.stdout) {
    return makeStdioBridge(); // plain process over stdio
  }
  throw new Error(
    "rstui plugin SDK: no host bridge / stdio / --ws — run as a process or via the V8 host.",
  );
}

export async function definePlugin(handlers) {
  const b = await bridge();
  let nextId = 1;
  /** id -> resolve fn for in-flight modal()/askUser() */
  const pending = new Map();

  const emit = (action) => b.emit(JSON.stringify(action));

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
    const raw = await b.next();
    if (raw === null || raw === undefined) break;
    let ev;
    try {
      ev = JSON.parse(raw);
    } catch {
      continue;
    }

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
