# ACP client ⇄ Codex CLI feature parity

A review of [OpenAI Codex CLI](https://github.com/openai/codex) against
`rstui-acp-client`, and the prioritized plan for closing the gap.

## Framing: what is in scope

Codex is an **agent _and_ its bespoke CLI**. `rstui-acp-client` is a
**generic [ACP](https://agentclientprotocol.com) client** that talks to *any*
agent (Claude Code, Codex, Gemini, …). So feature parity is not "reimplement
Codex" — it is "give the client every ergonomic Codex's TUI has that improves
working with *any* agent", wired through ACP where the protocol already carries
the data.

The `sacp` v11 / `agent-client-protocol-schema` v0.11 surface already supports
the data behind most of Codex's headline features:

| ACP capability | Codex feature it backs |
|---|---|
| `SessionModelState` + `session/set_model` | `/model` (model + reasoning effort) |
| `SessionModeState` + `session/set_mode` | `/plan`, approval/permission modes |
| `Usage` / `UsageUpdate` notifications | `/status` token usage |
| `session/load` + `SessionResumeCapabilities` | `/resume`, `/fork` |
| `authenticate` + `AuthCapabilities` | sign-in (ChatGPT / API key) |
| `AvailableCommandsUpdate` | agent-advertised slash commands *(already wired)* |
| `Plan` / `PlanEntry` | todo sidebar *(already wired)* |

### Explicitly out of scope (agent-side, not a generic client's job)

MCP server management, sandbox/execpolicy/guardian, memories, skills, hooks,
profiles, realtime voice, cloud tasks, the desktop app, `codex exec|apply|
doctor` subcommands, pets. These belong to the *agent* (Codex) and reach the
client only as ACP permission requests, session modes, or advertised commands —
which the client already renders. A Vim composer is also out: ADR 0015 keeps
the keymap shell-level and the composer raw, deliberately.

## Gap table

Legend — **Have**: already in `rstui-acp-client`. **Gap**: to add. **N/A**:
out of scope per above.

| Codex feature | Client today | Plan |
|---|---|---|
| Slash autocomplete, agent-advertised commands | Have (`app.rs` completion) | — |
| Todo / plan sidebar | Have (`Plan` wired) | — |
| Per-request permission modal | Have (`PendingPermission`) | — |
| Theme picker, keymap panel | Have (`rstui-theme`/`-keymap`) | — |
| Plugin extension surface | Have (8 reference plugins) | — |
| ~~Input history (↑/↓ recall, persisted)~~ | **Done** (iter 6) | W1-1 ✅ |
| ~~`/copy` last response to clipboard~~ | **Done** (iter 6) | W1-2 ✅ |
| ~~Terminal title (OSC 2: agent · status)~~ | **Done** (iter 6) | W1-3 ✅ |
| ~~Bell on turn completion~~ | **Done** (iter 6) | W1-4 ✅ |
| ~~`/init`, `/review` canned prompts~~ | **Done** (iter 6) | W1-5 ✅ |
| ~~Full-screen transcript pager (Codex `/transcript`)~~ | **Done** (iter 6) | W1-6 ✅ |
| ~~`/status` session + token usage~~ | **Done** (iter 7) | W2-1 ✅ |
| ~~`/model`~~ picker (model selection) | **Done** (iter 7) | W2-2 ✅ |
| ~~`/mode`~~ session-mode switch (covers `/plan`, approval modes) | **Done** (iter 7) | W2-3 ✅ |
| ~~`/resume`~~ list & load prior sessions | **Done** (iter 7) | W2-4 ✅ |
| ~~`@`-file mentions~~ with fuzzy completion | **Done** (iter 7) | W2-5 ✅ |
| **Sign-in** when the agent requires auth | **Gap** | W2-6 |
| External `$EDITOR` compose | **Gap** | W3-1 |
| `/diff` working-tree diff viewer | **Gap** | W3-2 |
| Image paste / attachment | **Gap** | W3-3 |
| MCP server mgmt, sandbox, memories, skills, hooks, voice, cloud | N/A | — |

## Delivery plan (one slice at a time)

Each row is one coherent, gate-green (`cargo xtask ci`), Harness-tested,
docs-updated slice, merged back under the serialized lock
(`docs/merging.md`). Ordered by value ÷ risk.

**Wave 1 — pure client-side (no protocol change, low risk):**

1. **W1-1 Input history** ✅ *(landed, iter 6)* — submitted prompts recalled
   with ↑/↓ (readline rule: only when the cursor can go no further within the
   draft); the half-typed draft restored on the way back; deduped + persisted
   to `~/.config/rstui/acp-client.history` (`src/history.rs`, mirrors the
   theme-persistence seam; `RSTUI_ACP_HISTORY` overrides; inert under
   `cargo test`).
2. **W1-2 `/copy`** ✅ *(landed, iter 6)* — copies the last agent answer to
   the system clipboard via OSC 52 (`src/clipboard.rs`, a faithful port of
   the kitchen-sink helper; dependency-free, terminal-gated, best-effort with
   a system-line breadcrumb).
3. **W1-3 Terminal title** ✅ *(landed, iter 6)* — OSC 2 reflecting the
   session (`src/title.rs`: pure `session_title` + sanitize, unit-tested;
   terminal-gated emit; cleared on exit; `● … approval needed` attention
   cue). Emitted only on change, from the single `update` interception.
4. **W1-4 Bell** ✅ *(landed, iter 6)* — BEL on `TurnEnded` (the "your turn"
   cue), `/bell` toggles per session, `RSTUI_ACP_BELL=0|false|no|off` sets
   the startup default; terminal-gated emit reusing `src/title.rs`.
5. **W1-5 `/init` + `/review`** ✅ *(landed, iter 6)* — two built-ins sending
   agent-agnostic "create/improve AGENTS.md" / "review my uncommitted
   changes" prompts through a new shared `send_user_prompt` (also de-duped
   the composer's own send path).
6. **W1-6 Transcript pager** ✅ *(landed, iter 6)* — `/transcript`
   full-screen overlay: scroll (`jk`/arrows/PgUp-Dn/`g`/`G`) + incremental
   `/` substring filter, a pure projection reusing `transcript_lines`
   verbatim and the chat's clamp-on-render scroll model (`PagerState` is the
   whole reducer surface).

**Wave 1 complete.**

**Wave 2 — ACP-wired:**

7. **W2-1 `/status`** ✅ *(landed, iter 7)* — the ACP `usage_update`
   notification is parsed in the driver's JSON arm (no `unstable_session_usage`
   feature needed) into `AcpEvent::Usage`; a `/status` overlay shows agent,
   cwd, connection, turn state, **context tokens + % of window**, theme,
   keymap, history size, bell.
8. **W2-2 `/model`** ✅ *(landed, iter 7)* — the driver lifts
   `NewSessionResponse.models` (`SessionModelState`) into `AcpEvent::Models`;
   a `/model` picker (↑↓/jk, current marked) issues `session/set_model` via
   `DriverCmd::SetModel`; the `ModelSelected` ack updates the active model
   (also shown in `/status`). *Reasoning-effort* is not a separate ACP axis —
   agents expose it either as distinct models here or as session
   modes/config (W2-3).
9. **W2-3 `/mode`** ✅ *(landed, iter 7)* — `NewSessionResponse.modes`
   (ungated `SessionModeState`) → `AcpEvent::Modes`; a `/mode` picker issues
   the **typed** `SetSessionModeRequest` (`session/set_mode`), and an
   agent-initiated `current_mode_update` notification is reflected back
   (`AcpEvent::ModeChanged`). Shown in `/status`. This is how Codex's
   plan/approval modes reach a generic client.
10. **W2-4 `/resume`** ✅ *(landed, iter 7)* — `src/sessions.rs` (a
    persisted `SessionStore`, mirrors `history.rs`): every
    `AcpEvent::SessionStarted` records `(id, cwd, agent, when)`; the
    `/resume` picker lists them newest-first and Enter issues the **typed**
    `LoadSessionRequest` (`session/load`) — the agent replays the
    conversation through the existing notification path (transcript cleared
    first to avoid mixing).
11. **W2-5 `@`-mentions** ✅ *(landed, iter 7)* — a composer `@token`
    opens a fuzzy file-completion popup over a **bounded** cwd scan
    (`scan_files`: depth/count caps, VCS/`target`/`node_modules` pruned;
    cached per token), ranked by `rank_paths` (basename-prefix > substring >
    path; pure, unit-tested). Tab/Enter inserts the path into the prompt
    text — the agent resolves `@path` itself, the Codex composer UX (formal
    ACP resource-link blocks deferred; plain text is what agents accept
    today). Mutually exclusive with the slash popup; `user@host` is not a
    mention.
12. **W2-6 Sign-in** — when `initialize`/prompt reports auth required, run the
    `authenticate` method per `AuthCapabilities`.

**Wave 3 — heavier / lower priority:**

13. **W3-1** external `$EDITOR` compose. **W3-2** `/diff` viewer. **W3-3**
    image paste.

Progress is tracked in the session task list; this document is the spec the
slices implement against.
