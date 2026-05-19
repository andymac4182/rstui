//! `rstui-acp-client` — a full-screen [Agent Client Protocol][acp] chat client
//! built entirely on the rstui TUI framework.
//!
//! [acp]: https://agentclientprotocol.com
//!
//! The crate is split so the UI logic is deterministically testable headlessly
//! (rstui's `Harness`) while the binary owns only the terminal + async-runtime
//! plumbing:
//!
//! - [`app`] — the rstui [`App`](rstui_runtime::App): the chat model, the
//!   `update` reducer, and the pure `view`. No terminal, no tokio: every
//!   screen is reachable from a `Harness` test.
//! - [`acp`] — the ACP transport: a tokio task that owns the agent child
//!   process and speaks `sacp` JSON-RPC, bridged to the reducer over channels.
//! - [`registry`] — the ACP registry loader + the agent picker model.
//! - [`plugin`] — the deny-by-default plugin extension layer (powerline
//!   footer, slash commands, ask-user overlay): a separate-process protocol
//!   reusing ADR 0007's posture, complementary to `rstui-plugin-host`.
//! - [`history`] — the persisted composer input history (↑/↓ prompt recall).
//! - [`input`] — the async terminal [`AsyncEventSource`](rstui_runtime::AsyncEventSource).
//!
//! [`run`] composes them into the live full-screen client.

pub mod acp;
pub mod app;
pub(crate) mod clipboard;
pub mod history;
pub mod input;
pub mod plugin;
pub mod registry;
pub mod theme;
pub mod ui;

use std::io::{self, Stdout};

use rstui_crossterm::{CrosstermBackend, LifecycleOptions, TerminalGuard};

use crate::app::ChatApp;
use crate::input::TerminalEvents;

/// Runtime configuration resolved from CLI args / environment before the
/// terminal is taken over.
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// An explicit agent launch command (`--agent "npx -y …"`). When `None`
    /// the client opens the registry picker instead.
    pub agent_command: Option<String>,
    /// Plugin launch commands to attach (`--plugin <cmd>`, repeatable).
    pub plugins: Vec<String>,
}

impl Config {
    /// Parses `--agent <cmd>` / `--plugin <cmd>` (repeatable) / `--help` from
    /// an argument iterator. Unknown flags are ignored so the binary stays
    /// forgiving in a terminal.
    pub fn from_args<I: IntoIterator<Item = String>>(args: I) -> Self {
        let mut cfg = Config::default();
        let mut it = args.into_iter();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--agent" => cfg.agent_command = it.next(),
                "--plugin" => {
                    if let Some(p) = it.next() {
                        cfg.plugins.push(p);
                    }
                }
                _ => {}
            }
        }
        cfg
    }
}

/// The reference plugin binary names shipped in this crate's `src/bin/`,
/// auto-discovered next to the client when no `--plugin` is given. Each is a
/// distinct, offline, std-only demonstration of one extension surface
/// (footer / status / sidebar panel / slash command / overlay / toast).
pub const REFERENCE_PLUGINS: &[&str] = &[
    "rstui-acp-plugin-powerline", // powerline footer (agent · dir · vibe · clock)
    "rstui-acp-plugin-btw",       // private side-notes + live panel
    "rstui-acp-plugin-ask-user",  // structured ask-user overlay
    "rstui-acp-plugin-session",   // session stopwatch + turn/prompt counters
    "rstui-acp-plugin-git",       // git branch / dirty + changed-files panel
    "rstui-acp-plugin-history",   // automatic prompt-history panel
    "rstui-acp-plugin-pomodoro",  // focus countdown timer
    "rstui-acp-plugin-fortune",   // developer-fortune toast per turn
];

/// Resolves the plugin launch commands.
///
/// Explicit `--plugin` commands win. Otherwise the client auto-discovers the
/// reference plugins sitting next to its own executable (so a fresh
/// `cargo run`/install shows the powerline footer and slash commands with no
/// flags) — still deny-by-default in spirit: only *these vetted, in-tree*
/// binaries, and only when actually present on disk.
#[must_use]
pub fn resolve_plugins(config: &Config) -> Vec<String> {
    if !config.plugins.is_empty() {
        return config.plugins.clone();
    }
    let Ok(exe) = std::env::current_exe() else {
        return Vec::new();
    };
    let Some(dir) = exe.parent() else {
        return Vec::new();
    };
    REFERENCE_PLUGINS
        .iter()
        .filter_map(|name| {
            let mut path = dir.join(name);
            if cfg!(windows) {
                path.set_extension("exe");
            }
            path.exists().then(|| path.display().to_string())
        })
        .collect()
}

/// Installs a panic + signal hook that restores the terminal before any
/// message prints, then drives the full-screen client to exit.
///
/// Mirrors `rstui_crossterm::run_app`'s panic policy (a crash leaves the shell
/// clean *and* the panic readable), but over the async loop `run_app` does not
/// expose.
///
/// # Errors
///
/// Propagates terminal-setup, render, and input failures from the runtime.
pub async fn run(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    rstui_crossterm::install_signal_restore_hook();
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        rstui_crossterm::restore_terminal();
        default_hook(info);
    }));

    let backend: TerminalGuard<Stdout> = TerminalGuard::with_options(
        CrosstermBackend::new(io::stdout()),
        LifecycleOptions::default(),
    )?;
    let mut events = TerminalEvents::new();
    let mut app = ChatApp::new(config);
    // `RSTUI_KEYMAP=<map name|/path/to/keymap>` remaps the global commands
    // without a rebuild or the in-app panel — the same typo-safe seam as
    // `RSTUI_THEME` (see docs/keymaps.md). Headless `Harness` tests build
    // `ChatApp::new` directly, so they are unaffected.
    if let Ok(km) = std::env::var("RSTUI_KEYMAP") {
        app = app.with_keymap(&km);
    }

    rstui_runtime::run_async(app, backend, &mut events).await?;

    rstui_crossterm::restore_terminal();
    Ok(())
}
