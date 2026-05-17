//! `cargo xtask record` — regenerate the documentation media with VHS.
//!
//! Every widget demo, the flagship `gallery`, and the `rstui-kitchen-sink`
//! showcase at four terminal resolutions are recorded to GIF/MP4 under
//! `docs/`, and a deterministic end-to-end smoke drives the *real* crossterm
//! binary and asserts the final frame. Deliberately **not** a `ci` gate (it
//! needs the VHS toolchain, which is not a CI dependency — same posture as
//! `bench`; see `docs/recording.md` and ADR 0005).
//!
//! Dependency-free: it only orchestrates `cargo build`, `vhs`, and the
//! filesystem with `std`. VHS is always run with the working directory at the
//! workspace root and `VHS_NO_SANDBOX=true` (the headless Chrome VHS drives
//! cannot sandbox in this environment), so every `Source`/`Output` path in a
//! tape is repo-relative and stable.
//!
//! Usage: `cargo xtask record [all|widgets|gallery|kitchen-sink|e2e] [--check]`
//! (no target means `all`). `e2e --check` fails if a marker is missing.

use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode};

/// Dispatch a `record` invocation. `args` is everything after `record`.
pub fn run(root: &Path, args: &[String]) -> ExitCode {
    let check = args.iter().any(|a| a == "--check");
    let target = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map_or("all", String::as_str);

    if !vhs_available() {
        eprintln!(
            "xtask record: `vhs` not found on PATH.\n\
             Install the recording toolchain:  brew install vhs ttyd ffmpeg\n\
             (ffmpeg is only needed for the .mp4 outputs.)  See docs/recording.md."
        );
        return ExitCode::FAILURE;
    }

    let ok = match target {
        "all" => {
            build_artifacts(root)
                && record_widgets(root)
                && record_gallery(root)
                && record_kitchen_sink(root)
                && record_e2e(root, check)
                && record_themes(root, args, check)
        }
        "widgets" => build_artifacts(root) && record_widgets(root),
        "gallery" => build_artifacts(root) && record_gallery(root),
        "kitchen-sink" => build_artifacts(root) && record_kitchen_sink(root),
        "e2e" => build_artifacts(root) && record_e2e(root, check),
        "themes" => build_artifacts(root) && record_themes(root, args, check),
        other => {
            eprintln!(
                "xtask record: unknown target `{other}`.\n\
                 Targets: all | widgets | gallery | kitchen-sink | e2e | themes   [--check]"
            );
            return ExitCode::from(2);
        }
    };

    if ok {
        println!("✓ xtask record: {target} complete.");
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// `true` if `vhs --version` runs (the toolchain is installed).
fn vhs_available() -> bool {
    Command::new("vhs")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Build everything the tapes launch: every `rstui-widgets` example, the
/// `rstui-kitchen-sink` binary, and the `rstui-crossterm` e2e example
/// fixtures. Pre-building keeps compile noise out of the recordings and the
/// tapes fast.
fn build_artifacts(root: &Path) -> bool {
    println!("xtask record: building examples + kitchen-sink…");
    cargo(root, &["build", "-p", "rstui-widgets", "--examples"])
        && cargo(root, &["build", "-p", "rstui-kitchen-sink"])
        && cargo(root, &["build", "-p", "rstui-crossterm", "--examples"])
}

/// Run `cargo <args…>` at the workspace root, streaming output.
fn cargo(root: &Path, args: &[&str]) -> bool {
    Command::new(env!("CARGO"))
        .args(args)
        .current_dir(root)
        .status()
        .is_ok_and(|s| s.success())
}

/// Run one tape with VHS at the workspace root (so repo-relative paths
/// resolve) with the no-sandbox flag the headless browser needs.
fn run_tape(root: &Path, tape: &Path) -> bool {
    Command::new("vhs")
        .arg(tape)
        .current_dir(root)
        .env("VHS_NO_SANDBOX", "true")
        .status()
        .is_ok_and(|s| s.success())
}

/// Generate one tape per `rstui-widgets` example (the `gallery` is recorded
/// separately) and render each to `docs/widgets/media/<name>.gif`.
fn record_widgets(root: &Path) -> bool {
    let examples_dir = root.join("crates/rstui-widgets/examples");
    let media = root.join("docs/widgets/media");
    let gen_dir = root.join("target/vhs/widgets");
    if fs::create_dir_all(&media).is_err() || fs::create_dir_all(&gen_dir).is_err() {
        eprintln!("xtask record: cannot create media/tape directories");
        return false;
    }

    let mut stems: Vec<String> = match fs::read_dir(&examples_dir) {
        Ok(rd) => rd
            .filter_map(Result::ok)
            .filter_map(|e| e.path().file_stem()?.to_str().map(str::to_owned))
            .filter(|s| s != "gallery")
            .collect(),
        Err(e) => {
            eprintln!("xtask record: cannot list {}: {e}", examples_dir.display());
            return false;
        }
    };
    stems.sort();

    let total = stems.len();
    let mut ok = true;
    for (i, stem) in stems.iter().enumerate() {
        println!("xtask record: widget [{}/{total}] {stem}", i + 1);
        let tape = gen_dir.join(format!("{stem}.tape"));
        let body = format!(
            "Output docs/widgets/media/{stem}.gif\n\
             Source vhs/common.tape\n\
             Set TypingSpeed 1ms\n\
             Hide\n\
             Type \"target/debug/examples/{stem}\"\n\
             Enter\n\
             Sleep 1200ms\n\
             Show\n\
             Sleep 2500ms\n"
        );
        if fs::write(&tape, body).is_err() || !run_tape(root, &tape) {
            eprintln!("xtask record: FAILED widget `{stem}`");
            ok = false;
        }
    }
    ok
}

/// Render the flagship `gallery` hero GIF.
fn record_gallery(root: &Path) -> bool {
    println!("xtask record: gallery…");
    run_tape(root, &root.join("vhs/gallery.tape"))
}

/// Render the kitchen sink at every committed resolution tape.
fn record_kitchen_sink(root: &Path) -> bool {
    if fs::create_dir_all(root.join("docs/media")).is_err() {
        eprintln!("xtask record: cannot create docs/media");
        return false;
    }
    let dir = root.join("vhs/kitchen-sink");
    let mut tapes: Vec<std::path::PathBuf> = match fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            // `_tour.tape` is a fragment Sourced by the resolution tapes.
            .filter(|p| {
                p.extension().is_some_and(|x| x == "tape")
                    && p.file_name().is_some_and(|n| n != "_tour.tape")
            })
            .collect(),
        Err(e) => {
            eprintln!("xtask record: cannot list {}: {e}", dir.display());
            return false;
        }
    };
    tapes.sort();

    let mut ok = true;
    for tape in &tapes {
        println!("xtask record: {}", tape.display());
        if !run_tape(root, tape) {
            eprintln!("xtask record: FAILED {}", tape.display());
            ok = false;
        }
    }
    ok
}

/// Run every `vhs/e2e/*.tape` against the real crossterm binary. With
/// `check`, assert every marker in the sibling `<name>.expect` file is
/// present in the captured terminal text (a regression gate); without it,
/// just report what was captured.
fn record_e2e(root: &Path, check: bool) -> bool {
    let dir = root.join("vhs/e2e");
    if fs::create_dir_all(root.join("target/vhs/e2e")).is_err() {
        eprintln!("xtask record: cannot create target/vhs/e2e");
        return false;
    }
    let mut tapes: Vec<std::path::PathBuf> = match fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "tape"))
            .collect(),
        Err(e) => {
            eprintln!("xtask record: cannot list {}: {e}", dir.display());
            return false;
        }
    };
    tapes.sort();

    let mut ok = true;
    for tape in &tapes {
        let name = tape.file_stem().and_then(|s| s.to_str()).unwrap_or("e2e");
        println!("xtask record: e2e {name}…");
        if !run_tape(root, tape) {
            eprintln!("xtask record: FAILED to run e2e tape {name}");
            ok = false;
            continue;
        }
        let capture_path = root.join(format!("target/vhs/e2e/{name}.txt"));
        let capture = fs::read_to_string(&capture_path).unwrap_or_default();
        if capture.trim().is_empty() {
            eprintln!("xtask record: e2e {name}: EMPTY capture (binary did not render)");
            ok = false;
            continue;
        }
        if !check {
            println!(
                "  captured {} bytes -> {}",
                capture.len(),
                capture_path.display()
            );
            continue;
        }
        let markers = read_markers(&dir.join(format!("{name}.expect")));
        let mut missing = Vec::new();
        for m in &markers {
            if !frame_contains(&capture, m) {
                missing.push(m.clone());
            }
        }
        if missing.is_empty() {
            println!("  ✓ e2e {name}: all {} markers present", markers.len());
        } else {
            eprintln!("  ✗ e2e {name}: MISSING markers: {missing:?}");
            ok = false;
        }
    }
    ok
}

/// Generate and run one VHS walkthrough per gpui-component theme.
///
/// For every theme the kitchen-sink reports (`--list-themes`, so xtask stays
/// std-only and the binary owns theme knowledge), launch the *real*
/// crossterm binary under `RSTUI_THEME=<name>`, visit all 21 screens (the 9
/// foundational by hotkey, the rest by command palette), open the settings
/// drawer (which renders the active theme name), and quit. With `check`,
/// assert every theme-invariant marker from `vhs/themes/walkthrough.expect`
/// *and* the theme's own name is present in the capture — proving the chosen
/// theme booted and the whole app was walked end to end, per theme.
///
/// `cargo xtask record themes [name-filter] [--check]` (filter = case-
/// insensitive substring, for fast local iteration on a subset).
fn record_themes(root: &Path, args: &[String], check: bool) -> bool {
    let gen_dir = root.join("target/vhs/themes");
    if fs::create_dir_all(&gen_dir).is_err() {
        eprintln!("xtask record: cannot create target/vhs/themes");
        return false;
    }

    let bin = root.join("target/debug/rstui-kitchen-sink");
    let names: Vec<String> = match Command::new(&bin)
        .arg("--list-themes")
        .current_dir(root)
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect(),
        _ => {
            eprintln!("xtask record: `rstui-kitchen-sink --list-themes` failed");
            return false;
        }
    };

    // Optional positional name filter (after the `themes` target word).
    let filter = args
        .iter()
        .filter(|a| !a.starts_with("--"))
        .nth(1)
        .map(|s| s.to_lowercase());
    let selected: Vec<String> = names
        .into_iter()
        .filter(|n| {
            filter
                .as_deref()
                .is_none_or(|f| n.to_lowercase().contains(f))
        })
        .collect();
    if selected.is_empty() {
        eprintln!("xtask record: no theme matched the filter");
        return false;
    }

    let invariants = read_markers(&root.join("vhs/themes/walkthrough.expect"));
    let total = selected.len();
    let mut ok = true;
    for (i, name) in selected.iter().enumerate() {
        let slug = slugify(name);
        println!("xtask record: theme [{}/{total}] {name}", i + 1);
        let tape = gen_dir.join(format!("{slug}.tape"));
        if fs::write(&tape, theme_tape(name, &slug)).is_err() || !run_tape(root, &tape) {
            eprintln!("xtask record: FAILED theme `{name}`");
            ok = false;
            continue;
        }
        let capture = fs::read_to_string(gen_dir.join(format!("{slug}.txt"))).unwrap_or_default();
        if capture.trim().is_empty() {
            eprintln!("  ✗ theme {name}: EMPTY capture (binary did not render)");
            ok = false;
            continue;
        }
        if !check {
            println!("  captured {} bytes", capture.len());
            continue;
        }
        // The theme's own name proves the env theme loaded *and* the drawer
        // rendered it (the launch echo is on a `target/debug/` line, which
        // `frame_contains` excludes — so a pass means the UI showed it).
        let mut markers = invariants.clone();
        markers.push(name.clone());
        let missing: Vec<&String> = markers
            .iter()
            .filter(|m| !frame_contains(&capture, m))
            .collect();
        if missing.is_empty() {
            println!("  ✓ theme {name}: all {} markers present", markers.len());
        } else {
            eprintln!("  ✗ theme {name}: MISSING markers: {missing:?}");
            ok = false;
        }
    }
    ok
}

/// A filesystem-safe slug for a theme name (`"macOS Classic Dark"` ->
/// `"macos-classic-dark"`): lowercase, non-alphanumerics collapsed to one
/// dash, trimmed.
fn slugify(name: &str) -> String {
    let mut s = String::new();
    let mut dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash {
            s.push('-');
            dash = true;
        }
    }
    s.trim_matches('-').to_owned()
}

/// Every kitchen-sink screen, in `Screen::ALL` order, paired with a command-
/// palette query whose first match (palette matches label OR title, lowest
/// `Screen::ALL` index wins) is exactly that screen. Palette-only navigation
/// is robust to screens being reordered or added (it is name-based, not
/// index-based) — which matters since the kitchen sink is actively growing.
const SCREEN_QUERIES: [&str; 21] = [
    "welcome",
    "forms",
    "navigation",
    "data display",
    "feedback",
    "containers",
    "rich text",
    "colour lab",
    "chat",
    "mail",
    "files",
    "dashboard",
    "music",
    "code editor",
    "settings",
    "sign in",
    "kanban",
    "live logs",
    "observability",
    "metrics",
    "traces",
];

/// The VHS tape that walks the whole kitchen-sink under one theme.
///
/// Navigation is command-palette only, with a `Tab` back to the sidebar pane
/// after every jump. This is deliberate: the kitchen sink routes *every*
/// character key to a focused text-entry screen (Chat/Ide/Login/Logs), so a
/// `:` or `g` typed while such a screen has focus would be swallowed as text.
/// `Tab` is a global chord (it toggles the pane even from a text screen), so
/// returning to the sidebar guarantees the next `:`/`g`/`q` is the global
/// chord, not text — the walkthrough stays correct for every screen.
///
/// Launch is `Hide`/`Show`-wrapped (boot noise off-capture); the command is a
/// single-quoted VHS string so the `RSTUI_THEME="<name>"` double quotes type
/// literally into bash.
fn theme_tape(name: &str, slug: &str) -> String {
    use std::fmt::Write as _;
    let mut t = String::new();
    let _ = writeln!(
        t,
        "# Auto-generated by `cargo xtask record themes` — DO NOT EDIT.\n\
         # Full kitchen-sink walkthrough under the {name:?} theme."
    );
    let _ = writeln!(t, "Output target/vhs/themes/{slug}.txt");
    t.push_str("Source vhs/common.tape\n");
    // Override AFTER the source (last `Set` wins): a wide terminal so the
    // centred header title is never truncated, and fast typing so 21 palette
    // queries do not dominate the runtime.
    t.push_str("Set FontSize 13\nSet Width 1280\nSet Height 800\nSet TypingSpeed 10ms\n");
    t.push_str("Hide\n");
    let _ = writeln!(
        t,
        "Type 'RSTUI_THEME=\"{name}\" target/debug/rstui-kitchen-sink'"
    );
    t.push_str("Enter\nSleep 2s\nShow\nSleep 700ms\n");
    t.push_str(
        "# Visit every screen by command palette; the app boots in the\n\
         # sidebar pane, and Tab returns there after each jump so ':' is\n\
         # always the global chord (never typed into a text screen).\n",
    );
    for q in SCREEN_QUERIES {
        let _ = writeln!(
            t,
            "Type \":\"\nSleep 200ms\nType \"{q}\"\nSleep 250ms\nEnter\nSleep 450ms\nTab\nSleep 120ms"
        );
    }
    t.push_str(
        "# Settings drawer (from the sidebar pane) renders the active theme\n\
         # name; close it, then quit via the confirmation modal.\n",
    );
    t.push_str(
        "Type \"g\"\nSleep 900ms\nEscape\nSleep 300ms\nType \"q\"\nSleep 300ms\nType \"y\"\nSleep 600ms\n",
    );
    t
}

/// Parse a `.expect` marker file: one required substring per line, blank
/// lines and `#` comments ignored.
fn read_markers(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// Whether `marker` appears in the capture. Checked against the last frame
/// first (VHS dumps every frame, separated by a full-width rule), then
/// against the whole capture minus the launch-command echo as a fallback —
/// robust against an in-TUI rule being mistaken for a frame delimiter.
fn frame_contains(capture: &str, marker: &str) -> bool {
    if last_frame(capture).contains(marker) {
        return true;
    }
    capture
        .lines()
        .filter(|l| !l.contains("target/debug/"))
        .any(|l| l.contains(marker))
}

/// The last complete frame in a VHS `.txt` capture. Frames are delimited by
/// a line that is entirely the box-drawing rule `─`; trailing blank rows are
/// trimmed. Falls back to the whole capture if no delimiter is found.
fn last_frame(capture: &str) -> String {
    let is_rule = |line: &str| {
        let t = line.trim();
        t.chars().count() >= 20 && t.chars().all(|c| c == '─')
    };
    let mut frames: Vec<String> = Vec::new();
    let mut cur = String::new();
    for line in capture.lines() {
        if is_rule(line) {
            if !cur.trim().is_empty() {
                frames.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        } else {
            cur.push_str(line);
            cur.push('\n');
        }
    }
    if !cur.trim().is_empty() {
        frames.push(cur);
    }
    frames.pop().unwrap_or_else(|| capture.to_owned())
}
