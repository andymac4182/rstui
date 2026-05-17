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
use rstui_core::{Event, KeyCode, KeyEvent, Size};
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
