//! `pie` Mermaid diagram renderer.
//!
//! A browser draws a pie chart as a circle of coloured wedges. A character
//! grid has neither sub-pixel arcs nor a colour key the eye can map back to a
//! legend, so this renders the *clean terminal representation of a pie*: the
//! title, then one legend row per slice ordered exactly as Mermaid orders them
//! (value descending), each row a horizontal block bar whose length is
//! proportional to that slice's share of the whole, with the raw value and the
//! rounded percentage in aligned columns, and a final total. The bar carries
//! the same information the wedge angle would — area is length here — so the
//! relative sizes read at a glance without a colour lookup.
//!
//! Grammar handled (a lenient subset of Mermaid's):
//!
//! ```text
//! pie showData
//!     title Key metrics
//!     "Hits"   : 386
//!     "Misses" : 85
//! ```
//!
//! `title …` and `showData` are optional; `showData` is implied here because
//! the values are always shown. A slice value may be an integer or a float.
//! Lines that do not parse are skipped; a `pie` with no slices degrades to the
//! shared honest placeholder. Percentages are computed in fixed-point tenths
//! and rounded half-up so a snapshot is deterministic.

use rstui_core::{Buffer, Rect, Style};

use super::MermaidTheme;
use super::draw::{BoxStyle, Surface};

/// One parsed pie slice: its label and its non-negative value scaled to
/// tenths (so `42.5` is stored as `425`) for exact integer percentage math.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Slice {
    /// The quoted label text.
    label: String,
    /// The value × 10, rounded half-up from the source number.
    value_tenths: u64,
}

/// A parsed `pie` chart: optional title and the slices in source order.
#[derive(Debug, Default, PartialEq, Eq)]
struct Pie {
    /// The `title …` text, if any.
    title: Option<String>,
    /// The slices as written; sorting happens at render time.
    slices: Vec<Slice>,
}

/// Strips Mermaid preamble noise from a raw source: drops a leading
/// `--- … ---` frontmatter block, `%%{…}%%` directives / `%%` comments, blank
/// lines, and the `pie` header (with any trailing `showData`/`title`).
fn clean_lines(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_front = false;
    let mut seen_header = false;
    for raw in src.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let line = match line.find("%%") {
            Some(i) => &line[..i],
            None => line,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !seen_header && line == "---" {
            in_front = !in_front;
            continue;
        }
        if in_front {
            continue;
        }
        if !seen_header {
            // The first significant line is the `pie` header; everything
            // after the keyword on it (`showData`, `title …`) is consumed
            // here so a one-line `pie title T` still works.
            seen_header = true;
            let rest = line.strip_prefix("pie").unwrap_or("").trim();
            let rest = rest.strip_prefix("showData").unwrap_or(rest).trim();
            if let Some(t) = rest.strip_prefix("title") {
                out.push(format!("title {}", t.trim()));
            }
            continue;
        }
        out.push(line.to_string());
    }
    out
}

/// Parses a `"Label" : number` pair (value int or float). Returns `None` for
/// any line that is not a well-formed slice.
fn parse_slice(line: &str) -> Option<Slice> {
    let rest = line.strip_prefix('"')?;
    let end = rest.find('"')?;
    let label = rest[..end].to_string();
    let after = rest[end + 1..].trim_start();
    let after = after.strip_prefix(':')?.trim();
    let value_tenths = parse_tenths(after)?;
    Some(Slice {
        label,
        value_tenths,
    })
}

/// Parses a non-negative decimal number into tenths, rounding half-up at the
/// first fractional digit (`42.56` → `426`). Rejects signs and non-numerics.
fn parse_tenths(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() || s.starts_with('-') || s.starts_with('+') {
        return None;
    }
    let (int_part, frac_part) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s, ""),
    };
    let int_v: u64 = if int_part.is_empty() {
        0
    } else {
        int_part.parse().ok()?
    };
    if !frac_part.is_empty() && !frac_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let tenths = frac_part
        .chars()
        .next()
        .and_then(|c| c.to_digit(10))
        .unwrap_or(0) as u64;
    // Round the tenths up if the hundredths digit is >= 5.
    let round_up = frac_part
        .chars()
        .nth(1)
        .and_then(|c| c.to_digit(10))
        .map(|d| d >= 5)
        .unwrap_or(false);
    Some(int_v.checked_mul(10)?.checked_add(tenths)? + u64::from(round_up))
}

/// Parses a whole `pie` source into a [`Pie`], skipping unparseable lines.
fn parse(src: &str) -> Pie {
    let mut pie = Pie::default();
    for line in clean_lines(src) {
        if let Some(t) = line.strip_prefix("title ") {
            pie.title = Some(t.trim().to_string());
            continue;
        }
        if let Some(s) = parse_slice(&line) {
            pie.slices.push(s);
        }
    }
    pie
}

/// The 8-step horizontal block ramp (`▏…█`), the house bar glyphs shared with
/// [`crate::BarChart`], used to draw a slice's proportional length.
const RAMP: [char; 8] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// Formats a tenths value back to its shortest decimal string: an integer
/// when the fraction is zero, else `n.d`.
fn fmt_tenths(t: u64) -> String {
    if t % 10 == 0 {
        format!("{}", t / 10)
    } else {
        format!("{}.{}", t / 10, t % 10)
    }
}

/// Renders a `pie` Mermaid diagram from `src` into `area`.
///
/// Layout: an outer rounded frame; a centred title row; then one legend row
/// per slice (Mermaid order: value descending, ties keep source order) of the
/// form `▇ Label …… 42.5 (35.0%)`, the bar column width-proportional to the
/// share; finally a `total N` line. Percentages are integer tenths summing
/// deterministically. Empty or unparseable input draws the shared placeholder.
pub(crate) fn render(src: &str, area: Rect, buf: &mut Buffer, base: Style, theme: &MermaidTheme) {
    let mut pie = parse(src);
    if pie.slices.is_empty() {
        super::diagram_placeholder("pie", "no slices", area, buf, base, theme);
        return;
    }
    // Mermaid sorts wedges by value descending; a stable sort by the negated
    // key keeps the source order for equal values, which keeps the snapshot
    // deterministic. (`u64` can't be negated, so compare on `Reverse`.)
    pie.slices
        .sort_by_key(|s| std::cmp::Reverse(s.value_tenths));

    let total: u64 = pie.slices.iter().map(|s| s.value_tenths).sum();

    let frame = base.patch(theme.node_border);
    let title_st = base.patch(theme.node_label);
    let label_st = base.patch(theme.cluster);
    let bar_st = base.patch(theme.edge);
    let value_st = base.patch(theme.edge_label);

    // Column widths: the longest label, and the longest formatted value.
    let label_w = pie
        .slices
        .iter()
        .map(|s| s.label.chars().count())
        .max()
        .unwrap_or(0)
        .max(5) as i32;
    let value_w = pie
        .slices
        .iter()
        .map(|s| fmt_tenths(s.value_tenths).chars().count())
        .max()
        .unwrap_or(1) as i32;

    // Percentage strings are always like `100.0%` → at most 6 chars.
    let pct_w = 7;
    // The proportional bar gets whatever horizontal room is left, clamped to
    // a sane band so a wide terminal does not draw a kilometre of blocks.
    let bar_max = ((area.width as i32) - label_w - value_w - pct_w - 8)
        .clamp(0, 24)
        .max(0);

    let body_rows = pie.slices.len() as i32;
    let title_rows = i32::from(pie.title.is_some());
    // title + slices + total + 2 frame rows (+1 blank under the title).
    let need_h = body_rows + title_rows + 1 + 2 + title_rows;
    let content_w = label_w + 2 + bar_max + 1 + value_w + 1 + pct_w + 4;
    // Clamp against the area, but never let the lower bound exceed the upper
    // (a sub-8-cell area must still clip cleanly, not panic).
    let w = content_w
        .min(area.width as i32)
        .max((area.width as i32).min(8));
    let h = need_h
        .min(area.height as i32)
        .max((area.height as i32).min(3));

    let mut s = Surface::new(w, h);
    s.rect(0, 0, w, h, BoxStyle::Round, frame);

    let inner_left = 2;
    let inner_w = (w - 4).max(0);
    let mut y = 1;

    if let Some(t) = &pie.title {
        s.text_centered(inner_left, y, inner_w, t, title_st);
        y += 2;
    }

    // A non-zero divisor: when every slice is zero `total` is 0, but then
    // every numerator is 0 too, so dividing by 1 still yields 0% correctly.
    let denom = total.max(1);
    for slice in &pie.slices {
        if y >= h - 2 {
            break;
        }
        // share% × 10, rounded half-up: (value·1000 + total/2) / total.
        let pct_tenths = (slice.value_tenths * 1000 + total / 2) / denom;
        // Bar length is the same proportion, in eighths, of the bar column.
        let filled_eighths = (slice.value_tenths * (bar_max as u64) * 8 + total / 2) / denom;
        let full = (filled_eighths / 8) as i32;
        let rem = (filled_eighths % 8) as usize;

        let mut x = inner_left;
        s.text_clipped(x, y, &slice.label, label_w, label_st);
        x += label_w + 1;

        // The proportional block bar (the wedge "area").
        for i in 0..full.min(bar_max) {
            s.set(x + i, y, '█', bar_st);
        }
        if full < bar_max && rem > 0 {
            s.set(x + full, y, RAMP[rem - 1], bar_st);
        }
        // A leading filled cell so a zero/tiny slice still shows a marker.
        if full == 0 && rem == 0 {
            s.set(x, y, '▏', bar_st);
        }
        x += bar_max + 1;

        let vs = fmt_tenths(slice.value_tenths);
        let pad = value_w - vs.chars().count() as i32;
        s.text(x + pad.max(0), y, &vs, value_st);
        x += value_w + 1;

        let pct = format!("({}.{:01}%)", pct_tenths / 10, pct_tenths % 10);
        s.text(x, y, &pct, value_st);

        y += 1;
    }

    if y < h - 1 {
        let line = format!("total {}", fmt_tenths(total));
        s.text(inner_left, y, &line, label_st);
    }

    s.blit(area, buf, base);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mermaid;
    use rstui_core::{Position, Widget};

    /// Renders `widget` into a fresh `w`×`h` buffer; returns its glyphs as one
    /// newline-terminated line per row (the shared mermaid snapshot idiom).
    fn lines(src: &str, w: u16, h: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        Mermaid::new(src).render(buf.area(), &mut buf);
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    // --- parser ------------------------------------------------------------

    #[test]
    fn parses_title_and_slices_int_and_float() {
        let p = parse("pie title Pets\n\"Dogs\" : 30\n\"Cats\" : 12.5\n");
        assert_eq!(p.title.as_deref(), Some("Pets"));
        assert_eq!(p.slices.len(), 2);
        assert_eq!(p.slices[0].label, "Dogs");
        assert_eq!(p.slices[0].value_tenths, 300);
        assert_eq!(p.slices[1].value_tenths, 125);
    }

    #[test]
    fn show_data_and_inline_title_on_header() {
        let p = parse("pie showData title Sales\n\"A\":1\n");
        assert_eq!(p.title.as_deref(), Some("Sales"));
        assert_eq!(p.slices.len(), 1);
    }

    #[test]
    fn frontmatter_and_comments_are_skipped() {
        let src =
            "---\ntitle: ignored\n---\npie\n%% a comment\n\"X\" : 5\n%%{init: {}}%%\n\"Y\" : 5\n";
        let p = parse(src);
        assert_eq!(p.title, None);
        assert_eq!(p.slices.len(), 2);
    }

    #[test]
    fn bad_lines_are_skipped_not_panic() {
        let p = parse("pie\nnot a slice\n\"Ok\" : 4\n\"NoColon\" 9\n\"Neg\" : -3\n");
        assert_eq!(p.slices.len(), 1);
        assert_eq!(p.slices[0].label, "Ok");
    }

    #[test]
    fn fractional_value_rounds_half_up_to_tenths() {
        assert_eq!(parse_tenths("42.56"), Some(426));
        assert_eq!(parse_tenths("42.54"), Some(425));
        assert_eq!(parse_tenths("7"), Some(70));
        assert_eq!(parse_tenths(".5"), Some(5));
        assert_eq!(parse_tenths("x"), None);
    }

    // --- render snapshots --------------------------------------------------

    #[test]
    fn empty_pie_is_placeholder() {
        let out = lines("pie\n", 40, 5);
        assert!(out.contains("mermaid · pie: no slices"), "{out}");
    }

    #[test]
    fn no_header_at_all_is_placeholder() {
        // Not even a `pie` keyword → routed to the legacy flowchart path.
        let out = lines("\"A\" : 1\n", 40, 3);
        assert!(out.contains("mermaid") || out.contains("[mermaid"), "{out}");
    }

    #[test]
    fn typical_pie_full_render_snapshot() {
        let out = lines("pie title Votes\n\"Yes\" : 60\n\"No\" : 40\n", 38, 7);
        let expected = [
            "╭────────────────────────────────────╮",
            "│               Votes                │",
            "│                                    │",
            "│ Yes   █████████▋       60 (60.0%)  │",
            "│ No    ██████▍          40 (40.0%)  │",
            "│ total 100                          │",
            "╰────────────────────────────────────╯",
            "",
        ]
        .join("\n");
        assert_eq!(out, expected);
    }

    #[test]
    fn slices_are_sorted_value_descending() {
        let out = lines("pie\n\"Small\" : 10\n\"Big\" : 90\n", 36, 6);
        let big = out.find("Big").unwrap();
        let small = out.find("Small").unwrap();
        assert!(big < small, "Big should sort before Small:\n{out}");
    }

    #[test]
    fn float_values_and_percentages_render() {
        let out = lines("pie\n\"A\" : 1.5\n\"B\" : 0.5\n", 34, 6);
        assert!(out.contains("1.5"), "{out}");
        assert!(out.contains("0.5"), "{out}");
        assert!(out.contains("75.0%"), "{out}");
        assert!(out.contains("25.0%"), "{out}");
        assert!(out.contains("total 2"), "{out}");
    }

    #[test]
    fn tiny_area_does_not_panic_and_clips() {
        // Far too small for the framed layout — must still be a clean clip.
        let _ = lines("pie\n\"A\" : 1\n", 6, 2);
        let _ = lines("pie title T\n\"A\" : 1\n\"B\" : 2\n", 3, 1);
    }
}
