//! `cargo xtask bench` — build and run the `rstui-bench` hot-path benchmark
//! crate in **release**, forwarding any extra arguments through to it.
//!
//! This is the deliberate *slow loop*. ADR 0005 keeps benchmarks out of
//! `xtask ci` so the gate loop every agent stream runs before every commit
//! stays fast; an agent reaches for this only when changing a hot path. The
//! wrapper exists so "run the benchmarks" is one command that is *always* a
//! release build (a debug build would report meaningless numbers) and is
//! pinned to the same toolchain that launched `xtask` (the `cargo_bin` helper
//! shared with the `ci` module).

use std::path::Path;
use std::process::{Command, ExitCode};

use crate::ci::cargo_bin;

/// Build and run `rstui-bench` in release from `root`, forwarding `extra`
/// (a scenario substring filter, `--list`, `--help`, …) to the binary.
/// Stdio is inherited so the table streams live. `--quiet` suppresses cargo's
/// compile chatter so the first thing printed is the benchmark header.
pub(crate) fn run(root: &Path, extra: &[String]) -> ExitCode {
    let cargo = cargo_bin();
    let mut cmd = Command::new(&cargo);
    cmd.current_dir(root)
        .args([
            "run",
            "--quiet",
            "--release",
            "--package",
            "rstui-bench",
            "--",
        ])
        .args(extra);
    match cmd.status() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(err) => {
            eprintln!("xtask bench: could not launch `{cargo}`: {err}");
            ExitCode::FAILURE
        }
    }
}
