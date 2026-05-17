//! Exercises [`ThemePicker`] the way an app uses it: a caller-owned
//! [`ThemePickerState`] on the model, a key loop that moves the highlight /
//! edits the filter, and — every frame — the app theming itself from
//! `state.selected_theme()`'s palette so moving the highlight *is* the live
//! preview. On `Enter` the app saves the name with [`Theme::write_choice`]
//! and reloads it next launch with [`Theme::read_choice`].
//!
//! Rendered over a [`TestBackend`] it is TTY-free and deterministic (a pure
//! projection, no clock), so it doubles as a snapshot smoke test.
//!
//! ```text
//! cargo run -p rstui-theme --example theme_picker_demo
//! ```

use rstui_core::{Color, Modifier, Style, Terminal, TestBackend, Widget};
use rstui_theme::{Theme, ThemePicker, ThemePickerState};

fn main() {
    // The picker state an app owns. Drive it from key events:
    //   ↑/↓ -> state.prev()/next()   (each frame: re-theme from preview)
    //   char -> state.push_filter(c) ; Backspace -> state.pop_filter()
    //   Enter -> Theme::write_choice(path, &state.selected_theme()?.name)
    let mut state = ThemePickerState::new();
    state.push_filter('t'); // pretend the user typed a filter
    state.next(); // …and moved the highlight

    let previewed = state
        .selected_theme()
        .map(|t| t.name.clone())
        .unwrap_or_default();

    let mut terminal = Terminal::new(TestBackend::new(48, 16)).expect("TestBackend is infallible");
    terminal
        .draw(|frame| {
            ThemePicker::new(&state)
                .title("Pick a theme")
                .style(Style::new().fg(Color::White))
                .highlight_style(
                    Style::new()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .render(frame.area(), frame.buffer_mut());
        })
        .expect("draw is infallible on TestBackend");

    let out = terminal.backend().to_string();
    println!("{out}");
    println!("(previewing: {previewed})");

    // Persistence round-trips (the "save" half), in a scratch path.
    let path = std::env::temp_dir().join("rstui-theme-picker-demo-choice");
    Theme::write_choice(&path, &previewed).expect("save the choice");
    let restored = Theme::read_choice(&path).expect("reload the saved choice");
    let _ = std::fs::remove_file(&path);

    // Doubles as a deterministic snapshot test.
    assert!(out.contains("Pick a theme"), "header renders");
    assert!(out.contains("Enter keep"), "key hint renders");
    assert!(out.contains('▸'), "the highlight renders");
    assert_eq!(
        restored.name, previewed,
        "write_choice/read_choice round-trip"
    );
}
