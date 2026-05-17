/**
 * `@rstui-acp/plugin-sdk` — the TypeScript SDK for rstui-acp-client plugins.
 *
 * A TS plugin written against this SDK runs *inside the V8 host*
 * (`sdk/v8-host`), which owns the JSON-RPC 2.0 transport to the rstui
 * client. The SDK only marshals handlers ↔ the host bridge; it never
 * touches stdio itself (that is the host's job — and, under secure-exec,
 * the sandbox has no stdio anyway).
 *
 * The event/action shapes mirror the Rust `rstui-acp-plugin-sdk` `proto`
 * exactly (snake_case `type` discriminants) so both SDKs speak one wire.
 */

export interface FooterSegment {
  text: string;
  fg?: string | null;
  bg?: string | null;
}

export type HostEvent =
  | { type: "init"; api_version: string; client: string; cwd: string }
  | { type: "session_start"; agent: string }
  | { type: "user_prompt"; text: string }
  | { type: "turn_ended"; stop_reason: string }
  | { type: "command"; name: string; args: string }
  | { type: "modal_response"; id: number; button: string; cancelled: boolean }
  | {
      type: "ask_response";
      id: number;
      selections: string[];
      text: string;
      cancelled: boolean;
    }
  | { type: "refresh" }
  | { type: "shutdown" };

export type PluginAction =
  | { type: "register_command"; name: string; description: string }
  | { type: "set_status"; key: string; value: string }
  | { type: "footer"; segments: FooterSegment[] }
  | {
      type: "ask_user";
      id: number;
      question: string;
      context?: string;
      options?: string[];
      allow_freeform?: boolean;
    }
  | {
      type: "register_keybinding";
      keys: string;
      command: string;
      description: string;
    }
  | { type: "modal"; id: number; title: string; body?: string[]; buttons?: string[] }
  | { type: "panel"; title: string; body: string[] }
  | { type: "note"; text: string }
  | { type: "log"; text: string };

/** The ergonomic emit handle passed to every handler. */
export interface Host {
  registerCommand(name: string, description: string): void;
  registerKeybinding(keys: string, command: string, description: string): void;
  setStatus(key: string, value: string): void;
  footer(segments: FooterSegment[]): void;
  panel(title: string, body: string[]): void;
  note(text: string): void;
  log(text: string): void;
  /** Show a modal; resolves with the chosen button (or null if cancelled). */
  modal(
    title: string,
    body: string[],
    buttons: string[],
  ): Promise<string | null>;
  /** Ask a structured question; resolves with the user's answer. */
  askUser(opts: {
    question: string;
    context?: string;
    options?: string[];
    allowFreeform?: boolean;
  }): Promise<{ selections: string[]; text: string; cancelled: boolean }>;
  /** Emit a raw action (escape hatch). */
  emit(action: PluginAction): void;
}

export interface PluginHandlers {
  onInit?(
    e: { apiVersion: string; client: string; cwd: string },
    host: Host,
  ): void | Promise<void>;
  onSessionStart?(agent: string, host: Host): void | Promise<void>;
  onPrompt?(text: string, host: Host): void | Promise<void>;
  onTurnEnded?(stopReason: string, host: Host): void | Promise<void>;
  onCommand?(name: string, args: string, host: Host): void | Promise<void>;
  onTick?(host: Host): void | Promise<void>;
  onShutdown?(host: Host): void | Promise<void>;
}

/** Registers the plugin and runs its event loop against the host bridge. */
export function definePlugin(handlers: PluginHandlers): Promise<void>;
