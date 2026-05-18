//! The hot-path scenarios, each measured through `rstui-core`'s public API
//! only — this crate never reaches into another stream's internals (it is a
//! consumer of the published surface, exactly like a downstream app).
//!
//! Every scenario does its allocation/setup *outside* the timed closure and
//! measures only the operation named in the brief as a hot path:
//!
//! - `buffer/diff/*` — the per-frame change set a backend flushes. Four
//!   shapes because the cost profile differs sharply: idle (no change),
//!   a small update, a full repaint, and the resize-invalidation path.
//! - `buffer/fill` — allocating and filling a frame-sized grid.
//! - `buffer/set_str` — stamping styled text into every row (what widgets do).
//! - `buffer/clear_region` — the opaque-overlay reclaim a modal/popup runs.
//! - `layout/split/nested` — solving a realistic nested app layout.
//!
//! Frame size is a fixed 160×48 (a large-but-ordinary terminal, 7 680 cells)
//! so numbers are comparable run to run.

use rstui_core::{
    Buffer, Cell, Constraint, Event, KeyModifiers, Layout, MouseEvent, MouseEventKind, Position,
    Rect, Selection, Style, TextArea, Widget, selected_text,
};
use rstui_runtime::{App, Cmd, Frame, Harness};
use rstui_widgets::{List, ListItem, Markdown, Paragraph, Row, Table, Tree, TreeItem, Wrap};

use crate::measure::{Bench, Stats};

/// A scenario's measuring function: do its own setup, then time only the hot
/// op via [`Bench::run`], returning the per-iteration [`Stats`]. A named
/// alias because the bare `fn` pointer is, on its own, a `clippy`
/// `type_complexity` violation everywhere it appears.
pub(crate) type Scenario = fn(&Bench) -> Stats;

/// Width of the benchmark frame, in cells.
const FRAME_W: u16 = 160;
/// Height of the benchmark frame, in cells.
const FRAME_H: u16 = 48;

/// A fixed large-but-ordinary terminal frame, at the origin.
fn frame() -> Rect {
    Rect::new(0, 0, FRAME_W, FRAME_H)
}

/// A representative mixed-glyph line. `set_str` clips at the right edge, so a
/// line longer than the frame just fills the row.
const PATTERN: &str = "the quick brown fox jumps over the lazy dog 0123456789 ── ";

/// Fill every row of `buf` with [`PATTERN`] in the default style.
fn paint(buf: &mut Buffer) {
    let style = Style::new();
    for y in 0..FRAME_H {
        buf.set_str(Position::new(0, y), PATTERN, style);
    }
}

/// `buffer/diff/identical` — diff two identical painted frames. Zero changes,
/// but every cell is still compared: the idle steady-state redraw cost.
fn buffer_diff_identical(bench: &Bench) -> Stats {
    let mut previous = Buffer::empty(frame());
    paint(&mut previous);
    let current = previous.clone();
    bench.run(|| current.diff(&previous))
}

/// `buffer/diff/sparse` — one changed row against an otherwise identical
/// frame: a status line or cursor blink update.
fn buffer_diff_sparse(bench: &Bench) -> Stats {
    let mut previous = Buffer::empty(frame());
    paint(&mut previous);
    let mut current = previous.clone();
    current.set_str(
        Position::new(0, FRAME_H / 2),
        "── changed row ──",
        Style::new(),
    );
    bench.run(|| current.diff(&previous))
}

/// `buffer/diff/full` — every cell differs: a full repaint or scroll.
fn buffer_diff_full(bench: &Bench) -> Stats {
    let previous = Buffer::empty(frame());
    let current = Buffer::filled(frame(), Cell::new('#'));
    bench.run(|| current.diff(&previous))
}

/// `buffer/diff/resized` — areas differ, so the whole surface is invalidated
/// and re-emitted (the resize path, distinct from a same-size diff).
fn buffer_diff_resized(bench: &Bench) -> Stats {
    let previous = Buffer::empty(frame());
    let current = Buffer::empty(Rect::new(0, 0, FRAME_W + 1, FRAME_H));
    bench.run(|| current.diff(&previous))
}

/// `buffer/fill` — allocate and fill a fresh frame-sized grid.
fn buffer_fill(bench: &Bench) -> Stats {
    bench.run(|| Buffer::filled(frame(), Cell::new('x')))
}

/// `buffer/set_str` — stamp a styled line into every row of a reused frame
/// buffer: the per-frame text-rendering throughput widgets pay.
fn buffer_set_str(bench: &Bench) -> Stats {
    let mut buf = Buffer::empty(frame());
    let style = Style::new();
    bench.run(|| {
        let mut last = Position::ORIGIN;
        for y in 0..FRAME_H {
            last = buf.set_str(Position::new(0, y), PATTERN, style);
        }
        last
    })
}

/// `buffer/clear_region` — reclaim an inner rectangle, the opaque-overlay
/// primitive a modal/popup runs every frame it is visible. The trailing
/// read keeps the optimizer from eliding the clear.
fn buffer_clear_region(bench: &Bench) -> Stats {
    let mut buf = Buffer::filled(frame(), Cell::new('x'));
    let inner = Rect::new(40, 12, 80, 24);
    bench.run(|| {
        buf.clear_region(inner);
        buf.get(Position::new(40, 12)).cloned()
    })
}

/// `layout/split/nested` — solve a realistic app frame: a header/body/footer
/// vertical split, then a sidebar/content/aside horizontal split of the body.
fn layout_split_nested(bench: &Bench) -> Stats {
    let area = frame();
    let outer = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ]);
    let inner = Layout::horizontal([
        Constraint::Length(24),
        Constraint::Fill(1),
        Constraint::Min(20),
    ]);
    bench.run(|| {
        let rows = outer.split(area);
        inner.split(rows[1])
    })
}

/// `edit/textarea/insert` — one keystroke into a mid-sized document: the
/// per-keypress cost the chat composer / IDE editor pays (BN03). Insert then
/// immediately delete so the document and cursor are stable across
/// iterations, isolating the `byte_at` + cached `char_len` edit path
/// (CM-2/CM-4) rather than measuring an ever-growing buffer.
fn edit_textarea_insert(bench: &Bench) -> Stats {
    let doc: String = (0..200)
        .map(|i| format!("{i:>4}  the quick brown fox jumps over the lazy dog 0123456789\n"))
        .collect();
    let mut ta = TextArea::from_value(doc);
    ta.set_cursor(100, 20);
    bench.run(|| {
        ta.insert_char('x');
        ta.delete_backward()
    })
}

/// `selection/extract` — the copy path: read a large drag-selected region
/// out of the content buffer as text (BN03). A full-frame selection over a
/// painted 160×48 buffer, the worst case `selected_text` walks.
fn selection_extract(bench: &Bench) -> Stats {
    let mut buf = Buffer::empty(frame());
    paint(&mut buf);
    let mut sel = Selection::new();
    sel.start(Position::new(0, 0));
    sel.extend(Position::new(FRAME_W - 1, FRAME_H - 1));
    bench.run(|| selected_text(&buf, &sel))
}

/// A representative multi-cell dataset, built once outside the timed
/// closure (the scenario brief: only the hot op is measured).
fn rows_data(n: usize) -> Vec<String> {
    (0..n)
        .map(|i| format!("{i:>5}  the quick brown fox jumps over the lazy dog 0123456789"))
        .collect()
}

/// `widget/list/render` — project + stamp a long `List` (the kitchen-sink
/// log/menu pane): the immediate-mode per-frame cost of building the
/// `Vec<ListItem>` from borrowed rows and rendering the visible window.
fn widget_list_render(bench: &Bench) -> Stats {
    let data = rows_data(1000);
    let mut buf = Buffer::empty(frame());
    bench.run(|| {
        List::new(data.iter().map(|s| ListItem::new(s.as_str()))).render(frame(), &mut buf);
        buf.get(Position::ORIGIN).map(|c| c.symbol)
    })
}

/// `widget/table/render` — a 200×4 data grid: per-frame `Vec<Row>` build,
/// column solve, and cell stamping (the `Table` hot path T1/T3/T5 cover).
fn widget_table_render(bench: &Bench) -> Stats {
    let data: Vec<[String; 4]> = (0..200)
        .map(|i| {
            [
                format!("row {i}"),
                format!("value {}", i * 7),
                "the quick brown fox".to_owned(),
                format!("{}%", i % 100),
            ]
        })
        .collect();
    let mut buf = Buffer::empty(frame());
    bench.run(|| {
        Table::new(
            data.iter().map(|c| Row::new(c.iter().map(String::as_str))),
            [Constraint::Fill(1); 4],
        )
        .render(frame(), &mut buf);
        buf.get(Position::ORIGIN).map(|c| c.symbol)
    })
}

/// `widget/tree/render` — a 300-node tree (the files/navigation pane): the
/// clean per-widget exemplar, bounded to the visible window.
fn widget_tree_render(bench: &Bench) -> Stats {
    let data = rows_data(300);
    let mut buf = Buffer::empty(frame());
    bench.run(|| {
        Tree::new(
            data.iter()
                .enumerate()
                .map(|(i, s)| TreeItem::new((i % 5) as u16, s.as_str())),
        )
        .render(frame(), &mut buf);
        buf.get(Position::ORIGIN).map(|c| c.symbol)
    })
}

/// `widget/paragraph/render` — soft-wrap + stamp a ~400-line document (the
/// scrollable transcript/description pane); PG-1 caps this to the window.
fn widget_paragraph_render(bench: &Bench) -> Stats {
    let text = rows_data(400).join("\n");
    let mut buf = Buffer::empty(frame());
    bench.run(|| {
        Paragraph::new(text.as_str())
            .wrap(Wrap { trim: false })
            .render(frame(), &mut buf);
        buf.get(Position::ORIGIN).map(|c| c.symbol)
    })
}

/// `widget/markdown/render` — the heaviest widget: a full CommonMark
/// parse + layout on **every** render (MD-1), the dominant cost of any
/// frame with a Markdown pane until a caller-owned cache lands (Tier-2).
/// The shared ~120-section CommonMark document the markdown render/cached
/// scenarios both measure (apples-to-apples: same input, with vs without
/// the caller-owned `MarkdownCache`).
fn markdown_doc() -> String {
    let mut src = String::new();
    for i in 0..120 {
        src.push_str(&format!(
            "# Heading {i}\n\nThe quick **brown** fox _jumps_ over the `lazy` dog. \
             A short paragraph with [a link](https://example.com) and some text.\n\n\
             - bullet one\n- bullet two\n\n```rust\nfn main() {{ let x = {i}; }}\n```\n\n"
        ));
    }
    src
}

fn widget_markdown_render(bench: &Bench) -> Stats {
    let src = markdown_doc();
    let mut buf = Buffer::empty(frame());
    bench.run(|| {
        Markdown::new(src.as_str()).render(frame(), &mut buf);
        buf.get(Position::ORIGIN).map(|c| c.symbol)
    })
}

/// `widget/markdown/cached` — the same document rendered with a
/// caller-owned [`MarkdownCache`](rstui_widgets::MarkdownCache) attached:
/// the whole CommonMark parse + width-aware layout happens on the first
/// frame (a miss) and every later frame is an `O(1)` keyed lookup + a row
/// clone. This is the steady-state cost after R3-2 — it must collapse from
/// the ~1.5 ms `widget/markdown/render` class to the windowed-widget class.
fn widget_markdown_cached(bench: &Bench) -> Stats {
    let src = markdown_doc();
    let cache = rstui_widgets::MarkdownCache::new();
    let mut buf = Buffer::empty(frame());
    // Warm the cache once (the first real frame's miss) so the measured
    // loop is the steady state every later idle frame actually pays.
    Markdown::new(src.as_str())
        .cache(&cache)
        .render(frame(), &mut buf);
    bench.run(|| {
        Markdown::new(src.as_str())
            .cache(&cache)
            .render(frame(), &mut buf);
        buf.get(Position::ORIGIN).map(|c| c.symbol)
    })
}

/// A markdown doc with one embedded Mermaid fence and one embedded
/// Structurizr fence — the kitchen-sink Rich Text §10 shape.
fn diagram_doc() -> String {
    "\
# Embedding diagrams\n\nProse before the diagram, a couple of lines so the \
markdown parser and line layout do real work around the fences.\n\n\
```mermaid\ngraph TD\n  A[fence] --> B[Markdown]\n  B --> C{diagram?}\n  \
C -->|yes| D[render widget]\n  C -->|no| E[code block]\n```\n\n\
Prose between the two diagrams.\n\n\
```structurizr\nworkspace \"demo\" {\n  model {\n    u = person \"Reader\"\n\
    s = softwareSystem \"Markdown\" \"Embeds diagrams inline\"\n    \
u -> s \"Scrolls\"\n  }\n  views {\n    systemContext s \"Ctx\" { include * }\n\
  }\n}\n```\n\nTrailing prose after both diagrams.\n"
        .to_owned()
}

/// `widget/markdown/diagrams_render` — `Markdown::diagrams(true)`
/// **uncached**: every render re-parses + re-lays-out *both* embedded
/// diagrams (Mermaid + Structurizr) and rasterises each through a scratch
/// buffer. This is the per-frame cost an animated screen pays without a
/// caller-owned cache (the 9fps regression); `diagrams_cached` is the fix.
fn widget_markdown_diagrams_render(bench: &Bench) -> Stats {
    let src = diagram_doc();
    let mut buf = Buffer::empty(frame());
    bench.run(|| {
        Markdown::new(src.as_str())
            .diagrams(true)
            .render(frame(), &mut buf);
        buf.get(Position::ORIGIN).map(|c| c.symbol)
    })
}

/// `widget/markdown/diagrams_cached` — the same doc rendered with a
/// caller-owned [`DiagramCache`](rstui_widgets::DiagramCache) attached: the
/// expensive parse+layout+rasterise happens on the first frame (a cache
/// miss) and every later frame is an `O(1)` keyed lookup + a row clone.
/// This is the steady-state cost after the fix — it must be back in the
/// `widget/markdown/render` class, not the `diagrams_render` class.
fn widget_markdown_diagrams_cached(bench: &Bench) -> Stats {
    let src = diagram_doc();
    let cache = rstui_widgets::DiagramCache::new();
    let mut buf = Buffer::empty(frame());
    // Warm the cache once (the first real frame's miss) so the measured
    // loop is the steady state every later idle frame actually pays.
    Markdown::new(src.as_str())
        .diagrams(true)
        .diagram_cache(&cache)
        .render(frame(), &mut buf);
    bench.run(|| {
        Markdown::new(src.as_str())
            .diagrams(true)
            .diagram_cache(&cache)
            .render(frame(), &mut buf);
        buf.get(Position::ORIGIN).map(|c| c.symbol)
    })
}

/// A representative multi-widget app: a selectable `List` beside a wrapped
/// `Paragraph`, split by `Layout` — the shape a real screen's `view`
/// projects every frame. State is caller-owned (immediate-mode); the
/// widgets are rebuilt from borrowed data each `view`.
struct FrameApp {
    rows: Vec<String>,
    para: String,
    sel: usize,
}

/// `Bump` moves the selection one row — the "one widget changed" frame.
enum FrameMsg {
    Bump,
}

impl App for FrameApp {
    type Message = FrameMsg;

    fn update(&mut self, message: FrameMsg) -> Cmd<FrameMsg> {
        match message {
            FrameMsg::Bump => {
                self.sel = (self.sel + 1) % self.rows.len().max(1);
            }
        }
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let cols = Layout::horizontal([Constraint::Percentage(40), Constraint::Fill(1)])
            .split(frame.area());
        frame.render_widget(
            List::new(self.rows.iter().map(|s| ListItem::new(s.as_str()))).selected(Some(self.sel)),
            cols[0],
        );
        frame.render_widget(
            Paragraph::new(self.para.as_str()).wrap(Wrap { trim: false }),
            cols[1],
        );
    }
}

fn frame_app() -> FrameApp {
    FrameApp {
        rows: rows_data(500),
        para: rows_data(120).join("\n"),
        sel: 0,
    }
}

/// `runtime/frame/idle` — the steady-state idle re-render through the public
/// `Harness`: `view` re-projects the whole screen, `Buffer::diff` finds
/// (near) zero changes, the backend flushes nothing. The dominant cost of an
/// animated/idle app, and what `Buffer::diff`/`Terminal::reset` ultimately
/// pay into (BN01). `Harness::tick` re-renders even with no state change.
fn runtime_frame_idle(bench: &Bench) -> Stats {
    let mut h = Harness::new(frame_app(), FRAME_W, FRAME_H);
    bench.run(|| {
        h.tick();
        h.app().sel
    })
}

/// `runtime/frame/changed` — one widget's state changed: `update` folds the
/// message, `view` re-projects, `Buffer::diff` emits the small delta, the
/// backend flushes it. The realistic interactive frame cost (BN01).
fn runtime_frame_changed(bench: &Bench) -> Stats {
    let mut h = Harness::new(frame_app(), FRAME_W, FRAME_H);
    bench.run(|| {
        h.message(FrameMsg::Bump);
        h.app().sel
    })
}

/// `runtime/input/mouse_move` — one pointer-motion frame on a representative
/// two-pane app: `on_event` (no modelled message) then the full
/// `view`+`diff`+`flush` a naive loop pays *per mouse-move sample*. This is
/// the "pause from moving the mouse over the screen" signal (ADR 0018): the
/// RT-01 coalesce/skip is what stops a real run paying this every sample, so
/// a regression here is exactly the freeze-while-moving class. The cursor
/// advances each iteration so it is real motion, not a repeated no-op cell.
fn runtime_input_mouse_move(bench: &Bench) -> Stats {
    let mut h = Harness::new(frame_app(), FRAME_W, FRAME_H);
    let mut x: u16 = 0;
    bench.run(|| {
        x = (x + 1) % FRAME_W;
        h.handle(Event::Mouse(MouseEvent::new(
            MouseEventKind::Moved,
            Position::new(x, 1),
            KeyModifiers::NONE,
        )));
        h.app().sel
    })
}

/// The scenario registry: stable `name` → measuring function. `main` filters
/// and iterates this; the names are the substring-filter and `--list`
/// vocabulary, so keep them stable and `/`-segmented.
pub(crate) const SCENARIOS: &[(&str, Scenario)] = &[
    ("buffer/diff/identical", buffer_diff_identical),
    ("buffer/diff/sparse", buffer_diff_sparse),
    ("buffer/diff/full", buffer_diff_full),
    ("buffer/diff/resized", buffer_diff_resized),
    ("buffer/fill", buffer_fill),
    ("buffer/set_str", buffer_set_str),
    ("buffer/clear_region", buffer_clear_region),
    ("layout/split/nested", layout_split_nested),
    ("edit/textarea/insert", edit_textarea_insert),
    ("selection/extract", selection_extract),
    ("widget/list/render", widget_list_render),
    ("widget/table/render", widget_table_render),
    ("widget/tree/render", widget_tree_render),
    ("widget/paragraph/render", widget_paragraph_render),
    ("widget/markdown/render", widget_markdown_render),
    ("widget/markdown/cached", widget_markdown_cached),
    (
        "widget/markdown/diagrams_render",
        widget_markdown_diagrams_render,
    ),
    (
        "widget/markdown/diagrams_cached",
        widget_markdown_diagrams_cached,
    ),
    ("runtime/frame/idle", runtime_frame_idle),
    ("runtime/frame/changed", runtime_frame_changed),
    ("runtime/input/mouse_move", runtime_input_mouse_move),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1-iteration run of every registered scenario: proves each one
    /// compiles against the public API, runs, and produces a sane summary —
    /// `cargo test` therefore guards the benchmarks from bit-rotting even
    /// though they are not a CI gate.
    #[test]
    fn every_scenario_runs_and_summarizes() {
        let bench = Bench {
            warmup: 0,
            iters: 1,
        };
        for (name, scenario) in SCENARIOS {
            let stats = scenario(&bench);
            assert_eq!(stats.samples, 1, "{name} must record its iteration");
            assert!(
                stats.min_ns <= stats.median_ns,
                "{name} produced an inconsistent summary"
            );
        }
    }

    /// Scenario names are the user-facing filter vocabulary: unique, and
    /// `/`-segmented so a prefix like `buffer/diff` selects a family.
    #[test]
    fn scenario_names_are_unique_and_segmented() {
        let mut seen = Vec::new();
        for (name, _) in SCENARIOS {
            assert!(name.contains('/'), "scenario `{name}` must be /-segmented");
            assert!(!seen.contains(name), "duplicate scenario name `{name}`");
            seen.push(name);
        }
    }
}
