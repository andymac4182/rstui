//! `cargo xtask ci` — run the whole project gate sequence locally with one
//! command, in the same order and with the same invocations CI runs, fail-fast
//! with a clear per-gate banner so an agent (or human) sees *exactly* which
//! gate failed and what it catches without scrolling.
//!
//! rstui values "what CI runs == what you run locally" (see
//! `.cargo/config.toml` and ADR 0003 §7). Before this task that contract was
//! a *list of commands a contributor had to remember and run by hand* — five
//! separate invocations, easy to run partially or in the wrong order. This
//! collapses the loop to a single command whose pass/fail and timing per gate
//! are unambiguous, which is the loop every parallel agent stream runs before
//! every commit. The naming gate is run **in-process** (not by shelling out to
//! `cargo run -p xtask`) so `xtask ci` never recursively re-invokes itself.

use std::path::Path;
use std::process::{Command, ExitCode};
use std::time::Instant;

use crate::naming;

/// How a single gate is executed.
enum Gate {
    /// Shell out to `cargo <args…>`, with optional extra environment (used for
    /// the rustdoc gate's `RUSTDOCFLAGS=-D warnings`). Stdio is inherited so
    /// the underlying tool's output streams live.
    Cargo {
        /// Arguments passed to `cargo` (e.g. `["fmt", "--all", "--check"]`).
        args: &'static [&'static str],
        /// Extra `(key, value)` environment for this invocation only.
        env: &'static [(&'static str, &'static str)],
    },
    /// The vague-generic-name guardrail, run in-process via [`naming::scan`]
    /// — identical to `cargo xtask lint-names`, but without the recursive
    /// `cargo run -p xtask` that shelling out would cause.
    LintNames,
}

/// One gate in the sequence: a short label, a one-line description of the
/// defect class it catches (shown in the banner so a failure is
/// self-explanatory), and how to run it.
struct Step {
    /// Short stable identifier, e.g. `clippy`. Shown in banners and the
    /// final summary.
    label: &'static str,
    /// One line naming the defect class this gate exists to catch.
    catches: &'static str,
    /// How this gate is executed.
    gate: Gate,
}

/// The gate sequence, fail-fast.
///
/// Coverage is exactly CI's (`.github/workflows/ci.yml`): fmt, clippy,
/// naming, rustdoc, test. The only ordering difference from CI is that the
/// instant in-process naming scan is hoisted ahead of the slow clippy build,
/// so a naming mistake fails in milliseconds instead of after a full clippy
/// compile. Every other gate keeps CI's order, so the first failure here is
/// the first failure CI would report.
const STEPS: &[Step] = &[
    Step {
        label: "fmt",
        catches: "unformatted code (`cargo fmt --all`)",
        gate: Gate::Cargo {
            args: &["fmt", "--all", "--check"],
            env: &[],
        },
    },
    Step {
        label: "lint-names",
        catches: "banned vague generic names (docs/conventions/naming.md)",
        gate: Gate::LintNames,
    },
    Step {
        label: "clippy",
        catches: "clippy lints, denied (`-D warnings`)",
        gate: Gate::Cargo {
            args: &[
                "clippy",
                "--all-targets",
                "--all-features",
                "--",
                "-D",
                "warnings",
            ],
            env: &[],
        },
    },
    Step {
        label: "doc",
        catches: "broken intra-doc links / missing docs (ADR 0003 §4)",
        gate: Gate::Cargo {
            args: &["doc", "--no-deps", "--all-features", "--workspace"],
            env: &[("RUSTDOCFLAGS", "-D warnings")],
        },
    },
    Step {
        label: "test",
        catches: "failing unit tests, integration tests and doctests",
        gate: Gate::Cargo {
            args: &["test", "--all-features"],
            env: &[],
        },
    },
];

/// The `cargo` binary to drive sub-gates with: cargo sets `$CARGO` to its own
/// path when it invokes an xtask, so reusing it pins every gate to the same
/// toolchain that launched `xtask`; `cargo` on `PATH` is the fallback. Shared
/// with the `bench` task so it builds `rstui-bench` with that same toolchain.
pub(crate) fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

/// Run one external `cargo` gate, inheriting stdio. `true` on a clean exit.
fn run_cargo(cargo: &str, root: &Path, args: &[&str], env: &[(&str, &str)]) -> bool {
    let mut cmd = Command::new(cargo);
    cmd.args(args).current_dir(root);
    for (key, value) in env {
        cmd.env(key, value);
    }
    match cmd.status() {
        Ok(status) => status.success(),
        Err(err) => {
            eprintln!("  could not launch `{cargo}`: {err}");
            false
        }
    }
}

/// Run the full gate sequence rooted at `root`, fail-fast. `true` only if
/// every gate passes. Output is identical to what a contributor expects from
/// `cargo xtask ci`; the `bool` lets callers (the `ci` task wrapper, and
/// `merge-check`) branch without `ExitCode` (which is not comparable).
pub(crate) fn run_all(root: &Path) -> bool {
    let cargo = cargo_bin();
    let total = Instant::now();
    let count = STEPS.len();

    for (idx, step) in STEPS.iter().enumerate() {
        println!(
            "\n━━━ xtask ci [{}/{count}] {} — catches {} ━━━",
            idx + 1,
            step.label,
            step.catches
        );
        let started = Instant::now();
        let passed = match &step.gate {
            Gate::LintNames => naming::check_and_report(root),
            Gate::Cargo { args, env } => run_cargo(&cargo, root, args, env),
        };
        let secs = started.elapsed().as_secs_f64();

        if passed {
            println!("✓ {} passed ({secs:.1}s)", step.label);
        } else {
            eprintln!(
                "\n✗ xtask ci: gate `{}` FAILED after {secs:.1}s — fix the {} \
                 reported above, then re-run `cargo xtask ci`.",
                step.label, step.catches
            );
            eprintln!(
                "  (failed at gate {}/{count}; later gates were skipped — \
                 fail-fast.)",
                idx + 1
            );
            return false;
        }
    }

    println!(
        "\n✓ xtask ci: all {count} gates passed in {:.1}s — this is exactly \
         what CI enforces.",
        total.elapsed().as_secs_f64()
    );
    true
}

/// [`run_all`] as a process [`ExitCode`] — the `cargo xtask ci` entry point.
pub(crate) fn run(root: &Path) -> ExitCode {
    if run_all(root) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sequence must cover precisely CI's **`check`-job** gate set. If a
    /// future slice removes a gate from `xtask ci` (silently weakening the
    /// local loop) or adds an unlabelled one, this fails — the gate set is a
    /// contract, not an incidental list. CI's separate `msrv` / `unused-deps`
    /// legs (ADR 0003 §7/§8) are deliberately *not* gates and are not in this
    /// set; see `docs/development.md` ("Additional CI legs").
    #[test]
    fn step_set_matches_ci_gates() {
        let labels: Vec<&str> = STEPS.iter().map(|s| s.label).collect();
        assert_eq!(
            labels,
            ["fmt", "lint-names", "clippy", "doc", "test"],
            "xtask ci must run exactly CI's check-job gates; update \
             .github/workflows/ci.yml and docs/development.md together if this \
             changes (the msrv/unused-deps CI legs are separate, not gates)"
        );
    }

    /// Every banner field is non-empty and labels are unique and lowercase —
    /// the banner is the only thing an agent reads on failure, so it must be
    /// well-formed.
    #[test]
    fn steps_are_well_formed() {
        let mut seen = Vec::new();
        for step in STEPS {
            assert!(!step.label.is_empty(), "empty step label");
            assert!(
                !step.catches.is_empty(),
                "step `{}` has no `catches` description",
                step.label
            );
            assert_eq!(
                step.label,
                step.label.to_ascii_lowercase(),
                "step label `{}` must be lowercase",
                step.label
            );
            assert!(
                !seen.contains(&step.label),
                "duplicate step label `{}`",
                step.label
            );
            seen.push(step.label);
        }
    }

    /// Benchmarks are deliberately *not* a gate: `xtask ci` must stay fast
    /// for the inner loop (ADR 0005). If a future slice folds a `bench` step
    /// into the gate sequence, this fails — the fast-loop / slow-loop split
    /// is a contract, not an accident.
    #[test]
    fn bench_is_never_a_ci_gate() {
        assert!(
            !STEPS.iter().any(|s| s.label.contains("bench")),
            "benchmarks must stay out of `xtask ci`; run them via `xtask bench`"
        );
    }

    /// Exactly one gate is the in-process naming scan; everything else is a
    /// `cargo` invocation. This pins the no-recursion design (the naming gate
    /// must never become a `cargo run -p xtask` shell-out).
    #[test]
    fn naming_gate_is_in_process_and_unique() {
        let in_process = STEPS
            .iter()
            .filter(|s| matches!(s.gate, Gate::LintNames))
            .count();
        assert_eq!(
            in_process, 1,
            "exactly one gate must be the in-process naming scan"
        );
    }
}
