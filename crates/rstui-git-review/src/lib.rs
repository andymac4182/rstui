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

/// What [`init`](rstui_runtime::App::init)'s first load resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    /// Newest-first commit list.
    pub commits: Vec<Commit>,
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

/// `git log` → newest-first [`Commit`]s, plus the current branch. Fields are
/// joined with US (`\x1f`) so a subject containing any normal punctuation
/// still parses; rows are newline-separated.
///
/// # Errors
///
/// Propagates the `git` helper's error when the directory is not a repository, `git`
/// is absent, or the revision range is invalid.
pub fn load(repo: &Path, rev: Option<&str>) -> Result<Loaded, String> {
    let mut args: Vec<&str> = vec![
        "log",
        "--no-color",
        "--date=short",
        "--pretty=format:%H%x1f%h%x1f%s%x1f%an%x1f%ad",
        "-n",
        "400",
    ];
    if let Some(r) = rev {
        args.push(r);
    }
    let raw = git(repo, &args)?;
    let commits: Vec<Commit> = raw
        .lines()
        .filter_map(|line| {
            let mut f = line.split('\u{1f}');
            Some(Commit {
                sha: f.next()?.to_owned(),
                short: f.next()?.to_owned(),
                subject: f.next().unwrap_or_default().to_owned(),
                author: f.next().unwrap_or_default().to_owned(),
                date: f.next().unwrap_or_default().to_owned(),
            })
        })
        .collect();
    if commits.is_empty() {
        return Err("no commits in range".to_owned());
    }
    let branch = git(repo, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .filter(|b| b != "HEAD" && !b.is_empty())
        .or_else(|| git(repo, &["rev-parse", "--short", "HEAD"]).ok())
        .unwrap_or_else(|| "?".to_owned());
    Ok(Loaded { commits, branch })
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
    rstui_crossterm::run_app(GitReview::new(config))?;
    Ok(())
}
