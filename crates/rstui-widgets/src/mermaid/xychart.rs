//! `xychart-beta` Mermaid diagram renderer.
//!
//! A framed Cartesian plot: a y-axis with tick labels down the left, an
//! x-axis with the category labels along the bottom, every `bar [...]` series
//! drawn as vertical block columns with sub-cell eighth precision (the same
//! ramp [`crate::BarChart`] uses) and every `line [...]` series drawn as a
//! connected path of `•` markers overlaid on top. With more than one series a
//! small legend names them. The value axis auto-scales to the data when no
//! explicit `y-axis lo --> hi` is given, and the whole plot scales to fill the
//! available area.
//!
//! Grammar handled (a lenient subset of Mermaid's):
//!
//! ```text
//! xychart-beta
//!     title "Revenue"
//!     x-axis "Month" [jan, feb, mar]
//!     y-axis "USD" 0 --> 100
//!     bar [30, 60, 45]
//!     line [20, 40, 80]
//! ```
//!
//! `xychart-beta horizontal` is accepted (the orientation hint is parsed; the
//! terminal rendering stays column-oriented for legibility). `x-axis` may be a
//! quoted-label-plus-list, a bare list, or a numeric `1 --> 10` range;
//! `y-axis` may carry a label and/or an explicit range. Unparseable lines are
//! skipped; nothing plottable degrades to the shared honest placeholder.
//! Every axis number and bar height is integer fixed-point so a snapshot is
//! deterministic.

use rstui_core::{Buffer, Rect, Style};

use super::MermaidTheme;
use super::draw::Surface;

/// One named numeric data series and whether it draws as bars or as a line.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Series {
    /// `true` for a `bar [...]`, `false` for a `line [...]`.
    is_bar: bool,
    /// The values × 100 (fixed-point) so fractional inputs stay exact.
    values: Vec<i64>,
}

/// A parsed `xychart-beta`: title, category labels, optional explicit y-range
/// (×100), and the series in source order.
#[derive(Debug, Default, PartialEq, Eq)]
struct Chart {
    /// The `title "…"` text, if any.
    title: Option<String>,
    /// The x-axis category labels (synthesised for a numeric range).
    categories: Vec<String>,
    /// An explicit `y-axis lo --> hi` as `(lo×100, hi×100)`, if given.
    y_range: Option<(i64, i64)>,
    /// The bar/line series.
    series: Vec<Series>,
}

/// Drops Mermaid preamble noise and the `xychart-beta[ horizontal]` header,
/// returning the significant body lines trimmed.
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

/// Parses a number into fixed-point ×100, rounding half-up. Rejects garbage.
fn parse_fp(s: &str) -> Option<i64> {
    let s = s.trim();
    let (neg, body) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    if body.is_empty() {
        return None;
    }
    let (int_part, frac_part) = match body.split_once('.') {
        Some((i, f)) => (i, f),
        None => (body, ""),
    };
    let int_v: i64 = if int_part.is_empty() {
        0
    } else {
        int_part.parse().ok()?
    };
    if !frac_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut frac = frac_part.chars();
    let d0 = frac.next().and_then(|c| c.to_digit(10)).unwrap_or(0) as i64;
    let d1 = frac.next().and_then(|c| c.to_digit(10)).unwrap_or(0) as i64;
    let round = i64::from(frac.next().and_then(|c| c.to_digit(10)).unwrap_or(0) >= 5);
    let mag = int_v.checked_mul(100)? + d0 * 10 + d1 + round;
    Some(if neg { -mag } else { mag })
}

/// Splits a `[a, b, c]` bracketed list into its trimmed, unquoted items.
fn parse_list(s: &str) -> Vec<String> {
    let s = s.trim();
    let inner = s
        .strip_prefix('[')
        .and_then(|r| r.strip_suffix(']'))
        .unwrap_or(s);
    inner
        .split(',')
        .map(|t| t.trim().trim_matches('"').trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Strips a leading optional `"quoted label"` off an axis line, returning
/// `(label, rest)` where `rest` is what follows the label.
fn split_quoted(s: &str) -> (Option<String>, &str) {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix('"') {
        if let Some(end) = rest.find('"') {
            return (Some(rest[..end].to_string()), rest[end + 1..].trim());
        }
    }
    (None, s)
}

/// Parses a whole `xychart-beta` source into a [`Chart`], skipping bad lines.
fn parse(src: &str) -> Chart {
    let mut chart = Chart::default();
    for line in clean_lines(src) {
        if let Some(rest) = line.strip_prefix("title") {
            let (q, plain) = split_quoted(rest);
            chart.title = Some(q.unwrap_or_else(|| plain.trim().to_string()));
            continue;
        }
        if let Some(rest) = line.strip_prefix("x-axis") {
            let (_label, body) = split_quoted(rest);
            if let Some((a, b)) = body.split_once("-->") {
                // Numeric range → synthesise integer category labels.
                if let (Some(lo), Some(hi)) = (parse_fp(a), parse_fp(b)) {
                    let (lo, hi) = (lo / 100, hi / 100);
                    if hi >= lo && hi - lo < 64 {
                        chart.categories = (lo..=hi).map(|v| v.to_string()).collect();
                    }
                    continue;
                }
            }
            chart.categories = parse_list(body);
            continue;
        }
        if let Some(rest) = line.strip_prefix("y-axis") {
            let (_label, body) = split_quoted(rest);
            if let Some((a, b)) = body.split_once("-->") {
                if let (Some(lo), Some(hi)) = (parse_fp(a), parse_fp(b)) {
                    chart.y_range = Some((lo.min(hi), lo.max(hi)));
                }
            }
            continue;
        }
        let is_bar = line.starts_with("bar");
        let is_line = line.starts_with("line");
        if is_bar || is_line {
            let body = if is_bar { &line[3..] } else { &line[4..] };
            let values: Vec<i64> = parse_list(body)
                .iter()
                .filter_map(|t| parse_fp(t))
                .collect();
            if !values.is_empty() {
                chart.series.push(Series { is_bar, values });
            }
        }
    }
    chart
}

/// The 8-step vertical block ramp (`▁…█`), the house bar glyphs shared with
/// [`crate::BarChart`]/[`crate::Sparkline`].
const VBARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Formats a fixed-point ×100 value compactly for an axis tick (drops a zero
/// fraction; one decimal otherwise).
fn fmt_fp(v: i64) -> String {
    let whole = v / 100;
    let frac = (v % 100).abs();
    if frac == 0 {
        format!("{whole}")
    } else if frac % 10 == 0 {
        format!("{whole}.{}", frac / 10)
    } else {
        format!("{whole}.{frac:02}")
    }
}

/// Renders an `xychart-beta` Mermaid diagram from `src` into `area`.
///
/// Draws a framed plot: y tick labels and a left axis, x category labels and a
/// bottom axis, each `bar` series as proportional vertical block columns and
/// each `line` series as a connected path overlaid; a legend when more than
/// one series is present. The value scale is the explicit `y-axis` range or,
/// absent that, the data extent (clamped to include zero). Empty input draws
/// the shared placeholder; everything is integer-scaled and deterministic.
pub(crate) fn render(src: &str, area: Rect, buf: &mut Buffer, base: Style, theme: &MermaidTheme) {
    let chart = parse(src);
    if chart.series.is_empty() {
        super::diagram_placeholder("xychart", "no series", area, buf, base, theme);
        return;
    }

    let title_st = base.patch(theme.node_label);
    let axis_st = base.patch(theme.edge);
    let tick_st = base.patch(theme.edge_label);
    let legend_st = base.patch(theme.cluster);

    // The number of x slots = the widest series (categories pad/clip to it).
    let n = chart
        .series
        .iter()
        .map(|s| s.values.len())
        .max()
        .unwrap_or(0)
        .max(1);

    // Value range: explicit y-axis, else the data extent including zero.
    let (lo, mut hi) = chart.y_range.unwrap_or_else(|| {
        let mut mn = 0i64;
        let mut mx = 0i64;
        for s in &chart.series {
            for &v in &s.values {
                mn = mn.min(v);
                mx = mx.max(v);
            }
        }
        (mn, mx)
    });
    if hi <= lo {
        hi = lo + 100; // a flat series still gets a unit span
    }
    let span = hi - lo;

    let multi = chart.series.len() > 1;
    let legend_rows = i32::from(multi);

    let w = (area.width as i32).max(1);
    let h = (area.height as i32).max(1);
    let mut s = Surface::new(w, h);

    let mut top = 0;
    if let Some(t) = &chart.title {
        s.text_centered(0, 0, w, t, title_st);
        top = 2;
    }

    // Y tick labels need a gutter as wide as the widest formatted bound.
    let y_lab_w = fmt_fp(hi).chars().count().max(fmt_fp(lo).chars().count()) as i32;
    let axis_x = y_lab_w + 1; // axis sits just right of the labels
    let plot_l = axis_x + 1;
    let plot_r = w - 1;
    let plot_top = top;
    let bottom = h - 1 - legend_rows; // x-axis label row
    let plot_bot = bottom - 2; // value cells stop above the axis + label row
    let plot_h = plot_bot - plot_top + 1;
    let plot_w = plot_r - plot_l + 1;

    if plot_h < 1 || plot_w < 1 {
        // Area too small for a usable plot region — blit whatever fit (the
        // title) and bail; a clean clip, never a panic.
        s.blit(area, buf, base);
        return;
    }

    // Frame: left vertical axis + bottom horizontal axis (an "L").
    let axis_row = plot_bot + 1;
    s.vline(axis_x, plot_top, plot_h, '│', axis_st);
    s.hline(axis_x, axis_row, plot_w + 1, '─', axis_st);

    // Y ticks: hi at the top, lo at the axis row, one at the midpoint.
    let denom = (plot_h.max(1) - 1).max(1) as i64;
    for &ry in &[0, plot_h / 2, plot_h - 1] {
        let v = hi - span * ry as i64 / denom;
        let lab = fmt_fp(v);
        let lx = y_lab_w - lab.chars().count() as i32;
        s.text(lx.max(0), plot_top + ry, &lab, tick_st);
        s.set(axis_x, plot_top + ry, '┤', axis_st);
    }
    s.set(axis_x, axis_row, '└', axis_st);

    // Map a value to a row offset from the plot top (clamped into the plot).
    let val_to_row = |v: i64| -> i32 {
        let frac =
            ((v - lo) as i128 * (plot_h as i128 - 1) + span as i128 / 2) / span.max(1) as i128;
        (plot_h - 1 - frac as i32).clamp(0, plot_h - 1)
    };
    // Map a value to how many eighths of the column it fills from the axis.
    let val_to_eighths = |v: i64| -> i64 {
        let clamped = v.clamp(lo, hi);
        (((clamped - lo) as i128 * plot_h as i128 * 8 + span as i128 / 2) / span.max(1) as i128)
            as i64
    };

    // Each x slot gets an equal horizontal band.
    let slot_w = (plot_w / n as i32).max(1);

    // Bars first (so a line series overlays them).
    let bar_count = chart.series.iter().filter(|s| s.is_bar).count().max(1);
    for (bar_idx, ser) in chart.series.iter().filter(|s| s.is_bar).enumerate() {
        // Several bar series share a slot side by side.
        let sub_w = (slot_w / bar_count as i32).max(1);
        for (i, &v) in ser.values.iter().enumerate() {
            let col0 = plot_l + i as i32 * slot_w + bar_idx as i32 * sub_w;
            let e = val_to_eighths(v).max(0);
            let full = (e / 8) as i32;
            let rem = (e % 8) as usize;
            for bx in col0..(col0 + sub_w).min(plot_r + 1) {
                for fy in 0..full {
                    s.set(bx, plot_bot - fy, '█', axis_st);
                }
                if rem > 0 && full < plot_h {
                    s.set(bx, plot_bot - full, VBARS[rem - 1], axis_st);
                }
            }
        }
    }

    // Line series: a connected path through each value's cell.
    for ser in chart.series.iter().filter(|s| !s.is_bar) {
        let pt_x = |i: usize| plot_l + i as i32 * slot_w + slot_w / 2;
        for i in 0..ser.values.len() {
            let x = pt_x(i).min(plot_r);
            let y = plot_top + val_to_row(ser.values[i]);
            // Connect to the previous point with a coarse step fill so the
            // trend reads without sub-cell line glyphs.
            if i > 0 {
                let px = pt_x(i - 1).min(plot_r);
                let py = plot_top + val_to_row(ser.values[i - 1]);
                let steps = (x - px).max(1);
                for st in 1..steps {
                    let ix = px + st;
                    let iy = py + (y - py) * st / steps;
                    if s.glyph(ix, iy) == ' ' {
                        s.set(ix, iy, '·', tick_st);
                    }
                }
            }
            s.set(x, y, '●', tick_st);
        }
    }

    // X category labels centred under each slot (clipped to the slot width).
    for i in 0..n {
        let label = chart.categories.get(i).map(String::as_str).unwrap_or("");
        if label.is_empty() {
            continue;
        }
        let cx = plot_l + i as i32 * slot_w;
        s.text_centered(cx, axis_row + 1, slot_w, label, tick_st);
    }

    // Legend for multiple series.
    if multi {
        let mut lx = plot_l;
        for (k, ser) in chart.series.iter().enumerate() {
            let glyph = if ser.is_bar { '█' } else { '●' };
            let tag = format!("{} S{}", glyph, k + 1);
            s.text(lx, h - 1, &tag, legend_st);
            lx += tag.chars().count() as i32 + 2;
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
    fn parses_title_axes_and_series() {
        let c = parse(
            "xychart-beta\ntitle \"Rev\"\nx-axis \"M\" [jan, feb]\ny-axis \"$\" 0 --> 100\nbar [10, 20]\nline [5, 15]\n",
        );
        assert_eq!(c.title.as_deref(), Some("Rev"));
        assert_eq!(c.categories, vec!["jan", "feb"]);
        assert_eq!(c.y_range, Some((0, 10000)));
        assert_eq!(c.series.len(), 2);
        assert!(c.series[0].is_bar);
        assert!(!c.series[1].is_bar);
        assert_eq!(c.series[0].values, vec![1000, 2000]);
    }

    #[test]
    fn x_axis_numeric_range_synthesises_labels() {
        let c = parse("xychart-beta\nx-axis 1 --> 4\nbar [1,2,3,4]\n");
        assert_eq!(c.categories, vec!["1", "2", "3", "4"]);
    }

    #[test]
    fn bare_list_x_axis_and_no_y_range() {
        let c = parse("xychart-beta\nx-axis [a, b, c]\nline [1.5, 2.25, 3]\n");
        assert_eq!(c.categories, vec!["a", "b", "c"]);
        assert_eq!(c.y_range, None);
        assert_eq!(c.series[0].values, vec![150, 225, 300]);
    }

    #[test]
    fn horizontal_hint_and_comments_tolerated() {
        let c = parse("xychart-beta horizontal\n%% c\nbar [1, 2]\n");
        assert_eq!(c.series.len(), 1);
    }

    #[test]
    fn bad_lines_skipped_not_panic() {
        let c = parse("xychart-beta\ngarbage\nbar [x, y]\nbar [1, 2]\n");
        // `bar [x, y]` filters to empty and is dropped; only the valid one.
        assert_eq!(c.series.len(), 1);
        assert_eq!(c.series[0].values, vec![100, 200]);
    }

    // --- render snapshots --------------------------------------------------

    #[test]
    fn empty_chart_is_placeholder() {
        let out = lines("xychart-beta\n", 40, 6);
        assert!(out.contains("mermaid · xychart: no series"), "{out}");
    }

    #[test]
    fn single_bar_series_full_render_snapshot() {
        let out = lines(
            "xychart-beta\ntitle \"Q\"\nx-axis [a, b, c]\ny-axis 0 --> 100\nbar [25, 50, 100]\n",
            22,
            10,
        );
        // Title centred; y ticks 100/40/0 with `┤`; bars rise 25<50<100; the
        // bottom axis is an `└───` "L"; categories centred under each slot.
        let expected = [
            "          Q           ",
            "                      ",
            "100 ┤          █████  ",
            "    │          █████  ",
            "    │          █████  ",
            " 40 ┤     ██████████  ",
            "    │▄▄▄▄▄██████████  ",
            "  0 ┤███████████████  ",
            "    └─────────────────",
            "       a    b    c    ",
            "",
        ]
        .join("\n");
        assert_eq!(out, expected);
    }

    #[test]
    fn line_series_draws_markers() {
        let out = lines(
            "xychart-beta\nx-axis [a, b, c]\ny-axis 0 --> 30\nline [0, 30, 15]\n",
            20,
            8,
        );
        assert!(out.contains('●'), "expected line markers:\n{out}");
    }

    #[test]
    fn multi_series_has_legend() {
        let out = lines(
            "xychart-beta\nx-axis [a, b]\nbar [1, 2]\nline [2, 1]\n",
            24,
            9,
        );
        assert!(out.contains("S1"), "{out}");
        assert!(out.contains("S2"), "{out}");
    }

    #[test]
    fn auto_range_when_no_y_axis() {
        // No explicit y-axis: range is data extent incl. zero, top tick=8.
        let out = lines("xychart-beta\nx-axis [a, b]\nbar [4, 8]\n", 18, 7);
        assert!(out.contains('8'), "auto top tick should show 8:\n{out}");
    }

    #[test]
    fn tiny_area_does_not_panic() {
        let _ = lines("xychart-beta\nbar [1, 2, 3]\n", 4, 2);
        let _ = lines("xychart-beta\ntitle \"T\"\nbar [9]\n", 1, 1);
        let _ = lines("xychart-beta\nx-axis [a]\nline [5]\n", 6, 3);
    }
}
