// Shared, dependency-free harness: a WebSocket client + a plugin driver
// that speaks JSON-RPC 2.0 to a plugin process over stdio OR websocket.
// Used by the ws smoke test and the profiler (bench.mjs).

import { spawn } from "node:child_process";
import net from "node:net";
import crypto from "node:crypto";

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

// ---- a transport-agnostic driver over a plugin process ----------------
//
// cmd/args spawn the plugin. transport ∈ "stdio" | "ws". Returns an object
// with send(obj), a `lines` array of parsed inbound JSON-RPC, waitFor(),
// and kill().
export async function startPlugin({ cmd, args, transport, wsPort }) {
  const lines = [];
  const api = { onMessage: null };
  const ingest = (obj) => {
    lines.push(obj);
    api.onMessage?.(obj);
  };
  let child;
  let ws = null;

  if (transport === "ws") {
    child = spawn(cmd, [...args, "--ws", String(wsPort)], {
      stdio: ["ignore", "ignore", "inherit"],
      env: { ...process.env, RSTUI_PLUGIN_WS: String(wsPort) },
    });
    // Wait for the server to accept, then connect.
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
    ws.onMessage = (m) => {
      try {
        ingest(JSON.parse(m));
      } catch {}
    };
  } else {
    child = spawn(cmd, args, { stdio: ["pipe", "pipe", "inherit"] });
    let buf = "";
    child.stdout.on("data", (d) => {
      buf += d.toString();
      let i;
      while ((i = buf.indexOf("\n")) >= 0) {
        const l = buf.slice(0, i).trim();
        buf = buf.slice(i + 1);
        if (l) {
          try {
            ingest(JSON.parse(l));
          } catch {}
        }
      }
    });
  }

  const send = (o) => {
    const s = JSON.stringify(o);
    if (transport === "ws") ws.send(s);
    else child.stdin.write(`${s}\n`);
  };
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
      child.kill("SIGKILL");
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
