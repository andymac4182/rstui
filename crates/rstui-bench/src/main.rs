//! `rstui-bench` — the rstui hot-path benchmark runner.
//!
//! A deterministic, dependency-free timing aid (ADR 0005) over `rstui-core`'s
//! public API. It is **not** a statistical benchmark and is **not** a CI gate:
//! `cargo xtask ci` stays fast precisely because this lives outside it. Run it
//! through the wrapper so it is always built in release:
//!
//! ```sh
//! cargo xtask bench                 # every scenario
//! cargo xtask bench buffer/diff     # only scenarios whose name contains this
//! cargo xtask bench --list          # list scenario names
//! ```
//!
//! Output is a fixed-width table of `min` / `median` / `mean` per iteration.
//! Compare `min` across runs to spot a regression; the full workflow
//! (including CPU/memory profiling on macOS and Linux) is in
//! `docs/benchmarking.md`.

mod measure;
mod scenarios;

use std::process::ExitCode;

use measure::{Bench, humanize};
use scenarios::{SCENARIOS, Scenario};

/// `--help` / `-h` text. Mirrors the flags `main` actually parses.
const HELP: &str = "\
Usage: rstui-bench [FILTER] [--list]
       cargo xtask bench [FILTER]

Runs the rstui hot-path benchmarks. FILTER, if given, keeps only scenarios
whose name contains it as a substring (e.g. `buffer/diff`).

Environment:
  RSTUI_BENCH_ITERS    measured iterations per scenario (default 1000)
  RSTUI_BENCH_WARMUP   untimed warmup iterations       (default 100)

Options:
  --list   print scenario names and exit
  --json   emit one machine-readable JSON object (for `cargo xtask perf`)
  --help   print this message and exit

This is a deterministic, dependency-free timing aid, not a statistical
benchmark. See docs/benchmarking.md and
docs/adr/0005-benchmarking-and-profiling-strategy.md.";

/// Parse `key` as a `u32`, falling back to `default` for unset or unparsable
/// values so a typo never silently runs zero iterations.
fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|raw| raw.parse().ok())
        .filter(|&v| v > 0)
        .unwrap_or(default)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{HELP}");
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|a| a == "--list") {
        for (name, _) in SCENARIOS {
            println!("{name}");
        }
        return ExitCode::SUCCESS;
    }
    // Machine output for `cargo xtask perf`: exact ns, no humanize
    // round-trip, no header/notes (so the consumer parses stdout cleanly).
    let json = args.iter().any(|a| a == "--json");

    let filter = args.into_iter().find(|a| !a.starts_with('-'));
    let want = filter.as_deref();
    let mut selected: Vec<(&str, Scenario)> = Vec::new();
    for &(name, scenario) in SCENARIOS {
        if want.is_none_or(|w| name.contains(w)) {
            selected.push((name, scenario));
        }
    }
    if selected.is_empty() {
        eprintln!(
            "rstui-bench: no scenario matches filter {:?}; run with --list.",
            want.unwrap_or_default()
        );
        return ExitCode::FAILURE;
    }

    let bench = Bench {
        warmup: env_u32("RSTUI_BENCH_WARMUP", 100),
        iters: env_u32("RSTUI_BENCH_ITERS", 1000),
    };

    if json {
        // A flat object: scenario → {min_ns, median_ns, mean_ns}. Scenario
        // names are static `/`-segmented identifiers (no `"`/`\`/control
        // chars), so they need no JSON escaping. Stable key order =
        // selection order, so a textual diff of two runs is meaningful.
        print!("{{");
        for (i, (name, scenario)) in selected.into_iter().enumerate() {
            let s = scenario(&bench);
            if i > 0 {
                print!(",");
            }
            print!(
                "\"{name}\":{{\"min_ns\":{},\"median_ns\":{},\"mean_ns\":{}}}",
                s.min_ns, s.median_ns, s.mean_ns
            );
        }
        println!("}}");
        return ExitCode::SUCCESS;
    }

    let build = if cfg!(debug_assertions) {
        "debug — prefer `cargo xtask bench` for a release build"
    } else {
        "release"
    };
    println!(
        "rstui-bench — {} scenario(s), {} iters, {} warmup ({build})",
        selected.len(),
        bench.iters,
        bench.warmup
    );
    println!(
        "{:<34}{:>13}{:>13}{:>13}",
        "scenario", "min", "median", "mean"
    );
    for (name, scenario) in selected {
        let stats = scenario(&bench);
        println!(
            "{name:<34}{:>13}{:>13}{:>13}",
            humanize(stats.min_ns),
            humanize(stats.median_ns),
            humanize(stats.mean_ns)
        );
    }
    println!("\nnote: deterministic timing aid, not a statistical benchmark.");
    println!("      see docs/benchmarking.md + ADR 0005 for the full workflow.");
    ExitCode::SUCCESS
}
