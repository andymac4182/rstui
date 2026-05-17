//! `kanban` Mermaid diagram renderer.
//!
//! Mermaid's *kanban* board declares columns at indent 0 and cards nested
//! (indented) beneath them; each card may carry a metadata block. The
//! dispatcher in [`super`] routes a `kanban` source here; this module owns the
//! hand-written *indentation-significant* line parser, a deterministic
//! equal-split column layout, and the [`Surface`] render the shared blit
//! centres into `area`.
//!
//! # Supported subset
//!
//! * `colId[Column Title]` (or a bare `Column Title`) at indent 0 — a lane.
//! * An indented `taskId[Card text]` — a card stacked in the lane above it.
//! * A card's metadata, either inline after the card
//!   (`taskId[Text]@{ priority: 'High', assigned: 'A' }`) or on the next, more
//!   deeply indented line (`@{ ticket: 'T-1', assigned: 'B' }`). Recognised
//!   keys: `priority`, `assigned`/`assignee`, `ticket`.
//!
//! # Leniency
//!
//! Unparseable lines are skipped, never a panic. A board with no columns falls
//! back to [`super::diagram_placeholder`]. Column widths are an integer split
//! of `area` so the rendered image is a stable snapshot.

use rstui_core::{Buffer, Rect, Style};

use super::MermaidTheme;
use super::draw::{BoxStyle, Surface};

/// One card in a lane: its text and an optional compact meta line.
#[derive(Debug, Clone, Default)]
struct Card {
    /// The card's display text.
    text: String,
    /// `priority` metadata, if any.
    priority: Option<String>,
    /// `assigned` / `assignee` metadata, if any.
    assignee: Option<String>,
    /// `ticket` metadata, if any.
    ticket: Option<String>,
}

impl Card {
    /// A one-line compact meta string (`!High @alice #T-1`) or empty.
    fn meta_line(&self) -> String {
        let mut parts = Vec::new();
        if let Some(p) = &self.priority {
            parts.push(format!("!{p}"));
        }
        if let Some(a) = &self.assignee {
            parts.push(format!("@{a}"));
        }
        if let Some(t) = &self.ticket {
            parts.push(format!("#{t}"));
        }
        parts.join(" ")
    }
}

/// One board lane: a title and its stacked cards.
#[derive(Debug, Clone, Default)]
struct Column {
    /// The lane title.
    title: String,
    /// Cards top-to-bottom in source order.
    cards: Vec<Card>,
}

/// Strips a Mermaid preamble from `src`, returning `(indent, text)` for each
/// significant line. **Indentation is preserved** (leading-space count) since
/// kanban nesting is meaningful; a tab counts as one space-equivalent of 4.
fn body_lines(src: &str) -> Vec<(i32, String)> {
    let mut out = Vec::new();
    let mut in_front = false;
    let mut seen_header = false;
    for raw in src.split('\n') {
        let line = raw.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "---" {
            in_front = !in_front;
            continue;
        }
        if in_front {
            continue;
        }
        if trimmed.starts_with("%%") {
            continue;
        }
        if !seen_header {
            seen_header = true;
            let word: String = trimmed.chars().take_while(|c| !c.is_whitespace()).collect();
            if word == "kanban" {
                continue;
            }
        }
        // Leading-whitespace count (tab == 4) defines the nesting level.
        let mut indent = 0i32;
        for c in line.chars() {
            match c {
                ' ' => indent += 1,
                '\t' => indent += 4,
                _ => break,
            }
        }
        out.push((indent, trimmed.to_string()));
    }
    out
}

/// Extracts `id[Label]` → `(id, label)`, or treats the whole token as a bare
/// label (id == label). A trailing `@{ ... }` is *not* consumed here.
fn split_id_label(s: &str) -> (String, String) {
    let s = s.trim();
    if let Some(open) = s.find('[') {
        if let Some(close) = s.rfind(']') {
            if close > open {
                let id = s[..open].trim().to_string();
                let label = s[open + 1..close].trim().to_string();
                let id = if id.is_empty() { label.clone() } else { id };
                return (id, label);
            }
        }
    }
    (s.to_string(), s.to_string())
}

/// Parses a `@{ key: 'val', ... }` metadata blob into a [`Card`]'s fields.
fn apply_meta(card: &mut Card, blob: &str) {
    let inner = blob
        .trim()
        .trim_start_matches("@{")
        .trim_end_matches('}')
        .trim();
    for pair in inner.split(',') {
        let Some((k, v)) = pair.split_once(':') else {
            continue;
        };
        let key = k.trim().to_ascii_lowercase();
        let val = v.trim().trim_matches(['\'', '"']).trim().to_string();
        if val.is_empty() {
            continue;
        }
        match key.as_str() {
            "priority" => card.priority = Some(val),
            "assigned" | "assignee" => card.assignee = Some(val),
            "ticket" => card.ticket = Some(val),
            _ => {}
        }
    }
}

/// Splits a card line into `(decl, inline_meta)` at a top-level `@{`.
fn split_inline_meta(line: &str) -> (&str, Option<&str>) {
    match line.find("@{") {
        Some(i) => (line[..i].trim_end(), Some(&line[i..])),
        None => (line, None),
    }
}

/// Parses `kanban` body lines into ordered [`Column`]s.
fn parse(src: &str) -> Vec<Column> {
    let lines = body_lines(src);
    let mut cols: Vec<Column> = Vec::new();
    // The indent of column declarations: set from the first column line, since
    // a board may indent its columns uniformly (e.g. all at 2). A line at this
    // indent is a new lane; anything deeper is a card; a `@{...}` is metadata.
    let mut column_indent: Option<i32> = None;
    for (indent, text) in lines {
        if text.starts_with("@{") {
            // A standalone metadata line for the most recent card.
            if let Some(card) = cols.last_mut().and_then(|c| c.cards.last_mut()) {
                apply_meta(card, &text);
            }
            continue;
        }
        let is_column = match column_indent {
            None => true,             // the first declaration is a lane
            Some(ci) => indent <= ci, // at-or-shallower than the lane indent
        };
        if is_column {
            column_indent.get_or_insert(indent);
            let (_, title) = split_id_label(&text);
            cols.push(Column {
                title,
                cards: Vec::new(),
            });
            continue;
        }
        // Deeper than the column indent → a card in the last lane.
        let Some(col) = cols.last_mut() else {
            continue;
        };
        let (decl, inline) = split_inline_meta(&text);
        let (_, label) = split_id_label(decl);
        let mut card = Card {
            text: label,
            ..Card::default()
        };
        if let Some(m) = inline {
            apply_meta(&mut card, m);
        }
        col.cards.push(card);
    }
    cols
}

/// Minimum lane width that can still show a border + a little text.
const MIN_COL_W: i32 = 10;
/// Card box height: a border row, a text row, a meta row, a border row.
const CARD_H: i32 = 4;
/// Vertical gap between stacked cards.
const CARD_GAP: i32 = 1;

/// Renders a `kanban` Mermaid diagram from `src` into `area`.
pub(crate) fn render(src: &str, area: Rect, buf: &mut Buffer, base: Style, theme: &MermaidTheme) {
    if area.is_empty() {
        return;
    }
    let cols = parse(src);
    if cols.is_empty() {
        super::diagram_placeholder("kanban", "no columns", area, buf, base, theme);
        return;
    }

    let n = cols.len() as i32;
    // Lay the lanes out across the available width, never below MIN_COL_W.
    let avail = area.width as i32;
    let col_w = (avail / n).max(MIN_COL_W);
    let board_w = col_w * n;

    let max_cards = cols.iter().map(|c| c.cards.len()).max().unwrap_or(0) as i32;
    // Lane = title row + border + stacked cards + padding.
    let lane_h = 3 + max_cards * (CARD_H + CARD_GAP) + 1;
    let h = lane_h.max(area.height as i32).max(2);
    let w = board_w.max(2);
    let mut s = Surface::new(w, h);

    let border = base.patch(theme.node_border);
    let label = base.patch(theme.node_label);
    let cluster = base.patch(theme.cluster);
    let meta_st = base.patch(theme.edge_label);

    for (i, col) in cols.iter().enumerate() {
        let x = i as i32 * col_w;
        // The lane frame, titled along the top border.
        s.rect(x, 0, col_w, lane_h, BoxStyle::Round, cluster);
        let title = format!(" {} ", col.title);
        s.text_centered(x + 1, 0, col_w - 2, &title, cluster);

        let inner_x = x + 2;
        let inner_w = (col_w - 4).max(2);
        for (k, card) in col.cards.iter().enumerate() {
            let cy = 2 + k as i32 * (CARD_H + CARD_GAP);
            s.rect(inner_x, cy, inner_w, CARD_H, BoxStyle::Square, border);
            s.text_clipped(inner_x + 1, cy + 1, &card.text, inner_w - 2, label);
            let meta = card.meta_line();
            if !meta.is_empty() {
                s.text_clipped(inner_x + 1, cy + 2, &meta, inner_w - 2, meta_st);
            }
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
    fn parses_columns_and_cards() {
        let src = "kanban\n  todo[To Do]\n    t1[Task one]\n    t2[Task two]\n  done[Done]\n    d1[Shipped]";
        let cols = parse(src);
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0].title, "To Do");
        assert_eq!(cols[0].cards.len(), 2);
        assert_eq!(cols[0].cards[0].text, "Task one");
        assert_eq!(cols[1].title, "Done");
        assert_eq!(cols[1].cards.len(), 1);
        assert_eq!(cols[1].cards[0].text, "Shipped");
    }

    #[test]
    fn indent_zero_starts_new_column() {
        let src = "kanban\ntodo[A]\n  c1[card]\ndone[B]\n  c2[card2]";
        let cols = parse(src);
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0].cards.len(), 1);
        assert_eq!(cols[1].cards.len(), 1);
    }

    #[test]
    fn bare_titles_without_brackets() {
        let src = "kanban\n  Backlog\n    item one";
        let cols = parse(src);
        assert_eq!(cols[0].title, "Backlog");
        assert_eq!(cols[0].cards[0].text, "item one");
    }

    #[test]
    fn parses_inline_metadata() {
        let src = "kanban\n  todo[Todo]\n    t1[Fix bug]@{ priority: 'High', assigned: 'Ann', ticket: 'T-9' }";
        let cols = parse(src);
        let card = &cols[0].cards[0];
        assert_eq!(card.text, "Fix bug");
        assert_eq!(card.priority.as_deref(), Some("High"));
        assert_eq!(card.assignee.as_deref(), Some("Ann"));
        assert_eq!(card.ticket.as_deref(), Some("T-9"));
        assert_eq!(card.meta_line(), "!High @Ann #T-9");
    }

    #[test]
    fn parses_standalone_metadata_line() {
        let src = "kanban\n  todo[Todo]\n    t1[Card]\n      @{ assigned: 'Bob', priority: 'Low' }";
        let cols = parse(src);
        let card = &cols[0].cards[0];
        assert_eq!(card.assignee.as_deref(), Some("Bob"));
        assert_eq!(card.priority.as_deref(), Some("Low"));
    }

    #[test]
    fn assignee_alias_is_accepted() {
        let src = "kanban\n  c[C]\n    t[T]@{ assignee: 'Zed' }";
        let cols = parse(src);
        assert_eq!(cols[0].cards[0].assignee.as_deref(), Some("Zed"));
    }

    #[test]
    fn lenient_skips_garbage_no_panic() {
        let src = "kanban\n  ok[Ok]\n    @{ bad\n    good[Good]";
        let cols = parse(src);
        assert_eq!(cols[0].title, "Ok");
        assert!(cols[0].cards.iter().any(|c| c.text == "Good"));
    }

    #[test]
    fn skips_frontmatter_and_comments() {
        let src = "---\ntitle: T\n---\nkanban\n%% note\n  todo[Todo]\n    t1[X]";
        let cols = parse(src);
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].cards[0].text, "X");
    }

    #[test]
    fn indentation_is_significant() {
        // Same tokens, different indent → column vs card.
        let nested = parse("kanban\n  Col\n    Card");
        assert_eq!(nested.len(), 1);
        assert_eq!(nested[0].cards.len(), 1);
        let two_cols = parse("kanban\nCol\nCard");
        assert_eq!(two_cols.len(), 2);
    }

    // --- Render snapshots -------------------------------------------------

    #[test]
    fn empty_source_renders_placeholder() {
        let out = lines(Mermaid::new("kanban"), 40, 3);
        assert!(out.contains("mermaid"), "{out}");
        assert!(out.contains("kanban"));
    }

    #[test]
    fn no_columns_renders_placeholder() {
        let out = lines(Mermaid::new("kanban\n%% just a comment"), 40, 3);
        assert!(out.contains("no columns"), "{out}");
    }

    #[test]
    fn single_column_with_card_snapshot() {
        let out = lines(
            Mermaid::new("kanban\n  todo[To Do]\n    t1[Write tests]"),
            22,
            10,
        );
        assert!(out.contains("To Do"), "title missing:\n{out}");
        assert!(out.contains("Write"), "card text missing:\n{out}");
        // Rounded lane border + square card border.
        assert!(out.contains('╭') && out.contains('╯'), "{out}");
        assert!(out.contains('┌') && out.contains('┘'), "{out}");
    }

    #[test]
    fn columns_render_side_by_side() {
        let out = lines(
            Mermaid::new("kanban\n  a[Alpha]\n    x[one]\n  b[Beta]\n    y[two]"),
            40,
            10,
        );
        let title_row = out.lines().find(|r| r.contains("Alpha")).unwrap();
        assert!(
            title_row.contains("Alpha") && title_row.contains("Beta"),
            "lanes not side by side:\n{out}"
        );
        // Two lane frames.
        let top = out.lines().find(|r| r.contains('╭')).unwrap();
        assert_eq!(top.matches('╭').count(), 2, "{out}");
    }

    #[test]
    fn meta_line_is_rendered_under_card() {
        let out = lines(
            Mermaid::new("kanban\n  todo[Todo]\n    t1[Bug]@{ priority: 'High', assigned: 'Al' }"),
            26,
            10,
        );
        assert!(out.contains("Bug"), "{out}");
        assert!(out.contains("!High"), "priority meta missing:\n{out}");
        assert!(out.contains("@Al"), "assignee meta missing:\n{out}");
    }

    #[test]
    fn long_text_is_clipped_not_overflowed() {
        let out = lines(
            Mermaid::new(
                "kanban\n  c[Column With A Very Long Title Indeed]\n    t[An extremely long card description that cannot fit]",
            ),
            20,
            9,
        );
        assert!(out.contains('…'), "expected clipping ellipsis in:\n{out}");
        // Every row is exactly 20 cells wide (no overflow).
        for row in out.lines() {
            assert_eq!(row.chars().count(), 20, "row wrong width:\n{out}");
        }
    }

    #[test]
    fn tiny_area_does_not_panic_and_clips() {
        let out = lines(
            Mermaid::new("kanban\n  a[A]\n    x[1]\n  b[B]\n    y[2]"),
            6,
            3,
        );
        assert_eq!(out.lines().count(), 3);
    }

    #[test]
    fn one_by_one_area_is_safe() {
        let _ = lines(Mermaid::new("kanban\n  a[A]\n    x[1]"), 1, 1);
    }
}
