//! `cargo run -p rstui-theme --example theme_gallery`
//!
//! Renders every built-in theme as a swatch card into an rstui [`Buffer`] —
//! the same `palette → Style → Buffer` path a real app uses — then emits the
//! buffer as 24-bit ANSI so you can eyeball all of them at once. Pass a
//! substring to filter (`… --example theme_gallery -- tokyo`).
//!
//! This is a demonstration, not a gate: it shows the intended wiring (thread
//! [`ThemePalette`](rstui_theme::ThemePalette) styles into widget builders /
//! the buffer at the call site), which is how rstui themes its UI.

use rstui_core::{Buffer, Color, Modifier, Position, Rect, Style};
use rstui_theme::Theme;

const WIDTH: u16 = 100;

fn main() {
    let filter = std::env::args().nth(1).unwrap_or_default().to_lowercase();
    let themes: Vec<Theme> = Theme::all()
        .into_iter()
        .filter(|t| filter.is_empty() || t.name.to_lowercase().contains(&filter))
        .collect();

    if themes.is_empty() {
        eprintln!("no theme matches {filter:?}");
        return;
    }

    // Three rows per card + one trailing spacer.
    let rows_per = 4u16;
    let height = rows_per * u16::try_from(themes.len()).unwrap_or(u16::MAX);
    let area = Rect {
        x: 0,
        y: 0,
        width: WIDTH,
        height,
    };
    let mut buf = Buffer::empty(area);

    for (i, theme) in themes.iter().enumerate() {
        let top = rows_per * u16::try_from(i).unwrap_or(0);
        paint_card(&mut buf, top, theme);
    }

    print!("{}", to_ansi(&buf));
    println!(
        "\x1b[0m{} theme(s). Each card: title bar · primary button · selected row · status · raw swatches.",
        themes.len()
    );
}

/// Paint one theme's card: a styled title/controls line, a status line, and a
/// raw-swatch strip — everything via the palette's [`Style`] constructors and
/// colour fields, exactly as a widget caller would.
fn paint_card(buf: &mut Buffer, top: u16, theme: &Theme) {
    let p = &theme.palette;

    // Row 0 — title bar, primary button, selected list row.
    fill_row(buf, top, p.screen());
    let mut x = put(buf, 1, top, &format!(" {} ", theme.name), p.screen());
    x = put(buf, x + 1, top, " Primary ", p.button_primary());
    x = put(buf, x + 1, top, " Secondary ", p.button_secondary());
    let _ = put(buf, x + 1, top, " ▌selected row ", p.selection());

    // Row 1 — status vocabulary on a raised surface.
    fill_row(buf, top + 1, p.surface());
    let mut x = put(buf, 1, top + 1, " info ", p.info_text().patch(p.surface()));
    x = put(
        buf,
        x,
        top + 1,
        " success ",
        p.success_text().patch(p.surface()),
    );
    x = put(
        buf,
        x,
        top + 1,
        " warning ",
        p.warning_text().patch(p.surface()),
    );
    x = put(
        buf,
        x,
        top + 1,
        " danger ",
        p.danger_text().patch(p.surface()),
    );
    x = put(
        buf,
        x + 1,
        top + 1,
        "link",
        p.link_style().patch(p.surface()),
    );
    let _ = put(
        buf,
        x + 2,
        top + 1,
        "dim caption",
        p.dim_text().patch(p.surface()),
    );

    // Row 2 — raw palette swatches (two cells each).
    let swatches: [(&str, Color); 14] = [
        ("bg", p.background),
        ("fg", p.foreground),
        ("border", p.border),
        ("primary", p.primary),
        ("accent", p.accent),
        ("ring", p.ring),
        ("red", p.red),
        ("green", p.green),
        ("yellow", p.yellow),
        ("blue", p.blue),
        ("magenta", p.magenta),
        ("cyan", p.cyan),
        ("scroll", p.scrollbar_thumb),
        ("sel", p.list_active),
    ];
    fill_row(buf, top + 2, p.screen());
    let mut x = 1u16;
    for (_, c) in swatches {
        let _ = put(buf, x, top + 2, "  ", Style::new().bg(c));
        x += 2;
    }
    let _ = put(
        buf,
        x + 1,
        top + 2,
        &format!(
            "{}  ({})",
            if p.is_dark() { "dark" } else { "light" },
            theme.set_name
        ),
        p.dim_text(),
    );
}

/// Write `text` at `(x, top)` with `style`; returns the next free column.
fn put(buf: &mut Buffer, x: u16, top: u16, text: &str, style: Style) -> u16 {
    let end = buf.set_str(Position { x, y: top }, text, style);
    end.x
}

/// Paint the whole card width with `style` so a theme's background reaches the
/// edges (cards are full-bleed, like a real screen).
fn fill_row(buf: &mut Buffer, y: u16, style: Style) {
    buf.set_style(
        Rect {
            x: 0,
            y,
            width: WIDTH,
            height: 1,
        },
        style,
    );
}

/// Serialise the buffer to 24-bit ANSI. Only the colours a palette produces
/// (`Color::Rgb`) and the modifiers these cards use are emitted; anything else
/// falls back to the terminal default.
fn to_ansi(buf: &Buffer) -> String {
    let area = buf.area();
    let mut out = String::new();
    for y in 0..area.height {
        for x in 0..area.width {
            let cell = buf.get(Position { x, y }).expect("cell in area");
            out.push_str("\x1b[0m");
            if cell.modifier.contains(Modifier::BOLD) {
                out.push_str("\x1b[1m");
            }
            if cell.modifier.contains(Modifier::UNDERLINED) {
                out.push_str("\x1b[4m");
            }
            if let Color::Rgb(r, g, b) = cell.fg {
                out.push_str(&format!("\x1b[38;2;{r};{g};{b}m"));
            }
            if let Color::Rgb(r, g, b) = cell.bg {
                out.push_str(&format!("\x1b[48;2;{r};{g};{b}m"));
            }
            let s = cell.symbol;
            out.push(if s == '\0' { ' ' } else { s });
        }
        out.push_str("\x1b[0m\n");
    }
    out
}
