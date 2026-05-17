// @rstui-acp/plugin-sdk — runtime (ESM JS; types in index.d.ts).
//
// Runs inside the V8 host. The host injects a bridge as
// `globalThis.__rstuiHost = { next(): Promise<string|null>, emit(s) }`:
// `next()` yields the next HostEvent JSON (null = end), `emit()` sends a
// PluginAction JSON. Under secure-exec these are sandbox `bindings`; in the
// host's dev fallback they are plain functions — the SDK is identical.

function bridge() {
  const b = globalThis.__rstuiHost;
  if (!b || typeof b.next !== "function" || typeof b.emit !== "function") {
    throw new Error(
      "rstui plugin SDK: no host bridge. Run this plugin via the rstui V8 host.",
    );
  }
  return b;
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
      return;
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
}

export default { definePlugin };
