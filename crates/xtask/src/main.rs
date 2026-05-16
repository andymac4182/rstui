//! `cargo xtask` — rstui workspace automation (the cargo-xtask convention,
//! endorsed by ADR 0003 §7 as the home for project-specific gates that
//! don't fit clippy or rustdoc).
//!
//! Today it hosts one task: `lint-names`, the vague-generic-naming
//! guardrail from the iteration-19 steering note. Run it with
//! `cargo xtask lint-names` (the alias is in `.cargo/config.toml`) or
//! `cargo run -p xtask -- lint-names`. The convention itself is documented
//! in `docs/conventions/naming.md`.

mod naming;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
Usage: cargo xtask <TASK>

Tasks:
  lint-names   Fail if any crate name, source path, module, or public
               item uses a banned vague generic name. The default task.
               See docs/conventions/naming.md.
  help         Show this message.";

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
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
    let violations = naming::scan(&workspace_root());
    if violations.is_empty() {
        println!(
            "xtask lint-names: OK — no banned vague generic names in crate \
             names, source paths, modules, or public items."
        );
        return ExitCode::SUCCESS;
    }
    eprintln!(
        "xtask lint-names: {} banned vague generic name(s) found.\n\
         The convention and how to register a documented exception: \
         docs/conventions/naming.md\n",
        violations.len()
    );
    for v in &violations {
        eprintln!(
            "  {} [{}] `{}` contains banned generic segment `{}`",
            v.location,
            v.kind.label(),
            v.name,
            v.banned
        );
    }
    ExitCode::FAILURE
}
