//! `cargo xtask` — rstui workspace automation (the cargo-xtask convention,
//! endorsed by ADR 0003 §7 as the home for project-specific gates that
//! don't fit clippy or rustdoc).
//!
//! Tasks:
//!
//! - `ci` — run the whole project gate sequence (fmt, naming, clippy,
//!   rustdoc, test) with one command, fail-fast, exactly as CI runs it.
//!   This is the loop every contributor and agent stream runs before a
//!   commit; see `docs/development.md`.
//! - `lint-names` — the vague-generic-naming guardrail from the
//!   iteration-19 steering note; see `docs/conventions/naming.md`.
//!
//! Run via `cargo xtask <task>` (the alias is in `.cargo/config.toml`) or
//! `cargo run -p xtask -- <task>`.

mod ci;
mod naming;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
Usage: cargo xtask <TASK>

Tasks:
  ci           Run every project gate (fmt, lint-names, clippy, doc, test)
               fail-fast, exactly as CI does. See docs/development.md.
  lint-names   Fail if any crate name, source path, module, or public
               item uses a banned vague generic name. The default task.
               See docs/conventions/naming.md.
  help         Show this message.";

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("ci") => ci::run(&workspace_root()),
        Some("lint-names") | None => lint_names(),
        Some("help" | "--help" | "-h") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("xtask: unknown task `{other}`\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// `<root>/crates/xtask` → `<root>`. `CARGO_MANIFEST_DIR` is embedded at
/// compile time, so this resolves identically from any working directory
/// (CI checkout, local, nested cwd).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("xtask is at <root>/crates/xtask")
        .to_path_buf()
}

fn lint_names() -> ExitCode {
    if naming::check_and_report(&workspace_root()) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
