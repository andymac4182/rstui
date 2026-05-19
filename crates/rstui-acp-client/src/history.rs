//! Composer **input history** — the submitted prompts, recalled with ↑/↓ on
//! the composer (readline / Codex-CLI ergonomics), persisted across runs.
//!
//! Persistence mirrors the theme seam in [`crate::theme`]: a single small
//! file under `$XDG_CONFIG_HOME`/`~/.config`, written best-effort and never
//! fatal — a history file that cannot be read or written must never take the
//! client down. The store is dependency-free: one record per line, with `\`
//! and newline escaped, so a multi-line prompt round-trips without pulling in
//! a serialization crate (ADR 0001/0003).

use std::io::IsTerminal;
use std::path::PathBuf;

/// Most recent prompts retained (older ones are dropped on save). Generous
/// enough to span a working session; small enough that the file stays tiny
/// and the rewrite-on-record cost is irrelevant.
pub const HISTORY_MAX: usize = 500;

/// Where the composer history persists. An explicit `RSTUI_ACP_HISTORY` path
/// wins (the same typo-safe override convention as `RSTUI_THEME` /
/// `RSTUI_KEYMAP`); otherwise `$XDG_CONFIG_HOME` or `~/.config` →
/// `rstui/acp-client.history`. Mirrors [`crate::theme::theme_config_path`].
#[must_use]
pub fn history_config_path() -> PathBuf {
    if let Some(p) = std::env::var_os("RSTUI_ACP_HISTORY") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".rstui"));
    base.join("rstui").join("acp-client.history")
}

/// The file to load/save, or `None` to skip persistence entirely.
///
/// An explicit `RSTUI_ACP_HISTORY` is always honoured (users *and* the
/// round-trip tests). Otherwise persistence only happens when `stdout` is a
/// real terminal — so `cargo test` (captured stdout, no env) never reads or
/// writes the developer's real config dir, the exact gate the OSC 52
/// clipboard helper uses for the same reason.
fn persistence_target() -> Option<PathBuf> {
    if std::env::var_os("RSTUI_ACP_HISTORY").is_some() || std::io::stdout().is_terminal() {
        Some(history_config_path())
    } else {
        None
    }
}

/// Escapes a prompt to one storage line (`\` → `\\`, newline → `\n`).
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out
}

/// Inverse of [`encode`]. A trailing lone `\` (corrupt line) is dropped.
fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The submitted-prompt ring plus the ↑/↓ browse cursor.
///
/// `entries` is oldest→newest. `pos` is `Some(i)` while browsing (showing
/// `entries[i]`) and `None` while editing the live draft; `draft` holds the
/// text that was in the composer when browsing began, so stepping forward
/// past the newest entry restores exactly what the user was typing.
#[derive(Debug, Clone, Default)]
pub struct InputHistory {
    entries: Vec<String>,
    pos: Option<usize>,
    draft: String,
}

impl InputHistory {
    /// An empty history (no disk I/O — used by headless tests).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads the persisted history, newest entries last. Never fails: a
    /// missing/unreadable file (or a skipped persistence target under
    /// `cargo test`) yields an empty history.
    #[must_use]
    pub fn load() -> Self {
        let mut h = Self::new();
        let Some(path) = persistence_target() else {
            return h;
        };
        if let Ok(text) = std::fs::read_to_string(path) {
            h.entries = Self::parse(&text);
            if h.entries.len() > HISTORY_MAX {
                let drop = h.entries.len() - HISTORY_MAX;
                h.entries.drain(0..drop);
            }
        }
        h
    }

    /// The on-disk form: one escaped record per line, oldest→newest.
    fn serialize(&self) -> String {
        self.entries
            .iter()
            .map(|e| encode(e))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Inverse of [`serialize`](Self::serialize): blank lines dropped, each
    /// surviving line un-escaped back to a (possibly multi-line) prompt.
    fn parse(text: &str) -> Vec<String> {
        text.lines().filter(|l| !l.is_empty()).map(decode).collect()
    }

    /// The retained prompts, oldest→newest (read-only; for tests/inspection).
    #[must_use]
    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    /// `true` while ↑/↓ is browsing a recalled entry (not the live draft).
    #[must_use]
    pub fn browsing(&self) -> bool {
        self.pos.is_some()
    }

    /// Records a just-submitted prompt: appended unless empty or identical to
    /// the most recent, capped to [`HISTORY_MAX`], browse cursor reset.
    /// In-memory only — the caller persists with [`save`](Self::save) so the
    /// pure ring/nav logic is unit-testable without touching disk.
    pub fn record(&mut self, text: &str) {
        self.reset();
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if self.entries.last().map(String::as_str) == Some(text) {
            return;
        }
        self.entries.push(text.to_owned());
        if self.entries.len() > HISTORY_MAX {
            let drop = self.entries.len() - HISTORY_MAX;
            self.entries.drain(0..drop);
        }
    }

    /// Stops browsing, dropping the saved draft. Called on any composer edit
    /// so a fresh ↑ starts again from the newest entry.
    pub fn reset(&mut self) {
        self.pos = None;
        self.draft.clear();
    }

    /// Steps to the previous (older) entry, given the composer's current text
    /// (stashed as the draft the first time so [`newer`](Self::newer) can
    /// restore it). Returns the text to show, or `None` if there is nothing
    /// older to show.
    pub fn older(&mut self, current: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        match self.pos {
            None => {
                self.draft = current.to_owned();
                let i = self.entries.len() - 1;
                self.pos = Some(i);
                Some(self.entries[i].clone())
            }
            Some(0) => Some(self.entries[0].clone()),
            Some(i) => {
                self.pos = Some(i - 1);
                Some(self.entries[i - 1].clone())
            }
        }
    }

    /// Steps to the next (newer) entry. Past the newest, restores the stashed
    /// draft and stops browsing. Returns the text to show, or `None` if not
    /// currently browsing.
    pub fn newer(&mut self) -> Option<String> {
        match self.pos {
            None => None,
            Some(i) if i + 1 < self.entries.len() => {
                self.pos = Some(i + 1);
                Some(self.entries[i + 1].clone())
            }
            Some(_) => {
                self.pos = None;
                Some(std::mem::take(&mut self.draft))
            }
        }
    }

    /// Persists the ring (best-effort, silent on any failure — exactly the
    /// theme-write posture). Call after [`record`](Self::record). A no-op
    /// when persistence is skipped (no terminal under `cargo test`, no
    /// `RSTUI_ACP_HISTORY` override).
    pub fn save(&self) {
        let Some(path) = persistence_target() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, self.serialize());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_round_trips_backslash_and_newlines() {
        for s in ["plain", "a\nb", "back\\slash", "mix\\\nend", "", "\n\n"] {
            assert_eq!(decode(&encode(s)), s, "round-trip failed for {s:?}");
        }
    }

    #[test]
    fn record_dedups_consecutive_and_skips_empty() {
        let mut h = InputHistory::new();
        h.record("one");
        h.record("one"); // consecutive dup ignored
        h.record("   "); // whitespace-only ignored
        h.record("two");
        assert_eq!(h.entries(), ["one", "two"]);
    }

    #[test]
    fn older_newer_walk_and_restore_the_draft() {
        let mut h = InputHistory::new();
        h.record("first");
        h.record("second");

        // Start browsing from a half-typed draft.
        assert_eq!(h.older("draft").as_deref(), Some("second"));
        assert!(h.browsing());
        assert_eq!(h.older("draft").as_deref(), Some("first"));
        // Clamped at the oldest entry.
        assert_eq!(h.older("draft").as_deref(), Some("first"));

        // Walking forward returns to the newest, then restores the draft.
        assert_eq!(h.newer().as_deref(), Some("second"));
        assert_eq!(h.newer().as_deref(), Some("draft"));
        assert!(!h.browsing(), "past the newest, browsing ends");
        assert_eq!(h.newer(), None, "not browsing → nothing to do");
    }

    #[test]
    fn record_resets_browsing() {
        let mut h = InputHistory::new();
        h.record("a");
        h.older("live");
        assert!(h.browsing());
        h.record("b");
        assert!(!h.browsing());
        assert_eq!(h.entries(), ["a", "b"]);
    }

    #[test]
    fn older_on_empty_history_is_none() {
        let mut h = InputHistory::new();
        assert_eq!(h.older("x"), None);
        assert!(!h.browsing());
    }

    #[test]
    fn serialize_parse_round_trips_including_multiline_and_backslash() {
        let mut h = InputHistory::new();
        h.record("one");
        h.record("multi\nline\nprompt");
        h.record("back\\slash and \\n literal");
        // The on-disk form survives a full round-trip with no loss, so a
        // restored session sees exactly the prompts it submitted.
        assert_eq!(InputHistory::parse(&h.serialize()), h.entries());
    }
}
