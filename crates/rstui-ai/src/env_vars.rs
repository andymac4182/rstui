//! [`EnvVars`] — an env-var table with a mask/reveal toggle and per-row copy:
//! the "environment" panel a sandbox/exec tool projects (secrets dotted out
//! until the user reveals them).
//!
//! # A pure projection of `&[(key,value)]` + caller-owned `show`
//!
//! The ai-elements `EnvironmentVariables` is a key/value table with an
//! eye-toggle to reveal masked values and a per-row copy button. Whether
//! values are revealed is ordinary application state. So `EnvVars` owns
//! nothing: it projects the caller's `&[(String, String)]` and a
//! caller-owned [`show`](EnvVars::show) `bool` (values rendered as `•` until
//! set).
//!
//! Both controls are the documented hit-test seam, never a callback: the host
//! maps a click in [`toggle_rect`](EnvVars::toggle_rect) to
//! [`EnvVarsIntent::ToggleReveal`] and a click in a
//! [`copy_rects`](EnvVars::copy_rects) entry to
//! [`EnvVarsIntent::CopyRow`].
//!
//! # Clamp, don't panic
//!
//! Per the [`Gauge`](rstui_widgets::Gauge) totality rule a zero/tiny area, an
//! empty table, and over-many rows are all safe clips — never a panic.

use rstui_core::{Buffer, Modifier, Position, Rect, Style, Widget};

/// The reducer-consumed intent an [`EnvVars`] surfaces — the host maps a
/// click in [`toggle_rect`](EnvVars::toggle_rect) /
/// [`copy_rects`](EnvVars::copy_rects) to this; the reducer flips `show` or
/// copies the row's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvVarsIntent {
    /// The eye toggle was activated — flip the caller's `show`.
    ToggleReveal,
    /// The copy affordance on the row at this index was activated.
    CopyRow(usize),
}

/// The masked-value glyph (one per value character).
const DOT: char = '•';

/// An env-var table with a mask/reveal toggle and per-row copy.
///
/// The first row is a `[ value of show ]`-style eye toggle (`◉ shown` /
/// `○ hidden`). Each subsequent row is `KEY = value` (the value is `•`
/// repeated until [`show`](Self::show)), with a trailing `⧉` copy glyph.
/// `EnvVars` owns no state — see the [module docs](self).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_ai::env_vars::{EnvVars, EnvVarsIntent};
///
/// let vars = [("API_KEY".to_string(), "sk-secret".to_string())];
/// let widget = EnvVars::new(&vars);
/// let area = Rect::new(0, 0, 24, 2);
///
/// // The eye toggle is row 0; the copy glyph trails row 1.
/// assert!(widget.toggle_rect(area).is_some());
/// assert_eq!(widget.copy_rects(area).len(), 1);
///
/// let mut buf = Buffer::empty(area);
/// widget.render(buf.area(), &mut buf);
/// // Hidden by default → the value is dotted out.
/// assert_eq!(buf.get(Position::new(0, 1)).unwrap().symbol, 'A'); // API_KEY
/// ```
#[derive(Debug, Clone)]
pub struct EnvVars<'a> {
    vars: &'a [(String, String)],
    show: bool,
    style: Style,
}

impl<'a> EnvVars<'a> {
    /// A table of `vars` (`(key, value)` pairs) with values masked.
    #[must_use]
    pub fn new(vars: &'a [(String, String)]) -> Self {
        Self {
            vars,
            show: false,
            style: Style::new(),
        }
    }

    /// Sets the caller-owned reveal flag (the reducer flips it on a
    /// [`toggle_rect`](Self::toggle_rect) click; values un-mask when set).
    #[must_use]
    pub fn show(mut self, show: bool) -> Self {
        self.show = show;
        self
    }

    /// Sets the base [`Style`].
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The eye-toggle row [`Rect`] (row 0), or `None` for an empty area. The
    /// host hit-tests a click here → [`EnvVarsIntent::ToggleReveal`].
    #[must_use]
    pub fn toggle_rect(&self, area: Rect) -> Option<Rect> {
        if area.is_empty() {
            return None;
        }
        Some(Rect::new(area.left(), area.top(), area.width, 1))
    }

    /// The 1×1 copy-glyph [`Rect`] of every visible var row, in order
    /// (parallel to the slice, clipped to the area). The host maps a click
    /// to [`EnvVarsIntent::CopyRow`] with that index.
    #[must_use]
    pub fn copy_rects(&self, area: Rect) -> Vec<Rect> {
        if area.is_empty() || area.height <= 1 {
            return Vec::new();
        }
        let rows = (area.height as usize - 1).min(self.vars.len());
        let glyph_x = area.right().saturating_sub(1);
        (0..rows)
            .map(|n| {
                Rect::new(
                    glyph_x,
                    area.top().saturating_add(1).saturating_add(n as u16),
                    1,
                    1,
                )
            })
            .collect()
    }
}

impl Widget for EnvVars<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        buf.set_style(area, self.style);

        // Row 0: the eye toggle.
        let toggle = if self.show { "◉ shown" } else { "○ hidden" };
        let mut x = area.left();
        let toggle_style = self.style.add_modifier(Modifier::BOLD);
        for ch in toggle.chars() {
            if x >= area.right() {
                break;
            }
            buf.set_cell(Position::new(x, area.top()), ch, toggle_style);
            x = x.saturating_add(1);
        }

        // Var rows.
        let glyph_x = area.right().saturating_sub(1);
        let value_right = glyph_x.saturating_sub(1);
        for (n, (key, value)) in self
            .vars
            .iter()
            .take((area.height as usize).saturating_sub(1))
            .enumerate()
        {
            let y = area.top().saturating_add(1).saturating_add(n as u16);
            // The copy glyph always survives on the last column.
            buf.set_cell(Position::new(glyph_x, y), '⧉', self.style);

            let shown = if self.show {
                value.clone()
            } else {
                DOT.to_string().repeat(value.chars().count())
            };
            let line = format!("{key} = {shown}");
            let mut lx = area.left();
            for ch in line.chars() {
                if lx >= value_right {
                    break;
                }
                buf.set_cell(Position::new(lx, y), ch, self.style);
                lx = lx.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Color;

    fn vars() -> Vec<(String, String)> {
        vec![
            ("API_KEY".to_string(), "sk-x".to_string()),
            ("HOST".to_string(), "prod".to_string()),
        ]
    }

    fn lines(widget: EnvVars<'_>, w: u16, h: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        widget.render(buf.area(), &mut buf);
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn values_are_masked_until_revealed() {
        let v = vars();
        let hidden = lines(EnvVars::new(&v), 16, 3);
        assert!(hidden.contains("○ hidden"), "got {hidden:?}");
        assert!(hidden.contains("API_KEY = ••••"), "got {hidden:?}");
        let shown = lines(EnvVars::new(&v).show(true), 16, 3);
        assert!(shown.contains("◉ shown"), "got {shown:?}");
        assert!(shown.contains("API_KEY = sk-x"), "got {shown:?}");
    }

    #[test]
    fn each_row_has_a_trailing_copy_glyph() {
        let v = vars();
        let out = lines(EnvVars::new(&v), 16, 3);
        // Last column of rows 1 and 2 is the copy glyph.
        for line in out.lines().skip(1) {
            assert!(line.ends_with('⧉'), "got {line:?}");
        }
    }

    #[test]
    fn toggle_rect_is_row_zero_copy_rects_track_each_var_row() {
        let v = vars();
        let area = Rect::new(0, 0, 16, 3);
        assert_eq!(
            EnvVars::new(&v).toggle_rect(area),
            Some(Rect::new(0, 0, 16, 1))
        );
        let copies = EnvVars::new(&v).copy_rects(area);
        assert_eq!(copies, vec![Rect::new(15, 1, 1, 1), Rect::new(15, 2, 1, 1)]);
    }

    #[test]
    fn over_many_rows_clip_to_the_area() {
        let v = vars();
        // height 2 → toggle + only the first var; copy_rects has 1 entry.
        let area = Rect::new(0, 0, 16, 2);
        assert_eq!(EnvVars::new(&v).copy_rects(area).len(), 1);
    }

    #[test]
    fn an_empty_table_is_just_the_toggle() {
        let empty: [(String, String); 0] = [];
        let out = lines(EnvVars::new(&empty), 12, 3);
        assert!(out.starts_with("○ hidden"), "got {out:?}");
        assert!(
            EnvVars::new(&empty)
                .copy_rects(Rect::new(0, 0, 12, 3))
                .is_empty()
        );
    }

    #[test]
    fn tiny_and_zero_areas_are_safe() {
        let v = vars();
        assert_eq!(EnvVars::new(&v).toggle_rect(Rect::new(0, 0, 0, 0)), None);
        assert!(
            EnvVars::new(&v)
                .copy_rects(Rect::new(0, 0, 5, 1))
                .is_empty()
        );
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        EnvVars::new(&v).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn the_base_style_cascades() {
        let v = vars();
        let mut buf = Buffer::empty(Rect::new(0, 0, 16, 3));
        EnvVars::new(&v)
            .style(Style::new().bg(Color::Blue))
            .render(buf.area(), &mut buf);
        assert_eq!(buf.get(Position::new(0, 1)).unwrap().bg, Color::Blue);
    }
}
