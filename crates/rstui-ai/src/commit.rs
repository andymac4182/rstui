//! [`Commit`] — a git commit card: the hash / message / author beat an
//! agent's VCS tools drop, with a collapsible changed-file list.
//!
//! # A pure projection of caller-owned commit data + `open`
//!
//! The ai-elements `Commit` is a card with the short hash, message, author,
//! relative time, and an expandable file list. Whether the file list is open
//! is ordinary application state (the
//! [`Accordion`](rstui_widgets::Accordion) `expanded` precedent), and the
//! relative time string is the **caller's** to supply — no time crate is
//! pulled in (the brief's scope line; rstui has no wall clock in `view`). So
//! `Commit` owns nothing: it projects the caller's
//! [`CommitInfo`] + `&[CommitFile]` and a caller-owned
//! [`open`](Commit::open) `bool`.
//!
//! It is a framed [`Card`] (header = `hash message`,
//! footer = `author · when`) — *reusing* the widget — and exposes
//! [`header_rect`](Commit::header_rect) so the host can hit-test a click to
//! toggle the file list (no callback, the documented seam).
//!
//! # Clamp, don't panic
//!
//! Per the [`Gauge`](rstui_widgets::Gauge) totality rule a zero/tiny area, an
//! empty file list, and over-many files are all safe clips — never a panic.

use rstui_core::{Buffer, Color, Position, Rect, Style, Widget};
use rstui_widgets::{Block, Card};

/// The header/footer facts of a commit (the caller supplies the relative
/// time — no time crate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    /// The short hash (e.g. `a1b2c3d`).
    pub hash: String,
    /// The commit subject line.
    pub message: String,
    /// The author's name.
    pub author: String,
    /// A caller-formatted relative time (e.g. `2 hours ago`).
    pub when: String,
}

impl CommitInfo {
    /// A commit `hash` with `message`, `author`, and a caller-formatted
    /// relative `when`.
    pub fn new(
        hash: impl Into<String>,
        message: impl Into<String>,
        author: impl Into<String>,
        when: impl Into<String>,
    ) -> Self {
        Self {
            hash: hash.into(),
            message: message.into(),
            author: author.into(),
            when: when.into(),
        }
    }
}

/// One changed file in a commit: a path, an add/delete count, and a status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitFile {
    /// The file path.
    pub path: String,
    /// Lines added.
    pub additions: u32,
    /// Lines deleted.
    pub deletions: u32,
    /// The change status (drives the leading glyph).
    pub status: FileStatus,
}

impl CommitFile {
    /// A `path` changed by `additions`/`deletions` with `status`.
    pub fn new(
        path: impl Into<String>,
        additions: u32,
        deletions: u32,
        status: FileStatus,
    ) -> Self {
        Self {
            path: path.into(),
            additions,
            deletions,
            status,
        }
    }
}

/// How a [`CommitFile`] changed, selecting its status glyph.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    /// Modified (`~`) — the default.
    #[default]
    Modified,
    /// Added (`+`).
    Added,
    /// Deleted (`-`).
    Deleted,
    /// Renamed (`»`).
    Renamed,
}

impl FileStatus {
    /// The glyph prefixing a file of this status.
    #[must_use]
    pub fn glyph(self) -> char {
        match self {
            Self::Modified => '~',
            Self::Added => '+',
            Self::Deleted => '-',
            Self::Renamed => '»',
        }
    }
}

/// A git commit card with a collapsible changed-file list.
///
/// A framed [`Card`]: header `hash message`, footer
/// `author · when`. When [`open`](Self::open) the body lists the files
/// (`<glyph> path  +adds -dels`). `Commit` owns no state — see the
/// [module docs](self).
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Position, Rect, Widget};
/// use rstui_ai::commit::{Commit, CommitFile, CommitInfo, FileStatus};
///
/// let info = CommitInfo::new("a1b2c3d", "Fix parser", "Ada", "2h ago");
/// let files = [CommitFile::new("src/p.rs", 12, 3, FileStatus::Modified)];
/// let card = Commit::new(&info, &files).open(true);
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 28, 6));
/// card.render(buf.area(), &mut buf);
/// assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌'); // framed
/// assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, 'a'); // hash
/// ```
#[derive(Debug, Clone)]
pub struct Commit<'a> {
    info: &'a CommitInfo,
    files: &'a [CommitFile],
    open: bool,
    style: Style,
}

impl<'a> Commit<'a> {
    /// A card for `info` with its changed `files`, file list collapsed.
    #[must_use]
    pub fn new(info: &'a CommitInfo, files: &'a [CommitFile]) -> Self {
        Self {
            info,
            files,
            open: false,
            style: Style::new(),
        }
    }

    /// Sets the caller-owned open flag (the reducer flips it on a
    /// [`header_rect`](Self::header_rect) click; the widget only reads it).
    #[must_use]
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Sets the base [`Style`] (the card frame/background).
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The framing card (header `hash message`, footer `author · when`).
    fn card(&self) -> Card<'a> {
        Card::new()
            .block(Block::bordered().style(self.style))
            .header(format!("{} {}", self.info.hash, self.info.message))
            .footer(format!("{} · {}", self.info.author, self.info.when))
    }

    /// The header row [`Rect`] (where `hash message` is drawn) — the host
    /// hit-tests a click here to toggle [`open`](Self::open).
    #[must_use]
    pub fn header_rect(&self, area: Rect) -> Rect {
        let inner = Block::bordered().inner(area);
        Rect::new(inner.left(), inner.top(), inner.width, inner.height.min(1))
    }
}

impl Widget for Commit<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let card = self.card();
        let body = card.inner(area);
        card.render(area, buf);
        if !self.open || body.is_empty() {
            return;
        }
        for (row, file) in self.files.iter().take(body.height as usize).enumerate() {
            let y = body.top().saturating_add(row as u16);
            let line = format!(
                "{} {}  +{} -{}",
                file.status.glyph(),
                file.path,
                file.additions,
                file.deletions
            );
            let mut x = body.left();
            for ch in line.chars() {
                if x >= body.right() {
                    break;
                }
                buf.set_cell(Position::new(x, y), ch, self.style.fg(Color::Reset));
                x = x.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> CommitInfo {
        CommitInfo::new("a1b2c3d", "Fix parser", "Ada", "2h ago")
    }

    fn files() -> Vec<CommitFile> {
        vec![
            CommitFile::new("src/p.rs", 12, 3, FileStatus::Modified),
            CommitFile::new("NEW.md", 5, 0, FileStatus::Added),
        ]
    }

    fn lines(widget: Commit<'_>, w: u16, h: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        widget.render(buf.area(), &mut buf);
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn the_header_is_hash_and_message_the_footer_author_and_time() {
        let i = info();
        let f = files();
        let out = lines(Commit::new(&i, &f), 24, 4);
        assert!(out.contains("a1b2c3d Fix parser"), "got {out:?}");
        assert!(out.contains("Ada · 2h ago"), "got {out:?}");
    }

    #[test]
    fn a_closed_card_hides_the_file_list() {
        let i = info();
        let f = files();
        // Body rows blank between header and footer.
        let out = lines(Commit::new(&i, &f), 24, 5);
        assert!(!out.contains("src/p.rs"), "got {out:?}");
    }

    #[test]
    fn an_open_card_lists_files_with_status_glyphs_and_counts() {
        let i = info();
        let f = files();
        let out = lines(Commit::new(&i, &f).open(true), 26, 6);
        assert!(out.contains("~ src/p.rs  +12 -3"), "got {out:?}");
        assert!(out.contains("+ NEW.md  +5 -0"), "got {out:?}");
    }

    #[test]
    fn header_rect_is_the_first_inner_row() {
        let i = info();
        let f = files();
        let area = Rect::new(0, 0, 24, 6);
        let hr = Commit::new(&i, &f).header_rect(area);
        assert_eq!(hr, Rect::new(1, 1, 22, 1));
    }

    #[test]
    fn file_status_glyphs_are_distinct() {
        assert_eq!(FileStatus::Modified.glyph(), '~');
        assert_eq!(FileStatus::Added.glyph(), '+');
        assert_eq!(FileStatus::Deleted.glyph(), '-');
        assert_eq!(FileStatus::Renamed.glyph(), '»');
    }

    #[test]
    fn over_many_files_clip_to_the_body() {
        let i = info();
        let many: Vec<CommitFile> = (0..9)
            .map(|n| CommitFile::new(format!("f{n}"), 1, 1, FileStatus::Modified))
            .collect();
        // Small card → only the body rows that fit; no panic.
        let out = lines(Commit::new(&i, &many).open(true), 20, 5);
        assert!(out.contains("~ f0"), "got {out:?}");
    }

    #[test]
    fn zero_area_is_a_no_op() {
        let i = info();
        let f = files();
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        Commit::new(&i, &f).render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }
}
