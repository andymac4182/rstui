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
//! - **End-user config, serde-free.** [`Keymaps::load_overrides`] parses a
//!   trivial `id = keys` text file (hand-rolled, the same ethos as
//!   [`Chord::parse`] — no serde, ADR 0002) and [`Keymaps::set_active`]
//!   picks a map by name. A user customises keys by editing a file; no
//!   bespoke app UI required (the kitchen sink wires both through
//!   `RSTUI_KEYMAP`, mirroring `RSTUI_THEME`).
//!
//! # The user config file
//!
//! One `id = keys` per line; full-line `#` comments and blank lines are
//! ignored; unknown ids are skipped (a typo never breaks your keys):
//!
//! ```text
//! # ~/.config/myapp/keymap — edit and restart
//! keymap      = Vim          # optionally pick the active map by name
//! app.palette = ctrl+p, /    # remap (comma-separates alternatives)
//! app.help    = none         # disable an action
//! ```
//!
//! Keys are exactly what [`Chord::parse`] accepts; the `id`s are the
//! stable [`Action::id`] strings ([`Action::from_id`] is the inverse).
//!
//! # Defining your own actions
//!
//! The built-in [`Action`]s are a starter set, not a closed one. An app
//! adds its **own** actions with [`Action::Custom`] and registers them
//! *on top of* every shipped map with [`Keymaps::bind`]:
//!
//! ```
//! use rstui_keymap::{Action, Keymaps};
//!
//! const SAVE: Action = Action::Custom("myapp.save");
//! const FIND: Action = Action::Custom("myapp.find");
//!
//! let mut keymaps = Keymaps::new();   // keeps Quit/Help/Palette/…
//! keymaps.bind(SAVE, "ctrl+s");       // …and adds yours, in every map
//! keymaps.bind(FIND, "ctrl+f, /");
//! ```
//!
//! From here on app actions are indistinguishable from built-ins:
//! `resolve` returns them, `keys_for(SAVE)` powers the help/footer, a
//! user override or a `myapp.save = ctrl+k` config line remaps them. An
//! app whose vocabulary is entirely its own can instead build complete
//! maps with [`Keymap::new`]/[`Keymap::bound`] and [`Keymaps::from_maps`].

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
    /// An **application-defined** action, identified by its own stable
    /// dotted id (`"myapp.save"`). Apps add their own actions *on top of*
    /// these built-ins with [`Keymaps::bind`] (or build a whole map with
    /// [`Keymap::bound`]); everything else — resolve, the help/footer
    /// reverse-lookup, user overrides, config files — treats them exactly
    /// like the built-ins. The id is a `&'static str` so [`Action`] stays
    /// `Copy` and allocation-free.
    Custom(&'static str),
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
            Action::Custom(id) => id,
        }
    }

    /// The inverse of [`Action::id`] for the **built-ins** — resolves a
    /// config-file id back to its action. `"app.goto"` returns `None` on
    /// purpose: the nine screen-jump digits are conventional and multi-bound,
    /// so a single file override of them is ill-defined (they stay `1`–`9`).
    /// App-defined ids can't be reconstructed here (the `&'static str` isn't
    /// known) — use [`Keymaps::action_for_id`], which also matches the
    /// app's currently-bound [`Action::Custom`]s, so user config files
    /// remap app actions too.
    pub fn from_id(id: &str) -> Option<Action> {
        Some(match id {
            "app.quit" => Action::Quit,
            "app.help" => Action::Help,
            "app.palette" => Action::Palette,
            "app.drawer" => Action::Drawer,
            "app.focus_toggle" => Action::FocusToggle,
            "edit.copy" => Action::Copy,
            "edit.cut" => Action::Cut,
            "edit.paste" => Action::Paste,
            "app.cycle_keymap" => Action::CycleKeymap,
            _ => return None,
        })
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
            // App actions carry their help in the app (it owns the copy);
            // the engine only needs the binding + the reverse-lookup.
            Action::Custom(_) => "",
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
    /// An empty, leaderless keymap — the start of an app's own map.
    /// Chain [`Keymap::bound`] / [`Keymap::with_leader`]:
    ///
    /// ```
    /// # use rstui_keymap::{Action, Keymap};
    /// const SAVE: Action = Action::Custom("myapp.save");
    /// let km = Keymap::new("MyApp")
    ///     .bound(Action::Quit, &["q", "ctrl+c"])
    ///     .bound(SAVE, &["ctrl+s"]);
    /// ```
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            leader: None,
            leader_timeout_ms: 0,
            binds: Vec::new(),
        }
    }

    /// Set an opencode-style leader chord + its timeout (ms) for
    /// `<leader> x` chains in this map.
    #[must_use]
    pub fn with_leader(mut self, leader: Chord, timeout_ms: u64) -> Self {
        self.leader = Some(leader);
        self.leader_timeout_ms = timeout_ms;
        self
    }

    /// Builder form of [`Keymap::bind`] (chainable).
    #[must_use]
    pub fn bound(mut self, action: Action, specs: &[&str]) -> Self {
        self.bind(action, specs);
        self
    }

    /// A `Key` trigger equal to this keymap's leader is invalid — the
    /// prefix is reserved, it can never also fire a plain action.
    fn keeps(&self, t: &Trigger) -> bool {
        !matches!((t, self.leader), (Trigger::Key(c), Some(l)) if *c == l)
    }

    /// Bind `action` to one or more trigger specs (`"ctrl+s"`, `"g s"`,
    /// `"<leader> p"`). Per-keymap-leader-safe and de-duped; call once per
    /// action when building a map.
    pub fn bind(&mut self, action: Action, specs: &[&str]) {
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

/// What [`Keymaps::dispatch`] tells the app to do with one key — the whole
/// listen→map→act decision in three cases, so the reducer is one `match`
/// instead of the hand-written resolve / `armed` / fall-through trio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatch {
    /// A binding fired — perform this [`Action`].
    Act(Action),
    /// A leader/prefix was just armed — swallow this key and wait for the
    /// rest of the sequence (do *not* hand it to the screen).
    Pending,
    /// Unbound by the keymap — hand the raw key to the focused screen
    /// (arrows, typing, pane-relative motions, …).
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

    /// A registry over the app's **own** keymaps (built with [`Keymap::new`]
    /// / [`Keymap::bound`]) instead of the three batteries-included ones —
    /// for an app whose action vocabulary is entirely its own. An empty
    /// `maps` falls back to [`Keymaps::new`] so `effective`/`resolve` never
    /// index out of bounds (a config typo can't blank the keymap).
    pub fn from_maps(maps: Vec<Keymap>) -> Self {
        let sets = if maps.is_empty() {
            vec![default_map(), vim_map(), leader_map()]
        } else {
            maps
        };
        Self {
            sets,
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

    /// Register an **app-defined** action *on top of* every shipped keymap,
    /// so it works whichever map (Default/Vim/Leader, or your own) is
    /// active — the "additional to ours" path. `keys` is comma-separated
    /// trigger specs (exactly [`Chord::parse`] tokens / `<leader> x`
    /// chains). Call once per app action at startup:
    ///
    /// ```
    /// # use rstui_keymap::{Action, Keymaps};
    /// const SAVE: Action = Action::Custom("myapp.save");
    /// let mut keymaps = Keymaps::new();
    /// keymaps.bind(SAVE, "ctrl+s");
    /// // now `resolve` returns `SAVE`, and `keys_for(SAVE)` lights up the
    /// // help/footer — same as any built-in, in every map.
    /// ```
    pub fn bind(&mut self, action: Action, keys: &str) {
        let specs: Vec<&str> = keys
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        for km in &mut self.sets {
            km.bind(action, &specs);
        }
    }

    /// Resolve a config-file id to an action: a built-in via
    /// [`Action::from_id`], else the app's currently-bound
    /// [`Action::Custom`] with that id — so a user keymap file can remap
    /// app-defined actions with no extra app code.
    pub fn action_for_id(&self, id: &str) -> Option<Action> {
        if let Some(a) = Action::from_id(id) {
            return Some(a);
        }
        self.sets
            .iter()
            .flat_map(|km| km.binds.iter())
            .map(|b| b.action)
            .find(|a| matches!(a, Action::Custom(s) if *s == id))
    }

    /// The names of every shipped keymap (`Default` / `Vim` / `Leader`) —
    /// the choices a user picks between (for a UI list or config docs).
    pub fn map_names(&self) -> Vec<&'static str> {
        self.sets.iter().map(|k| k.name).collect()
    }

    /// Pick the active keymap **by name** (case-insensitive), so a user can
    /// jump straight to `"Vim"` instead of cycling. Clears any half-typed
    /// sequence. Returns `false` (and changes nothing) for an unknown name,
    /// so a typo just keeps the current map.
    pub fn set_active(&mut self, name: &str) -> bool {
        if let Some(i) = self
            .sets
            .iter()
            .position(|k| k.name.eq_ignore_ascii_case(name.trim()))
        {
            self.active = i;
            self.pending = None;
            true
        } else {
            false
        }
    }

    /// Apply an end-user keymap config: one `id = keys` per line. Full-line
    /// `#` comments and blank lines are ignored; the special `keymap = Name`
    /// line picks the active map ([`Self::set_active`]); every other line is
    /// `id = keys` fed to [`Self::set_override`] (so `keys` is exactly what
    /// [`Chord::parse`] accepts, comma-separated, or `none` to disable).
    /// Unknown ids and unparseable lines are skipped — a typo never errors
    /// the app or breaks the other bindings. Returns how many lines were
    /// applied. Serde-free by design (hand-parsed, ADR 0002); reading the
    /// file is the app's one-liner (`fs::read_to_string`), mirroring how
    /// `rstui-theme` is wired through `RSTUI_THEME`.
    pub fn load_overrides(&mut self, text: &str) -> usize {
        let mut applied = 0;
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, val)) = line.split_once('=') else {
                continue;
            };
            let (key, val) = (key.trim().to_ascii_lowercase(), val.trim());
            if key == "keymap" {
                if self.set_active(val) {
                    applied += 1;
                }
            } else if let Some(action) = self.action_for_id(&key) {
                self.set_override(action, val);
                applied += 1;
            }
        }
        applied
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
    /// state machine.
    ///
    /// `now_ms` is a **monotonic millisecond clock** the caller supplies
    /// (e.g. `Instant::elapsed().as_millis()` live, a controlled value
    /// under the `Harness`). It is *only* the leader-sequence deadline:
    /// resolution itself is event-driven, and a stale prefix self-clears
    /// here on the very next key, so **no animation loop is required** —
    /// an app whose keymap has no leader sequence can pass `0` forever.
    /// Most apps should call [`Keymaps::dispatch`] instead.
    pub fn resolve(&mut self, ev: &KeyEvent, now_ms: u64) -> Option<Action> {
        let km = self.effective();

        // An armed, un-expired leader: this key completes (or breaks) it.
        if let Some((first, deadline)) = self.pending {
            self.pending = None;
            if now_ms <= deadline {
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
                let deadline = now_ms.saturating_add(km.leader_timeout_ms);
                self.pending = Some((c, deadline));
                None
            }
            Resolved::Fall => None,
        }
    }

    /// Drop a leader that has sat un-completed past its timeout, so a
    /// stale *armed indicator* does not linger forever on a truly idle
    /// screen. Purely cosmetic — `resolve` already self-clears a stale
    /// prefix on the next key, so calling this is **optional**: only an
    /// app that both ships a leader keymap *and* shows the armed hint
    /// needs it, and only from whatever clock it already has (never add
    /// an animation loop for it). `now_ms` is the same monotonic ms clock.
    pub fn expire(&mut self, now_ms: u64) {
        if let Some((_, deadline)) = self.pending {
            if now_ms > deadline {
                self.pending = None;
            }
        }
    }

    /// The whole **listen → map → act** seam in one call: resolve `ev`,
    /// and say what the app should do. Collapses the
    /// resolve / `armed` / fall-through dance every app used to hand-write.
    ///
    /// `now_ms`: see [`resolve`](Self::resolve) — a monotonic ms clock, or
    /// just `0` if your keymap has no leader sequence (no clock, no
    /// animation loop, still correct).
    ///
    /// ```
    /// # use rstui_keymap::{Dispatch, Keymaps};
    /// # use rstui_core::{KeyCode, KeyEvent, KeyModifiers};
    /// # let mut keymaps = Keymaps::new();
    /// # let ev = KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE);
    /// match keymaps.dispatch(&ev, 0) {
    ///     Dispatch::Act(action) => { /* perform `action` */ }
    ///     Dispatch::Pending     => { /* a leader was armed — swallow */ }
    ///     Dispatch::Fall        => { /* unbound — hand the raw key on */ }
    /// }
    /// ```
    pub fn dispatch(&mut self, ev: &KeyEvent, now_ms: u64) -> Dispatch {
        match self.resolve(ev, now_ms) {
            Some(action) => Dispatch::Act(action),
            None if self.armed() => Dispatch::Pending,
            None => Dispatch::Fall,
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
    fn leader_sequence_resolves_and_times_out_in_real_milliseconds() {
        let mut k = Keymaps::new();
        k.cycle(); // Vim
        k.cycle(); // Leader (ctrl+x prefix, 2000 ms timeout — real ms now)
        let lead = ev(KeyCode::Char('x'), KeyModifiers::CONTROL);
        // leader@0 ms, then `p` 1500 ms later (< 2000) → palette.
        assert_eq!(k.resolve(&lead, 0), None, "leader arms, no action yet");
        assert_eq!(
            k.resolve(&ev(KeyCode::Char('p'), KeyModifiers::NONE), 1500),
            Some(Action::Palette),
            "within the 2000 ms window the sequence completes"
        );
        // leader@0, idle past 2000 ms: `expire` drops the stale prefix…
        assert_eq!(k.resolve(&lead, 0), None);
        k.expire(2500);
        // …so `p` 2500 ms later is a fresh (unbound-in-Leader) key, not a
        // swallowed continuation — and *resolution self-clears anyway*.
        assert_eq!(
            k.resolve(&ev(KeyCode::Char('p'), KeyModifiers::NONE), 2500),
            None,
            "an expired leader never eats a much-later key"
        );
        // No clock loop needed: even without calling `expire`, the next
        // key past the deadline self-clears the pending prefix.
        assert_eq!(k.resolve(&lead, 0), None);
        assert_eq!(
            k.resolve(&ev(KeyCode::Char('p'), KeyModifiers::NONE), 9999),
            None,
            "stale prefix self-clears on the next key — event-driven"
        );
    }

    #[test]
    fn dispatch_is_the_one_call_seam_act_pending_fall() {
        let mut k = Keymaps::new(); // Default map (no leader)
        // A bound chord → Act; an unbound key → Fall; `0` clock is fine
        // because the Default map has no leader sequence.
        assert_eq!(
            k.dispatch(&ev(KeyCode::Char(':'), KeyModifiers::NONE), 0),
            Dispatch::Act(Action::Palette)
        );
        assert_eq!(
            k.dispatch(&ev(KeyCode::Up, KeyModifiers::NONE), 0),
            Dispatch::Fall
        );
        // Leader map: the prefix arms → Pending, the next key → Act.
        k.cycle();
        k.cycle(); // Leader
        let lead = ev(KeyCode::Char('x'), KeyModifiers::CONTROL);
        assert_eq!(k.dispatch(&lead, 0), Dispatch::Pending);
        assert_eq!(
            k.dispatch(&ev(KeyCode::Char('p'), KeyModifiers::NONE), 100),
            Dispatch::Act(Action::Palette)
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

    #[test]
    fn from_id_round_trips_every_shown_action() {
        for a in Action::shown() {
            // `Goto` is deliberately not file-rebindable (see below).
            if matches!(a, Action::Goto(_)) {
                continue;
            }
            assert_eq!(
                Action::from_id(a.id()),
                Some(a),
                "id {} should round-trip",
                a.id()
            );
        }
        // `app.goto` is deliberately not user-rebindable from a file; an
        // unknown id is `None`; `Custom` ids aren't reconstructible here.
        assert_eq!(Action::from_id("app.goto"), None);
        assert_eq!(Action::from_id("nope"), None);
        assert_eq!(Action::from_id("myapp.save"), None);
    }

    #[test]
    fn set_active_picks_a_map_by_name_case_insensitively() {
        let mut k = Keymaps::new();
        assert_eq!(k.map_names(), vec!["Default", "Vim", "Leader"]);
        assert!(k.set_active("vim"));
        assert_eq!(k.active_name(), "Vim");
        // `/` opens the palette only in Vim — proves the map really switched.
        assert_eq!(
            k.resolve(&ev(KeyCode::Char('/'), KeyModifiers::NONE), 0),
            Some(Action::Palette)
        );
        assert!(!k.set_active("does-not-exist"));
        assert_eq!(k.active_name(), "Vim", "unknown name keeps current map");
    }

    #[test]
    fn load_overrides_remaps_disables_and_selects_a_map() {
        let mut k = Keymaps::new();
        let n = k.load_overrides(
            "# a user keymap file\n\
             keymap = Vim\n\
             app.palette = ctrl+p\n\
             app.help = none\n\
             bogus.id = whatever\n\
             # trailing comment\n",
        );
        assert_eq!(n, 3, "keymap + palette + help applied; bogus skipped");
        assert_eq!(k.active_name(), "Vim");
        assert_eq!(
            k.resolve(&ev(KeyCode::Char('p'), KeyModifiers::CONTROL), 0),
            Some(Action::Palette)
        );
        // Textual semantics: the old palette key is gone unless re-listed.
        assert_eq!(
            k.resolve(&ev(KeyCode::Char(':'), KeyModifiers::NONE), 0),
            None
        );
        assert_eq!(k.effective().keys_for(Action::Help), "—");
    }

    const SAVE: Action = Action::Custom("myapp.save");

    #[test]
    fn app_defined_action_works_like_a_builtin_in_every_map() {
        let mut k = Keymaps::new();
        k.bind(SAVE, "ctrl+s");
        let press = ev(KeyCode::Char('s'), KeyModifiers::CONTROL);
        // Resolves under the Default map…
        assert_eq!(k.resolve(&press, 0), Some(SAVE));
        // …and still after switching maps (added on top of every one).
        k.set_active("Leader");
        assert_eq!(k.resolve(&press, 0), Some(SAVE));
        // Powers the help/footer reverse-lookup like any built-in.
        assert!(k.effective().keys_for(SAVE).contains('S'));
        assert_eq!(SAVE.id(), "myapp.save");
    }

    #[test]
    fn from_maps_builds_an_app_only_registry_and_config_remaps_custom() {
        let mut k = Keymaps::from_maps(vec![
            Keymap::new("App")
                .bound(Action::Quit, &["q"])
                .bound(SAVE, &["ctrl+s"]),
        ]);
        assert_eq!(k.map_names(), vec!["App"]);
        assert_eq!(
            k.resolve(&ev(KeyCode::Char('s'), KeyModifiers::CONTROL), 0),
            Some(SAVE)
        );
        // A user keymap file can remap the app's own action by its id.
        assert_eq!(k.action_for_id("myapp.save"), Some(SAVE));
        assert_eq!(k.load_overrides("myapp.save = ctrl+k\n"), 1);
        assert_eq!(
            k.resolve(&ev(KeyCode::Char('k'), KeyModifiers::CONTROL), 0),
            Some(SAVE)
        );
        // Empty map list never panics — falls back to the built-ins.
        assert_eq!(Keymaps::from_maps(vec![]).map_names().len(), 3);
    }
}
