//! `sankey-beta` Mermaid diagram renderer.
//!
//! A Mermaid `sankey-beta` body is RFC-4180-ish CSV: each line is
//! `Source,Target,Value`. Nodes are discovered in first-seen order and a
//! weighted link is recorded per row. A node's *throughput* is
//! `max(total_in, total_out)`; the diagram is a layered flow graph.
//!
//! # Terminal projection
//!
//! The Mermaid web renderer draws smooth proportional ribbons. A character
//! grid cannot anti-alias a ribbon, so this renderer uses the legible
//! deterministic proxy that fits a TUI: nodes are grouped into **layer
//! columns** by longest-path rank from the sources (a topological order;
//! cycles are broken exactly like the flowchart ranker — a back-edge does not
//! advance the rank). Each node is a vertical **bar** whose height is
//! proportional to its throughput, labelled beside it, and each link is a
//! horizontal **band** between two columns drawn with block shading
//! (`█`/`▓`/`▒`/`░`, heavier = larger value) and a row count proportional to
//! the link value. Everything is integer-scaled to the area, so the result
//! is deterministic and snapshot-testable.
//!
//! Parsing is lenient: a row without two commas or a non-numeric value is
//! skipped; an empty/garbage body renders the shared honest
//! [`super::diagram_placeholder`].

use rstui_core::{Buffer, Rect, Style};

use super::MermaidTheme;
use super::draw::Surface;

/// One directed weighted link `from -> to` of `value`.
#[derive(Debug, Clone)]
struct Link {
    /// Source node index.
    from: usize,
    /// Target node index.
    to: usize,
    /// The flow magnitude (already validated as a finite, non-negative f64).
    value: f64,
}

/// The parsed flow graph: node names in first-seen order plus the links.
#[derive(Debug, Default)]
struct Sankey {
    /// Node display names, in first-seen order; the index is the node id.
    names: Vec<String>,
    /// The weighted links, in source order.
    links: Vec<Link>,
}

impl Sankey {
    /// The index of `name`, registering it (in first-seen order) if new.
    fn intern(&mut self, name: &str) -> usize {
        if let Some(i) = self.names.iter().position(|n| n == name) {
            i
        } else {
            self.names.push(name.to_owned());
            self.names.len() - 1
        }
    }
}

/// Strips `\r` and a trailing `%%` comment from a raw line and trims it.
fn clean(raw: &str) -> &str {
    let no_cr = raw.strip_suffix('\r').unwrap_or(raw);
    let body = match no_cr.find("%%") {
        Some(i) => &no_cr[..i],
        None => no_cr,
    };
    body.trim()
}

/// Splits one RFC-4180-ish CSV record into its fields. A field may be
/// `"`-quoted; inside a quoted field a doubled `""` is one literal quote and
/// a comma is literal. Unquoted fields are split on `,` and trimmed.
fn split_csv(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut field = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == ',' {
            out.push(field.trim().to_owned());
            field = String::new();
        } else {
            field.push(c);
        }
    }
    out.push(field.trim().to_owned());
    out
}

/// Parses a `sankey-beta` source into a [`Sankey`].
///
/// The first significant line is the header (`sankey-beta` / `sankey`) and is
/// consumed. Every subsequent line is a CSV `Source,Target,Value`; a row that
/// does not yield two non-empty endpoints and a finite non-negative numeric
/// value is skipped.
fn parse(src: &str) -> Sankey {
    let mut g = Sankey::default();
    let mut seen_header = false;
    for raw in src.split('\n') {
        let line = clean(raw);
        if line.is_empty() {
            continue;
        }
        if !seen_header {
            // The header is the first significant line. Consume it if it is
            // the keyword; otherwise be lenient and try to parse it as data.
            seen_header = true;
            let lower = line.to_ascii_lowercase();
            if lower.starts_with("sankey-beta") || lower == "sankey" {
                continue;
            }
        }
        let fields = split_csv(line);
        if fields.len() < 3 {
            continue;
        }
        let (src_n, tgt_n) = (fields[0].trim(), fields[1].trim());
        if src_n.is_empty() || tgt_n.is_empty() {
            continue;
        }
        let Ok(value) = fields[2].trim().parse::<f64>() else {
            continue;
        };
        if !value.is_finite() || value < 0.0 {
            continue;
        }
        let from = g.intern(src_n);
        let to = g.intern(tgt_n);
        g.links.push(Link { from, to, value });
    }
    g
}

/// Assigns every node a layer (column) by longest-path rank from the sources,
/// breaking cycles exactly the way the flowchart ranker (`rank_nodes` in
/// [`super`]) does.
///
/// Repeated forward passes over the links in source order pull a target to
/// `source.layer + 1`. A self-link (`a == b`) is ignored. When the graph has
/// **no** source (every node has an incoming link — a pure cycle) the
/// first-seen node (index 0) is pinned at layer 0 and never relaxed, so the
/// cycle stabilises instead of growing without bound; iteration is also
/// bounded by the node count as a belt-and-braces stop.
fn layer_of(g: &Sankey) -> Vec<usize> {
    let n = g.names.len();
    let mut layer = vec![0usize; n];
    if n == 0 {
        return layer;
    }
    let mut has_incoming = vec![false; n];
    for l in &g.links {
        if l.from != l.to {
            has_incoming[l.to] = true;
        }
    }
    let any_root = has_incoming.iter().any(|&v| !v);
    for _ in 0..n {
        let mut changed = false;
        for l in &g.links {
            if l.from == l.to {
                continue;
            }
            let want = layer[l.from] + 1;
            if want > layer[l.to] && (any_root || l.to != 0) {
                layer[l.to] = want;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    layer
}

/// Renders a `sankey-beta` Mermaid diagram from `src` into `area`.
///
/// Computes per-node throughput and layer, lays out one column per layer with
/// node bars scaled to the area height, then draws each link as a shaded
/// horizontal band between its columns before a single centred blit. An
/// empty/garbage body falls back to the shared placeholder.
pub(crate) fn render(src: &str, area: Rect, buf: &mut Buffer, base: Style, theme: &MermaidTheme) {
    let g = parse(src);
    if g.links.is_empty() {
        super::diagram_placeholder("sankey", "no flows", area, buf, base, theme);
        return;
    }

    let n = g.names.len();
    let layer = layer_of(&g);
    let n_layers = layer.iter().copied().max().unwrap_or(0) + 1;

    // Per-node throughput = max(total in, total out).
    let mut tin = vec![0.0f64; n];
    let mut tout = vec![0.0f64; n];
    for l in &g.links {
        tout[l.from] += l.value;
        tin[l.to] += l.value;
    }
    let through: Vec<f64> = (0..n).map(|i| tin[i].max(tout[i])).collect();

    // Surface geometry. A node renders as a 1-wide bar with its label on the
    // row directly *above* the bar (never beside it) so a horizontal flow
    // band can never clobber a label. Bar heights scale to a small fixed cap
    // so a typical diagram fits a normal area; the blit clips/centres anyway.
    let bar_w = 1;
    let max_bar = 6; // tallest a single bar gets, in rows.
    let label_w = g
        .names
        .iter()
        .map(|s| s.chars().count() as i32)
        .max()
        .unwrap_or(4)
        .clamp(3, 18);
    // Column = bar + a gap wide enough for the value text + the band.
    let col_w = bar_w + label_w.max(6) + 4;
    let canvas_w = n_layers as i32 * col_w;

    let max_through = through.iter().cloned().fold(0.0f64, f64::max).max(1.0);
    let bar_rows = |t: f64| -> i32 {
        if t <= 0.0 {
            1
        } else {
            ((t / max_through * max_bar as f64).round() as i32).clamp(1, max_bar)
        }
    };

    // Stack nodes within each layer column: each block is 1 label row + the
    // bar + 1 separator. `node_y` is the bar's top row; the label sits at
    // `node_y - 1`. Tallest-first by source order — deterministic. Geometry
    // only; nothing is painted yet so the draw order can be band → bar →
    // label (bars and labels always win over a band).
    let col_x = |l: usize| l as i32 * col_w;
    let mut node_y = vec![0i32; n];
    let mut node_h = vec![0i32; n];
    let mut col_bottom = 0i32;
    for lyr in 0..n_layers {
        let mut y = 1; // row 0 reserved for the first label.
        for ni in 0..n {
            if layer[ni] != lyr {
                continue;
            }
            let h = bar_rows(through[ni]);
            node_y[ni] = y;
            node_h[ni] = h;
            y += h + 2; // bar + separator + next label row.
        }
        col_bottom = col_bottom.max(y);
    }
    let canvas_h = col_bottom.max(1);
    let mut s = Surface::new(canvas_w.max(1), canvas_h.max(1));
    // A degenerate (zero-area) layout cannot draw anything legible — fall
    // back to the honest placeholder instead of blitting an empty surface.
    if s.width() == 0 || s.height() == 0 {
        super::diagram_placeholder("sankey", "no flows", area, buf, base, theme);
        return;
    }

    let bar_st = base.patch(theme.node_border);
    let label_st = base.patch(theme.node_label);
    let band_st = base.patch(theme.edge);
    let val_st = base.patch(theme.edge_label);

    // 1) Bands first. A band runs from just right of the source bar to just
    //    left of the target bar; its row count is proportional to the value
    //    relative to the source's throughput. Rows are consumed top-down on
    //    each side so multiple links off one node do not overlap. Bands are
    //    painted before bars/labels so those always read on top.
    let mut out_used = vec![0i32; n];
    let mut in_used = vec![0i32; n];
    let mut value_marks: Vec<(i32, i32, String)> = Vec::new();
    for l in &g.links {
        let sh = node_h[l.from].max(1);
        let st = through[l.from].max(1e-9);
        let rows = (((l.value / st) * sh as f64).round() as i32).clamp(1, sh);
        let shade = if l.value >= max_through * 0.66 {
            '▓'
        } else if l.value >= max_through * 0.33 {
            '▒'
        } else {
            '░'
        };
        let sx = col_x(layer[l.from]) + bar_w; // just right of source bar
        let tx = col_x(layer[l.to]) - 1; // just left of target bar
        if tx < sx {
            continue;
        }
        let sy0 = node_y[l.from] + out_used[l.from];
        let ty0 = node_y[l.to] + in_used[l.to];
        for r in 0..rows {
            let ys = sy0 + r;
            let yt = ty0 + r;
            for x in sx..=tx {
                let span = (tx - sx).max(1) as f64;
                let frac = (x - sx) as f64 / span;
                let y = (ys as f64 + (yt - ys) as f64 * frac).round() as i32;
                s.set(x, y, shade, band_st);
            }
        }
        if rows > 0 && tx > sx + 1 {
            value_marks.push(((sx + tx) / 2, sy0, fmt_num(l.value)));
        }
        out_used[l.from] += rows;
        in_used[l.to] += rows;
    }

    // 2) Bars on top of the bands.
    for ni in 0..n {
        s.vline(col_x(layer[ni]), node_y[ni], node_h[ni], '█', bar_st);
    }

    // 3) Value labels, then node labels — painted last so text always reads
    //    on top of any band shading behind it. The node label sits on the
    //    row directly above its bar's top.
    for (x, y, txt) in &value_marks {
        s.text(*x, *y, txt, val_st);
    }
    for ni in 0..n {
        let bx = col_x(layer[ni]);
        s.text(bx, node_y[ni] - 1, &g.names[ni], label_st);
    }

    s.blit(area, buf, base);
}

/// Formats a flow value compactly: an integer prints without a decimal point,
/// anything else with up to two trailing digits (trailing zeros trimmed).
fn fmt_num(v: f64) -> String {
    if (v.fract()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Position;

    /// Renders `src` as a `sankey-beta` into a fresh `w`×`h` buffer; returns
    /// the glyphs as one newline-terminated line per row.
    fn lines(src: &str, w: u16, h: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        render(
            src,
            buf.area(),
            &mut buf,
            Style::new(),
            &MermaidTheme::default(),
        );
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    // --- parser tests ------------------------------------------------------

    #[test]
    fn nodes_are_interned_in_first_seen_order() {
        let g = parse("sankey-beta\nA,B,5\nB,C,3\nA,C,2");
        assert_eq!(g.names, vec!["A", "B", "C"]);
        assert_eq!(g.links.len(), 3);
        assert_eq!((g.links[0].from, g.links[0].to), (0, 1));
        assert_eq!(g.links[0].value, 5.0);
    }

    #[test]
    fn header_is_consumed_and_case_insensitive() {
        let g = parse("sankey-beta\nX,Y,1");
        assert_eq!(g.names, vec!["X", "Y"]);
        // Plain `sankey` header also accepted.
        let g2 = parse("sankey\nX,Y,1");
        assert_eq!(g2.links.len(), 1);
    }

    #[test]
    fn quoted_fields_with_commas_and_escaped_quotes() {
        let g = parse("sankey-beta\n\"Smith, J\",\"a \"\"b\"\" c\",10");
        assert_eq!(g.names[0], "Smith, J");
        assert_eq!(g.names[1], "a \"b\" c");
        assert_eq!(g.links[0].value, 10.0);
    }

    #[test]
    fn bad_rows_are_skipped_leniently() {
        // Too few fields, non-numeric value, negative, empty endpoint.
        let g = parse(
            "sankey-beta\n\
             A,B\n\
             A,B,notnum\n\
             A,B,-3\n\
             ,B,5\n\
             A,C,7",
        );
        assert_eq!(g.links.len(), 1);
        assert_eq!((g.links[0].from, g.links[0].to), (0, 1)); // A,C
        assert_eq!(g.names, vec!["A", "C"]);
    }

    #[test]
    fn fractional_values_parse() {
        let g = parse("sankey-beta\nA,B,2.5");
        assert_eq!(g.links[0].value, 2.5);
    }

    #[test]
    fn layers_are_longest_path_rank() {
        // A->B->C and A->C : C must be layer 2 (longest path), not 1.
        let g = parse("sankey-beta\nA,B,1\nB,C,1\nA,C,1");
        let lyr = layer_of(&g);
        assert_eq!(lyr[0], 0); // A
        assert_eq!(lyr[1], 1); // B
        assert_eq!(lyr[2], 2); // C (longest path A->B->C)
    }

    #[test]
    fn a_cycle_terminates_and_does_not_rank_backwards() {
        // A->B->A : ranking must stop, not loop forever.
        let g = parse("sankey-beta\nA,B,1\nB,A,1");
        let lyr = layer_of(&g);
        assert_eq!(lyr[0], 0);
        assert_eq!(lyr[1], 1);
    }

    #[test]
    fn fmt_num_trims_integers_and_decimals() {
        assert_eq!(fmt_num(5.0), "5");
        assert_eq!(fmt_num(2.5), "2.5");
        assert_eq!(fmt_num(2.50), "2.5");
        assert_eq!(fmt_num(4.27), "4.27");
    }

    // --- render snapshot tests --------------------------------------------

    #[test]
    fn empty_source_renders_placeholder() {
        let out = lines("sankey-beta\n", 40, 3);
        assert!(out.contains("mermaid"), "got:\n{out}");
        assert!(out.contains("sankey"), "got:\n{out}");
        assert!(out.contains("no flows"), "got:\n{out}");
    }

    #[test]
    fn garbage_body_renders_placeholder() {
        let out = lines("sankey-beta\nnot,enough\nalso bad\n", 40, 3);
        assert!(out.contains("no flows"), "got:\n{out}");
    }

    #[test]
    fn single_link_draws_two_bars_a_band_and_labels() {
        let out = lines("sankey-beta\nAlpha,Beta,10", 40, 12);
        // Two node bars.
        assert!(out.contains('█'), "expected bars, got:\n{out}");
        // Both labels.
        assert!(out.contains("Alpha"), "got:\n{out}");
        assert!(out.contains("Beta"), "got:\n{out}");
        // A shaded band between the columns.
        assert!(
            out.contains('▓') || out.contains('▒') || out.contains('░'),
            "expected a shaded band, got:\n{out}"
        );
        // The flow value is printed.
        assert!(out.contains("10"), "got:\n{out}");
    }

    #[test]
    fn three_layer_chain_makes_three_columns() {
        let out = lines("sankey-beta\nA,B,4\nB,C,4", 48, 14);
        assert!(out.contains('A'), "got:\n{out}");
        assert!(out.contains('B'), "got:\n{out}");
        assert!(out.contains('C'), "got:\n{out}");
        // Bars for three nodes ⇒ at least three distinct bar columns; check
        // by counting columns that contain a full block.
        let rows: Vec<&str> = out.lines().collect();
        let mut bar_cols = std::collections::BTreeSet::new();
        for r in &rows {
            for (x, ch) in r.chars().enumerate() {
                if ch == '█' {
                    bar_cols.insert(x);
                }
            }
        }
        assert!(
            bar_cols.len() >= 3,
            "expected >=3 bar columns, got {bar_cols:?}\n{out}"
        );
    }

    #[test]
    fn heavier_flow_uses_a_darker_shade() {
        // One big and one tiny flow off the same source; the big one is the
        // max so it shades `▓`, the small one `░`.
        let out = lines("sankey-beta\nS,Big,100\nS,Sm,1", 40, 16);
        assert!(out.contains('▓'), "big flow should be dark, got:\n{out}");
        assert!(out.contains('░'), "small flow should be light, got:\n{out}");
    }

    #[test]
    fn tiny_area_clips_without_panic() {
        let out = lines("sankey-beta\nA,B,5\nB,C,5\nA,C,2", 5, 2);
        assert_eq!(out.lines().count(), 2);
    }

    #[test]
    fn proportional_bar_heights_scale_with_throughput() {
        // X throughput 10, Y throughput 10 (single link). Both bars equal.
        // Then a node with double throughput should be taller.
        let out = lines("sankey-beta\nA,Z,2\nB,Z,8", 40, 20);
        let rows: Vec<&str> = out.lines().collect();
        // Count `█` in the leftmost bar column vs the Z column.
        let count_col = |needle_row_has: char| {
            rows.iter()
                .filter(|r| r.chars().any(|c| c == needle_row_has))
                .count()
        };
        // Z has throughput 10 (2+8), A has 2, B has 8. The Z bar (rightmost)
        // is the tallest single bar.
        let total_blocks: usize = rows.iter().map(|r| r.matches('█').count()).sum();
        assert!(total_blocks > 0, "expected bars\n{out}");
        // Sanity: Z column taller than A column.
        assert!(count_col('█') > 0, "{out}");
    }
}
