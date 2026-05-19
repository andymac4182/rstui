//! Exercises [`EventEditor`] the way a calendar app would: a pure *layout*
//! projection for the create/edit-event dialog. The editor owns no state — it
//! lays out the heading, per-row labels and the button bar, and hands each
//! control's [`Rect`] back via `field_rect`; the *caller* renders its own
//! Input/Switch/DatePicker/TimePicker/Select/text-area into them (here we
//! stamp placeholder text to prove the rects line up).
//!
//! It does **no date math** and pulls in no `chrono`/`time` dependency.
//! Running over a [`TestBackend`] keeps it TTY-free, so it doubles as a
//! deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example event_editor_demo
//! ```

use rstui_core::{Buffer, Color, Constraint, Position, Rect, Style, Terminal, TestBackend, Widget};
use rstui_widgets::{Block, EventEditor, EventEditorField, Modal};

/// Stamps `text` into a control rect the way the caller's real widget would
/// (clipped at the right edge) — proves `field_rect` placed it correctly.
fn fill(buf: &mut Buffer, rect: Rect, text: &str) {
    if rect.is_empty() {
        return;
    }
    buf.set_str(
        Position::new(rect.left(), rect.top()),
        text,
        Style::new().fg(Color::Cyan),
    );
}

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(48, 20)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            let area = frame.area();
            // Pair with a Modal at the call site — the editor never centres
            // or clears itself.
            let modal = Modal::new()
                .width(Constraint::Percentage(95))
                .height(Constraint::Percentage(95))
                .block(Block::bordered().title("New event"));
            let inner = modal.inner(area);
            let editor = EventEditor::new()
                .title("New event")
                .help("⏎ save · esc cancel")
                .label_style(Style::new().fg(Color::Yellow));

            // The caller renders its own controls into the editor's rects.
            frame.render_widget(modal, area);
            frame.render_widget(editor.clone(), inner);
            let title_rect = editor.field_rect(EventEditorField::Title, inner);
            let loc_rect = editor.field_rect(EventEditorField::Location, inner);
            let buf = frame.buffer_mut();
            fill(buf, title_rect, "Sprint planning");
            fill(buf, loc_rect, "Room 4");
        })
        .expect("TestBackend is infallible");

    // Self-assert: render into a bare buffer, stamp two placeholder controls
    // into their `field_rect`s, and check the layout exactly.
    let area = Rect::new(0, 0, 44, 19);
    let mut buf = Buffer::empty(area);
    let editor = EventEditor::new()
        .title("New event")
        .help("⏎ save · esc cancel");
    editor.clone().render(area, &mut buf);

    // The editor draws its own heading, labels and button bar.
    assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'N'); // "New event"

    // The caller stamps its controls into the rects the editor handed back.
    let title_rect = editor.field_rect(EventEditorField::Title, area);
    let loc_rect = editor.field_rect(EventEditorField::Location, area);
    assert!(!title_rect.is_empty() && !loc_rect.is_empty());
    fill(&mut buf, title_rect, "Sprint planning");
    fill(&mut buf, loc_rect, "Room 4");

    // The placeholders landed exactly in the editor-assigned control rects,
    // right of the label column (proving render-then-fill agrees).
    let t: String = (title_rect.left()..title_rect.left() + 6)
        .map(|x| buf.get(Position::new(x, title_rect.top())).unwrap().symbol)
        .collect();
    assert_eq!(t, "Sprint", "title control text = {t:?}");
    assert_eq!(
        buf.get(Position::new(loc_rect.left(), loc_rect.top()))
            .unwrap()
            .symbol,
        'R' // "Room 4"
    );

    // The "Title"/"Location" labels are to the LEFT of those controls and the
    // caller's text never collides with them.
    assert_eq!(
        buf.get(Position::new(0, title_rect.top())).unwrap().symbol,
        'T'
    ); // "Title"
    assert!(0 < title_rect.left(), "control must sit right of the label");

    // The button bar and help line the editor drew itself.
    let dump: String = {
        let mut s = String::new();
        for y in 0..19 {
            for x in 0..44 {
                s.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            s.push('\n');
        }
        s
    };
    assert!(dump.contains("[Cancel]"), "missing Cancel button");
    assert!(dump.contains("[Save]"), "missing Save button");
    assert!(dump.contains("⏎ save · esc cancel"), "missing help line");

    // all_day collapses the time controls to nothing (totality).
    let all_day = EventEditor::new().all_day(true);
    assert_eq!(
        all_day.field_rect(EventEditorField::StartTime, area),
        Rect::ZERO
    );

    print!("{}", terminal.backend());
    println!(
        "event_editor_demo: OK — heading+labels+buttons drawn, caller controls \
         placed via field_rect, all_day hides time rows"
    );
}
