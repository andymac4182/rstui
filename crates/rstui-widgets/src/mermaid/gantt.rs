//! `gantt` Mermaid diagram renderer — a deterministic, integer-only Gantt
//! chart drawn on the shared [`Surface`].
//!
//! # What it draws
//!
//! A Mermaid `gantt` source is a `title`, an optional `dateFormat`, a sequence
//! of `section` headers, and the tasks under each section. Every task has a
//! start (an explicit `YYYY-MM-DD` date or `after <id>` = the referenced
//! task's end), a duration (`Nd`/`Nw`/`Nh`), and optional status flags
//! (`done`/`active`/`crit`/`milestone`). This renderer parses that into a flat
//! list of `(section, label, start_day, end_day, flags)` rows, maps the global
//! minimum start day to column `0`, scales the day span to the available
//! width, and paints one horizontal bar per task row beneath a date header,
//! grouped under section headers in a left gutter.
//!
//! # Integer date math (no `chrono`)
//!
//! Dates are reduced to an absolute *day number* by a tiny proleptic-Gregorian
//! `ymd → days` function ([`days_from_civil`]). Only differences matter, so the
//! epoch is irrelevant; `Nd` adds days directly, `Nw` multiplies by 7, and
//! `Nh` is rounded up to whole days (the chart's granularity is one day). This
//! keeps the whole layout deterministic and dependency-free.
//!
//! # Leniency & totality
//!
//! Every parse step skips a line it cannot understand rather than failing, and
//! a source that yields no tasks falls back to the shared
//! [`super::diagram_placeholder`]. Rendering is pure arithmetic into a
//! [`Surface`] that silently clips out-of-bounds writes, so a tiny area
//! degrades to a clipped image and nothing ever panics.

use rstui_core::{Buffer, Rect, Style};

use super::MermaidTheme;
use super::draw::{BoxStyle, Surface};

/// Status flags that may precede a task id (`done,`/`active,`/`crit,`/
/// `milestone,`), captured as a small bitset so the renderer can pick a glyph
/// and style per task.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Flags {
    /// `done` — the task is complete (drawn with a lighter glyph).
    done: bool,
    /// `active` — the task is in progress (kept for completeness/leniency).
    active: bool,
    /// `crit` — a critical task (drawn with the emphasised label style).
    crit: bool,
    /// `milestone` — a zero-width point event (drawn as a single `◆`).
    milestone: bool,
}

/// One parsed task: its section, label, inclusive start/end day numbers, and
/// status [`Flags`]. `end` is `start` for a milestone (a point in time).
#[derive(Debug, Clone)]
struct Task {
    /// The 0-based index of the section this task belongs to.
    section: usize,
    /// The optional task id (used to resolve a later `after <id>`).
    id: String,
    /// The human label shown in the left gutter.
    label: String,
    /// The absolute start day number (see [`days_from_civil`]).
    start: i64,
    /// The absolute end day number, inclusive (`== start` for a milestone).
    end: i64,
    /// The parsed status flags.
    flags: Flags,
}

/// The parsed model of a `gantt` source: a title, the ordered section names,
/// and the ordered tasks (each carrying its section index).
#[derive(Debug, Default)]
struct Gantt {
    /// The `title` text, if any.
    title: Option<String>,
    /// Section names in declaration order; index is [`Task::section`].
    sections: Vec<String>,
    /// Tasks in declaration order.
    tasks: Vec<Task>,
}

/// Days since an arbitrary fixed epoch for a proleptic-Gregorian `y-m-d`.
///
/// This is the standard Howard Hinnant `days_from_civil` algorithm: only
/// differences between two results are ever used, so the chosen epoch
/// (0000-03-01 internally) does not matter. Returns `None` if the components
/// are out of range, so a malformed date skips its task rather than panicking.
fn days_from_civil(y: i64, m: i64, d: i64) -> Option<i64> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    Some(era * 146_097 + doe - 719_468)
}

/// Parses a `YYYY-MM-DD` (or `YYYY/MM/DD`) date into an absolute day number,
/// tolerating surrounding whitespace; `None` on any malformed component.
fn parse_date(s: &str) -> Option<i64> {
    let t = s.trim();
    let mut it = t.split(['-', '/']);
    let y: i64 = it.next()?.trim().parse().ok()?;
    let m: i64 = it.next()?.trim().parse().ok()?;
    let d: i64 = it.next()?.trim().parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    days_from_civil(y, m, d)
}

/// Parses a duration token (`5d`, `2w`, `36h`) into whole days (`>= 1`).
///
/// Weeks are `× 7`; hours are rounded *up* to whole days because the chart's
/// granularity is one day. A bare integer is treated as days. Anything else
/// yields `None` so the task is skipped.
fn parse_duration(s: &str) -> Option<i64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let (num, unit) = match t.chars().last() {
        Some(c) if c.is_ascii_alphabetic() => (&t[..t.len() - 1], c.to_ascii_lowercase()),
        _ => (t, 'd'),
    };
    let n: i64 = num.trim().parse().ok()?;
    if n <= 0 {
        return None;
    }
    Some(match unit {
        'd' => n,
        'w' => n * 7,
        'h' => (n + 23) / 24,
        // An unknown unit (e.g. minutes) falls back to day granularity rather
        // than rejecting the whole task.
        _ => n,
    })
}

/// Splits the leading status flags off a comma-separated field list, returning
/// the [`Flags`] and the remaining fields (id?, start, duration).
fn take_flags<'a>(fields: &'a [&'a str]) -> (Flags, &'a [&'a str]) {
    let mut flags = Flags::default();
    let mut i = 0;
    while i < fields.len() {
        match fields[i].trim() {
            "done" => flags.done = true,
            "active" => flags.active = true,
            "crit" => flags.crit = true,
            "milestone" => flags.milestone = true,
            _ => break,
        }
        i += 1;
    }
    (flags, &fields[i..])
}

/// Parses a whole `gantt` source into a [`Gantt`] model, leniently.
///
/// The line parser is hand-written: split on `\n`, strip a trailing `\r`, drop
/// a leading `--- … ---` frontmatter block and any `%%`/`%%{}%%` directive,
/// skip the `gantt` header, then trim. Each surviving line is a `title`,
/// `dateFormat`/`axisFormat`/`excludes`/`todayMarker` (recognised then
/// ignored), a `section`, or a task. Unrecognised or malformed lines are
/// skipped. A task with no resolvable start/duration is dropped.
fn parse(src: &str) -> Gantt {
    let mut g = Gantt::default();
    let mut in_front = false;
    let mut seen_header = false;
    let mut cur_section: Option<usize> = None;

    for raw in src.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        // Drop `%%{ ... }%%` directives and `%%` line comments wholesale.
        let line = match line.find("%%") {
            Some(i) => &line[..i],
            None => line,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // A leading `--- … ---` YAML frontmatter block is skipped entirely.
        if line == "---" {
            in_front = !in_front;
            continue;
        }
        if in_front {
            continue;
        }
        if !seen_header {
            // The first significant line is the `gantt` keyword; tolerate a
            // missing header by treating the first line as content only if it
            // is not literally `gantt`.
            if line == "gantt" || line.starts_with("gantt ") {
                seen_header = true;
                continue;
            }
            seen_header = true;
        }

        if let Some(rest) = line.strip_prefix("title ") {
            g.title = Some(rest.trim().to_owned());
            continue;
        }
        if line == "title" {
            g.title = Some(String::new());
            continue;
        }
        // Directives we recognise but intentionally ignore (the chart is
        // day-granular and has no calendar axis labels).
        const IGNORED: [&str; 5] = [
            "dateFormat",
            "axisFormat",
            "excludes",
            "todayMarker",
            "tickInterval",
        ];
        if IGNORED
            .iter()
            .any(|k| line == *k || line.starts_with(&format!("{k} ")))
        {
            continue;
        }
        if let Some(rest) = line.strip_prefix("section ") {
            let name = rest.trim().to_owned();
            g.sections.push(name);
            cur_section = Some(g.sections.len() - 1);
            continue;
        }

        // Otherwise: a task line `Label : <flags,> [id,] start, dur`.
        let Some((label, spec)) = line.split_once(':') else {
            continue;
        };
        let label = label.trim();
        if label.is_empty() {
            continue;
        }
        let fields: Vec<&str> = spec.split(',').map(str::trim).collect();
        let (mut flags, rest) = take_flags(&fields);
        // Resolve a start token (`after <id>` or an explicit date).
        let resolve_start = |g: &Gantt, tok: &str| -> Option<i64> {
            if let Some(after) = tok.strip_prefix("after ") {
                resolve_after(g, after.trim())
            } else {
                parse_date(tok)
            }
        };

        match rest.len() {
            // `[id,] start, dur` — the common form. The trailing field is a
            // duration or (rarely) an explicit end date; whatever precedes the
            // start field is the optional id.
            n if n >= 2 => {
                let dur_tok = rest[n - 1];
                let start_field = rest[n - 2];
                let id = if n >= 3 { rest[n - 3] } else { "" };
                let Some(start) = resolve_start(&g, start_field) else {
                    continue;
                };
                let end = if let Some(days) = parse_duration(dur_tok) {
                    start + days - 1
                } else if let Some(d) = parse_date(dur_tok) {
                    d.max(start)
                } else if flags.milestone {
                    // A milestone often carries a `0d`/`0` duration: a point.
                    start
                } else {
                    continue;
                };
                push_task(&mut g, cur_section, label, id, start, end, flags);
            }
            // A single field: a milestone date, or `after <id>` point.
            1 => {
                let Some(start) = resolve_start(&g, rest[0]) else {
                    continue;
                };
                flags.milestone = true;
                push_task(&mut g, cur_section, label, "", start, start, flags);
            }
            _ => {}
        }
    }

    g
}

/// Records a task under the current section (or a synthesised default one),
/// keeping declaration order.
fn push_task(
    g: &mut Gantt,
    cur_section: Option<usize>,
    label: &str,
    id: &str,
    start: i64,
    end: i64,
    flags: Flags,
) {
    let section = match cur_section {
        Some(s) => s,
        None => {
            if g.sections.is_empty() {
                g.sections.push(String::new());
            }
            g.sections.len() - 1
        }
    };
    g.tasks.push(Task {
        section,
        id: id.to_owned(),
        label: label.to_owned(),
        start,
        end: end.max(start),
        flags,
    });
}

/// Resolves `after <id>` to the referenced task's end day (`+1`, since a
/// dependent task starts the day *after* its predecessor finishes). Falls back
/// to the latest known end so a forward/typo reference still lays out.
fn resolve_after(g: &Gantt, id: &str) -> Option<i64> {
    if let Some(t) = g.tasks.iter().find(|t| t.id == id) {
        return Some(t.end + 1);
    }
    g.tasks.iter().map(|t| t.end).max().map(|e| e + 1)
}

/// The [`BoxStyle`] a milestone marker is framed with, encoding its status:
/// a critical milestone is a hard deadline (heavy), a completed one is settled
/// (double), an active one is in flight (round), and a plain one is square.
const fn milestone_box(f: &Flags) -> BoxStyle {
    if f.crit {
        BoxStyle::Heavy
    } else if f.done {
        BoxStyle::Double
    } else if f.active {
        BoxStyle::Round
    } else {
        BoxStyle::Square
    }
}

/// Renders a `gantt` Mermaid diagram from `src` into `area`.
pub(crate) fn render(src: &str, area: Rect, buf: &mut Buffer, base: Style, theme: &MermaidTheme) {
    let g = parse(src);
    if g.tasks.is_empty() {
        super::diagram_placeholder("gantt", "no tasks", area, buf, base, theme);
        return;
    }

    let w = area.width as i32;
    let h = area.height as i32;
    if w < 4 || h < 3 {
        super::diagram_placeholder("gantt", "area too small", area, buf, base, theme);
        return;
    }

    let border = base.patch(theme.node_border);
    let label_st = base.patch(theme.node_label);
    let axis = base.patch(theme.edge);
    let date_st = base.patch(theme.edge_label);
    let crit_st = base.patch(theme.edge_label);
    let section_st = base.patch(theme.cluster);

    // Day window: global min start → day 0, inclusive max end.
    let min_day = g.tasks.iter().map(|t| t.start).min().unwrap_or(0);
    let max_day = g.tasks.iter().map(|t| t.end).max().unwrap_or(min_day);
    let span_days = (max_day - min_day + 1).max(1);

    let mut s = Surface::new(w, h);

    // Row 0: centred title (if any). Row for the date header sits just above
    // the first task; tasks fill the remaining rows.
    let mut y = 0;
    if let Some(t) = &g.title {
        if !t.is_empty() {
            s.text_centered(0, y, w, t, section_st);
            y += 1;
        }
    }

    // Left gutter width: the widest section/task label, clamped so the chart
    // keeps at least a third of the width.
    let max_label = g
        .tasks
        .iter()
        .map(|t| t.label.chars().count())
        .chain(g.sections.iter().map(|s| s.chars().count() + 2))
        .max()
        .unwrap_or(0) as i32;
    let gutter = max_label.clamp(3, (w * 2 / 3).max(3)) + 1;
    let chart_x = gutter + 1;
    let chart_w = (w - chart_x).max(1);

    // Map an absolute day to a chart column (left edge), and to the inclusive
    // right column for an end day. Scale so the whole span fills `chart_w`.
    let col = |day: i64| -> i32 {
        let off = day - min_day;
        ((off * i64::from(chart_w)) / span_days) as i32
    };
    let col_end = |day: i64| -> i32 {
        let off = day - min_day + 1;
        (((off * i64::from(chart_w)) / span_days) as i32 - 1).max(col(day))
    };

    // Date header: a row of the window's start date (left) and end date
    // (right, when it fits), then a separate axis rule row beneath it. When
    // the area is too short to spare two rows for the header, the date row is
    // dropped and only the rule remains, so tasks still get space.
    let (sy, sm, sd) = civil_from_days(min_day);
    let (ey, em, ed) = civil_from_days(max_day);
    let start_lbl = format!("{sy:04}-{sm:02}-{sd:02}");
    let end_lbl = format!("{ey:04}-{em:02}-{ed:02}");
    if h - y >= 4 {
        s.text_clipped(chart_x, y, &start_lbl, chart_w, date_st);
        if chart_w as usize > end_lbl.chars().count() + start_lbl.chars().count() + 1 {
            let ex = chart_x + chart_w - end_lbl.chars().count() as i32;
            s.text(ex, y, &end_lbl, date_st);
        }
        y += 1;
    }
    s.hline(chart_x, y, chart_w, '─', axis);
    s.text_clipped(0, y, "days", gutter, axis);
    let rule_row = y;
    y += 1;

    // One row per task, grouped: emit a section header row whenever the
    // section changes, then the task bar. `s.height()` is the canonical
    // row-clip bound (the surface is exactly the area, but reading it back
    // keeps the bound and the grid in lock-step).
    let rows = s.height();
    let mut last_section: Option<usize> = None;
    for t in &g.tasks {
        if y >= rows {
            break;
        }
        if last_section != Some(t.section) {
            let name = &g.sections[t.section];
            if !name.is_empty() {
                s.text_clipped(0, y, &format!("▸ {name}"), gutter, section_st);
                y += 1;
                if y >= rows {
                    break;
                }
            }
            last_section = Some(t.section);
        }

        // Gutter label (indented one cell so it nests under the section).
        s.text_clipped(1, y, &t.label, gutter - 1, label_st);

        let bar_st = if t.flags.crit { crit_st } else { border };
        if t.flags.milestone {
            let cx = chart_x + col(t.start);
            // A milestone is a point event; emphasise it with a small box
            // whose border weight encodes its status (a critical milestone
            // is a hard deadline → heavy; a completed one → double; a plain
            // one → round) when the row has a free row above and below for
            // the box, else fall back to a bare diamond. If another task's
            // bar already occupies the cell, keep the diamond visible by
            // overlaying a cross join read back via `glyph`.
            let want_box = y - 1 > rule_row && y + 1 < rows && cx >= 1 && cx + 1 < s.width();
            if want_box {
                s.labeled_box(
                    cx - 1,
                    y - 1,
                    3,
                    3,
                    milestone_box(&t.flags),
                    "◆",
                    bar_st,
                    bar_st,
                );
            } else if s.glyph(cx, y) != ' ' {
                s.set(cx, y, '╪', bar_st);
            } else {
                s.set(cx, y, '◆', bar_st);
            }
        } else {
            let x0 = chart_x + col(t.start);
            let x1 = chart_x + col_end(t.end);
            let glyph = if t.flags.done { '▒' } else { '█' };
            let len = (x1 - x0 + 1).max(1);
            s.hline(x0, y, len, glyph, bar_st);
        }
        y += 1;
    }

    s.blit(area, buf, base);
}

/// The inverse of [`days_from_civil`]: an absolute day number back to
/// `(year, month, day)`. Used only to label the date header row.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Buffer, Position, Rect};

    /// Renders `src` into a fresh `width`×`height` buffer and returns the
    /// glyphs as one newline-terminated line per row (mirrors `mod.rs`'s
    /// `tests::lines` helper for snapshot assertions).
    fn lines(src: &str, width: u16, height: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        render(
            src,
            buf.area(),
            &mut buf,
            Style::new(),
            &MermaidTheme::default(),
        );
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    // --- date math ---------------------------------------------------------

    #[test]
    fn days_from_civil_round_trips() {
        for &(y, m, d) in &[
            (2024, 1, 1),
            (2024, 2, 29),
            (2024, 12, 31),
            (2000, 3, 1),
            (1999, 12, 31),
            (2023, 6, 15),
        ] {
            let z = days_from_civil(y, m, d).unwrap();
            assert_eq!(civil_from_days(z), (y, m, d), "round trip {y}-{m}-{d}");
        }
    }

    #[test]
    fn day_difference_is_calendar_correct() {
        let a = days_from_civil(2024, 1, 1).unwrap();
        let b = days_from_civil(2024, 1, 11).unwrap();
        assert_eq!(b - a, 10);
        // 2024 is a leap year: Jan 1 → Mar 1 is 31 + 29 = 60 days.
        let c = days_from_civil(2024, 3, 1).unwrap();
        assert_eq!(c - a, 60);
    }

    #[test]
    fn parse_date_rejects_garbage() {
        assert!(parse_date("2024-13-01").is_none());
        assert!(parse_date("2024-01").is_none());
        assert!(parse_date("nope").is_none());
        assert!(parse_date("2024-01-01-01").is_none());
        assert_eq!(parse_date(" 2024/01/02 "), days_from_civil(2024, 1, 2));
    }

    #[test]
    fn parse_duration_units() {
        assert_eq!(parse_duration("5d"), Some(5));
        assert_eq!(parse_duration("2w"), Some(14));
        assert_eq!(parse_duration("24h"), Some(1));
        assert_eq!(parse_duration("25h"), Some(2));
        assert_eq!(parse_duration("3"), Some(3));
        assert_eq!(parse_duration("0d"), None);
        assert_eq!(parse_duration("xd"), None);
        assert_eq!(parse_duration(""), None);
    }

    // --- parser ------------------------------------------------------------

    #[test]
    fn parses_title_sections_and_tasks() {
        let src = "\
gantt
    title My Plan
    dateFormat YYYY-MM-DD
    section Phase 1
    Design   :a1, 2024-01-01, 5d
    Build    :a2, after a1, 3d
    section Phase 2
    Ship     :milestone, m1, 2024-01-15, 0d";
        let g = parse(src);
        assert_eq!(g.title.as_deref(), Some("My Plan"));
        assert_eq!(g.sections, vec!["Phase 1", "Phase 2"]);
        assert_eq!(g.tasks.len(), 3);
        let a1 = &g.tasks[0];
        assert_eq!(a1.label, "Design");
        assert_eq!(a1.end - a1.start, 4); // 5 inclusive days
        let a2 = &g.tasks[1];
        assert_eq!(a2.start, a1.end + 1); // after a1
        assert_eq!(a2.end - a2.start, 2);
        assert!(g.tasks[2].flags.milestone);
        assert_eq!(g.tasks[2].section, 1);
    }

    #[test]
    fn status_flags_before_id_are_captured() {
        let src = "\
gantt
    section S
    Crit task :crit, done, c1, 2024-01-01, 2d
    Active    :active, a1, 2024-01-03, 1d";
        let g = parse(src);
        assert!(g.tasks[0].flags.crit && g.tasks[0].flags.done);
        assert_eq!(g.tasks[0].id, "c1");
        assert!(g.tasks[1].flags.active);
    }

    #[test]
    fn explicit_end_date_form_is_supported() {
        // `Task :start, end` with two dates.
        let src = "gantt\nsection S\nSpan :2024-01-01, 2024-01-10";
        let g = parse(src);
        assert_eq!(g.tasks.len(), 1);
        assert_eq!(g.tasks[0].end - g.tasks[0].start, 9);
    }

    #[test]
    fn bare_milestone_date_becomes_a_point() {
        let src = "gantt\nsection S\nKickoff :2024-06-01";
        let g = parse(src);
        assert_eq!(g.tasks.len(), 1);
        assert!(g.tasks[0].flags.milestone);
        assert_eq!(g.tasks[0].start, g.tasks[0].end);
    }

    #[test]
    fn frontmatter_and_comments_are_dropped() {
        let src = "\
---
title: ignored frontmatter
---
%%{init: {'theme':'dark'}}%%
gantt
    %% a comment
    title Real
    section S
    T :a, 2024-01-01, 1d";
        let g = parse(src);
        assert_eq!(g.title.as_deref(), Some("Real"));
        assert_eq!(g.tasks.len(), 1);
    }

    #[test]
    fn malformed_lines_are_skipped_not_panicked() {
        let src = "\
gantt
    section S
    Good :g, 2024-01-01, 2d
    Bad date :b, not-a-date, 3d
    Bad dur  :c, 2024-01-05, zz
    garbage with no colon
    Good2 :g2, 2024-01-10, 1d";
        let g = parse(src);
        assert_eq!(g.tasks.len(), 2);
        assert_eq!(g.tasks[0].label, "Good");
        assert_eq!(g.tasks[1].label, "Good2");
    }

    #[test]
    fn task_without_section_synthesises_a_default() {
        let src = "gantt\nLone :x, 2024-01-01, 1d";
        let g = parse(src);
        assert_eq!(g.tasks.len(), 1);
        assert_eq!(g.sections.len(), 1);
        assert_eq!(g.tasks[0].section, 0);
    }

    // --- render snapshots --------------------------------------------------

    #[test]
    fn empty_source_renders_placeholder() {
        let out = lines("gantt", 40, 5);
        assert!(out.contains("gantt: no tasks"), "got:\n{out}");
    }

    #[test]
    fn nothing_parseable_renders_placeholder() {
        let out = lines("not a diagram at all", 40, 5);
        assert!(out.contains("gantt: no tasks"), "got:\n{out}");
    }

    #[test]
    fn tiny_area_renders_placeholder_not_panic() {
        // 3×2 is below the chart minimum: an honest clipped placeholder, and
        // crucially no panic. Assert by *char* count (the clip appends `…`,
        // a multi-byte glyph, so a byte-length check would be wrong).
        let out = lines("gantt\nsection S\nT :a, 2024-01-01, 1d", 3, 2);
        assert_eq!(out.chars().filter(|&c| c != '\n').count(), 3 * 2);
        assert_eq!(out.matches('\n').count(), 2);
    }

    #[test]
    fn single_task_full_render_snapshot() {
        let src = "\
gantt
title Plan
section S
Design :a, 2024-01-01, 4d";
        let out = lines(src, 28, 6);
        assert_eq!(
            out,
            "            Plan            \n\
             \u{20}       2024-01-01          \n\
             days    ────────────────────\n\
             ▸ S                         \n\
             \u{20}Design ████████████████████\n\
             \u{20}                           \n"
        );
    }

    #[test]
    fn two_tasks_scale_across_the_window() {
        // Window is 2024-01-01 .. 2024-01-10 (10 days). Task A is days 0..4,
        // task B days 5..9, so the bars tile the chart with no overlap and
        // exactly fill the chart area to the right edge.
        let src = "\
gantt
section P
A :a, 2024-01-01, 5d
B :b, 2024-01-06, 5d";
        let out = lines(src, 30, 5);
        assert_eq!(
            out,
            "     2024-01-01     2024-01-10\n\
             days ─────────────────────────\n\
             ▸ P                           \n\
             \u{20}A   ████████████             \n\
             \u{20}B               █████████████\n"
        );
    }

    #[test]
    fn crit_done_and_milestone_glyphs() {
        let src = "\
gantt
section S
Done t :done, d, 2024-01-01, 5d
Crit t :crit, c, 2024-01-01, 5d
Mile   :milestone, m, 2024-01-05, 0d";
        let out = lines(src, 24, 6);
        // Done → ▒ glyph, crit → █ (styled, glyph still █), milestone → ◆.
        assert!(out.contains('▒'), "done bar missing:\n{out}");
        assert!(out.contains('◆'), "milestone missing:\n{out}");
    }

    #[test]
    fn after_dependency_offsets_the_bar() {
        let src = "\
gantt
section S
First  :f, 2024-01-01, 3d
Second :s, after f, 2d";
        let g = parse(src);
        // First spans days 0..2; Second starts day 3 (after f).
        assert_eq!(g.tasks[1].start - g.tasks[0].start, 3);
        // And it renders without panicking on a small area.
        let _ = lines(src, 20, 5);
    }

    #[test]
    fn section_header_groups_tasks() {
        let src = "\
gantt
section Alpha
T1 :t1, 2024-01-01, 2d
section Beta
T2 :t2, 2024-01-03, 2d";
        let out = lines(src, 26, 7);
        assert!(out.contains("▸ Alpha"), "got:\n{out}");
        assert!(out.contains("▸ Beta"), "got:\n{out}");
        // Beta's header appears on a row after Alpha's task.
        let a = out.find("Alpha").unwrap();
        let b = out.find("Beta").unwrap();
        assert!(a < b);
    }

    #[test]
    fn more_tasks_than_rows_clip_without_panic() {
        let mut src = String::from("gantt\nsection S\n");
        for i in 0..50 {
            src.push_str(&format!("Task{i} :x{i}, 2024-01-01, 1d\n"));
        }
        let out = lines(&src, 30, 6);
        assert_eq!(out.matches('\n').count(), 6);
    }
}
