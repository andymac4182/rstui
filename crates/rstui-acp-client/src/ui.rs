//! The pure `view`: a projection of [`ChatApp`] state
//! onto the frame. No mutation, no I/O — exactly the rstui discipline that
//! keeps every screen `Harness`-testable.
//!
//! Composition only uses the foundational rstui primitives (`Layout`,
//! `Block`, `Paragraph`, `List`, styled `Line`/`Span`, and direct `Buffer`
//! writes for opaque overlays), so the whole UI is a deterministic function
//! of state.

use rstui_core::{Color, Constraint, Layout, Line, Position, Rect, Span, Style};
use rstui_runtime::Frame;
use rstui_widgets::{Block, KeymapView, List, ListItem, Markdown, Paragraph, Wrap};

use crate::app::{ChatApp, MD_WIDTH, Role, Screen};
use crate::plugin::FooterSegment;

const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Renders the whole client for one frame.
pub fn render(app: &ChatApp, frame: &mut Frame<'_>) {
    let area = frame.area();
    if area.width < 4 || area.height < 4 {
        return;
    }
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(area);

    render_header(app, frame, header);
    match app.screen() {
        Screen::Picker => render_picker(app, frame, body),
        Screen::Connecting | Screen::Chat => render_chat(app, frame, body),
    }
    render_footer(app, frame, footer);

    if app.pager().open() {
        render_transcript_pager(app, frame, area);
    } else if app.help_visible() {
        render_help(app, frame, area);
    } else if app.log_visible() {
        render_log(app, frame, area);
    } else if app.status_visible() {
        render_status(app, frame, area);
    } else if app.model_picker_open() {
        render_model_picker(app, frame, area);
    } else if app.mode_picker_open() {
        render_mode_picker(app, frame, area);
    } else if app.resume_picker_open() {
        render_resume_picker(app, frame, area);
    } else if app.auth_picker_open() {
        render_auth_picker(app, frame, area);
    } else if app.plugins_overlay() {
        render_plugins_overlay(app, frame, area);
    } else if app.keymap_panel_open() {
        render_keymap_panel(app, frame, area);
    }
    if let Some(perm) = app.pending_permission() {
        render_permission(perm, frame, area);
    }
    if let Some(ask) = app.ask() {
        render_ask(ask, frame, area);
    }
    if let Some(m) = app.modal() {
        render_modal(m, frame, area);
    }
    if app.picking() {
        render_theme_picker(app, frame, area);
    }
    render_toasts(app, frame, area);
}

/// The `/theme` picker — the reusable [`rstui_theme::ThemePicker`] in a
/// centred panel. The whole client is already painted in the highlighted
/// theme (live preview), so the panel previews it too.
fn render_theme_picker(app: &ChatApp, frame: &mut Frame<'_>, area: Rect) {
    let t = app.theme();
    let w = area.width.saturating_mul(3) / 5;
    let h = area.height.saturating_mul(7) / 10;
    let rect = centered(area, w.clamp(28, 72), h.clamp(8, 30));
    let block = Block::bordered()
        .title(format!(" Theme — {} ", t.name))
        .border_style(t.accent_text())
        .style(t.base());
    let inner = block.inner(rect);
    clear(frame, rect);
    frame.render_widget(block, rect);
    frame.render_widget(
        rstui_theme::ThemePicker::new(app.theme_picker())
            .title("Browse · preview live")
            .style(t.base())
            .highlight_style(t.selection()),
        inner,
    );
}

fn render_modal(m: &crate::app::ModalState, frame: &mut Frame<'_>, area: Rect) {
    let h = (m.body().len() as u16 + 6).clamp(7, area.height.saturating_sub(2).max(7));
    let rect = centered(area, area.width.min(70), h);
    let block = Block::bordered().title(format!(" {} ", m.title()));
    let inner = block.inner(rect);
    clear(frame, rect);
    frame.render_widget(block, rect);

    let [body_area, _, btn_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    let body: Vec<Line> = m
        .body()
        .iter()
        .map(|l| Line::styled(l.clone(), Style::new().fg(Color::Gray)))
        .collect();
    frame.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), body_area);

    // A horizontal row of buttons; the selected one is highlighted.
    let mut spans: Vec<Span> = Vec::new();
    for (i, b) in m.buttons().iter().enumerate() {
        let style = if i == m.selected() {
            Style::new().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::new().fg(Color::White)
        };
        spans.push(Span::styled(format!(" {b} "), style));
        spans.push(Span::raw("  "));
    }
    spans.push(Span::styled(
        "  (←→ select · Enter · Esc)",
        Style::new().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), btn_area);
}

fn render_header(app: &ChatApp, frame: &mut Frame<'_>, area: Rect) {
    let spinner = if app.is_streaming() {
        format!(" {} ", SPINNER[app.spinner_frame() % SPINNER.len()])
    } else {
        " ▪ ".to_owned()
    };
    let title = format!(
        "{spinner}rstui-acp-client │ {}",
        truncate(app.status_line(), area.width.saturating_sub(20) as usize)
    );
    let style = app.theme().header();
    fill(frame, area, style);
    frame.buffer_mut().set_str(
        Position::new(area.x, area.y),
        &clamp(&title, area.width),
        style,
    );
    // Live render rate, right-aligned on the bar (when there is room).
    let fps = app.fps_label();
    let fw = fps.chars().count() as u16;
    if area.width > fw + 2 {
        frame.buffer_mut().set_str(
            Position::new(area.x + area.width - fw - 1, area.y),
            &fps,
            style,
        );
    }
}

fn render_footer(app: &ChatApp, frame: &mut Frame<'_>, area: Rect) {
    fill(frame, area, app.theme().footer());
    let mut x = area.x;
    for seg in app.footer_segments() {
        x = draw_segment(frame, area, x, seg);
        if x >= area.x + area.width {
            return;
        }
    }
    // Right-aligned hint when there is room.
    let hint = "F1 help · Ctrl+K keymap · Esc cancel · Ctrl+C quit";
    let hw = hint.chars().count() as u16;
    if area.x + area.width > x + hw + 2 {
        let hx = area.x + area.width - hw;
        frame
            .buffer_mut()
            .set_str(Position::new(hx, area.y), hint, app.theme().footer());
    }
}

fn draw_segment(frame: &mut Frame<'_>, area: Rect, x: u16, seg: &FooterSegment) -> u16 {
    let text = format!(" {} ", seg.text);
    let style = Style::new()
        .fg(seg.fg.as_deref().map_or(Color::White, color_by_name))
        .bg(seg.bg.as_deref().map_or(Color::DarkGray, color_by_name));
    let avail = (area.x + area.width).saturating_sub(x);
    let shown = clamp(&text, avail);
    frame
        .buffer_mut()
        .set_str(Position::new(x, area.y), &shown, style);
    x + shown.chars().count() as u16
}

fn render_picker(app: &ChatApp, frame: &mut Frame<'_>, area: Rect) {
    let reg = app.registry();
    let block = Block::bordered().title(" Agents — ACP registry ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if reg.agents.is_empty() {
        frame.render_widget(
            Paragraph::new("Loading the ACP registry…\n\nIf this stays empty, network/curl is unavailable; the built-in agents will appear shortly.")
                .wrap(Wrap { trim: false }),
            inner,
        );
        return;
    }

    let items: Vec<ListItem> = reg
        .agents
        .iter()
        .map(|a| {
            let avail = a
                .command
                .as_ref()
                .map_or("(no command for this platform)", |_| "");
            let head = Span::styled(format!("{}  ", a.name), Style::new().fg(Color::Cyan));
            let desc = Span::styled(
                format!("{} {}", truncate(&a.description, 70), avail),
                Style::new().fg(Color::Gray),
            );
            ListItem::new(Line::from(vec![head, desc]))
        })
        .collect();

    let list = List::new(items)
        .highlight_symbol("▸ ")
        .highlight_style(app.theme().selection())
        .selected(Some(app.picker_selected()))
        .offset(picker_offset(app.picker_selected(), inner.height));
    frame.render_widget(list, inner);
}

fn render_chat(app: &ChatApp, frame: &mut Frame<'_>, area: Rect) {
    // Dock the todo sidebar on the right when there is room for it.
    let main = if app.sidebar_visible() && area.width >= 60 {
        let sidebar_w = area.width / 4;
        let [m, side] = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(sidebar_w.clamp(24, 40)),
        ])
        .areas(area);
        render_sidebar(app, frame, side);
        m
    } else {
        area
    };
    let [transcript_area, composer_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(5)]).areas(main);

    // ---- transcript ----
    let block = Block::bordered().title(transcript_title(app));
    let inner = block.inner(transcript_area);
    frame.render_widget(block, transcript_area);

    let lines = transcript_lines(app);
    let para = Paragraph::new(lines).wrap(Wrap { trim: false });
    let total = para.line_count(inner.width.max(1)) as u16;
    let max_scroll = total.saturating_sub(inner.height);
    let scroll = if app_follows(app) {
        max_scroll
    } else {
        app.scroll().min(max_scroll)
    };
    frame.render_widget(para.scroll((0, scroll)), inner);

    // ---- composer ----
    let cblock = Block::bordered().title(composer_title(app));
    let cinner = cblock.inner(composer_area);
    frame.render_widget(cblock, composer_area);

    let comp_lines: Vec<Line> = app
        .composer()
        .lines()
        .iter()
        .map(|l| Line::raw(l.clone()))
        .collect();
    let placeholder = comp_lines.len() == 1 && comp_lines[0].width() == 0;
    if placeholder {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "Type a message, or /help for commands",
                Style::new().fg(Color::DarkGray),
            )),
            cinner,
        );
    } else {
        frame.render_widget(
            Paragraph::new(comp_lines).wrap(Wrap { trim: false }),
            cinner,
        );
    }

    // Park the caret in the composer when it owns focus.
    if app.pending_permission().is_none() && app.ask().is_none() && !app.help_visible() {
        let (row, col) = app.composer().cursor();
        let cx = cinner.x + (col as u16).min(cinner.width.saturating_sub(1));
        let cy = cinner.y + (row as u16).min(cinner.height.saturating_sub(1));
        frame.set_cursor_position(Position::new(cx, cy));
    }

    // The slash-command autocomplete / `@`-mention popup floats just above
    // the composer (they are mutually exclusive, so at most one draws).
    render_completion(app, frame, composer_area);
    render_mention(app, frame, composer_area);
}

/// The `@`-mention file-completion popup — fuzzy workspace paths for the
/// `@token` at the cursor (Codex's `@` mention). Pure projection of
/// [`crate::app::MentionState`].
fn render_mention(app: &ChatApp, frame: &mut Frame<'_>, composer_area: Rect) {
    let Some(m) = app.mention() else { return };
    if m.items.is_empty() {
        return;
    }
    let above = composer_area.y.saturating_sub(1);
    if above < 3 {
        return;
    }
    let rows = (m.items.len() as u16).min(above.saturating_sub(2));
    let h = rows + 2;
    let rect = Rect {
        x: composer_area.x,
        y: composer_area.y - h,
        width: composer_area.width,
        height: h,
    };
    let block = Block::bordered().title(" @ files — ↑↓ Tab/Enter Esc ");
    let body = block.inner(rect);
    clear(frame, rect);
    frame.render_widget(block, rect);

    let items: Vec<ListItem> = m
        .items
        .iter()
        .map(|p| {
            ListItem::new(Line::from(Span::styled(
                format!("@{}", truncate(p, body.width.saturating_sub(2) as usize)),
                Style::new().fg(Color::Cyan),
            )))
        })
        .collect();
    frame.render_widget(
        List::new(items)
            .highlight_symbol("▸ ")
            .highlight_style(app.theme().selection())
            .selected(Some(m.selected)),
        body,
    );
}

fn render_completion(app: &ChatApp, frame: &mut Frame<'_>, composer_area: Rect) {
    let Some(comp) = app.completion() else { return };
    if comp.items.is_empty() {
        return;
    }
    let above = composer_area.y.saturating_sub(1);
    if above < 3 {
        return;
    }
    let rows = (comp.items.len() as u16).min(above.saturating_sub(2));
    let h = rows + 2;
    let rect = Rect {
        x: composer_area.x,
        y: composer_area.y - h,
        width: composer_area.width,
        height: h,
    };
    let name_w = comp
        .items
        .iter()
        .map(|c| c.name.chars().count())
        .max()
        .unwrap_or(0)
        + 1;
    let block = Block::bordered().title(" commands — ↑↓ Tab/Enter Esc ");
    let body = block.inner(rect);
    clear(frame, rect);
    frame.render_widget(block, rect);

    let items: Vec<ListItem> = comp
        .items
        .iter()
        .map(|c| {
            let tag = match &c.source {
                crate::app::CommandSource::Builtin => "",
                crate::app::CommandSource::Plugin(_) => " ⚙",
                crate::app::CommandSource::Agent => " ◆",
            };
            let head = format!("/{:<width$}{}  ", c.name, tag, width = name_w);
            ListItem::new(Line::from(vec![
                Span::styled(head, Style::new().fg(Color::Cyan)),
                Span::styled(
                    truncate(
                        &c.description,
                        body.width.saturating_sub(name_w as u16 + 6) as usize,
                    ),
                    Style::new().fg(Color::Gray),
                ),
            ]))
        })
        .collect();
    frame.render_widget(
        List::new(items)
            .highlight_symbol("▸ ")
            .highlight_style(app.theme().selection())
            .selected(Some(comp.selected)),
        body,
    );
}

fn section(out: &mut Vec<Line<'static>>, title: &str) {
    if !out.is_empty() {
        out.push(Line::raw(""));
    }
    out.push(Line::styled(
        format!("── {title} "),
        Style::new().fg(Color::Cyan),
    ));
}

/// The docked sidebar: stacks the Todos panel, any plugin-contributed
/// panels, and a plugin status/commands block — the opencode "sidebar
/// slots" analogue.
fn render_sidebar(app: &ChatApp, frame: &mut Frame<'_>, area: Rect) {
    let block = Block::bordered().title(" Sidebar ");
    let inner = block.inner(area);
    clear(frame, area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line<'static>> = Vec::new();

    if !app.todos().is_empty() {
        let (done, total) = app.todo_progress();
        section(&mut lines, &format!("Todos {done}/{total}"));
        for todo in app.todos() {
            // opencode glyphs/colours: ✓ done (muted), • in-progress
            // (warning), space pending (muted).
            let (glyph, color) = match todo.status {
                crate::acp::TodoStatus::Completed => ("✓", Color::DarkGray),
                crate::acp::TodoStatus::InProgress => ("•", Color::Yellow),
                crate::acp::TodoStatus::Pending => (" ", Color::Gray),
            };
            lines.push(Line::from(vec![
                Span::styled(format!("[{glyph}] "), Style::new().fg(color)),
                Span::styled(todo.content.clone(), Style::new().fg(color)),
            ]));
        }
    }

    for (plugin, (title, body)) in app.panels() {
        section(&mut lines, &format!("{title} ({plugin})"));
        for l in body {
            lines.push(Line::styled(l.clone(), Style::new().fg(Color::Gray)));
        }
    }

    if !app.statuses().is_empty() {
        section(&mut lines, "Status");
        for (k, v) in app.statuses() {
            lines.push(Line::from(vec![
                Span::styled(format!("{k}: "), Style::new().fg(Color::DarkGray)),
                Span::styled(v.clone(), Style::new().fg(Color::White)),
            ]));
        }
    }

    let names = app.plugin_names();
    if !names.is_empty() {
        section(&mut lines, "Plugins");
        for n in names {
            lines.push(Line::styled(format!("⚙ {n}"), Style::new().fg(Color::Gray)));
        }
    }

    if lines.is_empty() {
        lines.push(Line::styled(
            "No todos or plugin content yet.",
            Style::new().fg(Color::DarkGray),
        ));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// The `/plugins` overlay: a plugin-manager panel — each loaded plugin with
/// the slash commands it registered and any status keys it set.
fn render_plugins_overlay(app: &ChatApp, frame: &mut Frame<'_>, area: Rect) {
    let rect = centered(area, area.width.min(78), area.height.clamp(10, 22));
    let block = Block::bordered().title(" Plugins (Esc to close) ");
    let inner = block.inner(rect);
    clear(frame, rect);
    frame.render_widget(block, rect);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let names = app.plugin_set();
    if names.is_empty() {
        lines.push(Line::styled(
            "No plugins loaded. Launch with --plugin <cmd>, or drop the",
            Style::new().fg(Color::Gray),
        ));
        lines.push(Line::styled(
            "reference plugins next to the binary for auto-discovery.",
            Style::new().fg(Color::Gray),
        ));
    }
    for name in &names {
        lines.push(Line::styled(
            format!("⚙ {name}"),
            Style::new().fg(Color::Cyan),
        ));
        for spec in app.command_specs() {
            if let crate::app::CommandSource::Plugin(p) = &spec.source {
                if p == name {
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("    /{:<12} ", spec.name),
                            Style::new().fg(Color::White),
                        ),
                        Span::styled(spec.description.clone(), Style::new().fg(Color::Gray)),
                    ]));
                }
            }
        }
        if let Some((title, body)) = app.panels().get(name) {
            lines.push(Line::styled(
                format!("    ▸ panel “{title}” ({} lines)", body.len()),
                Style::new().fg(Color::DarkGray),
            ));
        }
    }
    if !app.statuses().is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled("Status keys:", Style::new().fg(Color::Yellow)));
        for (k, v) in app.statuses() {
            lines.push(Line::styled(
                format!("  {k} = {v}"),
                Style::new().fg(Color::Gray),
            ));
        }
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn transcript_title(app: &ChatApp) -> String {
    if app.screen() == Screen::Connecting {
        " Transcript — connecting… ".to_owned()
    } else {
        format!(" Transcript — {} messages ", app.transcript().len())
    }
}

fn composer_title(app: &ChatApp) -> String {
    if app.is_streaming() {
        " Message — Esc cancels the streaming turn ".to_owned()
    } else {
        " Message — Enter send · Shift+Enter newline · type / for commands ".to_owned()
    }
}

fn transcript_lines(app: &ChatApp) -> Vec<Line<'static>> {
    let ascii = std::env::var("RSTUI_ACP_TOOL_ICONS")
        .map(|v| v == "ascii")
        .unwrap_or(false);
    let mut out: Vec<Line<'static>> = Vec::new();
    for entry in app.transcript() {
        if entry.role == Role::Tool {
            if let Some(tool) = app.tool_call(&entry.text) {
                tool_card_lines(tool, app.details(), app.spinner_frame(), ascii, &mut out);
                out.push(Line::raw(""));
                continue;
            }
        }
        let (label, color) = match entry.role {
            Role::User => ("you", Color::Green),
            Role::Agent => ("agent", Color::Cyan),
            Role::Thought => ("thinking", Color::DarkGray),
            Role::Tool => ("tool", Color::Yellow),
            Role::Plan => ("plan", Color::Magenta),
            Role::RichUi => ("ui", Color::Blue),
            Role::System => ("·", Color::Gray),
        };
        out.push(Line::from(vec![Span::styled(
            format!("{label}:"),
            Style::new().fg(color),
        )]));

        // Parse markdown for agent responses to support links. UI-1/MD-1:
        // reuse the parse cached in `update` for finalized (non-last)
        // entries; the still-streaming last entry has no cache and is
        // parsed fresh here exactly as before — and a fresh parse is also
        // the fallback for any uncached entry, so output is byte-identical
        // while the per-frame whole-transcript re-parse is eliminated.
        if entry.role == Role::RichUi {
            // ADR 0017: re-project the agent's declarative UI document
            // from its verbatim source every frame (pure projection — no
            // retained tree), bounded so one document cannot dominate the
            // transcript.
            for line in crate::acp::render_rich_ui(&entry.text, MD_WIDTH, 40) {
                out.push(line);
            }
        } else if entry.role == Role::Agent {
            let fresh;
            let md_lines: &[Line<'static>] = match &entry.md_cache {
                Some(cached) => cached,
                None => {
                    fresh = Markdown::new(&entry.text).lines(MD_WIDTH);
                    &fresh
                }
            };
            for line in md_lines {
                // Indent each markdown line
                let mut spans = vec![Span::styled("  ", Style::new())];
                spans.extend(line.spans.iter().cloned());
                out.push(Line::from(spans));
            }
        } else {
            for raw in entry.text.split('\n') {
                out.push(Line::from(vec![
                    Span::styled("  ", Style::new()),
                    Span::styled(raw.to_owned(), Style::new().fg(line_color(entry.role))),
                ]));
            }
        }
        out.push(Line::raw(""));
    }
    if out.is_empty() {
        out.push(Line::styled(
            "No messages yet — say hello to the agent.",
            Style::new().fg(Color::DarkGray),
        ));
    }
    out
}

/// The per-kind glyph (opencode-inspired); ASCII set via
/// `RSTUI_ACP_TOOL_ICONS=ascii` — a deliberate customization seam.
fn tool_icon(kind: crate::acp::ToolKind, ascii: bool) -> &'static str {
    use crate::acp::ToolKind as K;
    if ascii {
        return match kind {
            K::Read => "R",
            K::Edit => "W",
            K::Delete => "D",
            K::Move => "M",
            K::Search => "S",
            K::Execute => "$",
            K::Think => "*",
            K::Fetch => "%",
            K::SwitchMode => "~",
            K::Other => "+",
        };
    }
    match kind {
        K::Read => "→",
        K::Edit => "✎",
        K::Delete => "␡",
        K::Move => "↦",
        K::Search => "✱",
        K::Execute => "$",
        K::Think => "…",
        K::Fetch => "%",
        K::SwitchMode => "⇄",
        K::Other => "⚙",
    }
}

/// `(glyph, label, colour)` for a tool status.
fn tool_status_style(
    status: crate::acp::ToolStatus,
    spinner: usize,
) -> (String, &'static str, Color) {
    use crate::acp::ToolStatus as S;
    match status {
        S::Pending => ("~".to_owned(), "pending", Color::Gray),
        S::InProgress => (
            SPINNER[spinner % SPINNER.len()].to_string(),
            "running",
            Color::Yellow,
        ),
        S::Completed => ("✓".to_owned(), "done", Color::Green),
        S::Failed => ("✗".to_owned(), "failed", Color::Red),
    }
}

/// Renders one tool call as a rich card (header + input + collapsible body).
fn tool_card_lines(
    tool: &crate::acp::ToolCallInfo,
    details: bool,
    spinner: usize,
    ascii: bool,
    out: &mut Vec<Line<'static>>,
) {
    let icon = tool_icon(tool.kind, ascii);
    let (glyph, status_label, color) = tool_status_style(tool.status, spinner);
    out.push(Line::from(vec![
        Span::styled(format!("{icon} "), Style::new().fg(color)),
        Span::styled(tool.title.clone(), Style::new().fg(color)),
        Span::styled(
            format!("  [{glyph} {status_label}]"),
            Style::new().fg(color),
        ),
    ]));
    if !tool.input.is_empty() {
        out.push(Line::from(vec![Span::styled(
            format!("   {} · {}", tool.kind.label(), tool.input),
            Style::new().fg(Color::DarkGray),
        )]));
    }

    // opencode rule: a completed, successful tool collapses its body when
    // details are off (errors and running tools always expand).
    let collapsed = !details && tool.status == crate::acp::ToolStatus::Completed;
    if collapsed {
        if !tool.body.is_empty() {
            out.push(Line::styled(
                "   … (output hidden — /details)".to_owned(),
                Style::new().fg(Color::DarkGray),
            ));
        }
        return;
    }

    for body in &tool.body {
        match body {
            crate::acp::ToolBody::Text(text) => {
                push_capped(out, text.lines(), 14, |l| {
                    Line::from(vec![Span::styled(
                        format!("   {l}"),
                        Style::new().fg(Color::Gray),
                    )])
                });
            }
            crate::acp::ToolBody::Diff { path, text } => {
                out.push(Line::styled(
                    format!("   ± {path}"),
                    Style::new().fg(Color::Magenta),
                ));
                push_capped(out, text.lines(), 20, |l| {
                    let c = match l.as_bytes().first() {
                        Some(b'+') => Color::Green,
                        Some(b'-') => Color::Red,
                        _ => Color::Gray,
                    };
                    Line::from(vec![Span::styled(format!("   {l}"), Style::new().fg(c))])
                });
            }
        }
    }
}

/// Pushes mapped lines, truncating to `cap` with a `… (N more)` marker.
fn push_capped<'a, I, F>(out: &mut Vec<Line<'static>>, iter: I, cap: usize, mut f: F)
where
    I: Iterator<Item = &'a str>,
    F: FnMut(String) -> Line<'static>,
{
    let lines: Vec<&str> = iter.collect();
    let n = lines.len();
    for l in lines.iter().take(cap) {
        out.push(f((*l).to_owned()));
    }
    if n > cap {
        out.push(Line::styled(
            format!("   … ({} more lines)", n - cap),
            Style::new().fg(Color::DarkGray),
        ));
    }
}

fn line_color(role: Role) -> Color {
    match role {
        Role::Thought => Color::DarkGray,
        Role::System => Color::Gray,
        Role::Tool => Color::Yellow,
        Role::Plan => Color::Magenta,
        _ => Color::White,
    }
}

fn render_permission(perm: &crate::app::PendingPermission, frame: &mut Frame<'_>, area: Rect) {
    let h = (perm.options().len() as u16 + 6).min(area.height.saturating_sub(2));
    let rect = centered(area, area.width.min(72), h.max(7));
    let block = Block::bordered().title(" Permission requested ");
    let inner = block.inner(rect);
    clear(frame, rect);
    frame.render_widget(block, rect);

    let [head, list_area, hint] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    frame.render_widget(
        Paragraph::new(Line::styled(
            truncate(perm.title(), inner.width as usize),
            Style::new().fg(Color::Yellow),
        ))
        .wrap(Wrap { trim: false }),
        head,
    );
    let items: Vec<ListItem> = perm
        .options()
        .iter()
        .map(|o| ListItem::new(Line::raw(o.label.clone())))
        .collect();
    frame.render_widget(
        List::new(items)
            .highlight_symbol("▸ ")
            .highlight_style(Style::new().fg(Color::Black).bg(Color::Cyan))
            .selected(Some(perm.selected())),
        list_area,
    );
    frame.render_widget(
        Paragraph::new(Line::styled(
            "↑↓ choose · Enter approve · Esc decline",
            Style::new().fg(Color::Gray),
        )),
        hint,
    );
}

fn render_ask(ask: &crate::app::AskState, frame: &mut Frame<'_>, area: Rect) {
    let rect = centered(area, area.width.min(76), area.height.clamp(9, 16));
    let block = Block::bordered().title(format!(" {} asks ", ask.plugin()));
    let inner = block.inner(rect);
    clear(frame, rect);
    frame.render_widget(block, rect);

    let [q, body, ff, hint] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .areas(inner);

    let mut qlines = vec![Line::styled(
        ask.question().to_owned(),
        Style::new().fg(Color::Cyan),
    )];
    if !ask.context().is_empty() {
        qlines.push(Line::styled(
            ask.context().to_owned(),
            Style::new().fg(Color::Gray),
        ));
    }
    frame.render_widget(Paragraph::new(qlines).wrap(Wrap { trim: false }), q);

    let items: Vec<ListItem> = ask
        .options()
        .iter()
        .map(|o| ListItem::new(Line::raw(o.clone())))
        .collect();
    frame.render_widget(
        List::new(items)
            .highlight_symbol("▸ ")
            .highlight_style(if ask.freeform_focused() {
                Style::new().fg(Color::Gray)
            } else {
                Style::new().fg(Color::Black).bg(Color::Cyan)
            })
            .selected(Some(ask.selected())),
        body,
    );

    if ask.allow_freeform() {
        let fb = Block::bordered().title(if ask.freeform_focused() {
            " freeform (focused) "
        } else {
            " freeform (Tab to focus) "
        });
        let fi = fb.inner(ff);
        frame.render_widget(fb, ff);
        frame.render_widget(
            Paragraph::new(Line::raw(ask.freeform().lines().join(" "))),
            fi,
        );
    }
    frame.render_widget(
        Paragraph::new(Line::styled(
            "↑↓ choose · Tab freeform · Enter submit · Esc cancel",
            Style::new().fg(Color::Gray),
        )),
        hint,
    );
}

fn render_help(app: &ChatApp, frame: &mut Frame<'_>, area: Rect) {
    let rect = centered(area, area.width.min(74), area.height.clamp(10, 20));
    let block = Block::bordered().title(" Help — keys & commands ");
    let inner = block.inner(rect);
    clear(frame, rect);
    frame.render_widget(block, rect);

    let mut lines = vec![
        kv("Enter", "send message (Shift+Enter = newline)"),
        kv("/ then ↑↓", "slash autocomplete · Tab complete · Enter run"),
        kv("↑ ↓ ← →", "move the composer caret"),
        kv("PageUp/Down", "scroll the transcript"),
        kv("Esc", "cancel a streaming turn / close overlay"),
        kv("F1", "toggle this help"),
        kv(
            "Ctrl+K  ·  or k",
            "customise these keybindings (keymap editor)",
        ),
        kv("Ctrl+C / F10", "quit"),
        Line::raw(""),
        Line::styled(
            "Slash commands  (⚙ plugin · ◆ agent):",
            Style::new().fg(Color::Yellow),
        ),
    ];
    for spec in app.command_specs() {
        let tag = match spec.source {
            crate::app::CommandSource::Builtin => String::new(),
            crate::app::CommandSource::Plugin(p) => format!("  (⚙ {p})"),
            crate::app::CommandSource::Agent => "  (◆ agent)".to_owned(),
        };
        lines.push(kv(
            &format!("/{}", spec.name),
            &format!("{}{tag}", spec.description),
        ));
    }
    if !app.keybindings().is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Plugin keybindings:",
            Style::new().fg(Color::Yellow),
        ));
        for (chord, (plugin, command, desc)) in app.keybindings() {
            lines.push(kv(chord, &format!("{desc} → /{command} (⚙ {plugin})")));
        }
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// The keymap settings panel — the shared [`KeymapView`] widget (the exact
/// one the kitchen sink and git-review use), a pure projection of the live
/// keymap. The reducer owns the cursor + capture FSM; this only draws it.
fn render_keymap_panel(app: &ChatApp, frame: &mut Frame<'_>, area: Rect) {
    let t = app.theme();
    let rect = centered(area, area.width.min(60), area.height.clamp(8, 16));
    let block = Block::bordered().title(" Keymap ");
    let inner = block.inner(rect);
    clear(frame, rect);
    frame.render_widget(block, rect);

    let rows = app.keymap_panel_rows();
    let (map, os, capturing) = app.keymap_panel_status();
    let footer = if capturing {
        "● press a key to bind — Esc cancels".to_owned()
    } else {
        "↑↓/jk select · ⏎/r rebind · x disable · Esc close".to_owned()
    };
    frame.render_widget(
        KeymapView::new(&rows)
            .header(format!(" {map} · {os} — global commands, remappable"))
            .footer(footer)
            .separator("")
            .style(t.base())
            .label_style(t.base())
            .id_style(t.dim_text())
            .key_style(t.accent_text())
            .selected_style(t.selection())
            .capturing_style(t.accent_text())
            .disabled_style(t.dim_text()),
        inner,
    );
}

/// The `/status` overlay — session configuration + token usage, the
/// information Codex's `/status` surfaces. A pure projection of state.
fn render_status(app: &ChatApp, frame: &mut Frame<'_>, area: Rect) {
    let t = app.theme();
    let rect = centered(area, area.width.min(72), area.height.clamp(10, 18));
    let block = Block::bordered()
        .title(" Status (Esc to close) ")
        .border_style(t.accent_text())
        .style(t.base());
    let inner = block.inner(rect);
    clear(frame, rect);
    frame.render_widget(block, rect);

    let conn = match app.screen() {
        Screen::Picker => "no session (picker)",
        Screen::Connecting => "connecting…",
        Screen::Chat => "connected",
    };
    let agent = {
        let a = app.agent_command();
        if a.is_empty() {
            "(none — pick one with /agents)".to_owned()
        } else {
            a.to_owned()
        }
    };
    let context = match app.usage() {
        Some((used, size)) if size > 0 => {
            let pct = (used as f64 / size as f64 * 100.0).round() as u64;
            format!("{used} / {size} tokens ({pct}% of window)")
        }
        Some((used, _)) => format!("{used} tokens in context"),
        None => "— (agent has not reported usage)".to_owned(),
    };
    let (map, os, _) = app.keymap_panel_status();
    let lines = vec![
        kv("Agent", &agent),
        kv("Working dir", &app.cwd().display().to_string()),
        kv("Connection", conn),
        kv(
            "Turn",
            if app.is_streaming() {
                "streaming…"
            } else {
                "idle"
            },
        ),
        kv("Model", &app.current_model_name()),
        kv("Mode", &app.current_mode_name()),
        kv("Context", &context),
        Line::raw(""),
        kv("Theme", &t.name),
        kv("Keymap", &format!("{map} ({os})")),
        kv(
            "History",
            &format!("{} recalled prompts", app.history().entries().len()),
        ),
        kv("Bell", if app.bell_enabled() { "on" } else { "off" }),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// The `/model` picker — the agent's advertised model catalogue, the
/// currently-active one marked, Enter issues `session/set_model`. A pure
/// projection of `app.models()` + the reducer-owned selection.
fn render_model_picker(app: &ChatApp, frame: &mut Frame<'_>, area: Rect) {
    let t = app.theme();
    let models = app.models();
    let h = (models.len() as u16 + 4).clamp(7, area.height.saturating_sub(2).max(7));
    let rect = centered(area, area.width.min(72), h);
    let block = Block::bordered()
        .title(" Model (↑↓ select · Enter switch · Esc cancel) ")
        .border_style(t.accent_text())
        .style(t.base());
    let inner = block.inner(rect);
    clear(frame, rect);
    frame.render_widget(block, rect);

    let cur = app.current_model();
    let lines: Vec<Line> = models
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let marker = if Some(m.id.as_str()) == cur {
                "●"
            } else {
                " "
            };
            let label = if m.description.is_empty() {
                format!(" {marker} {}", m.name)
            } else {
                format!(" {marker} {} — {}", m.name, m.description)
            };
            if i == app.model_sel() {
                Line::styled(label, t.selection())
            } else {
                Line::styled(label, t.base())
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// The `/mode` picker — the agent's advertised session modes (Codex's
/// plan/approval modes, surfaced generically), the active one marked,
/// Enter issues `session/set_mode`. A pure projection.
fn render_mode_picker(app: &ChatApp, frame: &mut Frame<'_>, area: Rect) {
    let t = app.theme();
    let modes = app.modes();
    let h = (modes.len() as u16 + 4).clamp(7, area.height.saturating_sub(2).max(7));
    let rect = centered(area, area.width.min(72), h);
    let block = Block::bordered()
        .title(" Mode (↑↓ select · Enter switch · Esc cancel) ")
        .border_style(t.accent_text())
        .style(t.base());
    let inner = block.inner(rect);
    clear(frame, rect);
    frame.render_widget(block, rect);

    let cur = app.current_mode();
    let lines: Vec<Line> = modes
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let marker = if Some(m.id.as_str()) == cur {
                "●"
            } else {
                " "
            };
            let label = if m.description.is_empty() {
                format!(" {marker} {}", m.name)
            } else {
                format!(" {marker} {} — {}", m.name, m.description)
            };
            if i == app.mode_sel() {
                Line::styled(label, t.selection())
            } else {
                Line::styled(label, t.base())
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// The `/resume` picker — the sessions this client has started, newest
/// first; Enter asks the agent to `session/load` the chosen one. A pure
/// projection of the persisted `SessionStore`.
fn render_resume_picker(app: &ChatApp, frame: &mut Frame<'_>, area: Rect) {
    let t = app.theme();
    let list = app.resume_sessions();
    let h = (list.len() as u16 + 4).clamp(7, area.height.saturating_sub(2).max(7));
    let rect = centered(area, area.width.min(90), h);
    let block = Block::bordered()
        .title(" Resume (↑↓ select · Enter load · Esc cancel) ")
        .border_style(t.accent_text())
        .style(t.base());
    let inner = block.inner(rect);
    clear(frame, rect);
    frame.render_widget(block, rect);

    let lines: Vec<Line> = list
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let id_short: String = s.id.chars().take(8).collect();
            let agent = if s.agent.is_empty() {
                "agent"
            } else {
                &s.agent
            };
            let label = format!(" {agent}  ·  {}  ·  #{id_short}", s.cwd);
            if i == app.resume_sel() {
                Line::styled(label, t.selection())
            } else {
                Line::styled(label, t.base())
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// The sign-in picker — the agent's ACP auth methods (Codex's "Sign in
/// with ChatGPT / API key"); Enter runs `authenticate`. Pure projection.
fn render_auth_picker(app: &ChatApp, frame: &mut Frame<'_>, area: Rect) {
    let t = app.theme();
    let methods = app.auth_methods();
    let h = (methods.len() as u16 + 4).clamp(7, area.height.saturating_sub(2).max(7));
    let rect = centered(area, area.width.min(72), h);
    let block = Block::bordered()
        .title(" Sign in (↑↓ select · Enter · Esc dismiss) ")
        .border_style(t.accent_text())
        .style(t.base());
    let inner = block.inner(rect);
    clear(frame, rect);
    frame.render_widget(block, rect);

    let lines: Vec<Line> = methods
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let label = if m.description.is_empty() {
                format!("  {}", m.name)
            } else {
                format!("  {} — {}", m.name, m.description)
            };
            if i == app.auth_sel() {
                Line::styled(label, t.selection())
            } else {
                Line::styled(label, t.base())
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_log(app: &ChatApp, frame: &mut Frame<'_>, area: Rect) {
    let rect = centered(area, area.width.min(90), area.height.clamp(8, 20));
    let block = Block::bordered().title(" Log (Esc to close) ");
    let inner = block.inner(rect);
    clear(frame, rect);
    frame.render_widget(block, rect);
    let start = app.log().len().saturating_sub(inner.height as usize);
    let lines: Vec<Line> = app.log()[start..]
        .iter()
        .map(|l| Line::styled(l.clone(), Style::new().fg(Color::Gray)))
        .collect();
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// The plain text of a styled line (its spans concatenated) — used by the
/// pager's case-insensitive substring filter.
fn line_plain(line: &Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// The full-screen `/transcript` pager (Codex's pager overlay): the whole
/// rendered transcript, scrollable, with an incremental substring filter.
/// A pure projection — it reuses [`transcript_lines`] verbatim (so what you
/// search is exactly what the chat shows) and the chat's clamp-on-render
/// scroll model; the reducer owns only the few [`PagerState`] fields.
///
/// [`PagerState`]: crate::app::PagerState
fn render_transcript_pager(app: &ChatApp, frame: &mut Frame<'_>, area: Rect) {
    let t = app.theme();
    let p = app.pager();

    let mut lines = transcript_lines(app);
    let total_lines = lines.len();
    let q = p.query().to_lowercase();
    if !q.is_empty() {
        lines.retain(|l| line_plain(l).to_lowercase().contains(&q));
    }
    let matched = lines.len();
    if !q.is_empty() && lines.is_empty() {
        lines.push(Line::styled(
            format!("(no lines match {:?})", p.query()),
            t.dim_text(),
        ));
    }

    let title = if p.searching() {
        format!(" Transcript — search: {}▏ ", p.query())
    } else if q.is_empty() {
        format!(" Transcript — {total_lines} lines ")
    } else {
        format!(
            " Transcript — {matched}/{total_lines} match {:?} ",
            p.query()
        )
    };
    let block = Block::bordered()
        .title(title)
        .border_style(t.accent_text())
        .style(t.base());
    let inner = block.inner(area);
    clear(frame, area);
    frame.render_widget(block, area);

    let [body, hint] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(inner);

    let para = Paragraph::new(lines).wrap(Wrap { trim: false });
    let total = para.line_count(body.width.max(1)) as u16;
    let max_scroll = total.saturating_sub(body.height);
    let scroll = if p.follows() {
        max_scroll
    } else {
        p.scroll().min(max_scroll)
    };
    frame.render_widget(para.scroll((0, scroll)), body);

    let footer = if p.searching() {
        "type to filter · Enter apply · Esc cancel".to_owned()
    } else {
        "↑↓/jk PgUp/Dn scroll · g/G top/bottom · / search · Esc/q close".to_owned()
    };
    frame.render_widget(Paragraph::new(Line::styled(footer, t.dim_text())), hint);
}

fn render_toasts(app: &ChatApp, frame: &mut Frame<'_>, area: Rect) {
    let mut y = area.y + 1;
    for toast in app.toasts() {
        let text = format!(" {} ", truncate(&toast.text, 48));
        let w = text.chars().count() as u16;
        if w + 2 >= area.width {
            continue;
        }
        let x = area.x + area.width - w - 1;
        let rect = Rect {
            x,
            y,
            width: w,
            height: 1,
        };
        clear(frame, rect);
        frame.buffer_mut().set_str(
            Position::new(x, y),
            &text,
            Style::new().fg(Color::Black).bg(Color::Yellow),
        );
        y += 1;
        if y >= area.y + area.height {
            break;
        }
    }
}

// ---- small helpers ----

fn app_follows(app: &ChatApp) -> bool {
    // scroll() == 0 means the user has not paged up; stay pinned to the
    // bottom (the reducer resets scroll to 0 on new output).
    app.scroll() == 0
}

fn picker_offset(selected: usize, height: u16) -> usize {
    let h = height.max(1) as usize;
    if selected >= h { selected + 1 - h } else { 0 }
}

fn kv(key: &str, val: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:>14}  "), Style::new().fg(Color::Cyan)),
        Span::styled(val.to_owned(), Style::new().fg(Color::White)),
    ])
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

fn fill(frame: &mut Frame<'_>, area: Rect, style: Style) {
    let blank: String = " ".repeat(area.width as usize);
    for row in 0..area.height {
        frame
            .buffer_mut()
            .set_str(Position::new(area.x, area.y + row), &blank, style);
    }
}

fn clear(frame: &mut Frame<'_>, area: Rect) {
    fill(frame, area, Style::new().bg(Color::Black));
}

fn clamp(text: &str, width: u16) -> String {
    let w = width as usize;
    if text.chars().count() <= w {
        text.to_owned()
    } else {
        text.chars().take(w).collect()
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max || max == 0 {
        text.to_owned()
    } else {
        let mut s: String = text.chars().take(max.saturating_sub(1)).collect();
        s.push('…');
        s
    }
}

fn color_by_name(name: &str) -> Color {
    match name.to_ascii_lowercase().as_str() {
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "black" => Color::Black,
        "gray" | "grey" => Color::Gray,
        _ => Color::White,
    }
}
