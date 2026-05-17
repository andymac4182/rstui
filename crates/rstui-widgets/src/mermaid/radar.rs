//! `radar-beta` Mermaid diagram renderer.
//!
//! A spider / radar chart drawn on a character grid: a centre point, one spoke
//! per axis radiating outward with the axis name at the rim, concentric
//! graticule rings, and every `curve` plotted as a closed polygon whose
//! per-axis vertex sits at a radius proportional to that axis's value over the
//! configured `max`. Browser Mermaid uses a true circular plot with anti-
//! aliased strokes; a terminal has neither, so this is an honest integer
//! approximation — the spokes and polygon vertices are placed with a baked
//! 360-entry sine lookup (`cos θ = sin(θ+90°)`) scaled by 1000, so the layout
//! is byte-for-byte deterministic and snapshot-stable with no floating-point
//! anywhere on the render path.
//!
//! Grammar handled (a lenient subset of Mermaid's newer block grammar):
//!
//! ```text
//! radar-beta
//!     title "Skills"
//!     axis a["Speed"], b["Power"], c["Range"]
//!     curve hero["Hero"]{ 80, 50, 90 }
//!     curve foe{ a: 30, b: 95, c: 40 }
//!     max 100
//!     min 0
//!     ticks 4
//!     graticule polygon
//! ```
//!
//! `axis`/`curve` may appear once or many times and may be comma-joined on one
//! line. A curve body is either positional `{1, 2, 3}` (mapped to axes in
//! declaration order) or keyed `{ id: value, … }`. `max`/`min`/`ticks`/`title`/
//! `graticule` are optional. Unparseable lines are skipped; a radar with no
//! axes or no curve degrades to the shared honest placeholder.

use rstui_core::{Buffer, Rect, Style};

use super::MermaidTheme;
use super::draw::Surface;

/// One radar axis: a stable `id` used by keyed curve bodies and a display
/// `label` drawn at the rim (falls back to the id).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Axis {
    /// The identifier referenced by keyed `curve` bodies.
    id: String,
    /// The text drawn at the spoke's outer end.
    label: String,
}

/// One radar curve: a display `name` and one value per axis, already aligned
/// to the axis declaration order (a missing keyed entry is `0`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Curve {
    /// The legend / series name.
    name: String,
    /// One integer value per axis, in axis-declaration order.
    values: Vec<i64>,
}

/// A parsed `radar-beta`: title, axes, curves and the value bounds/tick count.
#[derive(Debug, PartialEq, Eq)]
struct Radar {
    /// The `title "…"` text, if any.
    title: Option<String>,
    /// The axes in declaration order.
    axes: Vec<Axis>,
    /// The curves in declaration order.
    curves: Vec<Curve>,
    /// The value mapped to the outer rim (`max N`, default `100`).
    max: i64,
    /// The value mapped to the centre (`min N`, default `0`).
    min: i64,
    /// The number of graticule rings (`ticks N`, default `4`).
    ticks: i64,
}

impl Default for Radar {
    fn default() -> Self {
        Self {
            title: None,
            axes: Vec::new(),
            curves: Vec::new(),
            max: 100,
            min: 0,
            ticks: 4,
        }
    }
}

/// `round(sin(d°) × 1000)` for `d` in `0..360` — the single deterministic
/// trig source for the whole renderer (`cos d° == SIN1000[(d+90)%360]`).
const SIN1000: [i32; 360] = [
    0, 17, 35, 52, 70, 87, 105, 122, 139, 156, 174, 191, 208, 225, 242, 259, 276, 292, 309, 326,
    342, 358, 375, 391, 407, 423, 438, 454, 469, 485, 500, 515, 530, 545, 559, 574, 588, 602, 616,
    629, 643, 656, 669, 682, 695, 707, 719, 731, 743, 755, 766, 777, 788, 799, 809, 819, 829, 839,
    848, 857, 866, 875, 883, 891, 899, 906, 914, 921, 927, 934, 940, 946, 951, 956, 961, 966, 970,
    974, 978, 982, 985, 988, 990, 993, 995, 996, 998, 999, 999, 1000, 1000, 1000, 999, 999, 998,
    996, 995, 993, 990, 988, 985, 982, 978, 974, 970, 966, 961, 956, 951, 946, 940, 934, 927, 921,
    914, 906, 899, 891, 883, 875, 866, 857, 848, 839, 829, 819, 809, 799, 788, 777, 766, 755, 743,
    731, 719, 707, 695, 682, 669, 656, 643, 629, 616, 602, 588, 574, 559, 545, 530, 515, 500, 485,
    469, 454, 438, 423, 407, 391, 375, 358, 342, 326, 309, 292, 276, 259, 242, 225, 208, 191, 174,
    156, 139, 122, 105, 87, 70, 52, 35, 17, 0, -17, -35, -52, -70, -87, -105, -122, -139, -156,
    -174, -191, -208, -225, -242, -259, -276, -292, -309, -326, -342, -358, -375, -391, -407, -423,
    -438, -454, -469, -485, -500, -515, -530, -545, -559, -574, -588, -602, -616, -629, -643, -656,
    -669, -682, -695, -707, -719, -731, -743, -755, -766, -777, -788, -799, -809, -819, -829, -839,
    -848, -857, -866, -875, -883, -891, -899, -906, -914, -921, -927, -934, -940, -946, -951, -956,
    -961, -966, -970, -974, -978, -982, -985, -988, -990, -993, -995, -996, -998, -999, -999,
    -1000, -1000, -1000, -999, -999, -998, -996, -995, -993, -990, -988, -985, -982, -978, -974,
    -970, -966, -961, -956, -951, -946, -940, -934, -927, -921, -914, -906, -899, -891, -883, -875,
    -866, -857, -848, -839, -829, -819, -809, -799, -788, -777, -766, -755, -743, -731, -719, -707,
    -695, -682, -669, -656, -643, -629, -616, -602, -588, -574, -559, -545, -530, -515, -500, -485,
    -469, -454, -438, -423, -407, -391, -375, -358, -342, -326, -309, -292, -276, -259, -242, -225,
    -208, -191, -174, -156, -139, -122, -105, -87, -70, -52, -35, -17,
];

/// `sin(d°) × 1000` for any integer degree (wraps modulo 360).
fn sin1000(deg: i32) -> i32 {
    SIN1000[deg.rem_euclid(360) as usize]
}

/// `cos(d°) × 1000` for any integer degree.
fn cos1000(deg: i32) -> i32 {
    sin1000(deg + 90)
}

/// Drops Mermaid preamble noise and the `radar-beta` header, returning the
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

/// Parses one `axis` declaration body (possibly several comma-joined items
/// like `a["A"], b["B"]`) into [`Axis`]es, appending to `axes`.
fn parse_axes(body: &str, axes: &mut Vec<Axis>) {
    for item in split_top_level(body) {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let (id, label) = match item.split_once('[') {
            Some((id, rest)) => {
                let lab = rest
                    .trim_end_matches(']')
                    .trim()
                    .trim_matches('"')
                    .to_string();
                (id.trim().to_string(), lab)
            }
            None => (item.to_string(), item.to_string()),
        };
        if id.is_empty() {
            continue;
        }
        let label = if label.is_empty() { id.clone() } else { label };
        axes.push(Axis { id, label });
    }
}

/// Parses one `curve` declaration into a [`Curve`] aligned to `axes`. The
/// head is `id` or `id["Name"]`; the `{ … }` body is positional (`{1,2,3}`)
/// or keyed (`{ a: 1, b: 2 }`). Returns `None` if there is no body.
fn parse_curve(body: &str, axes: &[Axis]) -> Option<Curve> {
    let brace = body.find('{')?;
    let head = body[..brace].trim();
    let inner = body[brace + 1..].trim_end().trim_end_matches('}').trim();

    let (id, disp) = match head.split_once('[') {
        Some((id, rest)) => {
            let n = rest
                .trim_end_matches(']')
                .trim()
                .trim_matches('"')
                .to_string();
            (id.trim().to_string(), n)
        }
        None => (head.to_string(), head.to_string()),
    };
    let name = if disp.is_empty() { id } else { disp };

    let mut values = vec![0i64; axes.len()];
    let parts = split_top_level(inner);
    let keyed = parts.iter().any(|p| p.contains(':'));
    if keyed {
        for p in parts {
            if let Some((k, v)) = p.split_once(':') {
                let k = k.trim();
                if let (Some(idx), Some(val)) =
                    (axes.iter().position(|a| a.id == k), parse_int(v.trim()))
                {
                    values[idx] = val;
                }
            }
        }
    } else {
        for (i, p) in parts.iter().enumerate() {
            if i >= values.len() {
                break;
            }
            if let Some(v) = parse_int(p.trim()) {
                values[i] = v;
            }
        }
    }
    Some(Curve { name, values })
}

/// Splits on top-level commas, ignoring commas nested inside `[]` or `{}` so
/// `a["x,y"], b` and `{1, 2}, {3, 4}` split correctly.
fn split_top_level(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for ch in s.chars() {
        match ch {
            '[' | '{' | '(' => {
                depth += 1;
                cur.push(ch);
            }
            ']' | '}' | ')' => {
                depth -= 1;
                cur.push(ch);
            }
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut cur));
            }
            _ => cur.push(ch),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// Parses a (possibly fractional) number, truncating toward zero to an int —
/// the radar grid has integer cells so sub-unit precision is not meaningful.
fn parse_int(s: &str) -> Option<i64> {
    let s = s.trim();
    let body = s.split_once('.').map(|(i, _)| i).unwrap_or(s);
    if body.is_empty() || body == "-" || body == "+" {
        return Some(0);
    }
    body.parse().ok()
}

/// Parses a whole `radar-beta` source into a [`Radar`], skipping bad lines.
fn parse(src: &str) -> Radar {
    let mut r = Radar::default();
    // Curves are buffered until every axis is known so positional/keyed
    // bodies align even when `curve` precedes a later `axis`.
    let mut raw_curves: Vec<String> = Vec::new();
    for line in clean_lines(src) {
        if let Some(rest) = line.strip_prefix("title") {
            let t = rest.trim().trim_matches('"').trim();
            r.title = Some(t.to_string());
        } else if let Some(rest) = line.strip_prefix("axis ") {
            parse_axes(rest, &mut r.axes);
        } else if let Some(rest) = line.strip_prefix("curve ") {
            raw_curves.push(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("max ") {
            if let Some(v) = parse_int(rest) {
                r.max = v;
            }
        } else if let Some(rest) = line.strip_prefix("min ") {
            if let Some(v) = parse_int(rest) {
                r.min = v;
            }
        } else if let Some(rest) = line.strip_prefix("ticks ") {
            if let Some(v) = parse_int(rest) {
                r.ticks = v.clamp(1, 12);
            }
        } else if line.starts_with("graticule") {
            // Parsed for grammar completeness; the terminal always draws a
            // polygonal graticule (a stepped "circle" reads the same).
        }
    }
    for rc in raw_curves {
        if let Some(c) = parse_curve(&rc, &r.axes) {
            r.curves.push(c);
        }
    }
    if r.max <= r.min {
        r.max = r.min + 1; // a degenerate range still maps without /0
    }
    r
}

/// Renders a `radar-beta` Mermaid diagram from `src` into `area`.
///
/// Draws a centred title, a spoke per axis with its label at the rim,
/// `ticks` concentric polygon graticule rings, and each curve as a closed
/// polygon (its vertices at `radius ∝ (value − min) / (max − min)`) marked
/// with a per-curve glyph plus a legend. All trig is the baked integer
/// [`SIN1000`] table so the image is deterministic. No axes / no curve draws
/// the shared placeholder; a tiny area clips cleanly rather than panicking.
pub(crate) fn render(src: &str, area: Rect, buf: &mut Buffer, base: Style, theme: &MermaidTheme) {
    let r = parse(src);
    if r.axes.is_empty() || r.curves.is_empty() {
        super::diagram_placeholder("radar", "no axes or curves", area, buf, base, theme);
        return;
    }

    let frame_st = base.patch(theme.node_border);
    let title_st = base.patch(theme.node_label);
    let grid_st = base.patch(theme.edge);
    let label_st = base.patch(theme.edge_label);
    let legend_st = base.patch(theme.cluster);

    let w = (area.width as i32).max(1);
    let h = (area.height as i32).max(1);
    let mut s = Surface::new(w, h);

    let mut top = 0;
    if let Some(t) = &r.title {
        s.text_centered(0, 0, w, t, title_st);
        top = 1;
    }

    // The plot region below the title, leaving a row for a legend.
    let legend_h = 1;
    let plot_top = top;
    let plot_bot = h - 1 - legend_h;
    let plot_h = (plot_bot - plot_top + 1).max(1);

    // Centre and the maximum drawable radius (terminal cells are ~2:1 tall,
    // so the x radius is doubled to keep the polygon visually round).
    let cx = w / 2;
    let cy = plot_top + plot_h / 2;
    // Leave a 1-cell margin so rim labels are not clipped at the edges.
    let rx = ((w / 2) - 2).max(1);
    let ry = ((plot_h / 2) - 1).max(1);

    if plot_h < 3 || w < 5 {
        // Too small for a readable radar — show whatever fit and clip.
        s.blit(area, buf, base);
        return;
    }

    let n = r.axes.len() as i32;
    // Angle of axis i: start at the top (−90°) and step clockwise so the
    // first axis points up like Mermaid's.
    let angle = |i: i32| -> i32 { -90 + 360 * i / n };

    // Map a (radius-fraction in tenths-of-permille, i.e. 0..1000) to a cell
    // offset from the centre along axis `i`.
    let project = |i: i32, perm: i64| -> (i32, i32) {
        let a = angle(i);
        let dx = cos1000(a) as i64 * rx as i64 * perm / 1_000_000;
        let dy = sin1000(a) as i64 * ry as i64 * perm / 1_000_000;
        (cx + dx as i32, cy + dy as i32)
    };

    // Graticule first (faintest layer): concentric polygons at each tick.
    for t in 1..=r.ticks {
        let perm = 1000 * t / r.ticks.max(1);
        let pts: Vec<(i32, i32)> = (0..n).map(|i| project(i, perm)).collect();
        for k in 0..n as usize {
            draw_seg(
                &mut s,
                pts[k],
                pts[(k + 1) % n as usize],
                '·',
                grid_st,
                false,
            );
        }
    }

    // Spokes over the graticule (they overwrite a `·` so the axes read), and
    // the axis label nudged just outside the rim.
    for i in 0..n {
        let (ex, ey) = project(i, 1000);
        draw_seg(&mut s, (cx, cy), (ex, ey), '·', grid_st, false);
        let label = &r.axes[i as usize].label;
        let lw = label.chars().count() as i32;
        // Place the label outside the rim point: left of it on the chart's
        // left half, right of it on the right half, centred when vertical.
        let lx = if ex < cx {
            ex - lw
        } else if ex > cx {
            ex + 1
        } else {
            ex - lw / 2
        };
        let ly = if ey < cy {
            ey - 1
        } else if ey > cy {
            ey + 1
        } else {
            ey
        };
        s.text_clipped(lx.max(0), ly, label, lw.min(w), label_st);
    }

    // Each curve as a closed polygon with a distinct marker glyph; polygon
    // edges overwrite the graticule so the data shape is unambiguous.
    const MARKERS: [char; 6] = ['●', '◆', '▲', '■', '★', '✦'];
    let span = (r.max - r.min).max(1);
    for (ci, curve) in r.curves.iter().enumerate() {
        let glyph = MARKERS[ci % MARKERS.len()];
        let pts: Vec<(i32, i32)> = (0..n)
            .map(|i| {
                let v = *curve.values.get(i as usize).unwrap_or(&0);
                let clamped = v.clamp(r.min, r.max);
                let perm = (clamped - r.min) * 1000 / span;
                project(i, perm)
            })
            .collect();
        for k in 0..n as usize {
            draw_seg(
                &mut s,
                pts[k],
                pts[(k + 1) % n as usize],
                '─',
                frame_st,
                true,
            );
        }
        for &(px, py) in &pts {
            s.set(px, py, glyph, frame_st);
        }
    }

    // The centre cross is drawn last so it is always visible on top of the
    // graticule, spokes, and any curve passing through the middle.
    s.set(cx, cy, '┼', frame_st);

    // Legend on the bottom row: marker + curve name, space-separated.
    let mut lx = 0;
    for (ci, curve) in r.curves.iter().enumerate() {
        let glyph = MARKERS[ci % MARKERS.len()];
        let tag = format!("{glyph} {}", curve.name);
        s.text(lx, h - 1, &tag, legend_st);
        lx += tag.chars().count() as i32 + 2;
        if lx >= w {
            break;
        }
    }

    s.blit(area, buf, base);
}

/// Draws a straight segment from `(x0, y0)` to `(x1, y1)` with `ch` using an
/// integer Bresenham walk. When `overwrite` is false an already-set non-blank
/// cell is left untouched (so a fainter layer never clobbers a stronger one);
/// when true the segment paints over everything (used for curve polygons so
/// the data shape is unambiguous).
fn draw_seg(
    s: &mut Surface,
    a: (i32, i32),
    b: (i32, i32),
    ch: char,
    style: Style,
    overwrite: bool,
) {
    let (x0, y0) = a;
    let (x1, y1) = b;
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let (mut x, mut y) = (x0, y0);
    loop {
        if overwrite || s.glyph(x, y) == ' ' {
            s.set(x, y, ch, style);
        }
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
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

    // --- deterministic trig ------------------------------------------------

    #[test]
    fn sine_table_is_exact_at_quadrants() {
        assert_eq!(sin1000(0), 0);
        assert_eq!(sin1000(90), 1000);
        assert_eq!(sin1000(180), 0);
        assert_eq!(sin1000(270), -1000);
        assert_eq!(cos1000(0), 1000);
        assert_eq!(cos1000(90), 0);
        // Wrapping is modular, never a panic.
        assert_eq!(sin1000(360), 0);
        assert_eq!(sin1000(-90), -1000);
        assert_eq!(sin1000(450), 1000);
    }

    // --- parser ------------------------------------------------------------

    #[test]
    fn parses_axes_curves_and_bounds() {
        let r = parse(
            "radar-beta\ntitle \"S\"\naxis a[\"Speed\"], b[\"Power\"]\ncurve h[\"Hero\"]{80, 50}\nmax 100\nmin 0\nticks 5\n",
        );
        assert_eq!(r.title.as_deref(), Some("S"));
        assert_eq!(r.axes.len(), 2);
        assert_eq!(r.axes[0].id, "a");
        assert_eq!(r.axes[0].label, "Speed");
        assert_eq!(r.curves.len(), 1);
        assert_eq!(r.curves[0].name, "Hero");
        assert_eq!(r.curves[0].values, vec![80, 50]);
        assert_eq!(r.max, 100);
        assert_eq!(r.ticks, 5);
    }

    #[test]
    fn keyed_curve_body_aligns_to_axis_ids() {
        let r = parse("radar-beta\naxis a, b, c\ncurve x{ c: 9, a: 3 }\n");
        assert_eq!(r.axes.len(), 3);
        // a=3, b=missing→0, c=9, in axis order.
        assert_eq!(r.curves[0].values, vec![3, 0, 9]);
    }

    #[test]
    fn curve_before_axis_still_aligns() {
        let r = parse("radar-beta\ncurve y{1, 2, 3}\naxis a, b, c\n");
        assert_eq!(r.curves[0].values, vec![1, 2, 3]);
    }

    #[test]
    fn bare_axis_id_used_as_its_own_label() {
        let r = parse("radar-beta\naxis alpha, beta\ncurve c{1,2}\n");
        assert_eq!(r.axes[0].label, "alpha");
        assert_eq!(r.axes[1].label, "beta");
    }

    #[test]
    fn bad_lines_skipped_not_panic() {
        let r = parse("radar-beta\ngibberish\naxis a, b\nnonsense {\ncurve c{1, 2}\n");
        assert_eq!(r.axes.len(), 2);
        assert_eq!(r.curves.len(), 1);
    }

    #[test]
    fn degenerate_bounds_do_not_divide_by_zero() {
        let r = parse("radar-beta\naxis a\ncurve c{5}\nmax 0\nmin 0\n");
        assert!(r.max > r.min);
    }

    // --- render snapshots --------------------------------------------------

    #[test]
    fn no_axes_is_placeholder() {
        let out = lines("radar-beta\ncurve c{1}\n", 40, 8);
        assert!(out.contains("mermaid · radar: no axes or curves"), "{out}");
    }

    #[test]
    fn no_curve_is_placeholder() {
        let out = lines("radar-beta\naxis a, b, c\n", 40, 8);
        assert!(out.contains("mermaid · radar: no axes or curves"), "{out}");
    }

    #[test]
    fn three_axis_radar_full_render_snapshot() {
        let out = lines(
            "radar-beta\ntitle \"Kit\"\naxis a[\"A\"], b[\"B\"], c[\"C\"]\ncurve s[\"S\"]{ 100, 50, 0 }\n",
            21,
            13,
        );
        // Deterministic byte-for-byte: title centred, three labelled spokes
        // ("A" up, "B" lower-right, "C" lower-left), the curve polygon and
        // graticule rings of `·`, a legend row. The exact glyphs come from
        // the baked integer sine table, so this never drifts.
        let expected = [
            "         Kit         ",
            "          A          ",
            "          ●          ",
            "         ·──         ",
            "        ··──·        ",
            "       ···─·─·       ",
            "      · ··┼── ·      ",
            "     ·······─●··     ",
            "    ·············    ",
            "   C             B   ",
            "                     ",
            "                     ",
            "● S                  ",
        ]
        .join("\n")
            + "\n";
        assert_eq!(out, expected);
    }

    #[test]
    fn multiple_curves_have_distinct_markers_and_legend() {
        let out = lines(
            "radar-beta\naxis a, b, c, d\ncurve one{10,20,30,40}\ncurve two{40,30,20,10}\n",
            30,
            14,
        );
        assert!(out.contains("one"), "legend one:\n{out}");
        assert!(out.contains("two"), "legend two:\n{out}");
        assert!(out.contains('●'), "curve 1 marker:\n{out}");
        assert!(out.contains('◆'), "curve 2 marker:\n{out}");
    }

    #[test]
    fn many_axes_stay_readable() {
        let src = "radar-beta\naxis a,b,c,d,e,f,g,h\ncurve c{1,2,3,4,5,6,7,8}\n";
        let out = lines(src, 40, 18);
        // Eight spokes must not panic and must still draw the centre.
        assert!(out.contains('┼'), "centre present:\n{out}");
    }

    #[test]
    fn tiny_area_does_not_panic() {
        let _ = lines("radar-beta\naxis a,b,c\ncurve c{1,2,3}\n", 4, 3);
        let _ = lines("radar-beta\naxis a\ncurve c{1}\n", 1, 1);
        let _ = lines("radar-beta\ntitle \"T\"\naxis a,b\ncurve c{1,2}\n", 6, 2);
    }
}
