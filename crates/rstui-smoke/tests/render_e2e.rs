//! End-to-end rendering + colour tests: prove that *every* `Color` variant
//! and *every* `Modifier` flag survives the full text→span→line→buffer
//! cascade and lands on the exact `Cell`, and that focus/accent styling
//! actually colours the cells the widgets claim to.
//!
//! `harness.snapshot()` is glyph-only, so a screen looking right in a string
//! says nothing about colour. These read the rendered `Cell` directly
//! (`Buffer::get(pos).fg/bg/modifier`) — the only way to assert "this is red,
//! bold, on blue" — closing the colour-coverage gap the glyph snapshots
//! cannot see.

use rstui_core::{
    Buffer, Cell, Color, Line, Modifier, Position, Rect, Span, Style, Stylize, Text, Widget,
};
use rstui_widgets::{Block, Button, Checkbox, Gauge, Radio};

/// Render any widget into a fresh `width`×`height` buffer and hand it back.
fn paint<W: Widget>(widget: W, width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    widget.render(area, &mut buf);
    buf
}

/// The cell at `(x, y)` (must be in bounds).
fn at(buf: &Buffer, x: u16, y: u16) -> &Cell {
    buf.get(Position::new(x, y)).expect("cell in bounds")
}

/// Whether any cell in the buffer satisfies `pred` — robust for "the fill
/// style is somewhere on this row" without pinning an exact column.
fn any_cell(buf: &Buffer, pred: impl Fn(&Cell) -> bool) -> bool {
    let a = buf.area();
    (a.top()..a.bottom())
        .flat_map(|y| (a.left()..a.right()).map(move |x| (x, y)))
        .any(|(x, y)| pred(at(buf, x, y)))
}

// --- 1. every Color variant lands on the cell -----------------------------

#[test]
fn every_color_variant_reaches_the_cell_fg_and_bg() {
    let colors = [
        Color::Reset,
        Color::Black,
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Magenta,
        Color::Cyan,
        Color::Gray,
        Color::DarkGray,
        Color::LightRed,
        Color::LightGreen,
        Color::LightYellow,
        Color::LightBlue,
        Color::LightMagenta,
        Color::LightCyan,
        Color::White,
        Color::Indexed(0),
        Color::Indexed(207),
        Color::Indexed(255),
        Color::Rgb(0, 0, 0),
        Color::Rgb(10, 20, 30),
        Color::Rgb(255, 128, 64),
    ];
    for fg in colors {
        for bg in [Color::Reset, Color::Indexed(99), Color::Rgb(1, 2, 3)] {
            let buf = paint(Span::styled("Z", Style::new().fg(fg).bg(bg)), 3, 1);
            let cell = at(&buf, 0, 0);
            assert_eq!(cell.symbol, 'Z');
            assert_eq!(cell.fg, fg, "fg {fg:?} on bg {bg:?}");
            assert_eq!(cell.bg, bg, "bg {bg:?} under fg {fg:?}");
        }
    }
}

// --- 2. every Modifier flag lands and is isolated -------------------------

#[test]
fn every_modifier_flag_reaches_the_cell_independently() {
    let flags = [
        Modifier::BOLD,
        Modifier::DIM,
        Modifier::ITALIC,
        Modifier::UNDERLINED,
        Modifier::SLOW_BLINK,
        Modifier::RAPID_BLINK,
        Modifier::REVERSED,
        Modifier::HIDDEN,
        Modifier::CROSSED_OUT,
    ];
    for set in flags {
        let buf = paint(Span::styled("M", Style::new().add_modifier(set)), 2, 1);
        let cell = at(&buf, 0, 0);
        assert!(cell.modifier.contains(set), "{set:?} must be present");
        // A different flag must NOT have leaked in (flags are independent).
        let other = if set == Modifier::BOLD {
            Modifier::ITALIC
        } else {
            Modifier::BOLD
        };
        assert!(
            !cell.modifier.contains(other),
            "{set:?} must not imply {other:?}"
        );
    }
    // Combined flags coexist on one cell.
    let buf = paint(
        Span::styled(
            "X",
            Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ),
        2,
        1,
    );
    let m = at(&buf, 0, 0).modifier;
    assert!(m.contains(Modifier::BOLD) && m.contains(Modifier::UNDERLINED));
}

// --- 3. the text ▸ line ▸ span style cascade ------------------------------

#[test]
fn style_cascades_text_then_line_then_span() {
    // Span sets fg+BOLD; the Line sets bg; the Text sets the inherited fg.
    let line = Line::from(vec![
        Span::styled(
            "AB",
            Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::raw("cd"),
    ])
    .style(Style::new().bg(Color::Blue));
    let text = Text::from(vec![line]).style(Style::new().fg(Color::White));

    let buf = paint(text, 6, 1);

    // 'A'/'B' come from the styled span: its fg wins, the line's bg shows
    // through (span left bg unset), and BOLD is applied.
    let a = at(&buf, 0, 0);
    assert_eq!(a.symbol, 'A');
    assert_eq!(a.fg, Color::Red, "span fg overrides text fg");
    assert_eq!(a.bg, Color::Blue, "line bg cascades onto the span cell");
    assert!(a.modifier.contains(Modifier::BOLD));

    // 'c'/'d' come from the bare span: it sets nothing, so the fg falls back
    // to the Text base and the bg to the Line base.
    let c = at(&buf, 2, 0);
    assert_eq!(c.symbol, 'c');
    assert_eq!(c.fg, Color::White, "bare span inherits the text fg");
    assert_eq!(c.bg, Color::Blue, "bare span inherits the line bg");
    assert!(!c.modifier.contains(Modifier::BOLD), "BOLD did not leak");
}

// --- 4. the Stylize fluent shorthands == the explicit Style ---------------

#[test]
fn stylize_shorthands_equal_the_explicit_style() {
    let fluent = paint("x".red().bold().on_blue(), 1, 1);
    let explicit = paint(
        Span::styled(
            "x",
            Style::new()
                .fg(Color::Red)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ),
        1,
        1,
    );
    assert_eq!(at(&fluent, 0, 0), at(&explicit, 0, 0));
}

// --- 5. Cell::apply_style patch semantics ---------------------------------

#[test]
fn apply_style_is_a_patch_set_overrides_unset_inherits() {
    let mut cell = Cell::new('a');
    // Turn on BOLD|ITALIC, set fg Green, leave bg unset (inherit).
    cell.apply_style(
        Style::new()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD | Modifier::ITALIC),
    );
    assert_eq!(cell.fg, Color::Green);
    assert_eq!(cell.bg, Color::Reset, "unset bg inherits, not forced");
    assert!(cell.modifier.contains(Modifier::ITALIC));

    // Patch again: override fg, add UNDERLINED, remove ITALIC; bg still unset.
    cell.apply_style(
        Style::new()
            .fg(Color::Magenta)
            .add_modifier(Modifier::UNDERLINED)
            .remove_modifier(Modifier::ITALIC),
    );
    assert_eq!(cell.fg, Color::Magenta, "set fg overrides the previous fg");
    assert_eq!(cell.bg, Color::Reset, "still-unset bg keeps inheriting");
    assert!(cell.modifier.contains(Modifier::BOLD), "BOLD survived");
    assert!(
        cell.modifier.contains(Modifier::UNDERLINED),
        "UNDERLINED added"
    );
    assert!(
        !cell.modifier.contains(Modifier::ITALIC),
        "ITALIC removed by sub_modifier"
    );
}

// --- 6. Block border style colours the border glyphs ----------------------

#[test]
fn block_border_style_colours_the_border_cells() {
    let buf = paint(
        Block::bordered().border_style(Style::new().fg(Color::Magenta)),
        6,
        3,
    );
    let corner = at(&buf, 0, 0);
    assert_ne!(corner.symbol, ' ', "the corner is a border glyph");
    assert_eq!(corner.fg, Color::Magenta, "border style colours the glyph");
}

// --- 7. focus / accent fills colour cells only when active ----------------

#[test]
fn focus_fill_colours_cells_only_when_focused() {
    let cyan = |c: &Cell| c.bg == Color::Cyan;

    // Button: focus fill present when focused, absent when not.
    assert!(
        any_cell(
            &paint(
                Button::new("OK")
                    .focused(true)
                    .focus_style(Style::new().bg(Color::Cyan)),
                8,
                1,
            ),
            cyan,
        ),
        "focused Button paints its focus fill"
    );
    assert!(
        !any_cell(
            &paint(
                Button::new("OK")
                    .focused(false)
                    .focus_style(Style::new().bg(Color::Cyan)),
                8,
                1,
            ),
            cyan,
        ),
        "unfocused Button paints no focus fill"
    );

    // Checkbox and Radio share the focus-visual contract.
    assert!(
        any_cell(
            &paint(
                Checkbox::new("agree")
                    .focused(true)
                    .focus_style(Style::new().bg(Color::Cyan)),
                12,
                1,
            ),
            cyan,
        ),
        "focused Checkbox paints its focus fill"
    );
    assert!(
        any_cell(
            &paint(
                Radio::new("one")
                    .selected(true)
                    .focused(true)
                    .focus_style(Style::new().bg(Color::Cyan)),
                12,
                1,
            ),
            cyan,
        ),
        "focused Radio paints its focus fill"
    );
}

// --- 8. Gauge accent colour fills the bar ---------------------------------

#[test]
fn gauge_style_colours_the_progress_bar() {
    let buf = paint(
        Gauge::default()
            .ratio(0.5)
            .gauge_style(Style::new().fg(Color::Yellow).bg(Color::DarkGray)),
        10,
        1,
    );
    assert!(
        any_cell(&buf, |c| c.fg == Color::Yellow),
        "the gauge's accent colour reaches the filled cells"
    );
}
