//! Keyboard, mouse, focus, and resize input events.
//!
//! This is the vocabulary every interactive layer shares: the runtime delivers
//! it as messages, components match on it, and focus routing dispatches it.
//! Like the rest of `rstui-core` it is pure data — no terminal, no async — so
//! input handling is unit-testable without a TTY by constructing events by
//! hand.
//!
//! rstui defines its own event types rather than re-exporting a terminal
//! crate's (the way ratatui re-exports crossterm) because the architecture is
//! "dependency-free core; a real terminal backend in its own crate translates
//! *into* these". The shape deliberately mirrors the de-facto crossterm model
//! so the future backend bridge is a 1:1 mapping and existing Rust TUI
//! knowledge transfers — but coordinates use rstui's own [`Position`] and
//! [`Size`] so events compose directly with [`geometry`](crate::geometry) for
//! hit-testing and resize handling.
//!
//! Niche surface is deferred rather than stubbed: the Kitty-protocol-only
//! `HYPER`/`META` modifiers, lock-state flags, and media/keypad key codes are
//! intentionally omitted. The enums are exhaustive for the best `match`
//! ergonomics while the framework is pre-1.0; `#[non_exhaustive]` is a
//! documented hardening step for later.
//!
//! # Example
//!
//! ```
//! use rstui_core::event::{Event, KeyCode, KeyEvent, KeyModifiers};
//!
//! // A real backend builds these from terminal input; here, by hand.
//! let ctrl_c = Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
//!
//! // Apps match the way they will inside `update`:
//! if let Some(key) = ctrl_c.as_key_press() {
//!     assert_eq!(key.code, KeyCode::Char('c'));
//!     assert!(key.modifiers.contains(KeyModifiers::CONTROL));
//! }
//!
//! // The quit-key shortcut ignores modified and non-press keys:
//! assert!(Event::from(KeyEvent::char('q')).is_key(KeyCode::Char('q')));
//! assert!(!ctrl_c.is_key(KeyCode::Char('c'))); // Ctrl is held, not a bare 'c'
//! ```

use crate::geometry::{Position, Size};

/// Modifier keys held while another key or mouse event occurred.
///
/// A small hand-rolled bitset rather than a `bitflags` dependency, consistent
/// with [`Modifier`](crate::Modifier). The Kitty-protocol-only `HYPER`/`META`
/// modifiers are omitted as a scope choice; being a bitset it can gain them
/// later without breaking callers.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyModifiers(u8);

impl KeyModifiers {
    /// No modifier keys held.
    pub const NONE: Self = Self(0);
    /// The Shift key.
    pub const SHIFT: Self = Self(1 << 0);
    /// The Control key.
    pub const CONTROL: Self = Self(1 << 1);
    /// The Alt / Option key.
    pub const ALT: Self = Self(1 << 2);
    /// The Super / Command / Windows key.
    pub const SUPER: Self = Self(1 << 3);

    /// Returns `true` if every modifier in `other` is also held in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Returns `true` if `self` and `other` share at least one modifier.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Returns `true` if no modifier keys are held.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns `self` with the modifiers in `other` also set.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOr for KeyModifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl std::ops::BitOrAssign for KeyModifiers {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}

/// A logical key, independent of any modifiers held with it.
///
/// `Char` carries the produced character (already shifted, e.g. `Char('A')`
/// for Shift+A). Function keys are `F(1)..=F(12)`; backends clamp to what the
/// terminal emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    /// A character key, e.g. `'a'`, `'Z'`, `'5'`, `' '`.
    Char(char),
    /// A function key (`F(1)` is F1).
    F(u8),
    /// Backspace.
    Backspace,
    /// Enter / Return.
    Enter,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// Tab.
    Tab,
    /// Shift+Tab (back-tab).
    BackTab,
    /// Delete (forward delete).
    Delete,
    /// Insert.
    Insert,
    /// Escape.
    Esc,
}

/// Whether a key was pressed, auto-repeated, or released.
///
/// Most terminals only report [`Press`](KeyEventKind::Press); `Repeat` and
/// `Release` require the Kitty keyboard protocol, which a future backend may
/// negotiate. `Press` is the default so hand-built events read naturally.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyEventKind {
    /// The key was pressed.
    #[default]
    Press,
    /// The key was held and auto-repeated.
    Repeat,
    /// The key was released.
    Release,
}

/// A keyboard event: a [`KeyCode`] plus the modifiers held and the kind.
///
/// Derives `Hash`/`Eq` so it works directly as a keymap key
/// (`HashMap<KeyEvent, Action>`). Equality is exact — there is no Shift
/// normalization — which keeps matching predictable; refinement is deferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyEvent {
    /// The logical key.
    pub code: KeyCode,
    /// Modifier keys held with it.
    pub modifiers: KeyModifiers,
    /// Whether this is a press, repeat, or release.
    pub kind: KeyEventKind,
}

impl KeyEvent {
    /// A [`KeyEventKind::Press`] of `code` with `modifiers`.
    #[must_use]
    pub const fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self {
            code,
            modifiers,
            kind: KeyEventKind::Press,
        }
    }

    /// A press of `code` with no modifiers held.
    #[must_use]
    pub const fn from_code(code: KeyCode) -> Self {
        Self::new(code, KeyModifiers::NONE)
    }

    /// A press of the character `c` with no modifiers held.
    #[must_use]
    pub const fn char(c: char) -> Self {
        Self::from_code(KeyCode::Char(c))
    }

    /// `true` if this is a press or auto-repeat (i.e. not a release).
    ///
    /// Repeats count as presses so that holding a key keeps acting, which is
    /// what nearly every handler wants; release-aware code matches `kind`
    /// directly.
    #[must_use]
    pub const fn is_press(self) -> bool {
        matches!(self.kind, KeyEventKind::Press | KeyEventKind::Repeat)
    }
}

impl From<KeyCode> for KeyEvent {
    fn from(code: KeyCode) -> Self {
        Self::from_code(code)
    }
}

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// Left button.
    Left,
    /// Right button.
    Right,
    /// Middle button (often the wheel click).
    Middle,
}

/// What the mouse did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseEventKind {
    /// A button was pressed.
    Down(MouseButton),
    /// A button was released.
    Up(MouseButton),
    /// The mouse moved with a button held.
    Drag(MouseButton),
    /// The mouse moved with no button held.
    Moved,
    /// The wheel scrolled up.
    ScrollUp,
    /// The wheel scrolled down.
    ScrollDown,
    /// The wheel scrolled left.
    ScrollLeft,
    /// The wheel scrolled right.
    ScrollRight,
}

/// A mouse event: what happened, where, and which modifiers were held.
///
/// The location is an rstui [`Position`] rather than separate column/row
/// fields so it composes directly with
/// [`Rect::contains`](crate::Rect::contains) for hit-testing widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MouseEvent {
    /// What the mouse did.
    pub kind: MouseEventKind,
    /// Where, in terminal cell coordinates.
    pub position: Position,
    /// Modifier keys held.
    pub modifiers: KeyModifiers,
}

impl MouseEvent {
    /// Builds a mouse event.
    #[must_use]
    pub const fn new(kind: MouseEventKind, position: Position, modifiers: KeyModifiers) -> Self {
        Self {
            kind,
            position,
            modifiers,
        }
    }
}

/// Anything the input layer can deliver to the application.
///
/// A backend translates its native events into this; the runtime forwards it
/// to the app as a message and components match on it. `Paste` carries
/// bracketed-paste text, so `Event` is `Clone` but not `Copy`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Event {
    /// A keyboard event.
    Key(KeyEvent),
    /// A mouse event.
    Mouse(MouseEvent),
    /// The terminal was resized to this new size.
    Resize(Size),
    /// The terminal window gained focus.
    FocusGained,
    /// The terminal window lost focus.
    FocusLost,
    /// A bracketed paste delivered this text in a single chunk.
    Paste(String),
}

impl Event {
    /// The key event if this is a key **press or repeat**, otherwise `None`.
    ///
    /// The common case: most apps act on presses and ignore releases. Mirrors
    /// the accessor ratatui apps reach for first.
    #[must_use]
    pub fn as_key_press(&self) -> Option<KeyEvent> {
        match self {
            Self::Key(key) if key.is_press() => Some(*key),
            _ => None,
        }
    }

    /// The key event if this is one of any kind, otherwise `None`.
    #[must_use]
    pub fn as_key(&self) -> Option<KeyEvent> {
        match self {
            Self::Key(key) => Some(*key),
            _ => None,
        }
    }

    /// The mouse event if this is one, otherwise `None`.
    #[must_use]
    pub fn as_mouse(&self) -> Option<MouseEvent> {
        match self {
            Self::Mouse(mouse) => Some(*mouse),
            _ => None,
        }
    }

    /// `true` if this is a press/repeat of exactly `code` with no modifiers.
    ///
    /// The ergonomic shortcut for "quit on `q`" / "cancel on `Esc`" without
    /// spelling out a full [`KeyEvent`] match.
    #[must_use]
    pub fn is_key(&self, code: KeyCode) -> bool {
        self.as_key_press()
            .is_some_and(|key| key.code == code && key.modifiers.is_empty())
    }
}

impl From<KeyEvent> for Event {
    fn from(key: KeyEvent) -> Self {
        Self::Key(key)
    }
}

impl From<MouseEvent> for Event {
    fn from(mouse: MouseEvent) -> Self {
        Self::Mouse(mouse)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_modifiers_behave_like_a_bitset() {
        let cs = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        assert!(cs.contains(KeyModifiers::CONTROL));
        assert!(cs.contains(KeyModifiers::SHIFT));
        assert!(!cs.contains(KeyModifiers::ALT));
        assert!(cs.contains(cs));
        assert!(cs.intersects(KeyModifiers::SHIFT));
        assert!(!cs.intersects(KeyModifiers::ALT));
        assert!(!cs.is_empty());

        let mut acc = KeyModifiers::NONE;
        assert!(acc.is_empty());
        assert_eq!(KeyModifiers::default(), KeyModifiers::NONE);
        acc |= KeyModifiers::ALT;
        assert!(acc.contains(KeyModifiers::ALT));
        assert!(!acc.contains(KeyModifiers::CONTROL));
    }

    #[test]
    fn key_event_constructors_default_to_a_press() {
        assert_eq!(KeyEventKind::default(), KeyEventKind::Press);

        let a = KeyEvent::char('a');
        assert_eq!(a.code, KeyCode::Char('a'));
        assert!(a.modifiers.is_empty());
        assert_eq!(a.kind, KeyEventKind::Press);
        assert!(a.is_press());

        let from_code: KeyEvent = KeyCode::Enter.into();
        assert_eq!(from_code, KeyEvent::from_code(KeyCode::Enter));

        let ctrl_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert!(ctrl_d.modifiers.contains(KeyModifiers::CONTROL));
        assert!(ctrl_d.is_press());
    }

    #[test]
    fn is_press_treats_repeat_as_press_but_not_release() {
        let mut key = KeyEvent::char('x');
        assert!(key.is_press());
        key.kind = KeyEventKind::Repeat;
        assert!(key.is_press());
        key.kind = KeyEventKind::Release;
        assert!(!key.is_press());
    }

    #[test]
    fn mouse_event_carries_position_and_modifiers() {
        let ev = MouseEvent::new(
            MouseEventKind::Down(MouseButton::Left),
            Position::new(7, 3),
            KeyModifiers::ALT,
        );
        assert_eq!(ev.kind, MouseEventKind::Down(MouseButton::Left));
        assert_eq!(ev.position, Position::new(7, 3));
        assert!(ev.modifiers.contains(KeyModifiers::ALT));
    }

    #[test]
    fn as_key_press_filters_by_kind_and_variant() {
        let press = Event::from(KeyEvent::char('k'));
        assert_eq!(press.as_key_press(), Some(KeyEvent::char('k')));

        let mut released = KeyEvent::char('k');
        released.kind = KeyEventKind::Release;
        let released = Event::Key(released);
        assert_eq!(released.as_key_press(), None);
        // ...but as_key still surfaces the release.
        assert!(released.as_key().is_some());

        let resize = Event::Resize(Size::new(80, 24));
        assert_eq!(resize.as_key_press(), None);
        assert_eq!(resize.as_key(), None);
    }

    #[test]
    fn as_mouse_only_matches_mouse_events() {
        let click = Event::from(MouseEvent::new(
            MouseEventKind::Up(MouseButton::Right),
            Position::ORIGIN,
            KeyModifiers::NONE,
        ));
        assert!(click.as_mouse().is_some());
        assert!(Event::FocusGained.as_mouse().is_none());
    }

    #[test]
    fn is_key_is_a_bare_unmodified_press_shortcut() {
        assert!(Event::from(KeyEvent::char('q')).is_key(KeyCode::Char('q')));
        assert!(Event::from(KeyEvent::from_code(KeyCode::Esc)).is_key(KeyCode::Esc));

        // Wrong code, a held modifier, and a release all fail the shortcut.
        assert!(!Event::from(KeyEvent::char('q')).is_key(KeyCode::Char('w')));
        let ctrl_q = Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));
        assert!(!ctrl_q.is_key(KeyCode::Char('q')));
        let mut up = KeyEvent::from_code(KeyCode::Esc);
        up.kind = KeyEventKind::Release;
        assert!(!Event::Key(up).is_key(KeyCode::Esc));

        assert!(!Event::Paste("q".to_string()).is_key(KeyCode::Char('q')));
    }

    #[test]
    fn paste_event_is_clonable_and_compares_by_value() {
        let a = Event::Paste("hello".to_string());
        let b = a.clone();
        assert_eq!(a, b);
        assert_ne!(a, Event::Paste("world".to_string()));
    }

    #[test]
    fn key_event_works_as_a_keymap_key() {
        use std::collections::HashMap;

        let mut bindings = HashMap::new();
        bindings.insert(
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
            "save",
        );
        bindings.insert(KeyEvent::from_code(KeyCode::Esc), "cancel");

        assert_eq!(
            bindings
                .get(&KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL))
                .copied(),
            Some("save")
        );
        assert_eq!(bindings.get(&KeyEvent::char('s')), None);
    }
}
