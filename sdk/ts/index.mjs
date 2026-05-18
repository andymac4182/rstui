// @rstui-acp/plugin-sdk — the ACP layer (ESM JS; types in index.d.ts).
//
// ADR 0021: this is the thin ACP twin of the Rust `rstui-acp-plugin-sdk`.
// All transport + the JSON-RPC loop live in the app-agnostic
// ./core.mjs (`bridge(proto)`); here we inject only ACP's vocabulary
// (`ACTION_METHOD` + the `initialize` ack + the shutdown sentinel) and
// the ergonomic `definePlugin` host surface. Another Node/Bun app builds
// its own SDK the same way: `import { bridge } from
// "@rstui-acp/plugin-sdk/core"`, pass your own `proto`, write your loop.
// The public surface here (`definePlugin`, default export) is unchanged.

import { bridge } from "./core.mjs";

// Plugin → host JSON-RPC method per action `type` (matches the Rust
// `proto`). This is the ACP vocabulary — the one app-specific table.
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

// The ACP protocol injected into the generic core: the only ACP-specific
// bits the transport-agnostic loop needs.
const ACP = {
  actionMethod: ACTION_METHOD,
  initializeResult: { ok: true, apiVersion: "1" },
  isShutdown: (params) => params?.type === "shutdown",
};

export async function definePlugin(handlers) {
  const b = await bridge(ACP);
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

// Advanced/other-app reuse: the app-agnostic framework, re-exported so it
// is reachable via the package too (other apps inject their own `proto`).
export { bridge, makeBridgeCore } from "./core.mjs";

export default { definePlugin };
