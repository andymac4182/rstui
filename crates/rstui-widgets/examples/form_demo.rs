//! Exercises [`Form`] the way a real settings dialog will: a framed,
//! label-aligned column whose controls are an [`Input`], a [`Switch`], and a
//! [`Slider`] — each rendered by the *caller* into the rect [`Form::layout`]
//! hands back.
//!
//! The load-bearing point: [`Form`] owns **no application state**. The field
//! values (`TextEdit`, the `bool`, the `f64`) are plain caller-owned model
//! data — exactly what an app's model holds and its reducer mutates. `Form`
//! is a *pure layout projection*: [`Form::layout`] is a pure geometry function
//! (like [`Block::inner`]) returning each field's control rect, and
//! [`Form::render`] draws only the labels, the per-field help line, and the
//! frame. The controls are composed in by the caller, so focus/edit/validity
//! all stay in the model. Running over a [`TestBackend`] keeps it TTY-free, so
//! it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example form_demo
//! ```

use rstui_core::{Color, Style, Terminal, TestBackend, TextEdit};
use rstui_widgets::{Block, Form, FormField, Input, Slider, Switch};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(40, 9)).expect("TestBackend is infallible");

    // Every field's value is plain caller-owned model state — `Form` never
    // sees it. A reducer would mutate these in `update`; here they are fixed.
    let name = TextEdit::from_value("Ada Lovelace");
    let notifications = true;
    let volume = 65.0_f64;
    let focused_field = 0usize; // the reducer owns "which field is focused"

    terminal
        .draw(|frame| {
            let form = Form::new()
                .block(Block::bordered().title("Profile"))
                .label_width(8)
                .row_spacing(1)
                .label_style(Style::new().fg(Color::Cyan))
                .help_style(Style::new().fg(Color::DarkGray))
                .field(FormField::new("Name", 1).help("shown on your public profile"))
                .field(FormField::new("Notify", 1))
                .field(FormField::new("Volume", 1));

            // `layout` is a pure geometry function — no state, like
            // `Block::inner`. The caller renders its own controls into it.
            let rects = form.layout(frame.area());
            frame.render_widget(form, frame.area());

            let focus_style = Style::new().fg(Color::Black).bg(Color::Cyan);

            frame.render_widget(
                Input::new(&name)
                    .focused(focused_field == 0)
                    .focus_style(focus_style),
                rects[0],
            );
            frame.render_widget(
                Switch::new()
                    .on(notifications)
                    .on_label("on")
                    .off_label("off")
                    .focused(focused_field == 1)
                    .focus_style(focus_style),
                rects[1],
            );
            frame.render_widget(
                Slider::new()
                    .range(0.0, 100.0)
                    .value(volume)
                    .value_label("65")
                    .focused(focused_field == 2)
                    .thumb_style(Style::new().fg(Color::Cyan))
                    .focus_style(focus_style),
                rects[2],
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
