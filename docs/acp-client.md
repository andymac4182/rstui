# ACP client

`rstui-acp-client` is a full-screen [Agent Client Protocol][acp] chat client
built **entirely on the rstui framework**. It is the proof that the framework
scales to a real, streaming, plugin-extensible application — and a worked
example of every doc on this site.

[acp]: https://agentclientprotocol.com

## Why it exists

It demonstrates, in one binary, the things the rest of the docs describe in
isolation:

- the [pure-projection model](architecture.md) at application scale;
- the [async event loop](adr/0011-async-event-loop.md) (`run_async`) driving a
  streaming JSON-RPC agent without making the reducer async;
- a [deny-by-default plugin layer](plugins.md) reusing ADR 0007's posture;
- the [`Harness`](testing.md) deterministically testing every screen with no
  terminal, no tokio, no agent process.

## Architecture: the determinism split

The crate is split so **all UI logic is headlessly testable** while the binary
owns only the terminal + async plumbing:

| Module | Role | Async? |
|--------|------|--------|
| `app` | The rstui `App`: chat model, `update` reducer, pure `view`. Every screen reachable from a `Harness` test. | no |
| `acp` | The ACP transport: a tokio task owning the agent child process, speaking `sacp` JSON-RPC, bridged to the reducer over channels. | yes |
| `registry` | The ACP registry loader + the agent-picker model. | no |
| `plugin` | The deny-by-default plugin extension layer (powerline footer, slash commands, ask-user overlay). | task |
| `input` | The async terminal `AsyncEventSource`. | yes |
| `ui` | The pure view: screen layout + widget composition. | no |

```
 terminal ──input(AsyncEventSource)──┐
                                     ▼
 agent child ◀─acp (tokio task)─▶ channels ─▶ app::update (sync reducer) ─▶ app::view ─▶ rstui frame
 plugin procs ◀─plugin layer───────┘                ▲
                                                    └ run_async (ADR 0011) multiplexes
                                                      input · agent events · plugin events · ticks
```

The reducer is synchronous and determinism-first. Async ACP streaming and
plugin processes run in separate tokio tasks and reach the reducer **only as
messages over channels** — exactly the [ADR 0011](adr/0011-async-event-loop.md)
"only IO multiplexing is async, the reducer is unchanged" rule. That is why
`app` is fully `Harness`-testable.

## Running it

```sh
cargo run -p rstui-acp-client                 # picks an agent from the registry
cargo run -p rstui-acp-client -- --help       # CLI options (agent command, plugins)
cargo test -p rstui-acp-client                # the reducer + screens, headless
```

`Config` is resolved from CLI/env *before* the terminal is taken over; `run`
then composes `TerminalGuard` + `CrosstermBackend` + the async terminal events
+ `run_async` into the live client.

## Feature timeline

The client was built in iterations, each a merged slice on `main`:

| Iter | Feature |
|------|---------|
| 1 | Slash commands + autocomplete (`/help`, `/agents`, `/clear`, `/new`, `/quit`, `/todos`, `/details`, `/log`, `/cancel`); a popup merging built-in, agent-advertised and plugin-contributed commands |
| 2 | Todos panel — a sidebar driven by ACP `session/plan_update`, auto show/hide, `/todos` toggle |
| 3 | Rich, customizable tool calls — tool-call blocks with formatted output, status badges, expandable details |
| 4 | Plugins in the TUI — the reference plugins, the plugin SDK, the in-client plugin host, the permission UX |

## The plugin layer

The ACP client's plugin layer **deliberately does not depend on
`rstui-plugin-host`**. The two solve different problems:

| | `rstui-plugin-host` | the ACP client's `plugin` layer |
|--|---------------------|----------------------------------|
| Hooks model | security capability mediation (`SessionStart`/`BeforeCapability`/`SessionEnd`) | **UI extension** (powerline footer, slash commands, ask-user overlay) |
| Wire | length-prefixed binary frames | JSON-RPC 2.0 (`rstui-acp-plugin-sdk`) |
| Shares | — | reuses ADR 0007's *posture*: separate process, deny-by-default |

The wire, transports and plugin-author API live in the sibling crate
**`rstui-acp-plugin-sdk`**, shared by the client (host side) and every
reference plugin.

### Reference plugins

Self-contained binaries under `crates/rstui-acp-client/src/bin/`, each a
worked example of the SDK:

| Plugin | Contributes |
|--------|-------------|
| `rstui-acp-plugin-powerline` | a right-aligned powerline status footer |
| `rstui-acp-plugin-git` | git branch/status footer segments |
| `rstui-acp-plugin-btw` | a `/btw` slash command |
| `rstui-acp-plugin-ask-user` | an ask-user modal overlay during a turn |
| `rstui-acp-plugin-fortune` | a `/fortune` command |
| `rstui-acp-plugin-history` | session history |
| `rstui-acp-plugin-pomodoro` | a pomodoro timer footer |
| `rstui-acp-plugin-session` | session controls |

```sh
# the client auto-discovers reference plugins; or pass them explicitly via Config
cargo run -p rstui-acp-client
```

## Where to read more

- The general plugin security model: [Plugin system](plugins.md) (ADR 0007).
- The async loop it rides on: [ADR 0011](adr/0011-async-event-loop.md).
- The model it follows: [Architecture](architecture.md).
- How to test an app like this without a terminal: [Testing](testing.md).
