//! `gitGraph` Mermaid diagram renderer.
//!
//! A Mermaid `gitGraph` is an ordered command script — `commit`, `branch`,
//! `checkout`/`switch`, `merge`, `cherry-pick` — replayed against a set of
//! branch *lanes*. `main` is lane 0; every `branch` creates the next lane in
//! declaration order; the "current" branch moves with `checkout`/`switch`.
//!
//! # Terminal projection
//!
//! Mermaid draws git graphs left-to-right (the default; `LR:` is explicit and
//! `TB:` is the vertical variant). This renderer always uses the **left→right
//! lane** form, the only one that stays legible in a character grid: each
//! branch is one horizontal lane row, labelled at the left margin, and every
//! commit is a node glyph at a deterministic column equal to its global
//! commit index. A `branch` drops a `┐`/`├` connector from the parent lane
//! down to the new lane at the branch column; a `merge` draws a connector
//! from the merged branch's tip back into the current lane and then a merge
//! commit node. Commit ids/tags print on the row directly above their node.
//!
//! Glyphs: `●` normal commit, `◉` `HIGHLIGHT`, `◌` `REVERSE`. Parsing is
//! lenient (an unknown or malformed command line is skipped); a script with
//! no commits renders the shared honest [`super::diagram_placeholder`].

use rstui_core::{Buffer, Rect, Style};

use super::MermaidTheme;
use super::draw::Surface;

/// A commit's render style, from its optional `type:` option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitType {
    /// `type:NORMAL` (the default) — a filled dot `●`.
    Normal,
    /// `type:REVERSE` — a hollow dot `◌` (a revert).
    Reverse,
    /// `type:HIGHLIGHT` — a fat dot `◉` (an emphasised commit).
    Highlight,
}

impl CommitType {
    /// The node glyph for this commit type.
    const fn glyph(self) -> char {
        match self {
            Self::Normal => '●',
            Self::Reverse => '◌',
            Self::Highlight => '◉',
        }
    }

    /// Parses a `type:` value (case-insensitive); unknown ⇒ `Normal`.
    fn parse(v: &str) -> Self {
        match v.trim().to_ascii_uppercase().as_str() {
            "REVERSE" => Self::Reverse,
            "HIGHLIGHT" => Self::Highlight,
            _ => Self::Normal,
        }
    }
}

/// One placed commit node on a lane.
#[derive(Debug, Clone)]
struct Commit {
    /// The lane (branch) this commit sits on.
    lane: usize,
    /// The global commit order index (also the render column rank).
    order: usize,
    /// The commit's display id (`id:"..."`, else an auto `c{n}`).
    id: String,
    /// An optional `tag:"..."`.
    tag: Option<String>,
    /// The commit glyph kind.
    kind: CommitType,
    /// `true` when this node is a merge commit (drawn with the merge join).
    is_merge: bool,
    /// For a merge commit, the lane the merge came *from*.
    merge_from: Option<usize>,
}

/// A branch lane: its display name, lane index (0 = `main`), the order index
/// of the commit it forked from, and the lane it forked off.
#[derive(Debug, Clone)]
struct Lane {
    /// The branch name as written.
    name: String,
    /// Global commit order at the moment the branch was created.
    start_order: usize,
    /// The lane this branch forked from (none for `main`).
    parent_lane: Option<usize>,
}

/// The whole parsed git graph: lanes in creation order and commits/merges in
/// global order.
#[derive(Debug, Default)]
struct GitGraph {
    /// Branch lanes, lane 0 first (`main`).
    lanes: Vec<Lane>,
    /// Every commit/merge node, in global order.
    commits: Vec<Commit>,
}

/// Strips `\r`, a trailing `%%` comment, and trims a raw line.
fn clean(raw: &str) -> &str {
    let no_cr = raw.strip_suffix('\r').unwrap_or(raw);
    let body = match no_cr.find("%%") {
        Some(i) => &no_cr[..i],
        None => no_cr,
    };
    body.trim()
}

/// Reads a `key:value` option token, honouring `"`-quoted values that may
/// contain spaces. Returns the value with quotes removed.
fn opt_value(tokens: &str, key: &str) -> Option<String> {
    // Scan for `key:` then take a quoted run or a bare word.
    let mut search = tokens;
    loop {
        let i = search.find(key)?;
        let after = &search[i + key.len()..];
        if let Some(rest) = after.strip_prefix(':') {
            let rest = rest.trim_start();
            if let Some(q) = rest.strip_prefix('"') {
                let end = q.find('"').unwrap_or(q.len());
                return Some(q[..end].to_owned());
            }
            let word: String = rest.chars().take_while(|c| !c.is_whitespace()).collect();
            if word.is_empty() {
                return None;
            }
            return Some(word);
        }
        // `key` matched mid-word; continue past it.
        search = &search[i + key.len()..];
    }
}

/// Parses a `gitGraph` script into a [`GitGraph`].
///
/// The first significant line is the header (`gitGraph`, optionally
/// `gitGraph LR:` / `gitGraph TB:` / `gitGraph:`); it is consumed. `main`
/// is pre-created as lane 0 and is the initial current branch. Unknown or
/// malformed command lines are skipped.
fn parse(src: &str) -> GitGraph {
    let mut g = GitGraph::default();
    g.lanes.push(Lane {
        name: "main".to_owned(),
        start_order: 0,
        parent_lane: None,
    });
    let mut current = 0usize; // lane index
    let mut order = 0usize;
    let mut auto = 0usize;
    let mut seen_header = false;

    // Lane lookup by name (kept in sync with `g.lanes`).
    let lane_of = |g: &GitGraph, name: &str| g.lanes.iter().position(|l| l.name == name);

    for raw in src.split('\n') {
        let line = clean(raw);
        if line.is_empty() {
            continue;
        }
        if !seen_header {
            // Header forms: `gitGraph`, `gitGraph:`, `gitGraph LR:`,
            // `gitGraph TB:`. Consume the first significant line if it
            // starts with the keyword; otherwise be lenient and treat it
            // as a command (it will simply be skipped if unknown).
            if line.starts_with("gitGraph") {
                seen_header = true;
                continue;
            }
            seen_header = true;
        }

        // Split the leading command word.
        let (cmd, rest) = match line.split_once(char::is_whitespace) {
            Some((c, r)) => (c, r.trim()),
            None => (line, ""),
        };
        match cmd {
            "commit" => {
                let kind = opt_value(rest, "type")
                    .map(|v| CommitType::parse(&v))
                    .unwrap_or(CommitType::Normal);
                let id = opt_value(rest, "id").unwrap_or_else(|| {
                    auto += 1;
                    format!("c{auto}")
                });
                let tag = opt_value(rest, "tag");
                g.commits.push(Commit {
                    lane: current,
                    order,
                    id,
                    tag,
                    kind,
                    is_merge: false,
                    merge_from: None,
                });
                order += 1;
            }
            "branch" => {
                let name = rest.split_whitespace().next().unwrap_or("");
                if name.is_empty() {
                    continue;
                }
                if lane_of(&g, name).is_none() {
                    g.lanes.push(Lane {
                        name: name.to_owned(),
                        start_order: order,
                        parent_lane: Some(current),
                    });
                }
                if let Some(l) = lane_of(&g, name) {
                    current = l;
                }
            }
            "checkout" | "switch" => {
                let name = rest.split_whitespace().next().unwrap_or("");
                if let Some(l) = lane_of(&g, name) {
                    current = l;
                }
            }
            "merge" => {
                let name = rest.split_whitespace().next().unwrap_or("");
                let Some(from) = lane_of(&g, name) else {
                    continue;
                };
                let tag = opt_value(rest, "tag");
                let kind = opt_value(rest, "type")
                    .map(|v| CommitType::parse(&v))
                    .unwrap_or(CommitType::Normal);
                auto += 1;
                g.commits.push(Commit {
                    lane: current,
                    order,
                    id: format!("m{auto}"),
                    tag,
                    kind,
                    is_merge: true,
                    merge_from: Some(from),
                });
                order += 1;
            }
            "cherry-pick" => {
                let id = opt_value(rest, "id").unwrap_or_else(|| {
                    auto += 1;
                    format!("p{auto}")
                });
                g.commits.push(Commit {
                    lane: current,
                    order,
                    id,
                    tag: None,
                    kind: CommitType::Normal,
                    is_merge: false,
                    merge_from: None,
                });
                order += 1;
            }
            _ => {} // unknown command: skip leniently
        }
    }
    g
}

/// Renders a `gitGraph` Mermaid diagram from `src` into `area`.
///
/// Lays out one horizontal lane row per branch (labelled at the left) and one
/// column per global commit order, draws every commit node, the branch-point
/// drops, and the merge joins, with ids/tags on the row above, then blits
/// once centred. A script with no commits falls back to the placeholder.
pub(crate) fn render(src: &str, area: Rect, buf: &mut Buffer, base: Style, theme: &MermaidTheme) {
    let g = parse(src);
    if g.commits.is_empty() {
        super::diagram_placeholder("gitGraph", "no commits", area, buf, base, theme);
        return;
    }

    let label_w = g
        .lanes
        .iter()
        .map(|l| l.name.chars().count() as i32)
        .max()
        .unwrap_or(4)
        .max(4);
    let n_orders = g.commits.iter().map(|c| c.order).max().unwrap_or(0) + 1;

    // Layout: label gutter, then each commit order is a 4-wide column.
    let gutter = label_w + 2; // name + " " + a separating space.
    let col_w = 4;
    let lane_gap = 2; // blank rows between lanes for tag/id text.
    let lane_y = |lane: usize| 1 + lane as i32 * lane_gap;

    let sw = gutter + n_orders as i32 * col_w + 2;
    let sh = lane_y(g.lanes.len().saturating_sub(1)) + 2;
    let mut s = Surface::new(sw.max(1), sh.max(1));
    // A degenerate (zero-area) layout cannot draw anything legible — fall
    // back to the honest placeholder instead of blitting an empty surface.
    if s.width() == 0 || s.height() == 0 {
        super::diagram_placeholder("gitGraph", "no commits", area, buf, base, theme);
        return;
    }

    let edge = base.patch(theme.edge);
    let node_st = base.patch(theme.node_border);
    let label_st = base.patch(theme.cluster);
    let tag_st = base.patch(theme.edge_label);

    // Column x for a given commit order.
    let col_x = |order: usize| gutter + order as i32 * col_w + 1;

    // The rightmost order this lane reaches (so its rule does not overrun).
    let mut lane_max_x = vec![gutter; g.lanes.len()];
    for c in &g.commits {
        let x = col_x(c.order);
        if x > lane_max_x[c.lane] {
            lane_max_x[c.lane] = x;
        }
    }
    // A lane with no commits of its own but used as a merge source still
    // needs its rule to reach the branch/merge column.
    for c in &g.commits {
        if let Some(from) = c.merge_from {
            let x = col_x(c.order);
            if x > lane_max_x[from] {
                lane_max_x[from] = x;
            }
        }
    }

    // The leftmost x a lane's rule should reach: its first own commit, else
    // its branch-point column (a lane used only as a merge source).
    let lane_start_x = |li: usize| -> i32 {
        g.commits
            .iter()
            .filter(|c| c.lane == li)
            .map(|c| col_x(c.order))
            .min()
            .unwrap_or_else(|| gutter + g.lanes[li].start_order as i32 * col_w + 1)
    };

    // 1) Lane rules + labels.
    for (li, (lane, &end_x)) in g.lanes.iter().zip(&lane_max_x).enumerate() {
        let y = lane_y(li);
        s.text(0, y, &lane.name, label_st);
        let start_x = lane_start_x(li);
        if end_x >= start_x {
            s.hline(start_x, y, end_x - start_x + 1, '─', edge);
        }
    }

    // A vertical connector segment from `y0+1`..`y1-1` at column `x`: a plain
    // `│`, but where it crosses an existing lane rule (`─`) the join becomes
    // a `┼` so a branch/merge that spans intervening lanes reads cleanly.
    // This is the `tree.rs`/flowchart line-join idiom, via `Surface::glyph`.
    let vjoin = |s: &mut Surface, x: i32, y0: i32, y1: i32| {
        let (top, bot) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
        for y in (top + 1)..bot {
            let g = if s.glyph(x, y) == '─' { '┼' } else { '│' };
            s.set(x, y, g, edge);
        }
    };

    // 2) Branch-point drops: parent lane → child lane at the branch column.
    for li in 0..g.lanes.len() {
        let Some(parent) = g.lanes[li].parent_lane else {
            continue;
        };
        let py = lane_y(parent);
        let cy = lane_y(li);
        // Branch column = one column left of the child's first commit.
        let bx = lane_start_x(li) - 1;
        vjoin(&mut s, bx, py, cy);
        // Tee off the parent rule, elbow into the child rule.
        s.set(bx, py, if py < cy { '┬' } else { '┴' }, edge);
        s.set(bx, cy, if py < cy { '╰' } else { '╭' }, edge);
    }

    // 3) Commit nodes + ids/tags + merge joins.
    for c in &g.commits {
        let x = col_x(c.order);
        let y = lane_y(c.lane);
        if let Some(from) = c.merge_from.filter(|_| c.is_merge) {
            let fy = lane_y(from);
            vjoin(&mut s, x, fy, y);
            s.set(x, fy, if fy < y { '┬' } else { '┴' }, edge);
        }
        s.set(x, y, c.kind.glyph(), node_st);
        // id above the node; tag (if any) above the id.
        let label = c.tag.as_deref().unwrap_or(&c.id);
        let lx = x - (label.chars().count() as i32 / 2);
        s.text(lx.max(0), y - 1, label, tag_st);
    }

    s.blit(area, buf, base);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::Position;

    /// Renders `src` as a `gitGraph` into a fresh `w`×`h` buffer; returns the
    /// glyphs as one newline-terminated line per row.
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
    fn main_lane_exists_and_commits_are_ordered() {
        let g = parse("gitGraph\ncommit\ncommit");
        assert_eq!(g.lanes.len(), 1);
        assert_eq!(g.lanes[0].name, "main");
        assert_eq!(g.commits.len(), 2);
        assert_eq!(g.commits[0].order, 0);
        assert_eq!(g.commits[1].order, 1);
        assert_eq!(g.commits[0].lane, 0);
    }

    #[test]
    fn commit_options_id_tag_type_parse() {
        let g = parse(
            "gitGraph\n\
             commit id:\"init\"\n\
             commit tag:\"v1.0\"\n\
             commit type:HIGHLIGHT\n\
             commit type:REVERSE",
        );
        assert_eq!(g.commits[0].id, "init");
        assert_eq!(g.commits[1].tag.as_deref(), Some("v1.0"));
        assert_eq!(g.commits[2].kind, CommitType::Highlight);
        assert_eq!(g.commits[3].kind, CommitType::Reverse);
    }

    #[test]
    fn branch_creates_lane_and_checkout_switches() {
        let g = parse(
            "gitGraph\n\
             commit\n\
             branch dev\n\
             commit\n\
             checkout main\n\
             commit",
        );
        assert_eq!(g.lanes.len(), 2);
        assert_eq!(g.lanes[1].name, "dev");
        assert_eq!(g.lanes[1].parent_lane, Some(0));
        // c0 on main, c1 on dev, c2 back on main.
        assert_eq!(g.commits[0].lane, 0);
        assert_eq!(g.commits[1].lane, 1);
        assert_eq!(g.commits[2].lane, 0);
    }

    #[test]
    fn switch_keyword_is_an_alias_for_checkout() {
        let g = parse(
            "gitGraph\n\
             branch dev\n\
             commit\n\
             switch main\n\
             commit",
        );
        assert_eq!(g.commits[0].lane, 1); // dev
        assert_eq!(g.commits[1].lane, 0); // main
    }

    #[test]
    fn merge_adds_a_merge_commit_on_the_current_lane() {
        let g = parse(
            "gitGraph\n\
             commit\n\
             branch dev\n\
             commit\n\
             checkout main\n\
             merge dev tag:\"rel\"",
        );
        let m = g.commits.last().unwrap();
        assert!(m.is_merge);
        assert_eq!(m.lane, 0); // merged into main
        assert_eq!(m.merge_from, Some(1)); // from dev
        assert_eq!(m.tag.as_deref(), Some("rel"));
    }

    #[test]
    fn cherry_pick_adds_a_commit_with_its_id() {
        let g = parse("gitGraph\ncommit id:\"A\"\ncherry-pick id:\"A\"");
        assert_eq!(g.commits.len(), 2);
        assert_eq!(g.commits[1].id, "A");
        assert!(!g.commits[1].is_merge);
    }

    #[test]
    fn header_variants_are_all_consumed() {
        for hdr in ["gitGraph", "gitGraph:", "gitGraph LR:", "gitGraph TB:"] {
            let g = parse(&format!("{hdr}\ncommit"));
            assert_eq!(g.commits.len(), 1, "header {hdr:?}");
        }
    }

    #[test]
    fn unknown_command_lines_are_skipped() {
        let g = parse("gitGraph\ncommit\nfrobnicate the foo\ncommit");
        assert_eq!(g.commits.len(), 2);
    }

    // --- render snapshot tests --------------------------------------------

    #[test]
    fn empty_source_renders_placeholder() {
        let out = lines("gitGraph\n", 40, 3);
        assert!(out.contains("mermaid"), "got:\n{out}");
        assert!(out.contains("gitGraph"), "got:\n{out}");
        assert!(out.contains("no commits"), "got:\n{out}");
    }

    #[test]
    fn header_only_is_a_placeholder() {
        let out = lines("gitGraph LR:\n%% nothing\n", 40, 3);
        assert!(out.contains("no commits"), "got:\n{out}");
    }

    #[test]
    fn linear_history_draws_a_main_lane_with_nodes() {
        let out = lines("gitGraph\ncommit\ncommit\ncommit", 24, 3);
        assert!(out.contains("main"), "got:\n{out}");
        assert!(out.contains('●'), "got:\n{out}");
        // Three commit dots on one lane.
        assert_eq!(out.matches('●').count(), 3, "got:\n{out}");
        // A horizontal lane rule connects them.
        assert!(out.contains('─'), "got:\n{out}");
    }

    #[test]
    fn commit_type_glyphs_render_distinctly() {
        let out = lines(
            "gitGraph\ncommit\ncommit type:HIGHLIGHT\ncommit type:REVERSE",
            24,
            3,
        );
        assert!(out.contains('●'), "normal dot, got:\n{out}");
        assert!(out.contains('◉'), "highlight dot, got:\n{out}");
        assert!(out.contains('◌'), "reverse dot, got:\n{out}");
    }

    #[test]
    fn tag_prints_above_its_commit() {
        let out = lines("gitGraph\ncommit tag:\"v1\"", 16, 4);
        assert!(out.contains("v1"), "got:\n{out}");
        // The tag is on a row above the node row.
        let rows: Vec<&str> = out.lines().collect();
        let node_row = rows.iter().position(|r| r.contains('●')).unwrap();
        assert!(node_row >= 1, "node should not be on row 0\n{out}");
        assert!(
            rows[node_row - 1].contains("v1"),
            "tag should be directly above node\n{out}"
        );
    }

    #[test]
    fn branch_then_merge_draws_a_second_lane_and_a_join() {
        let src = "gitGraph\n\
             commit\n\
             branch dev\n\
             commit\n\
             checkout main\n\
             merge dev";
        let out = lines(src, 32, 6);
        assert!(out.contains("main"), "got:\n{out}");
        assert!(out.contains("dev"), "got:\n{out}");
        // Two lanes ⇒ a vertical connector somewhere.
        assert!(out.contains('│'), "expected a lane connector\n{out}");
        // Branch-point elbow + merge tee glyphs.
        assert!(
            out.contains('╰') || out.contains('╭'),
            "expected a branch elbow\n{out}"
        );
    }

    #[test]
    fn exact_render_two_commit_main() {
        let out = lines("gitGraph\ncommit\ncommit", 16, 3);
        // Auto ids (`c1`, `c2`) print on the row above their nodes.
        let expected =
            ["      c1  c2    ", "main   ●───●    ", "                "].join("\n") + "\n";
        assert_eq!(out, expected, "got:\n{out}");
    }

    #[test]
    fn tiny_area_clips_without_panic() {
        let out = lines("gitGraph\ncommit\nbranch x\ncommit\nmerge x", 5, 2);
        assert_eq!(out.lines().count(), 2);
    }
}
