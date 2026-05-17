//! Translation from crossterm's native input into rstui's owned event model.
//!
//! [`from_crossterm`] is the only public entry point; the per-type converters
//! are deliberately private (rstui keeps its public surface minimal and grows
//! it only when a concrete consumer needs it).
//!
//! The translation is **total and pure**: every crossterm event either maps to
//! exactly one [`rstui_core::event::Event`] or, for input rstui intentionally
//! does not model, to [`None`]. Nothing here touches a terminal, so the whole
//! map is exercised by the unit tests below with hand-built events and no TTY —
//! the deterministic test story (ADR 0001, testing layer L4a) holds for the
//! one non-deterministic crate.
//!
//! Two deliberate, recorded divergences are encoded here, both matching the
//! `rstui_core::event` module's documented "defer, do not stub" stance:
//!
//! - Key codes rstui does not model — crossterm's `Null` and the Kitty-only
//!   `CapsLock`/`ScrollLock`/`NumLock`/`PrintScreen`/`Pause`/`Menu`/
//!   `KeypadBegin`/`Media`/`Modifier` — drop the **whole key event** to
//!   `None`. Callers `filter_map`, so an unmodeled key is simply skipped.
//! - Modifier bits rstui does not model — crossterm's Kitty-only `HYPER` and
//!   `META` — are dropped from the modifier set, but the key itself still
//!   maps: an unmodeled *modifier* must not discard a perfectly good key.
//!
//! `crossterm::event::KeyEvent::state` (caps/num-lock, keypad origin) is
//! dropped wholesale for the same reason — rstui models no lock state.
//!
//! The `match`es are intentionally **exhaustive with no wildcard arm**: a
//! crossterm upgrade that adds an input variant will fail to compile here,
//! forcing a deliberate review of how (or whether) rstui should model it,
//! rather than silently dropping new input.

use crossterm::event::{
    Event as CtEvent, KeyCode as CtKeyCode, KeyEvent as CtKeyEvent, KeyEventKind as CtKeyEventKind,
    KeyModifiers as CtKeyModifiers, MouseButton as CtMouseButton, MouseEvent as CtMouseEvent,
    MouseEventKind as CtMouseEventKind,
};
use rstui_core::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use rstui_core::geometry::{Position, Size};

/// Translates a crossterm input event into rstui's [`Event`].
///
/// Returns [`None`] when the event carries input rstui deliberately does not
/// model (a Kitty-only lock/media/modifier key code), so the backend's read
/// loop can `filter_map` it away. Every other event maps to exactly one
/// rstui [`Event`].
///
/// Takes the event by value because crossterm's bracketed-paste `Event` owns a
/// `String`; consuming it moves the pasted text through with no copy.
#[must_use]
pub fn from_crossterm(event: CtEvent) -> Option<Event> {
    Some(match event {
        CtEvent::FocusGained => Event::FocusGained,
        CtEvent::FocusLost => Event::FocusLost,
        // crossterm reports resize as (columns, rows); rstui `Size` is
        // (width, height) — the same order, columns == width.
        CtEvent::Resize(columns, rows) => Event::Resize(Size::new(columns, rows)),
        CtEvent::Paste(text) => Event::Paste(text),
        CtEvent::Key(key) => Event::Key(convert_key(key)?),
        CtEvent::Mouse(mouse) => Event::Mouse(convert_mouse(mouse)),
    })
}

/// `None` if the key code is one rstui does not model (drops the whole event).
fn convert_key(key: CtKeyEvent) -> Option<KeyEvent> {
    // `key.state` (lock/keypad flags) is intentionally not carried: rstui
    // models no lock state.
    Some(KeyEvent {
        code: convert_key_code(key.code)?,
        modifiers: convert_modifiers(key.modifiers),
        kind: convert_key_kind(key.kind),
    })
}

fn convert_key_code(code: CtKeyCode) -> Option<KeyCode> {
    Some(match code {
        CtKeyCode::Char(c) => KeyCode::Char(c),
        CtKeyCode::F(n) => KeyCode::F(n),
        CtKeyCode::Backspace => KeyCode::Backspace,
        CtKeyCode::Enter => KeyCode::Enter,
        CtKeyCode::Left => KeyCode::Left,
        CtKeyCode::Right => KeyCode::Right,
        CtKeyCode::Up => KeyCode::Up,
        CtKeyCode::Down => KeyCode::Down,
        CtKeyCode::Home => KeyCode::Home,
        CtKeyCode::End => KeyCode::End,
        CtKeyCode::PageUp => KeyCode::PageUp,
        CtKeyCode::PageDown => KeyCode::PageDown,
        CtKeyCode::Tab => KeyCode::Tab,
        CtKeyCode::BackTab => KeyCode::BackTab,
        CtKeyCode::Delete => KeyCode::Delete,
        CtKeyCode::Insert => KeyCode::Insert,
        CtKeyCode::Esc => KeyCode::Esc,
        // Deliberately unmodeled; the whole key event is dropped (see module
        // docs). Listed individually with no wildcard so a future crossterm
        // key code is a compile error, not a silent drop.
        CtKeyCode::Null
        | CtKeyCode::CapsLock
        | CtKeyCode::ScrollLock
        | CtKeyCode::NumLock
        | CtKeyCode::PrintScreen
        | CtKeyCode::Pause
        | CtKeyCode::Menu
        | CtKeyCode::KeypadBegin
        | CtKeyCode::Media(_)
        | CtKeyCode::Modifier(_) => return None,
    })
}

fn convert_modifiers(modifiers: CtKeyModifiers) -> KeyModifiers {
    let mut out = KeyModifiers::NONE;
    if modifiers.contains(CtKeyModifiers::SHIFT) {
        out |= KeyModifiers::SHIFT;
    }
    if modifiers.contains(CtKeyModifiers::CONTROL) {
        out |= KeyModifiers::CONTROL;
    }
    if modifiers.contains(CtKeyModifiers::ALT) {
        out |= KeyModifiers::ALT;
    }
    if modifiers.contains(CtKeyModifiers::SUPER) {
        out |= KeyModifiers::SUPER;
    }
    // HYPER / META are intentionally not mapped: rstui models only the four
    // common modifiers. An unmodeled modifier bit must not discard the key.
    out
}

fn convert_key_kind(kind: CtKeyEventKind) -> KeyEventKind {
    match kind {
        CtKeyEventKind::Press => KeyEventKind::Press,
        CtKeyEventKind::Repeat => KeyEventKind::Repeat,
        CtKeyEventKind::Release => KeyEventKind::Release,
    }
}

fn convert_mouse(mouse: CtMouseEvent) -> MouseEvent {
    MouseEvent::new(
        convert_mouse_kind(mouse.kind),
        Position::new(mouse.column, mouse.row),
        convert_modifiers(mouse.modifiers),
    )
}

fn convert_mouse_kind(kind: CtMouseEventKind) -> MouseEventKind {
    match kind {
        CtMouseEventKind::Down(button) => MouseEventKind::Down(convert_mouse_button(button)),
        CtMouseEventKind::Up(button) => MouseEventKind::Up(convert_mouse_button(button)),
        CtMouseEventKind::Drag(button) => MouseEventKind::Drag(convert_mouse_button(button)),
        CtMouseEventKind::Moved => MouseEventKind::Moved,
        CtMouseEventKind::ScrollDown => MouseEventKind::ScrollDown,
        CtMouseEventKind::ScrollUp => MouseEventKind::ScrollUp,
        CtMouseEventKind::ScrollLeft => MouseEventKind::ScrollLeft,
        CtMouseEventKind::ScrollRight => MouseEventKind::ScrollRight,
    }
}

fn convert_mouse_button(button: CtMouseButton) -> MouseButton {
    match button {
        CtMouseButton::Left => MouseButton::Left,
        CtMouseButton::Right => MouseButton::Right,
        CtMouseButton::Middle => MouseButton::Middle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_and_resize_and_paste_map_directly() {
        assert_eq!(
            from_crossterm(CtEvent::FocusGained),
            Some(Event::FocusGained)
        );
        assert_eq!(from_crossterm(CtEvent::FocusLost), Some(Event::FocusLost));

        // crossterm (columns, rows) -> rstui Size(width, height), same order.
        assert_eq!(
            from_crossterm(CtEvent::Resize(120, 40)),
            Some(Event::Resize(Size::new(120, 40)))
        );

        // The pasted String moves through without a copy.
        assert_eq!(
            from_crossterm(CtEvent::Paste("hello world".to_owned())),
            Some(Event::Paste("hello world".to_owned()))
        );
    }

    #[test]
    fn char_key_carries_modeled_modifiers_and_drops_unmodeled_ones() {
        let native = CtEvent::Key(CtKeyEvent::new(
            CtKeyCode::Char('s'),
            CtKeyModifiers::CONTROL | CtKeyModifiers::SHIFT,
        ));
        let key = from_crossterm(native).unwrap().as_key_press().unwrap();
        assert_eq!(key.code, KeyCode::Char('s'));
        assert!(key.modifiers.contains(KeyModifiers::CONTROL));
        assert!(key.modifiers.contains(KeyModifiers::SHIFT));
        assert!(!key.modifiers.contains(KeyModifiers::ALT));

        // HYPER/META are unmodeled, but the key still maps with no modifiers.
        let hyper = CtEvent::Key(CtKeyEvent::new(
            CtKeyCode::Char('x'),
            CtKeyModifiers::HYPER | CtKeyModifiers::META,
        ));
        let key = from_crossterm(hyper).unwrap().as_key_press().unwrap();
        assert_eq!(key.code, KeyCode::Char('x'));
        assert!(key.modifiers.is_empty());

        // ALT and SUPER map too.
        let alt_super = CtEvent::Key(CtKeyEvent::new(
            CtKeyCode::Char('a'),
            CtKeyModifiers::ALT | CtKeyModifiers::SUPER,
        ));
        let key = from_crossterm(alt_super).unwrap().as_key_press().unwrap();
        assert!(key.modifiers.contains(KeyModifiers::ALT));
        assert!(key.modifiers.contains(KeyModifiers::SUPER));
    }

    #[test]
    fn all_modeled_special_key_codes_map() {
        let cases = [
            (CtKeyCode::Backspace, KeyCode::Backspace),
            (CtKeyCode::Enter, KeyCode::Enter),
            (CtKeyCode::Left, KeyCode::Left),
            (CtKeyCode::Right, KeyCode::Right),
            (CtKeyCode::Up, KeyCode::Up),
            (CtKeyCode::Down, KeyCode::Down),
            (CtKeyCode::Home, KeyCode::Home),
            (CtKeyCode::End, KeyCode::End),
            (CtKeyCode::PageUp, KeyCode::PageUp),
            (CtKeyCode::PageDown, KeyCode::PageDown),
            (CtKeyCode::Tab, KeyCode::Tab),
            (CtKeyCode::BackTab, KeyCode::BackTab),
            (CtKeyCode::Delete, KeyCode::Delete),
            (CtKeyCode::Insert, KeyCode::Insert),
            (CtKeyCode::Esc, KeyCode::Esc),
            (CtKeyCode::F(5), KeyCode::F(5)),
            (CtKeyCode::Char('Z'), KeyCode::Char('Z')),
        ];
        for (native, expected) in cases {
            let event = CtEvent::Key(CtKeyEvent::new(native, CtKeyModifiers::NONE));
            assert_eq!(
                from_crossterm(event),
                Some(Event::Key(KeyEvent::from_code(expected))),
                "crossterm {native:?} should map to {expected:?}",
            );
        }
    }

    #[test]
    fn unmodeled_key_codes_drop_the_whole_event() {
        for native in [
            CtKeyCode::Null,
            CtKeyCode::CapsLock,
            CtKeyCode::ScrollLock,
            CtKeyCode::NumLock,
            CtKeyCode::PrintScreen,
            CtKeyCode::Pause,
            CtKeyCode::Menu,
            CtKeyCode::KeypadBegin,
            CtKeyCode::Media(crossterm::event::MediaKeyCode::Play),
            CtKeyCode::Modifier(crossterm::event::ModifierKeyCode::LeftShift),
        ] {
            let event = CtEvent::Key(CtKeyEvent::new(native, CtKeyModifiers::NONE));
            assert_eq!(
                from_crossterm(event),
                None,
                "crossterm {native:?} is unmodeled and must drop to None",
            );
        }
    }

    #[test]
    fn key_event_kind_maps_and_state_is_ignored() {
        use crossterm::event::KeyEventState;

        for (native_kind, expected_kind) in [
            (CtKeyEventKind::Press, KeyEventKind::Press),
            (CtKeyEventKind::Repeat, KeyEventKind::Repeat),
            (CtKeyEventKind::Release, KeyEventKind::Release),
        ] {
            // Set a lock state too: rstui models none, so it must not change
            // the mapping or block it.
            let native = CtEvent::Key(CtKeyEvent {
                code: CtKeyCode::Char('k'),
                modifiers: CtKeyModifiers::NONE,
                kind: native_kind,
                state: KeyEventState::CAPS_LOCK,
            });
            let key = from_crossterm(native).unwrap().as_key().unwrap();
            assert_eq!(key.code, KeyCode::Char('k'));
            assert_eq!(key.kind, expected_kind);
        }
    }

    #[test]
    fn mouse_position_uses_column_as_x_and_row_as_y() {
        let native = CtEvent::Mouse(CtMouseEvent {
            kind: CtMouseEventKind::Down(CtMouseButton::Left),
            column: 12,
            row: 7,
            modifiers: CtKeyModifiers::ALT,
        });
        let mouse = from_crossterm(native).unwrap().as_mouse().unwrap();
        assert_eq!(mouse.kind, MouseEventKind::Down(MouseButton::Left));
        assert_eq!(mouse.position, Position::new(12, 7));
        assert!(mouse.modifiers.contains(KeyModifiers::ALT));
    }

    #[test]
    fn every_mouse_kind_and_button_maps() {
        let kinds = [
            (
                CtMouseEventKind::Down(CtMouseButton::Left),
                MouseEventKind::Down(MouseButton::Left),
            ),
            (
                CtMouseEventKind::Up(CtMouseButton::Right),
                MouseEventKind::Up(MouseButton::Right),
            ),
            (
                CtMouseEventKind::Drag(CtMouseButton::Middle),
                MouseEventKind::Drag(MouseButton::Middle),
            ),
            (CtMouseEventKind::Moved, MouseEventKind::Moved),
            (CtMouseEventKind::ScrollDown, MouseEventKind::ScrollDown),
            (CtMouseEventKind::ScrollUp, MouseEventKind::ScrollUp),
            (CtMouseEventKind::ScrollLeft, MouseEventKind::ScrollLeft),
            (CtMouseEventKind::ScrollRight, MouseEventKind::ScrollRight),
        ];
        for (native_kind, expected_kind) in kinds {
            let native = CtEvent::Mouse(CtMouseEvent {
                kind: native_kind,
                column: 0,
                row: 0,
                modifiers: CtKeyModifiers::NONE,
            });
            assert_eq!(
                from_crossterm(native),
                Some(Event::Mouse(MouseEvent::new(
                    expected_kind,
                    Position::ORIGIN,
                    KeyModifiers::NONE,
                ))),
                "crossterm {native_kind:?} should map to {expected_kind:?}",
            );
        }
    }

    /// Every special key rstui models, paired with its crossterm source. This
    /// is the full set an input field navigates by — Home/End/arrows/Page/
    /// Delete — so the matrix below proves each survives the *real* terminal
    /// translation, not just the in-memory `TextEdit` model.
    fn special_key_pairs() -> [(CtKeyCode, KeyCode); 16] {
        [
            (CtKeyCode::Backspace, KeyCode::Backspace),
            (CtKeyCode::Enter, KeyCode::Enter),
            (CtKeyCode::Left, KeyCode::Left),
            (CtKeyCode::Right, KeyCode::Right),
            (CtKeyCode::Up, KeyCode::Up),
            (CtKeyCode::Down, KeyCode::Down),
            (CtKeyCode::Home, KeyCode::Home),
            (CtKeyCode::End, KeyCode::End),
            (CtKeyCode::PageUp, KeyCode::PageUp),
            (CtKeyCode::PageDown, KeyCode::PageDown),
            (CtKeyCode::Tab, KeyCode::Tab),
            (CtKeyCode::BackTab, KeyCode::BackTab),
            (CtKeyCode::Delete, KeyCode::Delete),
            (CtKeyCode::Insert, KeyCode::Insert),
            (CtKeyCode::Esc, KeyCode::Esc),
            (CtKeyCode::F(5), KeyCode::F(5)),
        ]
    }

    /// Every modifier rstui models, paired with its crossterm source, plus the
    /// real-world combinations a text field sees (Ctrl+Shift for word-select,
    /// Ctrl+Alt, Alt+Shift). HYPER/META are unmodeled and asserted dropped
    /// elsewhere; here every entry must round-trip exactly.
    fn modifier_pairs() -> [(CtKeyModifiers, KeyModifiers); 9] {
        [
            (CtKeyModifiers::NONE, KeyModifiers::NONE),
            (CtKeyModifiers::SHIFT, KeyModifiers::SHIFT),
            (CtKeyModifiers::CONTROL, KeyModifiers::CONTROL),
            (CtKeyModifiers::ALT, KeyModifiers::ALT),
            (CtKeyModifiers::SUPER, KeyModifiers::SUPER),
            (
                CtKeyModifiers::CONTROL | CtKeyModifiers::SHIFT,
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
            (
                CtKeyModifiers::CONTROL | CtKeyModifiers::ALT,
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            ),
            (
                CtKeyModifiers::ALT | CtKeyModifiers::SHIFT,
                KeyModifiers::ALT | KeyModifiers::SHIFT,
            ),
            (
                CtKeyModifiers::CONTROL | CtKeyModifiers::ALT | CtKeyModifiers::SHIFT,
                KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT,
            ),
        ]
    }

    #[test]
    fn every_special_key_maps_under_every_modeled_modifier() {
        // 16 navigation/edit keys × 9 modifier sets = 144 combinations. Each
        // must keep its code AND carry exactly the modeled modifier bits — the
        // end-to-end guarantee that "Home/End/arrows/Delete + Ctrl/Alt/Shift"
        // reach an input unchanged from a real terminal.
        for (ct_code, want_code) in special_key_pairs() {
            for (ct_mods, want_mods) in modifier_pairs() {
                let native = CtEvent::Key(CtKeyEvent::new(ct_code, ct_mods));
                let key = from_crossterm(native)
                    .unwrap_or_else(|| panic!("{ct_code:?}+{ct_mods:?} must map"))
                    .as_key()
                    .expect("a Key event");
                assert_eq!(key.code, want_code, "code for {ct_code:?}+{ct_mods:?}");
                assert_eq!(
                    key.modifiers, want_mods,
                    "modifiers for {ct_code:?}+{ct_mods:?}",
                );
            }
        }
    }

    #[test]
    fn char_keys_carry_every_modeled_modifier() {
        // Typed characters with each modifier — Ctrl+A (home), Ctrl+E (end),
        // Alt+Backspace (word delete), etc. are how editors bind shortcuts, so
        // the modifier must survive on Char too.
        for (ct_mods, want_mods) in modifier_pairs() {
            let native = CtEvent::Key(CtKeyEvent::new(CtKeyCode::Char('a'), ct_mods));
            let key = from_crossterm(native).unwrap().as_key().unwrap();
            assert_eq!(key.code, KeyCode::Char('a'));
            assert_eq!(key.modifiers, want_mods, "Char('a')+{ct_mods:?}");
        }
    }

    #[test]
    fn navigation_keys_round_trip_press_repeat_and_release_kinds() {
        // Holding an arrow key autorepeats; releasing it emits a Release. An
        // input must see the right kind on Home/End/arrows so key-repeat
        // cursor motion and press-only bindings both behave.
        use crossterm::event::KeyEventState;

        for code in [
            CtKeyCode::Home,
            CtKeyCode::End,
            CtKeyCode::Left,
            CtKeyCode::Right,
            CtKeyCode::Up,
            CtKeyCode::Down,
        ] {
            for (native_kind, want_kind) in [
                (CtKeyEventKind::Press, KeyEventKind::Press),
                (CtKeyEventKind::Repeat, KeyEventKind::Repeat),
                (CtKeyEventKind::Release, KeyEventKind::Release),
            ] {
                let native = CtEvent::Key(CtKeyEvent {
                    code,
                    modifiers: CtKeyModifiers::CONTROL,
                    kind: native_kind,
                    state: KeyEventState::NONE,
                });
                let key = from_crossterm(native).unwrap().as_key().unwrap();
                assert_eq!(key.kind, want_kind, "{code:?} {native_kind:?}");
                assert!(key.modifiers.contains(KeyModifiers::CONTROL));
                // `as_key_press` is the input path: Press/Repeat are presses,
                // Release is not (so a binding does not fire twice).
                let native = CtEvent::Key(CtKeyEvent {
                    code,
                    modifiers: CtKeyModifiers::NONE,
                    kind: native_kind,
                    state: KeyEventState::NONE,
                });
                let translated = from_crossterm(native).unwrap();
                assert_eq!(
                    translated.as_key_press().is_some(),
                    want_kind != KeyEventKind::Release,
                    "{code:?} {native_kind:?} press-ness",
                );
            }
        }
    }
}
