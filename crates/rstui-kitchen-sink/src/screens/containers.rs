//! The Containers screen: all four [`Block`] border types, a [`Card`], a
//! [`Grid`], a [`SplitPane`], horizontal + vertical [`Divider`]s, a centred
//! [`Align`] box, and a scrollable [`ScrollView`] with both its built-in and
//! a standalone [`Scrollbar`]. `↑/↓` (and the wheel) scroll the view.

use rstui_core::{Alignment, Buffer, Constraint, KeyCode, Layout, Line, Position, Rect, Style};
use rstui_runtime::Frame;
use rstui_widgets::{
    Align, Block, BorderType, Card, Divider, DividerOrientation, Grid, Paragraph, ScrollView,
    Scrollbar, ScrollbarOrientation, SplitPane, VerticalAlignment, Wrap,
};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// Logical rows in the scrollable document.
const DOC_LEN: u16 = 40;

/// The scroll offset into the [`ScrollView`] document.
#[derive(Debug)]
pub(crate) struct State {
    scroll: u16,
}

impl State {
    /// Scrolled to the top.
    pub(crate) fn new() -> Self {
        Self { scroll: 0 }
    }

    /// `↑/↓` and `PgUp/PgDn` scroll; `←` falls back to the rail.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        match code {
            KeyCode::Up => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Down => self.scroll = (self.scroll + 1).min(DOC_LEN),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(5),
            KeyCode::PageDown => self.scroll = (self.scroll + 5).min(DOC_LEN),
            KeyCode::Left => return ScreenOutcome::ignored(),
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// Wheel scroll moves the document.
    pub(crate) fn on_scroll(&mut self, up: bool) {
        if up {
            self.scroll = self.scroll.saturating_sub(2);
        } else {
            self.scroll = (self.scroll + 2).min(DOC_LEN);
        }
    }

    /// Draw the containers gallery.
    pub(crate) fn view(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let [borders, mid, bottom] = Layout::vertical([
            Constraint::Length(4),
            Constraint::Length(8),
            Constraint::Fill(1),
        ])
        .areas(area);

        // Row 1 — the four border types.
        let kinds = [
            (BorderType::Plain, "Plain"),
            (BorderType::Rounded, "Rounded"),
            (BorderType::Double, "Double"),
            (BorderType::Thick, "Thick"),
        ];
        let cols = Layout::horizontal([Constraint::Fill(1); 4]).split(borders);
        for (cell, (kind, name)) in cols.iter().zip(kinds) {
            frame.render_widget(
                Block::bordered()
                    .border_type(kind)
                    .title(Line::from(format!(" {name} ")).style(theme.caption()))
                    .border_style(theme.border())
                    .style(theme.body()),
                *cell,
            );
        }

        // Row 2 — Card | Grid | Align.
        let [card_a, grid_a, align_a] = Layout::horizontal([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Fill(1),
        ])
        .areas(mid);

        let card = Card::new()
            .title(Line::from(" Card ").style(theme.heading()))
            .footer(Line::from(" header · body · footer ").style(theme.caption()))
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(theme.border()),
            )
            .style(theme.body());
        let card_in = card.inner(card_a);
        frame.render_widget(card, card_a);
        frame.render_widget(
            Paragraph::new("A framed surface with optional header and footer rows.")
                .style(theme.body())
                .wrap(Wrap { trim: true }),
            card_in,
        );

        let grid = Grid::new(
            [Constraint::Fill(1), Constraint::Fill(1)],
            [Constraint::Fill(1), Constraint::Fill(1)],
        )
        .row_spacing(1)
        .column_spacing(1)
        .block(framed(theme, "Grid 2×2"))
        .style(theme.body());
        let cells = grid.split(grid_a);
        frame.render_widget(grid, grid_a);
        for (r, row) in cells.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                frame.render_widget(
                    Block::bordered()
                        .border_type(BorderType::Plain)
                        .title(Line::from(format!("{r},{c}")).style(theme.caption()))
                        .border_style(theme.border())
                        .style(Style::new().bg(theme.raised)),
                    *cell,
                );
            }
        }

        let aligned = Align::new()
            .horizontal(Alignment::Center)
            .vertical(VerticalAlignment::Center)
            .width(Constraint::Length(14))
            .height(Constraint::Length(3))
            .block(framed(theme, "Align"))
            .style(theme.body());
        let inner = aligned.inner(align_a);
        frame.render_widget(aligned, align_a);
        frame.render_widget(
            Paragraph::new(Line::from("centred").style(theme.accent_text()).centered())
                .style(theme.body()),
            inner,
        );

        // Row 3 — SplitPane: ScrollView (left) | dividers + Scrollbar (right).
        let split = SplitPane::new(Constraint::Percentage(58))
            .divider('│')
            .divider_style(theme.border())
            .style(theme.body());
        let (left, right) = split.split(bottom);
        frame.render_widget(split, bottom);

        // Build the scrollable document buffer.
        let view_block = framed(theme, "ScrollView · ↑↓ / wheel");
        let view_in = view_block.inner(left);
        frame.render_widget(view_block, left);
        let doc_w = view_in.width.max(1);
        let mut doc = Buffer::empty(Rect::new(0, 0, doc_w, DOC_LEN));
        for y in 0..DOC_LEN {
            let s = format!(
                "{:>3}  line {y} — scroll me with the arrows or the wheel",
                y + 1
            );
            doc.set_str(Position::new(0, y), &s, theme.body());
        }
        let vh = view_in.height;
        let max_off = DOC_LEN.saturating_sub(vh);
        let off = self.scroll.min(max_off);
        frame.render_widget(
            ScrollView::new(&doc)
                .offset(0, off)
                .vertical_scrollbar(true)
                .style(theme.body())
                .thumb_style(Style::new().fg(theme.accent)),
            view_in,
        );

        let [divs, bar] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(2)]).areas(right);
        let [hdiv, vdiv] =
            Layout::vertical([Constraint::Length(3), Constraint::Fill(1)]).areas(divs);
        frame.render_widget(
            Divider::new()
                .label(Line::from(" horizontal divider ").style(theme.caption()))
                .style(theme.border()),
            Rect::new(hdiv.x, hdiv.y + 1, hdiv.width, 1),
        );
        frame.render_widget(
            Divider::new()
                .orientation(DividerOrientation::Vertical)
                .style(theme.border()),
            Rect::new(vdiv.x + vdiv.width / 2, vdiv.y, 1, vdiv.height),
        );
        frame.render_widget(
            Paragraph::new("standalone\nScrollbar →")
                .style(theme.caption())
                .wrap(Wrap { trim: true }),
            Rect::new(vdiv.x + 2, vdiv.y + 1, vdiv.width.saturating_sub(2), 2),
        );
        frame.render_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .content_length(DOC_LEN as usize)
                .viewport_length(vh as usize)
                .position(off as usize)
                .style(theme.border())
                .thumb_style(Style::new().fg(theme.accent)),
            bar,
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

/// A drag-select stays inside the Card body or the ScrollView document —
/// never across the borders/grid. Mirrors [`State::view`]'s layout.
pub(crate) fn selection_region(pos: Position, content: Rect) -> Option<Rect> {
    let [_, mid, bottom] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(8),
        Constraint::Fill(1),
    ])
    .areas(content);
    let [card_a, _grid, _align] = Layout::horizontal([
        Constraint::Percentage(34),
        Constraint::Percentage(33),
        Constraint::Fill(1),
    ])
    .areas(mid);
    if card_a.contains(pos) {
        return Some(crate::screens::block_inner(card_a));
    }
    let (left, _right) = SplitPane::new(Constraint::Percentage(58)).split(bottom);
    left.contains(pos)
        .then(|| crate::screens::block_inner(left))
}
