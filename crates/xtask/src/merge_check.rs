//! `cargo xtask merge-check` — the pre-merge-back preflight.
//!
//! The recurring red-`main` class had one root cause: streams validated
//! something *other* than "the rebased branch, every gate" before pushing
//! (most often skipping the rustdoc `doc` gate, or merging a stale branch).
//! The hardened protocol in `docs/merging.md` says the right thing; this
//! makes the right thing **one command that cannot be half-done**:
//!
//! 1. you are on a stream branch, not `main`;
//! 2. the working tree is clean (the slice is committed);
//! 3. the branch is rebased on the latest `origin/main` (no stale base);
//! 4. every gate is green — the full `cargo xtask ci`, `doc` included.
//!
//! It then prints an explicit GO / NO-GO. It deliberately does **not** take
//! the lock, touch the main checkout, or push: the dirty-main / conflict /
//! red-merged-`main` decisions need judgment and must not be auto-forced
//! (that is exactly the "never force state" rule). GO means "now run the
//! serialized merge-back"; NO-GO names the one thing to fix first.

use std::path::Path;
use std::process::{Command, ExitCode};

use crate::ci;

/// Run `git` with `args` in `root`, capturing stdout. `None` if git could
/// not be launched; otherwise `(success, trimmed stdout)`.
fn git(root: &Path, args: &[&str]) -> Option<(bool, String)> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    Some((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
    ))
}

/// Emit the NO-GO banner naming the failed invariant and its remedy.
fn no_go(invariant: &str, remedy: &str) -> ExitCode {
    eprintln!("\n✗ xtask merge-check: NO-GO — {invariant}");
    eprintln!("  fix: {remedy}");
    eprintln!("  (nothing was pushed; the main checkout was not touched.)");
    ExitCode::FAILURE
}

/// Preflight the current stream worktree for a serialized merge-back.
pub(crate) fn run(root: &Path) -> ExitCode {
    println!("━━━ xtask merge-check — preflight before the serialized merge-back ━━━");

    // 1. On a stream branch, not the main checkout.
    let Some((_, branch)) = git(root, &["branch", "--show-current"]) else {
        return no_go("could not run `git`", "ensure git is installed and on PATH");
    };
    if branch.is_empty() || branch == "main" {
        return no_go(
            &format!("on branch `{branch}`, not a stream branch"),
            "run this from your stream worktree (a `stream/…` branch)",
        );
    }
    println!("✓ on stream branch `{branch}`");

    // 2. Working tree clean — the slice must already be committed.
    match git(root, &["status", "--porcelain"]) {
        Some((_, s)) if s.is_empty() => println!("✓ working tree clean"),
        Some((_, _)) => {
            return no_go(
                "working tree has uncommitted changes",
                "commit the slice first (a merge-back pushes commits, not your tree)",
            );
        }
        None => return no_go("could not run `git status`", "ensure git works here"),
    }

    // 3. Rebased on the latest origin/main (no stale base — the thing that
    //    silently turned `main` red).
    println!("• fetching origin/main…");
    if matches!(
        git(root, &["fetch", "origin", "main"]),
        Some((false, _)) | None
    ) {
        return no_go(
            "could not `git fetch origin main`",
            "check network/remote, then retry",
        );
    }
    match git(
        root,
        &["merge-base", "--is-ancestor", "origin/main", "HEAD"],
    ) {
        Some((true, _)) => println!("✓ branch is rebased on the latest origin/main"),
        Some((false, _)) => {
            return no_go(
                "origin/main has moved — your branch is on a stale base",
                "git rebase origin/main, re-run merge-check",
            );
        }
        None => return no_go("could not run `git merge-base`", "ensure git works here"),
    }

    // 4. Every gate green — the full loop, doc gate included.
    println!("• running the full gate (cargo xtask ci)…\n");
    if !ci::run_all(root) {
        return no_go(
            "a gate failed (see the banner above)",
            "fix the reported gate, re-run merge-check",
        );
    }

    println!(
        "\n✓ xtask merge-check: GO — rebased, clean, all gates green.\n  \
         Now run the serialized merge-back (docs/merging.md): take the lock, \
         merge --no-ff into the main checkout, re-run `cargo xtask ci` THERE, \
         push only if it is green, release the lock, rebase."
    );
    ExitCode::SUCCESS
}
