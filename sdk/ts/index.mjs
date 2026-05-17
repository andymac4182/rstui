// @rstui-acp/plugin-sdk — runtime (ESM JS; types in index.d.ts).
//
// Runs inside the V8 host. The host injects a bridge as
// `globalThis.__rstuiHost = { next(): Promise<string|null>, emit(s) }`:
// `next()` yields the next HostEvent JSON (null = end), `emit()` sends a
// PluginAction JSON. Under secure-exec these are sandbox `bindings`; in the
// host's dev fallback they are plain functions — the SDK is identical.

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

// A built-in stdio JSON-RPC bridge, so a plugin run as a plain process
// (`node plugin.mjs`, `bun plugin.ts`) IS a plugin — no V8 host needed,
// uniform with native Rust plugins (see sdk/RUNTIME_DECISION.md).
function makeStdioBridge() {
  const p = globalThis.process;
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
  const onLine = (line) => {
    const s = line.trim();
    if (!s) return;
    let msg;
    try {
      msg = JSON.parse(s);
    } catch {
      return;
    }
    if (msg.method === undefined) return; // a response
    if (msg.method === "initialize" && msg.id !== undefined) {
      p.stdout.write(
        `${JSON.stringify({ jsonrpc: "2.0", id: msg.id, result: { ok: true, apiVersion: "1" } })}\n`,
      );
    }
    if (msg.params && typeof msg.params === "object") {
      push(JSON.stringify(msg.params));
      if (msg.params.type === "shutdown") finish();
    }
  };
  // Use node:readline (same as the V8 host) — it reliably drains stdin
  // including bytes a launcher buffered before this listener attached
  // (real launchers, incl. the rstui client, send `initialize`
  // immediately on spawn). `process.getBuiltinModule` keeps this
  // synchronous and adds no static `node:` import (so the SDK still
  // loads cleanly inside a non-Node sandbox, where this path is unused).
  const rl = p
    .getBuiltinModule("node:readline")
    .createInterface({ input: p.stdin });
  rl.on("line", onLine);
  rl.on("close", finish);
  return {
    emit(actionJson) {
      let a;
      try {
        a = JSON.parse(actionJson);
      } catch {
        return;
      }
      const method = ACTION_METHOD[a.type];
      if (!method) return;
      p.stdout.write(`${JSON.stringify({ jsonrpc: "2.0", method, params: a })}\n`);
    },
    next() {
      if (queue.length > 0) return Promise.resolve(queue.shift());
      if (done) return Promise.resolve(null);
      return new Promise((res) => {
        waiting = res;
      });
    },
    // We *are* the whole process here, so once the loop ends (shutdown or
    // EOF) close stdin/readline and exit — otherwise the open stdin handle
    // keeps Node alive forever. (The injected V8-host bridge has no close;
    // the host owns that process's lifecycle.)
    close() {
      try {
        rl.close();
      } catch {
        /* already closed */
      }
      p.exit(0);
    },
  };
}

function bridge() {
  const injected = globalThis.__rstuiHost;
  if (
    injected &&
    typeof injected.next === "function" &&
    typeof injected.emit === "function"
  ) {
    return injected; // running under the V8 host
  }
  if (globalThis.process?.stdin && globalThis.process?.stdout) {
    return makeStdioBridge(); // running as a plain process
  }
  throw new Error(
    "rstui plugin SDK: no host bridge and no stdio — run this plugin as a " +
      "process (node/bun) or via the rstui V8 host.",
  );
}

export async function definePlugin(handlers) {
  const b = bridge();
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

    // Route modal/ask answers back to their awaiting promise.
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

    // Shutdown is awaited (it can't depend on the pump) then ends the loop.
    if (ev.type === "shutdown") {
      try {
        await handlers.onShutdown?.(host);
      } catch (err) {
        host.log(`plugin onShutdown error: ${err?.message ?? err}`);
      }
      break;
    }

    // Dispatch WITHOUT blocking the pump: a handler may `await host.modal()`
    // / `host.askUser()`, whose answer arrives as a later event the pump
    // must still deliver. Errors are logged, never thrown into the loop.
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
  // Loop ended (shutdown or EOF): let a standalone process exit.
  b.close?.();
}

export default { definePlugin };
