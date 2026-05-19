//! Headless `Harness` integration tests for the `rstui-acp-client` chat app.
//!
//! These exercise the public, terminal-free surface (`ChatApp` as an
//! `rstui_runtime::App`, `Config`, and `Registry`) through `rstui`'s
//! deterministic `Harness`: no TTY, no tokio, no wall clock. They assert
//! structural facts — enum states, counts, a known agent name substring — and
//! deliberately avoid pinning exact layout text, so incidental wording or
//! spacing changes do not make them brittle.

use rstui_acp_client::Config;
use rstui_acp_client::app::{ChatApp, Msg, Screen};
use rstui_acp_client::registry::Registry;
use rstui_core::{Event, KeyCode, KeyEvent, KeyModifiers, Size};
use rstui_runtime::Harness;

/// Builds a harness over a freshly constructed `ChatApp`. `Harness::new` runs
/// `App::init`, which returns `Cmd::message(Msg::Boot)`. With the default
/// `Config` (no `--agent`) `Boot` schedules a blocking registry fetch that
/// shells out to `curl`; under the harness's inline executor that runs
/// synchronously here, so the result is environment dependent (it collapses to
/// the offline fallback when offline). We therefore assert only that nothing
/// panicked and that *something* was rendered.
fn booted(width: u16, height: u16) -> Harness<ChatApp> {
    Harness::new(ChatApp::new(Config::default()), width, height)
}

#[test]
fn boot_does_not_panic_and_renders_a_non_empty_frame() {
    let mut harness = booted(100, 30);
    // `init` already drove `Msg::Boot` and settled it. Driving it again must
    // still be inert/safe (the reducer never `await`s; ADR 0011).
    harness.message(Msg::Boot);

    assert!(harness.is_running(), "boot must not quit the app");
    let snapshot = harness.snapshot();
    assert!(
        !snapshot.trim().is_empty(),
        "a booted app must render a non-empty frame"
    );
    // The picker is the initial screen; a registry fetch may or may not have
    // resolved synchronously, but the app must still be on a valid screen.
    assert!(matches!(
        harness.app().screen(),
        Screen::Picker | Screen::Connecting | Screen::Chat
    ));
}

#[test]
fn registry_loaded_populates_the_picker_with_known_agents() {
    let mut harness = booted(100, 30);
    harness.message(Msg::RegistryLoaded(Box::new(Registry::offline_fallback())));

    assert_eq!(
        harness.app().screen(),
        Screen::Picker,
        "loading the registry keeps the user on the picker"
    );
    let agents = &harness.app().registry().agents;
    assert!(
        !agents.is_empty(),
        "the offline fallback ships built-in agents"
    );
    assert!(
        harness.app().registry().offline,
        "the offline fallback flags itself offline so the UI can say so"
    );
    assert!(
        agents.iter().any(|a| a.name.contains("Claude Code")),
        "the built-in catalogue includes Claude Code; got {:?}",
        agents.iter().map(|a| &a.name).collect::<Vec<_>>()
    );

    // The rendered frame should surface a known agent name somewhere.
    let snapshot = harness.snapshot();
    assert!(
        !snapshot.trim().is_empty(),
        "the picker renders a non-empty frame once agents are loaded"
    );
}

#[test]
fn picker_navigation_advances_and_clamps_the_selection() {
    let mut harness = booted(100, 30);
    harness.message(Msg::RegistryLoaded(Box::new(Registry::offline_fallback())));
    let count = harness.app().registry().agents.len();
    assert!(count >= 2, "need >=2 agents to test navigation movement");

    assert_eq!(harness.app().picker_selected(), 0, "selection starts at 0");

    // Down advances by one.
    harness.message(Msg::Key(KeyEvent::from_code(KeyCode::Down)));
    assert_eq!(
        harness.app().picker_selected(),
        1,
        "Down advances the picker selection"
    );

    // Up returns to 0, and a further Up clamps (saturating) at 0.
    harness.message(Msg::Key(KeyEvent::from_code(KeyCode::Up)));
    assert_eq!(harness.app().picker_selected(), 0, "Up moves back up");
    harness.message(Msg::Key(KeyEvent::from_code(KeyCode::Up)));
    assert_eq!(
        harness.app().picker_selected(),
        0,
        "Up clamps at the first agent"
    );

    // Pressing Down past the end clamps at the last index.
    for _ in 0..(count + 5) {
        harness.message(Msg::Key(KeyEvent::from_code(KeyCode::Down)));
    }
    assert_eq!(
        harness.app().picker_selected(),
        count - 1,
        "Down clamps at the last agent"
    );
    assert!(harness.is_running(), "navigation must not quit");
}

#[test]
fn typing_in_the_picker_does_not_reach_the_composer_and_does_not_crash() {
    // In the picker, character keys are not composer input. This asserts the
    // robust structural fact: typing is safe and leaves the composer empty
    // (there is no public way to force the Chat screen without a live agent).
    let mut harness = booted(100, 30);
    harness.message(Msg::RegistryLoaded(Box::new(Registry::offline_fallback())));
    assert_eq!(harness.app().screen(), Screen::Picker);

    for c in "hello world".chars() {
        harness.message(Msg::Key(KeyEvent::char(c)));
    }
    harness.message(Msg::Paste("pasted text".to_owned()));

    assert!(harness.is_running(), "typing in the picker must not quit");
    let lines = harness.app().composer().lines();
    let joined = lines.join("\n");
    assert!(
        joined.is_empty(),
        "picker keystrokes/paste must not leak into the composer; got {joined:?}"
    );
    assert_eq!(
        harness.app().screen(),
        Screen::Picker,
        "typing in the picker keeps the picker screen"
    );
}

#[test]
fn config_from_args_parses_agent_and_repeated_plugins() {
    let args = ["--agent", "X", "--plugin", "p1", "--plugin", "p2"]
        .into_iter()
        .map(String::from);
    let cfg = Config::from_args(args);

    assert_eq!(cfg.agent_command.as_deref(), Some("X"));
    assert_eq!(cfg.plugins, vec!["p1".to_owned(), "p2".to_owned()]);

    // Unknown flags are ignored; an empty iterator yields the default.
    let empty = Config::from_args(std::iter::empty::<String>());
    assert_eq!(empty.agent_command, None);
    assert!(empty.plugins.is_empty());
}

/// `--agent`, `--cmd` and `--command` are synonyms for the custom
/// local-stdio ACP command, and the `RSTUI_ACP_AGENT` env fallback (folded
/// in by `with_agent_env`, passed explicitly so the test never touches the
/// process env) only applies when no switch was given.
#[test]
fn custom_command_switch_synonyms_and_env_precedence() {
    for flag in ["--agent", "--cmd", "--command"] {
        let cfg = Config::from_args([flag, "my-acp --stdio"].into_iter().map(String::from));
        assert_eq!(
            cfg.agent_command.as_deref(),
            Some("my-acp --stdio"),
            "{flag} sets the custom command"
        );
        // An explicit switch wins over the env var.
        let cfg = cfg.with_agent_env(Some("ENV_CMD".to_owned()));
        assert_eq!(cfg.agent_command.as_deref(), Some("my-acp --stdio"));
    }

    // No switch → the env var supplies the command…
    let cfg =
        Config::from_args(std::iter::empty::<String>()).with_agent_env(Some("envd-acp".to_owned()));
    assert_eq!(cfg.agent_command.as_deref(), Some("envd-acp"));

    // …but a blank/whitespace env var is ignored (still the picker).
    let cfg =
        Config::from_args(std::iter::empty::<String>()).with_agent_env(Some("   ".to_owned()));
    assert_eq!(cfg.agent_command, None);
    let cfg = Config::from_args(std::iter::empty::<String>()).with_agent_env(None);
    assert_eq!(cfg.agent_command, None);
}

#[test]
fn registry_parse_resolves_an_npx_agent_command() {
    let json = r#"{
        "agents": [
            {
                "id": "demo",
                "name": "Demo Agent",
                "description": "a test agent",
                "distribution": { "npx": { "package": "@scope/demo-acp", "args": ["--acp"] } }
            }
        ]
    }"#;

    let registry = Registry::parse(json).expect("valid registry JSON must parse");
    assert!(!registry.offline, "a parsed registry is not the fallback");
    assert_eq!(registry.agents.len(), 1);

    let agent = &registry.agents[0];
    assert_eq!(agent.id, "demo");
    assert_eq!(agent.name, "Demo Agent");
    let command = agent
        .command
        .as_deref()
        .expect("an npx distribution resolves to a launch command");
    assert!(
        command.starts_with("npx -y "),
        "npx packages become `npx -y <package> …`; got {command:?}"
    );
    assert!(
        command.contains("@scope/demo-acp"),
        "the resolved command includes the npx package; got {command:?}"
    );

    // Malformed JSON surfaces an error rather than panicking.
    assert!(Registry::parse("{ not json").is_err());
}

#[test]
fn help_overlay_toggles_with_f1_and_esc_in_the_picker() {
    let mut harness = booted(100, 30);
    harness.message(Msg::RegistryLoaded(Box::new(Registry::offline_fallback())));
    assert_eq!(harness.app().screen(), Screen::Picker);
    assert!(!harness.app().help_visible(), "help starts hidden");

    harness.message(Msg::Key(KeyEvent::from_code(KeyCode::F(1))));
    assert!(harness.app().help_visible(), "F1 opens the help overlay");

    harness.message(Msg::Key(KeyEvent::from_code(KeyCode::Esc)));
    assert!(!harness.app().help_visible(), "Esc closes the help overlay");
    // Closing help with Esc must not have quit the app (Esc only quits the
    // picker when no overlay is open).
    assert!(harness.is_running());
    assert_eq!(harness.app().screen(), Screen::Picker);
}

#[test]
fn tick_after_boot_is_safe_and_preserves_the_surface_size() {
    let mut harness = booted(80, 24);
    harness.message(Msg::RegistryLoaded(Box::new(Registry::offline_fallback())));

    let before = harness.snapshot();
    let before_rows = before.lines().count();

    // An explicit tick is one elapsed period: it advances the spinner and ages
    // toasts, and must not panic or resize the surface.
    let spinner_before = harness.app().spinner_frame();
    harness.message(Msg::Tick);
    harness.tick();

    assert!(harness.is_running(), "ticking must not quit the app");
    let after = harness.snapshot();
    assert_eq!(
        after.lines().count(),
        before_rows,
        "ticking must not change the rendered surface height"
    );
    // The spinner frame is housekeeping state; it must have advanced (wrapping)
    // across the two tick messages, never panicking.
    assert_ne!(
        harness.app().spinner_frame(),
        spinner_before,
        "Msg::Tick advances the spinner frame"
    );
}

#[test]
fn resize_to_a_tiny_surface_does_not_panic() {
    let mut harness = booted(100, 30);
    harness.message(Msg::RegistryLoaded(Box::new(Registry::offline_fallback())));

    // Both the harness resize path (delivers Event::Resize) and an explicit
    // Msg::Resize must be safe at extreme small sizes.
    harness.resize(5, 5);
    let small = harness.snapshot();
    assert_eq!(
        small.lines().count(),
        5,
        "a 5x5 surface renders exactly 5 rows"
    );

    harness.message(Msg::Resize(Size::new(1, 1)));
    harness.resize(1, 1);
    assert!(
        harness.is_running(),
        "extreme resizes must not crash or quit the app"
    );
    assert!(
        !harness.snapshot().is_empty(),
        "even a 1x1 surface renders something"
    );
}

#[test]
fn scroll_messages_adjust_the_offset_without_panicking() {
    let mut harness = booted(100, 30);
    harness.message(Msg::RegistryLoaded(Box::new(Registry::offline_fallback())));

    assert_eq!(harness.app().scroll(), 0, "scroll starts at the bottom (0)");

    // Negative delta scrolls down (saturating add); positive scrolls back up
    // (saturating sub). From 0 a positive delta saturates to 0.
    harness.message(Msg::Scroll(5));
    assert_eq!(
        harness.app().scroll(),
        0,
        "scrolling back from the bottom saturates at 0"
    );

    harness.message(Msg::Scroll(-7));
    assert_eq!(
        harness.app().scroll(),
        7,
        "a negative delta scrolls the transcript down"
    );

    harness.message(Msg::Scroll(3));
    assert_eq!(
        harness.app().scroll(),
        4,
        "a positive delta scrolls back up by that many rows"
    );
    assert!(harness.is_running(), "scrolling must not quit the app");
}

#[test]
fn event_routed_keys_reach_the_reducer_through_on_event() {
    // `Harness::handle` goes through `App::on_event`, the real input path.
    // A key-press event must normalize to `Msg::Key` and drive picker nav.
    let mut harness = booted(100, 30);
    harness.message(Msg::RegistryLoaded(Box::new(Registry::offline_fallback())));
    assert!(harness.app().registry().agents.len() >= 2);
    assert_eq!(harness.app().picker_selected(), 0);

    harness.handle(Event::from(KeyEvent::from_code(KeyCode::Down)));
    assert_eq!(
        harness.app().picker_selected(),
        1,
        "a Down key event routed via on_event advances the picker"
    );

    harness.handle(Event::from(KeyEvent::from_code(KeyCode::F(1))));
    assert!(
        harness.app().help_visible(),
        "an F1 key event routed via on_event opens help"
    );
    assert!(harness.is_running());
}

// ---- Iteration 1: slash commands + autocomplete ----

use rstui_acp_client::acp::AcpEvent;
use rstui_acp_client::app::CommandSource;

/// Drives the app into the connected chat screen with no tokio/agent:
/// `Status("session ready")` flips the screen to `Chat` in the reducer.
fn chatting(width: u16, height: u16) -> Harness<ChatApp> {
    let mut h = booted(width, height);
    h.message(Msg::Acp(AcpEvent::Status("session ready".to_owned())));
    assert_eq!(h.app().screen(), Screen::Chat, "session ready ⇒ Chat");
    h
}

fn typ(h: &mut Harness<ChatApp>, s: &str) {
    for c in s.chars() {
        h.message(Msg::Key(KeyEvent::from_code(KeyCode::Char(c))));
    }
}

#[test]
fn typing_slash_opens_the_autocomplete_with_builtins() {
    let mut h = chatting(100, 30);
    assert!(h.app().completion().is_none(), "no popup before '/'");
    typ(&mut h, "/");
    let comp = h.app().completion().expect("'/' opens the popup");
    assert!(!comp.items.is_empty());
    assert!(
        comp.items.iter().any(|c| c.name == "help"),
        "built-in /help is offered"
    );
    // Popup is drawn.
    assert!(h.snapshot().contains("/help"));
}

#[test]
fn autocomplete_filters_as_you_type_and_navigates_and_wraps() {
    let mut h = chatting(100, 30);
    typ(&mut h, "/he");
    let comp = h.app().completion().expect("popup visible");
    assert!(
        comp.items
            .iter()
            .all(|c| c.name.contains("he") || c.description.to_ascii_lowercase().contains("he")),
        "every candidate matches the query"
    );
    assert!(comp.items.iter().any(|c| c.name == "help"));
    let first = h.app().completion().unwrap().selected;
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::Down)));
    let second = h.app().completion().unwrap().selected;
    assert!(second != first || h.app().completion().unwrap().items.len() == 1);
    // Up from index 0 wraps to the last item.
    let mut h2 = chatting(100, 30);
    typ(&mut h2, "/");
    let n = h2.app().completion().unwrap().items.len();
    h2.message(Msg::Key(KeyEvent::from_code(KeyCode::Up)));
    assert_eq!(h2.app().completion().unwrap().selected, n - 1, "Up wraps");
}

#[test]
fn tab_completes_to_command_with_trailing_space_and_closes_popup() {
    let mut h = chatting(100, 30);
    typ(&mut h, "/hel");
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::Tab)));
    assert!(
        h.app().completion().is_none(),
        "Tab accepts and the popup closes (a space now follows the command)"
    );
    assert_eq!(h.app().composer().lines(), &["/help ".to_owned()]);
}

#[test]
fn enter_on_a_selection_runs_the_command() {
    let mut h = chatting(100, 30);
    typ(&mut h, "/help");
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::Enter)));
    assert!(h.app().help_visible(), "Enter ran /help");
    assert!(h.app().completion().is_none());
    assert!(h.app().composer().is_empty(), "composer cleared after run");
}

#[test]
fn escape_hides_the_autocomplete_without_running() {
    let mut h = chatting(100, 30);
    typ(&mut h, "/quit");
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::Esc)));
    assert!(h.app().completion().is_none(), "Esc hides the popup");
    assert!(h.is_running(), "Esc must NOT have run /quit");
}

#[test]
fn builtin_clear_command_empties_the_transcript() {
    let mut h = chatting(100, 30);
    h.message(Msg::Acp(AcpEvent::AgentText("hello there".to_owned())));
    assert!(!h.app().transcript().is_empty());
    typ(&mut h, "/clear");
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::Enter)));
    // Only the "transcript cleared" system line remains.
    assert!(
        h.app().transcript().len() <= 1,
        "/clear empties the transcript"
    );
}

#[test]
fn agent_advertised_commands_merge_into_the_command_set() {
    let mut h = chatting(100, 30);
    h.message(Msg::Acp(AcpEvent::AvailableCommands(vec![(
        "deploy".to_owned(),
        "Ship to prod".to_owned(),
    )])));
    let specs = h.app().command_specs();
    let deploy = specs
        .iter()
        .find(|c| c.name == "deploy")
        .expect("agent command merged into command_specs");
    assert_eq!(deploy.source, CommandSource::Agent);
    // And it shows up in the autocomplete when its prefix is typed.
    typ(&mut h, "/dep");
    let comp = h.app().completion().expect("popup");
    assert!(comp.items.iter().any(|c| c.name == "deploy"));
}

// ---- Iteration 2: todos panel (ACP plan) ----

use rstui_acp_client::acp::{TodoEntry, TodoStatus};

fn todo(content: &str, status: TodoStatus) -> TodoEntry {
    TodoEntry {
        content: content.to_owned(),
        status,
        priority: "medium".to_owned(),
    }
}

#[test]
fn acp_plan_populates_todos_and_progress() {
    let mut h = chatting(120, 36);
    h.message(Msg::Acp(AcpEvent::Plan(vec![
        todo("scaffold", TodoStatus::Completed),
        todo("wire transport", TodoStatus::InProgress),
        todo("write tests", TodoStatus::Pending),
    ])));
    assert_eq!(h.app().todos().len(), 3);
    assert_eq!(h.app().todo_progress(), (1, 3));
    // The sidebar auto-shows while work is open, and the panel is drawn.
    assert!(h.app().sidebar_visible());
    let snap = h.snapshot();
    assert!(snap.contains("Todos"));
    assert!(snap.contains("wire transport"));
}

#[test]
fn acp_plan_replaces_the_whole_list_each_update() {
    let mut h = chatting(120, 36);
    h.message(Msg::Acp(AcpEvent::Plan(vec![todo(
        "a",
        TodoStatus::Pending,
    )])));
    h.message(Msg::Acp(AcpEvent::Plan(vec![
        todo("x", TodoStatus::InProgress),
        todo("y", TodoStatus::Pending),
    ])));
    assert_eq!(
        h.app().todos().len(),
        2,
        "newest plan replaces, not appends"
    );
    assert_eq!(h.app().todos()[0].content, "x");
}

#[test]
fn sidebar_auto_hides_once_every_todo_is_completed() {
    let mut h = chatting(120, 36);
    h.message(Msg::Acp(AcpEvent::Plan(vec![
        todo("done a", TodoStatus::Completed),
        todo("done b", TodoStatus::Completed),
    ])));
    assert!(
        !h.app().sidebar_visible(),
        "all-completed ⇒ Auto sidebar hides (opencode parity)"
    );
}

#[test]
fn slash_todos_toggles_the_sidebar() {
    let mut h = chatting(120, 36);
    h.message(Msg::Acp(AcpEvent::Plan(vec![todo(
        "t",
        TodoStatus::Pending,
    )])));
    assert!(h.app().sidebar_visible(), "auto-visible with open work");
    typ(&mut h, "/todos");
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::Enter)));
    assert!(!h.app().sidebar_visible(), "/todos hides it");
    typ(&mut h, "/todos");
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::Enter)));
    assert!(h.app().sidebar_visible(), "/todos again brings it back");
}

// ---- Iteration 3: rich, customizable tool calls ----

use rstui_acp_client::acp::{ToolBody, ToolCallInfo, ToolCallPatch, ToolKind, ToolStatus};

fn tool(id: &str, title: &str, kind: ToolKind, status: ToolStatus) -> ToolCallInfo {
    ToolCallInfo {
        id: id.to_owned(),
        title: title.to_owned(),
        kind,
        status,
        input: "path=src/main.rs".to_owned(),
        body: vec![ToolBody::Text("line one\nline two".to_owned())],
    }
}

#[test]
fn tool_call_registers_and_renders_a_card() {
    let mut h = chatting(120, 40);
    h.message(Msg::Acp(AcpEvent::ToolCall(tool(
        "t1",
        "Read main.rs",
        ToolKind::Read,
        ToolStatus::InProgress,
    ))));
    assert_eq!(h.app().tool_calls().len(), 1);
    assert!(h.app().tool_call("t1").is_some());
    let snap = h.snapshot();
    assert!(snap.contains("Read main.rs"), "tool title is shown");
    assert!(snap.contains("running"), "in-progress status label shown");
}

#[test]
fn tool_call_update_merges_and_keeps_one_card() {
    let mut h = chatting(120, 40);
    h.message(Msg::Acp(AcpEvent::ToolCall(tool(
        "t1",
        "Edit file",
        ToolKind::Edit,
        ToolStatus::Pending,
    ))));
    h.message(Msg::Acp(AcpEvent::ToolCallUpdate(ToolCallPatch {
        id: "t1".to_owned(),
        title: None,
        kind: None,
        status: Some(ToolStatus::Completed),
        input: None,
        body: Some(vec![ToolBody::Diff {
            path: "a.rs".to_owned(),
            text: "-old\n+new".to_owned(),
        }]),
    })));
    assert_eq!(
        h.app().tool_calls().len(),
        1,
        "an update merges, it does not add a second card"
    );
    assert_eq!(
        h.app().tool_call("t1").unwrap().status,
        ToolStatus::Completed
    );
}

#[test]
fn details_toggle_collapses_completed_tool_output() {
    let mut h = chatting(120, 40);
    h.message(Msg::Acp(AcpEvent::ToolCall(tool(
        "t1",
        "Read file",
        ToolKind::Read,
        ToolStatus::Completed,
    ))));
    assert!(h.app().details(), "details on by default");
    assert!(
        h.snapshot().contains("line one"),
        "completed tool body shown when details on"
    );
    typ(&mut h, "/details");
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::Enter)));
    assert!(!h.app().details());
    let snap = h.snapshot();
    assert!(
        snap.contains("output hidden") && !snap.contains("line one"),
        "completed tool body collapses when details off (opencode rule)"
    );
}

#[test]
fn failed_tool_keeps_its_body_even_with_details_off() {
    let mut h = chatting(120, 40);
    typ(&mut h, "/details");
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::Enter)));
    assert!(!h.app().details());
    h.message(Msg::Acp(AcpEvent::ToolCall(tool(
        "bad",
        "Run build",
        ToolKind::Execute,
        ToolStatus::Failed,
    ))));
    let snap = h.snapshot();
    assert!(
        snap.contains("failed") && snap.contains("line one"),
        "a failed tool always expands (errors are never collapsed)"
    );
}

// ---- Iteration 4: plugins in the TUI ----

use rstui_acp_client::plugin::{PluginAction, PluginEvent};

fn plug(plugin: &str, action: PluginAction) -> Msg {
    Msg::Plugin(PluginEvent {
        plugin: plugin.to_owned(),
        action,
    })
}

#[test]
fn plugin_status_and_panel_surface_in_the_sidebar() {
    let mut h = chatting(120, 40);
    assert!(!h.app().sidebar_visible(), "nothing to show yet");
    h.message(plug(
        "powerline",
        PluginAction::SetStatus {
            key: "git".to_owned(),
            value: "main ✚2".to_owned(),
        },
    ));
    h.message(plug(
        "btw",
        PluginAction::Panel {
            title: "BTW notes".to_owned(),
            body: vec!["[09:14] ship the thing".to_owned()],
        },
    ));
    assert!(
        h.app().sidebar_visible(),
        "a plugin surface auto-shows the sidebar (no todos needed)"
    );
    assert_eq!(
        h.app().statuses().get("git").map(String::as_str),
        Some("main ✚2")
    );
    assert!(h.app().panels().contains_key("btw"));
    let snap = h.snapshot();
    assert!(snap.contains("BTW notes"), "plugin panel title rendered");
    assert!(
        snap.contains("ship the thing"),
        "plugin panel body rendered"
    );
    assert!(snap.contains("git"), "status key rendered in sidebar");
}

#[test]
fn empty_panel_body_removes_the_panel() {
    let mut h = chatting(120, 40);
    h.message(plug(
        "btw",
        PluginAction::Panel {
            title: "BTW notes".to_owned(),
            body: vec!["one".to_owned()],
        },
    ));
    assert!(h.app().panels().contains_key("btw"));
    h.message(plug(
        "btw",
        PluginAction::Panel {
            title: "BTW notes".to_owned(),
            body: vec![],
        },
    ));
    assert!(
        !h.app().panels().contains_key("btw"),
        "an empty body clears the panel"
    );
}

#[test]
fn plugins_overlay_toggles_and_lists_plugin_commands() {
    let mut h = chatting(120, 40);
    h.message(plug(
        "ask-user",
        PluginAction::RegisterCommand {
            name: "ask".to_owned(),
            description: "structured ask".to_owned(),
        },
    ));
    typ(&mut h, "/plugins");
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::Enter)));
    assert!(h.app().plugins_overlay(), "/plugins opens the overlay");
    let snap = h.snapshot();
    assert!(snap.contains("Plugins"), "overlay titled Plugins");
    assert!(snap.contains("/ask"), "registered plugin command listed");
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::Esc)));
    assert!(!h.app().plugins_overlay(), "Esc closes the overlay");
}

#[test]
fn plugin_commands_route_and_appear_in_autocomplete() {
    let mut h = chatting(120, 40);
    h.message(plug(
        "btw",
        PluginAction::RegisterCommand {
            name: "btw".to_owned(),
            description: "side note".to_owned(),
        },
    ));
    typ(&mut h, "/bt");
    let comp = h.app().completion().expect("popup");
    let btw = comp
        .items
        .iter()
        .find(|c| c.name == "btw")
        .expect("plugin command in autocomplete");
    assert!(matches!(
        btw.source,
        rstui_acp_client::app::CommandSource::Plugin(_)
    ));
}

#[test]
fn agent_markdown_links_render_with_styling() {
    let mut h = chatting(100, 40);
    // Inject an agent response with markdown link syntax
    h.message(Msg::Acp(AcpEvent::AgentText(
        "Check this out: [example](https://example.com)".to_owned(),
    )));
    let snap = h.snapshot();
    // The snapshot should contain the link text (the Markdown widget renders just the link text)
    assert!(
        snap.contains("example"),
        "link text should appear in output"
    );
    // The agent message should be present in the transcript
    let trans = h.app().transcript();
    assert!(
        !trans.is_empty(),
        "transcript should have the agent message"
    );
    let agent_msg = trans
        .iter()
        .find(|e| e.role == rstui_acp_client::app::Role::Agent)
        .expect("should have an agent message");
    assert!(
        agent_msg.text.contains("[example](https://example.com)"),
        "agent message should contain markdown link syntax"
    );
}

// ---- Plugin capabilities: keybindings + modals (opencode/pi parity) ----

#[test]
fn plugin_keybinding_registers_and_fires_its_command() {
    let mut h = chatting(100, 40);
    h.message(plug(
        "fortune",
        PluginAction::RegisterKeybinding {
            keys: "Ctrl+Y".to_owned(),
            command: "fortune".to_owned(),
            description: "Draw a fortune".to_owned(),
        },
    ));
    // Stored under the canonical chord, regardless of input casing.
    let kb = h.app().keybindings();
    let (plugin, command, _) = kb.get("ctrl+y").expect("chord registered");
    assert_eq!((plugin.as_str(), command.as_str()), ("fortune", "fortune"));

    // Pressing the chord routes the command (system breadcrumb is observable
    // headlessly; the plugin send is a no-op without a live host).
    h.message(Msg::Key(KeyEvent::new(
        KeyCode::Char('y'),
        KeyModifiers::CONTROL,
    )));
    let last = &h.app().transcript().last().expect("a system line").text;
    assert!(
        last.contains("⌨") && last.contains("/fortune"),
        "the chord fired its command: {last:?}"
    );
}

#[test]
fn bare_letters_are_never_stolen_as_shortcuts() {
    let mut h = chatting(100, 40);
    h.message(plug(
        "fortune",
        PluginAction::RegisterKeybinding {
            keys: "y".to_owned(), // no modifier ⇒ not a shortcut
            command: "fortune".to_owned(),
            description: "x".to_owned(),
        },
    ));
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::Char('y'))));
    // 'y' typed into the composer, not consumed as a shortcut.
    assert_eq!(h.app().composer().lines(), &["y".to_owned()]);
}

#[test]
fn plugin_modal_opens_navigates_and_answers() {
    let mut h = chatting(100, 40);
    h.message(plug(
        "session",
        PluginAction::Modal {
            id: 7,
            title: "Session".to_owned(),
            body: vec!["elapsed 00:10".to_owned()],
            buttons: vec!["Reset".to_owned(), "Close".to_owned()],
        },
    ));
    let m = h.app().modal().expect("modal open");
    assert_eq!(m.title(), "Session");
    assert_eq!(m.buttons(), ["Reset".to_owned(), "Close".to_owned()]);
    assert_eq!(m.selected(), 0);
    assert!(h.snapshot().contains("Session"));

    // Right moves to "Close"; Enter answers and dismisses.
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::Right)));
    assert_eq!(h.app().modal().unwrap().selected(), 1);
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::Enter)));
    assert!(h.app().modal().is_none(), "Enter closes the modal");
}

#[test]
fn plugin_modal_escape_cancels() {
    let mut h = chatting(100, 40);
    h.message(plug(
        "session",
        PluginAction::Modal {
            id: 1,
            title: "Confirm".to_owned(),
            body: vec![],
            buttons: vec!["OK".to_owned()],
        },
    ));
    assert!(h.app().modal().is_some());
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::Esc)));
    assert!(h.app().modal().is_none(), "Esc cancels the modal");
    assert!(h.is_running());
}

#[test]
fn keymap_panel_opens_with_ctrl_k_navigates_rebinds_and_closes() {
    let mut h = booted(100, 30);
    assert!(!h.app().keymap_panel_open(), "panel starts closed");

    // Ctrl+K is the global Action::Drawer binding — resolved through the
    // keymap on any screen, after the plugin-chord layer.
    h.message(Msg::Key(KeyEvent::new(
        KeyCode::Char('k'),
        KeyModifiers::CONTROL,
    )));
    assert!(h.app().keymap_panel_open(), "Ctrl+K opens the keymap panel");
    let s = h.snapshot();
    assert!(
        s.contains("Keymap") && s.contains("Quit"),
        "the shared KeymapView widget renders the live bindings:\n{s}"
    );

    // Navigate and arm a capture; the panel owns these keys (they do not
    // leak to the picker underneath).
    h.message(Msg::Key(KeyEvent::char('j')));
    h.message(Msg::Key(KeyEvent::char('r')));
    assert!(
        h.snapshot().contains("press a key"),
        "the row is armed for capture:\n{}",
        h.snapshot()
    );
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::F(5))));
    assert!(h.is_running(), "rebinding must not quit");

    h.message(Msg::Key(KeyEvent::from_code(KeyCode::Esc)));
    assert!(!h.app().keymap_panel_open(), "Esc closes the panel");
    assert!(h.is_running(), "closing the panel must not quit");
}

#[test]
fn help_then_k_is_the_universal_gateway_into_the_keymap_editor() {
    let mut h = booted(100, 30);
    h.message(Msg::RegistryLoaded(Box::new(Registry::offline_fallback())));
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::F(1))));
    assert!(h.app().help_visible(), "F1 opens help");
    assert!(
        h.snapshot().contains("customise these keybindings"),
        "help advertises the k gateway:\n{}",
        h.snapshot()
    );
    // `k` from help turns the cheat-sheet into the keymap editor.
    h.message(Msg::Key(KeyEvent::char('k')));
    assert!(!h.app().help_visible(), "k closes help");
    assert!(
        h.app().keymap_panel_open(),
        "help → k opened the keymap editor"
    );
    assert!(
        h.snapshot().contains("Keymap") && h.snapshot().contains("Quit"),
        "the KeymapView renders:\n{}",
        h.snapshot()
    );
    assert!(h.is_running());
}

// ── ADR 0017 end-to-end: the client sends the catalog, and an
// agent-authored A2UI / json-render document renders in the transcript
// through the real reducer + view (no tokio, no agent). ──────────────

use rstui_acp_client::acp::{RichUiFormat, RichUiPayload, render_capability_meta};

/// The exact client-capability `_meta` the ACP `initialize` sends must
/// carry the full catalog for *both* formats — not just a name.
#[test]
fn acp_initialize_meta_ships_the_full_catalog_for_both_formats() {
    let meta = render_capability_meta();

    // A2UI: the canonical id AND the self-contained inline catalog with
    // every one of the 18 basic-catalog components + the functions.
    let a2ui = &meta["a2uiClientCapabilities"]["v0.10"];
    assert_eq!(
        a2ui["supportedCatalogIds"][0],
        "https://a2ui.org/specification/v0_10/basic_catalog.json"
    );
    let catalog = &a2ui["inlineCatalogs"][0];
    let components = catalog["components"]
        .as_object()
        .expect("inline A2UI catalog carries the component schemas");
    assert_eq!(components.len(), 18, "all 18 A2UI components are sent");
    let button = &catalog["components"]["Button"];
    assert!(button.is_object(), "Button schema travels inline");
    assert!(
        serde_json::to_string(button).unwrap().contains("action"),
        "the Button schema carries its real props (child/action)"
    );
    assert!(catalog["functions"]["formatString"].is_object());
    assert!(
        !serde_json::to_string(catalog)
            .unwrap()
            .contains("common_types.json#"),
        "the inline catalog is self-contained (no external $ref)"
    );

    // json-render: the full component catalog + the LLM prompt.
    let json_render = &meta["rstuiJsonUi"]["jsonRender"];
    assert_eq!(
        json_render["catalog"].as_object().unwrap().len(),
        27,
        "the json-render component catalog is sent"
    );
    assert!(
        json_render["prompt"]
            .as_str()
            .unwrap()
            .contains("flat element map"),
        "the json-render authoring prompt is sent"
    );
}

/// An agent that replies with an A2UI document (createSurface +
/// updateComponents + updateDataModel JSONL) has it detected, folded
/// into the transcript, and rendered — verified through the public
/// reducer + the real `view`.
#[test]
fn agent_a2ui_document_renders_in_the_transcript() {
    let mut h = chatting(120, 40);
    let stream = [
        r#"{"version":"v0.10","createSurface":{"surfaceId":"s1","catalogId":"https://a2ui.org/specification/v0_10/basic_catalog.json"}}"#,
        r#"{"version":"v0.10","updateComponents":{"surfaceId":"s1","components":[{"id":"root","component":"Column","children":["greeting","cta"]},{"id":"greeting","component":"Text","text":{"path":"/who"}},{"id":"cta","component":"Button","child":"ctaLabel","action":{"event":{"name":"go"}}},{"id":"ctaLabel","component":"Text","text":"Proceed"}]}}"#,
        r#"{"version":"v0.10","updateDataModel":{"surfaceId":"s1","path":"/who","value":"Hello from the agent"}}"#,
    ]
    .join("\n");
    h.message(Msg::Acp(AcpEvent::RichUi(RichUiPayload {
        format: RichUiFormat::A2ui,
        source: stream,
    })));
    let screen = h.snapshot();
    assert!(
        screen.contains("Hello from the agent"),
        "the data-bound A2UI Text rendered:\n{screen}"
    );
    assert!(
        screen.contains("Proceed"),
        "the A2UI Button label rendered:\n{screen}"
    );
    assert!(h.is_running());
}

/// An agent that replies with a json-render flat spec has it detected
/// and rendered through the same path.
#[test]
fn agent_json_render_document_renders_in_the_transcript() {
    let mut h = chatting(120, 40);
    let spec = r#"{"root":"card","elements":{
        "card":{"type":"Card","props":{"title":"Status"},"children":["line"]},
        "line":{"type":"Text","props":{"text":"json-render works end to end"}}
    }}"#;
    h.message(Msg::Acp(AcpEvent::RichUi(RichUiPayload {
        format: RichUiFormat::JsonRender,
        source: spec.to_owned(),
    })));
    let screen = h.snapshot();
    assert!(
        screen.contains("json-render works end to end"),
        "the json-render document rendered in the transcript:\n{screen}"
    );
    assert!(h.is_running());
}

// ---- Codex-parity W1-1: composer input history (↑/↓ recall) ----

fn composer_text(h: &Harness<ChatApp>) -> String {
    h.app().composer().lines().join("\n")
}

fn key(h: &mut Harness<ChatApp>, code: KeyCode) {
    h.message(Msg::Key(KeyEvent::from_code(code)));
}

/// Submitting prompts records them; ↑ walks back through them (clamped at
/// the oldest), ↓ walks forward and finally restores the half-typed draft —
/// the readline / Codex-CLI contract. Persistence is intentionally inert
/// under `cargo test` (no terminal); only the in-memory behaviour is driven.
#[test]
fn up_down_recall_submitted_prompts_and_restore_the_draft() {
    let mut h = chatting(100, 30);

    typ(&mut h, "first prompt");
    key(&mut h, KeyCode::Enter);
    typ(&mut h, "second prompt");
    key(&mut h, KeyCode::Enter);
    assert!(h.app().composer().is_empty(), "composer clears on submit");
    assert_eq!(
        h.app().history().entries(),
        ["first prompt", "second prompt"]
    );

    // A half-typed draft is preserved across the browse.
    typ(&mut h, "draft");
    key(&mut h, KeyCode::Up);
    assert_eq!(composer_text(&h), "second prompt", "↑ recalls the newest");
    assert!(h.app().history().browsing());
    key(&mut h, KeyCode::Up);
    assert_eq!(composer_text(&h), "first prompt", "↑ again → older");
    key(&mut h, KeyCode::Up);
    assert_eq!(composer_text(&h), "first prompt", "clamped at the oldest");

    key(&mut h, KeyCode::Down);
    assert_eq!(composer_text(&h), "second prompt", "↓ → newer");
    key(&mut h, KeyCode::Down);
    assert_eq!(
        composer_text(&h),
        "draft",
        "↓ past newest restores the draft"
    );
    assert!(!h.app().history().browsing());
    assert!(h.is_running());
}

/// History dedups consecutive duplicates, and any composer edit ends the
/// browse so a fresh ↑ starts again from the newest entry.
#[test]
fn duplicate_submissions_dedup_and_editing_resets_the_browse() {
    let mut h = chatting(100, 30);
    typ(&mut h, "same");
    key(&mut h, KeyCode::Enter);
    typ(&mut h, "same");
    key(&mut h, KeyCode::Enter);
    assert_eq!(
        h.app().history().entries(),
        ["same"],
        "consecutive duplicate submissions are not stored twice"
    );

    key(&mut h, KeyCode::Up);
    assert_eq!(composer_text(&h), "same");
    assert!(h.app().history().browsing());
    // Editing the recalled text ends the browse.
    typ(&mut h, "X");
    assert!(!h.app().history().browsing(), "an edit resets the browse");
    assert_eq!(composer_text(&h), "sameX");
}

/// On a multi-line draft, ↑/↓ first move *within* the draft and only recall
/// history once the cursor can go no further (first / last row).
#[test]
fn arrows_move_within_a_multiline_draft_before_recalling_history() {
    let mut h = chatting(100, 30);
    typ(&mut h, "old");
    key(&mut h, KeyCode::Enter);

    // Two-row draft: "a" / "b", cursor on the last row.
    typ(&mut h, "a");
    h.message(Msg::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)));
    typ(&mut h, "b");
    assert_eq!(composer_text(&h), "a\nb");

    // First ↑ moves up inside the draft (row 1 → row 0), not history.
    key(&mut h, KeyCode::Up);
    assert_eq!(
        composer_text(&h),
        "a\nb",
        "still the draft after the first ↑"
    );
    assert!(!h.app().history().browsing());
    // Second ↑ is on the first row → now it recalls history.
    key(&mut h, KeyCode::Up);
    assert_eq!(
        composer_text(&h),
        "old",
        "↑ on the first row recalls history"
    );
    assert!(h.app().history().browsing());
}

// ---- Codex-parity W1-2: /copy last response to clipboard ----

/// `/copy` targets the most recent agent answer. The OS-clipboard hop is
/// inert under `cargo test` (no terminal), so the assertion is on the
/// observable system breadcrumb + the resolved payload, not the escape.
#[test]
fn slash_copy_targets_the_last_agent_answer() {
    let mut h = chatting(100, 30);

    // Nothing answered yet → /copy says so and copies nothing.
    typ(&mut h, "/copy");
    key(&mut h, KeyCode::Enter);
    assert!(h.app().last_response().is_none());
    assert!(
        h.app()
            .transcript()
            .last()
            .unwrap()
            .text
            .contains("nothing to copy"),
        "with no agent response, /copy reports nothing to copy"
    );

    // Agent answers; /copy now resolves that text as the payload.
    h.message(Msg::Acp(AcpEvent::AgentText(
        "The answer is **42**.".to_owned(),
    )));
    assert_eq!(h.app().last_response(), Some("The answer is **42**."));

    typ(&mut h, "/copy");
    key(&mut h, KeyCode::Enter);
    let last = &h.app().transcript().last().unwrap().text;
    assert!(
        last.contains("copied") || last.contains("copy unavailable"),
        "/copy reports its outcome (got {last:?})"
    );
    assert!(
        !last.contains("nothing to copy"),
        "with an answer present, /copy does not say nothing-to-copy"
    );
    assert!(h.is_running());
}

// ---- Codex-parity W1-3: terminal title (OSC 2) tracks the session ----

/// The OSC 2 emit is inert under `cargo test` (no terminal), but the derived
/// title is tracked headlessly: it must follow the screen and whether the
/// session needs the user (approval), so a backgrounded tab is informative.
#[test]
fn terminal_title_follows_the_session_state() {
    // Picker screen → "pick an agent".
    let mut h = booted(100, 30);
    h.message(Msg::RegistryLoaded(Box::new(Registry::offline_fallback())));
    assert_eq!(h.app().screen(), Screen::Picker);
    assert_eq!(h.app().terminal_title(), "rstui-acp — pick an agent");

    // Connected chat (no agent_command set in headless tests → "agent").
    let mut h = chatting(100, 30);
    assert_eq!(h.app().terminal_title(), "rstui-acp — agent");

    // A pending permission flips the title to the attention form.
    h.message(Msg::Acp(AcpEvent::Permission {
        id: 1,
        title: "Run `ls`".to_owned(),
        options: vec![],
    }));
    assert_eq!(
        h.app().terminal_title(),
        "● rstui-acp — agent — approval needed",
        "an open approval is surfaced in the tab title"
    );
    assert!(h.is_running());
}

// ---- Codex-parity W1-4: turn-completion bell ----

/// `/bell` toggles the turn-completion bell for the session, and a turn
/// ending with the bell armed is handled cleanly (the BEL emit itself is
/// inert under `cargo test` — no terminal).
#[test]
fn slash_bell_toggles_and_turn_end_is_handled() {
    let mut h = chatting(100, 30);
    // Default on (no RSTUI_ACP_BELL in the test env).
    assert!(h.app().bell_enabled(), "bell defaults on");

    typ(&mut h, "/bell");
    key(&mut h, KeyCode::Enter);
    assert!(!h.app().bell_enabled(), "/bell turned it off");
    assert!(
        h.app()
            .transcript()
            .last()
            .unwrap()
            .text
            .contains("bell: off")
    );

    typ(&mut h, "/bell");
    key(&mut h, KeyCode::Enter);
    assert!(h.app().bell_enabled(), "/bell turned it back on");

    // A turn ending with the bell armed must not panic / quit.
    h.message(Msg::Acp(AcpEvent::TurnEnded("end_turn".to_owned())));
    assert!(!h.app().is_streaming());
    assert!(h.is_running());
}

// ---- Codex-parity W1-5: /init + /review canned prompts ----

/// `/init` and `/review` are first-class built-ins (so autocomplete offers
/// them) and route to the shared prompt path — disconnected they hit the
/// "not connected" guard, *not* "unknown command", which is what proves the
/// wiring (the connected send is the async side, out of `Harness` by
/// ADR 0011, exactly as for a typed prompt).
#[test]
fn slash_init_and_review_are_builtins_wired_to_the_prompt_path() {
    let mut h = chatting(100, 30);

    let specs = h.app().command_specs();
    for name in ["init", "review"] {
        let spec = specs
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("/{name} is offered in autocomplete"));
        assert_eq!(
            spec.source,
            CommandSource::Builtin,
            "/{name} is a client built-in"
        );
    }

    typ(&mut h, "/init");
    key(&mut h, KeyCode::Enter);
    let last = &h.app().transcript().last().unwrap().text;
    assert!(
        last.contains("not connected"),
        "/init routed to the prompt path (got {last:?})"
    );
    assert!(
        !last.contains("unknown command"),
        "/init is recognised, not an unknown command"
    );

    typ(&mut h, "/review");
    key(&mut h, KeyCode::Enter);
    let last = &h.app().transcript().last().unwrap().text;
    assert!(last.contains("not connected") && !last.contains("unknown command"));
    assert!(h.is_running());
}

// ---- Codex-parity W1-6: full-screen transcript pager ----

fn open_pager(h: &mut Harness<ChatApp>) {
    typ(h, "/transcript");
    key(h, KeyCode::Enter);
    assert!(h.app().pager().open(), "/transcript opens the pager");
}

/// `/transcript` opens a full-screen pager; navigation uses the chat's
/// scroll model (`follow` + clamped offset); `/` runs an incremental
/// substring filter; Esc clears the filter first, then closes.
#[test]
fn transcript_pager_opens_scrolls_searches_and_closes() {
    let mut h = chatting(120, 30);
    h.message(Msg::Acp(AcpEvent::AgentText(
        "PAGERALPHA then PAGERBETA on the next idea".to_owned(),
    )));

    open_pager(&mut h);
    assert!(h.app().pager().follows(), "opens stuck to the latest");
    let screen = h.snapshot();
    assert!(
        screen.contains("Transcript"),
        "pager chrome drawn:\n{screen}"
    );

    // Navigation: Down unsticks from the bottom and advances the offset; G
    // re-sticks; g goes to the top.
    key(&mut h, KeyCode::Down);
    assert!(!h.app().pager().follows());
    assert_eq!(h.app().pager().scroll(), 1);
    key(&mut h, KeyCode::Char('G'));
    assert!(h.app().pager().follows(), "G jumps back to the latest");
    key(&mut h, KeyCode::Char('g'));
    assert!(!h.app().pager().follows());
    assert_eq!(h.app().pager().scroll(), 0, "g goes to the top");

    // Incremental search: '/' enters search mode, chars build the query,
    // Enter applies it.
    key(&mut h, KeyCode::Char('/'));
    assert!(h.app().pager().searching());
    for c in "PAGERBETA".chars() {
        key(&mut h, KeyCode::Char(c));
    }
    assert_eq!(h.app().pager().query(), "PAGERBETA");
    key(&mut h, KeyCode::Enter);
    assert!(!h.app().pager().searching(), "Enter applies the filter");
    assert!(
        h.snapshot().contains("match"),
        "the title reports the match count"
    );

    // Esc clears the active filter but keeps the pager open…
    key(&mut h, KeyCode::Esc);
    assert!(h.app().pager().open());
    assert!(h.app().pager().query().is_empty(), "Esc cleared the filter");
    // …a second Esc closes it.
    key(&mut h, KeyCode::Esc);
    assert!(!h.app().pager().open(), "Esc on a clean pager closes it");
    assert!(h.is_running());
}

// ---- Codex-parity W2-1: /status + token usage ----

/// An ACP `usage_update` is folded into state; `/status` opens an overlay
/// that surfaces it (with the % of the context window) plus the session
/// configuration, and Esc closes it.
#[test]
fn usage_update_feeds_the_status_overlay() {
    let mut h = chatting(110, 30);
    assert_eq!(h.app().usage(), None, "no usage reported yet");

    h.message(Msg::Acp(AcpEvent::Usage {
        used: 1500,
        size: 10000,
    }));
    assert_eq!(h.app().usage(), Some((1500, 10000)));

    typ(&mut h, "/status");
    key(&mut h, KeyCode::Enter);
    assert!(h.app().status_visible(), "/status opens the overlay");
    let screen = h.snapshot();
    assert!(screen.contains("Status"), "status chrome drawn:\n{screen}");
    assert!(
        screen.contains("1500") && screen.contains("15%"),
        "token usage and its window percentage are shown:\n{screen}"
    );

    key(&mut h, KeyCode::Esc);
    assert!(!h.app().status_visible(), "Esc closes /status");
    assert!(h.is_running());
}

// ---- Codex-parity W2-2: /model picker ----

use rstui_acp_client::acp::ModelOption;

fn model(id: &str, name: &str) -> ModelOption {
    ModelOption {
        id: id.to_owned(),
        name: name.to_owned(),
        description: String::new(),
    }
}

/// The agent's `NewSessionResponse.models` feeds `/model`; the picker lists
/// them with the current one marked, and `ModelSelected` (the agent's
/// `session/set_model` ack) updates the active model + breadcrumbs it.
#[test]
fn model_catalogue_drives_the_model_picker() {
    let mut h = chatting(110, 30);

    // No catalogue advertised → /model explains, opens nothing.
    typ(&mut h, "/model");
    key(&mut h, KeyCode::Enter);
    assert!(!h.app().model_picker_open());
    assert!(
        h.app()
            .transcript()
            .last()
            .unwrap()
            .text
            .contains("did not advertise")
    );

    // Agent reports a catalogue.
    h.message(Msg::Acp(AcpEvent::Models {
        current: "fast".to_owned(),
        available: vec![model("fast", "Fast"), model("smart", "Smart")],
    }));
    assert_eq!(h.app().current_model(), Some("fast"));
    assert_eq!(h.app().current_model_name(), "Fast");

    typ(&mut h, "/model");
    key(&mut h, KeyCode::Enter);
    assert!(h.app().model_picker_open(), "/model opens with a catalogue");
    assert_eq!(h.app().model_sel(), 0, "starts on the current model");
    let screen = h.snapshot();
    assert!(screen.contains("Fast") && screen.contains("Smart"));

    // Move to "smart" and choose it. No driver in headless tests, so the
    // switch reports "not connected" rather than sending — the picker still
    // closes and the wiring (recognised, routed) is what we assert.
    key(&mut h, KeyCode::Down);
    assert_eq!(h.app().model_sel(), 1);
    key(&mut h, KeyCode::Enter);
    assert!(!h.app().model_picker_open(), "Enter closes the picker");
    assert!(
        h.app()
            .transcript()
            .last()
            .unwrap()
            .text
            .contains("not connected")
    );

    // The agent's set_model ack authoritatively updates the active model.
    h.message(Msg::Acp(AcpEvent::ModelSelected("smart".to_owned())));
    assert_eq!(h.app().current_model(), Some("smart"));
    assert_eq!(h.app().current_model_name(), "Smart");
    assert!(
        h.app()
            .transcript()
            .last()
            .unwrap()
            .text
            .contains("model → Smart")
    );
    assert!(h.is_running());
}

// ---- Codex-parity W2-3: /mode session-mode switch ----

use rstui_acp_client::acp::ModeOption;

fn mode_opt(id: &str, name: &str) -> ModeOption {
    ModeOption {
        id: id.to_owned(),
        name: name.to_owned(),
        description: String::new(),
    }
}

/// The agent's `NewSessionResponse.modes` feeds `/mode`; an agent-initiated
/// `current_mode_update` (delivered as `AcpEvent::ModeChanged`) and our own
/// `session/set_mode` both update the active mode + breadcrumb it.
#[test]
fn session_modes_drive_the_mode_picker() {
    let mut h = chatting(110, 30);

    typ(&mut h, "/mode");
    key(&mut h, KeyCode::Enter);
    assert!(!h.app().mode_picker_open());
    assert!(
        h.app()
            .transcript()
            .last()
            .unwrap()
            .text
            .contains("did not advertise session modes")
    );

    h.message(Msg::Acp(AcpEvent::Modes {
        current: "default".to_owned(),
        available: vec![mode_opt("default", "Default"), mode_opt("plan", "Plan")],
    }));
    assert_eq!(h.app().current_mode(), Some("default"));
    assert_eq!(h.app().current_mode_name(), "Default");

    typ(&mut h, "/mode");
    key(&mut h, KeyCode::Enter);
    assert!(h.app().mode_picker_open());
    assert_eq!(h.app().mode_sel(), 0, "starts on the current mode");
    let screen = h.snapshot();
    assert!(screen.contains("Default") && screen.contains("Plan"));

    key(&mut h, KeyCode::Down);
    assert_eq!(h.app().mode_sel(), 1);
    key(&mut h, KeyCode::Enter);
    assert!(!h.app().mode_picker_open(), "Enter closes the picker");
    assert!(
        h.app()
            .transcript()
            .last()
            .unwrap()
            .text
            .contains("not connected")
    );

    // The agent switching mode itself (current_mode_update) is authoritative.
    h.message(Msg::Acp(AcpEvent::ModeChanged("plan".to_owned())));
    assert_eq!(h.app().current_mode(), Some("plan"));
    assert_eq!(h.app().current_mode_name(), "Plan");
    assert!(
        h.app()
            .transcript()
            .last()
            .unwrap()
            .text
            .contains("mode → Plan")
    );
    assert!(h.is_running());
}

// ---- Codex-parity W2-4: /resume prior sessions ----

/// Each started session (`AcpEvent::SessionStarted`) is remembered;
/// `/resume` lists them and Enter routes a `session/load` (the connected
/// send is the async side, out of Harness by ADR 0011 — so disconnected it
/// reports "not connected", which is what proves the wiring).
#[test]
fn started_sessions_feed_the_resume_picker() {
    let mut h = chatting(110, 30);

    // Nothing recorded yet.
    typ(&mut h, "/resume");
    key(&mut h, KeyCode::Enter);
    assert!(!h.app().resume_picker_open());
    assert!(
        h.app()
            .transcript()
            .last()
            .unwrap()
            .text
            .contains("no saved sessions")
    );

    // Two sessions start; both are remembered (dedup is by id).
    h.message(Msg::Acp(AcpEvent::SessionStarted("sess-A".to_owned())));
    h.message(Msg::Acp(AcpEvent::SessionStarted("sess-B".to_owned())));
    h.message(Msg::Acp(AcpEvent::SessionStarted("sess-A".to_owned())));
    let ids: Vec<String> = h
        .app()
        .resume_sessions()
        .iter()
        .map(|s| s.id.clone())
        .collect();
    assert_eq!(ids.len(), 2, "dedup by id; got {ids:?}");
    assert!(ids.contains(&"sess-A".to_owned()) && ids.contains(&"sess-B".to_owned()));

    typ(&mut h, "/resume");
    key(&mut h, KeyCode::Enter);
    assert!(h.app().resume_picker_open(), "/resume opens with history");
    assert!(h.snapshot().contains("Resume"), "resume chrome drawn");

    // Choose one — no driver headless, so it reports not-connected and the
    // picker still closes (the route is what we assert).
    key(&mut h, KeyCode::Enter);
    assert!(!h.app().resume_picker_open());
    assert!(
        h.app()
            .transcript()
            .last()
            .unwrap()
            .text
            .contains("not connected")
    );
    assert!(h.is_running());
}

// ---- Codex-parity W2-5: @-file mention completion ----

/// Typing `@<query>` opens a fuzzy file-completion popup over the cwd; the
/// test cwd (a crate or the workspace root) deterministically contains
/// `Cargo.toml`. Tab inserts the path into the prompt text (the agent
/// resolves `@path` itself — the Codex composer UX). `user@host` must NOT
/// trigger it (the `@` is not at a word start).
#[test]
fn at_mention_fuzzy_file_completion() {
    let mut h = chatting(120, 30);
    assert!(h.app().mention().is_none(), "no popup before '@'");

    typ(&mut h, "@Cargo");
    let m = h.app().mention().expect("'@Cargo' opens the file popup");
    assert!(
        m.items.iter().any(|p| p.ends_with("Cargo.toml")),
        "the cwd's Cargo.toml is offered; got {:?}",
        m.items
    );
    assert!(h.snapshot().contains("@ files"), "popup chrome drawn");

    // Down moves the highlight (when there is more than one match).
    let before = h.app().mention().unwrap().selected;
    key(&mut h, KeyCode::Down);
    let after = h.app().mention().unwrap().selected;
    assert!(after != before || h.app().mention().unwrap().items.len() == 1);

    // Select Cargo.toml explicitly, then Tab inserts it + a trailing space.
    loop {
        let m = h.app().mention().unwrap();
        if m.items[m.selected].ends_with("Cargo.toml") {
            break;
        }
        key(&mut h, KeyCode::Down);
    }
    key(&mut h, KeyCode::Tab);
    assert!(
        h.app().mention().is_none(),
        "Tab accepts and closes the popup"
    );
    let text = h.app().composer().lines().join("\n");
    assert!(
        text.contains("Cargo.toml "),
        "the path replaced the @token in the prompt; got {text:?}"
    );

    // An email-like `user@host` does not trigger the mention popup.
    let mut h2 = chatting(120, 30);
    typ(&mut h2, "mail user@hos");
    assert!(
        h2.app().mention().is_none(),
        "`@` mid-word (user@host) is not a mention"
    );
    // …but a fresh word starting with `@` does.
    typ(&mut h2, " @sr");
    assert!(
        h2.app().mention().is_some(),
        "a word-initial @ does trigger"
    );

    // Slash completion and @-mention are mutually exclusive.
    let mut h3 = chatting(120, 30);
    typ(&mut h3, "/he");
    assert!(h3.app().completion().is_some());
    assert!(
        h3.app().mention().is_none(),
        "slash popup suppresses mention"
    );
    assert!(h.is_running());
}

// ---- Codex-parity W2-6: sign-in (ACP authenticate) ----

use rstui_acp_client::acp::AuthOption;

/// An agent that rejects `session/new` and advertises auth methods makes the
/// driver emit `AuthRequired`; the client auto-opens a sign-in picker.
/// Enter routes `authenticate` (the connected send is the async side, out
/// of Harness by ADR 0011 → "not connected" headless). `/login` reopens it.
#[test]
fn auth_required_opens_the_sign_in_picker() {
    let mut h = chatting(110, 30);
    assert!(!h.app().auth_picker_open());

    typ(&mut h, "/login");
    key(&mut h, KeyCode::Enter);
    assert!(
        h.app()
            .transcript()
            .last()
            .unwrap()
            .text
            .contains("no sign-in needed"),
        "with no methods, /login explains"
    );

    h.message(Msg::Acp(AcpEvent::AuthRequired(vec![
        AuthOption {
            id: "chatgpt".to_owned(),
            name: "Sign in with ChatGPT".to_owned(),
            description: String::new(),
        },
        AuthOption {
            id: "api".to_owned(),
            name: "API key".to_owned(),
            description: "Use OPENAI_API_KEY".to_owned(),
        },
    ])));
    assert!(
        h.app().auth_picker_open(),
        "AuthRequired auto-opens sign-in"
    );
    let screen = h.snapshot();
    assert!(
        screen.contains("Sign in") && screen.contains("ChatGPT"),
        "the auth methods are listed:\n{screen}"
    );

    key(&mut h, KeyCode::Down);
    assert_eq!(h.app().auth_sel(), 1);
    key(&mut h, KeyCode::Enter);
    assert!(!h.app().auth_picker_open(), "Enter closes the picker");
    assert!(
        h.app()
            .transcript()
            .last()
            .unwrap()
            .text
            .contains("not connected")
    );

    // The methods are remembered, so /login can reopen the picker.
    typ(&mut h, "/login");
    key(&mut h, KeyCode::Enter);
    assert!(
        h.app().auth_picker_open(),
        "/login reopens the sign-in picker"
    );
    key(&mut h, KeyCode::Esc);
    assert!(!h.app().auth_picker_open(), "Esc dismisses sign-in");
    assert!(h.is_running());
}

// ---- Codex-parity W3-2: /diff working-tree viewer ----

/// `/diff` shells out (via `Cmd::perform`, the registry pattern) and opens a
/// scrollable, coloured overlay. The git call's output is environment-
/// dependent, so the structural assertions inject a known diff string; the
/// `/diff` round-trip only needs to deterministically open the overlay.
#[test]
fn slash_diff_opens_a_scrollable_overlay() {
    let mut h = chatting(120, 30);

    // The /diff command is a recognised builtin and round-trips to an
    // overlay (Harness runs Cmd::perform inline; real git output varies but
    // an overlay always opens).
    assert!(
        h.app()
            .command_specs()
            .iter()
            .any(|c| c.name == "diff" && c.source == CommandSource::Builtin)
    );
    typ(&mut h, "/diff");
    key(&mut h, KeyCode::Enter);
    assert!(h.app().diff().is_some(), "/diff opens the diff overlay");
    assert!(h.snapshot().contains("git diff"), "diff chrome drawn");

    // Inject a known diff so the body assertions are deterministic.
    h.message(Msg::DiffLoaded(
        "diff --git a/x b/x\n@@ -1 +1 @@\n-old line\n+new line\n".to_owned(),
    ));
    let screen = h.snapshot();
    assert!(
        screen.contains("old line") && screen.contains("new line") && screen.contains("@@"),
        "the unified diff renders:\n{screen}"
    );

    // Scroll model: Down advances, g resets to the top.
    key(&mut h, KeyCode::Down);
    assert_eq!(h.app().diff().unwrap().scroll(), 1);
    key(&mut h, KeyCode::Char('g'));
    assert_eq!(h.app().diff().unwrap().scroll(), 0);

    key(&mut h, KeyCode::Esc);
    assert!(h.app().diff().is_none(), "Esc closes the diff overlay");
    assert!(h.is_running());
}
