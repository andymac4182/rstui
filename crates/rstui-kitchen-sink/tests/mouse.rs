//! The mouse regression guard.
//!
//! Every test drives the real [`KitchenSink`] App through [`Harness`] and
//! synthesises actual [`Event::Mouse`] clicks/scrolls at precise cells. To
//! stay honest about "clicking what you see does the thing", targets are
//! located by **scanning the rendered snapshot** for their label and clicking
//! that cell — so if a screen's `view` layout and its `on_click` hit-test
//! ever drift apart (the whole class of bug this fixes, including clicks
//! after a resize), these fail `cargo test`, which is a CI gate.

use rstui_core::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind, Position, Size,
};
use rstui_kitchen_sink::KitchenSink;
use rstui_runtime::Harness;

fn harness() -> Harness<KitchenSink> {
    Harness::new(KitchenSink::new(Size::new(120, 40)), 120, 40)
}
fn ch(c: char) -> Event {
    Event::from(KeyEvent::char(c))
}
fn key(code: KeyCode) -> Event {
    Event::from(KeyEvent::from_code(code))
}
fn mouse(h: &mut Harness<KitchenSink>, kind: MouseEventKind, x: u16, y: u16) {
    h.handle(Event::Mouse(MouseEvent::new(
        kind,
        Position::new(x, y),
        KeyModifiers::NONE,
    )));
}
/// A real click is press **and** release at the same cell (no drag).
fn click_at(h: &mut Harness<KitchenSink>, x: u16, y: u16) {
    mouse(h, MouseEventKind::Down(MouseButton::Left), x, y);
    mouse(h, MouseEventKind::Up(MouseButton::Left), x, y);
}
/// A drag-selection: press, move across the row, release.
fn drag(h: &mut Harness<KitchenSink>, x1: u16, y1: u16, x2: u16, y2: u16) {
    mouse(h, MouseEventKind::Down(MouseButton::Left), x1, y1);
    let midx = (x1 + x2) / 2;
    mouse(h, MouseEventKind::Drag(MouseButton::Left), midx, y1);
    mouse(h, MouseEventKind::Drag(MouseButton::Left), x2, y2);
    mouse(h, MouseEventKind::Up(MouseButton::Left), x2, y2);
}
fn scroll(h: &mut Harness<KitchenSink>, up: bool, x: u16, y: u16) {
    let kind = if up {
        MouseEventKind::ScrollUp
    } else {
        MouseEventKind::ScrollDown
    };
    h.handle(Event::Mouse(MouseEvent::new(
        kind,
        Position::new(x, y),
        KeyModifiers::NONE,
    )));
}

/// The first cell (col,row) of `needle` in the current snapshot — the exact
/// terminal coordinate the user would aim at.
fn cell_of(h: &Harness<KitchenSink>, needle: &str) -> (u16, u16) {
    let want: Vec<char> = needle.chars().collect();
    for (y, line) in h.snapshot().lines().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        if chars.len() < want.len() {
            continue;
        }
        for start in 0..=chars.len() - want.len() {
            if chars[start..start + want.len()] == want[..] {
                return (start as u16, y as u16);
            }
        }
    }
    panic!("snapshot does not contain {needle:?}:\n{}", h.snapshot());
}
/// Click the centre of `needle`'s label.
fn click_label(h: &mut Harness<KitchenSink>, needle: &str) {
    let (x, y) = cell_of(h, needle);
    click_at(h, x + (needle.chars().count() as u16) / 2, y);
}

#[test]
fn clicking_a_rail_row_selects_that_screen() {
    let mut h = harness();
    // The rail lists every screen; click the "Kanban Board" row.
    click_label(&mut h, "Kanban Board");
    let s = h.snapshot();
    assert!(s.contains("Backlog"), "rail click opened Kanban:\n{s}");
}

#[test]
fn tab_strip_click_selects_the_clicked_tab_not_an_even_split() {
    let mut h = harness();
    h.handle(ch('3')); // Navigation (default sub-tab: List)
    assert!(h.snapshot().contains("fruit"), "navigation List shows");
    // Variable-width titles: an even-split hit-test would mis-select here.
    click_label(&mut h, "Table");
    assert!(
        h.snapshot().contains("people"),
        "clicking the Table tab shows the table:\n{}",
        h.snapshot()
    );
    click_label(&mut h, "Tree");
    assert!(
        h.snapshot().contains("files"),
        "clicking the Tree tab shows the tree:\n{}",
        h.snapshot()
    );
}

#[test]
fn markdown_link_click_follows_the_href() {
    let mut h = harness();
    h.handle(ch('7')); // Rich Text
    h.handle(key(KeyCode::Right)); // → Markdown sub-tab
    assert!(h.snapshot().contains("Markdown"), "on the Markdown tab");
    click_label(&mut h, "the rstui repo"); // the [text](href) label
    let s = h.snapshot();
    assert!(
        s.contains("Open link") && s.contains("github.com/andymac4182/rstui"),
        "clicking the markdown link toasts its href:\n{s}"
    );
}

#[test]
fn welcome_tour_markdown_link_click_works_too() {
    let mut h = harness();
    h.handle(ch('1')); // Welcome (its Markdown tour has a link)
    click_label(&mut h, "the rstui repo");
    assert!(
        h.snapshot().contains("Open link"),
        "the welcome tour link is clickable:\n{}",
        h.snapshot()
    );
}

#[test]
fn list_row_click_selects_the_row() {
    let mut h = harness();
    h.handle(ch('3')); // Navigation, List tab
    click_label(&mut h, "Cherry"); // a fruit row
    assert!(
        h.snapshot().contains("Picked Cherry"),
        "clicking a list row picks it:\n{}",
        h.snapshot()
    );
}

#[test]
fn dashboard_kpi_card_click_selects_it() {
    let mut h = harness();
    h.handle(ch(':'));
    for c in "dashboard".chars() {
        h.handle(ch(c));
    }
    h.handle(key(KeyCode::Enter));
    click_label(&mut h, "Active users");
    assert!(
        h.snapshot().contains("Selected Active users"),
        "clicking a KPI card selects it:\n{}",
        h.snapshot()
    );
}

#[test]
fn login_button_click_submits() {
    let mut h = harness();
    h.handle(ch(':'));
    for c in "sign in".chars() {
        h.handle(ch(c));
    }
    h.handle(key(KeyCode::Enter));
    // Click the Sign in button with both fields empty → validation error.
    click_label(&mut h, "Sign in");
    assert!(
        h.snapshot().contains("Sign-in failed"),
        "clicking the button submits (and validates):\n{}",
        h.snapshot()
    );
}

#[test]
fn clicks_are_still_correct_after_a_resize() {
    // This is the regression the geometry-capture fix exists for: before it,
    // hit-testing used a guessed size, so a click after a resize landed in
    // the wrong place. The target is found by snapshot scan, so the test
    // asserts the *rendered* geometry and the click math agree post-reflow.
    let mut h = harness();
    h.resize(94, 26);
    h.handle(ch('3')); // Navigation
    click_label(&mut h, "Menu"); // a tab, at the reflowed coordinates
    assert!(
        h.snapshot().contains("actions"),
        "tab click correct after resize:\n{}",
        h.snapshot()
    );
    h.resize(140, 44);
    click_label(&mut h, "Sign In"); // a rail row, at the new geometry
    assert!(
        h.snapshot().contains("Password"),
        "rail click correct after a second resize:\n{}",
        h.snapshot()
    );
}

#[test]
fn scroll_wheel_routes_to_the_content_under_the_pointer() {
    let mut h = harness();
    h.handle(ch('7')); // Rich Text, Paragraph tab (scrollable)
    let before = h.snapshot();
    // Scroll inside the content area.
    for _ in 0..6 {
        scroll(&mut h, false, 60, 20);
    }
    let after = h.snapshot();
    assert!(h.is_running());
    assert_ne!(before, after, "wheel scrolled the paragraph");
    for _ in 0..10 {
        scroll(&mut h, true, 60, 20); // back up; must not panic/underflow
    }
    assert!(h.is_running());
}

#[test]
fn clicking_every_screen_never_panics() {
    // Visit every screen via the palette and click around the content +
    // rail + chrome. on_click must be total everywhere (no panic, no hang).
    let screens = [
        "Welcome",
        "Forms",
        "Navigation",
        "Data Display",
        "Feedback",
        "Containers",
        "Rich Text",
        "Colour Lab",
        "Chat",
        "Mail",
        "Files",
        "Dashboard",
        "Music Player",
        "Code Editor",
        "Settings",
        "Sign In",
        "Kanban",
        "Live Logs",
    ];
    for name in screens {
        let mut h = harness();
        h.handle(ch(':'));
        for c in name.chars() {
            h.handle(ch(c));
        }
        h.handle(key(KeyCode::Enter));
        // A spray of clicks across the whole surface.
        for &(x, y) in &[
            (5u16, 4u16), // rail
            (60, 2),      // header
            (60, 20),     // content middle
            (118, 38),    // content bottom-right
            (23, 2),      // content top-left
            (60, 39),     // footer
        ] {
            click_at(&mut h, x, y);
        }
        scroll(&mut h, false, 60, 20);
        scroll(&mut h, true, 60, 20);
        assert!(h.is_running(), "{name}: clicks kept it running");
        assert!(!h.snapshot().is_empty(), "{name}: still renders");
    }
}

/// Drag across exactly the cells of `needle` on its row.
fn select_word(h: &mut Harness<KitchenSink>, needle: &str) {
    let (x, y) = cell_of(h, needle);
    drag(h, x, y, x + needle.chars().count() as u16 - 1, y);
}

#[test]
fn dragging_selects_and_copies_markdown_text() {
    let mut h = harness();
    h.handle(ch('7')); // Rich Text
    h.handle(key(KeyCode::Right)); // → Markdown sub-tab
    // "CommonMark" only appears in the rendered Markdown body.
    select_word(&mut h, "CommonMark");
    assert!(
        h.snapshot().contains("Copied"),
        "the copy is confirmed:\n{}",
        h.snapshot()
    );
    assert!(
        h.app().clipboard().contains("CommonMark"),
        "a read-only render auto-copies the covered text to the clipboard: {:?}",
        h.app().clipboard()
    );
}

#[test]
fn dragging_selects_paragraph_prose() {
    let mut h = harness();
    h.handle(ch('7')); // Rich Text, Paragraph tab (default)
    select_word(&mut h, "deterministically"); // a word only in the prose
    assert!(
        h.snapshot().contains("Copied"),
        "the copy is confirmed:\n{}",
        h.snapshot()
    );
    assert!(
        h.app().clipboard().contains("determ"),
        "the Paragraph prose is auto-copied: {:?}",
        h.app().clipboard()
    );
}

#[test]
fn a_plain_click_does_not_copy_anything() {
    let mut h = harness();
    h.handle(ch('7'));
    // Press + release on the same cell is a click, not a selection.
    click_at(&mut h, 60, 20);
    assert!(
        !h.snapshot().contains("Copied"),
        "a click must not leave/copy a selection:\n{}",
        h.snapshot()
    );
    assert!(h.is_running());
}

#[test]
fn selection_is_correct_after_a_resize() {
    // Same regression family as clicks: the highlight + extracted text must
    // follow the *rendered* geometry, not a guessed size.
    let mut h = harness();
    h.resize(96, 28);
    h.handle(ch('7'));
    h.handle(key(KeyCode::Right)); // Markdown tab
    select_word(&mut h, "CommonMark");
    assert!(
        h.snapshot().contains("Copied"),
        "drag-select still copies after a resize:\n{}",
        h.snapshot()
    );
}

#[test]
fn navigating_after_a_selection_clears_it_without_panicking() {
    let mut h = harness();
    h.handle(ch('7'));
    h.handle(key(KeyCode::Right));
    select_word(&mut h, "CommonMark");
    assert!(h.snapshot().contains("Copied"));
    // A navigation key drops the stale selection; the app stays healthy.
    h.handle(ch('1')); // jump to Welcome
    assert!(h.is_running());
    assert!(h.snapshot().contains("Welcome"));
}

/// Open a screen by fuzzy command-palette query.
fn goto(h: &mut Harness<KitchenSink>, query: &str) {
    h.handle(ch(':'));
    for c in query.chars() {
        h.handle(ch(c));
    }
    h.handle(key(KeyCode::Enter));
}

#[test]
fn data_diff_selection_stays_inside_the_diff_panel() {
    // The Data screen puts the Diff panel directly left of the Accordion on
    // the same rows. A drag that runs far right + down would, unclamped,
    // copy the Accordion too. It must not.
    let mut h = harness();
    goto(&mut h, "Data Display");
    let (x, y) = cell_of(&h, "render(area");
    drag(&mut h, x, y, x + 90, y + 8); // way past the diff, into the accordion
    let sel = h.app().clipboard();
    assert!(!sel.is_empty(), "something was selected");
    assert!(
        sel.contains("render(area") || sel.contains("pad"),
        "selection holds the diff text: {sel:?}"
    );
    for bad in ["Rendering", "Layout", "Events", "Concepts"] {
        assert!(
            !sel.contains(bad),
            "selection must NOT cross into the Accordion ({bad:?}): {sel:?}"
        );
    }
    assert!(h.snapshot().contains("Copied"));
}

#[test]
fn welcome_tour_selection_excludes_the_quickstart_card() {
    let mut h = harness();
    h.handle(ch('1')); // Welcome: Markdown tour (left) | Card (right)
    let (x, y) = cell_of(&h, "interactive"); // a tour-only word
    drag(&mut h, x, y, x + 80, y + 4); // would cross into the Card
    let sel = h.app().clipboard();
    assert!(
        sel.to_lowercase().contains("interactive"),
        "tour text selected: {sel:?}"
    );
    for bad in ["Quickstart", "Forms & Input", "Welcome (here)"] {
        assert!(
            !sel.contains(bad),
            "selection must NOT cross into the Card ({bad:?}): {sel:?}"
        );
    }
}

#[test]
fn markdown_code_block_is_selectable() {
    let mut h = harness();
    h.handle(ch('7'));
    h.handle(key(KeyCode::Right)); // Markdown sub-tab
    let (x, y) = cell_of(&h, "fn render"); // inside the ``` code block
    drag(&mut h, x, y, x + 40, y);
    assert!(
        h.app().clipboard().contains("fn render"),
        "the code block is selectable: {:?}",
        h.app().clipboard()
    );
}

#[test]
fn dragging_a_markdown_link_selects_its_label_and_does_not_follow_it() {
    let mut h = harness();
    h.handle(ch('7'));
    h.handle(key(KeyCode::Right));
    select_word(&mut h, "the rstui repo"); // a [label](href)
    let sel = h.app().clipboard();
    assert!(
        sel.contains("the rstui repo"),
        "the link label is selected by a drag: {sel:?}"
    );
    assert!(
        !h.snapshot().contains("Open link"),
        "a drag selects the link; it must not follow it"
    );
}

#[test]
fn clicking_a_markdown_link_still_follows_it() {
    // The click path (press + release, no drag) must still open the link.
    let mut h = harness();
    h.handle(ch('7'));
    h.handle(key(KeyCode::Right));
    click_label(&mut h, "the rstui repo");
    let s = h.snapshot();
    assert!(
        s.contains("Open link") && s.contains("github.com/andymac4182/rstui"),
        "a plain click on the link follows the href:\n{s}"
    );
}

#[test]
fn a_selection_never_includes_a_panel_border() {
    let mut h = harness();
    h.handle(ch('7')); // Rich Text, framed Paragraph body
    let (x, y) = cell_of(&h, "deterministically");
    drag(&mut h, x, y, x + 200, y + 60); // far past the panel on both axes
    let sel = h.app().clipboard();
    assert!(!sel.is_empty());
    for g in ['│', '─', '╭', '╮', '╰', '╯', '┌', '┐', '└', '┘'] {
        assert!(
            !sel.contains(g),
            "a selection must never include a border glyph {g:?}: {sel:?}"
        );
    }
}

/// A Ctrl-modified key (clipboard chords).
fn ctrl(c: char) -> Event {
    Event::from(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
}

#[test]
fn ctrl_c_quits_only_when_nothing_is_selected() {
    let mut h = harness();
    h.handle(ctrl('c'));
    assert!(!h.is_running(), "Ctrl+C with no selection still quits");
}

#[test]
fn editable_container_keeps_the_selection_until_ctrl_c_copies_it() {
    // Chat is editable → its drag-selection is NOT auto-copied; it stays
    // live with a hint, and Ctrl+C performs the copy.
    let mut h = harness();
    goto(&mut h, "chat"); // Chat (palette is index-stable; digit 9 is now Data Grid)
    let (x, y) = cell_of(&h, "Morning"); // a seeded thread message word
    drag(&mut h, x, y, x + 6, y);
    assert!(
        h.snapshot().contains("Selected") && !h.snapshot().contains("Copied"),
        "an editable container leaves it selected, not auto-copied:\n{}",
        h.snapshot()
    );
    assert!(h.app().clipboard().is_empty(), "nothing copied yet");
    h.handle(ctrl('c'));
    assert!(
        h.app().clipboard().contains("Morning"),
        "Ctrl+C copies the still-live selection: {:?}",
        h.app().clipboard()
    );
    assert!(h.snapshot().contains("Copied"));
    assert!(
        h.is_running(),
        "Ctrl+C with a selection copies, does not quit"
    );
}

#[test]
fn ctrl_x_cuts_selected_text_out_of_the_code_editor() {
    let mut h = harness();
    goto(&mut h, "Code Editor");
    assert!(
        h.snapshot().contains("KitchenSink"),
        "seed code is visible:\n{}",
        h.snapshot()
    );
    let (x, y) = cell_of(&h, "KitchenSink");
    drag(&mut h, x, y, x + "KitchenSink".len() as u16 - 1, y);
    h.handle(ctrl('x')); // cut
    assert!(
        h.app().clipboard().contains("KitchenSink"),
        "cut puts the text on the clipboard: {:?}",
        h.app().clipboard()
    );
    assert!(
        !h.snapshot().contains("KitchenSink"),
        "cut removed it from the buffer:\n{}",
        h.snapshot()
    );
    assert!(h.snapshot().contains("Cut"));
}

#[test]
fn ctrl_v_pastes_the_clipboard_into_a_focused_input() {
    let mut h = harness();
    // Auto-copy a word from a read-only render to load the clipboard.
    h.handle(ch('7'));
    h.handle(key(KeyCode::Right)); // Markdown tab
    select_word(&mut h, "CommonMark");
    assert!(h.app().clipboard().contains("CommonMark"));
    // Now paste it into the Logs filter input.
    goto(&mut h, "Live Logs");
    h.handle(ctrl('v'));
    assert!(
        h.snapshot().contains("CommonMark"),
        "Ctrl+V pasted the clipboard into the filter input:\n{}",
        h.snapshot()
    );
    assert!(h.is_running());
}

#[test]
fn kanban_card_drag_moves_it_to_the_target_column() {
    let mut h = harness();
    goto(&mut h, "Kanban Board");
    assert!(h.snapshot().contains("Backlog"), "Kanban opened");
    // Column headers locate the columns; `by` is the column-0 top row, so a
    // press at `by + 4` lands on its first card (cards start 3 rows down).
    let (bx, by) = cell_of(&h, "Backlog");
    let (dx, _) = cell_of(&h, "Done");
    let card_y = by + 4;
    // Press the first Backlog card, drag it across to the Done column.
    mouse(
        &mut h,
        MouseEventKind::Down(MouseButton::Left),
        bx + 1,
        card_y,
    );
    mouse(
        &mut h,
        MouseEventKind::Drag(MouseButton::Left),
        (bx + dx) / 2,
        card_y,
    );
    mouse(
        &mut h,
        MouseEventKind::Drag(MouseButton::Left),
        dx + 1,
        card_y,
    );
    mouse(
        &mut h,
        MouseEventKind::Up(MouseButton::Left),
        dx + 1,
        card_y,
    );
    assert!(
        h.snapshot().contains("Moved to Done"),
        "dragging a card to the Done column moved it there:\n{}",
        h.snapshot()
    );
    assert!(h.is_running());
}

#[test]
fn kanban_press_drag_release_never_panics() {
    let mut h = harness();
    goto(&mut h, "Kanban Board");
    let (bx, by) = cell_of(&h, "Backlog");
    // Pick a card, then flail the pointer (including off the board) and
    // release in the *same* column — a no-op move that must never panic.
    mouse(
        &mut h,
        MouseEventKind::Down(MouseButton::Left),
        bx + 1,
        by + 4,
    );
    for (x, y) in [(0u16, 0u16), (119, 39), (bx + 1, by + 7), (bx + 2, by + 4)] {
        mouse(&mut h, MouseEventKind::Drag(MouseButton::Left), x, y);
        assert!(h.is_running(), "a card drag must never quit the app");
    }
    mouse(
        &mut h,
        MouseEventKind::Up(MouseButton::Left),
        bx + 2,
        by + 4,
    );
    assert!(h.is_running());
    assert!(
        h.snapshot().contains("Backlog"),
        "the board still renders after a flailing drag"
    );
}
