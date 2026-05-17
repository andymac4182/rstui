//! A real, customisable keymap engine — the synthesis of how Textual and
//! OpenCode/opentui do it, adapted to rstui's pure-reducer model.
//!
//! - **Semantic [`Action`]s, not physical keys.** The shell reacts to
//!   `Action::Palette`, never to "`:`". Every action has a stable string
//!   `id` (Textual's binding id) so it can be re-bound by config.
//! - **A [`Keymap`] is a named list of binds**, each an action mapped to one
//!   or more [`Trigger`]s. A trigger is a single [`Chord`] *or* a two-chord
//!   **sequence** (opencode's `<leader> x`): a leader/prefix chord then a
//!   key, with a timeout.
//! - **Multiple keymaps.** `Default`, `Vim`, and an opencode-style `Leader`
//!   map ship; the user cycles them at runtime and the whole UI (help,
//!   footer, settings) re-derives from the active one — fixing the Textual
//!   bug where the footer didn't follow a remap.
//! - **Per-OS layers.** Bindings are built with `cfg!(target_os = …)` so
//!   macOS gets `⌘`-native chords while Linux/Windows get `Ctrl`/`Super`,
//!   and [`Chord::display`] renders `⌘⌥⌃⇧` vs `Ctrl/Alt/Super/Shift`.
//! - **Customisation merged over defaults.** A user override (Textual's
//!   `set_keymap`, opencode's merged `keybinds`) replaces one action's keys
//!   by id; `"none"` disables it. Only what you change differs.

use rstui_core::{KeyCode, KeyEvent, KeyModifiers};

/// Every shell-level thing the keymap can trigger. Screen-level keys (arrows,
/// typing, `PageUp`, …) are deliberately *not* here — they fall through to
/// the focused screen raw, exactly as bindings cascade past in Textual.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Ask to quit (opens the confirm modal).
    Quit,
    /// Toggle the help overlay.
    Help,
    /// Open the command palette.
    Palette,
    /// Open the settings drawer.
    Drawer,
    /// Toggle focus between the rail and the screen.
    FocusToggle,
    /// Jump straight to screen `n` (1-based).
    Goto(u8),
    /// Copy the selection (or quit, when nothing is selected).
    Copy,
    /// Cut the selection out of the focused editable.
    Cut,
    /// Paste the clipboard into the focused editable.
    Paste,
    /// Switch to the next keymap (Default → Vim → Leader → …).
    CycleKeymap,
}

impl Action {
    /// Stable id used for config/override lookup (Textual's binding id).
    pub fn id(self) -> &'static str {
        match self {
            Action::Quit => "app.quit",
            Action::Help => "app.help",
            Action::Palette => "app.palette",
            Action::Drawer => "app.drawer",
            Action::FocusToggle => "app.focus_toggle",
            Action::Goto(_) => "app.goto",
            Action::Copy => "edit.copy",
            Action::Cut => "edit.cut",
            Action::Paste => "edit.paste",
            Action::CycleKeymap => "app.cycle_keymap",
        }
    }

    /// One-line help text (shown in the help overlay / settings table).
    pub fn help(self) -> &'static str {
        match self {
            Action::Quit => "Quit (confirm)",
            Action::Help => "Toggle this help",
            Action::Palette => "Command palette",
            Action::Drawer => "Settings drawer",
            Action::FocusToggle => "Move focus: rail / screen",
            Action::Goto(_) => "Jump to screen 1-9",
            Action::Copy => "Copy selection",
            Action::Cut => "Cut selection",
            Action::Paste => "Paste clipboard",
            Action::CycleKeymap => "Next keymap",
        }
    }

    /// The actions shown in help/settings, in display order (the nine
    /// per-screen `Goto` binds collapse to one row).
    pub fn shown() -> [Action; 9] {
        [
            Action::Palette,
            Action::Help,
            Action::Drawer,
            Action::FocusToggle,
            Action::Goto(1),
            Action::Copy,
            Action::Cut,
            Action::Paste,
            Action::CycleKeymap,
        ]
    }
}

/// One key chord: a [`KeyCode`] plus the modifier set, normalised so
/// `Ctrl+C` and `Ctrl+c` (and `Shift`-implied uppercase) compare equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    code: KeyCode,
    mods: KeyModifiers,
}

impl Chord {
    /// Normalises a code/mods pair: letters fold to lowercase and drop the
    /// `Shift` bit (terminals deliver `Shift+a` as `A`), so author intent
    /// and runtime events meet in the middle.
    fn norm(code: KeyCode, mods: KeyModifiers) -> Self {
        if let KeyCode::Char(c) = code {
            if c.is_ascii_alphabetic() {
                // Rebuild without SHIFT (terminals send Shift+a as `A`),
                // canonicalised so equal sets compare equal.
                let mut m = KeyModifiers::NONE;
                for flag in [
                    KeyModifiers::CONTROL,
                    KeyModifiers::ALT,
                    KeyModifiers::SUPER,
                ] {
                    if mods.contains(flag) {
                        m = m.union(flag);
                    }
                }
                return Self {
                    code: KeyCode::Char(c.to_ascii_lowercase()),
                    mods: m,
                };
            }
        }
        Self { code, mods }
    }

    /// Parses `"ctrl+shift+p"`, `"cmd+k"`, `"alt+left"`, `"esc"`, `"f2"`,
    /// `"space"`, `"q"`, `"?"`. Modifier names: `ctrl`/`control`,
    /// `alt`/`opt`/`option`, `shift`, `cmd`/`super`/`win`/`meta`.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let mut mods = KeyModifiers::NONE;
        let parts: Vec<&str> = s.split('+').collect();
        let (mod_parts, key) = parts.split_at(parts.len() - 1);
        for p in mod_parts {
            mods = match p.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "c" => mods.union(KeyModifiers::CONTROL),
                "alt" | "opt" | "option" | "meta" => mods.union(KeyModifiers::ALT),
                "shift" => mods.union(KeyModifiers::SHIFT),
                "cmd" | "command" | "super" | "win" => mods.union(KeyModifiers::SUPER),
                _ => return None,
            };
        }
        let k = key[0].trim();
        let code = match k.to_ascii_lowercase().as_str() {
            "esc" | "escape" => KeyCode::Esc,
            "enter" | "return" | "cr" => KeyCode::Enter,
            "tab" => KeyCode::Tab,
            "backtab" => KeyCode::BackTab,
            "space" => KeyCode::Char(' '),
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pageup" | "pgup" => KeyCode::PageUp,
            "pagedown" | "pgdn" => KeyCode::PageDown,
            "del" | "delete" => KeyCode::Delete,
            "ins" | "insert" => KeyCode::Insert,
            "bs" | "backspace" => KeyCode::Backspace,
            f if f.starts_with('f') && f[1..].parse::<u8>().is_ok() => {
                KeyCode::F(f[1..].parse().ok()?)
            }
            other => {
                let mut ch = other.chars();
                let c = ch.next()?;
                if ch.next().is_some() {
                    return None; // multi-char, unknown token
                }
                KeyCode::Char(c)
            }
        };
        Some(Self::norm(code, mods))
    }

    /// Whether `ev` triggers this chord (both normalised).
    pub fn matches(&self, ev: &KeyEvent) -> bool {
        *self == Self::norm(ev.code, ev.modifiers)
    }

    /// The chord a key event represents (normalised) — the capture half of
    /// interactive re-binding.
    pub fn from_event(ev: &KeyEvent) -> Self {
        Self::norm(ev.code, ev.modifiers)
    }

    /// A canonical, **`parse`-able** spec (`"ctrl+c"`, `"esc"`, `"f2"`,
    /// `"q"`) — what a captured chord is stored as in an override.
    pub fn spec(&self) -> String {
        let mut s = String::new();
        if self.mods.contains(KeyModifiers::CONTROL) {
            s.push_str("ctrl+");
        }
        if self.mods.contains(KeyModifiers::ALT) {
            s.push_str("alt+");
        }
        if self.mods.contains(KeyModifiers::SHIFT) {
            s.push_str("shift+");
        }
        if self.mods.contains(KeyModifiers::SUPER) {
            s.push_str("super+");
        }
        let k = match self.code {
            KeyCode::Char(' ') => "space".to_string(),
            KeyCode::Char(c) => c.to_ascii_lowercase().to_string(),
            KeyCode::F(n) => format!("f{n}"),
            KeyCode::Esc => "esc".to_string(),
            KeyCode::Enter => "enter".to_string(),
            KeyCode::Tab => "tab".to_string(),
            KeyCode::BackTab => "backtab".to_string(),
            KeyCode::Up => "up".to_string(),
            KeyCode::Down => "down".to_string(),
            KeyCode::Left => "left".to_string(),
            KeyCode::Right => "right".to_string(),
            KeyCode::Home => "home".to_string(),
            KeyCode::End => "end".to_string(),
            KeyCode::PageUp => "pageup".to_string(),
            KeyCode::PageDown => "pagedown".to_string(),
            KeyCode::Delete => "del".to_string(),
            KeyCode::Insert => "ins".to_string(),
            KeyCode::Backspace => "bs".to_string(),
        };
        s.push_str(&k);
        s
    }

    /// Human-readable, OS-aware (`⌘K` on macOS, `Super+K` elsewhere).
    pub fn display(&self) -> String {
        let mac = cfg!(target_os = "macos");
        let mut out = String::new();
        let m = self.mods;
        if m.contains(KeyModifiers::CONTROL) {
            out.push_str(if mac { "⌃" } else { "Ctrl+" });
        }
        if m.contains(KeyModifiers::ALT) {
            out.push_str(if mac { "⌥" } else { "Alt+" });
        }
        if m.contains(KeyModifiers::SHIFT) {
            out.push_str(if mac { "⇧" } else { "Shift+" });
        }
        if m.contains(KeyModifiers::SUPER) {
            out.push_str(if mac { "⌘" } else { "Super+" });
        }
        let key = match self.code {
            KeyCode::Char(' ') => "Space".to_string(),
            KeyCode::Char(c) => c.to_ascii_uppercase().to_string(),
            KeyCode::F(n) => format!("F{n}"),
            KeyCode::Esc => "Esc".to_string(),
            KeyCode::Enter => "⏎".to_string(),
            KeyCode::Tab => "Tab".to_string(),
            KeyCode::BackTab => "⇧Tab".to_string(),
            KeyCode::Up => "↑".to_string(),
            KeyCode::Down => "↓".to_string(),
            KeyCode::Left => "←".to_string(),
            KeyCode::Right => "→".to_string(),
            KeyCode::Home => "Home".to_string(),
            KeyCode::End => "End".to_string(),
            KeyCode::PageUp => "PgUp".to_string(),
            KeyCode::PageDown => "PgDn".to_string(),
            KeyCode::Delete => "Del".to_string(),
            KeyCode::Insert => "Ins".to_string(),
            KeyCode::Backspace => "⌫".to_string(),
        };
        out.push_str(&key);
        out
    }
}

/// What fires an action: a single chord, or a two-chord **sequence** (a
/// leader/prefix chord then a key, opencode's `<leader> x`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// One chord.
    Key(Chord),
    /// `first` then `second` within the keymap's timeout.
    Chain(Chord, Chord),
}

impl Trigger {
    /// Display form (`<leader>` is shown when `lead` is the chain prefix).
    fn display(&self, lead: Option<Chord>) -> String {
        match self {
            Trigger::Key(c) => c.display(),
            Trigger::Chain(a, b) => {
                let head = if Some(*a) == lead {
                    "⟨leader⟩".to_string()
                } else {
                    a.display()
                };
                format!("{head} {}", b.display())
            }
        }
    }

    /// Parse one trigger token: `"ctrl+c"`, or a chain `"g ?"` /
    /// `"<leader> n"` (space-separated). `lead` substitutes `<leader>`.
    fn parse(tok: &str, lead: Option<Chord>) -> Option<Self> {
        let tok = tok.trim();
        if let Some((a, b)) = tok.split_once(' ') {
            let first = if a.trim().eq_ignore_ascii_case("<leader>") {
                lead?
            } else {
                Chord::parse(a)?
            };
            Some(Trigger::Chain(first, Chord::parse(b)?))
        } else {
            Some(Trigger::Key(Chord::parse(tok)?))
        }
    }
}

/// One action bound to its triggers.
#[derive(Debug, Clone)]
struct Bind {
    action: Action,
    triggers: Vec<Trigger>,
}

/// A named, self-contained keymap.
#[derive(Debug, Clone)]
pub struct Keymap {
    /// Display name (`Default` / `Vim` / `Leader`).
    pub name: &'static str,
    /// The leader chord for `⟨leader⟩`-prefixed chains, if any.
    pub leader: Option<Chord>,
    /// How long (ms) a pressed leader waits for the second key.
    pub leader_timeout_ms: u64,
    binds: Vec<Bind>,
}

impl Keymap {
    /// A `Key` trigger equal to this keymap's leader is invalid — the
    /// prefix is reserved, it can never also fire a plain action.
    fn keeps(&self, t: &Trigger) -> bool {
        !matches!((t, self.leader), (Trigger::Key(c), Some(l)) if *c == l)
    }

    fn bind(&mut self, action: Action, specs: &[&str]) {
        let mut triggers: Vec<Trigger> = Vec::new();
        for s in specs {
            if let Some(t) = Trigger::parse(s, self.leader) {
                // De-dupe (a per-OS chord that equals the portable one,
                // e.g. `ctrl+c` on Linux) and never shadow the leader.
                if self.keeps(&t) && !triggers.contains(&t) {
                    triggers.push(t);
                }
            }
        }
        self.binds.push(Bind { action, triggers });
    }

    /// Bind the nine screen-jump digits, each to its own `Goto(n)` so
    /// `resolve` returns the right screen (one help row, see `keys_for`).
    fn bind_goto(&mut self) {
        for n in 1u8..=9 {
            let d = char::from(b'0' + n).to_string();
            if let Some(t) = Trigger::parse(&d, self.leader) {
                self.binds.push(Bind {
                    action: Action::Goto(n),
                    triggers: vec![t],
                });
            }
        }
    }

    /// Apply a user override (Textual `set_keymap` / opencode merged
    /// `keybinds`): replace this action's triggers, or disable it with
    /// `"none"`/`""`. Comma-separates alternatives.
    pub fn override_action(&mut self, action: Action, keys: &str) {
        let triggers: Vec<Trigger> =
            if keys.trim().eq_ignore_ascii_case("none") || keys.trim().is_empty() {
                Vec::new()
            } else {
                keys.split(',')
                    .filter_map(|t| Trigger::parse(t, self.leader))
                    .filter(|t| self.keeps(t))
                    .collect()
            };
        if let Some(b) = self.binds.iter_mut().find(|b| b.action == action) {
            b.triggers = triggers;
        } else {
            self.binds.push(Bind { action, triggers });
        }
    }

    /// The display string of every key bound to `action` (joined by `/`),
    /// or `—` when it is unbound/disabled. Powers help + footer + settings,
    /// so they always show the *live* binding.
    pub fn keys_for(&self, action: Action) -> String {
        // The nine `Goto(n)` binds collapse to one help row.
        if matches!(action, Action::Goto(_)) {
            let mut digits: Vec<String> = self
                .binds
                .iter()
                .filter(|b| matches!(b.action, Action::Goto(_)))
                .flat_map(|b| b.triggers.iter().map(|t| t.display(self.leader)))
                .collect();
            if digits.is_empty() {
                return "—".to_string();
            }
            if digits == ["1", "2", "3", "4", "5", "6", "7", "8", "9"] {
                return "1–9".to_string();
            }
            digits.dedup();
            return digits.join(" / ");
        }
        for b in &self.binds {
            if b.action == action {
                if b.triggers.is_empty() {
                    return "—".to_string();
                }
                return b
                    .triggers
                    .iter()
                    .map(|t| t.display(self.leader))
                    .collect::<Vec<_>>()
                    .join(" / ");
            }
        }
        "—".to_string()
    }
}

/// What [`Keymaps::resolve`] decided for one key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resolved {
    /// Fire this action.
    Act(Action),
    /// This was a leader/prefix; wait for the next key.
    Pending,
    /// Nothing matched — let the key fall through to the screen.
    Fall,
}

/// The registry: every keymap, the active one, the live user overrides, and
/// the pending-sequence state. Owned by the app model; the reducer drives it.
#[derive(Debug, Clone)]
pub struct Keymaps {
    sets: Vec<Keymap>,
    active: usize,
    /// `(action, keys)` user customisations, applied over the active map.
    overrides: Vec<(Action, String)>,
    /// The first chord of an in-flight sequence + the tick it must beat.
    pending: Option<(Chord, u64)>,
}

impl Default for Keymaps {
    /// Equivalent to [`Keymaps::new`]: the three shipped keymaps with no
    /// user overrides and no leader armed.
    fn default() -> Self {
        Self::new()
    }
}

impl Keymaps {
    /// The three shipped keymaps, built with per-OS chords.
    pub fn new() -> Self {
        Self {
            sets: vec![default_map(), vim_map(), leader_map()],
            active: 0,
            overrides: Vec::new(),
            pending: None,
        }
    }

    /// The detected OS, for display.
    pub fn os_name() -> &'static str {
        if cfg!(target_os = "macos") {
            "macOS"
        } else if cfg!(target_os = "windows") {
            "Windows"
        } else {
            "Linux"
        }
    }

    /// The active keymap with the user overrides applied (what the UI shows
    /// and what `resolve` matches against).
    pub fn effective(&self) -> Keymap {
        let mut km = self.sets[self.active].clone();
        for (action, keys) in &self.overrides {
            km.override_action(*action, keys);
        }
        km
    }

    /// Switch to the next keymap (clears any half-typed sequence).
    pub fn cycle(&mut self) -> &'static str {
        self.active = (self.active + 1) % self.sets.len();
        self.pending = None;
        self.sets[self.active].name
    }

    /// Customise (or disable with `"none"`) one action's keys, live.
    pub fn set_override(&mut self, action: Action, keys: impl Into<String>) {
        let keys = keys.into();
        self.overrides.retain(|(a, _)| *a != action);
        self.overrides.push((action, keys));
    }

    /// Active keymap name and whether a sequence leader is currently armed
    /// (for the status hint).
    pub fn status(&self) -> (&'static str, bool) {
        (self.sets[self.active].name, self.pending.is_some())
    }

    /// Whether a leader/prefix has been pressed and we are waiting for the
    /// rest of the sequence (so the shell swallows the key).
    pub fn armed(&self) -> bool {
        self.pending.is_some()
    }

    /// The active keymap's name.
    pub fn active_name(&self) -> &'static str {
        self.sets[self.active].name
    }

    /// Resolve a key event to an [`Action`], threading the leader-sequence
    /// state machine and its timeout (deterministic on the `tick` clock so
    /// the headless harness can drive it).
    pub fn resolve(&mut self, ev: &KeyEvent, tick: u64) -> Option<Action> {
        let km = self.effective();
        let timeout_ticks = (km.leader_timeout_ms / 120).max(1);

        // An armed, un-expired leader: this key completes (or breaks) it.
        if let Some((first, deadline)) = self.pending {
            self.pending = None;
            if tick <= deadline {
                for b in &km.binds {
                    for t in &b.triggers {
                        if let Trigger::Chain(a, c) = t {
                            if *a == first && c.matches(ev) {
                                return Some(b.action);
                            }
                        }
                    }
                }
            }
            // Fell through: not a valid continuation — treat as a fresh key.
        }

        match Self::match_event(&km, ev) {
            Resolved::Act(a) => Some(a),
            Resolved::Pending => {
                let c = Chord::norm(ev.code, ev.modifiers);
                self.pending = Some((c, tick.saturating_add(timeout_ticks)));
                None
            }
            Resolved::Fall => None,
        }
    }

    /// Drop a leader that has sat un-completed past its timeout (called from
    /// the animation tick so a stale prefix never eats the next key).
    pub fn expire(&mut self, tick: u64) {
        if let Some((_, deadline)) = self.pending {
            if tick > deadline {
                self.pending = None;
            }
        }
    }

    /// One-shot match against the *armed* state: a complete `Key` wins; a
    /// chord that only ever starts a `Chain` arms the leader.
    fn match_event(km: &Keymap, ev: &KeyEvent) -> Resolved {
        for b in &km.binds {
            for t in &b.triggers {
                if let Trigger::Key(c) = t {
                    if c.matches(ev) {
                        return Resolved::Act(b.action);
                    }
                }
            }
        }
        for b in &km.binds {
            for t in &b.triggers {
                if let Trigger::Chain(a, _) = t {
                    if a.matches(ev) {
                        return Resolved::Pending;
                    }
                }
            }
        }
        Resolved::Fall
    }
}

// The per-OS layer, resolved at compile time (`cfg!` is a `bool` const, so
// these are plain `&'static str` literals — zero cost, no allocation).
const PALETTE_OS: &str = if cfg!(target_os = "macos") {
    "cmd+k"
} else {
    "ctrl+k"
};
const COPY_OS: &str = if cfg!(target_os = "macos") {
    "cmd+c"
} else {
    "ctrl+c"
};
const CUT_OS: &str = if cfg!(target_os = "macos") {
    "cmd+x"
} else {
    "ctrl+x"
};
const PASTE_OS: &str = if cfg!(target_os = "macos") {
    "cmd+v"
} else {
    "ctrl+v"
};

/// The default keymap: today's bindings, plus an OS-native alternate for
/// the palette/clipboard (so macOS users get `⌘`-style chords too).
fn default_map() -> Keymap {
    let mut km = Keymap {
        name: "Default",
        leader: None,
        leader_timeout_ms: 0,
        binds: Vec::new(),
    };
    km.bind(Action::Quit, &["q", "esc"]);
    km.bind(Action::Help, &["?"]);
    km.bind(Action::Palette, &[":", PALETTE_OS]);
    km.bind(Action::Drawer, &["g"]);
    km.bind(Action::FocusToggle, &["tab"]);
    km.bind_goto();
    km.bind(Action::Copy, &["ctrl+c", COPY_OS]);
    km.bind(Action::Cut, &["ctrl+x", CUT_OS]);
    km.bind(Action::Paste, &["ctrl+v", PASTE_OS]);
    km.bind(Action::CycleKeymap, &["f2"]);
    km
}

/// A Vim-flavoured map: same actions, Vim muscle memory, including two
/// leaderless sequences (`g?` help, `Z Z` quit).
fn vim_map() -> Keymap {
    let mut km = Keymap {
        name: "Vim",
        leader: None,
        leader_timeout_ms: 900,
        binds: Vec::new(),
    };
    km.bind(Action::Quit, &["esc", "Z Z"]);
    km.bind(Action::Help, &["g ?"]);
    km.bind(Action::Palette, &[":", "/"]);
    km.bind(Action::Drawer, &["g s"]);
    km.bind(Action::FocusToggle, &["ctrl+w"]);
    km.bind_goto();
    km.bind(Action::Copy, &["y", "ctrl+c", COPY_OS]);
    km.bind(Action::Cut, &["d", "ctrl+x", CUT_OS]);
    km.bind(Action::Paste, &["p", "ctrl+v", PASTE_OS]);
    km.bind(Action::CycleKeymap, &["f2"]);
    km
}

/// An opencode-style **leader** map: `Ctrl+X` is a prefix with a 2 s
/// timeout, e.g. `⟨leader⟩ p` opens the palette.
fn leader_map() -> Keymap {
    let lead = Chord::parse("ctrl+x");
    let mut km = Keymap {
        name: "Leader",
        leader: lead,
        leader_timeout_ms: 2000,
        binds: Vec::new(),
    };
    km.bind(Action::Quit, &["esc", "<leader> q"]);
    km.bind(Action::Help, &["?", "<leader> ?"]);
    km.bind(Action::Palette, &["<leader> p"]);
    km.bind(Action::Drawer, &["<leader> s"]);
    km.bind(Action::FocusToggle, &["tab", "<leader> w"]);
    km.bind_goto();
    km.bind(Action::Copy, &["<leader> y", "ctrl+c", COPY_OS]);
    km.bind(Action::Cut, &["<leader> d", "ctrl+x", CUT_OS]);
    km.bind(Action::Paste, &["<leader> v", "ctrl+v", PASTE_OS]);
    km.bind(Action::CycleKeymap, &["f2"]);
    km
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn chord_parse_and_match_normalise_case_and_shift() {
        let c = Chord::parse("ctrl+c").unwrap();
        assert!(c.matches(&ev(KeyCode::Char('c'), KeyModifiers::CONTROL)));
        assert!(c.matches(&ev(KeyCode::Char('C'), KeyModifiers::CONTROL)));
        assert!(!c.matches(&ev(KeyCode::Char('c'), KeyModifiers::NONE)));
        assert!(
            Chord::parse("esc")
                .unwrap()
                .matches(&ev(KeyCode::Esc, KeyModifiers::NONE))
        );
        assert!(
            Chord::parse("f2")
                .unwrap()
                .matches(&ev(KeyCode::F(2), KeyModifiers::NONE))
        );
        assert!(
            Chord::parse("?")
                .unwrap()
                .matches(&ev(KeyCode::Char('?'), KeyModifiers::NONE))
        );
        assert!(Chord::parse("nonsense++").is_none());
    }

    #[test]
    fn default_map_resolves_today_s_globals() {
        let mut k = Keymaps::new();
        assert_eq!(
            k.resolve(&ev(KeyCode::Char(':'), KeyModifiers::NONE), 0),
            Some(Action::Palette)
        );
        assert_eq!(
            k.resolve(&ev(KeyCode::Char('q'), KeyModifiers::NONE), 0),
            Some(Action::Quit)
        );
        assert_eq!(
            k.resolve(&ev(KeyCode::Char('5'), KeyModifiers::NONE), 0),
            Some(Action::Goto(5))
        );
        // An arrow is not a shell binding — it falls through to the screen.
        assert_eq!(k.resolve(&ev(KeyCode::Up, KeyModifiers::NONE), 0), None);
    }

    #[test]
    fn cycling_keymaps_changes_the_bindings() {
        let mut k = Keymaps::new();
        assert_eq!(k.status().0, "Default");
        assert_eq!(k.cycle(), "Vim");
        // Vim binds palette to `/` as well as `:`.
        assert_eq!(
            k.resolve(&ev(KeyCode::Char('/'), KeyModifiers::NONE), 0),
            Some(Action::Palette)
        );
    }

    #[test]
    fn leader_sequence_resolves_and_times_out() {
        let mut k = Keymaps::new();
        k.cycle(); // Vim
        k.cycle(); // Leader (ctrl+x prefix, 2000ms ≈ 16 ticks)
        let lead = ev(KeyCode::Char('x'), KeyModifiers::CONTROL);
        // leader then `p` → palette
        assert_eq!(k.resolve(&lead, 0), None, "leader arms, no action yet");
        assert_eq!(
            k.resolve(&ev(KeyCode::Char('p'), KeyModifiers::NONE), 1),
            Some(Action::Palette)
        );
        // leader then wait past the timeout → the next key is fresh
        assert_eq!(k.resolve(&lead, 100), None);
        k.expire(200);
        assert_eq!(
            k.resolve(&ev(KeyCode::Char('p'), KeyModifiers::NONE), 200),
            None,
            "expired leader does not swallow the key"
        );
    }

    #[test]
    fn user_override_remaps_and_disables_by_id() {
        let mut k = Keymaps::new();
        k.set_override(Action::Palette, "ctrl+p");
        assert_eq!(
            k.resolve(&ev(KeyCode::Char('p'), KeyModifiers::CONTROL), 0),
            Some(Action::Palette)
        );
        // Textual semantics: the old key is gone unless re-listed.
        assert_eq!(
            k.resolve(&ev(KeyCode::Char(':'), KeyModifiers::NONE), 0),
            None
        );
        k.set_override(Action::Help, "none");
        assert_eq!(k.effective().keys_for(Action::Help), "—");
    }

    #[test]
    fn keys_for_powers_the_help_text() {
        let k = Keymaps::new();
        let km = k.effective();
        assert!(km.keys_for(Action::Quit).contains('Q'));
        assert!(km.keys_for(Action::Goto(1)).contains('1'));
    }
}
