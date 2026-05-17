# Testing

rstui's defining property: **the same `App` you ship is the one you test**,
deterministically, with no terminal, no threads and no clock. There is no mock
layer and no test-only copy of the app.

Three layers, smallest first.

## 1. Widget snapshots (`TestBackend`)

A widget is a pure projection, so render it into an in-memory `TestBackend`
and assert on its string form. This is exactly what every `*_demo.rs` example
is — a runnable demo that doubles as a snapshot test.

```rust
use rstui_core::{Terminal, TestBackend, Style};
use rstui_core::layout::Alignment;
use rstui_widgets::Button;

#[test]
fn button_renders_centered_label() {
    let mut term = Terminal::new(TestBackend::new(20, 1)).unwrap();
    term.draw(|f| f.render_widget(
        Button::new("OK").alignment(Alignment::Center).focused(true),
        f.area(),
    )).unwrap();

    assert_eq!(term.backend().to_string(), "        OK          \n");
}
```

Run any widget's demo to see its frame printed:

```sh
cargo run -p rstui-widgets --example button_demo
```

Because the buffer is bounds-safe and every widget is *total*, a snapshot test
at a tiny or zero-area size proves the no-panic guarantee too.

## 2. Application behaviour (`Harness`)

`Harness` drives a whole `App` over a `TestBackend` with the **same `settle`
core the live loop uses** — same ordering, same command settling, just
synchronous and clock-free.

```rust
use rstui_runtime::Harness;
use rstui_core::event::{Event, KeyEvent, KeyCode};

#[test]
fn counter_increments_and_quits() {
    let mut h = Harness::new(Counter::default(), 40, 1);   // init + frame 0 done

    h.handle(Event::from(KeyEvent::char('+')));
    h.handle(Event::from(KeyEvent::char('+')));
    assert_eq!(h.app().n, 2);
    assert!(h.snapshot().contains("count: 2"));

    h.message(Msg::Quit);                  // inject straight into update
    assert!(!h.is_running());
}
```

- `handle(event)` — the full path: `on_event` → `update` → settle → render.
- `message(msg)` — skip `on_event`, test the reducer directly.
- `resize(w, h)` — resize + deliver `Event::Resize` (test reflow).
- `tick()` — advance time one tick deterministically (test animation).
- `snapshot()` — the rendered screen as a string; assert on it.
- `app()` — the model, for state assertions.

### Testing animation

`tick_rate`/`on_tick` are driven explicitly — no wall clock, so a spinner test
is deterministic:

```rust
let mut h = Harness::new(Spinner::default(), 10, 1);
let f0 = h.snapshot();
h.tick();
assert_ne!(h.snapshot(), f0);   // the frame advanced
```

### Testing effects

Under `Harness` the command executor is **inline**: `Cmd::perform` runs now and
`Cmd::tick` delays collapse to zero. So a background-load + retry flow is
asserted with no threads and no clock — see the `background_load` runtime
example, which is the same app under `Harness`, `run`, and `run_threaded`.

## 3. End-to-end via VHS (the real binary)

Layers 1–2 test the reducer and the projection. The VHS e2e layer tests the
*actual crossterm binary* — real escape sequences, real terminal sizing — by
driving the kitchen sink with scripted keystrokes and diffing its captured
output against a committed golden file.

```sh
cargo xtask record --e2e        # drive the live binary, capture .txt + .gif
cargo xtask record --check      # re-capture and diff against goldens (regression)
```

This complements, not replaces, the `Harness` tests: `Harness` proves the
logic; VHS proves the wiring (crossterm translation, lifecycle, sizing). See
[Recording](recording.md) for the tape format and
[Kitchen sink](kitchen-sink.md) for the multi-resolution captures.

## What runs in CI

The fast loop (`cargo xtask ci`) runs five gates fail-fast: `fmt`,
`lint-names`, `clippy`, `doc`, `test`. Layers 1 and 2 above are ordinary
`#[test]`s caught by the `test` gate. Every example is compiled by
`--all-targets` under the `clippy` gate, and the `gallery`/`kitchen_sink`
tests assert their snapshots. The VHS e2e layer is opt-in (it needs the VHS
toolchain) and documented in [Recording](recording.md); it is not part of the
five-gate fast loop. See [`docs/development.md`](development.md).

## The rule of thumb

| You want to verify… | Use |
|---------------------|-----|
| a widget renders a layout correctly | `TestBackend` snapshot (layer 1) |
| a widget never panics on degenerate input | `TestBackend` at 0×0 / 1×1 (layer 1) |
| a key produces the right state change | `Harness::handle` + `app()` (layer 2) |
| reducer logic in isolation | `Harness::message` (layer 2) |
| animation advances | `Harness::tick` (layer 2) |
| an async/background effect resolves | `Harness` (inline executor) (layer 2) |
| the real terminal wiring still works | `cargo xtask record --check` (layer 3) |
