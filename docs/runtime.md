# Runtime

`rstui-runtime` is the Elm-style application model: a trait-based `App`, a
side-effect `Cmd` returned from `update`, a deterministic `Harness` for
tests, and the live `run` loop they **share**. The headless harness is not a
copy of the loop — it is the loop with a `TestBackend` swapped in.

## The `App` contract

```rust
trait App {
    type Message;

    fn init(&mut self) -> Cmd<Self::Message> { Cmd::none() }
    fn on_event(&self, event: Event) -> Option<Self::Message> { None }
    fn tick_rate(&self) -> Option<Duration> { None }
    fn on_tick(&self) -> Option<Self::Message> { None }

    fn update(&mut self, message: Self::Message) -> Cmd<Self::Message>;  // required
    fn view(&self, frame: &mut Frame<'_>);                               // required
}
```

| Seam | `self` | Responsibility |
|------|--------|----------------|
| `init` | `&mut` | One-shot startup work, as a `Cmd` (load data, arm a tick). Runs before the first frame. |
| `on_event` | `&self` | Map an input `Event` to an optional message. Reads state to decide *meaning*; never mutates. |
| `tick_rate` | `&self` | The animation cadence as a pure function of state — `Some(dur)` to wake periodically, `None` to block purely on input. |
| `on_tick` | `&self` | Map the *passage of time* to a message. The temporal dual of `on_event`. |
| `update` | `&mut` | **The only place state changes.** Fold one message in, return a `Cmd`. |
| `view` | `&self` | Pure projection of state into the `Frame`. The frame starts blank every time. |

The split is the whole point: `on_event`/`on_tick`/`view` are read-only, so
they can never desync from `update`. See
[ADR 0006](adr/0006-runtime-tick-and-loop-model.md).

## `Cmd`: describing effects

A `Cmd` is an ordered *description* of side effects `update` hands back to the
runtime — not code that runs inline. Composition is concatenation; order is
always preserved.

```rust
Cmd::none()                              // state changed, nothing else to do
Cmd::quit()                              // stop after the current command settles
Cmd::message(m)                          // feed m straight back into update
Cmd::perform(|| -> M { ... })            // run work, feed its message back
Cmd::tick(delay, || -> M { ... })        // one-shot timer
Cmd::every(period, || -> M { ... })      // wall-clock-aligned repeat
Cmd::batch([cmd, cmd, ...])              // concatenate, order preserved
cmd.len() / cmd.is_empty()
```

Where the work runs is the *executor's* choice, and it is the only thing that
varies between test and production ([ADR 0008](adr/0008-async-command-executor.md)):

| Loop | Executor | `perform`/`tick` behaviour |
|------|----------|----------------------------|
| `Harness`, `run` | inline | runs *now*, timer delay collapses to zero — deterministic |
| `run_threaded` | thread-per-command | off the render loop; timers actually wait |
| `run_pooled` | bounded worker pool | like threaded, hard concurrency cap |
| `run_async` *(feature `async`)* | tokio | `spawn_blocking` + `tokio::time` |

The reducer is **identical** in every case. Only IO multiplexing changes.

### Command settling

After every input the runtime processes commands to a fixed point: a
`Cmd::perform` message re-enters `update`, whose returned command is processed
too, breadth-first and in order, until no work remains. `Cmd::quit` stops the
program; further input is ignored. A pathological app that emits messages
without end is bounded by a command budget (default 1024) and panics rather
than hanging. This `settle` core is shared verbatim by the `Harness` and every
`run*` loop — that shared core is *why* the harness is an exact stand-in.

## The live `run` loop

```rust
fn run        <A,B,S>(app, backend, &mut events) -> Result<A, RunError<…>>
fn run_threaded<A,B,S>(app, backend, &mut events) -> Result<A, RunError<…>>
fn run_pooled  <A,B,S>(app, backend, &mut events, workers: NonZeroUsize) -> …
#[cfg(feature="async")]
async fn run_async<A,B,S>(app, backend, &mut events) -> …   // AsyncEventSource

enum RunError<R, I> { Backend(R), Input(I) }
```

Flow: `init` → settle → render the first frame → block on
`poll_event(None)` (or `poll_event(Some(until_next_tick))` if the app
declares a tick rate) → input through `on_event`→`update`, a tick through
`on_tick`→`update` → settle → repaint (a no-op diff if nothing changed). A
`Cmd::quit` stops the loop; so does end-of-input *when the app is not
ticking* (a ticking app is in animation mode — a bounded `None` is a tick,
not the end).

`run` and `run_threaded`/`run_pooled` are generic over **any** `Backend` +
`EventSource`, so the same app runs on crossterm, on a channel, or on a test
double with no code change.

## The `Harness`: deterministic, TTY-free

```rust
Harness::new(app, width, height)              // runs init, settles, renders frame 0
        .with_command_budget(n)
harness.handle(event)                          // on_event → update → settle → render
harness.message(msg)                           // straight into update (skip on_event)
harness.resize(w, h)                           // + delivers Event::Resize
harness.tick()                                 // on_tick → update → settle → render
harness.app() -> &A        harness.is_running() -> bool
harness.backend() -> &TestBackend
harness.snapshot() -> String                   // the screen as a string — assert this
```

No threads, no wall clock, no terminal. `harness.tick()` advances time
explicitly, so animation is deterministic. This is the backbone of
[Testing](testing.md).

## crossterm: the live terminal

`rstui-crossterm` is the only crate with an external dependency
([ADR 0001](adr/0001-terminal-backend-strategy.md)). It provides the four
seams the live loop needs and one call that composes them.

```rust
struct CrosstermBackend<W: Write>;             // Backend over any io::Write
CrosstermBackend::new(stdout) / .writer() / .writer_mut()

struct CrosstermEventSource;                   // EventSource over poll/read
CrosstermEventSource::new()

struct LifecycleOptions { raw_mode, alternate_screen, mouse,
                          bracketed_paste, focus_events }
struct TerminalGuard<B: Backend>;              // panic-safe RAII; restores on drop
TerminalGuard::with_options(backend, opts)

fn from_crossterm(ev) -> Option<rstui_core::event::Event>   // event translation

fn run_app<A: App>(app: A) -> Result<(), CrosstermRunError>
```

`run_app` is the canonical wiring: it owns the alternate screen, raw mode,
mouse/paste/focus capture, the live loop, and a panic hook that restores the
terminal *before* the panic prints. From `main` it is one line:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    run_app(MyApp::default())?;
    Ok(())
}
```

`TerminalGuard` restores *exactly* the modes it enabled, even while unwinding
from a panic — so a crash never leaves a wrecked terminal.

## Example index

`crates/rstui-runtime/examples/` — each teaches one part of the contract:

| Example | Teaches | Run |
|---------|---------|-----|
| `counter` | the whole contract end to end; `Harness` drives it deterministically | `cargo run -p rstui-runtime --example counter` |
| `spinner` | the tick seam (`tick_rate`/`on_tick`); `Harness::tick()` advances time | `cargo run -p rstui-runtime --example spinner` |
| `app_shell` | full-screen layout, focus ring traversal, resize reflow, mouse hit-test, paste, OS focus | `cargo run -p rstui-runtime --example app_shell` |
| `external_input` | `run` with a non-crossterm `ChannelEventSource` fed by a thread | `cargo run -p rstui-runtime --example external_input` |
| `background_load` | `Cmd::perform` + `Cmd::tick` retry; same reducer, inline vs threaded | `cargo run -p rstui-runtime --example background_load` |

`crates/rstui-crossterm/examples/` — the same apps, live on a real TTY:

| Example | Teaches | Run |
|---------|---------|-----|
| `run_app` | one call from `main` to a live terminal | `cargo run -p rstui-crossterm --example run_app` |
| `fullscreen_shell` | the `app_shell` reducer, unchanged, on a real TTY | `cargo run -p rstui-crossterm --example fullscreen_shell` |

The proof rstui keeps making: the same `App`, same reducer, runs headless in a
deterministic test and live on a terminal with **no changes**.
