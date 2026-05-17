//! `rstui-git-review` — the full-screen git history review + code editing
//! binary.
//!
//! ```text
//! rstui-git-review                 # review the repo in the current directory
//! rstui-git-review path/to/repo    # review another working tree
//! rstui-git-review -- main~20..    # restrict history to a revision range
//! ```
//!
//! The same [`GitReview`](rstui_git_review::GitReview) App the headless
//! `Harness` tests drive, run on a real terminal through
//! [`rstui_crossterm::run_app`] — alternate screen, raw mode, off-loop
//! `git` effects, and panic-safe restore, all in one call.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return ExitCode::SUCCESS;
    }

    let config = rstui_git_review::Config::from_args(args);
    match rstui_git_review::run(config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            rstui_crossterm::restore_terminal();
            eprintln!("rstui-git-review: {err}");
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    println!(
        "rstui-git-review — review git history and edit the working tree\n\n\
         USAGE:\n  rstui-git-review [REPO] [-- <git log revision range>]\n\n\
         ARGS:\n\
         \x20 REPO            Working tree to review (default: current directory)\n\
         \x20 -- <range>      Restrict history, e.g. `-- main~20..` or `-- v1.0..HEAD`\n\n\
         KEYS:\n\
         \x20 [ / ]           Previous / next commit        Tab   Switch focus\n\
         \x20 j / k           Move selection / scroll diff  e     Edit first changed file\n\
         \x20 g / G           Newest / oldest commit        Ctrl-S Save (Edit mode)\n\
         \x20 ?               Help                          q     Quit"
    );
}
