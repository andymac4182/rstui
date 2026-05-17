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
    Buffer, Cell, Constraint, Layout, Position, Rect, Selection, Style, TextArea, selected_text,
};

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
