//! [`DateNavigator`] — the calendar-app toolbar: a one-row strip with a
//! `‹ prev` / `next ›` pair, a centred caller-supplied period label, a
//! `Today` and `＋ New` button, and a segmented Day/Week/Month/Year/Agenda
//! view-mode switch.
//!
//! # A pure projection, like [`Tabs`](crate::Tabs) and [`StatusBar`](crate::StatusBar)
//!
//! `DateNavigator` owns no state. It does **no date math** — the centred
//! period text is a *caller-formatted* `label` (the
//! reducer, or a date crate of the caller's choosing, formats `"May 2026"` /
//! `"Week 21"`; never `chrono`/`time` here), and the highlighted view is a
//! caller-owned [`mode`](DateNavigator::mode) index the widget only reads —
//! exactly the [`Tabs`](crate::Tabs)/[`List`](crate::List) projection, just
//! with several segments on one row. A click is mapped to a
//! [`NavTarget`] by [`target_at`](DateNavigator::target_at) and
//! dispatched to a reducer action; the widget never mutates anything.
//!
//! # A leaf control: one row, optional `Block`
//!
//! Like [`StatusBar`](crate::StatusBar)/[`Tabs`](crate::Tabs) it is one row;
//! the surrounding [`Layout`](rstui_core::Layout) owns which edge it sits on. A
//! framing [`block`](DateNavigator::block) is optional (the strip then draws on
//! [`block.inner`](crate::Block::inner)'s first row).
//!
//! # One layout, two readers — derived geometry is a projection
//!
//! The segments are laid out **once** (left controls → centred label → right
//! mode switch + next), and both [`render`](rstui_core::Widget::render) and
//! [`target_at`](DateNavigator::target_at) walk that same layout, so a click
//! can never land on a different control than the one drawn there — the same
//! "the hit-test is the inverse of the render walk" discipline
//! [`Tabs::tab_at`](crate::Tabs::tab_at) and [`List::row_at`](crate::List::row_at)
//! follow.
//!
//! # Total, never a panic
//!
//! Per the [`Gauge`](crate::Gauge) rule (a pure projection must be *total*): a
//! tiny area elides the buttons/label/segments from the right inward and is a
//! safe clip, an empty area is a no-op, an out-of-range
//! [`mode`](DateNavigator::mode) accents nothing, and every column is
//! saturating — never a panic. [`target_at`](DateNavigator::target_at) returns
//! `None` for any cell that is not a drawn, interactive segment.

use std::borrow::Cow;

use rstui_core::{Buffer, Position, Rect, Style, Widget};

use crate::block::Block;

/// The previous-period glyph (drawn `␣‹␣`, the pad belonging to the control).
const PREV: char = '‹';
/// The next-period glyph (drawn `␣›␣`).
const NEXT: char = '›';
/// The divider drawn between adjacent view-mode segments.
const DIVIDER: char = '│';
/// The default view-mode segment labels.
const DEFAULT_MODES: &[&str] = &["Day", "Week", "Month", "Year", "Agenda"];

/// Which control of a [`DateNavigator`] a click landed on — the calendar-app
/// reducer maps this to its navigation action (the
/// [`Tabs`](crate::Tabs)/[`List`](crate::List) "clicking what you see selects
/// it" discipline).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavTarget {
    /// The `‹` previous-period control.
    Prev,
    /// The `›` next-period control.
    Next,
    /// The `Today` button.
    Today,
    /// The `＋ New` button.
    New,
    /// The view-mode segment at this index (into
    /// [`modes`](DateNavigator::modes)).
    Mode(usize),
}

/// One laid-out cell span `[start, end)` (columns within the strip) and the
/// control it belongs to (`None` for the label, dividers, and slack).
#[derive(Debug, Clone, Copy)]
struct Seg {
    start: u16,
    end: u16,
    target: Option<NavTarget>,
}

/// A one-row calendar toolbar rendered as a pure projection of a caller-owned
/// [`mode`](Self::mode) index and caller-formatted `label`.
///
/// Laid out left → centre → right: `‹` prev, the `Today` and `＋ New` buttons,
/// the centred period label, the segmented mode switch, and `›` next. The
/// selected mode segment is accented with [`selected_style`](Self::selected_style);
/// a narrow strip elides the right-hand segments/label gracefully (never a
/// panic).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_widgets::{DateNavigator, NavTarget};
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 60, 1));
/// let nav = DateNavigator::new("May 2026").mode(2); // "Month" selected
/// // `mode` is plain caller-owned state the widget only reads.
/// nav.clone().render(buf.area(), &mut buf);
///
/// // The prev control is at the left edge…
/// assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, '‹');
/// // …and a click there maps back to the Prev action.
/// assert_eq!(nav.target_at(buf.area(), Position::new(1, 0)), Some(NavTarget::Prev));
/// ```
#[derive(Debug, Clone)]
pub struct DateNavigator<'a> {
    label: Cow<'a, str>,
    mode: usize,
    modes: &'a [&'a str],
    show_today: bool,
    show_new: bool,
    block: Option<Block<'a>>,
    style: Style,
    label_style: Style,
    button_style: Style,
    selected_style: Style,
}

impl<'a> DateNavigator<'a> {
    /// A toolbar showing the caller-formatted period `label` (e.g.
    /// `"May 2026"`). Mode `0` selected, the default
    /// `Day/Week/Month/Year/Agenda` segments, both buttons shown.
    pub fn new(label: impl Into<Cow<'a, str>>) -> Self {
        Self {
            label: label.into(),
            mode: 0,
            modes: DEFAULT_MODES,
            show_today: true,
            show_new: true,
            block: None,
            style: Style::new(),
            label_style: Style::new(),
            button_style: Style::new(),
            selected_style: Style::new(),
        }
    }

    /// Sets which view-mode segment is selected (accented). Out of range
    /// simply accents nothing — caller-owned state the widget only reads.
    #[must_use]
    pub fn mode(mut self, mode: usize) -> Self {
        self.mode = mode;
        self
    }

    /// Replaces the view-mode segment labels (default
    /// `["Day","Week","Month","Year","Agenda"]`). An empty slice draws no
    /// switch (total).
    #[must_use]
    pub fn modes(mut self, modes: &'a [&'a str]) -> Self {
        self.modes = modes;
        self
    }

    /// Sets whether the `Today` button is drawn (default `true`).
    #[must_use]
    pub fn show_today(mut self, show_today: bool) -> Self {
        self.show_today = show_today;
        self
    }

    /// Sets whether the `＋ New` button is drawn (default `true`).
    #[must_use]
    pub fn show_new(mut self, show_new: bool) -> Self {
        self.show_new = show_new;
        self
    }

    /// Frames the strip in `block`; the toolbar draws on
    /// [`block.inner`](crate::Block::inner)'s first row.
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base [`Style`]; it also fills the strip row so a background
    /// reads as one bar (the [`StatusBar`](crate::StatusBar) idiom).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the [`Style`] patched over the centred period label.
    #[must_use]
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }

    /// Sets the [`Style`] patched over the `‹`/`›`/`Today`/`＋ New` controls
    /// and the unselected mode segments.
    #[must_use]
    pub fn button_style(mut self, style: Style) -> Self {
        self.button_style = style;
        self
    }

    /// Sets the [`Style`] patched **last** over the selected mode segment, so
    /// it reads as accented (the [`Tabs`](crate::Tabs) highlight-wins-last
    /// idiom).
    #[must_use]
    pub fn selected_style(mut self, style: Style) -> Self {
        self.selected_style = style;
        self
    }

    /// The inner first-row strip rect (inside the framing
    /// [`block`](Self::block) when set), or an empty rect when there is no
    /// room.
    fn strip(&self, area: Rect) -> Rect {
        let inner = match &self.block {
            Some(b) => b.inner(area),
            None => area,
        };
        if inner.is_empty() {
            Rect::ZERO
        } else {
            Rect::new(inner.left(), inner.top(), inner.width, 1)
        }
    }

    /// The single source of geometry both [`render`](Widget::render) and
    /// [`target_at`](Self::target_at) walk: the ordered, non-overlapping
    /// `[start,end)` column spans (within the buffer) of every segment,
    /// elided from the right inward when the strip is too narrow.
    ///
    /// Order: `‹` prev, `Today`, `＋ New`, the centred label, then the mode
    /// switch and `›` next anchored to the right edge. A segment is dropped
    /// (not clipped) when it would not fit, so the layout never half-draws a
    /// label and the hit-test stays exact.
    fn segments(&self, strip: Rect) -> Vec<Seg> {
        let mut segs: Vec<Seg> = Vec::new();
        if strip.is_empty() {
            return segs;
        }
        let left = strip.left();
        let right = strip.right();

        // --- Left controls, packed from the left edge ---
        let mut x = left;
        // Pushes a `␣text␣`-padded control of `target` if it fits before the
        // running right boundary; advances `x`. Returns whether it fit.
        let push_left = |segs: &mut Vec<Seg>, x: &mut u16, label_w: u16, t: NavTarget| {
            let w = label_w.saturating_add(2); // one pad each side
            if x.saturating_add(w) > right {
                return false;
            }
            segs.push(Seg {
                start: *x,
                end: *x + w,
                target: Some(t),
            });
            *x += w;
            true
        };
        // Prev `‹` is one glyph (`␣‹␣`).
        push_left(&mut segs, &mut x, 1, NavTarget::Prev);
        if self.show_today {
            push_left(&mut segs, &mut x, "Today".len() as u16, NavTarget::Today);
        }
        if self.show_new {
            // "＋ New" is treated one-cell-per-char like the rest of the
            // house widgets (Tabs/StatusBar) — 5 columns.
            push_left(
                &mut segs,
                &mut x,
                "＋ New".chars().count() as u16,
                NavTarget::New,
            );
        }
        let left_end = x;

        // --- Right block: the mode switch then `›`, packed leftward from
        // the right edge so it stays pinned there. ---
        let mut rx = right;
        // The next `›` (`␣›␣`) takes the last 3 columns when it fits.
        let next_seg = if rx.saturating_sub(left_end) >= 3 {
            rx -= 3;
            Some(Seg {
                start: rx,
                end: rx + 3,
                target: Some(NavTarget::Next),
            })
        } else {
            None
        };

        // The mode switch: `␣Day␣│␣Week␣…` — measure its full width, then
        // place it just left of `›` only if it fits in the remaining gap.
        let mut mode_segs: Vec<Seg> = Vec::new();
        if !self.modes.is_empty() {
            let mut total: u32 = 0;
            for (i, m) in self.modes.iter().enumerate() {
                if i > 0 {
                    total += 1; // the divider
                }
                total += m.chars().count() as u32 + 2; // `␣label␣`
            }
            if total <= u32::from(rx.saturating_sub(left_end)) {
                let mut mx = rx - total as u16;
                for (i, m) in self.modes.iter().enumerate() {
                    if i > 0 {
                        mx += 1; // skip the divider column (no target)
                    }
                    let w = m.chars().count() as u16 + 2;
                    mode_segs.push(Seg {
                        start: mx,
                        end: mx + w,
                        target: Some(NavTarget::Mode(i)),
                    });
                    mx += w;
                }
                rx -= total as u16;
            }
        }

        // --- The centred label in the gap between the left controls and the
        // right block. It is *dropped, not clipped* when the whole label
        // (plus a one-cell breathing pad each side, so it never visually
        // abuts a control) does not fit — the same elide-don't-half-draw
        // discipline the buttons/switch use, so the hit-test stays exact and
        // a fragment never jams against a segment. ---
        let label_w = self.label.chars().count() as u16;
        if label_w > 0 && rx > left_end {
            let gap = rx - left_end;
            if label_w.saturating_add(2) <= gap {
                // Centre the label in the *full* strip width, then clamp it
                // into the gap (StatusBar's centre rule).
                let ideal = strip
                    .width
                    .saturating_sub(label_w)
                    .saturating_div(2)
                    .saturating_add(left);
                let start = ideal.clamp(left_end + 1, rx - 1 - label_w);
                segs.push(Seg {
                    start,
                    end: start + label_w,
                    target: None,
                });
            }
        }

        segs.extend(mode_segs);
        if let Some(seg) = next_seg {
            segs.push(seg);
        }
        segs
    }

    /// The control at cell `pos` for `area`, or `None` for any cell that is
    /// not a drawn, interactive segment (the label, dividers, slack, the
    /// border, off-strip rows).
    ///
    /// The pure inverse of the render walk — both share
    /// `segments`, so a click resolves to exactly the
    /// control drawn under it (no even-split guess). An app maps the returned
    /// [`NavTarget`] to a reducer action.
    #[must_use]
    pub fn target_at(&self, area: Rect, pos: Position) -> Option<NavTarget> {
        let strip = self.strip(area);
        if strip.is_empty() || pos.y != strip.top() {
            return None;
        }
        if pos.x < strip.left() || pos.x >= strip.right() {
            return None;
        }
        self.segments(strip).into_iter().find_map(|s| {
            (s.target.is_some() && pos.x >= s.start && pos.x < s.end).then_some(s.target)?
        })
    }
}

impl Widget for DateNavigator<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        // The block (if any) frames the strip and reserves the inner area.
        if let Some(b) = &self.block {
            b.clone().render(area, buf);
        }
        let strip = self.strip(area);
        if strip.is_empty() {
            return;
        }

        let y = strip.top();
        // Base fills the strip row so a background reads as one bar; segment
        // glyphs layer the strip → control cascade on top.
        buf.set_style(strip, self.style);

        let button_base = self.style.patch(self.button_style);
        let label_base = self.style.patch(self.label_style);
        let selected_base = button_base.patch(self.selected_style);

        // Stamps `text`'s chars over the closed span `[start,end)` in
        // `cell_style`, padding `text` to that span (a `␣text␣` control), so
        // the highlight/base covers the whole hit area, not just the glyphs.
        let stamp_padded =
            |buf: &mut Buffer, start: u16, end: u16, text: &str, cell_style: Style| {
                let span_w = end.saturating_sub(start) as usize;
                let text_w = text.chars().count();
                // One leading pad, then the text, then the trailing pad(s).
                let lead = if span_w > text_w { 1 } else { 0 };
                let mut col = start;
                for _ in 0..lead {
                    if col >= end {
                        return;
                    }
                    buf.set_cell(Position::new(col, y), ' ', cell_style);
                    col = col.saturating_add(1);
                }
                for ch in text.chars() {
                    if col >= end {
                        return;
                    }
                    buf.set_cell(Position::new(col, y), ch, cell_style);
                    col = col.saturating_add(1);
                }
                while col < end {
                    buf.set_cell(Position::new(col, y), ' ', cell_style);
                    col = col.saturating_add(1);
                }
            };

        for seg in self.segments(strip) {
            match seg.target {
                Some(NavTarget::Prev) => {
                    stamp_padded(buf, seg.start, seg.end, &PREV.to_string(), button_base);
                }
                Some(NavTarget::Next) => {
                    stamp_padded(buf, seg.start, seg.end, &NEXT.to_string(), button_base);
                }
                Some(NavTarget::Today) => {
                    stamp_padded(buf, seg.start, seg.end, "Today", button_base);
                }
                Some(NavTarget::New) => {
                    stamp_padded(buf, seg.start, seg.end, "＋ New", button_base);
                }
                Some(NavTarget::Mode(i)) => {
                    let style = if i == self.mode {
                        selected_base
                    } else {
                        button_base
                    };
                    let label = self.modes.get(i).copied().unwrap_or("");
                    stamp_padded(buf, seg.start, seg.end, label, style);
                    // The divider before this segment (none before the
                    // first) keeps the base style, like Tabs.
                    if i > 0 && seg.start > strip.left() {
                        buf.set_cell(Position::new(seg.start - 1, y), DIVIDER, button_base);
                    }
                }
                None => {
                    // The centred label: glyphs only, base/label style.
                    let mut col = seg.start;
                    for ch in self.label.chars() {
                        if col >= seg.end {
                            break;
                        }
                        buf.set_cell(Position::new(col, y), ch, label_base);
                        col = col.saturating_add(1);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Color, Modifier, Style};

    /// Renders `widget` into a fresh `width`×`height` buffer and returns the
    /// glyphs as one newline-terminated line per row.
    fn lines<W: Widget>(widget: W, width: u16, height: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        widget.render(buf.area(), &mut buf);
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    /// The glyphs of buffer row `y` as a `String`.
    fn row(buf: &Buffer, y: u16) -> String {
        let w = buf.area().width;
        (0..w)
            .map(|x| buf.get(Position::new(x, y)).unwrap().symbol)
            .collect()
    }

    #[test]
    fn renders_prev_buttons_centred_label_modes_and_next_left_to_right() {
        // Width 70: wide enough for the controls, the centred label, and the
        // full mode switch all at once.
        let mut buf = Buffer::empty(Rect::new(0, 0, 70, 1));
        DateNavigator::new("May 2026")
            .mode(2)
            .render(buf.area(), &mut buf);
        let r = row(&buf, 0);
        // Prev at the left edge (` ‹ `), then ` Today `, then ` ＋ New `.
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, '‹');
        assert!(r.contains("Today"));
        assert!(r.contains("＋ New"));
        // The centred caller-formatted label appears.
        assert!(r.contains("May 2026"));
        // The mode switch (right-anchored) shows every segment + divider.
        assert!(r.contains("Day"));
        assert!(r.contains("Month"));
        assert!(r.contains("Agenda"));
        assert!(r.contains('│'));
        // Next `›` is the last interactive glyph, in the last 3 cols (67..70).
        assert_eq!(buf.get(Position::new(68, 0)).unwrap().symbol, '›');
    }

    #[test]
    fn target_at_inverts_the_render_layout_exactly() {
        // Width 70: controls + centred label + full switch all present, so
        // every segment kind is exercised.
        let nav = DateNavigator::new("May 2026").mode(0);
        let area = Rect::new(0, 0, 70, 1);
        let mut buf = Buffer::empty(area);
        nav.clone().render(area, &mut buf);

        // The left controls are at fixed columns regardless of width.
        // Prev is `␣‹␣` at cols 0..3.
        assert_eq!(
            nav.target_at(area, Position::new(0, 0)),
            Some(NavTarget::Prev)
        );
        assert_eq!(
            nav.target_at(area, Position::new(2, 0)),
            Some(NavTarget::Prev)
        );
        // The ` Today ` block (7 cols, 3..10).
        assert_eq!(
            nav.target_at(area, Position::new(3, 0)),
            Some(NavTarget::Today)
        );
        assert_eq!(
            nav.target_at(area, Position::new(9, 0)),
            Some(NavTarget::Today)
        );
        // The ` ＋ New ` block (7 cols, 10..17).
        assert_eq!(
            nav.target_at(area, Position::new(10, 0)),
            Some(NavTarget::New)
        );
        assert_eq!(
            nav.target_at(area, Position::new(16, 0)),
            Some(NavTarget::New)
        );
        // Next `›` is `␣›␣` in the last 3 columns (67..70).
        assert_eq!(
            nav.target_at(area, Position::new(68, 0)),
            Some(NavTarget::Next)
        );

        // The centred label is non-interactive: its first glyph 'M' is a
        // drawn cell, but the hit-test there is `None` (not a control).
        let m_x = (0..70)
            .find(|&x| buf.get(Position::new(x, 0)).unwrap().symbol == 'M')
            .expect("the label is drawn");
        assert!(m_x > 16, "the label sits after the left controls");
        assert_eq!(nav.target_at(area, Position::new(m_x, 0)), None);
        // The blank slack just past the New button is also `None`.
        assert_eq!(nav.target_at(area, Position::new(17, 0)), None);
        // Off the strip row ⇒ None.
        assert_eq!(
            nav.target_at(Rect::new(0, 0, 70, 2), Position::new(1, 1)),
            None
        );
    }

    #[test]
    fn target_at_resolves_each_mode_segment() {
        let nav = DateNavigator::new("X");
        let area = Rect::new(0, 0, 60, 1);
        // Walk the mode switch by probing inside each ` label ` span.
        let segs = nav.segments(nav.strip(area));
        let mode_spans: Vec<_> = segs
            .iter()
            .filter_map(|s| match s.target {
                Some(NavTarget::Mode(i)) => Some((i, s.start, s.end)),
                _ => None,
            })
            .collect();
        assert_eq!(mode_spans.len(), 5);
        for (i, start, end) in mode_spans {
            let mid = start + (end - start) / 2;
            assert_eq!(
                nav.target_at(area, Position::new(mid, 0)),
                Some(NavTarget::Mode(i))
            );
        }
    }

    #[test]
    fn the_selected_mode_segment_is_accented_and_others_are_not() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 1));
        DateNavigator::new("X")
            .mode(1) // "Week"
            .selected_style(Style::new().bg(Color::Cyan))
            .render(buf.area(), &mut buf);
        // Find the "Week" segment span and assert its cells carry the accent.
        let nav = DateNavigator::new("X").mode(1);
        let segs = nav.segments(nav.strip(buf.area()));
        let week = segs
            .iter()
            .find(|s| s.target == Some(NavTarget::Mode(1)))
            .unwrap();
        for x in week.start..week.end {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Cyan);
        }
        // The "Day" segment (mode 0) is NOT accented.
        let day = segs
            .iter()
            .find(|s| s.target == Some(NavTarget::Mode(0)))
            .unwrap();
        for x in day.start..day.end {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Reset);
        }
    }

    #[test]
    fn an_out_of_range_mode_accents_nothing() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 1));
        DateNavigator::new("X")
            .mode(99)
            .selected_style(Style::new().bg(Color::Cyan))
            .render(buf.area(), &mut buf);
        for x in 0..60 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Reset);
        }
    }

    #[test]
    fn show_today_and_show_new_toggle_the_buttons() {
        let r = lines(
            DateNavigator::new("X").show_today(false).show_new(false),
            60,
            1,
        );
        assert!(!r.contains("Today"));
        assert!(!r.contains("＋ New"));
        // The prev/next controls and the mode switch are still there.
        assert!(r.contains('‹'));
        assert!(r.contains('›'));
        assert!(r.contains("Day"));
    }

    #[test]
    fn custom_modes_replace_the_default_set() {
        let modes = ["List", "Board"];
        let nav = DateNavigator::new("X").modes(&modes);
        let r = lines(nav, 50, 1);
        assert!(r.contains("List"));
        assert!(r.contains("Board"));
        assert!(!r.contains("Agenda"));
    }

    #[test]
    fn empty_modes_draws_no_switch_and_no_mode_targets() {
        let modes: [&str; 0] = [];
        let nav = DateNavigator::new("X").modes(&modes);
        let area = Rect::new(0, 0, 40, 1);
        let segs = nav.segments(nav.strip(area));
        assert!(
            segs.iter()
                .all(|s| !matches!(s.target, Some(NavTarget::Mode(_))))
        );
        // Prev/Next still resolve.
        assert_eq!(
            nav.target_at(area, Position::new(1, 0)),
            Some(NavTarget::Prev)
        );
        assert_eq!(
            nav.target_at(area, Position::new(39, 0)),
            Some(NavTarget::Next)
        );
    }

    #[test]
    fn a_block_frames_the_strip_on_its_inner_row() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 3));
        DateNavigator::new("X")
            .block(Block::bordered())
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌');
        // The strip is the inner first row (y=1, x=1..).
        assert_eq!(buf.get(Position::new(2, 1)).unwrap().symbol, '‹');
        // The hit-test accounts for the border inset.
        let nav = DateNavigator::new("X").block(Block::bordered());
        assert_eq!(
            nav.target_at(buf.area(), Position::new(2, 1)),
            Some(NavTarget::Prev)
        );
        assert_eq!(nav.target_at(buf.area(), Position::new(2, 0)), None); // border
    }

    #[test]
    fn base_style_fills_the_whole_strip_row() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 1));
        DateNavigator::new("X")
            .style(Style::new().bg(Color::Red))
            .render(buf.area(), &mut buf);
        for x in 0..30 {
            assert_eq!(buf.get(Position::new(x, 0)).unwrap().bg, Color::Red);
        }
    }

    #[test]
    fn label_and_button_styles_are_patched_over_the_base() {
        // Width 70 keeps the controls, the centred label, and the switch.
        let mut buf = Buffer::empty(Rect::new(0, 0, 70, 1));
        DateNavigator::new("MAY")
            .style(Style::new().fg(Color::White))
            .label_style(Style::new().add_modifier(Modifier::BOLD))
            .button_style(Style::new().fg(Color::Green))
            .render(buf.area(), &mut buf);
        // The prev glyph carries the button fg.
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().fg, Color::Green);
        // The label glyph 'M' is bold (located in the rendered row).
        let m_x = (0..70)
            .find(|&x| buf.get(Position::new(x, 0)).unwrap().symbol == 'M')
            .expect("the label is drawn");
        assert!(
            buf.get(Position::new(m_x, 0))
                .unwrap()
                .modifier
                .contains(Modifier::BOLD)
        );
        // A selected-style-free unselected mode segment keeps the button fg.
        let d_x = (0..70)
            .find(|&x| buf.get(Position::new(x, 0)).unwrap().symbol == 'D')
            .expect("the Day segment is drawn");
        assert_eq!(buf.get(Position::new(d_x, 0)).unwrap().fg, Color::Green);
    }

    #[test]
    fn a_narrow_strip_elides_from_the_right_inward_without_panicking() {
        // Just wide enough for the left controls; the mode switch + label are
        // dropped, not half-drawn.
        let r = lines(DateNavigator::new("May 2026"), 18, 1);
        assert!(r.contains('‹'));
        assert!(r.contains("Today"));
        // No room for the switch.
        assert!(!r.contains("Agenda"));
    }

    #[test]
    fn a_tiny_strip_keeps_only_what_fits_and_never_panics() {
        // Width 3: only ` ‹ ` fits; everything else is elided.
        let nav = DateNavigator::new("May 2026");
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        nav.clone().render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, '‹');
        assert_eq!(
            nav.target_at(buf.area(), Position::new(1, 0)),
            Some(NavTarget::Prev)
        );
        // Width 1: not even the prev control fits — a blank, total strip.
        assert_eq!(lines(DateNavigator::new("x"), 1, 1), " \n");
    }

    #[test]
    fn render_uses_the_area_origin_not_the_buffer_origin() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 4));
        DateNavigator::new("X").render(Rect::new(2, 3, 30, 1), &mut buf);
        // Prev glyph is at the area origin's `␣‹␣` (col 3 of row 3).
        assert_eq!(buf.get(Position::new(3, 3)).unwrap().symbol, '‹');
        // Untouched elsewhere.
        assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, ' ');
    }

    #[test]
    fn the_centred_label_never_overwrites_a_control() {
        // A very long label in a strip with little gap is clipped into the
        // gap, never over the prev/today/mode segments.
        let nav = DateNavigator::new("A very long period label indeed");
        let area = Rect::new(0, 0, 40, 1);
        let segs = nav.segments(nav.strip(area));
        let label = segs.iter().find(|s| s.target.is_none());
        if let Some(l) = label {
            // The label span does not overlap any interactive segment.
            for s in &segs {
                if s.target.is_some() {
                    assert!(
                        l.end <= s.start || l.start >= s.end,
                        "label {:?} overlaps {:?}",
                        (l.start, l.end),
                        (s.start, s.end)
                    );
                }
            }
        }
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 1));
        DateNavigator::new("X")
            .mode(1)
            .render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
