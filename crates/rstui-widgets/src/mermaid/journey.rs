//! `journey` Mermaid diagram renderer — a deterministic user-journey map
//! drawn on the shared [`Surface`].
//!
//! # What it draws
//!
//! A Mermaid `journey` source is a `title`, `section` headers, and tasks of
//! the form `Task name: <score>: <Actor>[, <Actor> …]` where `score` is
//! `0–5` and the actor list is optional. This renderer parses that into
//! sections each holding their tasks, then lays the sections out as
//! **labelled column groups left→right**. Within a section the tasks stack
//! downward; each task shows its name (clipped), a five-cell score meter
//! `●●●○○` (the score mapped to filled cells), and the actors' initials as a
//! chip (`AB`). A per-section average score is printed under each column as a
//! footer.
//!
//! # Deterministic layout
//!
//! Column widths are the available width divided evenly across the sections by
//! integer arithmetic, and every glyph is placed by integer math, so the image
//! is a pure function of the source and the area — fully snapshot-testable.
//! The score meter is a fixed five cells (`●` filled, `○` empty) rather than an
//! emoji face so each task occupies exactly one terminal cell per glyph with no
//! grapheme-width ambiguity (the same reasoning the sparkline/gauge use).
//!
//! # Leniency & totality
//!
//! A task line with no parseable score is skipped; a source with no tasks
//! falls back to the shared [`super::diagram_placeholder`]. All drawing clips
//! silently in the [`Surface`], so a tiny area degrades rather than panicking.

use rstui_core::{Buffer, Rect, Style};

use super::MermaidTheme;
use super::draw::Surface;

/// One journey step: its name, its `0..=5` satisfaction score, and the actors
/// involved (kept as parsed for the initials chip).
#[derive(Debug, Clone)]
struct Task {
    /// The 0-based section index this task falls under.
    section: usize,
    /// The step name shown in its column.
    name: String,
    /// The satisfaction score, clamped to `0..=5`.
    score: u8,
    /// The actors named after the score (may be empty).
    actors: Vec<String>,
}

/// The parsed model of a `journey` source: a title, the ordered section
/// names, and the ordered tasks (each carrying its section index).
#[derive(Debug, Default)]
struct Journey {
    /// The `title` text, if any.
    title: Option<String>,
    /// Section names in declaration order; index is [`Task::section`].
    sections: Vec<String>,
    /// Tasks in declaration order.
    tasks: Vec<Task>,
}

/// Parses a whole `journey` source into a [`Journey`] model, leniently.
///
/// The line parser is hand-written: split on `\n`, strip a trailing `\r`, drop
/// a leading `--- … ---` frontmatter block and any `%%`/`%%{}%%` directive,
/// skip the `journey` header, then trim. A `title …` sets the title; a
/// `section …` opens a new group; any other line is a task
/// `name : score [: actor, …]`. A line whose score does not parse is skipped.
fn parse(src: &str) -> Journey {
    let mut j = Journey::default();
    let mut in_front = false;
    let mut seen_header = false;
    let mut cur_section: Option<usize> = None;

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
        if line == "---" {
            in_front = !in_front;
            continue;
        }
        if in_front {
            continue;
        }
        if !seen_header {
            if line == "journey" || line.starts_with("journey ") {
                seen_header = true;
                continue;
            }
            seen_header = true;
        }

        if let Some(rest) = line.strip_prefix("title ") {
            j.title = Some(rest.trim().to_owned());
            continue;
        }
        if line == "title" {
            j.title = Some(String::new());
            continue;
        }
        if let Some(rest) = line.strip_prefix("section ") {
            j.sections.push(rest.trim().to_owned());
            cur_section = Some(j.sections.len() - 1);
            continue;
        }

        // A task: `name : score [: actor, …]`. The score field is required;
        // without it the line is not a task and is skipped.
        let mut parts = line.split(':');
        let Some(name) = parts.next() else { continue };
        let name = name.trim();
        let Some(score_tok) = parts.next() else {
            continue;
        };
        let Ok(raw_score) = score_tok.trim().parse::<i64>() else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let score = raw_score.clamp(0, 5) as u8;
        let actors: Vec<String> = parts
            .next()
            .map(|a| {
                a.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();

        let section = match cur_section {
            Some(s) => s,
            None => {
                if j.sections.is_empty() {
                    j.sections.push(String::new());
                }
                let s = j.sections.len() - 1;
                cur_section = Some(s);
                s
            }
        };
        j.tasks.push(Task {
            section,
            name: name.to_owned(),
            score,
            actors,
        });
    }

    j
}

/// The fixed five-cell score meter for `score` (`●` filled, `○` empty),
/// e.g. `3` → `●●●○○`. The score is already clamped to `0..=5`.
fn meter(score: u8) -> String {
    let filled = score.min(5) as usize;
    let mut m = String::with_capacity(5);
    for i in 0..5 {
        m.push(if i < filled { '●' } else { '○' });
    }
    m
}

/// The actors' initials joined into a short chip, e.g. `["Alice","Bob"]` →
/// `"AB"`. Empty when there are no actors.
fn initials(actors: &[String]) -> String {
    actors
        .iter()
        .filter_map(|a| a.chars().next())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Renders a `journey` Mermaid diagram from `src` into `area`.
pub(crate) fn render(src: &str, area: Rect, buf: &mut Buffer, base: Style, theme: &MermaidTheme) {
    let j = parse(src);
    if j.tasks.is_empty() {
        super::diagram_placeholder("journey", "no tasks", area, buf, base, theme);
        return;
    }

    let w = area.width as i32;
    let h = area.height as i32;
    if w < 8 || h < 5 {
        super::diagram_placeholder("journey", "area too small", area, buf, base, theme);
        return;
    }

    let title_st = base.patch(theme.cluster);
    let section_st = base.patch(theme.cluster);
    let name_st = base.patch(theme.node_label);
    let meter_st = base.patch(theme.node_border);
    let actor_st = base.patch(theme.edge_label);
    let axis_st = base.patch(theme.edge);

    let mut s = Surface::new(w, h);

    // Top: centred title (one row, when present and non-empty).
    let mut top = 0;
    if let Some(tt) = &j.title {
        if !tt.is_empty() {
            s.text_centered(0, top, w, tt, title_st);
            top += 1;
        }
    }

    // Sections lay out as equal-width columns left→right. Only sections that
    // actually contain a task get a column (an empty trailing `section` does
    // not waste space).
    let used: Vec<usize> = {
        let mut v: Vec<usize> = j.tasks.iter().map(|t| t.section).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let cols = used.len() as i32;
    let col_w = (w / cols).max(1);

    // A horizontal rule under the title separates the header from the columns.
    s.hline(0, top, w, '─', axis_st);
    let body_top = top + 1;

    for (ci, &sec) in used.iter().enumerate() {
        let cx = ci as i32 * col_w;
        let inner_w = col_w - 1; // leave a one-cell gutter between columns

        // Section header (column title), centred over the column.
        let name = &j.sections[sec];
        let header = if name.is_empty() {
            format!("· {}", sec + 1)
        } else {
            name.clone()
        };
        s.text_centered(cx, body_top, inner_w, &header, section_st);

        // Tasks of this section, stacked: name row then meter+chip row.
        let mut y = body_top + 1;
        let mut sum = 0u32;
        let mut count = 0u32;
        for t in j.tasks.iter().filter(|t| t.section == sec) {
            if y + 1 >= h {
                break;
            }
            s.text_clipped(cx, y, &t.name, inner_w, name_st);
            let m = meter(t.score);
            s.text(cx, y + 1, &m, meter_st);
            let chip = initials(&t.actors);
            if !chip.is_empty() {
                let cxx = cx + 6;
                s.text_clipped(cxx, y + 1, &chip, (inner_w - 6).max(0), actor_st);
            }
            sum += u32::from(t.score);
            count += 1;
            y += 2;
        }

        // Per-section average score footer on the bottom row of the surface.
        if count > 0 && h >= 2 {
            // Round-half-up average × 10 so we can show one decimal without
            // floats: avg10 = (sum*10 + count/2) / count.
            let avg10 = (sum * 10 + count / 2) / count;
            let footer = format!("avg {}.{}", avg10 / 10, avg10 % 10);
            s.text_clipped(cx, h - 1, &footer, inner_w, axis_st);
        }
    }

    // A faint divider between adjacent section columns makes the grouping
    // legible without a heavy box per column. `s.height()` is the canonical
    // bottom; a `┴` join ties each divider into the header rule, and `glyph`
    // is read back so the join only replaces the rule, never a stray cell.
    let bottom = s.height();
    for ci in 1..cols {
        let x = ci * col_w - 1;
        s.vline(x, body_top, (bottom - body_top).max(0), '│', axis_st);
        if s.glyph(x, top) == '─' {
            s.set(x, top, '┴', axis_st);
        }
    }

    s.blit(area, buf, base);
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

    // --- helpers -----------------------------------------------------------

    #[test]
    fn meter_maps_score_to_filled_cells() {
        assert_eq!(meter(0), "○○○○○");
        assert_eq!(meter(3), "●●●○○");
        assert_eq!(meter(5), "●●●●●");
        assert_eq!(meter(9), "●●●●●"); // already clamped upstream
    }

    #[test]
    fn initials_takes_first_letter_uppercased() {
        let a = vec!["alice".to_owned(), "Bob".to_owned()];
        assert_eq!(initials(&a), "AB");
        assert_eq!(initials(&[]), "");
    }

    // --- parser ------------------------------------------------------------

    #[test]
    fn parses_title_sections_and_tasks() {
        let src = "\
journey
    title My working day
    section Go to work
      Make tea: 5: Me
      Drive: 3: Me, Cat
    section Be at work
      Do work: 1: Me, Boss";
        let j = parse(src);
        assert_eq!(j.title.as_deref(), Some("My working day"));
        assert_eq!(j.sections, vec!["Go to work", "Be at work"]);
        assert_eq!(j.tasks.len(), 3);
        assert_eq!(j.tasks[0].name, "Make tea");
        assert_eq!(j.tasks[0].score, 5);
        assert_eq!(j.tasks[0].actors, vec!["Me"]);
        assert_eq!(j.tasks[1].actors, vec!["Me", "Cat"]);
        assert_eq!(j.tasks[2].section, 1);
    }

    #[test]
    fn score_without_actors_is_allowed() {
        let j = parse("journey\nsection S\nWalk: 4");
        assert_eq!(j.tasks.len(), 1);
        assert_eq!(j.tasks[0].score, 4);
        assert!(j.tasks[0].actors.is_empty());
    }

    #[test]
    fn score_is_clamped_to_zero_five() {
        let j = parse("journey\nsection S\nA: 9: X\nB: -3: Y");
        assert_eq!(j.tasks[0].score, 5);
        assert_eq!(j.tasks[1].score, 0);
    }

    #[test]
    fn task_without_a_numeric_score_is_skipped() {
        let src = "\
journey
section S
Good: 3: A
Bad: notanumber: B
Also bad with no colon
Good2: 2";
        let j = parse(src);
        assert_eq!(j.tasks.len(), 2);
        assert_eq!(j.tasks[0].name, "Good");
        assert_eq!(j.tasks[1].name, "Good2");
    }

    #[test]
    fn task_without_section_synthesises_default() {
        let j = parse("journey\nLone: 3: A");
        assert_eq!(j.sections.len(), 1);
        assert_eq!(j.tasks[0].section, 0);
    }

    #[test]
    fn frontmatter_and_comments_are_dropped() {
        let src = "\
---
title: ignored
---
%%{init: {}}%%
journey
    %% comment
    title Real
    section S
    T: 3: A";
        let j = parse(src);
        assert_eq!(j.title.as_deref(), Some("Real"));
        assert_eq!(j.tasks.len(), 1);
    }

    #[test]
    fn multiple_actors_split_on_comma() {
        let j = parse("journey\nsection S\nMeet: 4: Alice, Bob, Carol");
        assert_eq!(j.tasks[0].actors, vec!["Alice", "Bob", "Carol"]);
    }

    // --- render snapshots --------------------------------------------------

    #[test]
    fn empty_source_renders_placeholder() {
        let out = lines("journey", 40, 6);
        assert!(out.contains("journey: no tasks"), "got:\n{out}");
    }

    #[test]
    fn nothing_parseable_renders_placeholder() {
        let out = lines("not a journey", 40, 6);
        assert!(out.contains("journey: no tasks"), "got:\n{out}");
    }

    #[test]
    fn tiny_area_renders_placeholder_not_panic() {
        let out = lines("journey\nsection S\nT: 3: A", 6, 4);
        assert_eq!(out.chars().filter(|&c| c != '\n').count(), 6 * 4);
        assert_eq!(out.matches('\n').count(), 4);
    }

    #[test]
    fn single_section_single_task_snapshot() {
        let src = "\
journey
title Day
section Work
Tea: 5: Me";
        let out = lines(src, 16, 6);
        // The actor chip is the actors' *initials* ("Me" → "M"), placed after
        // the fixed five-cell meter with a one-cell gap.
        assert_eq!(
            out,
            "      Day       \n\
             ────────────────\n\
             \u{20}    Work       \n\
             Tea             \n\
             ●●●●● M         \n\
             avg 5.0         \n"
        );
    }

    #[test]
    fn two_sections_become_two_columns() {
        let src = "\
journey
section A
T1: 4: X
section B
T2: 2: Y";
        let out = lines(src, 24, 7);
        // A divider column between the two groups, both headers present.
        assert!(out.contains("┴"), "divider missing:\n{out}");
        assert!(out.contains('A') && out.contains('B'));
        assert!(out.contains("●●●●○"), "score-4 meter missing:\n{out}");
        assert!(out.contains("●●○○○"), "score-2 meter missing:\n{out}");
    }

    #[test]
    fn score_meter_and_actor_chip_render() {
        let src = "journey\nsection S\nStep: 3: Alice, Bob";
        let out = lines(src, 20, 6);
        assert!(out.contains("●●●○○"), "meter:\n{out}");
        assert!(out.contains("AB"), "chip:\n{out}");
    }

    #[test]
    fn per_section_average_is_shown() {
        // Scores 5 and 2 → average 3.5.
        let src = "\
journey
section S
A: 5: X
B: 2: Y";
        let out = lines(src, 18, 8);
        assert!(out.contains("avg 3.5"), "avg footer:\n{out}");
    }

    #[test]
    fn many_tasks_clip_without_panic() {
        let mut src = String::from("journey\nsection S\n");
        for i in 0..40 {
            src.push_str(&format!("Task{i}: 3: A\n"));
        }
        let out = lines(&src, 24, 6);
        assert_eq!(out.matches('\n').count(), 6);
    }

    #[test]
    fn three_sections_split_the_width_evenly() {
        let src = "\
journey
section One
A: 1: X
section Two
B: 2: Y
section Three
C: 3: Z";
        let out = lines(src, 30, 7);
        // Two dividers between three columns.
        assert_eq!(out.matches('┴').count(), 2);
    }

    #[test]
    fn unnamed_section_gets_a_synthetic_header() {
        // No `section` line at all → one synthetic column header "· 1".
        let out = lines("journey\nWalk: 4: A", 16, 6);
        assert!(out.contains("· 1"), "synthetic header:\n{out}");
    }
}
