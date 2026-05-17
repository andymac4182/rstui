#!/usr/bin/env node
// rstui-acp-client V8 plugin host.
//
//   node sdk/v8-host/host.mjs <plugin.(mjs|js)> [--sandbox]
//
// Speaks newline-delimited JSON-RPC 2.0 to the rstui client on stdio (the
// same wire as native Rust plugins), and runs the user plugin against the
// `@rstui-acp/plugin-sdk` bridge.
//
// Two execution modes:
//   * secure-exec (default when `secure-exec` is installed, or `--sandbox`):
//     the plugin runs in an isolated V8 sandbox with deny-by-default
//     fs/network — untrusted code is contained.
//   * dev fallback (no `secure-exec`): the plugin runs in-process via
//     dynamic import. NOT sandboxed; a stderr warning says so. This is the
//     "our own host, learning from secure-exec" path — identical bridge +
//     deny-by-default *intent*, minus the V8 isolate.

import { createInterface } from "node:readline";
import { pathToFileURL } from "node:url";
import { resolve as resolvePath } from "node:path";

// ---- JSON-RPC plumbing ------------------------------------------------

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

function writeLine(obj) {
  process.stdout.write(`${JSON.stringify(obj)}\n`);
}

// An async single-consumer queue of HostEvent JSON strings.
const queue = [];
let waiting = null;
let done = false;

function pushEvent(jsonStr) {
  if (waiting) {
    const w = waiting;
    waiting = null;
    w(jsonStr);
  } else {
    queue.push(jsonStr);
  }
}
function finish() {
  done = true;
  if (waiting) {
    const w = waiting;
    waiting = null;
    w(null);
  }
}

const bridge = {
  // sandbox/plugin -> host: a PluginAction JSON string.
  emit(actionJson) {
    let a;
    try {
      a = JSON.parse(actionJson);
    } catch {
      return;
    }
    const method = ACTION_METHOD[a.type];
    if (!method) return;
    writeLine({ jsonrpc: "2.0", method, params: a });
  },
  // host -> sandbox/plugin: the next HostEvent JSON string, or null at end.
  next() {
    if (queue.length > 0) return Promise.resolve(queue.shift());
    if (done) return Promise.resolve(null);
    return new Promise((res) => {
      waiting = res;
    });
  },
};

// Read JSON-RPC from the client; turn it into HostEvents for the plugin.
const rl = createInterface({ input: process.stdin });
rl.on("line", (line) => {
  const s = line.trim();
  if (!s) return;
  let msg;
  try {
    msg = JSON.parse(s);
  } catch {
    return; // ignore non-JSON noise
  }
  if (msg.method === undefined) return; // a response — not for us
  // Answer the JSON-RPC handshake; still deliver the Init event.
  if (msg.method === "initialize" && msg.id !== undefined) {
    writeLine({
      jsonrpc: "2.0",
      id: msg.id,
      result: { ok: true, apiVersion: "1" },
    });
  }
  // params carry the typed HostEvent payload ({"type":"init",...} etc).
  if (msg.params && typeof msg.params === "object") {
    pushEvent(JSON.stringify(msg.params));
    if (msg.params.type === "shutdown") finish();
  }
});
rl.on("close", finish);

// ---- plugin execution -------------------------------------------------

const pluginArg = process.argv[2];
if (!pluginArg) {
  process.stderr.write("usage: host.mjs <plugin.(mjs|js)> [--sandbox]\n");
  process.exit(2);
}
const pluginPath = resolvePath(process.cwd(), pluginArg);
const forceSandbox = process.argv.includes("--sandbox");

async function loadSecureExec() {
  try {
    return await import("secure-exec");
  } catch {
    return null;
  }
}

async function runDevFallback() {
  if (!forceSandbox) {
    process.stderr.write(
      "[rstui-v8-host] secure-exec not installed — running UNSANDBOXED " +
        "(dev mode). `npm i secure-exec` for V8 isolation.\n",
    );
  }
  globalThis.__rstuiHost = bridge;
  await import(pathToFileURL(pluginPath).href);
}

async function runSandboxed(se) {
  // Deny-by-default driver: the plugin gets no fs/network/process.
  const driver = se.createNodeDriver({
    permissions: {
      fs: () => ({ allow: false }),
      network: () => ({ allow: false }),
    },
  });
  const runtime = new se.NodeRuntime({
    systemDriver: driver,
    runtimeDriverFactory: se.createNodeRuntimeDriverFactory(),
    memoryLimit: 64,
    cpuTimeLimitMs: 0, // 0 = no per-call CPU cap (this is a long-lived loop)
    bindings: { rstui: { next: bridge.next, emit: bridge.emit } },
  });
  // The sandbox bootstrap wires the SDK bridge to the injected bindings,
  // then imports the user plugin (which calls definePlugin and loops).
  const sdkUrl = pathToFileURL(
    resolvePath(import.meta.dirname, "../ts/index.mjs"),
  ).href;
  const userUrl = pathToFileURL(pluginPath).href;
  const bootstrap = `
    globalThis.__rstuiHost = {
      next: (...a) => SecureExec.bindings.rstui.next(...a),
      emit: (...a) => SecureExec.bindings.rstui.emit(...a),
    };
    await import(${JSON.stringify(sdkUrl)});
    await import(${JSON.stringify(userUrl)});
  `;
  await runtime.run(bootstrap, "/rstui-bootstrap.mjs");
  runtime.dispose?.();
}

(async () => {
  const se = await loadSecureExec();
  try {
    if (se) {
      await runSandboxed(se);
    } else if (forceSandbox) {
      throw new Error("--sandbox requires `npm i secure-exec`");
    } else {
      await runDevFallback();
    }
  } catch (err) {
    writeLine({
      jsonrpc: "2.0",
      method: "ui/log",
      params: { type: "log", text: `v8-host: ${err?.message ?? err}` },
    });
    process.exitCode = 1;
  }
  process.exit(process.exitCode ?? 0);
})();
