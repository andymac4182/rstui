//! System-clipboard copy via **OSC 52** — the terminal-native, dependency-
//! free way to set the host clipboard from a TUI.
//!
//! A faithful port of `rstui-kitchen-sink`'s `clipboard` module (that one is
//! `pub(crate)`, and the ACP client deliberately does not depend on the
//! kitchen sink): the workspace forbids a clipboard crate (ADR 0001/0003) and
//! crossterm has no clipboard API, so "copy" is done the way every terminal
//! app does it — emit `ESC ] 52 ; c ; <base64> BEL`, which the terminal turns
//! into a real host-clipboard write (iTerm2, kitty, wezterm, Ghostty,
//! Alacritty, tmux with `set-clipboard on`, …). Reading the clipboard back
//! over OSC 52 is unreliable and often disabled, so *paste* comes the other
//! direction — the terminal's bracketed [`Event::Paste`](rstui_core::Event::Paste)
//! — and is handled by the app, not here.
//!
//! The escape is written to `/dev/tty` (the controlling terminal) rather than
//! `stdout`, so it can never interleave with the render loop's frame bytes;
//! `stdout` is the fallback. The whole thing is **best-effort and silent**:
//! any failure (no tty, write error) is swallowed — a clipboard that does not
//! work must never take the app down or print noise. It is also gated on
//! `stdout` being a real terminal, so `cargo test` (captured output) never
//! emits an escape into the developer's terminal.

use std::io::{IsTerminal, Write};

/// The standard base64 alphabet (RFC 4648). Hand-rolled so no `base64` crate
/// is pulled in for ~20 lines of encoding.
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with `=` padding.
fn base64(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Sets the host clipboard to `text` via OSC 52. Best-effort: returns `true`
/// if the escape was written, `false` if it was skipped/failed (the caller
/// still shows feedback regardless).
pub(crate) fn copy(text: &str) -> bool {
    // Don't touch a terminal that isn't there (tests, pipes): the system
    // line / toast still happen; only the OS-clipboard hop is skipped.
    if !std::io::stdout().is_terminal() {
        return false;
    }
    let payload = format!("\x1b]52;c;{}\x07", base64(text.as_bytes()));
    // Prefer the controlling terminal so the escape never interleaves with
    // the alternate-screen frame bytes on stdout; fall back to stdout.
    if let Ok(mut tty) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
        if tty.write_all(payload.as_bytes()).is_ok() && tty.flush().is_ok() {
            return true;
        }
    }
    let mut out = std::io::stdout();
    out.write_all(payload.as_bytes()).is_ok() && out.flush().is_ok()
}

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_round_trips_utf8_and_newlines() {
        // The payload the copy path actually feeds it.
        assert_eq!(base64("a\nb".as_bytes()), "YQpi");
        assert_eq!(base64("héllo".as_bytes()), "aMOpbGxv");
    }
}
