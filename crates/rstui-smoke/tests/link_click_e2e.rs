//! End-to-end lock for clickable links: a real `App` turns a terminal mouse
//! event into a link activation through the production `on_event → update →
//! view` pipeline, driven headlessly by `Harness`.
//!
//! The widget layer (`Markdown::link_activation_at`) is unit-tested; before
//! this, the *full pipeline* — `Event::Mouse(position)` → reducer → resolved
//! `href` — had no regression lock (only ad-hoc kitchen-sink usage). This is
//! the immediate-mode equivalent of Textual's per-span click meta: rstui has
//! no retained DOM, so the reducer hit-tests the click against the same area
//! it rendered into. If that contract breaks, this fails by name.

use rstui_core::event::{MouseButton, MouseEvent, MouseEventKind};
use rstui_core::{Event, KeyModifiers, Position, Rect};
use rstui_runtime::{App, Cmd, Frame, Harness};
use rstui_widgets::Markdown;

/// The markdown the app renders. One link in the middle of plain text.
const SRC: &str = "go [here](https://rstui.test/docs) ok";

/// A minimal browser-less app: clicking a link records its href, exactly as
/// a real app would before handing it to an OS-open command.
struct LinkApp {
    /// Terminal size, so the reducer can hit-test against the rendered area.
    area: Rect,
    /// The href of the last activated link (the "opened URL").
    opened: Option<String>,
    /// Count of clicks that hit no link (proves plain text does nothing).
    misses: u32,
}

impl LinkApp {
    fn new(w: u16, h: u16) -> Self {
        Self {
            area: Rect::new(0, 0, w, h),
            opened: None,
            misses: 0,
        }
    }
}

enum Msg {
    Click(Position),
}

impl App for LinkApp {
    type Message = Msg;

    fn on_event(&self, event: Event) -> Option<Msg> {
        match event {
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                position,
                ..
            }) => Some(Msg::Click(position)),
            _ => None,
        }
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        match message {
            Msg::Click(pos) => {
                // The whole clickable-link contract in one reducer line.
                match Markdown::new(SRC).link_activation_at(pos, self.area) {
                    Some(act) => self.opened = Some(act.href),
                    None => self.misses += 1,
                }
            }
        }
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        frame.render_widget(Markdown::new(SRC), area);
    }
}

/// A left-button release at `pos` — how a terminal delivers a click.
fn click(x: u16, y: u16) -> Event {
    Event::Mouse(MouseEvent::new(
        MouseEventKind::Up(MouseButton::Left),
        Position::new(x, y),
        KeyModifiers::NONE,
    ))
}

#[test]
fn clicking_a_markdown_link_activates_its_href_through_the_loop() {
    let (w, h) = (40u16, 4u16);
    let mut harness = Harness::new(LinkApp::new(w, h), w, h);

    // Find where the link actually rendered (no hardcoded column).
    let region = Markdown::new(SRC)
        .link_regions(Rect::new(0, 0, w, h))
        .into_iter()
        .next()
        .expect("the document has one link");
    let hit = Position::new(region.rect.x + region.rect.width / 2, region.rect.y);

    // Click the link: the reducer resolves it to the href, end to end.
    harness.handle(click(hit.x, hit.y));
    assert_eq!(
        harness.app().opened.as_deref(),
        Some("https://rstui.test/docs"),
        "a click on the link must activate its href through the pipeline"
    );
    assert_eq!(harness.app().misses, 0);
    assert!(harness.is_running());
}

#[test]
fn clicking_plain_text_activates_nothing() {
    let (w, h) = (40u16, 4u16);
    let mut harness = Harness::new(LinkApp::new(w, h), w, h);

    // Column 0 row 0 is the 'g' of "go" — plain text, never a link.
    harness.handle(click(0, 0));
    assert_eq!(harness.app().opened, None, "plain text is not a link");
    assert_eq!(harness.app().misses, 1, "the miss was counted");

    // A click well outside any content is also a clean miss (total).
    harness.handle(click(39, 3));
    assert_eq!(harness.app().opened, None);
    assert_eq!(harness.app().misses, 2);
}
