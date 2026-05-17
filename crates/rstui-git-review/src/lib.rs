//! `rstui-git-review` — a full-screen **git history review + code editing**
//! TUI built on rstui.
//!
//! It is the worked proof of the thesis in
//! [`docs/code-review-and-editing.md`](https://github.com/andymac4182/rstui/blob/main/docs/code-review-and-editing.md):
//! a code-review/editing tool needs *no* new framework widgets — the existing
//! [`List`](rstui_widgets::List) (commit history),
//! [`Diff`](rstui_widgets::Diff) (the per-commit patch),
//! [`Editor`](rstui_widgets::Editor) over a caller-owned
//! [`TextArea`](rstui_core::TextArea) (editing), plus
//! [`StatusBar`](rstui_widgets::StatusBar) /
//! [`HelpOverlay`](rstui_widgets::HelpOverlay) chrome compose the whole app,
//! and `git` is reached only as a **`Cmd`-seam subprocess** (the roadmap's
//! "git invocation is a `Cmd`/app concern, not a widget").
//!
//! ```text
//! rstui-git-review                 # review the repo in the current directory
//! rstui-git-review path/to/repo    # review another working tree
//! rstui-git-review -- main~20..    # restrict history to a revision range
//! ```
//!
//! The crate is split so the UI logic is deterministically testable headlessly
//! ([`GitReview`] driven by [`Harness`](rstui_runtime::Harness)) while
//! [`run`] owns the live crossterm lifecycle.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

mod app;
mod theme;

pub use app::{GitReview, Msg};

/// One parsed `git log` row. Plain owned data so it crosses the
/// [`Cmd::perform`](rstui_runtime::Cmd::perform) thread boundary freely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// Full 40-char object name.
    pub sha: String,
    /// Abbreviated object name (`git`'s own `%h`).
    pub short: String,
    /// First line of the commit message.
    pub subject: String,
    /// Author name (`%an`).
    pub author: String,
    /// Author date, `YYYY-MM-DD` (`%ad` with `--date=short`).
    pub date: String,
}

/// One physical row of `git log` output. With the graph on, `git` emits the
/// ASCII DAG: commit rows carry the `art` *and* a [`Commit`]; pure connector
/// rows (`|/`, `|\`, `| |`) carry only `art` and no commit, so the visual
/// tree is preserved while navigation still moves commit-to-commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRow {
    /// The leading graph-art column (empty when the graph is off).
    pub art: String,
    /// The commit on this row, or `None` for a pure connector row.
    pub commit: Option<Commit>,
}

/// What [`init`](rstui_runtime::App::init)'s first load resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    /// Newest-first rows (commit + connector rows, in `git log` order).
    pub rows: Vec<LogRow>,
    /// Current branch (or a short HEAD) for the status bar.
    pub branch: String,
}

/// Launch configuration parsed from argv.
#[derive(Debug, Clone)]
pub struct Config {
    /// The working tree to review (defaults to the current directory).
    pub repo: PathBuf,
    /// An optional `git log` revision range/argument (e.g. `main~20..`).
    pub rev: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            repo: PathBuf::from("."),
            rev: None,
        }
    }
}

impl Config {
    /// Parse `args` (argv minus the program name).
    ///
    /// The first non-`--` token is the repo path; everything after a literal
    /// `--` is treated as the revision range.
    #[must_use]
    pub fn from_args<I: IntoIterator<Item = String>>(args: I) -> Self {
        let mut cfg = Self::default();
        let mut after_ddash = false;
        let mut saw_repo = false;
        for a in args {
            if !after_ddash && a == "--" {
                after_ddash = true;
            } else if after_ddash {
                cfg.rev = Some(match cfg.rev.take() {
                    Some(r) => format!("{r} {a}"),
                    None => a,
                });
            } else if !saw_repo {
                cfg.repo = PathBuf::from(a);
                saw_repo = true;
            }
        }
        cfg
    }
}

/// Run one `git` invocation in `repo`, returning trimmed stdout or a
/// human-readable error. Never panics: a missing `git`, a non-repo directory,
/// or a non-zero exit all map to `Err(String)` the UI renders as a panel.
fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-c")
        .arg("color.ui=never")
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|e| format!("could not run `git` ({e}) — is it installed and on PATH?"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_owned())
    } else {
        let msg = String::from_utf8_lossy(&out.stderr);
        let msg = msg.trim();
        Err(if msg.is_empty() {
            "git exited non-zero (not a git repository?)".to_owned()
        } else {
            msg.to_owned()
        })
    }
}

/// `git log` → newest-first [`LogRow`]s, plus the current branch.
///
/// With `graph` on, `git log --graph` draws the commit DAG; each physical
/// line is one [`LogRow`]. The format begins with US (`\x1f`) so a commit
/// line is `<art>\x1f<H>\x1f<h>\x1f<s>\x1f<an>\x1f<ad>` and a pure connector
/// line has no `\x1f` at all — splitting on the first US cleanly separates
/// the graph art from the (optional) commit fields, and a subject with any
/// normal punctuation still parses.
///
/// # Errors
///
/// Propagates the `git` helper's error when the directory is not a
/// repository, `git` is absent, or the revision range is invalid.
pub fn load(repo: &Path, rev: Option<&str>, graph: bool) -> Result<Loaded, String> {
    let mut args: Vec<&str> = vec!["log", "--no-color", "--date=short"];
    if graph {
        args.push("--graph");
    }
    args.extend_from_slice(&[
        "--pretty=format:%x1f%H%x1f%h%x1f%s%x1f%an%x1f%ad",
        "-n",
        "400",
    ]);
    if let Some(r) = rev {
        args.push(r);
    }
    let raw = git(repo, &args)?;
    let mut rows: Vec<LogRow> = Vec::new();
    let mut any_commit = false;
    for line in raw.lines() {
        if let Some((art, rest)) = line.split_once('\u{1f}') {
            let mut f = rest.split('\u{1f}');
            let mut next = || f.next().unwrap_or_default().to_owned();
            let sha = next();
            if sha.is_empty() {
                rows.push(LogRow {
                    art: art.to_owned(),
                    commit: None,
                });
                continue;
            }
            any_commit = true;
            rows.push(LogRow {
                art: art.to_owned(),
                commit: Some(Commit {
                    sha,
                    short: next(),
                    subject: next(),
                    author: next(),
                    date: next(),
                }),
            });
        } else {
            // A pure connector row (`|/`, `|\`, `| |`): keep it for the tree.
            rows.push(LogRow {
                art: line.to_owned(),
                commit: None,
            });
        }
    }
    if !any_commit {
        return Err("no commits in range".to_owned());
    }
    let branch = git(repo, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .filter(|b| b != "HEAD" && !b.is_empty())
        .or_else(|| git(repo, &["rev-parse", "--short", "HEAD"]).ok())
        .unwrap_or_else(|| "?".to_owned());
    Ok(Loaded { rows, branch })
}

/// The unified patch for one commit (`git show -p`), message stripped so the
/// [`Diff`](rstui_widgets::Diff) widget gets a clean patch. The commit
/// subject/author is shown by the UI from the [`Commit`] row, not here.
///
/// # Errors
///
/// Propagates the `git` helper's error if the object does not resolve.
pub fn show(repo: &Path, sha: &str) -> Result<String, String> {
    git(repo, &["show", "--no-color", "--format=", "-p", sha])
}

/// The files a commit touched: `(status, path)` where status is git's
/// `--name-status` letter (`A`/`M`/`D`/`R…`).
///
/// # Errors
///
/// Propagates the `git` helper's error if the object does not resolve.
pub fn changed_files(repo: &Path, sha: &str) -> Result<Vec<(String, String)>, String> {
    let raw = git(
        repo,
        &["show", "--no-color", "--name-status", "--format=", sha],
    )?;
    Ok(raw
        .lines()
        .filter_map(|l| {
            let mut p = l.split('\t');
            let status = p.next()?.trim().to_owned();
            // Rename rows are `R100\told\tnew`; the reviewed path is the last.
            let path = p.next_back()?.trim().to_owned();
            if status.is_empty() || path.is_empty() {
                None
            } else {
                Some((status, path))
            }
        })
        .collect())
}

/// Read a working-tree file for editing (the live file, not the historical
/// blob — this is a *code editing* tool, not just a viewer).
///
/// # Errors
///
/// Returns a message if the path escapes the repo, is missing, or is not
/// UTF-8 text.
pub fn read_file(repo: &Path, rel: &str) -> Result<String, String> {
    if rel.is_empty() || Path::new(rel).is_absolute() || rel.split('/').any(|c| c == "..") {
        return Err(format!("refusing to open path outside the repo: {rel}"));
    }
    let full = repo.join(rel);
    match std::fs::read(&full) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|_| format!("{rel} is not UTF-8 text — cannot edit it here")),
        Err(e) => Err(format!("cannot open {rel}: {e}")),
    }
}

/// Write an edited buffer back to the working-tree file.
///
/// # Errors
///
/// Returns a message on the same guards as [`read_file`] or any I/O failure.
pub fn write_file(repo: &Path, rel: &str, contents: &str) -> Result<(), String> {
    if rel.is_empty() || Path::new(rel).is_absolute() || rel.split('/').any(|c| c == "..") {
        return Err(format!("refusing to write path outside the repo: {rel}"));
    }
    std::fs::write(repo.join(rel), contents).map_err(|e| format!("cannot save {rel}: {e}"))
}

/// Run the full-screen app on a real terminal.
///
/// Delegates to [`rstui_crossterm::run_app`], which owns the alternate
/// screen, raw mode, mouse/paste/focus capture, the off-loop event loop
/// (so a slow `git show` never freezes the UI), and panic-safe restore.
///
/// # Errors
///
/// Returns the crossterm lifecycle error if the terminal cannot be entered or
/// driven; the terminal is already restored on every return path.
pub fn run(config: Config) -> Result<(), Box<dyn Error>> {
    let mut app = GitReview::new(config);
    // `RSTUI_KEYMAP=<map name|/path/to/keymap>` remaps commands without a
    // rebuild or the in-app panel — the same typo-safe seam as `RSTUI_THEME`
    // (see docs/keymaps.md). Headless `Harness` tests use `GitReview::new`
    // directly, so they are unaffected.
    if let Ok(km) = std::env::var("RSTUI_KEYMAP") {
        app = app.with_keymap(&km);
    }
    rstui_crossterm::run_app(app)?;
    Ok(())
}
