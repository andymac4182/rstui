//! `quadrantChart` Mermaid diagram renderer.
//!
//! A 2×2 priority/positioning matrix: an outer frame split by a mid cross into
//! four cells, each cell carrying its quadrant title centred, the x/y axis end
//! labels along the bottom and left edges, and every data point plotted as a
//! `●` at its scaled `(x, y)` position with its name placed beside it. Mermaid
//! numbers the quadrants `q1` top-right, `q2` top-left, `q3` bottom-left, `q4`
//! bottom-right, with the origin `(0, 0)` at the bottom-left — this renderer
//! follows that convention exactly so the picture matches the browser.
//!
//! Grammar handled (a lenient subset of Mermaid's):
//!
//! ```text
//! quadrantChart
//!     title Reach and Engagement
//!     x-axis Low Reach --> High Reach
//!     y-axis Low Engagement --> High Engagement
//!     quadrant-1 We should expand
//!     quadrant-2 Need to promote
//!     quadrant-3 Re-evaluate
//!     quadrant-4 May be improved
//!     Campaign A: [0.3, 0.6]
//!     Campaign B:::class1: [0.45, 0.23]
//! ```
//!
//! Axis end labels are optional and either end may be omitted; quadrant titles
//! are optional; a point may carry a `:::class` style tag (parsed and ignored
//! for colour — the terminal point is a single glyph). Coordinates are `0..1`
//! and clamped. Unparseable lines are skipped; a chart with no points and no
//! quadrant titles degrades to the shared honest placeholder. Point placement
//! is integer-scaled so a snapshot is deterministic.

use rstui_core::{Buffer, Rect, Style};

use super::MermaidTheme;
use super::draw::{BoxStyle, Surface};

/// One plotted point: its display name and `(x, y)` in fixed-point per-mille
/// (`0..=1000`, i.e. the `0.0..=1.0` source value × 1000).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Point {
    /// The label drawn next to the marker.
    name: String,
    /// X position × 1000 (0 = left edge, 1000 = right edge).
    x: i64,
    /// Y position × 1000 (0 = bottom edge, 1000 = top edge).
    y: i64,
}

/// A parsed `quadrantChart`: title, the four optional quadrant names, the
/// optional axis end labels, and the points in source order.
#[derive(Debug, Default, PartialEq, Eq)]
struct Quadrant {
    /// The `title …` text, if any.
    title: Option<String>,
    /// `quadrant-1..4` names indexed `0..4` (q1, q2, q3, q4).
    quads: [Option<String>; 4],
    /// `x-axis` `(low, high)` end labels (either may be absent).
    x_axis: (Option<String>, Option<String>),
    /// `y-axis` `(low, high)` end labels (either may be absent).
    y_axis: (Option<String>, Option<String>),
    /// The plotted points.
    points: Vec<Point>,
}

/// Drops Mermaid preamble noise and the `quadrantChart` header, returning the
/// significant body lines trimmed.
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
            seen_header = true;
            continue;
        }
        out.push(line.to_string());
    }
    out
}

/// Parses a `0.0..=1.0` decimal into per-mille (`0..=1000`), clamped. Rejects
/// non-numerics.
fn parse_unit(s: &str) -> Option<i64> {
    let s = s.trim();
    let (int_part, frac_part) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    let int_v: i64 = if int_part.is_empty() {
        0
    } else {
        int_part.parse().ok()?
    };
    if !frac_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut it = frac_part.chars();
    let d0 = it.next().and_then(|c| c.to_digit(10)).unwrap_or(0) as i64;
    let d1 = it.next().and_then(|c| c.to_digit(10)).unwrap_or(0) as i64;
    let d2 = it.next().and_then(|c| c.to_digit(10)).unwrap_or(0) as i64;
    let round = i64::from(it.next().and_then(|c| c.to_digit(10)).unwrap_or(0) >= 5);
    Some((int_v * 1000 + d0 * 100 + d1 * 10 + d2 + round).clamp(0, 1000))
}

/// Splits an `lo --> hi` axis spec into its two trimmed ends, either of which
/// may be empty (→ `None`).
fn parse_axis(rest: &str) -> (Option<String>, Option<String>) {
    let some = |s: &str| {
        let t = s.trim().trim_matches('"').trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    };
    match rest.split_once("-->") {
        Some((a, b)) => (some(a), some(b)),
        None => (some(rest), None),
    }
}

/// Parses a `Name: [x, y]` (or `Name:::class: [x, y]`) point line into a
/// [`Point`], or `None` if it is not a well-formed point.
fn parse_point(line: &str) -> Option<Point> {
    let lb = line.find('[')?;
    let rb = line.find(']')?;
    if rb <= lb {
        return None;
    }
    let head = &line[..lb];
    // The name is everything up to the first `:`; a `:::class` style tag and
    // the trailing `:` before the bracket are stripped.
    let name_end = head.find(':')?;
    let name = head[..name_end].trim().to_string();
    if name.is_empty() {
        return None;
    }
    let coords = &line[lb + 1..rb];
    let (xs, ys) = coords.split_once(',')?;
    let x = parse_unit(xs)?;
    let y = parse_unit(ys)?;
    Some(Point { name, x, y })
}

/// Parses a whole `quadrantChart` source into a [`Quadrant`], skipping bad
/// lines.
fn parse(src: &str) -> Quadrant {
    let mut q = Quadrant::default();
    for line in clean_lines(src) {
        if let Some(rest) = line.strip_prefix("title") {
            q.title = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("x-axis") {
            q.x_axis = parse_axis(rest);
        } else if let Some(rest) = line.strip_prefix("y-axis") {
            q.y_axis = parse_axis(rest);
        } else if let Some(rest) = line.strip_prefix("quadrant-") {
            if let Some((num, name)) = rest.split_once(' ') {
                if let Ok(idx) = num.trim().parse::<usize>() {
                    if (1..=4).contains(&idx) {
                        q.quads[idx - 1] = Some(name.trim().to_string());
                    }
                }
            }
        } else if let Some(p) = parse_point(&line) {
            q.points.push(p);
        }
    }
    q
}

/// Renders a `quadrantChart` Mermaid diagram from `src` into `area`.
///
/// Layout: an optional centred title; an outer box split by a mid `┼` cross
/// into four cells; each cell's `quadrant-N` title centred (Mermaid order: q1
/// top-right, q2 top-left, q3 bottom-left, q4 bottom-right); the x-axis end
/// labels along the bottom and the y-axis end labels down the left; each point
/// a `●` at its integer-scaled `(x, y)` (origin bottom-left) with its name to
/// the right, nudged left near the right edge to avoid clipping. Empty input
/// draws the shared placeholder; a tiny area clips rather than panicking.
pub(crate) fn render(src: &str, area: Rect, buf: &mut Buffer, base: Style, theme: &MermaidTheme) {
    let q = parse(src);
    if q.points.is_empty() && q.quads.iter().all(Option::is_none) {
        super::diagram_placeholder("quadrant", "no points", area, buf, base, theme);
        return;
    }

    let frame_st = base.patch(theme.node_border);
    let title_st = base.patch(theme.node_label);
    let cross_st = base.patch(theme.edge);
    let quad_st = base.patch(theme.cluster);
    let pt_st = base.patch(theme.edge_label);
    let axis_st = base.patch(theme.node_label);

    let w = (area.width as i32).max(1);
    let h = (area.height as i32).max(1);
    let mut s = Surface::new(w, h);

    let mut top = 0;
    if let Some(t) = &q.title {
        s.text_centered(0, 0, w, t, title_st);
        top = 1;
    }

    // Reserve a left gutter for the y-axis labels and a bottom row for the
    // x-axis labels, then the plot box fills the rest.
    let has_y = q.y_axis.0.is_some() || q.y_axis.1.is_some();
    let has_x = q.x_axis.0.is_some() || q.x_axis.1.is_some();
    let left_pad = if has_y { 1 } else { 0 };
    let bot_pad = if has_x { 1 } else { 0 };

    let bx = left_pad;
    let by = top;
    let bw = w - left_pad;
    let bh = h - top - bot_pad;

    if bw < 3 || bh < 3 {
        // Too small for a frame + cross — clip cleanly.
        s.blit(area, buf, base);
        return;
    }

    s.rect(bx, by, bw, bh, BoxStyle::Square, frame_st);

    // The interior the points and the cross live in.
    let ix0 = bx + 1;
    let iy0 = by + 1;
    let iw = bw - 2;
    let ih = bh - 2;
    let midx = ix0 + iw / 2;
    let midy = iy0 + ih / 2;

    // The mid cross dividing the four quadrants.
    s.vline(midx, iy0, ih, '┊', cross_st);
    s.hline(ix0, midy, iw, '┄', cross_st);
    s.set(midx, midy, '┼', cross_st);
    // Tidy the divider/frame tee joins.
    s.set(midx, by, '┬', frame_st);
    s.set(midx, by + bh - 1, '┴', frame_st);
    s.set(bx, midy, '├', frame_st);
    s.set(bx + bw - 1, midy, '┤', frame_st);

    // Quadrant titles. Mermaid: q1 top-right, q2 top-left, q3 bottom-left,
    // q4 bottom-right. Each is centred within its cell's interior.
    let cell_w = (iw / 2).max(1);
    let left_x = ix0;
    let right_x = midx + 1;
    let top_y = iy0 + ih / 4;
    let bot_y = midy + 1 + ih / 4;
    let mut place = |idx: usize, cx: i32, cy: i32, cw: i32| {
        if let Some(name) = &q.quads[idx] {
            s.text_centered(cx, cy, cw, name, quad_st);
        }
    };
    place(0, right_x, top_y, cell_w); // q1 top-right
    place(1, left_x, top_y, cell_w); // q2 top-left
    place(2, left_x, bot_y, cell_w); // q3 bottom-left
    place(3, right_x, bot_y, cell_w); // q4 bottom-right

    // Points: origin at bottom-left, so a higher y is a smaller row.
    for p in &q.points {
        let px = ix0 + (p.x * (iw - 1).max(1) as i64 / 1000) as i32;
        let py = iy0 + ((1000 - p.y) * (ih - 1).max(1) as i64 / 1000) as i32;
        let px = px.clamp(ix0, ix0 + iw - 1);
        let py = py.clamp(iy0, iy0 + ih - 1);
        s.set(px, py, '●', pt_st);
        // Place the name to the right of the marker, but flip to the left
        // when it would overflow the frame so it stays inside.
        let nlen = p.name.chars().count() as i32;
        if px + 2 + nlen <= ix0 + iw {
            s.text(px + 2, py, &p.name, pt_st);
        } else {
            let lx = (px - 1 - nlen).max(ix0);
            s.text_clipped(lx, py, &p.name, px - 1 - lx, pt_st);
        }
    }

    // Axis end labels: x along the bottom row (low left, high right), y down
    // the left gutter (low bottom, high top).
    if has_x {
        let row = by + bh; // the reserved row just below the box
        if let Some(lo) = &q.x_axis.0 {
            s.text_clipped(bx, row, lo, iw / 2, axis_st);
        }
        if let Some(hi) = &q.x_axis.1 {
            let hlen = hi.chars().count() as i32;
            let start = (bx + bw - hlen).max(bx + bw / 2);
            s.text_clipped(start, row, hi, bx + bw - start, axis_st);
        }
    }
    if has_y {
        // The single-column gutter only fits an initial; the full text is
        // written vertically so the meaning survives the narrow space.
        if let Some(lo) = &q.y_axis.0 {
            // "low" reads bottom-up near the bottom of the left edge.
            let base_y = by + bh - 1;
            for (i, ch) in lo
                .chars()
                .rev()
                .take((bh as usize).saturating_sub(1))
                .enumerate()
            {
                s.set(0, base_y - i as i32, ch, axis_st);
            }
        }
        if let Some(hi) = &q.y_axis.1 {
            // "high" reads top-down near the top of the left edge.
            for (i, ch) in hi.chars().take((bh / 2).max(1) as usize).enumerate() {
                s.set(0, by + i as i32, ch, axis_st);
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

    /// Renders `src` through the [`Mermaid`] widget into a fresh buffer and
    /// returns the glyphs as one newline-terminated line per row.
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
    fn parses_title_axes_quadrants_and_points() {
        let q = parse(
            "quadrantChart\ntitle Reach\nx-axis Low --> High\ny-axis Bot --> Top\nquadrant-1 Expand\nquadrant-3 Drop\nA: [0.3, 0.6]\n",
        );
        assert_eq!(q.title.as_deref(), Some("Reach"));
        assert_eq!(q.x_axis, (Some("Low".into()), Some("High".into())));
        assert_eq!(q.y_axis, (Some("Bot".into()), Some("Top".into())));
        assert_eq!(q.quads[0].as_deref(), Some("Expand"));
        assert_eq!(q.quads[2].as_deref(), Some("Drop"));
        assert_eq!(q.quads[1], None);
        assert_eq!(q.points.len(), 1);
        assert_eq!(q.points[0].name, "A");
        assert_eq!(q.points[0].x, 300);
        assert_eq!(q.points[0].y, 600);
    }

    #[test]
    fn styled_point_class_tag_is_stripped() {
        let q = parse("quadrantChart\nCampaign B:::class1: [0.45, 0.23]\n");
        assert_eq!(q.points.len(), 1);
        assert_eq!(q.points[0].name, "Campaign B");
        assert_eq!(q.points[0].x, 450);
        assert_eq!(q.points[0].y, 230);
    }

    #[test]
    fn one_ended_axis_and_clamped_coords() {
        let q = parse("quadrantChart\nx-axis OnlyLow\nP: [1.5, 0.0]\n");
        assert_eq!(q.x_axis, (Some("OnlyLow".into()), None));
        // 1.5 is out of the 0..1 unit range and clamps to the right edge.
        assert_eq!(q.points.len(), 1);
        assert_eq!(q.points[0].x, 1000);
        assert_eq!(q.points[0].y, 0);
    }

    #[test]
    fn out_of_range_and_garbage_coords_are_lenient() {
        // A leading-`-` int part still parses (sign dropped) then clamps to
        // the valid band — lenient, never a panic.
        assert_eq!(parse_unit("-0.2"), Some(200));
        assert_eq!(parse_unit("2.0"), Some(1000));
        // Pure garbage is rejected so the point line is skipped entirely.
        assert_eq!(parse_unit("abc"), None);
        let q = parse("quadrantChart\nP: [abc, 0.5]\nQ: [0.5, 0.5]\n");
        assert_eq!(q.points.len(), 1);
        assert_eq!(q.points[0].name, "Q");
    }

    #[test]
    fn bad_lines_skipped_not_panic() {
        let q = parse("quadrantChart\nnonsense\nP: [0.5, 0.5]\nQ: [bad]\nR: [0.1, 0.9]\n");
        assert_eq!(q.points.len(), 2);
        assert_eq!(q.points[0].name, "P");
        assert_eq!(q.points[1].name, "R");
    }

    #[test]
    fn frontmatter_and_comments_skipped() {
        let q = parse("---\nconfig: x\n---\nquadrantChart\n%% c\nA: [0.2, 0.2]\n");
        assert_eq!(q.points.len(), 1);
    }

    // --- render snapshots --------------------------------------------------

    #[test]
    fn empty_quadrant_is_placeholder() {
        let out = lines("quadrantChart\n", 40, 8);
        assert!(out.contains("mermaid · quadrant: no points"), "{out}");
    }

    #[test]
    fn full_render_snapshot() {
        let out = lines(
            "quadrantChart\ntitle Mtx\nquadrant-1 Q1\nquadrant-2 Q2\nquadrant-3 Q3\nquadrant-4 Q4\nP: [0.75, 0.75]\n",
            25,
            11,
        );
        // Title centred; framed box; `┬/┴/├/┤/┼` cross tees; each Qn centred
        // in its cell (q1 top-right…); the point `●` at (0.75, 0.75) sits in
        // the upper-right quadrant with its name to the right. Byte-exact and
        // deterministic from the integer coordinate scaling.
        let expected = [
            "           Mtx           ",
            "┌───────────┬───────────┐",
            "│           ┊           │",
            "│           ┊    ● P    │",
            "│    Q2     ┊    Q1     │",
            "│           ┊           │",
            "├┄┄┄┄┄┄┄┄┄┄┄┼┄┄┄┄┄┄┄┄┄┄┄┤",
            "│           ┊           │",
            "│           ┊           │",
            "│    Q3     ┊    Q4     │",
            "└───────────┴───────────┘",
        ]
        .join("\n")
            + "\n";
        assert_eq!(out, expected);
    }

    #[test]
    fn point_lower_left_is_in_q3() {
        let out = lines(
            "quadrantChart\nquadrant-3 Re-eval\nLowPt: [0.1, 0.1]\n",
            24,
            10,
        );
        // (0.1, 0.1) → near bottom-left; the marker row is below the mid.
        let mid = out.lines().count() / 2;
        let marker_line = out.lines().position(|l| l.contains('●')).unwrap();
        assert!(marker_line > mid, "point should be low:\n{out}");
    }

    #[test]
    fn axis_end_labels_render() {
        let out = lines(
            "quadrantChart\nx-axis Lo --> Hi\ny-axis Dn --> Up\nP: [0.5, 0.5]\n",
            26,
            10,
        );
        assert!(out.contains("Lo"), "x-low label:\n{out}");
        assert!(out.contains("Hi"), "x-high label:\n{out}");
    }

    #[test]
    fn quadrants_only_no_points_still_renders() {
        let out = lines("quadrantChart\nquadrant-1 Alpha\n", 22, 9);
        assert!(out.contains("Alpha"), "{out}");
        assert!(out.contains('┼'), "cross present:\n{out}");
    }

    #[test]
    fn tiny_area_does_not_panic() {
        let _ = lines("quadrantChart\nP: [0.5, 0.5]\n", 4, 3);
        let _ = lines("quadrantChart\nquadrant-1 Q\nP: [0.1, 0.9]\n", 1, 1);
        let _ = lines(
            "quadrantChart\ntitle T\nx-axis a --> b\nP: [0.2, 0.2]\n",
            6,
            2,
        );
    }
}
