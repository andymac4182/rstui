//! Headless app-scale tests: drive the *exact* `GitReview` App the binary
//! runs through rstui's deterministic `Harness`. The crate lives inside this
//! git worktree, so `init`'s `git log` (run inline by the Harness) loads this
//! repo's real history — the tests assert structure/behaviour, never exact
//! commit text, so they stay deterministic as history grows.

use std::path::PathBuf;

use rstui_core::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind, Position,
};
use rstui_git_review::{Config, GitReview};
use rstui_runtime::Harness;

/// A left-button mouse event of `kind` at `(x, y)`.
fn mouse(kind: MouseEventKind, x: u16, y: u16) -> Event {
    Event::from(MouseEvent::new(
        kind,
        Position::new(x, y),
        KeyModifiers::NONE,
    ))
}

/// A config pointing at this crate (inside the repo worktree), so `git`
/// discovers the real repository regardless of the test's CWD.
fn repo_config() -> Config {
    Config {
        repo: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        rev: None,
    }
}

fn harness(config: Config) -> Harness<GitReview> {
    Harness::new(GitReview::new(config), 120, 40)
}
fn ch(c: char) -> Event {
    Event::from(KeyEvent::char(c))
}
fn key(code: KeyCode) -> Event {
    Event::from(KeyEvent::from_code(code))
}

#[test]
fn boots_and_renders_commit_history() {
    let h = harness(repo_config());
    assert!(h.is_running(), "boot must not quit");
    let s = h.snapshot();
    assert!(s.contains("Commits"), "the commit-list pane renders:\n{s}");
    // This repo has history, so a load succeeded and the body is not the
    // error panel.
    assert!(
        !s.contains("Cannot review this directory"),
        "a real repo must not show the error panel:\n{s}"
    );
    assert!(
        s.chars().any(|c| !c.is_whitespace()),
        "a non-blank frame renders"
    );
}

#[test]
fn commit_navigation_and_focus_never_panic() {
    let mut h = harness(repo_config());
    for ev in [
        ch(']'),
        ch(']'),
        ch('['),
        ch('j'),
        ch('k'),
        key(KeyCode::Tab),
        key(KeyCode::Down),
        key(KeyCode::PageDown),
        key(KeyCode::PageUp),
        ch('G'),
        ch('g'),
        key(KeyCode::Tab),
    ] {
        h.handle(ev);
        assert!(h.is_running(), "navigation must not quit the app");
    }
    assert!(
        h.snapshot().contains("Commits"),
        "still rendering after navigation"
    );
}

#[test]
fn help_overlay_opens_and_closes() {
    let mut h = harness(repo_config());
    h.handle(ch('?'));
    let s = h.snapshot();
    assert!(
        s.contains("Quit") && s.contains("commit"),
        "help cheat-sheet renders its bindings:\n{s}"
    );
    h.handle(key(KeyCode::Esc));
    assert!(h.is_running(), "closing help must not quit");
}

#[test]
fn edit_attempt_never_panics_on_real_history() {
    // Against arbitrary history the selected commit's first changed file may
    // be a binary (a `.gif`), a rename, or absent from the working tree —
    // the app must *gracefully* report that, never panic or quit. (The
    // happy-path edit→save is proven deterministically below.)
    let mut h = harness(repo_config());
    h.handle(ch('e'));
    assert!(
        h.is_running(),
        "`e` must not panic/quit, whatever the file is"
    );
    let opened = h.snapshot().contains("Ctrl-S save");
    if opened {
        // It was a text file: editing + Esc-back must round-trip cleanly.
        for ev in [ch('x'), key(KeyCode::Enter), key(KeyCode::Backspace)] {
            h.handle(ev);
            assert!(h.is_running(), "typing in the editor must not quit");
        }
        h.handle(key(KeyCode::Esc)); // Esc leaves Edit (does not quit).
        assert!(h.is_running(), "Esc returns to Review, not quit");
    } else {
        // It was binary/absent: the app stayed in Review and reported it.
        assert!(
            h.snapshot().chars().any(|c| !c.is_whitespace()),
            "a graceful status renders instead of the editor"
        );
    }
}

/// `git -c user.*=… <args>` in `dir`, asserting success — used only to build
/// a deterministic fixture repo for the round-trip test.
fn git_in(dir: &std::path::Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .args([
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git")
        .status
        .success();
    assert!(ok, "git {args:?} must succeed in the fixture");
}

#[test]
fn edit_round_trip_writes_the_working_tree_file() {
    // A throwaway repo with one commit adding one text file — deterministic,
    // independent of this repo's history.
    let dir = std::env::temp_dir().join(format!(
        "rgr-edit-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    git_in(&dir, &["init", "-q"]);
    std::fs::write(dir.join("note.txt"), "hello\n").expect("seed file");
    git_in(&dir, &["add", "note.txt"]);
    git_in(&dir, &["commit", "-q", "-m", "add note"]);

    let mut h = harness(Config {
        repo: dir.clone(),
        rev: None,
    });
    assert!(h.is_running());
    // `e` opens note.txt (the commit's only, text, working-tree file).
    h.handle(ch('e'));
    assert!(
        h.snapshot().contains("Ctrl-S save"),
        "the editor opened note.txt:\n{}",
        h.snapshot()
    );
    // Type a char, then save.
    h.handle(ch('Z'));
    h.handle(Event::from(KeyEvent::new(
        KeyCode::Char('s'),
        rstui_core::KeyModifiers::CONTROL,
    )));
    let on_disk = std::fs::read_to_string(dir.join("note.txt")).expect("read back");
    assert!(
        on_disk.contains('Z'),
        "Ctrl-S wrote the edit to the working tree: {on_disk:?}"
    );
    assert!(h.is_running(), "save must not quit");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn degrades_cleanly_outside_a_git_repo() {
    let dir = std::env::temp_dir().join(format!(
        "rstui-git-review-test-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut h = harness(Config {
        repo: dir.clone(),
        rev: None,
    });
    assert!(h.is_running(), "a non-repo must not crash the app");
    let s = h.snapshot();
    assert!(
        s.contains("Cannot review this directory"),
        "the error panel explains the failure:\n{s}"
    );
    // `q` still quits from the error state.
    h.handle(ch('q'));
    assert!(!h.is_running(), "q quits from the error panel");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tiny_terminal_is_a_safe_no_op() {
    let mut h = harness(repo_config());
    h.resize(4, 2);
    h.handle(ch(']'));
    assert!(h.is_running(), "a 4x2 terminal must not panic or quit");
    h.resize(120, 40);
    assert!(
        h.snapshot().contains("Commits"),
        "recovers when resized back"
    );
}

/// Build a throwaway repo with one commit per `subjects` entry (each adds a
/// distinct file), newest last. Returns the repo dir (caller cleans up).
fn fixture(subjects: &[&str]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rgr-fx-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    git_in(&dir, &["init", "-q"]);
    for (i, subj) in subjects.iter().enumerate() {
        std::fs::write(dir.join(format!("f{i}.txt")), format!("{subj}\n")).expect("seed");
        git_in(&dir, &["add", "."]);
        git_in(&dir, &["commit", "-q", "-m", subj]);
    }
    dir
}

#[test]
fn graph_tree_renders_and_filter_narrows() {
    let dir = fixture(&["alpha apple", "beta banana", "gamma grape"]);
    let mut h = harness(Config {
        repo: dir.clone(),
        rev: None,
    });
    let s = h.snapshot();
    assert!(s.contains("Commits 3"), "all 3 commits load:\n{s}");
    assert!(
        s.contains('*'),
        "the git --graph DAG art (`*`) is drawn:\n{s}"
    );

    // Filter to just the "beta" commit.
    h.handle(ch('/'));
    for c in "beta".chars() {
        h.handle(ch(c));
    }
    let s = h.snapshot();
    assert!(
        s.contains("Commits 1"),
        "the filter narrows to one commit:\n{s}"
    );
    // Esc clears the filter → all commits return.
    h.handle(key(KeyCode::Esc));
    assert!(
        h.snapshot().contains("Commits 3"),
        "Esc clears the filter back to all commits"
    );
    assert!(h.is_running());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn side_by_side_toggle_flips_the_diff_layout() {
    let dir = fixture(&["only commit"]);
    let mut h = harness(Config {
        repo: dir.clone(),
        rev: None,
    });
    assert!(
        h.snapshot().contains('≡'),
        "the detail title shows the unified marker by default"
    );
    h.handle(ch('s'));
    assert!(
        h.snapshot().contains('◫'),
        "`s` switches to the side-by-side marker"
    );
    h.handle(ch('s'));
    assert!(h.snapshot().contains('≡'), "`s` toggles back to unified");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn orientation_resize_and_graph_toggle_never_panic() {
    let dir = fixture(&["c1", "c2"]);
    let mut h = harness(Config {
        repo: dir.clone(),
        rev: None,
    });
    for ev in [
        ch('t'), // history → top
        ch('-'), // shrink split
        ch('-'),
        ch('='),  // grow split
        ch('t'),  // history → left
        ch('\\'), // graph off (reloads history)
        ch('\\'), // graph on
    ] {
        h.handle(ev);
        assert!(h.is_running(), "layout/graph toggles must not quit");
    }
    assert!(
        h.snapshot().contains("Commits 2"),
        "still renders the history after every toggle:\n{}",
        h.snapshot()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// Each fixture commit adds a distinct `f{i}.txt`, so the *patch* of commit i
// uniquely contains `f{i}.txt` — a robust discriminator for which commit the
// detail pane loaded (the subject text alone also appears in the list).

#[test]
fn mouse_click_in_history_selects_that_commit() {
    let dir = fixture(&["alpha apple", "beta banana", "gamma grape"]);
    let mut h = harness(Config {
        repo: dir.clone(),
        rev: None,
    });
    // First frame sets the geometry the mouse reducer hit-tests.
    let s = h.snapshot();
    assert!(
        s.contains("f2.txt") && !s.contains("f1.txt"),
        "newest commit (gamma → f2.txt) is selected on boot:\n{s}"
    );
    // Click the 2nd visible row (display row 1) inside the history pane.
    h.handle(mouse(MouseEventKind::Down(MouseButton::Left), 5, 2));
    let s = h.snapshot();
    assert!(
        s.contains("f1.txt"),
        "clicking row 2 selected the 2nd commit (beta → f1.txt):\n{s}"
    );
    assert!(h.is_running());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mouse_wheel_over_history_advances_selection() {
    let dir = fixture(&["alpha apple", "beta banana", "gamma grape"]);
    let mut h = harness(Config {
        repo: dir.clone(),
        rev: None,
    });
    assert!(h.snapshot().contains("f2.txt"), "boots on gamma");
    h.handle(mouse(MouseEventKind::ScrollDown, 5, 3));
    assert!(
        h.snapshot().contains("f1.txt"),
        "wheel-down over the history advances to the next commit (beta)"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mouse_divider_drag_resizes_without_panic() {
    let dir = fixture(&["c1", "c2"]);
    let mut h = harness(Config {
        repo: dir.clone(),
        rev: None,
    });
    let _ = h.snapshot(); // lay out → record geometry
    // Default Orient::Left, split 34 % of width 120 ⇒ divider near x = 40.
    h.handle(mouse(MouseEventKind::Down(MouseButton::Left), 40, 10));
    // The range is wide (6–94 %): at the extremes the history pane is a few
    // cells, so its full title is *correctly* truncated — the invariant is
    // "never panics, always renders something", not "the title fits".
    for x in [20u16, 118, 1, 60] {
        h.handle(mouse(MouseEventKind::Drag(MouseButton::Left), x, 10));
        assert!(h.is_running(), "a divider drag must never panic or quit");
        assert!(
            h.snapshot().chars().any(|c| !c.is_whitespace()),
            "still renders a non-blank frame mid-resize (x={x})"
        );
    }
    h.handle(mouse(MouseEventKind::Up(MouseButton::Left), 60, 10));
    assert!(h.is_running());
    // Back at ~50 %, the pane is wide enough for the full title again.
    assert!(
        h.snapshot().contains("Commits 2"),
        "the full title returns once the pane is a normal width again:\n{}",
        h.snapshot()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn keymap_panel_opens_navigates_rebinds_and_closes() {
    let mut h = harness(repo_config());
    // Ctrl+K opens the keymap settings panel (the new Action::Drawer).
    h.handle(Event::from(KeyEvent::new(
        KeyCode::Char('k'),
        rstui_core::KeyModifiers::CONTROL,
    )));
    let s = h.snapshot();
    assert!(
        s.contains("Keymap") && s.contains("git.split"),
        "the KeymapView panel renders the live bindings:\n{s}"
    );
    // Navigate, then capture-rebind the selected command; the panel owns
    // these keys (they do not leak to the app underneath).
    h.handle(ch('j'));
    h.handle(ch('r')); // arm capture
    assert!(
        h.snapshot().contains("press a key"),
        "the row is armed for capture:\n{}",
        h.snapshot()
    );
    h.handle(key(KeyCode::F(5))); // the new binding
    assert!(h.is_running(), "rebinding must not quit");
    // Esc closes the panel (does not quit the app).
    h.handle(key(KeyCode::Esc));
    assert!(h.is_running(), "closing the panel must not quit");
    assert!(
        h.snapshot().contains("Commits"),
        "back to the normal review screen:\n{}",
        h.snapshot()
    );
}

#[test]
fn commands_route_through_the_keymap_and_motions_stay_raw() {
    let dir = fixture(&["c1", "c2", "c3"]);
    let mut h = harness(Config {
        repo: dir.clone(),
        rev: None,
    });
    // A command (`s` → side-by-side) resolves through the keymap…
    h.handle(ch('s'));
    assert!(
        h.snapshot().contains('◫'),
        "the `s` command toggled side-by-side via the keymap:\n{}",
        h.snapshot()
    );
    // …and a raw motion (`]`/`[`) still steps commits with no keymap entry.
    h.handle(ch(']'));
    h.handle(ch('['));
    assert!(h.is_running(), "motions stay raw and never quit");
    assert!(h.snapshot().contains("Commits 3"));
    let _ = std::fs::remove_dir_all(&dir);
}
