//! A music-player experience: a playlist [`Table`], album art, a seek
//! [`Gauge`] with time labels, a transport row, a volume [`Slider`], and a
//! `tick`-driven [`Sparkline`] visualiser + [`Spinner`]. `Space` plays /
//! pauses, `↑/↓` pick a track, `←/→` seek, `+/-` volume.

use rstui_core::{
    Constraint, KeyCode, Layout, Line, Margin, Modifier, Position, Rect, Style, stylize::Stylize,
};
use rstui_runtime::Frame;
use rstui_widgets::{Block, BorderType, Gauge, Row, Slider, Sparkline, Spinner, Table};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// title, artist, length (seconds).
const TRACKS: [(&str, &str, u32); 6] = [
    ("Truecolor Dreams", "The RGB Trio", 214),
    ("Raw Mode", "Crossterm", 188),
    ("Pure Projection", "Reducer", 247),
    ("Alternate Screen", "Lifecycle", 201),
    ("Bracketed Paste", "stdin", 173),
    ("Panic-Safe Restore", "Drop", 226),
];

/// The player's caller-owned state.
#[derive(Debug)]
pub(crate) struct State {
    sel: usize,
    playing: bool,
    /// Playback position in seconds into the current track.
    pos: u32,
    vol: i32,
}

impl State {
    /// First track, paused, 60% volume.
    pub(crate) fn new() -> Self {
        Self {
            sel: 0,
            playing: false,
            pos: 42,
            vol: 60,
        }
    }

    fn len(&self) -> u32 {
        TRACKS[self.sel].2
    }

    /// `Space` play/pause, `↑↓` track, `←→` seek, `+/-` volume, `Enter`
    /// play the highlighted track from the top.
    pub(crate) fn on_key(&mut self, code: KeyCode) -> ScreenOutcome {
        match code {
            KeyCode::Char(' ') => self.playing = !self.playing,
            KeyCode::Up => {
                self.sel = self.sel.saturating_sub(1);
                self.pos = 0;
            }
            KeyCode::Down => {
                self.sel = (self.sel + 1).min(TRACKS.len() - 1);
                self.pos = 0;
            }
            KeyCode::Enter => {
                self.playing = true;
                self.pos = 0;
                return ScreenOutcome::with_toast(
                    crate::screens::ToastLevel::Success,
                    format!("▶ {}", TRACKS[self.sel].0),
                );
            }
            KeyCode::Left => self.pos = self.pos.saturating_sub(5),
            KeyCode::Right => self.pos = (self.pos + 5).min(self.len()),
            KeyCode::Char('+') | KeyCode::Char('=') => self.vol = (self.vol + 5).min(100),
            KeyCode::Char('-') => self.vol = (self.vol - 5).max(0),
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// Click a playlist row to play it; click the transport row to
    /// play/pause. Geometry mirrors [`view`].
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let [list, right] =
            Layout::horizontal([Constraint::Percentage(46), Constraint::Fill(1)]).areas(content);
        let lin = list.inner(Margin::new(1, 1));
        if lin.contains(pos) {
            // Header is row 0; tracks follow.
            if pos.y > lin.y {
                let idx = (pos.y - lin.y - 1) as usize;
                if idx < TRACKS.len() {
                    self.sel = idx;
                    self.playing = true;
                    self.pos = 0;
                    return ScreenOutcome::with_toast(
                        crate::screens::ToastLevel::Success,
                        format!("▶ {}", TRACKS[idx].0),
                    );
                }
            }
            return ScreenOutcome::consumed();
        }
        let nin = right.inner(Margin::new(1, 1));
        let [_art, _title, _seek, _times, transport, _vol, _viz] = Layout::vertical([
            Constraint::Length(5),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(nin);
        if transport.contains(pos) {
            self.playing = !self.playing;
            return ScreenOutcome::consumed();
        }
        ScreenOutcome::ignored()
    }

    /// Draw the player. `tick` drives the visualiser + spinner.
    pub(crate) fn view(&self, theme: &Theme, tick: u64, frame: &mut Frame<'_>, area: Rect) {
        let [list, right] =
            Layout::horizontal([Constraint::Percentage(46), Constraint::Fill(1)]).areas(area);

        // Playlist.
        let lblock = panel(theme, "Playlist");
        let lin = lblock.inner(list);
        frame.render_widget(lblock, list);
        frame.render_widget(
            Table::new(
                TRACKS.iter().enumerate().map(|(i, (t, a, s))| {
                    let mark = if i == self.sel && self.playing {
                        "▶"
                    } else if i == self.sel {
                        "•"
                    } else {
                        " "
                    };
                    Row::new([
                        mark.to_string(),
                        (*t).to_string(),
                        (*a).to_string(),
                        format!("{}:{:02}", s / 60, s % 60),
                    ])
                }),
                [
                    Constraint::Length(2),
                    Constraint::Fill(1),
                    Constraint::Length(14),
                    Constraint::Length(6),
                ],
            )
            .header(Row::new(["", "Title", "Artist", "Len"]).style(theme.accent_text()))
            .selected(Some(self.sel))
            .highlight_style(theme.selection())
            .style(theme.body())
            .column_spacing(1),
            lin,
        );

        // Now playing.
        let nblock = panel(theme, "Now playing");
        let nin = nblock.inner(right);
        frame.render_widget(nblock, right);
        let [art, title, seek, times, transport, vol, viz] = Layout::vertical([
            Constraint::Length(5),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(nin);

        // Album art: a 24-bit colour banner that shifts hue with the track.
        for y in 0..art.height {
            for x in 0..art.width {
                let shade = 70u8
                    .saturating_add(self.sel as u8 * 18)
                    .saturating_add((x as u8).wrapping_mul(3))
                    .saturating_add((y as u8) * 10);
                frame.buffer_mut().set_cell(
                    rstui_core::Position::new(art.x + x, art.y + y),
                    ' ',
                    Style::new().bg(rstui_core::Color::Rgb(
                        shade,
                        90,
                        200u8.saturating_sub(shade / 2),
                    )),
                );
            }
        }
        let (t, a, _) = TRACKS[self.sel];
        frame.render_widget(
            Line::from(t.fg(theme.text).bold()),
            Rect::new(title.x, title.y, title.width, 1),
        );
        frame.render_widget(
            Line::from(a.fg(theme.dim)),
            Rect::new(title.x, title.y + 1, title.width, 1),
        );

        let ratio = if self.len() == 0 {
            0.0
        } else {
            f64::from(self.pos) / f64::from(self.len())
        };
        frame.render_widget(
            Gauge::default()
                .ratio(ratio)
                .label(String::new())
                .style(theme.body())
                .gauge_style(Style::new().fg(theme.base).bg(theme.accent)),
            seek,
        );
        frame.render_widget(
            Line::from(vec![
                format!("{}:{:02}", self.pos / 60, self.pos % 60).fg(theme.dim),
                format!("  /  {}:{:02}", self.len() / 60, self.len() % 60).fg(theme.dim),
            ]),
            times,
        );

        let state = if self.playing {
            "⏸  Pause"
        } else {
            "▶  Play"
        };
        frame.render_widget(
            Line::from(vec![
                "  ⏮   ".fg(theme.dim),
                format!("[{state}]").fg(theme.base).bg(theme.accent).bold(),
                "   ⏭  ".fg(theme.dim),
                if self.playing {
                    "  streaming".fg(theme.ok)
                } else {
                    "  paused".fg(theme.warn)
                },
            ]),
            Rect::new(transport.x, transport.y, transport.width, 1),
        );

        frame.render_widget(
            Slider::new()
                .range(0.0, 100.0)
                .value(f64::from(self.vol))
                .label(Line::from("Vol".to_string()).style(theme.caption()))
                .value_label(Line::from(format!("{}%", self.vol)).style(theme.body()))
                .focused(true)
                .style(theme.body())
                .thumb_style(Style::new().fg(theme.accent))
                .focus_style(Style::new().fg(theme.base).bg(theme.accent)),
            vol,
        );

        // Visualiser: animates only while playing.
        let bars: Vec<u64> = (0..viz.width.max(1))
            .map(|x| {
                if !self.playing {
                    2
                } else {
                    let p = f64::from(x) * 0.6 + f64::from((tick % 40) as u32) * 0.5;
                    (p.sin().abs() * f64::from(viz.height.max(1)) * 2.0) as u64 + 1
                }
            })
            .collect();
        frame.render_widget(
            Sparkline::new(&bars).style(Style::new().fg(theme.accent_alt)),
            viz,
        );
        if self.playing {
            frame.render_widget(
                Spinner::new()
                    .tick(tick as usize)
                    .style(Style::new().fg(theme.accent).add_modifier(Modifier::BOLD)),
                Rect::new(nin.x + nin.width.saturating_sub(2), nin.y, 1, 1),
            );
        }
    }
}

/// A rounded display panel.
fn panel(theme: &Theme, title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Line::from(format!(" {title} ")).style(theme.caption()))
        .border_style(theme.border())
        .style(theme.body())
}
