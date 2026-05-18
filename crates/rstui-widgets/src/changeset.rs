//! [`Changeset`] — the multi-file unified-diff *model*: a splitter and an
//! ordered file/hunk index over a many-file patch, the substrate the
//! review-stream, hunk navigation and the diff symbol panel are built on.
//!
//! # Why this exists
//!
//! [`Diff`](crate::Diff) parses exactly **one** unified patch string and
//! exposes no structure — it is a renderer, not a model. A real review tool
//! works over a *changeset*: the output of `git show -p` / `git diff` is many
//! `diff --git a/… b/…` sections, each a file with a status (added / deleted /
//! modified / renamed / copied / binary), `+`/`-` stats and an ordered list of
//! hunks. The [code-review & editing roadmap](https://github.com/andymac4182/rstui/blob/main/docs/code-review-and-editing.md)
//! `C.1`/`R1a` calls for exactly this: a `Changeset → DiffFile → HunkRef`
//! model with an ordered index so "next/prev hunk" and "next/prev file" are
//! reducer arithmetic, not re-parsing.
//!
//! # A splitter, not a second grammar
//!
//! This module deliberately does **not** duplicate or fork
//! [`Diff`](crate::Diff)'s unified-diff grammar. `Changeset` is a *splitter +
//! index over* the raw patch: it segments the multi-file text into per-file
//! raw patch slices on `diff --git` / `diff --cc` / `diff --combined`
//! boundaries (and, for a header-less patch, a fresh `--- ` / a leading `@@`
//! hunk), recording each file's metadata and the line range of its slice and
//! of every hunk within it. Rendering a file's body is then delegated back to
//! the existing widget — `Diff::new(file.patch())` — which already owns the
//! grammar, the layouts, the themes and the word-LCS. We model *structure*,
//! the renderer renders.
//!
//! The prefix recognisers used here (the `a/`/`b/` strip, `/dev/null`,
//! `rename from`/`to`, `Binary files … differ`, the `@@`/`@@@` hunk fence)
//! mirror [`Diff`](crate::Diff)'s conventions so a slice this module hands to
//! `Diff::new` is read by `Diff` exactly as the original — but they are used
//! for *segmentation*, never for rendering.
//!
//! # Total
//!
//! [`Changeset::parse`] is total: malformed, empty, partial, CRLF, or
//! header-less input yields a best-effort [`Changeset`], never a panic. A
//! patch with zero `diff --git` headers is treated as one file when it has
//! `@@` hunks, else as the empty changeset.
//!
//! # Example
//!
//! ```
//! use rstui_widgets::{Changeset, Diff, FileStatus};
//!
//! let patch = "\
//! diff --git a/added.txt b/added.txt
//! new file mode 100644
//! --- /dev/null
//! +++ b/added.txt
//! @@ -0,0 +1,2 @@
//! +first
//! +second
//! diff --git a/old.txt b/new.txt
//! rename from old.txt
//! rename to new.txt
//! ";
//! let cs = Changeset::parse(patch);
//! assert_eq!(cs.files.len(), 2);
//! assert_eq!(cs.files[0].status, FileStatus::Added);
//! assert_eq!(cs.files[0].path, "added.txt");
//! assert_eq!(cs.total_additions(), 2);
//! assert_eq!(cs.files[1].status, FileStatus::Renamed);
//! assert_eq!(cs.files[1].old_path.as_deref(), Some("old.txt"));
//! assert_eq!(cs.files[1].path, "new.txt");
//!
//! // The per-file slice is exactly what the existing renderer consumes.
//! let _ = Diff::new(cs.files[0].patch());
//! ```

use core::ops::Range;

/// What happened to a file in a changeset.
///
/// Derived from the segment's `git` metadata and `--- `/`+++ ` sides, in
/// priority order: a binary notice ⇒ [`Binary`](FileStatus::Binary); a
/// `rename from`/`to` ⇒ [`Renamed`](FileStatus::Renamed); a `copy from`/`to`
/// ⇒ [`Copied`](FileStatus::Copied); a `/dev/null` *old* side (or
/// `new file mode`) ⇒ [`Added`](FileStatus::Added); a `/dev/null` *new* side
/// (or `deleted file mode`) ⇒ [`Deleted`](FileStatus::Deleted); otherwise
/// [`Modified`](FileStatus::Modified).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    /// A new file (old side `/dev/null`, or a `new file mode` line).
    Added,
    /// A removed file (new side `/dev/null`, or a `deleted file mode` line).
    Deleted,
    /// An in-place content change.
    Modified,
    /// A `rename from`/`rename to` pair (content may also have changed).
    Renamed,
    /// A `copy from`/`copy to` pair.
    Copied,
    /// A binary file (`Binary files … differ` or a `GIT binary patch`); has
    /// no textual hunks and no `+`/`-` stats.
    Binary,
}

/// One hunk's coordinates within its file's patch slice.
///
/// `old_start`/`new_start` come from the `@@ -<old> +<new> @@` header (a
/// missing range defaults its start to the other side, matching
/// [`Diff`](crate::Diff)). `patch_lines` is the line range of this hunk —
/// from its `@@` header line up to (but not including) the next hunk header or
/// the end of the file's slice — **relative to the file's own
/// [`patch()`](DiffFile::patch) slice**, 0-based and end-exclusive, so it
/// indexes straight into `file.patch().lines()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HunkRef {
    /// The `-` start line from the `@@` header.
    pub old_start: u32,
    /// The `+` start line from the `@@` header.
    pub new_start: u32,
    /// The verbatim `@@ … @@` header line (CRLF stripped), section label
    /// included if present.
    pub header: String,
    /// Line range of this hunk within the file's patch slice (0-based,
    /// end-exclusive).
    pub patch_lines: Range<usize>,
}

/// One file's worth of a changeset: its identity, status, stats, hunk index,
/// and the raw single-file patch slice to hand back to [`Diff`](crate::Diff).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFile {
    /// The file's path: the new path, or the old path for a pure delete.
    /// `a/`/`b/` stripped, surrounding quotes removed, a trailing
    /// tab-timestamp dropped — the same cleaning [`Diff`](crate::Diff) does.
    pub path: String,
    /// The *old* path — `Some` only for a [`Renamed`](FileStatus::Renamed) or
    /// [`Copied`](FileStatus::Copied) file, `None` otherwise.
    pub old_path: Option<String>,
    /// What happened to the file.
    pub status: FileStatus,
    /// Body `+` lines (the `+++` header line is **not** counted).
    pub additions: usize,
    /// Body `-` lines (the `---` header line is **not** counted).
    pub deletions: usize,
    /// The file's hunks in source order, each carrying its line range within
    /// [`patch()`](Self::patch).
    pub hunks: Vec<HunkRef>,
    /// The raw single-file patch slice (the bytes between this `diff --git`
    /// boundary and the next), CRLF-normalised, ready for
    /// `Diff::new(file.patch())`.
    patch: String,
}

impl DiffFile {
    /// The raw single-file unified patch for this file — exactly the slice to
    /// pass to [`Diff::new`](crate::Diff::new) to render its body. Rendering
    /// is delegated; this model never re-renders.
    #[must_use]
    pub fn patch(&self) -> &str {
        &self.patch
    }
}

/// An ordered model of a multi-file unified diff.
///
/// [`parse`](Self::parse) splits the patch into [`DiffFile`]s in source
/// order; [`hunk_index`](Self::hunk_index) / [`file_of_hunk`](Self::file_of_hunk)
/// expose the flat ordered `(file, hunk)` substrate every navigation key
/// needs. This model holds **no cursor**: the position is the reducer's
/// caller-owned state (per the pure-projection rule); this is just the
/// ordered index it walks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changeset {
    /// The files, in the order they appear in the patch.
    pub files: Vec<DiffFile>,
}

impl Changeset {
    /// Parse a multi-file unified diff (`git show -p` / `git diff` output).
    ///
    /// **Total.** Malformed, empty, partial, CRLF, or header-less input
    /// yields a best-effort [`Changeset`] and never panics. Splitting is on
    /// `diff --git` / `diff --cc` / `diff --combined` boundaries; a patch
    /// with none of those is treated as a single file when it carries any
    /// `@@` hunk (or a `--- `/`+++ ` pair), otherwise as the empty changeset.
    /// `additions`/`deletions` count body `+`/`-` lines only — never the
    /// `+++ `/`--- ` file-header lines.
    #[must_use]
    pub fn parse(patch: &str) -> Self {
        // Normalise newlines once (CRLF → LF); a trailing empty tail from a
        // final newline is not content. Splitting and all ranges are over
        // these normalised lines so a file's slice round-trips byte-for-byte
        // through `DiffFile::patch`.
        let mut lines: Vec<&str> = patch
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l))
            .collect();
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        if lines.is_empty() {
            return Self::default();
        }

        // Segment boundaries: the index of every line that *starts* a new
        // file. A `diff --git`/`--cc`/`--combined` line is the canonical
        // boundary. If there is no such header anywhere, fall back to a
        // fresh `--- ` as the boundary so a bare `git diff` body without the
        // `diff --git` line still splits per file.
        let has_diff_header = lines.iter().any(|l| is_diff_header(l));
        let mut starts: Vec<usize> = Vec::new();
        if has_diff_header {
            for (i, l) in lines.iter().enumerate() {
                if is_diff_header(l) {
                    starts.push(i);
                }
            }
        } else {
            // Header-less: a new `--- ` that is *not* immediately part of the
            // previous file's hunk body starts a file. We only ever see a
            // `--- ` here as a file header (a body line is `-…`, never
            // `--- `), so each `--- ` whose next line is `+++ ` is a boundary.
            for i in 0..lines.len() {
                if lines[i].starts_with("--- ")
                    && lines.get(i + 1).is_some_and(|n| n.starts_with("+++ "))
                {
                    starts.push(i);
                }
            }
            // No `diff --git` and no `--- `/`+++ ` pair: if there is any `@@`
            // hunk treat the whole input as one file, else it is empty.
            if starts.is_empty() {
                if lines.iter().any(|l| parse_hunk_start(l).is_some()) {
                    starts.push(0);
                } else {
                    return Self::default();
                }
            }
        }

        // Any preamble before the first boundary (a commit message from
        // `git show`, say) is not part of any file — it is dropped, matching
        // `Diff`'s "outside any hunk and not a header ⇒ best-effort drop".
        let mut files = Vec::with_capacity(starts.len());
        for (seg, &start) in starts.iter().enumerate() {
            let end = starts.get(seg + 1).copied().unwrap_or(lines.len());
            let slice = &lines[start..end];
            files.push(build_file(slice));
        }

        Self { files }
    }

    /// Total number of body `+` lines across every file.
    #[must_use]
    pub fn total_additions(&self) -> usize {
        self.files.iter().map(|f| f.additions).sum()
    }

    /// Total number of body `-` lines across every file.
    #[must_use]
    pub fn total_deletions(&self) -> usize {
        self.files.iter().map(|f| f.deletions).sum()
    }

    /// The flat ordered `(file_index, hunk_index)` list — the substrate for
    /// "next/prev hunk" and "next/prev file" navigation.
    ///
    /// No position state lives here; the reducer owns the cursor and walks
    /// this index. Its length is exactly the sum of every file's hunk count.
    #[must_use]
    pub fn hunk_index(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (fi, file) in self.files.iter().enumerate() {
            for hi in 0..file.hunks.len() {
                out.push((fi, hi));
            }
        }
        out
    }

    /// The index of the file containing the global hunk ordinal `n`
    /// (0-based, in [`hunk_index`](Self::hunk_index) order), or `None` if `n`
    /// is past the last hunk.
    #[must_use]
    pub fn file_of_hunk(&self, n: usize) -> Option<usize> {
        let mut seen = 0usize;
        for (fi, file) in self.files.iter().enumerate() {
            let count = file.hunks.len();
            if n < seen + count {
                return Some(fi);
            }
            seen += count;
        }
        None
    }
}

/// Whether a line is a file-segment boundary header (`diff --git`,
/// `diff --cc`, or `diff --combined`) — the same set [`Diff`](crate::Diff)
/// splits on.
fn is_diff_header(line: &str) -> bool {
    line.starts_with("diff --git ")
        || line.starts_with("diff --cc ")
        || line.starts_with("diff --combined ")
}

/// If `line` is a hunk header (`@@ … @@`, or a combined `@@@ … @@@`), parse
/// its `(old_start, new_start)`; otherwise `None`. A leading `@` run of width
/// `< 2` is not a hunk. Counts are ignored (only the starts are modelled
/// here); an omitted range defaults its start to the other side's, mirroring
/// [`Diff`](crate::Diff)'s `parse_hunk_header`.
fn parse_hunk_start(line: &str) -> Option<(u32, u32)> {
    let fence = line.bytes().take_while(|&b| b == b'@').count();
    if fence < 2 {
        return None;
    }
    let fence_str = &line[..fence];
    let rest = line[fence..].strip_prefix(' ')?;
    let close_pat = format!(" {fence_str}");
    let close = rest.find(&close_pat)?;
    let ranges = &rest[..close];

    // `parents` minus ranges (one per parent; ≥ 1) then exactly one plus
    // range. A combined `@@@` header has `fence - 1` parents.
    let parents = fence - 1;
    let mut parts = ranges.split(' ');
    let mut old_start: Option<u32> = None;
    for _ in 0..parents {
        let minus = parts.next()?.strip_prefix('-')?;
        let start = range_start(minus)?;
        old_start.get_or_insert(start);
    }
    let plus = parts.next()?.strip_prefix('+')?;
    let new_start = range_start(plus)?;
    if parts.next().is_some() {
        return None;
    }
    Some((old_start.unwrap_or(new_start), new_start))
}

/// The `start` of a `start[,count]` range (the count is irrelevant to the
/// model). `None` if `start` is not a number.
fn range_start(s: &str) -> Option<u32> {
    let head = s.split_once(',').map_or(s, |(h, _)| h);
    head.parse().ok()
}

/// Strip a trailing tab+timestamp, surrounding quotes, and a leading
/// `a/`/`b/` from a header path. Mirrors [`Diff`](crate::Diff)'s `clean_path`
/// conventions, but unquotes *before* stripping the prefix so a path with
/// spaces (git quotes the whole token, `"a/with space.txt"`) yields the bare
/// `with space.txt` — the robustness the changeset model explicitly needs.
fn clean_path(raw: &str) -> String {
    // Git/diff timestamps follow a tab; keep only the path before it.
    let path = raw.split('\t').next().unwrap_or(raw).trim();
    // A path with spaces (or other shell-special bytes) is wrapped in
    // double quotes by git, prefix included — unquote first.
    let path = path.trim_matches('"');
    let path = path
        .strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path);
    path.to_owned()
}

/// Whether a raw header path names the empty `/dev/null` side.
fn is_dev_null(raw: &str) -> bool {
    raw.split('\t').next().unwrap_or(raw).trim() == "/dev/null"
}

/// The path carried by a `diff --git a/… b/…` (or `--cc`/`--combined`) header,
/// if it can be recovered. Quoting (a path with spaces is quoted) and a
/// rename (the two halves differ) make this only a *fallback* — the `--- `/
/// `+++ ` pair, when present, is authoritative. Returns the cleaned `b/`
/// side, falling back to the `a/` side.
fn diff_header_path(line: &str) -> Option<String> {
    // After `diff --git ` (etc.) come the two paths. If neither half is
    // quoted and there is exactly one ` a/… b/…` split we can recover it;
    // otherwise leave it to the `--- `/`+++ ` pair.
    let rest = line
        .strip_prefix("diff --git ")
        .or_else(|| line.strip_prefix("diff --cc "))
        .or_else(|| line.strip_prefix("diff --combined "))?;
    if rest.contains('"') {
        return None;
    }
    // `a/<old> b/<new>` — find the ` b/` that begins the second path. (A
    // combined `diff --cc <path>` has just the one path; treat it as both.)
    if let Some(idx) = rest.rfind(" b/") {
        let new = &rest[idx + 1..];
        return Some(clean_path(new));
    }
    // `diff --cc foo/bar` style: a single bare path.
    Some(clean_path(rest.trim()))
}

/// Build one [`DiffFile`] from its raw normalised line slice (the lines from
/// its boundary header up to, but not including, the next file's). Records
/// metadata, stats and the per-hunk line ranges; the slice is preserved
/// verbatim as [`DiffFile::patch`] so it round-trips into
/// [`Diff::new`](crate::Diff::new).
fn build_file(slice: &[&str]) -> DiffFile {
    let patch = slice.join("\n");

    let mut old_hdr: Option<String> = None; // raw `--- ` path
    let mut new_hdr: Option<String> = None; // raw `+++ ` path
    let mut rename_from: Option<String> = None;
    let mut rename_to: Option<String> = None;
    let mut copy_from: Option<String> = None;
    let mut copy_to: Option<String> = None;
    let mut explicit_added = false; // `new file mode`
    let mut explicit_deleted = false; // `deleted file mode`
    let mut is_binary = false;
    let mut header_path: Option<String> = None; // recovered from `diff --git`

    let mut additions = 0usize;
    let mut deletions = 0usize;
    // Each hunk is pushed with its range open to the slice end; the next
    // hunk header (or, at the loop's end, the slice itself) closes the one
    // before it.
    let mut hunks: Vec<HunkRef> = Vec::new();
    let mut in_hunk = false;
    // A `GIT binary patch` block is followed by base85 payload; not content.
    let mut in_binary_payload = false;

    for (i, &line) in slice.iter().enumerate() {
        if is_diff_header(line) {
            header_path = diff_header_path(line);
            in_hunk = false;
            in_binary_payload = false;
            continue;
        }
        if let Some(p) = line.strip_prefix("--- ") {
            old_hdr = Some(p.to_owned());
            in_hunk = false;
            in_binary_payload = false;
            continue;
        }
        if let Some(p) = line.strip_prefix("+++ ") {
            new_hdr = Some(p.to_owned());
            in_hunk = false;
            in_binary_payload = false;
            continue;
        }
        if let Some((old_start, new_start)) = parse_hunk_start(line) {
            // This header closes the previous hunk's open range.
            if let Some(prev) = hunks.last_mut() {
                prev.patch_lines.end = i;
            }
            hunks.push(HunkRef {
                old_start,
                new_start,
                header: line.to_owned(),
                patch_lines: i..slice.len(),
            });
            in_hunk = true;
            in_binary_payload = false;
            continue;
        }

        if !in_hunk {
            // Metadata / binary notices live outside hunks. A binary notice
            // (`Binary files … differ` or `GIT binary patch`) makes the file
            // binary; the `GIT binary patch` payload that follows is skipped.
            if line == "GIT binary patch" {
                is_binary = true;
                in_binary_payload = true;
                continue;
            }
            if line.starts_with("Binary files ") {
                is_binary = true;
                continue;
            }
            if in_binary_payload {
                continue;
            }
            if let Some(p) = line.strip_prefix("rename from ") {
                rename_from = Some(p.trim().to_owned());
                continue;
            }
            if let Some(p) = line.strip_prefix("rename to ") {
                rename_to = Some(p.trim().to_owned());
                continue;
            }
            if let Some(p) = line.strip_prefix("copy from ") {
                copy_from = Some(p.trim().to_owned());
                continue;
            }
            if let Some(p) = line.strip_prefix("copy to ") {
                copy_to = Some(p.trim().to_owned());
                continue;
            }
            if line.starts_with("new file mode ") {
                explicit_added = true;
                continue;
            }
            if line.starts_with("deleted file mode ") {
                explicit_deleted = true;
                continue;
            }
            // Other metadata (`index …`, `old/new mode`, `similarity …`) and
            // any unrecognised preamble line: not stats, nothing to model.
            continue;
        }

        // Inside a hunk: stats. A `\ No newline at end of file` marker and a
        // combined-diff multi-sign line must not be miscounted — only a
        // single leading `+`/`-` (and, for safety on combined diffs, a `+`/
        // `-` anywhere in the lead sign columns) is a real add/del. We use
        // the dominant-sign rule `Diff` uses for combined hunks: classify by
        // the first body char, treating `\` as neither.
        match line.chars().next() {
            Some('\\') => {}
            Some('+') => additions += 1,
            Some('-') => deletions += 1,
            _ => {}
        }
    }

    // The final open hunk's range was pushed as `i..slice.len()`. But
    // `patch` is `slice.join("\n")`, and a trailing empty slice element (a
    // patch ending in `\n`, common in truncated input) makes `join` end in
    // `\n` which `str::lines` drops — so `patch().lines().count()` can be
    // *less* than `slice.len()`. `patch_lines` is contracted to index into
    // `patch().lines()`, so clamp every range to that authoritative count and
    // drop any hunk whose header line was itself trimmed away (start ≥ count;
    // it has no body to render anyway).
    let patch_line_count = patch.lines().count();
    hunks.retain_mut(|h| {
        if h.patch_lines.start >= patch_line_count {
            return false;
        }
        if h.patch_lines.end > patch_line_count {
            h.patch_lines.end = patch_line_count;
        }
        h.patch_lines.start < h.patch_lines.end
    });

    let (path, old_path, status) = resolve_identity(&IdentityInput {
        old_hdr: old_hdr.as_deref(),
        new_hdr: new_hdr.as_deref(),
        header_path: header_path.as_deref(),
        rename_from: rename_from.as_deref(),
        rename_to: rename_to.as_deref(),
        copy_from: copy_from.as_deref(),
        copy_to: copy_to.as_deref(),
        explicit_added,
        explicit_deleted,
        is_binary,
    });

    DiffFile {
        path,
        old_path,
        status,
        // A binary file carries no textual hunks; do not surface phantom
        // stats from a stray line.
        additions: if is_binary { 0 } else { additions },
        deletions: if is_binary { 0 } else { deletions },
        hunks,
        patch,
    }
}

/// The raw inputs `resolve_identity` weighs to decide `(path, old_path,
/// status)`. Grouped into one struct so the resolver is a single readable
/// decision table rather than an eleven-argument function.
struct IdentityInput<'a> {
    old_hdr: Option<&'a str>,
    new_hdr: Option<&'a str>,
    header_path: Option<&'a str>,
    rename_from: Option<&'a str>,
    rename_to: Option<&'a str>,
    copy_from: Option<&'a str>,
    copy_to: Option<&'a str>,
    explicit_added: bool,
    explicit_deleted: bool,
    is_binary: bool,
}

/// Decide a file's `(path, old_path, status)` from its collected metadata.
///
/// Priority: rename ⇒ [`Renamed`](FileStatus::Renamed); copy ⇒
/// [`Copied`](FileStatus::Copied); a `/dev/null` *old* side or
/// `new file mode` ⇒ [`Added`](FileStatus::Added); a `/dev/null` *new* side
/// or `deleted file mode` ⇒ [`Deleted`](FileStatus::Deleted); a binary notice
/// (with none of the above) ⇒ [`Binary`](FileStatus::Binary); otherwise
/// [`Modified`](FileStatus::Modified). `path` is the new path (the old path
/// for a pure delete); `old_path` is `Some` only for rename/copy.
fn resolve_identity(i: &IdentityInput<'_>) -> (String, Option<String>, FileStatus) {
    let old_clean = i.old_hdr.map(clean_path);
    let new_clean = i.new_hdr.map(clean_path);
    let old_null = i.old_hdr.is_some_and(is_dev_null);
    let new_null = i.new_hdr.is_some_and(is_dev_null);

    // Rename: the `rename from`/`to` pair is authoritative for the two paths.
    if i.rename_from.is_some() || i.rename_to.is_some() {
        let to = i
            .rename_to
            .map(str::to_owned)
            .or_else(|| new_clean.clone())
            .or_else(|| i.header_path.map(str::to_owned))
            .unwrap_or_default();
        let from = i
            .rename_from
            .map(str::to_owned)
            .or_else(|| old_clean.clone())
            .unwrap_or_default();
        return (to, Some(from), FileStatus::Renamed);
    }

    // Copy: same shape, different status.
    if i.copy_from.is_some() || i.copy_to.is_some() {
        let to = i
            .copy_to
            .map(str::to_owned)
            .or_else(|| new_clean.clone())
            .or_else(|| i.header_path.map(str::to_owned))
            .unwrap_or_default();
        let from = i
            .copy_from
            .map(str::to_owned)
            .or_else(|| old_clean.clone())
            .unwrap_or_default();
        return (to, Some(from), FileStatus::Copied);
    }

    // Added: old side is `/dev/null`, or git said `new file mode`. The new
    // path names it.
    if old_null || (i.explicit_added && !new_null) {
        let path = new_clean
            .filter(|p| !p.is_empty())
            .or_else(|| i.header_path.map(str::to_owned))
            .unwrap_or_default();
        return (path, None, FileStatus::Added);
    }

    // Deleted: new side is `/dev/null`, or git said `deleted file mode`. The
    // old path names it (there is no new path).
    if new_null || (i.explicit_deleted && !old_null) {
        let path = old_clean
            .filter(|p| !p.is_empty())
            .or_else(|| i.header_path.map(str::to_owned))
            .unwrap_or_default();
        return (path, None, FileStatus::Deleted);
    }

    // A pure binary change with no rename/copy/add/delete signal.
    if i.is_binary {
        let path = new_clean
            .filter(|p| !p.is_empty())
            .or(old_clean)
            .filter(|p| !p.is_empty())
            .or_else(|| i.header_path.map(str::to_owned))
            .unwrap_or_default();
        return (path, None, FileStatus::Binary);
    }

    // Ordinary in-place modification. Prefer the `+++` new path, fall back
    // to the `---` old path, then the `diff --git` recovered path.
    let path = new_clean
        .filter(|p| !p.is_empty())
        .or(old_clean)
        .filter(|p| !p.is_empty())
        .or_else(|| i.header_path.map(str::to_owned))
        .unwrap_or_default();
    (path, None, FileStatus::Modified)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3-file patch exercising the three common shapes: an add via
    /// `/dev/null`, a modify with two hunks, and a rename.
    const THREE_FILE: &str = "\
diff --git a/added.txt b/added.txt
new file mode 100644
index 0000000..3b18e51
--- /dev/null
+++ b/added.txt
@@ -0,0 +1,3 @@
+alpha
+beta
+gamma
diff --git a/src/changed.rs b/src/changed.rs
index 1234567..89abcde 100644
--- a/src/changed.rs
+++ b/src/changed.rs
@@ -1,4 +1,4 @@
 fn one() {}
-let x = 1;
+let x = 2;
 fn two() {}
@@ -10,3 +10,4 @@ fn ctx
 keep
-drop
+add a
+add b
diff --git a/old/name.txt b/new/name.txt
similarity index 95%
rename from old/name.txt
rename to new/name.txt
--- a/old/name.txt
+++ b/new/name.txt
@@ -1,2 +1,2 @@
 unchanged
-old line
+new line
";

    #[test]
    fn three_file_patch_files_paths_status_and_stats() {
        let cs = Changeset::parse(THREE_FILE);
        assert_eq!(cs.files.len(), 3, "three diff --git sections");

        // File 0: added via /dev/null.
        let f0 = &cs.files[0];
        assert_eq!(f0.status, FileStatus::Added);
        assert_eq!(f0.path, "added.txt");
        assert_eq!(f0.old_path, None);
        assert_eq!(f0.additions, 3);
        assert_eq!(f0.deletions, 0);
        assert_eq!(f0.hunks.len(), 1);

        // File 1: in-place modify, two hunks. additions/deletions are body
        // `+`/`-` only (the `+++`/`---` header lines must NOT be counted).
        let f1 = &cs.files[1];
        assert_eq!(f1.status, FileStatus::Modified);
        assert_eq!(f1.path, "src/changed.rs");
        assert_eq!(f1.old_path, None);
        assert_eq!(f1.hunks.len(), 2);
        assert_eq!(f1.additions, 3, "1 in hunk A + 2 in hunk B");
        assert_eq!(f1.deletions, 2, "1 in hunk A + 1 in hunk B");
        assert_eq!(f1.hunks[0].old_start, 1);
        assert_eq!(f1.hunks[0].new_start, 1);
        assert_eq!(f1.hunks[1].old_start, 10);
        assert_eq!(f1.hunks[1].new_start, 10);
        assert!(f1.hunks[0].header.starts_with("@@ -1,4 +1,4 @@"));
        assert!(f1.hunks[1].header.contains("fn ctx"));

        // File 2: rename (with a content tweak).
        let f2 = &cs.files[2];
        assert_eq!(f2.status, FileStatus::Renamed);
        assert_eq!(f2.path, "new/name.txt");
        assert_eq!(f2.old_path.as_deref(), Some("old/name.txt"));
        assert_eq!(f2.additions, 1);
        assert_eq!(f2.deletions, 1);
        assert_eq!(f2.hunks.len(), 1);

        // Totals fold the per-file stats: additions 3+3+1, deletions 0+2+1.
        assert_eq!(cs.total_additions(), 7);
        assert_eq!(cs.total_deletions(), 3);
    }

    #[test]
    fn file_patch_round_trips_its_slice() {
        let cs = Changeset::parse(THREE_FILE);
        // Concatenating every file's slice (newline-joined) reconstructs the
        // whole patch (modulo the trailing newline `parse` strips). This is
        // the contract that lets `Diff::new(file.patch())` render each file.
        let rejoined = cs
            .files
            .iter()
            .map(|f| f.patch().to_owned())
            .collect::<Vec<_>>()
            .join("\n");
        let expected = THREE_FILE.trim_end_matches('\n');
        assert_eq!(rejoined, expected);

        // Each slice begins at its own `diff --git` header.
        assert!(cs.files[0].patch().starts_with("diff --git a/added.txt"));
        assert!(
            cs.files[1]
                .patch()
                .starts_with("diff --git a/src/changed.rs")
        );
        assert!(cs.files[2].patch().starts_with("diff --git a/old/name.txt"));
    }

    #[test]
    fn hunk_patch_lines_are_within_the_files_slice() {
        let cs = Changeset::parse(THREE_FILE);
        for file in &cs.files {
            let n = file.patch().lines().count();
            for h in &file.hunks {
                assert!(h.patch_lines.start < h.patch_lines.end, "non-empty range");
                assert!(
                    h.patch_lines.end <= n,
                    "hunk range {:?} escapes the {n}-line slice",
                    h.patch_lines
                );
                // The range's first line is exactly this hunk's `@@` header.
                let first = file.patch().lines().nth(h.patch_lines.start).unwrap();
                assert_eq!(first, h.header);
            }
        }
        // The two hunks of file 1 are contiguous and ordered: hunk A ends
        // where hunk B begins.
        let f1 = &cs.files[1];
        assert_eq!(f1.hunks[0].patch_lines.end, f1.hunks[1].patch_lines.start);
    }

    #[test]
    fn dev_null_old_side_is_added() {
        let p = "\
diff --git a/n.txt b/n.txt
--- /dev/null
+++ b/n.txt
@@ -0,0 +1 @@
+only
";
        let cs = Changeset::parse(p);
        assert_eq!(cs.files.len(), 1);
        assert_eq!(cs.files[0].status, FileStatus::Added);
        assert_eq!(cs.files[0].path, "n.txt");
        assert_eq!(cs.files[0].additions, 1);
    }

    #[test]
    fn dev_null_new_side_is_deleted() {
        let p = "\
diff --git a/gone.txt b/gone.txt
deleted file mode 100644
--- a/gone.txt
+++ /dev/null
@@ -1,2 +0,0 @@
-was here
-also gone
";
        let cs = Changeset::parse(p);
        assert_eq!(cs.files.len(), 1);
        assert_eq!(cs.files[0].status, FileStatus::Deleted);
        assert_eq!(cs.files[0].path, "gone.txt");
        assert_eq!(cs.files[0].old_path, None);
        assert_eq!(cs.files[0].deletions, 2);
        assert_eq!(cs.files[0].additions, 0);
    }

    #[test]
    fn rename_without_a_body_is_detected() {
        let p = "\
diff --git a/a.txt b/b.txt
similarity index 100%
rename from a.txt
rename to b.txt
";
        let cs = Changeset::parse(p);
        assert_eq!(cs.files.len(), 1);
        let f = &cs.files[0];
        assert_eq!(f.status, FileStatus::Renamed);
        assert_eq!(f.path, "b.txt");
        assert_eq!(f.old_path.as_deref(), Some("a.txt"));
        assert_eq!(f.hunks.len(), 0);
        assert_eq!(f.additions, 0);
        assert_eq!(f.deletions, 0);
    }

    #[test]
    fn copy_is_detected_distinct_from_rename() {
        let p = "\
diff --git a/orig.txt b/dup.txt
similarity index 100%
copy from orig.txt
copy to dup.txt
";
        let cs = Changeset::parse(p);
        assert_eq!(cs.files[0].status, FileStatus::Copied);
        assert_eq!(cs.files[0].path, "dup.txt");
        assert_eq!(cs.files[0].old_path.as_deref(), Some("orig.txt"));
    }

    #[test]
    fn binary_file_is_detected_with_no_stats() {
        let p = "\
diff --git a/img.png b/img.png
index 1111111..2222222 100644
Binary files a/img.png and b/img.png differ
";
        let cs = Changeset::parse(p);
        assert_eq!(cs.files.len(), 1);
        assert_eq!(cs.files[0].status, FileStatus::Binary);
        assert_eq!(cs.files[0].path, "img.png");
        assert_eq!(cs.files[0].additions, 0);
        assert_eq!(cs.files[0].deletions, 0);
        assert_eq!(cs.files[0].hunks.len(), 0);
    }

    #[test]
    fn quoted_path_with_spaces_is_unquoted() {
        let p = "\
diff --git a/with space.txt b/with space.txt
--- \"a/with space.txt\"
+++ \"b/with space.txt\"
@@ -1 +1 @@
-x
+y
";
        let cs = Changeset::parse(p);
        assert_eq!(cs.files.len(), 1);
        assert_eq!(cs.files[0].path, "with space.txt");
        assert_eq!(cs.files[0].status, FileStatus::Modified);
    }

    #[test]
    fn combined_cc_merge_diff_segments() {
        // A `diff --cc` combined merge: two-column body signs. The dominant
        // first char still classifies; the file segments like any other.
        let p = "\
diff --cc merged.txt
index aaa,bbb..ccc
--- a/merged.txt
+++ b/merged.txt
@@@ -1,2 -1,2 +1,3 @@@
  context
- old a
 -old b
++new merged
";
        let cs = Changeset::parse(p);
        assert_eq!(cs.files.len(), 1);
        assert_eq!(cs.files[0].path, "merged.txt");
        assert_eq!(cs.files[0].hunks.len(), 1);
        assert_eq!(cs.files[0].hunks[0].old_start, 1);
        assert_eq!(cs.files[0].hunks[0].new_start, 1);
    }

    #[test]
    fn trailing_no_newline_marker_is_not_counted() {
        let p = "\
diff --git a/f.txt b/f.txt
--- a/f.txt
+++ b/f.txt
@@ -1 +1 @@
-old
\\ No newline at end of file
+new
\\ No newline at end of file
";
        let cs = Changeset::parse(p);
        assert_eq!(cs.files[0].additions, 1, "the \\ marker is not a + line");
        assert_eq!(cs.files[0].deletions, 1, "the \\ marker is not a - line");
    }

    #[test]
    fn crlf_patch_is_normalised() {
        let p = "diff --git a/c.txt b/c.txt\r\n--- a/c.txt\r\n+++ b/c.txt\r\n@@ -1 +1 @@\r\n-a\r\n+b\r\n";
        let cs = Changeset::parse(p);
        assert_eq!(cs.files.len(), 1);
        assert_eq!(cs.files[0].path, "c.txt");
        assert_eq!(cs.files[0].additions, 1);
        assert_eq!(cs.files[0].deletions, 1);
        // The preserved slice is LF-normalised (no stray \r), so Diff reads
        // it cleanly.
        assert!(!cs.files[0].patch().contains('\r'));
    }

    #[test]
    fn header_less_single_patch_is_one_file() {
        // No `diff --git` line at all — a bare `git diff --no-prefix`-ish or
        // hand-written hunk. Treated as one file when it has `@@` hunks.
        let p = "\
--- a/solo.txt
+++ b/solo.txt
@@ -1,2 +1,2 @@
 keep
-before
+after
";
        let cs = Changeset::parse(p);
        assert_eq!(cs.files.len(), 1);
        assert_eq!(cs.files[0].path, "solo.txt");
        assert_eq!(cs.files[0].additions, 1);
        assert_eq!(cs.files[0].deletions, 1);
        assert_eq!(cs.files[0].hunks.len(), 1);
    }

    #[test]
    fn header_less_hunk_only_is_one_file() {
        let p = "@@ -1 +1 @@\n-x\n+y\n";
        let cs = Changeset::parse(p);
        assert_eq!(cs.files.len(), 1);
        assert_eq!(cs.files[0].hunks.len(), 1);
        assert_eq!(cs.files[0].additions, 1);
        assert_eq!(cs.files[0].deletions, 1);
    }

    #[test]
    fn empty_and_garbage_input_is_the_empty_changeset() {
        assert_eq!(Changeset::parse(""), Changeset::default());
        assert_eq!(Changeset::parse("\n\n\n"), Changeset::default());
        // Prose with no diff structure at all → empty.
        assert_eq!(
            Changeset::parse("just some commit message\nwith no patch"),
            Changeset::default()
        );
    }

    #[test]
    fn git_show_preamble_is_dropped_files_still_parse() {
        // `git show -p` prefixes a commit header; it precedes the first
        // `diff --git` and is not part of any file.
        let p = "\
commit deadbeef
Author: A <a@example.com>
Date:   Sun May 18 00:00:00 2026

    a message

diff --git a/x.txt b/x.txt
--- a/x.txt
+++ b/x.txt
@@ -1 +1 @@
-a
+b
";
        let cs = Changeset::parse(p);
        assert_eq!(cs.files.len(), 1);
        assert_eq!(cs.files[0].path, "x.txt");
        assert!(cs.files[0].patch().starts_with("diff --git a/x.txt"));
    }

    #[test]
    fn hunk_index_and_file_of_hunk_order() {
        let cs = Changeset::parse(THREE_FILE);
        // File hunk counts: 1, 2, 1 → 4 global hunks.
        let idx = cs.hunk_index();
        assert_eq!(idx, vec![(0, 0), (1, 0), (1, 1), (2, 0)]);
        assert_eq!(
            idx.len(),
            cs.files.iter().map(|f| f.hunks.len()).sum::<usize>()
        );

        assert_eq!(cs.file_of_hunk(0), Some(0));
        assert_eq!(cs.file_of_hunk(1), Some(1));
        assert_eq!(cs.file_of_hunk(2), Some(1));
        assert_eq!(cs.file_of_hunk(3), Some(2));
        assert_eq!(cs.file_of_hunk(4), None, "past the last hunk");

        // A file with zero hunks (a pure rename) is skipped by file_of_hunk
        // but still occupies a file slot.
        let with_empty =
            format!("diff --git a/r.txt b/s.txt\nrename from r.txt\nrename to s.txt\n{THREE_FILE}");
        let cs2 = Changeset::parse(&with_empty);
        assert_eq!(cs2.files.len(), 4);
        assert_eq!(cs2.files[0].hunks.len(), 0);
        // Global hunk 0 now lives in file 1 (file 0 has none).
        assert_eq!(cs2.file_of_hunk(0), Some(1));
        assert_eq!(cs2.hunk_index().first(), Some(&(1, 0)));
    }

    /// Totality / invariant proptest, the fixed-seed LCG shape `rstui-core`'s
    /// `text_area.rs` uses (rstui is dependency-free — no `rand`). Feeds
    /// random byte/line soups *and* concatenations of the real sample
    /// patches; asserts `parse` never panics and the model invariants hold:
    ///
    /// 1. `sum(file.additions) == total_additions()` (and the `-` twin),
    /// 2. every `HunkRef.patch_lines` is non-empty and within its file's
    ///    slice, and its first line is exactly the hunk header,
    /// 3. `hunk_index().len() == sum(file.hunks.len())` and every
    ///    `file_of_hunk(n)` for `n` in range points at a real file.
    #[test]
    fn parse_is_total_and_keeps_model_invariants() {
        // Fixed-seed LCG — deterministic, no rand dep (the text_area.rs /
        // text_edit.rs / focus.rs technique).
        let mut state: u64 = 0x0bad_f00d_dead_beef;
        let mut rng = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state
        };

        // The line fragments the soup is assembled from: every structural
        // prefix plus pure garbage and multi-byte UTF-8, so the splitter and
        // every recogniser are hit with both well-formed and adversarial
        // input.
        let frags = [
            "diff --git a/x b/x",
            "diff --cc merged",
            "diff --combined m",
            "--- a/x",
            "--- /dev/null",
            "+++ b/y",
            "+++ /dev/null",
            "@@ -1,2 +1,3 @@",
            "@@ -10 +10 @@ section",
            "@@@ -1,2 -1,2 +1,3 @@@",
            "@@ totally broken @@",
            "new file mode 100644",
            "deleted file mode 100644",
            "rename from a",
            "rename to b",
            "copy from c",
            "copy to d",
            "index 111..222 100644",
            "similarity index 90%",
            "Binary files a/p and b/q differ",
            "GIT binary patch",
            "+added line",
            "-removed line",
            " context line",
            "\\ No newline at end of file",
            "",
            "random 日本 garbage 😀",
            "@",
            "@@",
            "+++",
            "---",
        ];
        let samples = [THREE_FILE, "", "@@ -1 +1 @@\n-x\n+y\n"];

        for iter in 0..4_000 {
            // Build a random patch: either a soup of random fragments or a
            // concatenation of the real samples (sometimes truncated).
            let patch = if rng() % 3 == 0 {
                let mut s = String::new();
                for _ in 0..(rng() % 40) {
                    s.push_str(frags[(rng() % frags.len() as u64) as usize]);
                    s.push(if rng() % 5 == 0 { '\r' } else { '\n' });
                }
                s
            } else {
                let mut s = String::new();
                for _ in 0..(1 + rng() % 4) {
                    s.push_str(samples[(rng() % samples.len() as u64) as usize]);
                }
                let cut = (rng() % (s.len() as u64 + 1)) as usize;
                // Truncate on a char boundary so we feed valid UTF-8 (we are
                // testing diff robustness, not String slicing).
                let mut cut = cut.min(s.len());
                while cut > 0 && !s.is_char_boundary(cut) {
                    cut -= 1;
                }
                s.truncate(cut);
                s
            };

            // Invariant: never panics.
            let cs = Changeset::parse(&patch);

            // Invariant 1: per-file stats fold to the totals.
            let add_sum: usize = cs.files.iter().map(|f| f.additions).sum();
            let del_sum: usize = cs.files.iter().map(|f| f.deletions).sum();
            assert_eq!(add_sum, cs.total_additions(), "iter {iter}: additions");
            assert_eq!(del_sum, cs.total_deletions(), "iter {iter}: deletions");

            // Invariant 2: every hunk range is non-empty and inside its
            // file's slice, and its first line is the recorded header.
            for file in &cs.files {
                let slice_lines: Vec<&str> = file.patch().lines().collect();
                for h in &file.hunks {
                    assert!(
                        h.patch_lines.start < h.patch_lines.end,
                        "iter {iter}: empty hunk range {:?}",
                        h.patch_lines
                    );
                    assert!(
                        h.patch_lines.end <= slice_lines.len(),
                        "iter {iter}: hunk range {:?} escapes {}-line slice",
                        h.patch_lines,
                        slice_lines.len()
                    );
                    assert_eq!(
                        slice_lines[h.patch_lines.start], h.header,
                        "iter {iter}: hunk range start is not the header"
                    );
                }
            }

            // Invariant 3: the flat index length is the hunk-count sum and
            // every in-range ordinal resolves to a real file.
            let idx = cs.hunk_index();
            let hunk_total: usize = cs.files.iter().map(|f| f.hunks.len()).sum();
            assert_eq!(idx.len(), hunk_total, "iter {iter}: hunk_index length");
            for (n, &(fi, hi)) in idx.iter().enumerate() {
                assert!(fi < cs.files.len(), "iter {iter}: file index in range");
                assert!(
                    hi < cs.files[fi].hunks.len(),
                    "iter {iter}: hunk index in range"
                );
                assert_eq!(
                    cs.file_of_hunk(n),
                    Some(fi),
                    "iter {iter}: file_of_hunk({n}) disagrees with hunk_index"
                );
            }
            assert_eq!(
                cs.file_of_hunk(hunk_total),
                None,
                "iter {iter}: one past the last hunk is None"
            );
        }
    }
}
