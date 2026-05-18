//! `cargo xtask perf` — the repeatable perf review (ADR 0018 §4).
//!
//! Runs `rstui-bench --json` in release, then either **saves** the result
//! as `docs/perf-baseline.json` or **diffs** the current run against that
//! baseline and prints a regression report. This is deliberately *not* a
//! `cargo xtask ci` gate (ADR 0005: timing is environment-sensitive;
//! gating on it makes CI flaky) — it is an on-demand report, run when you
//! touched a hot path or want a periodic review.
//!
//! ```sh
//! cargo xtask perf                 # diff vs baseline, print the report
//! cargo xtask perf --save          # (re)write docs/perf-baseline.json
//! cargo xtask perf --check         # exit non-zero if any scenario
//!                                  # regressed past the threshold
//! cargo xtask perf widget/         # restrict to a scenario substring
//! ```
//!
//! Threshold: a scenario's `min` (the cleanest cross-run signal, ADR
//! 0005) regressing by more than `RSTUI_PERF_THRESHOLD` percent
//! (default 10) is flagged; `--check` makes that a non-zero exit.

use std::path::Path;
use std::process::{Command, ExitCode};

use crate::ci::cargo_bin;

/// `(scenario, min_ns, median_ns, mean_ns)` for one run.
type Row = (String, u64, u64, u64);

/// Parses the fixed compact object `rstui-bench --json` emits:
/// `{"name":{"min_ns":N,"median_ns":N,"mean_ns":N},...}`. Tailored to that
/// exact shape (we produce it) — no general JSON parser, no dependency.
/// Malformed entries are skipped rather than aborting the whole report.
fn parse(json: &str) -> Vec<Row> {
    let mut out = Vec::new();
    // Strip exactly one outer brace each side — `trim_*_matches` is greedy
    // and would also eat the *last* entry's adjacent inner `}` (`…}}`).
    let trimmed = json.trim();
    let trimmed = trimmed.strip_prefix('{').unwrap_or(trimmed);
    let mut rest = trimmed.strip_suffix('}').unwrap_or(trimmed);
    while let Some(q0) = rest.find('"') {
        let after = &rest[q0 + 1..];
        let Some(qend) = after.find('"') else { break };
        let key = after[..qend].to_string();
        let tail = &after[qend + 1..];
        let Some(ob) = tail.find('{') else { break };
        let Some(cb) = tail[ob..].find('}') else {
            break;
        };
        let obj = &tail[ob + 1..ob + cb];
        let mut min = 0;
        let mut med = 0;
        let mut mean = 0;
        for field in obj.split(',') {
            let Some((k, v)) = field.split_once(':') else {
                continue;
            };
            let n: u64 = v.trim().parse().unwrap_or(0);
            match k.trim().trim_matches('"') {
                "min_ns" => min = n,
                "median_ns" => med = n,
                "mean_ns" => mean = n,
                _ => {}
            }
        }
        out.push((key, min, med, mean));
        rest = &tail[ob + cb + 1..];
    }
    out
}

/// Serialises rows back to the exact `--json` compact format, so the
/// checked-in baseline round-trips through [`parse`] unchanged.
fn to_json(rows: &[Row]) -> String {
    let mut s = String::from("{");
    for (i, (name, min, med, mean)) in rows.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "\"{name}\":{{\"min_ns\":{min},\"median_ns\":{med},\"mean_ns\":{mean}}}"
        ));
    }
    s.push_str("}\n");
    s
}

/// Human ns → `ns`/`µs`/`ms` (mirrors `rstui-bench`'s `humanize`).
fn human(ns: u64) -> String {
    if ns < 1_000 {
        format!("{ns}ns")
    } else if ns < 1_000_000 {
        format!("{:.2}µs", ns as f64 / 1_000.0)
    } else {
        format!("{:.2}ms", ns as f64 / 1_000_000.0)
    }
}

pub(crate) fn run(root: &Path, extra: &[String]) -> ExitCode {
    let save = extra.iter().any(|a| a == "--save");
    let check = extra.iter().any(|a| a == "--check");
    let threshold: f64 = std::env::var("RSTUI_PERF_THRESHOLD")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&v: &f64| v > 0.0)
        .unwrap_or(10.0);
    let filter: Vec<&String> = extra.iter().filter(|a| !a.starts_with('-')).collect();

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
            "--json",
        ])
        .args(&filter);
    eprintln!("xtask perf: running rstui-bench --json (release)…");
    let output = match cmd.output() {
        Ok(o) if o.status.success() => o.stdout,
        Ok(o) => {
            eprintln!(
                "xtask perf: rstui-bench failed:\n{}",
                String::from_utf8_lossy(&o.stderr)
            );
            return ExitCode::FAILURE;
        }
        Err(err) => {
            eprintln!("xtask perf: could not launch `{cargo}`: {err}");
            return ExitCode::FAILURE;
        }
    };
    let current = parse(&String::from_utf8_lossy(&output));
    if current.is_empty() {
        eprintln!("xtask perf: no scenarios measured (bad filter?)");
        return ExitCode::FAILURE;
    }

    let baseline_path = root.join("docs/perf-baseline.json");

    if save {
        if let Err(err) = std::fs::write(&baseline_path, to_json(&current)) {
            eprintln!(
                "xtask perf: could not write {}: {err}",
                baseline_path.display()
            );
            return ExitCode::FAILURE;
        }
        println!(
            "xtask perf: saved baseline ({} scenarios) → {}",
            current.len(),
            baseline_path.display()
        );
        return ExitCode::SUCCESS;
    }

    let baseline = match std::fs::read_to_string(&baseline_path) {
        Ok(s) => parse(&s),
        Err(_) => {
            eprintln!(
                "xtask perf: no baseline at {} — run `cargo xtask perf --save` first.",
                baseline_path.display()
            );
            return ExitCode::FAILURE;
        }
    };

    // Header columns as captures (not positional literal args) so
    // `clippy::print_literal` is satisfied while keeping the alignment.
    let (c_scn, c_base, c_cur, c_d, c_st) = ("scenario", "baseline", "current", "Δ%", "status");
    println!("{c_scn:<32}{c_base:>12}{c_cur:>12}{c_d:>9}  {c_st}");
    let mut regressed = false;
    for (name, min, _med, _mean) in &current {
        let base = baseline.iter().find(|(n, ..)| n == name);
        let (status, delta) = match base {
            None => ("new".to_owned(), 0.0),
            Some((_, bmin, ..)) => {
                let d = if *bmin == 0 {
                    0.0
                } else {
                    (*min as f64 - *bmin as f64) / *bmin as f64 * 100.0
                };
                let s = if d > threshold {
                    regressed = true;
                    "REGRESSED".to_owned()
                } else if d < -threshold {
                    "improved".to_owned()
                } else {
                    "ok".to_owned()
                };
                (s, d)
            }
        };
        let base_str = base.map_or_else(|| "—".to_owned(), |(_, b, ..)| human(*b));
        println!(
            "{name:<32}{base_str:>12}{:>12}{delta:>+8.1}%  {status}",
            human(*min)
        );
    }
    for (name, ..) in &baseline {
        if !current.iter().any(|(n, ..)| n == name) {
            // In the baseline but not measured this run (e.g. a filtered
            // run, or a scenario that was removed).
            let note = "(not measured this run)";
            println!("{name:<32}{note:>33}  GONE");
        }
    }
    println!(
        "\nbaseline: {}   threshold: ±{threshold:.0}%   (ADR 0005: a report, not a CI gate)",
        baseline_path.display()
    );

    if check && regressed {
        eprintln!("xtask perf: one or more scenarios regressed past the threshold.");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_round_trips_the_bench_json_shape() {
        let j = "{\"buffer/diff/identical\":{\"min_ns\":40123,\"median_ns\":41000,\"mean_ns\":40500},\
                 \"runtime/input/mouse_move\":{\"min_ns\":668917,\"median_ns\":674334,\"mean_ns\":671625}}";
        let rows = parse(j);
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0],
            ("buffer/diff/identical".to_owned(), 40123, 41000, 40500)
        );
        assert_eq!(rows[1].0, "runtime/input/mouse_move");
        assert_eq!(rows[1].1, 668_917);
        // to_json → parse is identity.
        assert_eq!(parse(&to_json(&rows)), rows);
    }

    #[test]
    fn parse_is_robust_to_trailing_newline_and_empty() {
        assert!(parse("").is_empty());
        assert!(parse("{}").is_empty());
        let rows = parse("{\"a\":{\"min_ns\":1,\"median_ns\":2,\"mean_ns\":3}}\n");
        assert_eq!(rows, vec![("a".to_owned(), 1, 2, 3)]);
    }

    #[test]
    fn human_scales_like_the_bench() {
        assert_eq!(human(900), "900ns");
        assert_eq!(human(12_300), "12.30µs");
        assert_eq!(human(1_050_000), "1.05ms");
    }
}
