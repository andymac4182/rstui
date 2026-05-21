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
use rstui_core::{Event, KeyCode, KeyEvent, KeyModifiers, Position, Size};
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

/// Both `--flag value` and the GNU `--flag=value` form set the same fields
/// (the latter was previously dropped → `--cmd=…` silently fell through to
/// the registry picker). Empty values are treated as absent.
#[test]
fn from_args_accepts_equals_and_space_forms() {
    // Space form (regression guard).
    let c = Config::from_args(
        ["--cmd", "my-acp --stdio", "--plugin", "p1"]
            .into_iter()
            .map(String::from),
    );
    assert_eq!(c.agent_command.as_deref(), Some("my-acp --stdio"));
    assert_eq!(c.plugins, ["p1"]);

    // `--flag=value` form — the bug this fixes.
    let c = Config::from_args(
        [
            "--cmd=my-acp --stdio",
            "--profile=dev",
            "--plugin=p1",
            "--plugin=p2",
        ]
        .into_iter()
        .map(String::from),
    );
    assert_eq!(
        c.agent_command.as_deref(),
        Some("my-acp --stdio"),
        "--cmd=value must set the command"
    );
    assert_eq!(c.profile.as_deref(), Some("dev"));
    assert_eq!(c.plugins, ["p1", "p2"]);

    // A value containing `=` survives (split is on the first `=` only).
    let c = Config::from_args(["--cmd=foo --opt=bar".to_owned()]);
    assert_eq!(c.agent_command.as_deref(), Some("foo --opt=bar"));

    // Empty / whitespace value ⇒ absent (still the picker).
    let c = Config::from_args(["--cmd=".to_owned()]);
    assert_eq!(c.agent_command, None);
    let c = Config::from_args(["--cmd", "  "].into_iter().map(String::from));
    assert_eq!(c.agent_command, None);
}

/// `--profile <name>` resolves a `(command, plugins)` recipe against the
/// profiles map (passed in, so the test never touches disk). Precedence is
/// `--cmd` › `--profile` › env; profile plugins union with `--plugin`.
#[test]
fn profile_switch_resolves_command_and_plugins_with_precedence() {
    use rstui_acp_client::profiles::parse_profiles;

    let profiles = parse_profiles(
        "[dev]\ncommand = ./my-acp --stdio\nplugin = rstui-acp-plugin-git\nplugin = ./extra\n",
    );

    // --profile alone supplies command + plugins.
    let cfg = Config::from_args(["--profile", "dev"].into_iter().map(String::from))
        .with_profile(&profiles);
    assert_eq!(cfg.agent_command.as_deref(), Some("./my-acp --stdio"));
    assert_eq!(cfg.plugins, ["rstui-acp-plugin-git", "./extra"]);

    // An explicit --cmd beats the profile's command; plugins still merge
    // (union — the explicit --plugin is kept, profile ones appended once).
    let cfg = Config::from_args(
        [
            "--cmd",
            "explicit-acp",
            "--profile",
            "dev",
            "--plugin",
            "./extra",
        ]
        .into_iter()
        .map(String::from),
    )
    .with_profile(&profiles);
    assert_eq!(cfg.agent_command.as_deref(), Some("explicit-acp"));
    assert_eq!(cfg.plugins, ["./extra", "rstui-acp-plugin-git"]);

    // Profile beats the env var; an unknown profile is an inert no-op.
    let cfg = Config::from_args(["--profile", "dev"].into_iter().map(String::from))
        .with_profile(&profiles)
        .with_agent_env(Some("ENV".to_owned()));
    assert_eq!(cfg.agent_command.as_deref(), Some("./my-acp --stdio"));
    let cfg = Config::from_args(["--profile", "nope"].into_iter().map(String::from))
        .with_profile(&profiles);
    assert_eq!(cfg.agent_command, None);
    assert!(cfg.plugins.is_empty());
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
fn render_is_a_builtin_command_that_primes_the_agent() {
    // `/render` is offered in the autocomplete (a registered built-in,
    // alongside the other canned-prompt commands).
    let mut h = chatting(100, 30);
    typ(&mut h, "/render");
    let comp = h.app().completion().expect("'/' opens the popup");
    assert!(
        comp.items.iter().any(|c| c.name == "render"),
        "/render is a built-in command"
    );

    // Running it is *handled* — it reaches the canned-prompt path
    // (`send_user_prompt`), it is NOT an "unknown command". In the
    // headless harness there is no live agent, so the deterministic
    // breadcrumb is the not-connected notice (same as /init, /review).
    typ(&mut h, " a dashboard");
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::Enter)));
    let last = &h
        .app()
        .transcript()
        .last()
        .expect("a system breadcrumb")
        .text;
    assert!(
        !last.contains("unknown command"),
        "/render is wired (not the unknown-command arm): {last:?}"
    );
    assert!(
        last.contains("not connected"),
        "headless: /render reached send_user_prompt: {last:?}"
    );
    assert!(h.is_running());
}

#[test]
fn a2ui_is_a_builtin_command_that_primes_the_agent() {
    // Symmetric twin of /render for A2UI — same canned-prompt path so
    // any agent (not just one reading the initialize _meta) learns the
    // A2UI format from the conversation.
    let mut h = chatting(100, 30);
    typ(&mut h, "/a2ui");
    let comp = h.app().completion().expect("'/' opens the popup");
    assert!(
        comp.items.iter().any(|c| c.name == "a2ui"),
        "/a2ui is a built-in command"
    );

    typ(&mut h, " a sign-up form");
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::Enter)));
    let last = &h
        .app()
        .transcript()
        .last()
        .expect("a system breadcrumb")
        .text;
    assert!(
        !last.contains("unknown command"),
        "/a2ui is wired (not the unknown-command arm): {last:?}"
    );
    assert!(
        last.contains("not connected"),
        "headless: /a2ui reached send_user_prompt: {last:?}"
    );
    assert!(h.is_running());
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
fn keymap_panel_opens_with_ctrl_x_navigates_rebinds_and_closes() {
    let mut h = booted(100, 30);
    assert!(!h.app().keymap_panel_open(), "panel starts closed");

    // Ctrl+X is the global Action::Drawer binding — resolved through the
    // keymap on any screen, after the plugin-chord layer. (It moved off
    // Ctrl+K so the composer can claim that for readline kill-line.)
    h.message(Msg::Key(KeyEvent::new(
        KeyCode::Char('x'),
        KeyModifiers::CONTROL,
    )));
    assert!(h.app().keymap_panel_open(), "Ctrl+X opens the keymap panel");
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
        37,
        "the json-render catalog is sent (chart set + form elements)"
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

#[test]
fn a_streamed_prose_wrapped_agent_document_renders_at_turn_end() {
    // The real-agent path the bug report hit: the reply is streamed
    // token-by-token, wrapped in prose, across many `agent_message_chunk`
    // events. Per-chunk detection cannot see it (each chunk is an
    // incomplete fragment); it must be detected on the *assembled*
    // message at turn end and split into [prose] [rendered UI] [prose].
    let mut h = chatting(120, 40);
    for chunk in [
        "Here is your dashboard:\n\n```json-render\n",
        "{\"root\":\"c\",\"elements\":{\"c\":{\"type\":\"Car",
        "d\",\"props\":{\"title\":\"Sales\"},\"children\":[\"t\"]},",
        "\"t\":{\"type\":\"Text\",\"props\":{\"text\":\"Revenue up 12%\"}}}}",
        "\n```\n\nHope that helps!",
    ] {
        h.message(Msg::Acp(AcpEvent::AgentText(chunk.to_owned())));
    }
    // Mid-stream it is still raw text (the fence is not closed yet).
    assert!(
        h.snapshot().contains("\"root\""),
        "before turn end the partial doc is raw text"
    );

    h.message(Msg::Acp(AcpEvent::TurnEnded("EndTurn".to_owned())));
    let screen = h.snapshot();
    assert!(
        screen.contains("Revenue up 12%"),
        "the streamed json-render doc is RENDERED at turn end:\n{screen}"
    );
    assert!(
        screen.contains("Here is your dashboard:") && screen.contains("Hope that helps!"),
        "the surrounding prose is kept (split, not discarded):\n{screen}"
    );
    assert!(
        !screen.contains("\"elements\""),
        "the raw JSON is no longer shown as text (it is the rendered UI):\n{screen}"
    );
    assert!(h.is_running());
}

#[test]
fn a_message_with_markdown_and_several_blocks_renders_each_inline() {
    // "Use markdown for the message AND turn json-render/A2UI (and the
    // diagram DSLs) into a UI" — interleaved prose + a Mermaid diagram +
    // prose + a json-render doc, streamed, all rendered at turn end.
    let mut h = chatting(120, 50);
    for chunk in [
        "# Plan\n\nThe flow:\n\n```mermaid\nflowchart LR\n  Ingest-->Score\n```",
        "\n\nAnd the live status:\n\n```json-render\n",
        "{\"root\":\"c\",\"elements\":{\"c\":{\"type\":\"Text\",",
        "\"props\":{\"text\":\"All systems go\"}}}}\n```\n\nDone.",
    ] {
        h.message(Msg::Acp(AcpEvent::AgentText(chunk.to_owned())));
    }
    h.message(Msg::Acp(AcpEvent::TurnEnded("EndTurn".to_owned())));
    let s = h.snapshot();
    // Markdown prose rendered (heading text present), the Mermaid
    // diagram rendered (node labels painted), the json-render doc
    // rendered (its text), and NO raw fences left as code.
    assert!(s.contains("Plan"), "markdown prose rendered:\n{s}");
    assert!(
        s.contains("Ingest") && s.contains("Score"),
        "the Mermaid diagram rendered inline:\n{s}"
    );
    assert!(
        s.contains("All systems go"),
        "the json-render doc rendered inline:\n{s}"
    );
    assert!(
        !s.contains("```mermaid") && !s.contains("```json-render"),
        "no raw fenced blocks shown as code (they became widgets):\n{s}"
    );
    assert!(h.is_running());
}

#[test]
fn clicking_a_rendered_button_round_trips_an_action_to_the_agent() {
    // The end-to-end interactive path: an agent streams an A2UI form
    // wrapped in prose; it renders as a real UI; clicking its button
    // resolves the action and routes it back to the agent. (Headless
    // has no live agent, so the deterministic proof the whole pipeline
    // — rich_hit → rich_click → send_agent_action — fired is the
    // not-connected breadcrumb, same convention as /render.)
    let mut h = chatting(140, 44);
    h.message(Msg::Resize(Size::new(140, 44)));
    for chunk in [
        "Here is the form:\n\n```a2ui\n",
        r#"{"version":"v0.10","createSurface":{"surfaceId":"s1","catalogId":"c"}}"#,
        "\n",
        r#"{"version":"v0.10","updateComponents":{"surfaceId":"s1","components":["#,
        r#"{"id":"root","component":"Column","children":["t","b"]},"#,
        r#"{"id":"t","component":"Text","text":"Ready?"},"#,
        r#"{"id":"b","component":"Button","child":"bl","action":{"event":{"name":"confirm"}}},"#,
        r#"{"id":"bl","component":"Text","text":"Confirm"}]}}"#,
        "\n```\n\nClick it when ready.",
    ] {
        h.message(Msg::Acp(AcpEvent::AgentText(chunk.to_owned())));
    }
    h.message(Msg::Acp(AcpEvent::TurnEnded("EndTurn".to_owned())));

    let screen = h.snapshot();
    // The A2UI JSONL stream actually RENDERED (the detect-JSONL fix):
    // the button label is on screen, not raw `"updateComponents"` JSON.
    assert!(
        screen.contains("Confirm") && !screen.contains("\"updateComponents\""),
        "the streamed A2UI form rendered as a UI:\n{screen}"
    );

    // Find the button label on screen and click it.
    let (bx, by) = screen
        .lines()
        .enumerate()
        .find_map(|(y, line)| line.find("Confirm").map(|cx| (cx as u16, y as u16)))
        .expect("button label is on screen");
    h.message(Msg::Acp(AcpEvent::Status("session ready".to_owned()))); // keep Chat
    h.message(Msg::RichClick(Position::new(bx + 1, by)));

    let after = h.snapshot();
    assert!(
        after.contains("not connected") || after.contains("UI action sent"),
        "clicking the button resolved + routed the action (round-trip \
         pipeline fired):\n{after}"
    );

    // A click on empty space below everything resolves to nothing new.
    let mut h2 = chatting(140, 44);
    h2.message(Msg::Resize(Size::new(140, 44)));
    h2.message(Msg::RichClick(Position::new(2, 40)));
    assert!(
        h2.is_running() && !h2.snapshot().contains("UI action sent"),
        "an off-target click does nothing"
    );
}

#[test]
fn clicking_a_rendered_checkbox_persists_the_toggle_across_redraws() {
    // Phase 2 — the interactive round-trip's local half: a clicked
    // two-way control mutates a *caller-owned stateful* doc, so the new
    // state survives the every-frame re-projection. An A2UI CheckBox
    // bound to `/agreed` renders `[ ]`; clicking it must flip to `[x]`
    // and STAY flipped on the next redraw (proof the toggle is kept,
    // not lost to a fresh parse), and clicking again flips it back
    // (proof the owned model — not the verbatim source — is mutated).
    let mut h = chatting(140, 44);
    h.message(Msg::Resize(Size::new(140, 44)));
    for chunk in [
        "Please confirm:\n\n```a2ui\n",
        r#"{"version":"v0.10","createSurface":{"surfaceId":"s1","catalogId":"c"}}"#,
        "\n",
        r#"{"version":"v0.10","updateComponents":{"surfaceId":"s1","components":["#,
        r#"{"id":"root","component":"Column","children":["agree"]},"#,
        r#"{"id":"agree","component":"CheckBox","label":"Agree","value":{"path":"/agreed"}}]}}"#,
        "\n```\n\nTick the box.",
    ] {
        h.message(Msg::Acp(AcpEvent::AgentText(chunk.to_owned())));
    }
    h.message(Msg::Acp(AcpEvent::TurnEnded("EndTurn".to_owned())));
    h.message(Msg::Acp(AcpEvent::Status("session ready".to_owned()))); // keep Chat

    let before = h.snapshot();
    assert!(
        before.contains("[ ] Agree") && !before.contains("[x] Agree"),
        "the A2UI CheckBox rendered unchecked:\n{before}"
    );

    // Click the checkbox row. The snapshot's box-drawing border is a
    // multibyte glyph, so the screen *column* is the char count before
    // the match, not the byte offset.
    let (cx, cy) = before
        .lines()
        .enumerate()
        .find_map(|(y, line)| {
            line.find("Agree")
                .map(|b| (line[..b].chars().count() as u16, y as u16))
        })
        .expect("checkbox label is on screen");
    h.message(Msg::RichClick(Position::new(cx, cy)));

    let after = h.snapshot();
    assert!(
        after.contains("[x] Agree") && !after.contains("[ ] Agree"),
        "the toggle PERSISTED across the every-frame re-projection:\n{after}"
    );

    // Click it again — the *owned* model toggles back (not a re-parse
    // of the immutable source, which would always re-show `[ ]`).
    let (cx2, cy2) = after
        .lines()
        .enumerate()
        .find_map(|(y, line)| {
            line.find("Agree")
                .map(|b| (line[..b].chars().count() as u16, y as u16))
        })
        .expect("checkbox still on screen");
    h.message(Msg::RichClick(Position::new(cx2, cy2)));
    let again = h.snapshot();
    assert!(
        again.contains("[ ] Agree") && !again.contains("[x] Agree"),
        "clicking again toggled the owned model back off:\n{again}"
    );
    assert!(h.is_running(), "the app stayed up through the interaction");
}

#[test]
fn real_boot_form_is_interactive_without_an_explicit_resize() {
    // The reported bug: forms render but aren't interactive. Root
    // cause — the runtime renders frame 1 from the real size but never
    // sends an initial `Resize`, so the keyboard/mouse hit-tests
    // (`form_ring`/`form_pane_inner`) ran against the default 80×24 and
    // silently produced no focus ring. `lib.rs::run` now seeds the real
    // size via `with_initial_size`; this reproduces that path and sends
    // NO `Msg::Resize`, so it fails without the fix.
    let mut h = Harness::new(
        ChatApp::new(Config::default()).with_initial_size(Size::new(120, 40)),
        120,
        40,
    );
    h.message(Msg::Acp(AcpEvent::Status("session ready".to_owned())));
    assert_eq!(h.app().screen(), Screen::Chat);
    for chunk in [
        "Fill it:\n\n```a2ui\n",
        r#"{"version":"v0.10","createSurface":{"surfaceId":"s1","catalogId":"c"}}"#,
        "\n",
        r#"{"version":"v0.10","updateComponents":{"surfaceId":"s1","components":["#,
        r#"{"id":"root","component":"Column","children":["who","go"]},"#,
        r#"{"id":"who","component":"TextField","label":"Who","value":{"path":"/who"}},"#,
        r#"{"id":"go","component":"Button","child":"gl","action":{"event":{"name":"save"}}},"#,
        r#"{"id":"gl","component":"Text","text":"Save"}]}}"#,
        "\n```\n",
    ] {
        h.message(Msg::Acp(AcpEvent::AgentText(chunk.to_owned())));
    }
    h.message(Msg::Acp(AcpEvent::TurnEnded("EndTurn".to_owned())));
    h.message(Msg::Acp(AcpEvent::Status("session ready".to_owned())));

    // NOTE: deliberately NO `Msg::Resize` — the real run path.
    assert!(h.app().form_open(), "an interactive doc opened");
    assert!(
        h.snapshot().contains("Agent UI"),
        "the pane renders at the real seeded size:\n{}",
        h.snapshot()
    );
    // The regression: Tab must FOCUS and STAY focused — proof the
    // keyboard hit-test derived a ring from the seeded size (without
    // the fix, last_size=80×24 ⇒ no ring ⇒ form_focus flips back off).
    key(&mut h, KeyCode::Tab);
    assert!(
        h.app().form_focus(),
        "Tab focused the form without an explicit resize (geometry uses the real size)"
    );
    typ(&mut h, "Ada");
    assert!(
        h.snapshot().contains("Ada"),
        "typing reached the bound field (interactive):\n{}",
        h.snapshot()
    );
    assert!(h.is_running());
}

#[test]
fn agent_form_opens_in_the_right_pane_and_keyboard_fills_then_submits() {
    // The goal end to end: an agent streams an A2UI form; it opens in
    // the interactive pane on the RIGHT next to chat; `Tab` focuses it;
    // typing fills a bound field (two-way → visible in the pane); `Tab`
    // to the submit button; `Enter` round-trips the spec envelope to
    // the agent (headless has no driver, so the deterministic proof the
    // whole pipeline fired is the "not connected" breadcrumb).
    let mut h = chatting(160, 44);
    h.message(Msg::Resize(Size::new(160, 44)));
    for chunk in [
        "Fill this in:\n\n```a2ui\n",
        r#"{"version":"v0.10","createSurface":{"surfaceId":"s1","catalogId":"c"}}"#,
        "\n",
        r#"{"version":"v0.10","updateComponents":{"surfaceId":"s1","components":["#,
        r#"{"id":"root","component":"Column","children":["name","submit"]},"#,
        r#"{"id":"name","component":"TextField","label":"Name","value":{"path":"/who"}},"#,
        r#"{"id":"submit","component":"Button","child":"sl","action":{"event":{"name":"save","context":{"who":{"path":"/who"}}}}},"#,
        r#"{"id":"sl","component":"Text","text":"Save"}]}}"#,
        "\n```\n\nThen press Save.",
    ] {
        h.message(Msg::Acp(AcpEvent::AgentText(chunk.to_owned())));
    }
    h.message(Msg::Acp(AcpEvent::TurnEnded("EndTurn".to_owned())));
    h.message(Msg::Acp(AcpEvent::Status("session ready".to_owned())));

    // The form opened in the right pane (chat still on the left).
    assert!(h.app().form_open(), "an interactive doc opened a pane");
    let pane = h.snapshot();
    assert!(
        pane.contains("Agent UI") && pane.contains("Name") && pane.contains("Save"),
        "the form renders in the right pane:\n{pane}"
    );
    assert!(!h.app().form_focus(), "focus starts on the composer");

    // Tab moves focus into the pane; typing fills the bound field.
    key(&mut h, KeyCode::Tab);
    assert!(h.app().form_focus(), "Tab focuses the form pane");
    typ(&mut h, "Ada");
    let typed = h.snapshot();
    assert!(
        typed.contains("Ada"),
        "the typed value is written back and shown in the pane (two-way):\n{typed}"
    );

    // Tab to the submit button, Enter submits → the spec round-trip
    // fired (headless: the not-connected breadcrumb proves the
    // pipeline; the exact envelope is unit-tested in `richui`).
    key(&mut h, KeyCode::Tab);
    key(&mut h, KeyCode::Enter);
    assert!(
        h.app()
            .transcript()
            .iter()
            .rev()
            .take(3)
            .any(|e| e.text.contains("not connected") || e.text.contains("UI action sent")),
        "Enter on the submit button round-tripped the form to the agent:\n{}",
        h.snapshot()
    );
    assert!(h.is_running());
}

#[test]
fn json_render_button_in_a_form_round_trips_when_clicked() {
    // The reported bug: a json-render Button doesn't submit. Stream a
    // json-render form with a `Button`; it must open in the pane,
    // render as a real button (not "[unsupported: Button]"), and a
    // click round-trip the action to the agent (headless: the
    // not-connected breadcrumb proves the whole pipeline fired).
    let mut h = chatting(160, 44);
    h.message(Msg::Resize(Size::new(160, 44)));
    for chunk in [
        "Here's a form:\n\n```json-render\n",
        r#"{"root":"f","elements":{"#,
        r#""f":{"type":"Box","children":["q","go"]},"#,
        r#""q":{"type":"TextInput","props":{"label":"Q","value":{"$bindState":"/q"}}},"#,
        r#""go":{"type":"Button","props":{"label":"SendIt","variant":"primary"},"on":{"press":{"action":"submitForm","params":{"q":{"$state":"/q"}}}}}"#,
        r#"},"state":{"q":""}}"#,
        "\n```\n",
    ] {
        h.message(Msg::Acp(AcpEvent::AgentText(chunk.to_owned())));
    }
    h.message(Msg::Acp(AcpEvent::TurnEnded("EndTurn".to_owned())));
    h.message(Msg::Acp(AcpEvent::Status("session ready".to_owned())));

    let screen = h.snapshot();
    assert!(
        screen.contains("SendIt") && !screen.contains("unsupported: Button"),
        "the json-render Button renders as a real button:\n{screen}"
    );
    // Click the button by label (column = char count; the border is
    // multibyte, like the other pane click tests).
    let (bx, by) = screen
        .lines()
        .enumerate()
        .find_map(|(y, line)| {
            line.find("SendIt")
                .map(|b| (line[..b].chars().count() as u16, y as u16))
        })
        .expect("the button label is on screen");
    h.message(Msg::RichClick(Position::new(bx + 1, by)));
    assert!(
        h.app()
            .transcript()
            .iter()
            .rev()
            .take(3)
            .any(|e| e.text.contains("not connected") || e.text.contains("UI action sent")),
        "clicking the json-render Button round-tripped the action to the agent:\n{}",
        h.snapshot()
    );
    assert!(h.is_running());
}

#[test]
fn agent_followup_updates_the_open_pane_in_place_closing_the_loop() {
    // The full two-way loop: a submitted action's *response* (an A2UI
    // `updateDataModel` for the same surface, no `createSurface`) folds
    // into the already-open live doc — the pane updates in place, no
    // duplicate entry — so the agent can drive the form back.
    let mut h = chatting(160, 44);
    h.message(Msg::Resize(Size::new(160, 44)));
    for chunk in [
        "Here:\n\n```a2ui\n",
        r#"{"version":"v0.10","createSurface":{"surfaceId":"s1","catalogId":"c"}}"#,
        "\n",
        r#"{"version":"v0.10","updateComponents":{"surfaceId":"s1","components":["#,
        r#"{"id":"root","component":"Column","children":["who"]},"#,
        r#"{"id":"who","component":"TextField","label":"Who","value":{"path":"/who"}}]}}"#,
        "\n```\n",
    ] {
        h.message(Msg::Acp(AcpEvent::AgentText(chunk.to_owned())));
    }
    h.message(Msg::Acp(AcpEvent::TurnEnded("EndTurn".to_owned())));
    h.message(Msg::Acp(AcpEvent::Status("session ready".to_owned())));
    assert!(h.app().form_open(), "the form opened");
    let rich_entries = |h: &Harness<ChatApp>| {
        h.app()
            .transcript()
            .iter()
            .filter(|e| e.rich.is_some())
            .count()
    };
    assert_eq!(rich_entries(&h), 1, "one live doc");

    // The agent responds with an update to the SAME surface (no
    // createSurface) — the spec's response shape.
    for chunk in [
        "Thanks!\n\n```a2ui\n",
        r#"{"version":"v0.10","updateDataModel":{"surfaceId":"s1","path":"/who","value":"Echoed"}}"#,
        "\n```\n",
    ] {
        h.message(Msg::Acp(AcpEvent::AgentText(chunk.to_owned())));
    }
    h.message(Msg::Acp(AcpEvent::TurnEnded("EndTurn".to_owned())));
    h.message(Msg::Acp(AcpEvent::Status("session ready".to_owned())));

    assert_eq!(
        rich_entries(&h),
        1,
        "the follow-up merged into the live doc — no duplicate entry"
    );
    assert!(
        h.app()
            .transcript()
            .iter()
            .any(|e| e.text.contains("the agent updated the form")),
        "a breadcrumb notes the in-place update"
    );
    assert!(
        h.snapshot().contains("Echoed"),
        "the open pane reflects the agent's update (loop closed):\n{}",
        h.snapshot()
    );
    assert!(h.is_running());
}

#[test]
fn clicking_a_rendered_json_render_control_persists_state_across_redraws() {
    // The same Phase-2 proof for the *other* interactive format: a
    // json-render `ConfirmInput` whose Yes button `setState`s `/done`,
    // with a `$cond`-bound status line. Clicking Yes must flip the
    // status and KEEP it flipped on the next every-frame re-projection
    // (the owned `JsonRenderDoc`'s model is mutated, not re-parsed).
    // The terminal is wide enough that the interactive right pane and a
    // ≥ MD_WIDTH transcript column coexist, so the *inline* click path
    // this test exercises still resolves (the pane path is covered by
    // `agent_form_opens_in_the_right_pane_and_keyboard_fills_then_submits`).
    let mut h = chatting(160, 40);
    h.message(Msg::Resize(Size::new(160, 40))); // set last_size for rich_hit
    let spec = r#"{"root":"col","elements":{
        "col":{"type":"Box","children":["status","btn"]},
        "status":{"type":"Text","props":{"text":{"$cond":{"$state":"/done"},"$then":"STATE=DONE","$else":"STATE=PENDING"}}},
        "btn":{"type":"ConfirmInput","props":{"message":"Mark done?","yesLabel":"YesGo"},"on":{"confirm":{"action":"setState","params":{"statePath":"/done","value":true}}}}
    },"state":{"done":false}}"#;
    h.message(Msg::Acp(AcpEvent::RichUi(RichUiPayload {
        format: RichUiFormat::JsonRender,
        source: spec.to_owned(),
    })));
    h.message(Msg::Acp(AcpEvent::Status("session ready".to_owned()))); // keep Chat

    let before = h.snapshot();
    assert!(
        before.contains("STATE=PENDING") && !before.contains("STATE=DONE"),
        "the json-render status line started PENDING:\n{before}"
    );

    // Column = char count before the match (the border glyph is
    // multibyte, so a byte offset would mis-target the narrow button).
    let (bx, by) = before
        .lines()
        .enumerate()
        .find_map(|(y, line)| {
            line.find("YesGo")
                .map(|b| (line[..b].chars().count() as u16, y as u16))
        })
        .expect("the Yes button is on screen");
    h.message(Msg::RichClick(Position::new(bx + 1, by)));

    let after = h.snapshot();
    assert!(
        after.contains("STATE=DONE") && !after.contains("STATE=PENDING"),
        "the json-render setState PERSISTED across re-projection:\n{after}"
    );
    assert!(h.is_running(), "the app stayed up through the interaction");
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

// ---- CC-2: in-app "Custom command…" picker entry ----

/// From the registry picker, `c` opens an inline custom-command input;
/// typing then Enter connects over local stdio (no restart, no flag). The
/// driver is the async side (inert headless, ADR 0011) but the screen
/// transition + resolved command are observable.
#[test]
fn picker_custom_command_entry_launches_without_a_flag() {
    let mut h = booted(100, 30);
    h.message(Msg::RegistryLoaded(Box::new(Registry::offline_fallback())));
    assert_eq!(h.app().screen(), Screen::Picker);
    assert!(h.app().picker_custom().is_none());

    // `c` opens the inline input; the affordance is drawn.
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::Char('c'))));
    assert!(h.app().picker_custom().is_some(), "c opens custom input");
    assert!(
        h.snapshot().contains("custom ACP command"),
        "the custom-command affordance is visible"
    );

    // Typing builds the command; `q` types, it does not quit.
    for c in "my-acp --stdio".chars() {
        h.message(Msg::Key(KeyEvent::from_code(KeyCode::Char(c))));
    }
    assert_eq!(
        h.app().picker_custom().unwrap().lines().join(" "),
        "my-acp --stdio"
    );
    assert!(h.is_running(), "typing in the custom input never quits");

    // Esc cancels back to the list (still the picker, no connect).
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::Esc)));
    assert!(h.app().picker_custom().is_none());
    assert_eq!(h.app().screen(), Screen::Picker);

    // Re-open, type, Enter → connect over local stdio (screen leaves the
    // picker; the resolved command is what we asked for).
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::Char('c'))));
    for c in "the-acp".chars() {
        h.message(Msg::Key(KeyEvent::from_code(KeyCode::Char(c))));
    }
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::Enter)));
    assert!(h.app().picker_custom().is_none(), "Enter closes the input");
    // `connect()` set the launch command to exactly what was typed. (The
    // headless driver immediately disconnects with no tokio — ADR 0011 —
    // and the reducer returns to the picker; `agent_command` is the
    // deterministic fact proving the custom command was the one launched.)
    assert_eq!(
        h.app().agent_command(),
        "the-acp",
        "the typed command is the one launched"
    );
    assert!(h.is_running());

    // Enter on an empty custom input is a no-op (stays on the picker).
    let mut h2 = booted(100, 30);
    h2.message(Msg::RegistryLoaded(Box::new(Registry::offline_fallback())));
    h2.message(Msg::Key(KeyEvent::from_code(KeyCode::Char('c'))));
    h2.message(Msg::Key(KeyEvent::from_code(KeyCode::Enter)));
    assert!(h2.app().picker_custom().is_none());
    assert_eq!(
        h2.app().screen(),
        Screen::Picker,
        "empty input does not connect"
    );
}

// ---- CC-5: live ACP wire console (raw stdio) ----

/// Raw stdio chunks are captured into a bounded ring (split per line,
/// direction-tagged); F2 and `/wire` pin the overlay open on any screen
/// (it also auto-shows while `Screen::Connecting` — the driver/connect
/// path is the async side, out of Harness by ADR 0011). The overlay is
/// hidden by default once connected (not pinned).
#[test]
fn wire_console_captures_stdio_and_toggles_with_f2() {
    use rstui_acp_client::acp::WireDir;

    let mut h = chatting(120, 30);
    assert!(h.app().wire().lines().is_empty());
    assert!(
        !h.app().wire_visible(),
        "connected & unpinned ⇒ the console is hidden (auto-closed)"
    );

    // A multi-line chunk becomes one ring entry per physical line.
    h.message(Msg::Acp(AcpEvent::Wire {
        dir: WireDir::ToAgent,
        text: "{\"jsonrpc\":\"2.0\",\"method\":\"initialize\"}\n".to_owned(),
    }));
    h.message(Msg::Acp(AcpEvent::Wire {
        dir: WireDir::FromAgent,
        text: "{\"result\":{}}".to_owned(),
    }));
    h.message(Msg::Acp(AcpEvent::Wire {
        dir: WireDir::Stderr,
        text: "agent: starting up".to_owned(),
    }));
    let lines = h.app().wire().lines();
    assert_eq!(lines.len(), 3, "blank trailing split piece is dropped");
    assert_eq!(lines[0].0, WireDir::ToAgent);
    assert!(lines[0].1.contains("initialize"));
    assert_eq!(lines[2].0, WireDir::Stderr);

    // F2 pins the console open on the chat screen and draws it.
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::F(2))));
    assert!(h.app().wire().pinned());
    assert!(h.app().wire_visible());
    let screen = h.snapshot();
    assert!(
        screen.contains("ACP wire") && screen.contains("initialize"),
        "the wire console renders the captured stdio:\n{screen}"
    );

    // F2 again unpins → hidden again (connected).
    h.message(Msg::Key(KeyEvent::from_code(KeyCode::F(2))));
    assert!(!h.app().wire().pinned());
    assert!(!h.app().wire_visible());

    // `/wire` is an equivalent toggle with a breadcrumb.
    typ(&mut h, "/wire");
    key(&mut h, KeyCode::Enter);
    assert!(h.app().wire().pinned());
    assert!(
        h.app()
            .transcript()
            .last()
            .unwrap()
            .text
            .contains("wire console: pinned")
    );
    assert!(h.is_running());
}

// ---- readline / emacs composer keybindings ----
//
// The editing logic itself is unit-tested in `readline.rs`; these assert the
// key → operation *wiring* in `chat_key` and that the moved keymap binding
// (Ctrl+X, not Ctrl+K) leaves the composer's readline keys unshadowed.

/// Sends a `Ctrl`-modified character chord.
fn ctrl_key(h: &mut Harness<ChatApp>, c: char) {
    h.message(Msg::Key(KeyEvent::new(
        KeyCode::Char(c),
        KeyModifiers::CONTROL,
    )));
}

/// Sends an `Alt`-modified (meta) character chord.
fn alt_key(h: &mut Harness<ChatApp>, c: char) {
    h.message(Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)));
}

#[test]
fn ctrl_a_and_ctrl_e_jump_to_the_line_ends() {
    let mut h = chatting(100, 30);
    typ(&mut h, "hello world");
    assert_eq!(h.app().composer().cursor(), (0, 11));
    ctrl_key(&mut h, 'a');
    assert_eq!(h.app().composer().cursor(), (0, 0), "Ctrl+A → line start");
    ctrl_key(&mut h, 'e');
    assert_eq!(h.app().composer().cursor(), (0, 11), "Ctrl+E → line end");
    // The chord moved the caret — it did not type a literal 'a' / 'e'.
    assert_eq!(composer_text(&h), "hello world");
}

#[test]
fn ctrl_b_and_ctrl_f_move_one_character() {
    let mut h = chatting(100, 30);
    typ(&mut h, "abc");
    ctrl_key(&mut h, 'b');
    assert_eq!(h.app().composer().cursor(), (0, 2));
    ctrl_key(&mut h, 'f');
    assert_eq!(h.app().composer().cursor(), (0, 3));
}

#[test]
fn alt_b_and_alt_f_move_by_whole_words() {
    let mut h = chatting(100, 30);
    typ(&mut h, "foo bar baz");
    alt_key(&mut h, 'b');
    assert_eq!(
        h.app().composer().cursor(),
        (0, 8),
        "Alt+B → start of 'baz'"
    );
    alt_key(&mut h, 'b');
    assert_eq!(
        h.app().composer().cursor(),
        (0, 4),
        "Alt+B → start of 'bar'"
    );
    alt_key(&mut h, 'f');
    assert_eq!(h.app().composer().cursor(), (0, 7), "Alt+F → end of 'bar'");
}

#[test]
fn ctrl_w_kills_a_word_and_ctrl_y_yanks_it_back() {
    let mut h = chatting(100, 30);
    typ(&mut h, "keep this");
    ctrl_key(&mut h, 'w');
    assert_eq!(composer_text(&h), "keep ", "Ctrl+W kills the word behind");
    ctrl_key(&mut h, 'y');
    assert_eq!(composer_text(&h), "keep this", "Ctrl+Y yanks it back");
}

#[test]
fn ctrl_k_kills_to_end_of_line_without_opening_the_drawer() {
    let mut h = chatting(100, 30);
    typ(&mut h, "hello world");
    ctrl_key(&mut h, 'a');
    ctrl_key(&mut h, 'k');
    assert_eq!(composer_text(&h), "", "Ctrl+K kills the whole line");
    assert!(
        !h.app().keymap_panel_open(),
        "Ctrl+K now edits the composer — it must not open the keymap drawer"
    );
}

#[test]
fn ctrl_u_kills_back_to_the_start_of_the_line() {
    let mut h = chatting(100, 30);
    typ(&mut h, "hello world");
    ctrl_key(&mut h, 'u');
    assert_eq!(composer_text(&h), "");
}

#[test]
fn alt_backspace_kills_the_word_behind_the_cursor() {
    let mut h = chatting(100, 30);
    typ(&mut h, "alpha beta");
    h.message(Msg::Key(KeyEvent::new(
        KeyCode::Backspace,
        KeyModifiers::ALT,
    )));
    assert_eq!(composer_text(&h), "alpha ");
}

#[test]
fn ctrl_t_transposes_the_characters_around_the_cursor() {
    let mut h = chatting(100, 30);
    typ(&mut h, "ab");
    ctrl_key(&mut h, 't');
    assert_eq!(composer_text(&h), "ba");
}

#[test]
fn alt_u_upcases_the_following_word() {
    let mut h = chatting(100, 30);
    typ(&mut h, "hello");
    ctrl_key(&mut h, 'a'); // caret back to the line start
    alt_key(&mut h, 'u');
    assert_eq!(composer_text(&h), "HELLO");
}

#[test]
fn ctrl_underscore_undoes_the_last_composer_edit() {
    let mut h = chatting(100, 30);
    typ(&mut h, "hello");
    ctrl_key(&mut h, 'w'); // kill the word "hello"
    assert_eq!(composer_text(&h), "");
    ctrl_key(&mut h, '_'); // undo the kill
    assert_eq!(
        composer_text(&h),
        "hello",
        "Ctrl+_ restores the killed text"
    );
}

#[test]
fn ctrl_d_on_an_empty_composer_does_not_quit() {
    let mut h = chatting(100, 30);
    assert!(h.app().composer().is_empty());
    ctrl_key(&mut h, 'd');
    assert!(
        h.is_running(),
        "Ctrl+D is delete-char-forward in the composer, never EOF/quit"
    );
    assert!(h.app().composer().is_empty());
}

#[test]
fn an_unbound_control_chord_does_not_type_a_literal_character() {
    let mut h = chatting(100, 30);
    // Ctrl+Z has no readline binding — it is ignored, never inserted as 'z'.
    ctrl_key(&mut h, 'z');
    assert_eq!(composer_text(&h), "", "an unbound Ctrl chord is not typing");
    assert!(h.is_running());
}

// ---- readline incremental history search (Ctrl+R / Ctrl+S) ----

/// Submits `s` as a prompt so it is recorded in the searchable history.
fn submit(h: &mut Harness<ChatApp>, s: &str) {
    typ(h, s);
    key(h, KeyCode::Enter);
}

#[test]
fn ctrl_r_starts_an_incremental_search_and_finds_a_match() {
    let mut h = chatting(100, 30);
    submit(&mut h, "deploy the server");
    submit(&mut h, "run the tests");

    ctrl_key(&mut h, 'r');
    assert!(h.app().isearch().is_some(), "Ctrl+R enters i-search");
    // Typing the query substring-matches a past prompt, shown live.
    typ(&mut h, "deploy");
    assert_eq!(composer_text(&h), "deploy the server");
    assert!(h.app().isearch().expect("still searching").matched);
}

#[test]
fn ctrl_r_steps_back_through_older_matches() {
    let mut h = chatting(100, 30);
    submit(&mut h, "alpha first");
    submit(&mut h, "beta");
    submit(&mut h, "alpha second");

    ctrl_key(&mut h, 'r');
    typ(&mut h, "alpha");
    assert_eq!(composer_text(&h), "alpha second", "the newest match first");
    ctrl_key(&mut h, 'r');
    assert_eq!(
        composer_text(&h),
        "alpha first",
        "Ctrl+R again steps to the older match"
    );
}

#[test]
fn the_isearch_prompt_is_shown_in_the_composer_title() {
    let mut h = chatting(100, 30);
    submit(&mut h, "searchable entry");
    ctrl_key(&mut h, 'r');
    typ(&mut h, "search");
    let s = h.snapshot();
    assert!(
        s.contains("reverse-i-search"),
        "the i-search prompt renders in the composer title:\n{s}"
    );
}

#[test]
fn a_query_with_no_match_marks_the_search_failing() {
    let mut h = chatting(100, 30);
    submit(&mut h, "the only entry");
    ctrl_key(&mut h, 'r');
    typ(&mut h, "zzzzz");
    assert!(
        !h.app().isearch().expect("still searching").matched,
        "an unmatched query marks the search failing"
    );
}

#[test]
fn ctrl_g_aborts_the_search_and_restores_the_draft() {
    let mut h = chatting(100, 30);
    submit(&mut h, "history one");
    typ(&mut h, "my draft");

    ctrl_key(&mut h, 'r');
    typ(&mut h, "one");
    assert_eq!(
        composer_text(&h),
        "history one",
        "the match replaces the draft"
    );
    ctrl_key(&mut h, 'g');
    assert!(h.app().isearch().is_none(), "Ctrl+G ends the search");
    assert_eq!(
        composer_text(&h),
        "my draft",
        "Ctrl+G restores the pre-search draft"
    );
}

#[test]
fn enter_during_isearch_accepts_the_match_and_submits_it() {
    let mut h = chatting(100, 30);
    submit(&mut h, "build everything");
    typ(&mut h, "scratch");

    ctrl_key(&mut h, 'r');
    typ(&mut h, "build");
    assert_eq!(composer_text(&h), "build everything");
    key(&mut h, KeyCode::Enter);
    assert!(h.app().isearch().is_none(), "Enter ends the search");
    assert!(
        h.app().composer().is_empty(),
        "Enter accepted the found line and submitted it"
    );
    assert!(h.is_running());
}
