# Getting started

## Requirements

- Rust **1.85+** (the workspace pins the edition to 2024; MSRV is 1.85).
- A terminal for the live examples. The headless examples and the entire test
  suite need no TTY at all.

## Add rstui to a project

rstui is a Cargo workspace. Depend on the layers you need:

```toml
[dependencies]
rstui-core = { git = "https://github.com/andymac4182/rstui" }      # primitives + the Widget trait
rstui-widgets = { git = "https://github.com/andymac4182/rstui" }   # the concrete widget set
rstui-runtime = { git = "https://github.com/andymac4182/rstui" }   # App / Cmd / Harness / run
rstui-crossterm = { git = "https://github.com/andymac4182/rstui" } # the real terminal driver
```

`rstui-core` has **zero external dependencies**. `rstui-crossterm` is the only
crate that pulls in a third-party dependency (crossterm), deliberately isolated
([ADR 0001](adr/0001-terminal-backend-strategy.md)).

## The smallest app

Every rstui application implements the `App` trait: `update` folds a message
into state, `view` projects state into a frame, `on_event` maps input to a
message. `run_app` wires it to a real terminal in one call.

```rust
use rstui_core::event::{Event, KeyCode};
use rstui_runtime::{App, Cmd};
use rstui_runtime::Frame;
use rstui_crossterm::run_app;

#[derive(Default)]
struct Counter { n: i64 }

enum Msg { Inc, Dec, Quit }

impl App for Counter {
    type Message = Msg;

    fn on_event(&self, event: Event) -> Option<Msg> {
        match event.as_key_press()?.code {
            KeyCode::Char('+') => Some(Msg::Inc),
            KeyCode::Char('-') => Some(Msg::Dec),
            KeyCode::Char('q') => Some(Msg::Quit),
            _ => None,
        }
    }

    fn update(&mut self, msg: Msg) -> Cmd<Msg> {
        match msg {
            Msg::Inc => { self.n += 1; Cmd::none() }
            Msg::Dec => { self.n -= 1; Cmd::none() }
            Msg::Quit => Cmd::quit(),
        }
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let line = format!("count: {}   (+/- to change, q to quit)", self.n);
        frame.buffer_mut().set_str(frame.area().position(), &line, Default::default());
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    run_app(Counter::default())?;
    Ok(())
}
```

That same `Counter` is testable with **no terminal** — see [Testing](testing.md)
and [Runtime](runtime.md).

## Run the examples

Every widget ships a tiny self-contained demo under
`crates/rstui-widgets/examples/`. They render one frame through an in-memory
backend and print it — TTY-free, deterministic, and doubling as snapshot tests.

```sh
# one widget in isolation (prints its rendered frame and exits):
cargo run -p rstui-widgets --example button_demo
cargo run -p rstui-widgets --example table_demo
cargo run -p rstui-widgets --example markdown_demo

# the flagship: every widget composed into one Elm-loop app
cargo run -p rstui-widgets --example gallery
cargo test -p rstui-widgets --example gallery     # the same script, asserted

# the runtime examples (App/Cmd patterns):
cargo run -p rstui-runtime --example counter
cargo run -p rstui-runtime --example spinner          # the tick seam
cargo run -p rstui-runtime --example background_load   # Cmd::perform + retry

# the live terminal (real TTY, alternate screen, mouse/paste/focus capture):
cargo run -p rstui-crossterm --example run_app
cargo run -p rstui-crossterm --example fullscreen_shell

# the permissioned plugin host, end to end:
cargo run -p rstui-plugin-host --example permissioned_plugin
```

The full annotated list is in the [Component library](widgets/README.md) (one
demo command per widget) and [Runtime](runtime.md#example-index).

## Run the kitchen sink

The kitchen sink is one interactive full-screen app that exercises every widget
across eight screens, with live keyboard + mouse, theming, overlays and
animation. It is the fastest way to *see* the whole library.

```sh
cargo run -p rstui-kitchen-sink           # live, on your terminal
cargo test -p rstui-kitchen-sink          # the same app, driven headless
```

Keys: `1`–`8` jump to a screen, `Tab` toggles sidebar/content focus, arrows
navigate, `:` opens the command palette, `?` help, `g` settings drawer, `q`
quits. The full tour and the multi-resolution recordings are in
[Kitchen sink](kitchen-sink.md).

## Where to go next

- You want the **mental model** → [Architecture](architecture.md).
- You want **API detail** → [Core reference](core-reference.md) and the
  [Component library](widgets/README.md).
- You want to **wire an app** → [Runtime](runtime.md).
- You want to **test without a terminal** → [Testing](testing.md).
