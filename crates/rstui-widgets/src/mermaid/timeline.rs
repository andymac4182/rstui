//! `timeline` Mermaid diagram renderer — a deterministic horizontal timeline
//! drawn on the shared [`Surface`].
//!
//! # What it draws
//!
//! A Mermaid `timeline` source is a `title`, optional `section` group
//! headers, and a sequence of *period* lines. A period line is
//! `<period> : <event> [: <event> …]`; a line that is *only* `: <event>`
//! continues the previous period (appending another event). This renderer
//! parses that into an ordered list of periods (each with its events and the
//! section it falls under), then lays them left→right along a central
//! horizontal rule: a tick per period on the rule, the period label just under
//! the tick, the period's events stacked as small boxes below that, and any
//! `section` name as a band label spanning its periods above the rule. The
//! title is centred on the top row.
//!
//! # Deterministic layout
//!
//! Periods are evenly spaced across the available width by integer division
//! (`column = margin + i * span / (n - 1)` for `n > 1`, else centred), so the
//! image is a pure function of the source and the area — perfectly
//! snapshot-testable. Event boxes always stack *downward* under their period
//! (a single, stable rule rather than an alternating above/below scheme) so
//! two timelines with the same data render identically regardless of how they
//! were built.
//!
//! # Leniency & totality
//!
//! Unparseable lines are skipped; a continuation with no preceding period is
//! dropped; a source with no periods falls back to the shared
//! [`super::diagram_placeholder`]. All drawing clips silently in the
//! [`Surface`], so a tiny area degrades rather than panicking.

use rstui_core::{Buffer, Rect, Style};

use super::MermaidTheme;
use super::draw::{BoxStyle, Surface};

/// One time period: its label, the section it belongs to, and its events in
/// declaration order (continuation lines append here).
#[derive(Debug, Clone)]
struct Period {
    /// The 0-based section index this period falls under.
    section: usize,
    /// The period label (e.g. `2002`, `Q1`, `Day 1`).
    label: String,
    /// The events listed for this period, in order.
    events: Vec<String>,
}

/// The parsed model of a `timeline` source: a title, the ordered section
/// names, and the ordered periods (each carrying its section index).
#[derive(Debug, Default)]
struct Timeline {
    /// The `title` text, if any.
    title: Option<String>,
    /// Section names in declaration order; index is [`Period::section`].
    sections: Vec<String>,
    /// Periods in declaration order.
    periods: Vec<Period>,
}

/// Parses a whole `timeline` source into a [`Timeline`] model, leniently.
///
/// The line parser is hand-written: split on `\n`, strip a trailing `\r`, drop
/// a leading `--- … ---` frontmatter block and any `%%`/`%%{}%%` directive,
/// skip the `timeline` header, then trim. A `title …` sets the title; a
/// `section …` opens a new group; a `period : a : b` line adds a period with
/// events; a leading-colon line (`: more`) appends events to the last period.
/// Anything else is skipped.
fn parse(src: &str) -> Timeline {
    let mut t = Timeline::default();
    let mut in_front = false;
    let mut seen_header = false;
    // Whether the header line was literally the `timeline` keyword. A bare
    // single-word period (no `:`) is only accepted inside a real timeline, so
    // an unrelated stray word does not masquerade as a one-period diagram.
    let mut real_header = false;
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
            if line == "timeline" || line.starts_with("timeline ") {
                seen_header = true;
                real_header = true;
                continue;
            }
            seen_header = true;
        }

        if let Some(rest) = line.strip_prefix("title ") {
            t.title = Some(rest.trim().to_owned());
            continue;
        }
        if line == "title" {
            t.title = Some(String::new());
            continue;
        }
        if let Some(rest) = line.strip_prefix("section ") {
            t.sections.push(rest.trim().to_owned());
            cur_section = Some(t.sections.len() - 1);
            continue;
        }

        // A continuation line: `: event [: event …]` appends to the last
        // period. Dropped if there is no period yet.
        if let Some(rest) = line.strip_prefix(':') {
            if let Some(p) = t.periods.last_mut() {
                for ev in rest.split(':') {
                    let ev = ev.trim();
                    if !ev.is_empty() {
                        p.events.push(ev.to_owned());
                    }
                }
            }
            continue;
        }

        // A period line: `period : event [: event …]`.
        let Some((label, evs)) = line.split_once(':') else {
            // A bare token with no events is a valid (empty) period only
            // inside a real timeline; otherwise a stray word is skipped.
            let label = line.trim();
            if label.is_empty() || !real_header {
                continue;
            }
            let section = ensure_section(&mut t, &mut cur_section);
            t.periods.push(Period {
                section,
                label: label.to_owned(),
                events: Vec::new(),
            });
            continue;
        };
        let label = label.trim();
        if label.is_empty() {
            continue;
        }
        let section = ensure_section(&mut t, &mut cur_section);
        let events: Vec<String> = evs
            .split(':')
            .map(str::trim)
            .filter(|e| !e.is_empty())
            .map(str::to_owned)
            .collect();
        t.periods.push(Period {
            section,
            label: label.to_owned(),
            events,
        });
    }

    t
}

/// Returns the current section index, synthesising a single unnamed section
/// the first time a period appears outside any explicit `section`.
fn ensure_section(t: &mut Timeline, cur: &mut Option<usize>) -> usize {
    match *cur {
        Some(s) => s,
        None => {
            if t.sections.is_empty() {
                t.sections.push(String::new());
            }
            let s = t.sections.len() - 1;
            *cur = Some(s);
            s
        }
    }
}

/// Renders a `timeline` Mermaid diagram from `src` into `area`.
pub(crate) fn render(src: &str, area: Rect, buf: &mut Buffer, base: Style, theme: &MermaidTheme) {
    let t = parse(src);
    if t.periods.is_empty() {
        super::diagram_placeholder("timeline", "no periods", area, buf, base, theme);
        return;
    }

    let w = area.width as i32;
    let h = area.height as i32;
    if w < 6 || h < 4 {
        super::diagram_placeholder("timeline", "area too small", area, buf, base, theme);
        return;
    }

    let rule_st = base.patch(theme.edge);
    let period_st = base.patch(theme.edge_label);
    let box_border = base.patch(theme.node_border);
    let box_text = base.patch(theme.node_label);
    let section_st = base.patch(theme.cluster);
    let title_st = base.patch(theme.cluster);

    let mut s = Surface::new(w, h);

    // Top: centred title (one row, when present and non-empty).
    let mut top = 0;
    if let Some(tt) = &t.title {
        if !tt.is_empty() {
            s.text_centered(0, top, w, tt, title_st);
            top += 1;
        }
    }

    // Column centre for period `i`. With one period it sits in the middle;
    // otherwise periods are evenly spread between a left and right margin so
    // the first/last event boxes do not run off the edge.
    let n = t.periods.len() as i32;
    let margin = 6.min(w / 4);
    let span = (w - 1 - 2 * margin).max(1);
    let center = |i: i32| -> i32 {
        if n == 1 {
            w / 2
        } else {
            margin + (i * span) / (n - 1)
        }
    };
    // The horizontal slot a period owns, used to clip its label/boxes.
    let slot = if n == 1 { w } else { (span / (n - 1)).max(3) };

    // Section band row (just below the title): each section's name centred
    // over the span of its periods.
    let band_y = top;
    let mut has_named_section = false;
    {
        let mut i = 0usize;
        while i < t.periods.len() {
            let sec = t.periods[i].section;
            let mut j = i;
            while j < t.periods.len() && t.periods[j].section == sec {
                j += 1;
            }
            let name = &t.sections[sec];
            if !name.is_empty() {
                has_named_section = true;
                let x0 = center(i as i32);
                let x1 = center(j as i32 - 1);
                let bw = (x1 - x0 + 1).max(1);
                s.text_centered(x0, band_y, bw, name, section_st);
            }
            i = j;
        }
    }
    let rule_y = top + i32::from(has_named_section);

    // The central rule with a tick under each period and the period label
    // beneath the tick.
    s.hline(0, rule_y, w, '─', rule_st);
    for (i, p) in t.periods.iter().enumerate() {
        let cx = center(i as i32);
        s.set(cx, rule_y, '┬', rule_st);
        s.text_centered(cx - slot / 2, rule_y + 1, slot, &p.label, period_st);
    }

    // Event boxes stack downward beneath each period, starting two rows under
    // the rule (one row is the period label). The first ("headline") event of
    // a period is framed with a doubled border so it stands out from the
    // follow-on events, which use plain square boxes. A short connector stem
    // joins the rule tick to the first box and is drawn with a join glyph so
    // it reads as continuous with the rule it grows from.
    let bottom = s.height();
    let ev_top = rule_y + 2;
    for (i, p) in t.periods.iter().enumerate() {
        let cx = center(i as i32);
        let box_slot = if n == 1 {
            (s.width() - 2).max(3)
        } else {
            (slot - 1).clamp(3, 18)
        };
        // Connector stem from the tick down to the first event box.
        if !p.events.is_empty() && rule_y + 1 < bottom {
            let join = if s.glyph(cx, rule_y) == '┬' {
                '┬'
            } else {
                '│'
            };
            s.set(cx, rule_y, join, rule_st);
        }
        let mut by = ev_top;
        for (depth, ev) in p.events.iter().enumerate() {
            if by + 2 >= bottom {
                break;
            }
            let label_w = ev.chars().count() as i32 + 2;
            let bw = label_w.clamp(3, box_slot);
            let bx = cx - bw / 2;
            let kind = if depth == 0 {
                BoxStyle::Double
            } else {
                BoxStyle::Square
            };
            s.labeled_box(bx, by, bw, 3, kind, ev, box_border, box_text);
            by += 3;
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

    // --- parser ------------------------------------------------------------

    #[test]
    fn parses_title_and_simple_periods() {
        let src = "\
timeline
    title History of Social Media
    2002 : LinkedIn
    2004 : Facebook
    2005 : YouTube";
        let t = parse(src);
        assert_eq!(t.title.as_deref(), Some("History of Social Media"));
        assert_eq!(t.periods.len(), 3);
        assert_eq!(t.periods[0].label, "2002");
        assert_eq!(t.periods[0].events, vec!["LinkedIn"]);
        assert_eq!(t.periods[2].label, "2005");
    }

    #[test]
    fn multi_event_period_splits_on_colon() {
        let src = "timeline\n2004 : Facebook : Google";
        let t = parse(src);
        assert_eq!(t.periods.len(), 1);
        assert_eq!(t.periods[0].events, vec!["Facebook", "Google"]);
    }

    #[test]
    fn continuation_line_appends_to_previous_period() {
        let src = "\
timeline
    2002 : LinkedIn
         : Friendster
         : MySpace";
        let t = parse(src);
        assert_eq!(t.periods.len(), 1);
        assert_eq!(
            t.periods[0].events,
            vec!["LinkedIn", "Friendster", "MySpace"]
        );
    }

    #[test]
    fn continuation_with_no_period_is_dropped() {
        let t = parse("timeline\n: orphan event");
        assert!(t.periods.is_empty());
    }

    #[test]
    fn sections_group_consecutive_periods() {
        let src = "\
timeline
    section 2000s
    2002 : LinkedIn
    2005 : YouTube
    section 2010s
    2010 : Pinterest";
        let t = parse(src);
        assert_eq!(t.sections, vec!["2000s", "2010s"]);
        assert_eq!(t.periods[0].section, 0);
        assert_eq!(t.periods[1].section, 0);
        assert_eq!(t.periods[2].section, 1);
    }

    #[test]
    fn bare_period_without_events_is_kept() {
        let t = parse("timeline\n2002\n2003 : x");
        assert_eq!(t.periods.len(), 2);
        assert!(t.periods[0].events.is_empty());
        assert_eq!(t.periods[0].label, "2002");
    }

    #[test]
    fn frontmatter_and_comments_are_dropped() {
        let src = "\
---
title: ignored
---
%%{init: {'theme':'forest'}}%%
timeline
    %% a comment
    title Real
    2002 : A";
        let t = parse(src);
        assert_eq!(t.title.as_deref(), Some("Real"));
        assert_eq!(t.periods.len(), 1);
    }

    #[test]
    fn period_without_section_synthesises_default() {
        let t = parse("timeline\n2002 : A");
        assert_eq!(t.sections.len(), 1);
        assert_eq!(t.periods[0].section, 0);
    }

    // --- render snapshots --------------------------------------------------

    #[test]
    fn empty_source_renders_placeholder() {
        let out = lines("timeline", 40, 6);
        assert!(out.contains("timeline: no periods"), "got:\n{out}");
    }

    #[test]
    fn nothing_parseable_renders_placeholder() {
        let out = lines("garbage", 40, 6);
        assert!(out.contains("timeline: no periods"), "got:\n{out}");
    }

    #[test]
    fn tiny_area_renders_placeholder_not_panic() {
        let out = lines("timeline\n2002 : A", 5, 3);
        // No panic; a clipped placeholder of exactly the area's cells.
        assert_eq!(out.chars().filter(|&c| c != '\n').count(), 5 * 3);
        assert_eq!(out.matches('\n').count(), 3);
    }

    #[test]
    fn single_period_centers_and_renders_box() {
        let src = "timeline\ntitle T\n2002 : Hi";
        let out = lines(src, 21, 7);
        // The first ("headline") event of a period is framed with a doubled
        // border so it stands out from any follow-on events.
        assert_eq!(
            out,
            "          T          \n\
             ──────────┬──────────\n\
             \u{20}       2002         \n\
             \u{20}       ╔══╗         \n\
             \u{20}       ║Hi║         \n\
             \u{20}       ╚══╝         \n\
             \u{20}                    \n"
        );
    }

    #[test]
    fn two_periods_spread_with_ticks_and_labels() {
        let src = "\
timeline
2002 : A
2010 : B";
        let out = lines(src, 24, 6);
        // Two ticks on the rule, period labels under them, a headline box per
        // period (doubled border, as the first event of each period).
        assert!(out.lines().next().unwrap().matches('┬').count() == 2);
        assert!(out.contains("2002") && out.contains("2010"));
        assert!(out.contains("║A║") && out.contains("║B║"));
    }

    #[test]
    fn section_band_labels_above_the_rule() {
        let src = "\
timeline
section Era
2002 : A
2010 : B";
        let out = lines(src, 24, 7);
        // The section name sits on the band row above the rule.
        let first_line = out.lines().next().unwrap();
        assert!(first_line.contains("Era"), "band missing:\n{out}");
        // The rule (with ticks) is the row directly under the band.
        assert!(out.lines().nth(1).unwrap().contains('┬'));
    }

    #[test]
    fn multiple_events_stack_downward() {
        let src = "\
timeline
2002 : First : Second";
        let out = lines(src, 20, 9);
        // Two boxes, the second strictly below the first under one period.
        let first_box = out.find("First").unwrap();
        let second_box = out.find("Second").unwrap();
        assert!(first_box < second_box);
        // Both boxes are around the single centred period tick.
        assert!(out.contains('┬'));
    }

    #[test]
    fn many_events_clip_at_the_bottom_without_panic() {
        let mut src = String::from("timeline\n2002 : E0\n");
        for i in 1..30 {
            src.push_str(&format!(": E{i}\n"));
        }
        let out = lines(&src, 20, 7);
        assert_eq!(out.matches('\n').count(), 7);
    }

    #[test]
    fn three_sections_each_label_their_span() {
        let src = "\
timeline
section A
2000 : x
section B
2005 : y
section C
2010 : z";
        let out = lines(src, 36, 8);
        let band = out.lines().next().unwrap();
        assert!(band.contains('A') && band.contains('B') && band.contains('C'));
    }
}
