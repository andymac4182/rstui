//! Terminal **window/tab title** via **OSC 2** — the title reflects the
//! session (agent + state) so a backgrounded tab says what it is doing,
//! exactly like the Codex CLI.
//!
//! Same posture as [`crate::clipboard`]: dependency-free, written to
//! `/dev/tty` (preferred, so it never interleaves with the alternate-screen
//! frame bytes) or `stdout`, best-effort and silent, and **terminal-gated**
//! so `cargo test` never emits an escape. The title string is derived by a
//! pure function ([`session_title`]) so it is unit- and `Harness`-testable
//! without a terminal; only the final byte-emit is the side effect.

use std::io::{IsTerminal, Write};

use crate::app::Screen;

/// The short agent token shown in the title: the last path component of the
/// last whitespace-separated word of the launch command
/// (`"npx -y @zed/claude-code-acp"` → `"claude-code-acp"`), empty → `"agent"`.
#[must_use]
pub fn short_agent(command: &str) -> &str {
    let word = command.split_whitespace().next_back().unwrap_or("");
    let tail = word.rsplit(['/', '\\']).next().unwrap_or(word);
    if tail.is_empty() { "agent" } else { tail }
}

/// Strips control characters (incl. newlines and the BEL/ESC that would
/// terminate or smuggle into the escape) and clamps the length, so the title
/// is always a safe single line — Codex sanitizes for the same reason.
#[must_use]
fn sanitize(s: &str) -> String {
    let cleaned: String = s.chars().filter(|c| !c.is_control()).collect();
    let cleaned = cleaned.trim();
    if cleaned.chars().count() > 96 {
        cleaned.chars().take(95).chain(['…']).collect()
    } else {
        cleaned.to_owned()
    }
}

/// The desired terminal title for the current session state. Pure: the app
/// compares it to the last-emitted value and only emits on a change, so this
/// is the single source of truth the tests pin.
#[must_use]
pub fn session_title(
    screen: Screen,
    agent_command: &str,
    streaming: bool,
    approval_pending: bool,
) -> String {
    let agent = short_agent(agent_command);
    let body = match screen {
        Screen::Picker => "pick an agent".to_owned(),
        Screen::Connecting => format!("connecting {agent}"),
        Screen::Chat if approval_pending => format!("{agent} — approval needed"),
        Screen::Chat if streaming => format!("{agent} — working…"),
        Screen::Chat => agent.to_owned(),
    };
    // A leading marker when the session needs the user, so a glance at the
    // tab strip is enough (Codex's "action required" cue).
    let prefix = if approval_pending { "● " } else { "" };
    sanitize(&format!("{prefix}rstui-acp — {body}"))
}

/// Emits `payload` to the controlling terminal (`/dev/tty` preferred, then
/// `stdout`). Best-effort; `false` if skipped/failed.
fn emit(payload: &str) -> bool {
    if !std::io::stdout().is_terminal() {
        return false;
    }
    if let Ok(mut tty) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
        if tty.write_all(payload.as_bytes()).is_ok() && tty.flush().is_ok() {
            return true;
        }
    }
    let mut out = std::io::stdout();
    out.write_all(payload.as_bytes()).is_ok() && out.flush().is_ok()
}

/// Sets the terminal title via OSC 2. Best-effort and terminal-gated (a no-op
/// under `cargo test`). The caller has already sanitized via
/// [`session_title`].
pub(crate) fn set(title: &str) -> bool {
    emit(&format!("\x1b]2;{title}\x07"))
}

/// Clears the title back to empty on exit, so the user's shell can reassert
/// its own (leaving a stale "working…" tab would be the rude default).
pub(crate) fn clear() -> bool {
    emit("\x1b]2;\x07")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_agent_takes_the_last_path_component_of_the_last_word() {
        assert_eq!(
            short_agent("npx -y @zed-industries/claude-code-acp"),
            "claude-code-acp"
        );
        assert_eq!(short_agent("/usr/local/bin/codex"), "codex");
        assert_eq!(short_agent("gemini"), "gemini");
        assert_eq!(short_agent(""), "agent");
        assert_eq!(short_agent("   "), "agent");
    }

    #[test]
    fn session_title_reflects_screen_and_state() {
        assert_eq!(
            session_title(Screen::Picker, "codex", false, false),
            "rstui-acp — pick an agent"
        );
        assert_eq!(
            session_title(Screen::Connecting, "bin/codex", false, false),
            "rstui-acp — connecting codex"
        );
        assert_eq!(
            session_title(Screen::Chat, "codex", false, false),
            "rstui-acp — codex"
        );
        assert_eq!(
            session_title(Screen::Chat, "codex", true, false),
            "rstui-acp — codex — working…"
        );
        // Approval needed wins over streaming and adds the attention marker.
        assert_eq!(
            session_title(Screen::Chat, "codex", true, true),
            "● rstui-acp — codex — approval needed"
        );
    }

    #[test]
    fn sanitize_strips_control_chars_and_clamps_length() {
        assert_eq!(sanitize("a\nb\x07c\x1bd"), "abcd");
        assert!(sanitize(&"x".repeat(500)).chars().count() <= 96);
        // A title can never break out of / terminate the OSC 2 escape.
        let t = session_title(Screen::Chat, "evil\x07\x1b]2;pwned", false, false);
        assert!(!t.contains('\x07') && !t.contains('\x1b'));
    }
}
