//! The rstui [`App`]: the chat model, the `update` reducer, and the pure
//! `view`. No terminal, no tokio — every screen is reachable from a `Harness`
//! test (ADR 0011's determinism mandate: the reducer never `await`s).

use std::collections::BTreeMap;
use std::path::PathBuf;

use rstui_core::{Event, KeyCode, KeyEvent, KeyModifiers, Size, TextArea};
use rstui_runtime::{App, Cmd, Frame};

use crate::Config;
use crate::acp::{
    AcpEvent, DriverCmd, DriverHandle, PermissionChoice, PermissionOption, spawn_driver,
};
use crate::plugin::{FooterSegment, HostEvent, PluginAction, PluginEvent, PluginHost};
use crate::registry::Registry;
use crate::ui;

/// Which top-level screen is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// Choosing an agent from the registry.
    Picker,
    /// Spawning / initializing the agent.
    Connecting,
    /// Connected; the chat transcript + composer.
    Chat,
}

/// The author of a transcript entry, which drives its styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The local user.
    User,
    /// The agent's answer.
    Agent,
    /// The agent's "thinking" (rendered dim).
    Thought,
    /// A tool call the agent made.
    Tool,
    /// A plan / todo update.
    Plan,
    /// Client-generated system line.
    System,
}

/// One block in the scrolling transcript.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Who produced it.
    pub role: Role,
    /// Its text (may contain newlines; wrapped at render time).
    pub text: String,
    /// `true` while an agent turn is still appending to this entry.
    pub open: bool,
}

/// A permission request awaiting the user's choice.
#[derive(Debug, Clone)]
pub struct PendingPermission {
    id: u64,
    title: String,
    options: Vec<PermissionOption>,
    selected: usize,
}

/// An in-flight plugin ask-user overlay.
#[derive(Debug)]
pub struct AskState {
    plugin: String,
    id: u64,
    question: String,
    context: String,
    options: Vec<String>,
    allow_freeform: bool,
    selected: usize,
    freeform: TextArea,
    freeform_focused: bool,
}

/// A transient corner notification.
#[derive(Debug, Clone)]
pub struct Toast {
    /// Body text.
    pub text: String,
    /// Age in ticks (dropped past a threshold).
    pub age: usize,
}

/// Messages the reducer folds. Input is normalized to [`Msg::Key`] /
/// [`Msg::Resize`] in `on_event` so all focus routing lives in `update`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// One-shot boot: launch plugins + registry / agent.
    Boot,
    /// A key press.
    Key(KeyEvent),
    /// Terminal resized.
    Resize(Size),
    /// Mouse wheel (positive = up / back in history).
    Scroll(i32),
    /// Bracketed-paste text into the composer.
    Paste(String),
    /// The registry finished loading.
    RegistryLoaded(Box<Registry>),
    /// A driver event (re-arms the drain unless terminal).
    Acp(AcpEvent),
    /// A plugin action (re-arms the drain).
    Plugin(PluginEvent),
    /// Animation / housekeeping tick.
    Tick,
    /// Begin shutdown.
    Quit,
}

/// The full-screen ACP chat client application state.
pub struct ChatApp {
    config: Config,
    cwd: PathBuf,
    screen: Screen,
    registry: Registry,
    picker_selected: usize,
    transcript: Vec<Entry>,
    scroll: u16,
    follow: bool,
    composer: TextArea,
    status_line: String,
    agent_label: String,
    streaming: bool,
    spinner: usize,
    driver: Option<DriverHandle>,
    plugins: Option<PluginHost>,
    pending_permission: Option<PendingPermission>,
    ask: Option<AskState>,
    footers: BTreeMap<String, Vec<FooterSegment>>,
    statuses: BTreeMap<String, String>,
    commands: BTreeMap<String, (String, String)>,
    toasts: Vec<Toast>,
    log: Vec<String>,
    show_log: bool,
    show_help: bool,
    last_size: Size,
    quitting: bool,
}

impl ChatApp {
    /// Builds the app from resolved [`Config`]. The terminal is not yet taken
    /// over; all I/O is deferred to [`Msg::Boot`].
    #[must_use]
    pub fn new(config: Config) -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            config,
            cwd,
            screen: Screen::Picker,
            registry: Registry::default(),
            picker_selected: 0,
            transcript: Vec::new(),
            scroll: 0,
            follow: true,
            composer: TextArea::new(),
            status_line: "starting…".to_owned(),
            agent_label: String::new(),
            streaming: false,
            spinner: 0,
            driver: None,
            plugins: None,
            pending_permission: None,
            ask: None,
            footers: BTreeMap::new(),
            statuses: BTreeMap::new(),
            commands: BTreeMap::new(),
            toasts: Vec::new(),
            log: Vec::new(),
            show_log: false,
            show_help: false,
            last_size: Size::new(80, 24),
            quitting: false,
        }
    }

    // ---- read-only accessors (used by the view + Harness tests) ----

    /// The current screen.
    #[must_use]
    pub fn screen(&self) -> Screen {
        self.screen
    }
    /// The transcript entries.
    #[must_use]
    pub fn transcript(&self) -> &[Entry] {
        &self.transcript
    }
    /// The registry as loaded (possibly the offline fallback).
    #[must_use]
    pub fn registry(&self) -> &Registry {
        &self.registry
    }
    /// The picker's selected index.
    #[must_use]
    pub fn picker_selected(&self) -> usize {
        self.picker_selected
    }
    /// The connection status line.
    #[must_use]
    pub fn status_line(&self) -> &str {
        &self.status_line
    }
    /// The composer document.
    #[must_use]
    pub fn composer(&self) -> &TextArea {
        &self.composer
    }
    /// Whether a turn is streaming.
    #[must_use]
    pub fn is_streaming(&self) -> bool {
        self.streaming
    }
    /// The pending permission prompt, if any.
    #[must_use]
    pub fn pending_permission(&self) -> Option<&PendingPermission> {
        self.pending_permission.as_ref()
    }
    /// The ask-user overlay, if any.
    #[must_use]
    pub fn ask(&self) -> Option<&AskState> {
        self.ask.as_ref()
    }
    /// Whether the help overlay is open.
    #[must_use]
    pub fn help_visible(&self) -> bool {
        self.show_help
    }
    /// Whether the log overlay is open.
    #[must_use]
    pub fn log_visible(&self) -> bool {
        self.show_log
    }
    /// The merged plugin footer segments (in plugin-name order).
    #[must_use]
    pub fn footer_segments(&self) -> Vec<FooterSegment> {
        self.footers.values().flatten().cloned().collect()
    }
    /// Plugin status keys.
    #[must_use]
    pub fn statuses(&self) -> &BTreeMap<String, String> {
        &self.statuses
    }
    /// Registered slash commands: name → (plugin, description).
    #[must_use]
    pub fn commands(&self) -> &BTreeMap<String, (String, String)> {
        &self.commands
    }
    /// Active toasts.
    #[must_use]
    pub fn toasts(&self) -> &[Toast] {
        &self.toasts
    }
    /// The plugin diagnostic log.
    #[must_use]
    pub fn log(&self) -> &[String] {
        &self.log
    }
    /// The spinner frame index.
    #[must_use]
    pub fn spinner_frame(&self) -> usize {
        self.spinner
    }
    /// The transcript scroll offset (rows).
    #[must_use]
    pub fn scroll(&self) -> u16 {
        self.scroll
    }

    // ---- internal helpers ----

    fn push_system(&mut self, text: impl Into<String>) {
        self.transcript.push(Entry {
            role: Role::System,
            text: text.into(),
            open: false,
        });
        self.follow = true;
    }

    fn toast(&mut self, text: impl Into<String>) {
        self.toasts.push(Toast {
            text: text.into(),
            age: 0,
        });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    fn append_agent(&mut self, role: Role, chunk: &str) {
        if let Some(last) = self.transcript.last_mut() {
            if last.role == role && last.open {
                last.text.push_str(chunk);
                return;
            }
        }
        if let Some(last) = self.transcript.last_mut() {
            last.open = false;
        }
        self.transcript.push(Entry {
            role,
            text: chunk.to_owned(),
            open: true,
        });
        self.follow = true;
    }

    fn close_open_entry(&mut self) {
        if let Some(last) = self.transcript.last_mut() {
            last.open = false;
        }
    }

    fn acp_subscription(handle: DriverHandle) -> Cmd<Msg> {
        Cmd::perform(move || {
            let event = handle
                .recv_blocking()
                .unwrap_or_else(|| AcpEvent::Disconnected("driver ended".to_owned()));
            Msg::Acp(event)
        })
    }

    fn plugin_subscription(host: PluginHost) -> Cmd<Msg> {
        Cmd::perform(move || match host.recv_blocking() {
            Some(ev) => Msg::Plugin(ev),
            None => Msg::Tick,
        })
    }

    fn connect(&mut self, command: String) -> Cmd<Msg> {
        self.agent_label = command.clone();
        self.screen = Screen::Connecting;
        self.status_line = format!("connecting: {command}");
        let handle = spawn_driver(command.clone(), self.cwd.clone());
        self.driver = Some(handle.clone());
        if let Some(host) = &self.plugins {
            host.broadcast(&HostEvent::SessionStart { agent: command });
        }
        Self::acp_subscription(handle)
    }

    fn submit_composer(&mut self) -> Cmd<Msg> {
        let text = self.composer.lines().join("\n");
        let text = text.trim().to_owned();
        if text.is_empty() {
            return Cmd::none();
        }
        self.composer.clear();
        self.follow = true;

        if let Some(rest) = text.strip_prefix('/') {
            return self.run_slash(rest);
        }

        if self.driver.is_none() {
            self.push_system("not connected — pick an agent with /agents");
            return Cmd::none();
        }
        self.transcript.push(Entry {
            role: Role::User,
            text: text.clone(),
            open: false,
        });
        self.streaming = true;
        if let Some(driver) = &self.driver {
            driver.send(DriverCmd::Prompt(text.clone()));
        }
        if let Some(host) = &self.plugins {
            host.broadcast(&HostEvent::UserPrompt { text });
        }
        Cmd::none()
    }

    fn run_slash(&mut self, rest: &str) -> Cmd<Msg> {
        let mut parts = rest.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("").to_owned();
        let args = parts.next().unwrap_or("").trim().to_owned();
        match name.as_str() {
            "help" => {
                self.show_help = true;
                Cmd::none()
            }
            "agents" => {
                self.screen = Screen::Picker;
                Cmd::none()
            }
            "log" => {
                self.show_log = !self.show_log;
                Cmd::none()
            }
            "cancel" => {
                if let Some(driver) = &self.driver {
                    driver.send(DriverCmd::Cancel);
                    self.push_system("cancel requested");
                }
                Cmd::none()
            }
            "quit" | "exit" => self.begin_quit(),
            other => {
                if let Some((plugin, _)) = self.commands.get(other).cloned() {
                    if let Some(host) = &self.plugins {
                        host.send_to(
                            &plugin,
                            &HostEvent::Command {
                                name: other.to_owned(),
                                args,
                            },
                        );
                    }
                    self.push_system(format!("/{other} → {plugin}"));
                } else {
                    self.push_system(format!("unknown command: /{other} (try /help)"));
                }
                Cmd::none()
            }
        }
    }

    fn begin_quit(&mut self) -> Cmd<Msg> {
        self.quitting = true;
        if let Some(driver) = &self.driver {
            driver.send(DriverCmd::Shutdown);
        }
        if let Some(host) = &self.plugins {
            host.broadcast(&HostEvent::Shutdown);
        }
        Cmd::quit()
    }

    fn page_rows(&self) -> u16 {
        self.last_size.height.saturating_sub(8).max(1)
    }

    fn apply_plugin_action(&mut self, plugin: &str, action: PluginAction) {
        match action {
            PluginAction::RegisterCommand { name, description } => {
                self.commands.insert(name, (plugin.to_owned(), description));
            }
            PluginAction::SetStatus { key, value } => {
                if value.is_empty() {
                    self.statuses.remove(&key);
                } else {
                    self.statuses.insert(key, value);
                }
            }
            PluginAction::Footer { segments } => {
                self.footers.insert(plugin.to_owned(), segments);
            }
            PluginAction::AskUser {
                id,
                question,
                context,
                options,
                allow_freeform,
            } => {
                self.ask = Some(AskState {
                    plugin: plugin.to_owned(),
                    id,
                    question,
                    context,
                    options,
                    allow_freeform,
                    selected: 0,
                    freeform: TextArea::new(),
                    freeform_focused: false,
                });
            }
            PluginAction::Note { text } => {
                self.toast(format!("{plugin}: {text}"));
                self.push_system(format!("[{plugin}] {text}"));
            }
            PluginAction::Log { text } => {
                self.log.push(format!("[{plugin}] {text}"));
            }
        }
    }

    fn answer_ask(&mut self, cancelled: bool) {
        let Some(ask) = self.ask.take() else { return };
        let selections = if ask.options.is_empty() || cancelled {
            Vec::new()
        } else {
            vec![ask.options[ask.selected.min(ask.options.len() - 1)].clone()]
        };
        let text = if cancelled {
            String::new()
        } else {
            ask.freeform.lines().join("\n").trim().to_owned()
        };
        if let Some(host) = &self.plugins {
            host.send_to(
                &ask.plugin,
                &HostEvent::AskResponse {
                    id: ask.id,
                    selections,
                    text,
                    cancelled,
                },
            );
        }
    }

    /// Routes a key by the active overlay/screen. Returns the follow-up `Cmd`.
    fn on_key(&mut self, key: KeyEvent) -> Cmd<Msg> {
        if self.show_help {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::F(1)) {
                self.show_help = false;
            }
            return Cmd::none();
        }
        if self.show_log && key.code == KeyCode::Esc {
            self.show_log = false;
            return Cmd::none();
        }
        if self.pending_permission.is_some() {
            return self.permission_key(key);
        }
        if self.ask.is_some() {
            return self.ask_key(key);
        }
        match self.screen {
            Screen::Picker => self.picker_key(key),
            Screen::Connecting | Screen::Chat => self.chat_key(key),
        }
    }

    fn picker_key(&mut self, key: KeyEvent) -> Cmd<Msg> {
        let len = self.registry.agents.len();
        match key.code {
            KeyCode::Up => {
                self.picker_selected = self.picker_selected.saturating_sub(1);
                Cmd::none()
            }
            KeyCode::Down => {
                if len > 0 {
                    self.picker_selected = (self.picker_selected + 1).min(len - 1);
                }
                Cmd::none()
            }
            KeyCode::Enter => {
                if let Some(agent) = self.registry.agents.get(self.picker_selected) {
                    match agent.command.clone() {
                        Some(cmd) => return self.connect(cmd),
                        None => self.toast("no launch command for this platform"),
                    }
                }
                Cmd::none()
            }
            KeyCode::Char('q') | KeyCode::Esc => self.begin_quit(),
            KeyCode::F(1) => {
                self.show_help = true;
                Cmd::none()
            }
            _ => Cmd::none(),
        }
    }

    fn permission_key(&mut self, key: KeyEvent) -> Cmd<Msg> {
        let Some(perm) = self.pending_permission.as_mut() else {
            return Cmd::none();
        };
        match key.code {
            KeyCode::Up => {
                perm.selected = perm.selected.saturating_sub(1);
            }
            KeyCode::Down if !perm.options.is_empty() => {
                perm.selected = (perm.selected + 1).min(perm.options.len() - 1);
            }
            KeyCode::Enter => {
                let id = perm.id;
                let choice = perm
                    .options
                    .get(perm.selected)
                    .map(|o| PermissionChoice::Selected(o.option_id.clone()))
                    .unwrap_or(PermissionChoice::Cancelled);
                self.pending_permission = None;
                if let Some(driver) = &self.driver {
                    driver.send(DriverCmd::Permission { id, choice });
                }
            }
            KeyCode::Esc => {
                let id = perm.id;
                self.pending_permission = None;
                if let Some(driver) = &self.driver {
                    driver.send(DriverCmd::Permission {
                        id,
                        choice: PermissionChoice::Cancelled,
                    });
                }
            }
            _ => {}
        }
        Cmd::none()
    }

    fn ask_key(&mut self, key: KeyEvent) -> Cmd<Msg> {
        let Some(ask) = self.ask.as_mut() else {
            return Cmd::none();
        };
        if ask.freeform_focused {
            match key.code {
                KeyCode::Esc => ask.freeform_focused = false,
                KeyCode::Enter => {
                    self.answer_ask(false);
                }
                KeyCode::Backspace => {
                    ask.freeform.delete_backward();
                }
                KeyCode::Char(c) => ask.freeform.insert_char(c),
                _ => {}
            }
            return Cmd::none();
        }
        match key.code {
            KeyCode::Up => ask.selected = ask.selected.saturating_sub(1),
            KeyCode::Down if !ask.options.is_empty() => {
                ask.selected = (ask.selected + 1).min(ask.options.len() - 1);
            }
            KeyCode::Tab if ask.allow_freeform => ask.freeform_focused = true,
            KeyCode::Enter => self.answer_ask(false),
            KeyCode::Esc => self.answer_ask(true),
            _ => {}
        }
        Cmd::none()
    }

    fn chat_key(&mut self, key: KeyEvent) -> Cmd<Msg> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('c') if ctrl => return self.begin_quit(),
            KeyCode::Char('q') if ctrl => return self.begin_quit(),
            KeyCode::F(1) => {
                self.show_help = true;
            }
            KeyCode::F(10) => return self.begin_quit(),
            KeyCode::Esc if self.streaming => {
                if let Some(driver) = &self.driver {
                    driver.send(DriverCmd::Cancel);
                }
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.composer.insert_newline();
            }
            KeyCode::Enter => return self.submit_composer(),
            KeyCode::Backspace => {
                self.composer.delete_backward();
            }
            KeyCode::Delete => {
                self.composer.delete_forward();
            }
            KeyCode::Left => {
                self.composer.move_left();
            }
            KeyCode::Right => {
                self.composer.move_right();
            }
            KeyCode::Up => {
                self.composer.move_up();
            }
            KeyCode::Down => {
                self.composer.move_down();
            }
            KeyCode::Home => self.composer.move_home(),
            KeyCode::End => self.composer.move_end(),
            KeyCode::PageUp => {
                self.follow = false;
                self.scroll = self.scroll.saturating_sub(self.page_rows());
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(self.page_rows());
            }
            KeyCode::Char(c) => self.composer.insert_char(c),
            _ => {}
        }
        Cmd::none()
    }
}

impl App for ChatApp {
    type Message = Msg;

    fn init(&mut self) -> Cmd<Msg> {
        Cmd::message(Msg::Boot)
    }

    fn on_event(&self, event: Event) -> Option<Msg> {
        match event {
            Event::Key(key) if key.kind == rstui_core::KeyEventKind::Press => Some(Msg::Key(key)),
            Event::Resize(size) => Some(Msg::Resize(size)),
            Event::Paste(text) => Some(Msg::Paste(text)),
            Event::Mouse(m) => match m.kind {
                rstui_core::MouseEventKind::ScrollUp => Some(Msg::Scroll(3)),
                rstui_core::MouseEventKind::ScrollDown => Some(Msg::Scroll(-3)),
                _ => None,
            },
            _ => None,
        }
    }

    fn tick_rate(&self) -> Option<std::time::Duration> {
        if self.streaming || !self.toasts.is_empty() || self.plugins.is_some() {
            Some(std::time::Duration::from_millis(120))
        } else {
            None
        }
    }

    fn on_tick(&self) -> Option<Msg> {
        Some(Msg::Tick)
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        match message {
            Msg::Boot => {
                let mut cmds = Vec::new();
                let plugin_cmds = crate::resolve_plugins(&self.config);
                if !plugin_cmds.is_empty() {
                    let host = PluginHost::launch(&plugin_cmds, &self.cwd);
                    host.broadcast(&HostEvent::Init {
                        api_version: crate::plugin::API_VERSION.to_owned(),
                        client: "rstui-acp-client".to_owned(),
                        cwd: self.cwd.display().to_string(),
                    });
                    self.log
                        .push(format!("loaded plugins: {}", host.names().join(", ")));
                    cmds.push(Self::plugin_subscription(host.clone()));
                    self.plugins = Some(host);
                }
                match self.config.agent_command.clone() {
                    Some(cmd) => cmds.push(self.connect(cmd)),
                    None => {
                        self.status_line = "loading registry…".to_owned();
                        cmds.push(Cmd::perform(|| {
                            Msg::RegistryLoaded(Box::new(Registry::fetch_blocking()))
                        }));
                    }
                }
                Cmd::batch(cmds)
            }
            Msg::RegistryLoaded(reg) => {
                self.registry = *reg;
                self.status_line = if self.registry.offline {
                    format!(
                        "registry offline — {} built-in agents (Enter to launch)",
                        self.registry.agents.len()
                    )
                } else {
                    format!(
                        "{} agents from the ACP registry (↑↓ then Enter)",
                        self.registry.agents.len()
                    )
                };
                Cmd::none()
            }
            Msg::Key(key) => self.on_key(key),
            Msg::Paste(text) => {
                if self.screen == Screen::Chat || self.screen == Screen::Connecting {
                    self.composer.insert_str(&text);
                }
                Cmd::none()
            }
            Msg::Resize(size) => {
                self.last_size = size;
                Cmd::none()
            }
            Msg::Scroll(delta) => {
                if delta > 0 {
                    self.follow = false;
                    self.scroll = self.scroll.saturating_sub(delta as u16);
                } else {
                    self.scroll = self.scroll.saturating_add((-delta) as u16);
                }
                Cmd::none()
            }
            Msg::Acp(event) => {
                let terminal = matches!(event, AcpEvent::Disconnected(_));
                self.handle_acp(event);
                if terminal {
                    Cmd::none()
                } else if let Some(driver) = &self.driver {
                    Self::acp_subscription(driver.clone())
                } else {
                    Cmd::none()
                }
            }
            Msg::Plugin(ev) => {
                self.apply_plugin_action(&ev.plugin, ev.action);
                if let Some(host) = &self.plugins {
                    Self::plugin_subscription(host.clone())
                } else {
                    Cmd::none()
                }
            }
            Msg::Tick => {
                self.spinner = self.spinner.wrapping_add(1);
                for toast in &mut self.toasts {
                    toast.age += 1;
                }
                self.toasts.retain(|t| t.age < 40);
                if self.spinner % 8 == 0 {
                    if let Some(host) = &self.plugins {
                        host.broadcast(&HostEvent::Refresh);
                    }
                }
                Cmd::none()
            }
            Msg::Quit => self.begin_quit(),
        }
    }

    fn view(&self, frame: &mut Frame<'_>) {
        ui::render(self, frame);
    }
}

impl ChatApp {
    fn handle_acp(&mut self, event: AcpEvent) {
        match event {
            AcpEvent::Connected(info) => {
                self.screen = Screen::Chat;
                self.status_line = format!("connected · {}", short(&info));
                self.push_system(format!("connected to agent: {}", short(&info)));
            }
            AcpEvent::Status(s) => {
                if s == "session ready" {
                    self.screen = Screen::Chat;
                }
                self.status_line = s.clone();
                self.log.push(format!("status: {s}"));
            }
            AcpEvent::AgentText(t) => self.append_agent(Role::Agent, &t),
            AcpEvent::Thought(t) => self.append_agent(Role::Thought, &t),
            AcpEvent::ToolCall(t) => {
                self.close_open_entry();
                self.transcript.push(Entry {
                    role: Role::Tool,
                    text: t,
                    open: false,
                });
                self.follow = true;
            }
            AcpEvent::Plan(t) => {
                self.close_open_entry();
                self.transcript.push(Entry {
                    role: Role::Plan,
                    text: t,
                    open: false,
                });
                self.follow = true;
            }
            AcpEvent::TurnEnded(reason) => {
                self.close_open_entry();
                self.streaming = false;
                self.status_line = format!("turn ended: {reason}");
                if let Some(host) = &self.plugins {
                    host.broadcast(&HostEvent::TurnEnded {
                        stop_reason: reason,
                    });
                }
            }
            AcpEvent::Permission { id, title, options } => {
                self.pending_permission = Some(PendingPermission {
                    id,
                    title,
                    options,
                    selected: 0,
                });
            }
            AcpEvent::Stderr(line) => self.log.push(format!("agent: {line}")),
            AcpEvent::Error(e) => {
                self.streaming = false;
                self.push_system(format!("error: {e}"));
                self.toast(format!("error: {e}"));
            }
            AcpEvent::Disconnected(why) => {
                self.streaming = false;
                self.driver = None;
                self.screen = Screen::Picker;
                self.status_line = format!("disconnected: {why}");
                self.push_system(format!("disconnected: {why} — pick an agent to reconnect"));
            }
        }
    }
}

fn short(s: &str) -> String {
    let s = s.trim();
    if s.len() > 60 {
        format!("{}…", &s[..60])
    } else {
        s.to_owned()
    }
}

// --- view-facing accessors for the overlay state (kept here, near the model) ---

impl PendingPermission {
    /// The agent's request title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }
    /// The offered options.
    #[must_use]
    pub fn options(&self) -> &[PermissionOption] {
        &self.options
    }
    /// The highlighted option index.
    #[must_use]
    pub fn selected(&self) -> usize {
        self.selected
    }
}

impl AskState {
    /// The originating plugin.
    #[must_use]
    pub fn plugin(&self) -> &str {
        &self.plugin
    }
    /// The question text.
    #[must_use]
    pub fn question(&self) -> &str {
        &self.question
    }
    /// Optional context.
    #[must_use]
    pub fn context(&self) -> &str {
        &self.context
    }
    /// The selectable options.
    #[must_use]
    pub fn options(&self) -> &[String] {
        &self.options
    }
    /// The highlighted option.
    #[must_use]
    pub fn selected(&self) -> usize {
        self.selected
    }
    /// Whether freeform entry is allowed.
    #[must_use]
    pub fn allow_freeform(&self) -> bool {
        self.allow_freeform
    }
    /// Whether the freeform field has focus.
    #[must_use]
    pub fn freeform_focused(&self) -> bool {
        self.freeform_focused
    }
    /// The freeform document.
    #[must_use]
    pub fn freeform(&self) -> &TextArea {
        &self.freeform
    }
}
