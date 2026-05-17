//! The landing screen: an [`Avatar`] + heading, a [`Markdown`] tour beside a
//! quickstart [`Card`], a [`Kbd`] keymap strip, and a labelled [`Divider`]
//! with [`Badge`]s. Stateless — it only reads the theme.

use rstui_core::{Constraint, Layout, Line, Position, Rect, Style, stylize::Stylize};
use rstui_runtime::Frame;
use rstui_widgets::{
    Avatar, Badge, BadgeLevel, Block, BorderType, Card, Divider, Flow, Kbd, Markdown, Paragraph,
    Wrap,
};

use crate::screens::ScreenOutcome;
use crate::theme::Theme;

/// The tour copy, rendered by the real Markdown widget.
const TOUR: &str = "\
# Everything, interactive

This is the **rstui kitchen sink** — one Elm-style app that drives every
widget in the catalog.

- Pick a screen from the rail with the *mouse* or the arrows
- `Tab` toggles focus between the rail and the screen
- Every screen responds to the keyboard *and* the mouse
- `:` opens a fuzzy command palette, `?` the keymap
- Docs: [the rstui repo](https://github.com/andymac4182/rstui) — click it!

Colours are 24-bit truecolor; `g` swaps the whole palette live.";

/// Welcome ignores all keys (so `←` falls back to the rail).
pub(crate) fn on_key(_code: rstui_core::KeyCode) -> ScreenOutcome {
    ScreenOutcome::ignored()
}

/// A click on the tour's Markdown link follows it (toasts the href). The
/// layout here mirrors [`view`] exactly so the hit-test lands on the label.
pub(crate) fn on_click(pos: Position, content: Rect) -> ScreenOutcome {
    let [_, mid, _, _] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(4),
        Constraint::Length(1),
    ])
    .areas(content);
    let [tour, _card] = Layout::horizontal([Constraint::Fill(3), Constraint::Fill(2)]).areas(mid);
    if tour.contains(pos) {
        let md = Markdown::new(TOUR).block(Block::bordered().border_type(BorderType::Rounded));
        if let Some(i) = md.link_at(pos, tour) {
            if let Some(link) = md.links().get(i) {
                return ScreenOutcome::with_toast(
                    crate::screens::ToastLevel::Success,
                    format!("Open link → {}", link.href),
                );
            }
        }
    }
    ScreenOutcome::ignored()
}

/// Draw the welcome screen.
pub(crate) fn view(theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
    let [banner, mid, keys, foot] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(4),
        Constraint::Length(1),
    ])
    .areas(area);

    // Banner: an Avatar swatch beside a bold heading.
    let [badge, heading] =
        Layout::horizontal([Constraint::Length(6), Constraint::Fill(1)]).areas(banner);
    frame.render_widget(
        Avatar::new("RS").style(Style::new().fg(theme.base).bg(theme.accent)),
        Rect::new(badge.x, badge.y, 5, 3),
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("rstui kitchen sink".bold().fg(theme.accent)),
            Line::from("56 widgets · 8 screens · full colour".fg(theme.dim)),
            Line::from("keyboard + mouse · headless-testable".fg(theme.dim)),
        ])
        .style(theme.body()),
        heading,
    );

    // The Markdown tour beside a quickstart Card.
    let [tour, card] = Layout::horizontal([Constraint::Fill(3), Constraint::Fill(2)]).areas(mid);
    frame.render_widget(
        Markdown::new(TOUR).style(theme.body()).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .title(Line::from(" Tour ").style(theme.heading()))
                .border_style(theme.border())
                .style(theme.body()),
        ),
        tour,
    );

    let quick = Card::new()
        .title(Line::from(" Quickstart ").style(theme.heading()))
        .footer(Line::from(" press 2 → Forms ").style(theme.caption()))
        .block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(theme.border()),
        )
        .style(theme.body());
    let quick_inner = quick.inner(card);
    frame.render_widget(quick, card);
    frame.render_widget(
        Paragraph::new(
            "1  Welcome (here)\n2  Forms & Input\n3  Navigation\n4  Data Display\n5  Feedback\n6  Containers\n7  Rich Text\n8  Colour Lab\n\nOr press : and type.",
        )
        .style(theme.body())
        .wrap(Wrap { trim: true }),
        quick_inner,
    );

    // A Kbd strip of the load-bearing keys.
    let kbd_style = Style::new().fg(theme.text).bg(theme.surface);
    let key_style = Style::new().fg(theme.base).bg(theme.accent);
    frame.render_widget(
        Kbd::new(["Tab", "↑", "↓", "Enter", "Space", ":", "?", "g", "q"])
            .style(kbd_style)
            .key_style(key_style)
            .separator_style(Style::new().fg(theme.dim)),
        Rect::new(keys.x, keys.y, keys.width, 1),
    );
    // A `Flow` chip-cloud: a wrapped run of variable-width pills (the one
    // layout plain `Layout` can't express — break points depend on content).
    frame.render_widget(
        Flow::new([
            " rust ",
            " pure projection ",
            " no retained tree ",
            " 24-bit colour ",
            " mouse + keyboard ",
            " headless-tested ",
            " 18 screens ",
            " 10 experiences ",
            " Elm runtime ",
        ])
        .gap(1, 0)
        .style(Style::new().fg(theme.base).bg(theme.accent_alt)),
        Rect::new(keys.x, keys.y + 1, keys.width, 3),
    );

    // A labelled divider with status badges.
    frame.render_widget(
        Divider::new()
            .label(Line::from(" status ").style(theme.caption()))
            .style(theme.border()),
        Rect::new(foot.x, foot.y, foot.width.saturating_sub(28), 1),
    );
    let bx = foot.right().saturating_sub(26);
    frame.render_widget(
        Badge::new("stable").level(BadgeLevel::Success),
        Rect::new(bx, foot.y, 10, 1),
    );
    frame.render_widget(
        Badge::new("v0.0.1").level(BadgeLevel::Info),
        Rect::new(bx + 11, foot.y, 12, 1),
    );
}
