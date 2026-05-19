//! The **resumable-session index** — the `(session_id, cwd, agent)` of every
//! session this client has started, so `/resume` can offer them and ask the
//! agent to `session/load` one (Codex's `/resume`).
//!
//! ACP sessions live agent-side (the agent persists the rollout); the client
//! only knows the ids it was handed. So resume is *client bookkeeping* + the
//! ACP `session/load` request. Persistence mirrors [`crate::history`]
//! exactly: a small file under `$XDG_CONFIG_HOME`/`~/.config`
//! (`RSTUI_ACP_SESSIONS` overrides), dependency-free, terminal-gated so
//! `cargo test` never touches the real config dir, pure
//! serialize/parse split from the I/O for unit-testing.

use std::io::IsTerminal;
use std::path::PathBuf;

/// Most recent sessions retained (older ones dropped on save).
pub const SESSIONS_MAX: usize = 100;

/// Where the session index persists (mirrors
/// [`crate::history::history_config_path`]); `RSTUI_ACP_SESSIONS` wins.
#[must_use]
pub fn sessions_config_path() -> PathBuf {
    if let Some(p) = std::env::var_os("RSTUI_ACP_SESSIONS") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".rstui"));
    base.join("rstui").join("acp-client.sessions")
}

/// `Some(path)` when persistence should happen (explicit env always; else
/// only with a real terminal — the OSC-52/history posture).
fn persistence_target() -> Option<PathBuf> {
    if std::env::var_os("RSTUI_ACP_SESSIONS").is_some() || std::io::stdout().is_terminal() {
        Some(sessions_config_path())
    } else {
        None
    }
}

/// Escapes one field (`\` → `\\`, newline → `\n`, tab → `\t`) so a record is
/// always a single tab-delimited line.
fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

/// Inverse of [`enc`].
fn dec(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// One resumable session this client started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRef {
    /// The ACP session id (handed back via `session/load`).
    pub id: String,
    /// Working directory the session ran in.
    pub cwd: String,
    /// The agent launch command (resume needs the *same* agent).
    pub agent: String,
    /// Unix seconds when it was started (sort key; newest first).
    pub when: u64,
}

/// The persisted session index (oldest→newest).
#[derive(Debug, Clone, Default)]
pub struct SessionStore {
    entries: Vec<SessionRef>,
}

impl SessionStore {
    /// An empty store (no I/O — headless tests).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads the persisted index. Never fails (missing/unreadable, or a
    /// skipped target under `cargo test`, → empty).
    #[must_use]
    pub fn load() -> Self {
        let mut s = Self::new();
        if let Some(path) = persistence_target() {
            if let Ok(text) = std::fs::read_to_string(path) {
                s.entries = Self::parse(&text);
                Self::cap(&mut s.entries);
            }
        }
        s
    }

    /// The sessions, newest first (what the picker shows).
    #[must_use]
    pub fn newest_first(&self) -> Vec<SessionRef> {
        let mut v = self.entries.clone();
        v.sort_by_key(|e| std::cmp::Reverse(e.when));
        v
    }

    /// Records a started session: replace any entry with the same id, then
    /// append (so it sorts newest). In-memory only — caller [`save`](Self::save)s.
    pub fn record(&mut self, s: SessionRef) {
        self.entries.retain(|e| e.id != s.id);
        self.entries.push(s);
        Self::cap(&mut self.entries);
    }

    fn cap(v: &mut Vec<SessionRef>) {
        if v.len() > SESSIONS_MAX {
            let drop = v.len() - SESSIONS_MAX;
            v.drain(0..drop);
        }
    }

    /// The on-disk form: one `id\twhen\tagent\tcwd` line per session.
    fn serialize(&self) -> String {
        self.entries
            .iter()
            .map(|e| {
                format!(
                    "{}\t{}\t{}\t{}",
                    enc(&e.id),
                    e.when,
                    enc(&e.agent),
                    enc(&e.cwd)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Inverse of [`serialize`](Self::serialize); malformed lines are skipped.
    fn parse(text: &str) -> Vec<SessionRef> {
        text.lines()
            .filter(|l| !l.is_empty())
            .filter_map(|l| {
                let mut f = l.split('\t');
                let id = dec(f.next()?);
                let when = f.next()?.parse().ok()?;
                let agent = dec(f.next()?);
                let cwd = dec(f.next()?);
                Some(SessionRef {
                    id,
                    cwd,
                    agent,
                    when,
                })
            })
            .collect()
    }

    /// Persists the index (best-effort, terminal-gated — the theme posture).
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

    fn s(id: &str, when: u64) -> SessionRef {
        SessionRef {
            id: id.to_owned(),
            cwd: "/w s/p".to_owned(),
            agent: "npx -y x".to_owned(),
            when,
        }
    }

    #[test]
    fn record_dedups_by_id_and_newest_first_sorts_by_time() {
        let mut st = SessionStore::new();
        st.record(s("a", 10));
        st.record(s("b", 30));
        st.record(s("a", 20)); // same id → replaces, not duplicated
        let nf = st.newest_first();
        assert_eq!(
            nf.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["b", "a"]
        );
        assert_eq!(nf[1].when, 20, "the id was updated, not duplicated");
    }

    #[test]
    fn serialize_parse_round_trips_with_tricky_fields() {
        let mut st = SessionStore::new();
        st.record(SessionRef {
            id: "sess-1".to_owned(),
            cwd: "/has\ttab\nand backslash\\".to_owned(),
            agent: "npx -y @scope/acp --flag".to_owned(),
            when: 1_700_000_000,
        });
        assert_eq!(SessionStore::parse(&st.serialize()), st.entries);
    }
}
