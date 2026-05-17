# `rstui-core` reference

`rstui-core` is the dependency-free substrate every other crate builds on.
Every public type is a **pure value** — total, panic-free, no terminal, no
async, no clock. This page is the API reference, by module.

> Convention used below: only the public surface is shown, signatures
> condensed. "Total" means *arbitrary input never panics and never leaves an
> invalid state* — it is a guarantee, not a hope ([ADR 0012](adr/0012-widget-composition-and-layout-model.md)).

- [Geometry](#geometry) · [Style](#style) · [Stylize](#stylize) ·
  [Layout](#layout) · [Buffer](#buffer) · [Backend](#backend) ·
  [Terminal](#terminal) · [Event](#event) · [EventSource](#eventsource) ·
  [Widget](#widget) · [Text](#text) · [TextEdit](#textedit) ·
  [TextArea](#textarea) · [Scroll](#scroll) · [Selection](#selection) ·
  [Focus](#focus)

## Geometry

`module rstui_core::geometry`. Origin `(0,0)` is top-left; x grows right, y
grows down. Constructors clamp so edges never overflow `u16`.

```rust
struct Position { x: u16, y: u16 }
Position::ORIGIN; Position::new(x, y); From<(u16,u16)>

struct Size { width: u16, height: u16 }
Size::new(w, h); size.area() -> u32; size.is_empty() -> bool

struct Margin { horizontal: u16, vertical: u16 }
Margin::new(h, v)

struct Rect { x: u16, y: u16, width: u16, height: u16 }
Rect::ZERO; Rect::new(x,y,w,h); Rect::from_size(size)
rect.area() -> u32                rect.is_empty() -> bool
rect.left()/right()/top()/bottom() -> u16   // half-open: right/bottom exclusive
rect.position() -> Position       rect.size() -> Size
rect.contains(pos) -> bool        rect.intersects(other) -> bool
rect.intersection(other) -> Rect  // zero-area if disjoint
rect.union(other) -> Rect
rect.inner(margin) -> Rect        // shrink; zero-area if margin too large
rect.positions() -> impl Iterator<Item = Position>   // row-major
```

`Rect` is the universal currency: `Layout::split` returns `Rect`s, every
widget renders into a `Rect`, container widgets expose `.inner(area) -> Rect`
so you nest by passing the child rect — there is no parent/child object graph.

## Style

`module rstui_core::style`.

```rust
enum Color {
    Reset,
    Black, Red, Green, Yellow, Blue, Magenta, Cyan, Gray,
    DarkGray, LightRed, LightGreen, LightYellow,
    LightBlue, LightMagenta, LightCyan, White,
    Indexed(u8),        // 256-color palette
    Rgb(u8, u8, u8),    // 24-bit truecolor
}

struct Modifier(/* bitset */);
Modifier::{EMPTY, BOLD, DIM, ITALIC, UNDERLINED, SLOW_BLINK,
           RAPID_BLINK, REVERSED, HIDDEN, CROSSED_OUT}
m.contains(o) / m.is_empty() / m.union(o) / m.difference(o)   // + BitOr

struct Style { fg: Option<Color>, bg: Option<Color>,
               add_modifier: Modifier, sub_modifier: Modifier }
Style::new()                 // changes nothing (unset = inherit)
Style::reset()               // explicit reset to terminal default
.fg(c) .bg(c) .add_modifier(m) .remove_modifier(m)
.patch(other) -> Style       // overlay `other` on top of `self`
```

`Style` is a *patch*, not an absolute: unset colors inherit, and modifiers are
split into add/remove sets so partial styles compose predictably. A cell's
final style is `text.style` ▸ `line.style` ▸ `span.style` (see [Text](#text)).

## Stylize

`module rstui_core::stylize`. One blanket trait giving fluent shorthands to
anything that carries a `Style` (`&str`, `String`, `Span`, `Line`, `Text`,
`Style`).

```rust
"error".red().bold()                 // -> Span
Line::raw("info").cyan().on_blue()   // -> Line
```

Every named color has a foreground form (`.red()`) and a background form
(`.on_red()`); every modifier has an on form (`.bold()`) and an off form
(`.not_bold()`). Plus `.fg(c)`, `.bg(c)`, `.reset()`,
`.add_modifier(m)`, `.remove_modifier(m)`.

## Layout

`module rstui_core::layout`. Deterministic, integer-only rectangle division —
no float rounding, no constraint solver.

```rust
enum Alignment { Left, Center, Right }            // default Left
enum Direction { Horizontal, Vertical }           // default Vertical; .opposite()

enum Constraint {
    Length(u16),        // exact cells
    Percentage(u16),    // 0..=100 of the span
    Ratio(u32, u32),    // numerator / denominator
    Min(u16),           // at least; absorbs leftover
    Max(u16),           // at most; absorbs leftover
    Fill(u16),          // weighted share of leftover
}
From<u16> for Constraint  // u16 -> Length

struct Layout { /* direction, constraints, margins, spacing */ }
Layout::new(dir, constraints) / Layout::vertical(cs) / Layout::horizontal(cs)
.direction(d) .constraints(cs) .margin(n)
.horizontal_margin(n) .vertical_margin(n) .spacing(n)
.split(area) -> Vec<Rect>
.areas::<N>(area) -> [Rect; N]      // destructure into a fixed array
```

The resolution algorithm (documented and reproducible): reserve spacing →
resolve fixed constraints and `Min` floors → distribute leftover by weight →
scale down proportionally on overflow → hand the rounding remainder to the
last segment so sizes always sum to the span exactly.

```rust
let [header, body, footer] = Layout::vertical([
    Constraint::Length(1),
    Constraint::Fill(1),
    Constraint::Length(1),
]).areas(area);
```

## Buffer

`module rstui_core::buffer`. A row-major grid of `Cell`s covering a `Rect`.
**Every method is bounds-safe and panic-free** — out-of-bounds writes are
silently ignored. This is the contract third-party widgets build on.

```rust
struct Cell { symbol: char, fg: Color, bg: Color, modifier: Modifier }
Cell::EMPTY; Cell::new(symbol)
cell.apply_style(style) / cell.style() -> Style / cell.reset()

struct Buffer { /* area + cells */ }
Buffer::empty(area) / Buffer::filled(area, cell)
buf.area() -> Rect            buf.cells() -> &[Cell]
buf.get(pos) -> Option<&Cell> buf.get_mut(pos) -> Option<&mut Cell>

// the public cell-stamping contract every widget uses:
buf.set_cell(pos, symbol, style)               // single cell; OOB ignored
buf.set_str(pos, text, style) -> Position      // clips at right edge
buf.set_style(area, style)                     // restyle a region
buf.clear_region(area)                         // the opaque-overlay primitive

buf.reset() / buf.resize(area)
buf.diff(&previous) -> Vec<(Position, &Cell)>  // changed cells only
```

`clear_region` is what makes modals/drawers/popovers opaque: blank the overlay
rect, then render the overlay into it. `diff` is what makes rendering cheap:
the terminal only receives the cells that changed since last frame.

## Backend

`module rstui_core::backend`. "Make the screen look like these cells." Not
object-safe; the runtime is monomorphized over a concrete backend.

```rust
trait Backend {
    type Error: std::error::Error;
    fn draw<'a, I: IntoIterator<Item=(Position, &'a Cell)>>(&mut self, cells: I)
        -> Result<(), Self::Error>;
    fn hide_cursor / show_cursor(&mut self) -> Result<(), Self::Error>;
    fn cursor_position(&mut self) -> Result<Position, Self::Error>;
    fn set_cursor_position(&mut self, pos) -> Result<(), Self::Error>;
    fn clear(&mut self) -> Result<(), Self::Error>;
    fn size(&self) -> Result<Size, Self::Error>;
    fn flush(&mut self) -> Result<(), Self::Error>;
}

struct TestBackend { /* in-memory; Error = Infallible */ }
TestBackend::new(w, h)   .buffer() -> &Buffer   .cursor_visible() -> bool
.resize(w, h)            impl Display            // one line per row — the snapshot
```

`TestBackend` is why every test is TTY-free: render into it, then assert on
`format!("{backend}")`. The crossterm backend lives in
[`rstui-crossterm`](runtime.md#crossterm-the-live-terminal).

## Terminal

`module rstui_core::terminal`. Drives the double-buffered render loop.

```rust
struct Frame<'a> { /* buffer + area + count + cursor */ }
frame.area() -> Rect          // anchored at origin (fullscreen viewport)
frame.size() -> Size
frame.count() -> usize        // zero-based frame number: the animation clock
frame.buffer_mut() -> &mut Buffer
frame.render_widget(widget, area)
frame.set_cursor_position(pos)

struct Terminal<B: Backend> { /* two buffers, front/back */ }
Terminal::new(backend) -> Result<Self, B::Error>
.backend() / .backend_mut() / .into_backend()
.area() -> Rect
.draw(|frame| { ... }) -> Result<CompletedFrame, B::Error>
.flush() / .clear() / .resize(area)
```

`Terminal::draw` *is* the render loop in one call: autoresize → run your
closure → diff against the previous buffer → send the diff to the backend →
position/hide the cursor → flush → swap buffers. `frame.count()` is the
deterministic animation clock that `Spinner`, `Skeleton` and friends project
(no wall clock anywhere).

## Event

`module rstui_core::event`. Everything the input layer can deliver.

```rust
struct KeyModifiers(u8);  // NONE SHIFT CONTROL ALT SUPER + BitOr
enum KeyCode {
    Char(char), F(u8), Backspace, Enter,
    Left, Right, Up, Down, Home, End, PageUp, PageDown,
    Tab, BackTab, Delete, Insert, Esc,
}
enum KeyEventKind { Press, Repeat, Release }   // default Press
struct KeyEvent { code, modifiers, kind }
KeyEvent::new(code, mods) / ::from_code(code) / ::char(c) / .is_press()

enum MouseButton { Left, Right, Middle }
enum MouseEventKind { Down(b), Up(b), Drag(b), Moved,
                      ScrollUp, ScrollDown, ScrollLeft, ScrollRight }
struct MouseEvent { kind, position: Position, modifiers }

enum Event { Key(KeyEvent), Mouse(MouseEvent), Resize(Size),
             FocusGained, FocusLost, Paste(String) }
event.as_key_press() -> Option<KeyEvent>   // press or repeat, not release
event.as_key() / event.as_mouse()
event.is_key(KeyCode) -> bool              // bare press, no modifiers
```

`KeyEvent` is `Eq + Hash`, so a keymap can be a `HashMap<KeyEvent, Msg>`. A
*tick* is deliberately **not** an `Event` variant — terminal input and runtime
timing are distinct concepts ([ADR 0006](adr/0006-runtime-tick-and-loop-model.md)).

## EventSource

`module rstui_core::event_source`. The input dual of `Backend`.

```rust
trait EventSource {
    type Error: std::error::Error;
    fn poll_event(&mut self, timeout: Option<Duration>)
        -> Result<Option<Event>, Self::Error>;
    // Some(dur): wait at most dur; None on timeout (transient miss)
    // None timeout: block until an event or permanent end-of-input
}

struct TestEventSource { /* replays a fixed script; ignores timeout */ }
TestEventSource::new() / ::with_events(iter) / .push(e) / .extend(iter)

struct ChannelEventSource { /* fed by another thread over mpsc */ }
ChannelEventSource::new() -> (Self, Sender<Event>)
ChannelEventSource::from_receiver(rx)
```

`TestEventSource` makes input deterministic in tests; `ChannelEventSource`
lets a background thread feed the live loop (see the `external_input`
runtime example).

## Widget

`module rstui_core::widget`. The single rendering seam.

```rust
trait Widget {
    fn render(self, area: Rect, buf: &mut Buffer) where Self: Sized;
}
impl Widget for &str / String        // one clipped line
impl<W: Widget> Widget for Option<W>  // Some renders, None is a no-op
```

Authoring a third-party widget is exactly this: depend on `rstui-core`,
implement `Widget`, stamp through `buf.set_cell`/`set_str`, tolerate a
zero-area `area` by returning early, and snapshot-test against `TestBackend`.
Every widget in [`rstui-widgets`](widgets/README.md) is a worked example of
this and nothing more.

## Text

`module rstui_core::text`. The styled-text model.

```rust
struct Span<'a> { style: Style, content: Cow<'a, str> }
Span::raw(s) / ::styled(s, style) / .style(s) / .patch_style(s) / .width()

struct Line<'a> { style: Style, alignment: Option<Alignment>, spans: Vec<Span> }
Line::raw(s) / ::styled(s, style) / .spans(iter) / .push_span(s)
.style(s) .alignment(a) .left_aligned() .centered() .right_aligned() .width()

struct Text<'a> { style: Style, alignment: Option<Alignment>, lines: Vec<Line> }
Text::raw(s)   // splits on '\n'
::styled(s, style) / .lines(iter) / .push_line(l)
.style(s) .alignment(a) .left_aligned() .centered() .right_aligned()
.width() .height()
```

`Span`, `Line` and `Text` all implement `Widget` and `Styled` (so `Stylize`
shorthands apply). Many widgets accept `impl Into<Line>` / `impl Into<Text>`,
so `"plain"`, a styled `Span`, or a multi-span `Line` are all valid arguments.

## TextEdit

`module rstui_core::text_edit`. The single-line editing model — a `String` +
a character-indexed cursor. **Total**: the cursor never lands mid-codepoint.
Lives in your model; the `Input`/`MaskedInput` widgets project it.

```rust
struct TextEdit { /* value + cursor */ }
TextEdit::new() / ::from_value(s)            // from_value: cursor at end
.value() -> &str  .cursor() -> usize  .len()  .is_empty()
.set_value(s) .clear() .set_cursor(i)        // set_cursor clamped to 0..=len
.move_left()/right()/home()/end()            // move_*: bool "did it move"
.insert_char(c) .insert_str(s)               // s = paste path
.delete_backward() .delete_forward()          // -> bool "changed"
```

## TextArea

`module rstui_core::text_area`. The multi-line editing model — a non-empty
`Vec<String>` of logical lines + a `(row, col)` cursor with a sticky goal
column for vertical motion. **Total.** The `Editor` widget projects it.

```rust
struct TextArea { /* lines + (row,col) + goal_col */ }
TextArea::new() / ::from_value(s)            // split on '\n'; cursor at end
.lines() -> &[String]  .line(r) -> Option<&str>  .cursor() -> (usize,usize)
.row_count() (>=1)  .is_empty()
.set_value(s) .clear() .set_cursor(r, c)
.insert_char(c)  // '\n' splits   .insert_str(s)  .insert_newline()
.delete_backward() .delete_forward()
.move_left()/right()/up()/down()/home()/end()
.move_doc_start()/doc_end()  .move_page_up(n)/page_down(n)
impl Display  // '\n'-joined
```

## Scroll

`module rstui_core::scroll`. The scroll-position model: an offset plus a
sticky-bottom intent. **Total.** The `Scrollbar`/`ScrollView` widgets project
it.

```rust
struct ScrollState { /* offset + follow_tail */ }
ScrollState::new()      // offset 0, FOLLOWING (streaming-transcript default)
ScrollState::default()  // offset 0, INERT (top-anchored pane)
.offset() .following()
.set_offset(n) .clamp(content_len, viewport_len)
.at_end(content_len, viewport_len) -> bool
.scroll_by(delta, content_len, viewport_len)   // re-arms follow at end
.scroll_to_top()  .scroll_to_end(content_len, viewport_len)
.on_content_change(content_len, viewport_len)  // snap to end if following
.show(child_y, child_h, viewport_len, content_len)
```

`new()` vs `default()` is the one deliberate divergence in the model family:
`new()` follows the tail (a log/chat transcript), `Default` is inert (a
fixed pane).

## Selection

`module rstui_core::selection`. An anchor/active pair, row-major terminal
stream semantics. **Total** for any buffer/selection.

```rust
struct Selection { /* Option<(anchor, active)> */ }
Selection::new() / .is_empty()
.start(pos) .extend(pos) .clear()
.ordered() -> Option<(Position, Position)>   // (top_left, bottom_right)
.contains(pos) -> bool

selected_text(&buffer, &selection) -> String  // anchor→EOL, mid rows, SOL→active
```

## Focus

`module rstui_core::focus`. Caller-owned focus: a token type plus an ordered
ring with a modal scope stack. **Total and panic-free.** `update` steps it;
`view` only reads it ([ADR 0004](adr/0004-focus-routing-architecture.md)).

```rust
struct FocusId(u64);
FocusId::new(raw)                 // const NAME: FocusId = FocusId::new(0);

struct FocusRing { /* order + focused + scope stack */ }
FocusRing::new() / ::with_ids(iter)   // with_ids: first focused
.len() .is_empty() .contains(id) .focused() -> Option<FocusId> .is_focused(id)
.focus(id) -> Option<FocusId>           // no-op if id not in active scope
.focus_next() / .focus_prev()           // wrapping
.push_scope(iter) -> Option<FocusId>    // open a modal: trap focus, capture prev
.pop_scope() -> Option<FocusId>         // close: validate-restore the captured id
.scope_depth() -> usize  .in_scope() -> bool   // reducer gates background keys
```

```rust
let mut ring = FocusRing::with_ids([NAME, EMAIL, SUBMIT]);
ring.focus_next();              // EMAIL
ring.push_scope([OK, CANCEL]);  // modal opens; NAME/EMAIL/SUBMIT trapped out
ring.focus(NAME);               // no-op while trapped
ring.pop_scope();               // restores SUBMIT
```

## Examples

```sh
cargo run -p rstui-core --example buffer_demo     # draw → diff → TestBackend
cargo run -p rstui-core --example terminal_loop   # 4 frames, frame.count() clock
```

Next: the widgets that project these models — [Component library](widgets/README.md) —
and the loop that drives them — [Runtime](runtime.md).
