//! The Rich Text screen: a [`Tabs`] strip over a scrollable [`Paragraph`], a
//! scrollable [`Markdown`] document (whose `[text](href)` exercises
//! [`Link`](rstui_widgets::Link)), a [`Mermaid`] flowchart, and a styled
//! [`Span`]/[`Line`] sampler — with a persistent [`Kbd`] strip. `←/→`
//! switches tabs, `↑/↓` scrolls.

use rstui_core::{
    Color, Constraint, KeyCode, Layout, Line, Modifier, Position, Rect, Span, Style,
    stylize::Stylize,
};
use rstui_runtime::Frame;
use rstui_widgets::mermaid::MermaidGraph;
use rstui_widgets::{
    Block, BorderType, JsonCanvas, Kbd, Markdown, Mermaid, Paragraph, Structurizr, Tabs, Wrap,
};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// The six sub-views: the three text renderers, the two diagram DSLs
/// (auto-layout C4 via [`Structurizr`] and explicit-placement
/// [`JsonCanvas`]), and the styled-span sampler.
const TABS: [&str; 6] = [
    "Paragraph",
    "Markdown",
    "Mermaid",
    "Structurizr",
    "JSON Canvas",
    "Spans",
];

/// The wrapped-paragraph body — a long-form field guide so the soft-wrap and
/// the scroll offset can be exercised over hundreds of reflowed rows.
const PROSE: &str = "\
THE RSTUI RENDERING MODEL — A FIELD GUIDE

rstui renders styled text through a three-level model: a Span is one run of characters that share one Style, a Line is a row of Spans with an optional Alignment, and a Text is a vertical stack of Lines. Nothing above the cell Buffer is ever retained between frames. A widget is handed an area and a mutable Buffer, it stamps glyphs and styles into the cells it owns, and then it is forgotten. There is no node, no element, no component instance that survives to the next paint. This is the single idea the rest of the framework is built on, and once it is internalised every other design decision reads as an obvious consequence rather than a surprise.

Immediate mode is often described as wasteful, on the theory that rebuilding the whole view every frame must cost more than mutating a small part of a retained tree. In a terminal that intuition is inverted. The output device is a grid of perhaps ten thousand cells, each holding one glyph and a handful of style bits. Composing that grid from scratch is a few hundred microseconds of perfectly cache-friendly work. Diffing a retained widget tree, reconciling it, and then walking it to produce the same grid is more work, not less, and it drags an entire class of stale-state bugs along with it. rstui simply does the cheap thing and keeps doing it.

This Paragraph is the proof. It turns on trimming soft word wrap, so the prose you are reading reflows to whatever width the panel currently has. Drag the terminal wider and the lines lengthen; drag it narrower and they break sooner. The wrap is recomputed every frame from the raw text, and it is recomputed identically whether you are scrolling, resizing, or sitting still, because the wrap function is pure: the same text at the same width always produces the same rows, with no floating-point arithmetic anywhere in the path.

Scroll it with the arrow keys, Page Up and Page Down, or the mouse wheel. The scroll offset is not owned by the widget. It is a single integer that lives in the screen's own state struct, and the Paragraph only ever reads it. When you press Down, a reducer adds one to that integer; on the next frame the widget skips that many composed rows before it starts painting. The widget has no idea you scrolled. It has no idea there was a previous frame at all. It is handed a number and it honours it, and that is the entire contract.

Because the offset is plain caller-owned state, clamping it is the caller's job too, and the caller does it at exactly the moment it has the information to do it well: in the view, where both the rendered row count and the visible height are known. The reducer increments the offset with saturating arithmetic and never worries about the end of the document. The view asks the Paragraph how many rows it composes at the current width, subtracts the viewport height, and pins the offset to that maximum before handing it over. Over-scroll therefore stops cleanly at the last screenful instead of revealing a void of blank rows below the text.

That split — unbounded intent in the reducer, bounded reality in the view — is worth dwelling on, because it recurs everywhere in rstui. Input handlers stay simple and total; they express what the user asked for. Clamping, layout, and truncation are presentation concerns and they live in the view, recomputed from scratch every frame against the geometry that actually exists. A handler never has to know how tall the panel is, and the view never has to remember what the user pressed. The two halves are decoupled by the frame boundary itself.

STYLE RESOLUTION

A Style in rstui is a small set of optional attributes: an optional foreground colour, an optional background colour, and a bitset of modifiers such as bold, italic, underline, reverse, and dim. The optionality matters. A Style does not say what the colour is; it says what the colour should become. Styles compose by patching: when a Line style sits under a Span style, the Span's set fields win and its unset fields fall through to the Line's, which in turn falls through to the widget's base style, which falls through to the theme. Resolution is a fold from the outside in, and because every layer is a patch rather than a full value, a widget can restyle a subtree by setting one field and leaving everything else untouched.

Colour itself is layered. There is the sixteen-colour ANSI palette for terminals that can do no better; there is the two-hundred-and-fifty-six colour indexed palette; and there is twenty-four-bit RGB truecolor for terminals that support it. rstui carries the author's intent at full fidelity and degrades it only at the final write, choosing the closest available representation for the terminal it actually finds itself talking to. The same view code produces a tasteful sixteen-colour rendering over an old serial link and a photographic gradient in a modern emulator, with no branches in the application.

MARKDOWN, MERMAID, AND THE SPAN SAMPLER

The other tabs on this screen are the same contract under more pressure. The Markdown tab parses a CommonMark-ish document into blocks and lays those blocks out into exactly the same Lines and Spans this Paragraph uses, which is why its links are clickable: the reducer hit-tests the click against the same rectangle the renderer drew into, with no retained DOM to consult. The Mermaid tab takes a textual graph description and routes edges through a grid, and the Span sampler shows every styling capability at once. None of them is a special case. They are all just text projected into a Buffer.

A clickable link is the sharpest illustration of why immediate mode is not a limitation here. In a retained framework a link is an object that remembers where it is and carries a click handler. In rstui there is no such object on the next frame. Instead, the same function that lays the document out can be asked where each link rendered, and the reducer compares the click position against those rectangles. The link is not a thing that persists; it is a question the reducer asks of the renderer using the geometry of the current frame. Nothing can go stale because nothing is kept.

PERFORMANCE AS A PROPERTY OF THE SHAPE

People reach for retained trees because they assume the alternative is to redo expensive work. rstui's answer is to make the work cheap and bounded instead of trying to avoid it. Composing this entire screen — parsing nothing, wrapping a few hundred lines of prose, resolving styles, and stamping cells — is a small, predictable amount of arithmetic with no allocation on the steady-state path and no pointer chasing. The cost is a function of the visible area, not of the history of the session, so it does not drift upward the longer the program runs.

That bound is the reason scrolling this document is smooth no matter how long it gets. The widget composes the rows it needs, skips the offset, paints the height, and stops. It does not compose the rows above the viewport that you have scrolled past, and it does not compose the rows below it that you have not reached. Doubling the length of the text does not double the per-frame cost, because the per-frame cost was never a function of the length; it was always a function of the window. This paragraph could be the ten-thousandth in the document and the frame that shows it would cost exactly what the first frame cost.

The deeper lesson is that performance in a terminal UI is a property of the shape of the program, not of a cache bolted onto it. Get the shape right — pure projection, caller-owned state, clamp in the view, never retain — and the fast path is the only path. There is no slow path to fall off, because there is no reconciliation step that can degrade, no tree that can grow unbounded, no subscription that can leak. The framework is fast because there is almost nothing in it, and the little that is in it runs the same way every single frame.

WIDGETS ARE FUNCTIONS, NOT OBJECTS

A rstui widget is closer to a function than to an object. It is constructed cheaply, configured with a few builder calls, consumed by a single render, and dropped. It holds no handle to the runtime, registers no callback, and owns none of the data it draws. Everything it needs arrives as borrowed references for the duration of one render call and is gone afterwards. This is why widgets compose without ceremony: there is no lifecycle to coordinate, no mounting and unmounting, no parent that must be told a child has changed, because there is no persistent child to change.

The practical consequence is that writing a new widget is unusually boring, in the best sense. You implement one method that takes an area and a Buffer and writes glyphs into it. You do not implement initialisation, teardown, change detection, or event subscription, because the framework does not have those concepts. A third-party widget is indistinguishable from a built-in one because the only thing the framework knows how to do is hand something an area and a Buffer and ask it to paint. The Buffer-stamping contract is the whole API surface a widget author has to learn.

State, correspondingly, lives entirely outside the widgets, in plain structs the application owns. A list's selected index, a text field's cursor, this document's scroll offset — all of it is application data, mutated only by the application's reducers, and merely read by the widgets that visualise it. There is exactly one place any piece of state can change, and it is never inside a widget. Debugging a rstui program is therefore mostly reading reducers, because that is the only place anything happens; the view is a deterministic function of the state and cannot surprise you.

LAYOUT IS SOLVED, NOT STORED

Layout in rstui is a constraint solve over a rectangle, run fresh every frame. You describe a split as a list of constraints — fixed lengths, proportional fills, percentages, minimums — and the solver hands back the sub-rectangles. Those rectangles are not stored anywhere. They are computed, used to render, and discarded, exactly like the wrapped rows of this paragraph. Resize the terminal and the solve simply runs again against the new outer rectangle and produces new sub-rectangles, and because the solver is pure the result is fully determined by the inputs.

This is the same idea as the text wrap, one level up. Wrapping turns a width and some text into rows; layout turns a rectangle and some constraints into rectangles. Neither remembers its previous answer, and neither needs to, because recomputing is cheap and recomputing is correct by construction. The bugs that plague retained layout systems — stale measurements, invalidation storms, a child whose size nobody told the parent about — cannot occur in a system that has no stored measurement to invalidate in the first place.

THE FRAME BOUNDARY IS THE ARCHITECTURE

Every hard guarantee rstui makes traces back to one line drawn through the program: the frame boundary. On one side of it is the reducer, which takes an event and the current state and produces the next state. On the other side is the view, which takes the state and produces a Buffer. The reducer never paints and the view never mutates. Events become state, state becomes pixels, and the two transformations never interleave. That single rule is what makes the whole thing predictable enough to reason about by reading it.

Once you trust that boundary, the rest of this guide is just consequences. Scrolling is a reducer adding to an integer and a view clamping it. A theme switch is a reducer swapping a Style table and every subsequent frame resolving against the new one with no repaint logic anywhere. A resize is the next frame's layout solve and text wrap running against new numbers. There is no incremental update machinery because there is nothing incremental: each frame is a complete, independent statement of what the screen should be, computed from scratch, cheaply, and then discarded to make room for the next one.

So scroll on. Every row you reveal is composed on demand, clamped against the real geometry, and stamped into cells by a widget that will not remember having done it. The document can be arbitrarily long and the frame cost will not move, because the cost was never about the document — it was always about the window, and the window is the size it has always been. That invariance, not a cache and not a clever diff, is what makes a terminal interface feel instant, and it is the one thing every rstui widget gets for free simply by agreeing to be a function of its inputs.

That is the whole framework. A Span, a Line, a Text. A reducer and a view. A frame boundary between them, and pure projection across it. Read it again from the top if you like — the words will wrap exactly the same way, because nothing here remembers that you already read them once.";

/// The Markdown document — a long handbook so the renderer and the scroll
/// offset get a real workout. Its links exercise the [`Link`] hit-test.
const DOC: &str = "\
# The rstui Handbook

A hand-written CommonMark-ish renderer projecting straight into the cell
buffer — **bold**, *italic*, `inline code`, headings, nested lists, ordered
lists, fenced code blocks, blockquotes, tables, and rules — all with no
retained tree behind them. Scroll it with the arrows, Page Up / Page Down,
or the mouse wheel; the offset is plain caller-owned state the widget reads.

> Everything below is rendered by the same projection contract every other
> widget in this kitchen sink uses. There is no special case for prose.

---

## 1. What this renderer is

The Markdown widget parses its source once per frame into a list of blocks,
lays those blocks out into the exact same `Line` and `Span` values the
`Paragraph` uses, and stamps them into the buffer. That is the whole story:

- **pure projection** — no DOM, no node identity, no reconciliation;
- **deterministic wrap** — same source at the same width, same rows, always;
- **clickable links** — the reducer hit-tests the click against the same
  rectangle the renderer drew into, so a link is a *question asked of the
  current frame*, not an object that persists across frames;
- **bounded cost** — the per-frame price tracks the visible window, not the
  length of the document, so this section is no cheaper than section 9.

See the source at [the rstui repo](https://github.com/andymac4182/rstui)
and the design notes in the [composition guide](https://rstui.test/compose).

## 2. The three-level text model

Everything reduces to three types:

1. a **Span** is one run of characters sharing one `Style`;
2. a **Line** is a row of `Span`s with an optional `Alignment`;
3. a **Text** is a vertical stack of `Line`s.

Widgets never retain any of it. They are handed an area and a mutable
buffer, they project these values into the cells they own, and then they
are dropped and forgotten until the next frame rebuilds them from scratch.

```rust
fn render(self, area: Rect, buf: &mut Buffer) {
    let inner = self.block.inner(area);
    for (y, row) in self.lines(inner.width)
        .into_iter()
        .skip(self.scroll as usize)
        .take(inner.height as usize)
        .enumerate()
    {
        buf.set_line(inner.x, inner.y + y as u16, &row, inner.width);
    }
}
```

That is the entire rendering path for a scrollable document: compose the
rows, skip the offset, take the height, stamp the cells, stop. Nothing
above the cell buffer outlives the call.

## 3. Styling

A `Style` is a patch, not a value — an optional foreground, an optional
background, and a modifier bitset. Styles compose from the outside in:

| Layer        | Sets                | Falls through to |
|--------------|---------------------|------------------|
| theme        | the base palette    | the terminal     |
| widget style | the widget default  | the theme        |
| line style   | a whole row         | the widget       |
| span style   | one run of glyphs   | the line         |

Because every layer is a patch, a widget can restyle a subtree by setting
*one* field and letting everything else fall through untouched. There is
no cascade to recompute and no stylesheet to invalidate.

> **Note:** colour is carried at full 24-bit fidelity and degraded only at
> the final write — sixteen-colour over an old link, photographic gradients
> in a modern emulator, *with no branches in the application*.

## 4. Lists nest, and so do quotes

- top-level item
  - a nested item, indented under its parent
  - another, with `inline code` and **emphasis**
    - and a third level, because the layout is recursive
- back to the top level

1. ordered lists keep their numbering
2. across wrapped lines and
3. across nested blocks alike

> Blockquotes can hold their own structure:
>
> - a list inside a quote
> - a second bullet
>
> > and a quote inside the quote, rendered one indent deeper.

---

## 5. Code blocks are verbatim

```
$ cargo run -p rstui-kitchen-sink
   Compiling rstui-core
   Compiling rstui-widgets
   Compiling rstui-kitchen-sink
    Finished dev profile
     Running target/debug/rstui-kitchen-sink
```

Fenced blocks are not wrapped and not reflowed; they are projected one
source line per row so that alignment-sensitive content survives intact.

## 6. Why immediate mode wins here

The output is a grid of perhaps ten thousand cells. Composing it from
scratch is a few hundred microseconds of cache-friendly arithmetic.
Diffing a retained tree to produce the *same* grid is strictly more work,
and it drags an entire class of stale-state defects along with it. rstui
does the cheap thing and keeps doing it, every frame, identically.

## 7. The frame boundary

One line is drawn through the whole program:

- on one side, the **reducer** turns an event plus the state into the
  next state — and never paints;
- on the other, the **view** turns the state into a buffer — and never
  mutates.

Events become state; state becomes cells; the two never interleave. Every
guarantee in this handbook is a consequence of that single rule.

## 8. Scrolling, precisely

The reducer adds to an integer with saturating arithmetic and never thinks
about the end of the document. The view asks this widget how many rows it
composes at the current width, subtracts the visible height, and pins the
offset to that maximum. Over-scroll stops at the last screenful instead of
revealing blank rows — the same clamp-in-the-view idiom the live log tail
and the paragraph reader both use.

## 9. The point

A terminal UI is fast when the *shape* of the program is right: pure
projection, caller-owned state, clamp in the view, never retain. Then the
fast path is the only path, because there is no reconciliation step that
can degrade and no tree that can grow without bound.

> Scroll back to the top. The document wraps exactly the same way it did
> the first time, because nothing here remembers that you already read it.

---

*End of handbook — keep scrolling to confirm the tail clamps cleanly.*";

/// The Mermaid flowchart source.
const GRAPH: &str = "\
graph TD
A[Event] --> B{on_event}
B -->|Some msg| C[update]
B -->|None| A
C --> D[view]
D --> A";

/// The Structurizr DSL source — a C4 model the widget *auto-lays-out* into
/// a System Context view (the agent describes structure, not positions).
const WORKSPACE: &str = "\
workspace \"rstui\" \"The TUI framework\" {
  model {
    dev = person \"Developer\" \"Builds a TUI app\"
    rstui = softwareSystem \"rstui\" \"Immediate-mode TUI framework\" {
      core = container \"rstui-core\" \"Buffer/layout/event\" \"Rust\"
      widgets = container \"rstui-widgets\" \"The widget catalog\" \"Rust\"
    }
    term = softwareSystem \"Terminal\" \"xterm / crossterm\" \"External\"
    dev -> widgets \"Composes\"
    widgets -> core \"Built on\"
    core -> term \"Draws to\"
  }
  views {
    systemContext rstui \"Context\" {
      include *
      autolayout lr
    }
  }
}";

/// The JSON Canvas source — the *explicit-placement* complement: every node
/// carries its own `x/y/width/height`, so the author controls the layout.
const CANVAS: &str = r#"{
  "nodes":[
    {"id":"g","type":"group","x":-20,"y":-20,"width":400,"height":220,"label":"Elm loop","color":"5"},
    {"id":"ev","type":"text","text":"Event","x":0,"y":0,"width":150,"height":70},
    {"id":"up","type":"text","text":"update()","x":210,"y":0,"width":150,"height":70,"color":"4"},
    {"id":"vw","type":"text","text":"view()","x":210,"y":110,"width":150,"height":70},
    {"id":"docs","type":"link","url":"https://jsoncanvas.org","x":470,"y":40,"width":210,"height":70}
  ],
  "edges":[
    {"id":"e1","fromNode":"ev","fromSide":"right","toNode":"up","toSide":"left","label":"msg"},
    {"id":"e2","fromNode":"up","fromSide":"bottom","toNode":"vw","toSide":"top"},
    {"id":"e3","fromNode":"vw","fromSide":"left","toNode":"ev","toSide":"bottom","label":"redraw"}
  ]
}"#;

/// The active tab and the document scroll offset.
#[derive(Debug)]
pub(crate) struct State {
    tab: usize,
    scroll: u16,
    /// MM-1/2: the [`GRAPH`] flowchart parsed **once** at construction.
    /// `GRAPH` is a `const`, so its parse is invariant — caching it here
    /// (model state the pure `view` only reads) replaces re-parsing +
    /// re-laying-out the same source every frame the Mermaid tab is up.
    /// `None` only if the const ever fails to parse, in which case the
    /// view falls back to `Mermaid::new(GRAPH)` (the identical error
    /// placeholder).
    mermaid: Option<MermaidGraph>,
}

impl State {
    /// Paragraph tab, scrolled to the top.
    pub(crate) fn new() -> Self {
        Self {
            tab: 0,
            scroll: 0,
            mermaid: Mermaid::parse(GRAPH).ok(),
        }
    }

    /// `←/→` switch tabs (resetting scroll), `↑/↓` scroll a row, `PgUp/PgDn`
    /// scroll a screenful. The offset grows unbounded with saturating
    /// arithmetic here; [`view`](Self::view) clamps it to the real geometry,
    /// so a long document scrolls deep and over-scroll pins to the tail.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        match code {
            KeyCode::Left => {
                if self.tab == 0 {
                    return ScreenOutcome::ignored();
                }
                self.tab -= 1;
                self.scroll = 0;
            }
            KeyCode::Right => {
                self.tab = (self.tab + 1).min(TABS.len() - 1);
                self.scroll = 0;
            }
            KeyCode::Up => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Down => self.scroll = self.scroll.saturating_add(1),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(15),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(15),
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// A click on the tab strip switches sub-view; a click on a link in the
    /// Markdown document follows it.
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let [tabs, body, _foot] = Self::rows(content);
        if let Some(i) = crate::screens::tab_index_at(tabs, &TABS, 2, pos) {
            self.tab = i;
            self.scroll = 0;
            return ScreenOutcome::consumed();
        }
        if self.tab == 1 && body.contains(pos) {
            // Same source / scroll / block geometry as `view` renders —
            // including the identical view-time scroll clamp — so the
            // widget's own link hit-test lands on exactly the drawn label.
            let sc = self.scroll.min(self.max_scroll(body));
            let md = Markdown::new(DOC)
                .scroll(sc)
                .block(Block::bordered().border_type(BorderType::Rounded));
            if let Some(idx) = md.link_at(pos, body) {
                if let Some(link) = md.links().get(idx) {
                    return ScreenOutcome::with_toast(
                        crate::screens::ToastLevel::Success,
                        format!("Open link → {}", link.href),
                    );
                }
            }
        }
        ScreenOutcome::ignored()
    }

    /// Wheel scroll moves the document (clamped in [`view`](Self::view)).
    pub(crate) fn on_scroll(&mut self, up: bool) {
        if up {
            self.scroll = self.scroll.saturating_sub(2);
        } else {
            self.scroll = self.scroll.saturating_add(2);
        }
    }

    /// The deepest scroll that still shows content for the active tab: the
    /// composed (post-wrap) row count of the body widget at the panel's
    /// inner width, minus the visible height. Clamping the offset to this in
    /// the view pins over-scroll to the last screenful instead of revealing
    /// blank rows below the text — the same view-time clamp the `logs`
    /// screen applies to its tail. `2`/`3` tabs do not scroll.
    fn max_scroll(&self, body: Rect) -> u16 {
        let inner = crate::screens::block_inner(body);
        let rows = match self.tab {
            0 => Paragraph::new(PROSE)
                .wrap(Wrap { trim: true })
                .line_count(inner.width),
            1 => Markdown::new(DOC).lines(inner.width).len(),
            _ => return 0,
        };
        u16::try_from(rows.saturating_sub(inner.height as usize)).unwrap_or(u16::MAX)
    }

    /// A drag-select stays inside the document body (the framed Markdown /
    /// Paragraph / Mermaid / Spans panel) — never the tab strip or footer.
    pub(crate) fn selection_region(&self, pos: Position, content: Rect) -> Option<Rect> {
        let [_, body, _] = Self::rows(content);
        body.contains(pos)
            .then(|| crate::screens::block_inner(body))
    }

    /// The three stacked bands shared by the renderer and the hit-test.
    fn rows(area: Rect) -> [Rect; 3] {
        Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(2),
        ])
        .areas(area)
    }

    /// Draw the rich-text screen.
    pub(crate) fn view(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let [tabs, body, foot] = Self::rows(area);

        frame.render_widget(
            Tabs::new(TABS)
                .selected(Some(self.tab))
                .divider("  ")
                .style(theme.body())
                .highlight_style(theme.selection()),
            tabs,
        );

        // Clamp the unbounded reducer offset to the real geometry here,
        // where both the composed row count and the viewport are known.
        let sc = self.scroll.min(self.max_scroll(body));
        match self.tab {
            0 => frame.render_widget(
                Paragraph::new(PROSE)
                    .wrap(Wrap { trim: true })
                    .scroll(Position::new(0, sc))
                    .style(theme.body())
                    .block(framed(theme, "Paragraph · ↑↓ PgUp/Dn scroll")),
                body,
            ),
            1 => frame.render_widget(
                Markdown::new(DOC)
                    .scroll(sc)
                    .style(theme.body())
                    .block(framed(theme, "Markdown · links + ↑↓ PgUp/Dn scroll")),
                body,
            ),
            2 => {
                // MM-1/2: render the parse cached once in `State::new`;
                // fall back to parsing the const only if it ever failed
                // (byte-identical — `Mermaid::new(GRAPH)` reproduces the
                // exact placeholder, and `from_graph` is the exact
                // `Flowchart` Ok arm).
                let mermaid = match &self.mermaid {
                    Some(graph) => Mermaid::from_graph(graph),
                    None => Mermaid::new(GRAPH),
                };
                frame.render_widget(
                    mermaid
                        .style(theme.body())
                        .block(framed(theme, "Mermaid · the rstui event loop")),
                    body,
                );
            }
            3 => frame.render_widget(
                Structurizr::new(WORKSPACE)
                    .style(theme.body())
                    .block(framed(theme, "Structurizr · C4 (auto-layout)")),
                body,
            ),
            4 => frame.render_widget(
                JsonCanvas::new(CANVAS)
                    .style(theme.body())
                    .block(framed(theme, "JSON Canvas · explicit placement")),
                body,
            ),
            _ => self.view_spans(theme, frame, body),
        }

        // Persistent Kbd strip.
        frame.render_widget(
            Kbd::new(["←/→ tabs", "↑/↓", "PgUp/Dn", "click a tab"])
                .style(theme.body())
                .key_style(Style::new().fg(theme.base).bg(theme.accent))
                .separator_style(Style::new().fg(theme.dim)),
            Rect::new(foot.x, foot.y + 1, foot.width, 1),
        );
    }

    /// The styled-text sampler: every Span/Line capability at once.
    fn view_spans(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let lines = vec![
            Line::from(vec![
                Span::raw("Span runs: "),
                "red ".red(),
                "green ".green(),
                "blue ".blue(),
                "on-accent".fg(theme.base).bg(theme.accent),
            ]),
            Line::from(vec![
                Span::raw("Modifiers: "),
                "bold ".bold(),
                "italic ".italic(),
                "underline ".underlined(),
                "reversed ".reversed(),
                "dim".add_modifier(Modifier::DIM),
            ]),
            Line::from(vec![
                Span::raw("24-bit RGB: "),
                Span::styled("■", Style::new().fg(Color::Rgb(255, 90, 95))),
                Span::styled("■", Style::new().fg(Color::Rgb(255, 170, 60))),
                Span::styled("■", Style::new().fg(Color::Rgb(120, 200, 80))),
                Span::styled("■", Style::new().fg(Color::Rgb(80, 170, 255))),
                Span::styled(" gradient-ready", Style::new().fg(theme.accent_alt)),
            ]),
            Line::from("Alignment: this line is centred")
                .style(theme.body())
                .centered(),
            Line::from("…and this one right-aligned")
                .style(theme.caption())
                .right_aligned(),
            Line::from(vec![
                Span::raw("Links live in Markdown/Mermaid — see the "),
                "Markdown".fg(theme.accent).underlined(),
                Span::raw(" tab’s "),
                "[the rstui repo]".fg(theme.accent),
                Span::raw(" anchor."),
            ]),
        ];
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: true })
                .style(theme.body())
                .block(framed(theme, "Span / Line / Text sampler")),
            area,
        );
    }
}

/// A plain rounded framing block.
fn framed(theme: &Theme, title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Line::from(format!(" {title} ")).style(theme.caption()))
        .border_style(theme.border())
        .style(theme.body())
}
