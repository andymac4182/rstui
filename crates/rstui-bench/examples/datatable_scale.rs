//! `datatable_scale` — a capacity sweep answering "how big a `DataTable` can
//! rstui actually handle?".
//!
//! Not a registered `rstui-bench` scenario (those are fixed-size, 1 000-iter
//! regression guards): this is an **exploratory** one-shot that sweeps the
//! `rows × columns` matrix up to 1 000 000 × 100 and reports, per cell:
//!
//! - **model**  — resident memory (RSS) the materialized `Vec<DataRow>`
//!   actually costs. Measured via `ps` against a **clean child process**
//!   baseline (one fresh process per matrix cell), so the allocator can
//!   never carry slack from one cell — or a sizing probe — into the
//!   measurement (this workspace forbids `unsafe`, so a counting global
//!   allocator is not an option — `ADR 0003`; `ps` RSS is the std-only,
//!   unsafe-free, and arguably more honest alternative).
//! - **project (identity / sorted / filtered)** — the O(rows) per-state-change
//!   cost: the no-op pass, the single-key sort (the `line_text` alloc hot
//!   path), and a substring filter.
//! - **render** — one virtualized frame into a 160×48 buffer (only the visible
//!   window is touched, so this is expected to be flat in `rows`).
//!
//! Row counts ascend, so each measured cell yields a bytes-per-cell figure
//! the orchestrator uses to **predict** the next, larger cell at the same
//! column count. A cell whose prediction exceeds [`MODEL_CAP_KIB`] is **not
//! spawned** — its size is reported as a prediction and its timings as
//! `n/a`. That infeasible corner is itself the answer for "1 000 000 rows ×
//! 100 columns".
//!
//! Run it in release (the only meaningful mode for a capacity claim):
//!
//! ```text
//! cargo run --release -p rstui-bench --example datatable_scale
//! ```

use std::hint::black_box;
use std::process::Command;
use std::time::Instant;

use rstui_core::{Buffer, Position, Rect, Widget};
use rstui_widgets::{
    DataColumn, DataRow, DataTable, DataTableState, SortDirection, data_table::project,
};

/// Above this resident model size a cell is reported but not materialized.
/// 6 GiB keeps a 16 GB machine alive while still letting 1 000 000 × 20 and
/// 100 000 × 100 build for real.
const MODEL_CAP_KIB: u64 = 6 * 1024 * 1024;

/// The benchmark frame: a large-but-ordinary terminal.
const FRAME: Rect = Rect {
    x: 0,
    y: 0,
    width: 160,
    height: 48,
};

/// This process's resident set size, in KiB, via `ps` (std-only, no
/// `unsafe`). `0` if `ps` is unavailable so the sweep still runs.
fn rss_kib() -> u64 {
    let pid = std::process::id().to_string();
    Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

/// Build one realistic row of `cols` cells. Content is deterministic and
/// varied (so a sort actually has work to do) and ~10–16 bytes per cell —
/// the shape a real data grid holds, not a degenerate one-char fixture.
fn make_row(i: usize, cols: usize) -> DataRow<'static> {
    // A cheap reproducible shuffle so the sort key is not already ordered.
    let shuffled = (i.wrapping_mul(2_654_435_761)) & 0x00ff_ffff;
    DataRow::new((0..cols).map(|c| match c {
        0 => format!("R{shuffled:08}"),
        1 => format!("name-{}", i % 9973),
        2 => format!("{}", (i as u64).wrapping_mul(7)),
        _ => format!("c{c}v{}", i % 1000),
    }))
}

fn columns(cols: usize) -> Vec<DataColumn<'static>> {
    (0..cols)
        .map(|c| DataColumn::new(format!("col{c}")))
        .collect()
}

/// Lower-of-`reps` wall time for `op`, in seconds.
fn time<T>(reps: u32, mut op: impl FnMut() -> T) -> f64 {
    let mut best = f64::INFINITY;
    for _ in 0..reps {
        let t = Instant::now();
        let out = op();
        let dt = t.elapsed().as_secs_f64();
        black_box(&out);
        best = best.min(dt);
    }
    best
}

/// The per-cell child: in a fresh process, snapshot a clean RSS baseline,
/// build `rows × cols`, measure the RSS delta and the project/render
/// timings, print one result line, exit. The OS reclaims everything on exit
/// so the next cell starts from a clean heap; no in-process probe, so the
/// allocator never masks the build's growth.
fn run_cell(rows: usize, cols: usize) {
    let cols_v = columns(cols);
    let base = rss_kib();
    let data: Vec<DataRow> = (0..rows).map(|i| make_row(i, cols)).collect();
    let model_kib = rss_kib().saturating_sub(base);

    // Heavier reps for the small cells; a single pass for the millions-row
    // sort (one pass is already seconds and dominates the sweep's wall time).
    let preps = if rows >= 500_000 { 1 } else { 3 };

    let st_ident = DataTableState::new();
    let ident_s = time(preps, || project(&cols_v, &data, &st_ident));

    let mut st_sort = DataTableState::new();
    st_sort.set_sort(Some((0, SortDirection::Ascending)));
    let sort_s = time(preps, || project(&cols_v, &data, &st_sort));

    let mut st_filt = DataTableState::new();
    st_filt.set_filter("v042");
    let filt_s = time(preps, || project(&cols_v, &data, &st_filt));

    let visual = project(&cols_v, &data, &st_ident);
    let mut buf = Buffer::empty(FRAME);
    let render_s = time(50, || {
        DataTable::new(&cols_v, &data, &visual, &st_ident).render(FRAME, &mut buf);
        buf.get(Position::ORIGIN).map(|c| c.symbol)
    });

    // Keep the model alive across every measurement above.
    black_box(&data);

    // rows cols model_kib MEASURED ident_ns sort_ns filt_ns render_ns
    println!(
        "{rows} {cols} {model_kib} MEASURED {} {} {} {}",
        (ident_s * 1e9) as u64,
        (sort_s * 1e9) as u64,
        (filt_s * 1e9) as u64,
        (render_s * 1e9) as u64,
    );
}

fn fmt_mib(kib: u64) -> String {
    format!("{:.1}M", kib as f64 / 1024.0)
}

fn fmt_ms(ns: u64) -> String {
    format!("{:.2}ms", ns as f64 / 1e6)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 4 && args[1] == "--cell" {
        let rows: usize = args[2].parse().expect("rows");
        let cols: usize = args[3].parse().expect("cols");
        run_cell(rows, cols);
        return;
    }

    // ---- orchestrator: spawn one isolated child per matrix cell ----
    let exe = std::env::current_exe().expect("current_exe");
    println!(
        "DataTable capacity sweep — frame {}x{} ({} visible body rows)\n\
         model = resident RSS the Vec<DataRow> costs; each cell in its own process.\n",
        FRAME.width,
        FRAME.height,
        FRAME.height - 1,
    );
    println!(
        "{:>9} {:>4} {:>10} {:>9} {:>11} {:>11} {:>11} {:>10}",
        "rows", "cols", "model", "B/cell", "proj/ident", "proj/sort", "proj/filt", "render"
    );
    println!("{}", "-".repeat(82));

    let row_counts = [1_000usize, 10_000, 100_000, 1_000_000];
    let col_counts = [4usize, 20, 100];

    for &cols in &col_counts {
        // Bytes/cell measured at the largest built cell for this column
        // count — used to predict the next, larger row count and skip
        // spawning a child that would OOM the machine.
        let mut b_per_cell_seen: Option<f64> = None;
        for &rows in &row_counts {
            if let Some(bpc) = b_per_cell_seen {
                let predicted_kib = (bpc * rows as f64 * cols as f64 / 1024.0) as u64;
                if predicted_kib > MODEL_CAP_KIB {
                    println!(
                        "{rows:>9} {cols:>4} {:>10} {:>9.0} {:>11} {:>11} {:>11} {:>10}   (predicted; not spawned — exceeds {} GiB cap)",
                        fmt_mib(predicted_kib),
                        bpc,
                        "n/a",
                        "n/a",
                        "n/a",
                        "n/a",
                        MODEL_CAP_KIB / (1024 * 1024),
                    );
                    continue;
                }
            }

            let out = Command::new(&exe)
                .args(["--cell", &rows.to_string(), &cols.to_string()])
                .output()
                .expect("spawn cell child");
            let line = String::from_utf8_lossy(&out.stdout);
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() != 8 {
                println!(
                    "{rows:>9} {cols:>4}   child failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                );
                continue;
            }
            let model_kib: u64 = f[2].parse().unwrap_or(0);
            let ident: u64 = f[4].parse().unwrap_or(0);
            let sort: u64 = f[5].parse().unwrap_or(0);
            let filt: u64 = f[6].parse().unwrap_or(0);
            let render: u64 = f[7].parse().unwrap_or(0);
            let b_per_cell = model_kib as f64 * 1024.0 / (rows as f64 * cols as f64);
            // Trust the per-cell figure only once the model is big enough
            // that page-granular RSS rounding is negligible (≥ ~16 MiB).
            if model_kib >= 16 * 1024 {
                b_per_cell_seen = Some(b_per_cell);
            }
            println!(
                "{rows:>9} {cols:>4} {:>10} {:>9.0} {:>11} {:>11} {:>11} {:>8.1}µs",
                fmt_mib(model_kib),
                b_per_cell,
                fmt_ms(ident),
                fmt_ms(sort),
                fmt_ms(filt),
                render as f64 / 1e3,
            );
        }
        println!("{}", "-".repeat(82));
    }
}
