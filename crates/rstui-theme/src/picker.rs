//! [`ThemePicker`] — a reusable "browse every theme, preview it live, then
//! keep it" widget, plus the tiny persistence pair that makes the choice
//! stick across launches.
//!
//! # The model
//!
//! Like the rest of rstui, the widget is a **pure projection of caller-owned
//! state**. The app keeps a [`ThemePickerState`] (the catalogue + which row
//! is highlighted + an optional filter); [`ThemePicker`] only draws it.
//!
//! "See it applied before you pick" falls out of that split: every frame the
//! app themes its UI from [`ThemePickerState::selected_theme`]'s palette, so
//! moving the highlight *is* the live preview — no special preview mode. On
//! `Enter` the app persists the choice with [`Theme::write_choice`] and
//! reloads it next launch with [`Theme::read_choice`]; on `Esc` it restores
//! the palette it had before opening the picker. The widget itself performs
//! no I/O and reads no clock, so it is deterministic under snapshot tests.
//!
//! ```
//! use rstui_theme::{ThemePicker, ThemePickerState};
//! use rstui_core::{Buffer, Rect, Widget};
//!
//! struct App { picker: ThemePickerState, open: bool }
//! # let app = App { picker: ThemePickerState::new(), open: true };
//! # let mut buf = Buffer::empty(Rect::new(0, 0, 40, 12));
//! if app.open {
//!     // app.theme = from(app.picker.selected_theme()?.palette)  // live preview
//!     ThemePicker::new(&app.picker).render(Rect::new(0, 0, 40, 12), &mut buf);
//! }
//! ```

use crate::registry::Theme;
use rstui_core::{Buffer, Modifier, Position, Rect, Style, Widget};
use std::path::Path;

/// Caller-owned theme-picker state: the catalogue to browse, the highlighted
/// row, and a name filter. Keep one on your model; drive it from key events.
#[derive(Debug, Clone)]
pub struct ThemePickerState {
    /// The themes offered (defaults to the full built-in catalogue).
    themes: Vec<Theme>,
    /// Highlighted index into the *filtered* view.
    selected: usize,
    /// Case-insensitive name filter (empty = show all).
    query: String,
}

impl Default for ThemePickerState {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemePickerState {
    /// A picker over every built-in theme ([`Theme::all`]).
    #[must_use]
    pub fn new() -> Self {
        Self::from_themes(Theme::all())
    }

    /// A picker over a caller-supplied set (e.g. built-ins plus
    /// [`Theme::load_dir`] user themes).
    #[must_use]
    pub fn from_themes(themes: Vec<Theme>) -> Self {
        Self {
            themes,
            selected: 0,
            query: String::new(),
        }
    }

    /// Indices into `themes` matching the current filter, in order.
    fn matches(&self) -> Vec<usize> {
        if self.query.is_empty() {
            return (0..self.themes.len()).collect();
        }
        let q = self.query.to_lowercase();
        self.themes
            .iter()
            .enumerate()
            .filter(|(_, t)| t.name.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect()
    }

    /// The highlighted theme, or `None` when the filter matches nothing.
    /// The app reads this every frame and themes itself from its palette —
    /// that *is* the live preview.
    #[must_use]
    pub fn selected_theme(&self) -> Option<&Theme> {
        let m = self.matches();
        m.get(self.selected.min(m.len().saturating_sub(1)))
            .map(|&i| &self.themes[i])
    }

    /// Move the highlight to the next match (wraps).
    pub fn next(&mut self) {
        let n = self.matches().len();
        if n > 0 {
            self.selected = (self.selected + 1) % n;
        }
    }

    /// Move the highlight to the previous match (wraps).
    pub fn prev(&mut self) {
        let n = self.matches().len();
        if n > 0 {
            self.selected = (self.selected + n - 1) % n;
        }
    }

    /// Append a character to the name filter (resets the highlight to the
    /// first match).
    pub fn push_filter(&mut self, c: char) {
        self.query.push(c);
        self.selected = 0;
    }

    /// Delete the last filter character (resets the highlight).
    pub fn pop_filter(&mut self) {
        self.query.pop();
        self.selected = 0;
    }

    /// The current filter text (for the app to echo if it wants).
    #[must_use]
    pub fn filter(&self) -> &str {
        &self.query
    }

    /// Number of themes matching the current filter.
    #[must_use]
    pub fn match_count(&self) -> usize {
        self.matches().len()
    }
}

/// A widget that draws a [`ThemePickerState`]: a scrollable theme list with
/// the highlight, a live swatch strip of the highlighted theme's own
/// colours, and a key hint. A pure projection — it owns nothing and only
/// writes the [`Buffer`], clipped to `area` (a no-op at zero size).
#[derive(Debug)]
pub struct ThemePicker<'a> {
    state: &'a ThemePickerState,
    style: Style,
    highlight: Style,
    title: &'a str,
}

impl<'a> ThemePicker<'a> {
    /// A picker view over `state` with sensible default styling.
    #[must_use]
    pub fn new(state: &'a ThemePickerState) -> Self {
        Self {
            state,
            style: Style::new(),
            highlight: Style::new().add_modifier(Modifier::REVERSED),
            title: "Theme",
        }
    }

    /// Sets the base text/background style of the panel.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the style of the highlighted row.
    #[must_use]
    pub fn highlight_style(mut self, style: Style) -> Self {
        self.highlight = style;
        self
    }

    /// Sets the heading shown on the first line.
    #[must_use]
    pub fn title(mut self, title: &'a str) -> Self {
        self.title = title;
        self
    }
}

impl Widget for ThemePicker<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        buf.set_style(area, self.style);
        let w = area.width as usize;

        // Row 0: title + match count + filter.
        let header = format!(
            "{}  ({})  ⌕ {}",
            self.title,
            self.state.match_count(),
            self.state.filter()
        );
        buf.set_str(
            Position::new(area.x, area.y),
            &clip(&header, w),
            self.style.add_modifier(Modifier::BOLD),
        );
        if area.height <= 1 {
            return;
        }

        // Bottom row: the key hint.
        let hint = "↑↓ preview · Enter keep · Esc cancel · type to filter";
        let hint_y = area.bottom() - 1;
        buf.set_str(
            Position::new(area.x, hint_y),
            &clip(hint, w),
            self.style.add_modifier(Modifier::DIM),
        );

        // Row above the hint: a live swatch strip of the highlighted theme.
        let mut list_bottom = hint_y;
        if area.height >= 3 {
            if let Some(t) = self.state.selected_theme() {
                let sw_y = hint_y - 1;
                let p = &t.palette;
                let swatches = [
                    p.background,
                    p.foreground,
                    p.primary,
                    p.accent,
                    p.success,
                    p.warning,
                    p.danger,
                    p.border,
                ];
                let mut x = area.x;
                for c in swatches {
                    if x + 2 > area.x + area.width {
                        break;
                    }
                    buf.set_str(Position::new(x, sw_y), "  ", Style::new().bg(c));
                    x += 2;
                }
                // Name of the previewed theme after the swatches.
                if x + 1 < area.x + area.width {
                    let label = format!(" {}", t.name);
                    let avail = (area.x + area.width - x) as usize;
                    buf.set_str(Position::new(x, sw_y), &clip(&label, avail), self.style);
                }
                list_bottom = sw_y;
            }
        }

        // Rows 1..list_bottom: the theme list, scrolled to keep the
        // highlight visible.
        let rows = list_bottom.saturating_sub(area.y + 1);
        if rows == 0 {
            return;
        }
        let matches = self.state.matches();
        if matches.is_empty() {
            buf.set_str(
                Position::new(area.x, area.y + 1),
                &clip("(no theme matches the filter)", w),
                self.style.add_modifier(Modifier::DIM),
            );
            return;
        }
        let rows = rows as usize;
        let sel = self.state.selected.min(matches.len() - 1);
        let offset = sel
            .saturating_sub(rows.saturating_sub(1))
            .min(matches.len().saturating_sub(rows));
        for (row, &idx) in matches.iter().skip(offset).take(rows).enumerate() {
            let y = area.y + 1 + row as u16;
            let is_sel = offset + row == sel;
            let mark = if is_sel { "▸ " } else { "  " };
            let line = format!("{mark}{}", self.state.themes[idx].name);
            let st = if is_sel { self.highlight } else { self.style };
            if is_sel {
                buf.set_style(Rect::new(area.x, y, area.width, 1), st);
            }
            buf.set_str(Position::new(area.x, y), &clip(&line, w), st);
        }
    }
}

/// Truncate `s` to `max` columns (char-wise; ASCII-safe for the matcher).
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        s.chars().take(max).collect()
    }
}

impl Theme {
    /// Persist a chosen theme **name** to `path` (creating parent dirs) so a
    /// picked theme survives a restart. Pair with [`read_choice`].
    ///
    /// [`read_choice`]: Theme::read_choice
    pub fn write_choice(path: impl AsRef<Path>, name: &str) -> std::io::Result<()> {
        let path = path.as_ref();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, format!("{}\n", name.trim()))
    }

    /// Resolve a previously [`write_choice`]-saved selection: read the name
    /// from `path` and look it up ([`by_name`], or [`from_set_file`] if it is
    /// a path to a theme file). `None` if the file is absent/unreadable or
    /// names nothing — so a missing choice cleanly falls back to a default.
    ///
    /// [`write_choice`]: Theme::write_choice
    /// [`by_name`]: Theme::by_name
    /// [`from_set_file`]: Theme::from_set_file
    #[must_use]
    pub fn read_choice(path: impl AsRef<Path>) -> Option<Theme> {
        let raw = std::fs::read_to_string(path).ok()?;
        let name = raw.trim();
        if name.is_empty() {
            return None;
        }
        if Path::new(name).is_file() {
            return Theme::from_set_file(name)
                .ok()?
                .into_iter()
                .find(|t| t.is_default)
                .or_else(|| Theme::from_set_file(name).ok()?.into_iter().next());
        }
        Theme::by_name(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_and_cycle_track_the_selected_theme() {
        let mut s = ThemePickerState::new();
        assert!(s.match_count() >= 21);
        let first = s.selected_theme().unwrap().name.clone();
        s.next();
        assert_ne!(s.selected_theme().unwrap().name, first);
        s.prev();
        assert_eq!(s.selected_theme().unwrap().name, first);

        // Filtering narrows the catalogue and keeps a valid selection.
        for c in "catppuccin".chars() {
            s.push_filter(c);
        }
        assert!(s.match_count() >= 1);
        assert!(
            s.selected_theme()
                .unwrap()
                .name
                .to_lowercase()
                .contains("catppuccin")
        );
        s.pop_filter();
        assert!(s.match_count() >= 1);
    }

    #[test]
    fn renders_and_is_total() {
        let s = ThemePickerState::new();
        // Zero-size: a no-op, never a panic.
        let mut z = Buffer::empty(Rect::new(0, 0, 0, 0));
        ThemePicker::new(&s).render(Rect::new(0, 0, 0, 0), &mut z);

        let mut buf = Buffer::empty(Rect::new(0, 0, 44, 14));
        ThemePicker::new(&s).render(Rect::new(0, 0, 44, 14), &mut buf);
        let mut text = String::new();
        for y in 0..14 {
            for x in 0..44 {
                text.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            text.push('\n');
        }
        assert!(text.contains("Theme"), "header renders");
        assert!(text.contains("Enter keep"), "hint renders");
        assert!(text.contains('▸'), "a row is highlighted");
    }

    #[test]
    fn write_then_read_choice_round_trips() {
        let dir = std::env::temp_dir().join(format!("rstui-theme-choice-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("theme");
        let pick = ThemePickerState::new();
        let name = pick.selected_theme().unwrap().name.clone();
        Theme::write_choice(&path, &name).unwrap();
        let back = Theme::read_choice(&path).expect("resolves the saved name");
        assert_eq!(back.name, name);
        assert!(Theme::read_choice(dir.join("absent")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
