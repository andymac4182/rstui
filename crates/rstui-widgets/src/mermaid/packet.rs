//! `packet-beta` Mermaid diagram renderer.
//!
//! Mermaid's *packet* diagram draws an RFC-style network packet: a bit-index
//! ruler across the top and a row of fields below, each field spanning the bit
//! columns it occupies and wrapping onto further rows when it crosses the
//! row's bit width. The dispatcher in [`super`] routes a `packet-beta` (or
//! `packet`) source here; this module owns the hand-written line parser, the
//! deterministic integer column layout, and the [`Surface`] render the shared
//! blit centres into `area`.
//!
//! # Supported subset
//!
//! Each body line declares one field by its bit range and a quoted label:
//!
//! * `0-15: "Source Port"` — an inclusive bit range.
//! * `32: "Flag"` — a single bit.
//! * `+16: "Field"` — *new syntax*: 16 bits starting at the running cursor.
//! * `+1: "Bit"` — a single bit from the cursor.
//!
//! The bits-per-row defaults to Mermaid's `32` and can be overridden by a
//! `bits N` / `packetBits N` directive.
//!
//! # Leniency
//!
//! A malformed line is skipped, never a panic. A source with no fields falls
//! back to [`super::diagram_placeholder`]. A field that runs past the row
//! width is split across rows; everything is integer arithmetic so the image
//! is a stable snapshot.

use rstui_core::{Buffer, Rect, Style};

use super::MermaidTheme;
use super::draw::{BoxStyle, Surface};

/// One parsed packet field: an inclusive bit range and its label.
#[derive(Debug, Clone)]
struct Field {
    /// First bit (inclusive).
    start: i32,
    /// Last bit (inclusive).
    end: i32,
    /// The field label.
    label: String,
}

/// The parsed packet: its fields and the bits drawn per row.
#[derive(Debug)]
struct Packet {
    /// Fields in source order.
    fields: Vec<Field>,
    /// Bits per rendered row (Mermaid default `32`).
    bits_per_row: i32,
}

/// Strips a Mermaid preamble from `src`, yielding the significant body lines
/// (trimmed, `\r` removed, no frontmatter / `%%` comments / header).
fn body_lines(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_front = false;
    let mut seen_header = false;
    for raw in src.split('\n') {
        let line = raw.trim_end_matches('\r').trim();
        if line.is_empty() {
            continue;
        }
        if line == "---" {
            in_front = !in_front;
            continue;
        }
        if in_front {
            continue;
        }
        if line.starts_with("%%") {
            continue;
        }
        if !seen_header {
            seen_header = true;
            let word: String = line.chars().take_while(|c| !c.is_whitespace()).collect();
            if word == "packet-beta" || word == "packet" {
                continue;
            }
        }
        out.push(line.to_string());
    }
    out
}

/// Removes one layer of surrounding single/double quotes (and trims).
fn unquote(s: &str) -> String {
    let t = s.trim();
    let t = t
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .or_else(|| t.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')))
        .unwrap_or(t);
    t.trim().to_string()
}

/// Parses `packet-beta` body lines into a [`Packet`], tracking a running bit
/// cursor for the `+N` relative syntax.
fn parse(src: &str) -> Packet {
    let mut p = Packet {
        fields: Vec::new(),
        bits_per_row: 32,
    };
    let mut cursor = 0i32;
    for line in body_lines(src) {
        // `bits N` / `packetBits N` directive.
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower
            .strip_prefix("bits")
            .or_else(|| lower.strip_prefix("packetbits"))
        {
            if let Ok(n) = rest.trim().parse::<i32>() {
                if n >= 1 {
                    p.bits_per_row = n;
                }
                continue;
            }
        }
        let Some((spec, rest)) = line.split_once(':') else {
            continue;
        };
        let spec = spec.trim();
        let label = unquote(rest);
        if label.is_empty() {
            continue;
        }
        if let Some(rel) = spec.strip_prefix('+') {
            // Relative: `+N` is N bits from the cursor.
            let Ok(n) = rel.trim().parse::<i32>() else {
                continue;
            };
            if n < 1 {
                continue;
            }
            let start = cursor;
            let end = cursor + n - 1;
            cursor = end + 1;
            p.fields.push(Field { start, end, label });
        } else if let Some((a, b)) = spec.split_once('-') {
            // Absolute range `A-B`.
            let (Ok(a), Ok(b)) = (a.trim().parse::<i32>(), b.trim().parse::<i32>()) else {
                continue;
            };
            let (start, end) = if a <= b { (a, b) } else { (b, a) };
            cursor = end + 1;
            p.fields.push(Field { start, end, label });
        } else if let Ok(bit) = spec.parse::<i32>() {
            // Single absolute bit `N`.
            cursor = bit + 1;
            p.fields.push(Field {
                start: bit,
                end: bit,
                label,
            });
        }
    }
    p
}

/// Width in surface cells of one bit column (two glyphs reads as a clear,
/// classic packet grid).
const BIT_W: i32 = 2;
/// Rows a single packet line occupies: a top rule row, a label row, a bottom
/// rule row.
const ROW_H: i32 = 3;

/// Renders a `packet-beta` Mermaid diagram from `src` into `area`.
pub(crate) fn render(src: &str, area: Rect, buf: &mut Buffer, base: Style, theme: &MermaidTheme) {
    if area.is_empty() {
        return;
    }
    let p = parse(src);
    if p.fields.is_empty() {
        super::diagram_placeholder("packet", "no fields", area, buf, base, theme);
        return;
    }
    let bpr = p.bits_per_row.max(1);

    // Split every field at row boundaries so a field that crosses the row
    // width becomes one box per row it touches.
    struct Seg {
        row: i32,
        c0: i32,
        c1: i32,
        label: String,
        first: bool,
    }
    let mut segs: Vec<Seg> = Vec::new();
    let mut max_row = 0i32;
    for f in &p.fields {
        let mut bit = f.start.max(0);
        let last = f.end.max(bit);
        let mut first = true;
        while bit <= last {
            let row = bit / bpr;
            let row_end = (row + 1) * bpr - 1;
            let c1 = last.min(row_end);
            segs.push(Seg {
                row,
                c0: bit % bpr,
                c1: c1 % bpr,
                label: f.label.clone(),
                first,
            });
            max_row = max_row.max(row);
            first = false;
            bit = c1 + 1;
        }
    }

    let grid_w = bpr * BIT_W + 1;
    // One ruler row at the top, then ROW_H per packet row (sharing borders is
    // avoided for legibility: each row is a full box band).
    let ruler_h = 2;
    let w = (grid_w + 6).max(2);
    let h = (ruler_h + (max_row + 1) * ROW_H).max(2);
    let mut s = Surface::new(w, h);

    let border = base.patch(theme.node_border);
    let label = base.patch(theme.node_label);
    let edge = base.patch(theme.edge);
    let off = base.patch(theme.edge_label);

    // Left margin holds the byte/bit offset of each row.
    let ox = 6;

    // Bit-index ruler: the baseline first, then a tick every 8 bits (plus the
    // row-width endpoint) painted *over* it so the ticks survive.
    s.hline(ox, 1, grid_w, '─', edge);
    for b in (0..=bpr).step_by(8) {
        let x = ox + b * BIT_W;
        let txt = b.to_string();
        s.text(x - (txt.chars().count() as i32 - 1).max(0), 0, &txt, edge);
        s.set(x, 1, '┬', edge);
    }
    if bpr % 8 != 0 {
        let x = ox + bpr * BIT_W;
        let txt = bpr.to_string();
        s.text(x - (txt.chars().count() as i32 - 1), 0, &txt, edge);
        s.set(x, 1, '┬', edge);
    }

    // Each packet row: its field boxes and a left-margin offset label.
    for row in 0..=max_row {
        let y = ruler_h + row * ROW_H;
        let bit0 = row * bpr;
        // Byte/bit offset of the row start.
        let lbl = if bit0 % 8 == 0 {
            format!("B{}", bit0 / 8)
        } else {
            format!("b{bit0}")
        };
        s.text(0, y + 1, &lbl, off);

        for seg in segs.iter().filter(|s| s.row == row) {
            let x = ox + seg.c0 * BIT_W;
            let bw = (seg.c1 - seg.c0 + 1) * BIT_W + 1;
            let kind = if seg.first {
                BoxStyle::Square
            } else {
                // A continuation of a wrapped field: a softer border.
                BoxStyle::Round
            };
            s.labeled_box(x, y, bw, ROW_H, kind, &seg.label, border, label);
            // The field's leading bit index on the box's top edge.
            let idx = (seg.c0 + bit0).to_string();
            s.text_clipped(x + 1, y, &idx, (bw - 2).max(0), edge);
        }
    }

    s.blit(area, buf, base);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mermaid;
    use rstui_core::{Position, Widget};

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

    // --- Parser -----------------------------------------------------------

    #[test]
    fn parses_absolute_ranges() {
        let p = parse("packet-beta\n0-15: \"Source Port\"\n16-31: \"Dest Port\"");
        assert_eq!(p.fields.len(), 2);
        assert_eq!((p.fields[0].start, p.fields[0].end), (0, 15));
        assert_eq!(p.fields[0].label, "Source Port");
        assert_eq!((p.fields[1].start, p.fields[1].end), (16, 31));
        assert_eq!(p.bits_per_row, 32);
    }

    #[test]
    fn parses_single_bit() {
        let p = parse("packet-beta\n32: \"Flag\"");
        assert_eq!(p.fields.len(), 1);
        assert_eq!((p.fields[0].start, p.fields[0].end), (32, 32));
        assert_eq!(p.fields[0].label, "Flag");
    }

    #[test]
    fn parses_relative_plus_syntax() {
        let p = parse("packet\n+16: \"A\"\n+16: \"B\"\n+1: \"C\"");
        assert_eq!(p.fields.len(), 3);
        assert_eq!((p.fields[0].start, p.fields[0].end), (0, 15));
        assert_eq!((p.fields[1].start, p.fields[1].end), (16, 31));
        assert_eq!((p.fields[2].start, p.fields[2].end), (32, 32));
    }

    #[test]
    fn relative_continues_after_absolute() {
        let p = parse("packet-beta\n0-7: \"X\"\n+8: \"Y\"");
        assert_eq!((p.fields[1].start, p.fields[1].end), (8, 15));
    }

    #[test]
    fn bits_directive_overrides_default() {
        let p = parse("packet-beta\nbits 16\n0-15: \"Word\"");
        assert_eq!(p.bits_per_row, 16);
        assert_eq!(p.fields.len(), 1);
    }

    #[test]
    fn reversed_range_is_normalised() {
        let p = parse("packet-beta\n31-16: \"R\"");
        assert_eq!((p.fields[0].start, p.fields[0].end), (16, 31));
    }

    #[test]
    fn lenient_skips_bad_lines_no_panic() {
        let p = parse("packet-beta\ngarbage\n0-3: \"Ok\"\nxx-yy: \"bad\"\n%% c");
        assert_eq!(p.fields.len(), 1);
        assert_eq!(p.fields[0].label, "Ok");
    }

    #[test]
    fn skips_frontmatter_and_comments() {
        let p = parse("---\ntitle: T\n---\npacket-beta\n%% hi\n0-7: \"H\"");
        assert_eq!(p.fields.len(), 1);
        assert_eq!(p.fields[0].label, "H");
    }

    // --- Render snapshots -------------------------------------------------

    #[test]
    fn empty_source_renders_placeholder() {
        let out = lines(Mermaid::new("packet-beta"), 40, 3);
        assert!(out.contains("mermaid"), "{out}");
        assert!(out.contains("packet"));
    }

    #[test]
    fn no_fields_renders_placeholder() {
        let out = lines(Mermaid::new("packet-beta\nbits 16"), 40, 3);
        assert!(out.contains("no fields"), "{out}");
    }

    #[test]
    fn single_field_snapshot_has_box_and_label() {
        let out = lines(Mermaid::new("packet-beta\nbits 8\n0-7: \"Byte\""), 30, 6);
        assert!(out.contains("Byte"), "{out}");
        assert!(out.contains('┌') && out.contains('┘'), "{out}");
        // Ruler tick at bit 0.
        assert!(out.contains('┬'), "{out}");
    }

    #[test]
    fn two_fields_share_a_row() {
        let out = lines(
            Mermaid::new("packet-beta\nbits 8\n0-3: \"Hi\"\n4-7: \"Lo\""),
            30,
            6,
        );
        assert!(out.contains("Hi") && out.contains("Lo"), "{out}");
        let top = out.lines().find(|r| r.contains('┌')).unwrap();
        assert_eq!(top.matches('┌').count(), 2, "{out}");
    }

    #[test]
    fn field_wrapping_rows_splits_into_two_boxes() {
        // 0-15 with 8 bits/row spans two rows.
        let out = lines(Mermaid::new("packet-beta\nbits 8\n0-15: \"Wide\""), 30, 9);
        // The continuation row uses the rounded border.
        assert!(out.contains('╭') || out.contains('╰'), "{out}");
        assert!(out.matches("Wide").count() >= 2, "{out}");
    }

    #[test]
    fn offset_labels_are_drawn() {
        let out = lines(
            Mermaid::new("packet-beta\nbits 8\n0-7: \"A\"\n8-15: \"B\""),
            30,
            9,
        );
        assert!(out.contains("B0"), "byte0 offset missing:\n{out}");
        assert!(out.contains("B1"), "byte1 offset missing:\n{out}");
    }

    #[test]
    fn tiny_area_does_not_panic_and_clips() {
        let out = lines(Mermaid::new("packet-beta\n0-31: \"X\""), 5, 2);
        assert_eq!(out.lines().count(), 2);
    }

    #[test]
    fn one_by_one_area_is_safe() {
        let _ = lines(Mermaid::new("packet-beta\n0-7: \"X\""), 1, 1);
    }
}
