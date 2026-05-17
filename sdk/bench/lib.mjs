// Shared, dependency-free harness: a WebSocket client + a plugin driver
// that speaks JSON-RPC 2.0 to a plugin process over stdio OR websocket.
// Used by the ws smoke test and the profiler (bench.mjs).

import { spawn } from "node:child_process";
import net from "node:net";
import crypto from "node:crypto";
import os from "node:os";
import path from "node:path";
import fs from "node:fs";

const GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

// ---- minimal RFC 6455 client (mask client→server, read server frames) --

class WsClient {
  constructor(sock) {
    this.sock = sock;
    this.buf = Buffer.alloc(0);
    this.frags = [];
    this.onMessage = null;
    this.onClose = null;
    sock.on("data", (c) => this._data(c));
    sock.on("close", () => this.onClose?.());
    sock.on("error", () => this.onClose?.());
  }
  static connect(port, host = "127.0.0.1") {
    return new Promise((resolve, reject) => {
      const sock = net.connect(port, host, () => {
        const key = crypto.randomBytes(16).toString("base64");
        sock.write(
          `GET / HTTP/1.1\r\nHost: ${host}:${port}\r\nUpgrade: websocket\r\n` +
            `Connection: Upgrade\r\nSec-WebSocket-Key: ${key}\r\n` +
            "Sec-WebSocket-Version: 13\r\n\r\n",
        );
      });
      let hbuf = Buffer.alloc(0);
      const onHead = (c) => {
        hbuf = Buffer.concat([hbuf, c]);
        const i = hbuf.indexOf("\r\n\r\n");
        if (i < 0) return;
        sock.removeListener("data", onHead);
        const rest = hbuf.slice(i + 4);
        const cli = new WsClient(sock);
        if (rest.length) cli._data(rest);
        resolve(cli);
      };
      sock.on("data", onHead);
      sock.on("error", reject);
    });
  }
  _data(chunk) {
    this.buf = Buffer.concat([this.buf, chunk]);
    for (;;) {
      if (this.buf.length < 2) break;
      const fin = (this.buf[0] & 0x80) !== 0;
      const opcode = this.buf[0] & 0x0f;
      let len = this.buf[1] & 0x7f;
      let off = 2;
      if (len === 126) {
        if (this.buf.length < 4) break;
        len = this.buf.readUInt16BE(2);
        off = 4;
      } else if (len === 127) {
        if (this.buf.length < 10) break;
        len = Number(this.buf.readBigUInt64BE(2));
        off = 10;
      }
      if (this.buf.length < off + len) break;
      const payload = this.buf.slice(off, off + len);
      this.buf = this.buf.slice(off + len);
      if (opcode === 0x8) {
        this.onClose?.();
        return;
      }
      this.frags.push(payload);
      if (fin) {
        const msg = Buffer.concat(this.frags).toString("utf8");
        this.frags.length = 0;
        this.onMessage?.(msg);
      }
    }
  }
  send(str) {
    const payload = Buffer.from(str, "utf8");
    const n = payload.length;
    let header;
    const mask = crypto.randomBytes(4);
    if (n < 126) header = Buffer.from([0x81, 0x80 | n]);
    else if (n <= 0xffff)
      header = Buffer.from([0x81, 0x80 | 126, n >> 8, n & 0xff]);
    else {
      header = Buffer.alloc(10);
      header[0] = 0x81;
      header[1] = 0x80 | 127;
      header.writeBigUInt64BE(BigInt(n), 2);
    }
    const masked = Buffer.from(payload);
    for (let i = 0; i < masked.length; i++) masked[i] ^= mask[i % 4];
    this.sock.write(Buffer.concat([header, mask, masked]));
  }
  close() {
    try {
      this.sock.end();
    } catch {}
  }
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ---- length-prefixed framing (client side; mirrors the SDK) -----------
const lpEncode = (s) => {
  const body = Buffer.from(s, "utf8");
  const head = Buffer.allocUnsafe(4);
  head.writeUInt32BE(body.length, 0);
  return Buffer.concat([head, body]);
};
const makeLpDecoder = (onMsg) => {
  let buf = Buffer.alloc(0);
  let start = 0; // mirrors the SDK decoder so the bench measures shipped code
  return (chunk) => {
    if (start === buf.length) {
      buf = chunk;
      start = 0;
    } else if (start > 0) {
      buf = Buffer.concat([buf.subarray(start), chunk]);
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
};
const makeNlDecoder = (onMsg) => {
  let buf = "";
  return (chunk) => {
    buf += chunk;
    let i;
    while ((i = buf.indexOf("\n")) >= 0) {
      const l = buf.slice(0, i).trim();
      buf = buf.slice(i + 1);
      if (l) onMsg(l);
    }
  };
};
let udsSeq = 0;

// ---- a transport-agnostic driver over a plugin process ----------------
//
// cmd/args spawn the plugin. transport ∈ "stdio" | "stdio-lp" | "uds" |
// "uds-lp" | "ws"  ("-lp" = length-prefixed binary framing; "uds" = the
// plugin binds a Unix-domain socket and we connect as the client — same
// roles as the Rust serve_unix / TS makeUdsBridge servers). Returns an
// object with send(obj), a `lines` array of parsed inbound JSON-RPC,
// waitFor(), and kill().
export async function startPlugin({ cmd, args, transport, wsPort }) {
  const lines = [];
  const api = { onMessage: null };
  const ingest = (obj) => {
    lines.push(obj);
    api.onMessage?.(obj);
  };
  const onJson = (s) => {
    try {
      ingest(JSON.parse(s));
    } catch {}
  };

  const lp = transport.endsWith("-lp");
  const base = transport.replace(/-lp$/, "");
  let child;
  let ws = null;
  let sock = null;
  let udsPath = null;
  let sendRaw;

  if (base === "ws") {
    child = spawn(cmd, [...args, "--ws", String(wsPort)], {
      stdio: ["ignore", "ignore", "inherit"],
      env: { ...process.env, RSTUI_PLUGIN_WS: String(wsPort) },
    });
    let lastErr;
    for (let i = 0; i < 100; i++) {
      try {
        ws = await WsClient.connect(wsPort);
        break;
      } catch (e) {
        lastErr = e;
        await sleep(50);
      }
    }
    if (!ws) throw new Error(`ws connect failed: ${lastErr?.message}`);
    ws.onMessage = onJson;
    sendRaw = (s) => ws.send(s);
  } else if (base === "uds") {
    udsPath = path.join(
      os.tmpdir(),
      `rstui-bench-${process.pid}-${udsSeq++}.sock`,
    );
    const extra = lp ? ["--uds", udsPath, "--lp"] : ["--uds", udsPath];
    child = spawn(cmd, [...args, ...extra], {
      stdio: ["ignore", "ignore", "inherit"],
      env: {
        ...process.env,
        RSTUI_PLUGIN_UDS: udsPath,
        ...(lp ? { RSTUI_PLUGIN_LP: "1" } : {}),
      },
    });
    let lastErr;
    for (let i = 0; i < 200; i++) {
      try {
        sock = await new Promise((res, rej) => {
          const s = net.connect(udsPath);
          s.once("connect", () => res(s));
          s.once("error", rej);
        });
        break;
      } catch (e) {
        lastErr = e;
        await sleep(25);
      }
    }
    if (!sock) throw new Error(`uds connect failed: ${lastErr?.message}`);
    const dec = lp ? makeLpDecoder(onJson) : makeNlDecoder(onJson);
    if (!lp) sock.setEncoding("utf8");
    sock.on("data", dec);
    sendRaw = (s) => sock.write(lp ? lpEncode(s) : `${s}\n`);
  } else {
    const extra = lp ? ["--lp"] : [];
    child = spawn(cmd, [...args, ...extra], {
      stdio: ["pipe", "pipe", "inherit"],
      env: lp ? { ...process.env, RSTUI_PLUGIN_LP: "1" } : process.env,
    });
    const dec = lp ? makeLpDecoder(onJson) : makeNlDecoder(onJson);
    if (!lp) child.stdout.setEncoding("utf8");
    child.stdout.on("data", dec);
    sendRaw = (s) => child.stdin.write(lp ? lpEncode(s) : `${s}\n`);
  }

  const send = (o) => sendRaw(JSON.stringify(o));
  const waitFor = async (pred, what, t = 10000) => {
    const start = Date.now();
    for (;;) {
      const hit = lines.find(pred);
      if (hit) return hit;
      if (Date.now() - start > t)
        throw new Error(`timeout: ${what} (got ${lines.length} msgs)`);
      await sleep(2);
    }
  };
  const kill = () => {
    try {
      ws?.close();
    } catch {}
    try {
      sock?.destroy();
    } catch {}
    try {
      child.kill("SIGKILL");
    } catch {}
    if (udsPath)
      try {
        fs.unlinkSync(udsPath);
      } catch {}
  };
  return {
    child,
    lines,
    send,
    waitFor,
    kill,
    transport,
    set onMessage(fn) {
      api.onMessage = fn;
    },
  };
}

export { sleep };
