//! The full-colour lab: the 16 named ANSI colours, the 256-colour indexed
//! cube, 24-bit RGB truecolor gradients, and the text-attribute sampler —
//! with a keyboard/mouse cursor over the 256 grid.

use rstui_core::{Color, Constraint, Layout, Modifier, Position, Rect, Style};
use rstui_runtime::Frame;
use rstui_widgets::{Block, BorderType};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// The 16 named ANSI colours, in palette order.
const NAMED: [(Color, &str); 16] = [
    (Color::Black, "Black"),
    (Color::Red, "Red"),
    (Color::Green, "Green"),
    (Color::Yellow, "Yellow"),
    (Color::Blue, "Blue"),
    (Color::Magenta, "Magenta"),
    (Color::Cyan, "Cyan"),
    (Color::Gray, "Gray"),
    (Color::DarkGray, "DarkGray"),
    (Color::LightRed, "LightRed"),
    (Color::LightGreen, "LightGreen"),
    (Color::LightYellow, "LightYellow"),
    (Color::LightBlue, "LightBlue"),
    (Color::LightMagenta, "LightMagenta"),
    (Color::LightCyan, "LightCyan"),
    (Color::White, "White"),
];

/// The attribute sampler entries.
const MODS: [(Modifier, &str); 7] = [
    (Modifier::BOLD, "BOLD"),
    (Modifier::DIM, "DIM"),
    (Modifier::ITALIC, "ITALIC"),
    (Modifier::UNDERLINED, "UNDERLINED"),
    (Modifier::REVERSED, "REVERSED"),
    (Modifier::CROSSED_OUT, "CROSSED_OUT"),
    (Modifier::SLOW_BLINK, "SLOW_BLINK"),
];

/// Where the indexed-cube cursor sits and which attribute is sampled.
#[derive(Debug)]
pub(crate) struct State {
    /// The selected 256-colour index (0..=255).
    cursor: u8,
    /// The selected attribute in [`MODS`].
    mod_idx: usize,
}

impl State {
    /// Cursor on index 0, BOLD sampled.
    pub(crate) fn new() -> Self {
        Self {
            cursor: 0,
            mod_idx: 0,
        }
    }

    /// Arrows move the cube cursor; `m` cycles the sampled attribute. `←` at
    /// the left column is released so the rail can take it back.
    pub(crate) fn on_key(&mut self, code: rstui_core::KeyCode) -> ScreenOutcome {
        use rstui_core::KeyCode::{Char, Down, Left, Right, Up};
        match code {
            Left => {
                if self.cursor % 16 == 0 {
                    return ScreenOutcome::ignored();
                }
                self.cursor -= 1;
            }
            Right => self.cursor = self.cursor.saturating_add(1),
            Up => self.cursor = self.cursor.saturating_sub(16),
            Down => {
                if self.cursor <= 255 - 16 {
                    self.cursor += 16;
                }
            }
            Char('m') | Char(' ') => self.mod_idx = (self.mod_idx + 1) % MODS.len(),
            _ => return ScreenOutcome::ignored(),
        }
        ScreenOutcome::consumed()
    }

    /// A click inside the indexed cube selects that swatch.
    pub(crate) fn on_click(&mut self, pos: Position, content: Rect) -> ScreenOutcome {
        let cube = Self::cube_rect(content);
        if cube.contains(pos) {
            let col = ((pos.x - cube.x) / 2).min(15);
            let row = (pos.y - cube.y).min(15);
            self.cursor = (row * 16 + col) as u8;
            return ScreenOutcome::consumed();
        }
        ScreenOutcome::ignored()
    }

    /// The 16×16 indexed-cube rect — the one geometry the renderer and the
    /// click hit-test share.
    fn cube_rect(content: Rect) -> Rect {
        let [_, _, cube, _, _] = Self::rows(content);
        Rect::new(cube.x, cube.y, 32.min(cube.width), 16.min(cube.height))
    }

    /// The five stacked sections of the lab.
    fn rows(area: Rect) -> [Rect; 5] {
        Layout::vertical([
            Constraint::Length(3),  // named ANSI
            Constraint::Length(1),  // cube caption
            Constraint::Length(16), // 256 cube
            Constraint::Length(5),  // RGB gradients
            Constraint::Fill(1),    // attribute sampler
        ])
        .areas(area)
    }

    /// Draw the colour lab.
    pub(crate) fn view(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let [named, caption, _cube, grad, mods] = Self::rows(area);

        // 16 named ANSI swatches.
        let cols = Layout::horizontal([Constraint::Fill(1); 8]).split(Rect::new(
            named.x,
            named.y,
            named.width,
            1,
        ));
        let cols2 = Layout::horizontal([Constraint::Fill(1); 8]).split(Rect::new(
            named.x,
            named.y + 1,
            named.width,
            1,
        ));
        for (i, (colour, name)) in NAMED.iter().enumerate() {
            let row = if i < 8 { &cols[i] } else { &cols2[i - 8] };
            let buf = frame.buffer_mut();
            buf.set_style(*row, Style::new().bg(*colour));
            let fg = if matches!(
                colour,
                Color::Black | Color::Blue | Color::DarkGray | Color::Red | Color::Magenta
            ) {
                Color::White
            } else {
                Color::Black
            };
            buf.set_str(row.position(), name, Style::new().fg(fg).bg(*colour));
        }
        frame.render_widget(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .title(rstui_core::Line::from(" 16 named ANSI ").style(theme.heading()))
                .border_style(theme.border()),
            Rect::new(named.x, named.y + 2, named.width, 1),
        );

        // The 256-colour indexed cube with the cursor.
        let label = format!(
            " 256-indexed cube — Indexed({})  [↑↓←→ move · click · m attr] ",
            self.cursor
        );
        frame.render_widget(
            rstui_core::Line::from(label).style(theme.caption()),
            caption,
        );
        let cube_rect = Self::cube_rect(area);
        for idx in 0u16..256 {
            let (col, row) = (idx % 16, idx / 16);
            let x = cube_rect.x + col * 2;
            let y = cube_rect.y + row;
            if x + 1 >= cube_rect.right() || y >= cube_rect.bottom() {
                continue;
            }
            let selected = idx as u8 == self.cursor;
            let bg = Color::Indexed(idx as u8);
            let buf = frame.buffer_mut();
            let cell = Style::new().bg(bg);
            buf.set_cell(Position::new(x, y), if selected { '[' } else { ' ' }, cell);
            buf.set_cell(
                Position::new(x + 1, y),
                if selected { ']' } else { ' ' },
                cell,
            );
        }

        // 24-bit RGB truecolor gradients: red, green, blue, hue.
        let bands = Layout::vertical([Constraint::Length(1); 4]).split(grad);
        for (b, band) in bands.iter().enumerate() {
            let w = band.width.max(1);
            for x in 0..w {
                let t = (x as u32 * 255 / w as u32) as u8;
                let colour = match b {
                    0 => Color::Rgb(t, 0, 0),
                    1 => Color::Rgb(0, t, 0),
                    2 => Color::Rgb(0, 0, t),
                    _ => hue(x as u32 * 360 / w as u32),
                };
                frame.buffer_mut().set_cell(
                    Position::new(band.x + x, band.y),
                    ' ',
                    Style::new().bg(colour),
                );
            }
        }

        // The attribute sampler.
        let mod_rows = Layout::vertical([Constraint::Length(1); 7]).split(mods);
        for (i, (modifier, name)) in MODS.iter().enumerate() {
            let Some(row) = mod_rows.get(i) else { continue };
            let chosen = i == self.mod_idx;
            let marker = if chosen { "▶ " } else { "  " };
            let sample = Style::new()
                .fg(if chosen { theme.accent } else { theme.text })
                .bg(theme.surface)
                .add_modifier(*modifier);
            let buf = frame.buffer_mut();
            let p = buf.set_str(row.position(), marker, theme.caption());
            buf.set_str(p, &format!("The quick brown fox — {name}"), sample);
        }
    }
}

/// A point on the HSV hue wheel at full saturation/value, as 24-bit RGB.
fn hue(deg: u32) -> Color {
    let h = (deg % 360) as f64 / 60.0;
    let x = (1.0 - (h % 2.0 - 1.0).abs()) * 255.0;
    let x = x as u8;
    let (r, g, b) = match h as u32 {
        0 => (255, x, 0),
        1 => (x, 255, 0),
        2 => (0, 255, x),
        3 => (0, x, 255),
        4 => (x, 0, 255),
        _ => (255, 0, x),
    };
    Color::Rgb(r, g, b)
}
