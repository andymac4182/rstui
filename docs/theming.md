# Theming

`rstui-theme` ships every [gpui-component](https://github.com/longbridge/gpui-component)
theme as a terminal-ready palette. It is a separate, optional crate
(`rstui-core` stays dependency-free and primitives-only per ADR 0002); an
app opts in by depending on `rstui-theme`.

## What you get

36 themes across 21 sets — Catppuccin (Latte/Frappé/Macchiato/Mocha),
Tokyo Night (Night/Storm/Moon), Gruvbox, Solarized, Ayu, Everforest,
Flexoki, Molokai, Mellifluous, Hybrid, macOS Classic, Adventure,
Spaceduck, Jellybeans, Matrix, and more — each light/dark variant a
separate selectable entry. Run them all:

```sh
cargo run -p rstui-theme --example theme_gallery          # every theme
cargo run -p rstui-theme --example theme_gallery -- tokyo  # filter by name
```

## Using a theme

A [`Theme`] carries a [`ThemePalette`]: ~110 semantic colours already
reduced to opaque terminal [`Color`]s, plus [`Style`] constructors for
the cases widgets actually consume. rstui's convention is that the app
threads theme-derived `Style`s into widget builders at the call site
(the same shape `DiffTheme`/`MarkdownTheme` and the kitchen-sink already
use) — this crate does **not** change the `Widget` render contract or
rewrite widgets.

```rust
use rstui_theme::Theme;
use rstui_widgets::{Block, List};

let theme = Theme::by_name("Catppuccin Mocha")
    .unwrap_or_else(Theme::default_dark);
let p = &theme.palette;

// Thread palette styles into the existing builders.
let list = List::new(items)
    .style(p.screen())
    .highlight_style(p.selection());
let block = Block::bordered().border_style(p.border_style());
```

Keep `theme` in your app state; swap it (e.g. a theme picker) and the
next `view()` re-renders in the new palette — no other change needed.

### Common style constructors

| Method | Use |
|---|---|
| `screen()` | the root fill: default text on the theme background |
| `text()` / `dim_text()` | body text / de-emphasised captions |
| `surface()` | popovers, cards, dialogs (raised background) |
| `border_style()` / `focus_ring()` | a plain border / a focused border |
| `selection()` / `text_selection()` | selected row / selected input text |
| `button_primary()` / `button_secondary()` | call-to-action / neutral buttons |
| `info_text()` `success_text()` `warning_text()` `danger_text()` | status |
| `link_style()` / `accent_text()` | hyperlinks / accent emphasis |

Every raw colour is also a public field on [`ThemePalette`]
(`p.primary`, `p.scrollbar_thumb`, `p.red_light`, …) for anything the
constructors don't cover.

## Picking a theme at runtime

[`ThemePicker`] is a reusable widget for "browse every theme, see it
applied live, then keep it" — the same drop-in pattern the kitchen-sink,
ACP client, and git-review all use. It is a pure projection of a
caller-owned [`ThemePickerState`] (catalogue + highlight + filter), so
the wiring is:

```rust
use rstui_theme::{Theme, ThemePicker, ThemePickerState};

// On your model:
struct App { picker: ThemePickerState, picking: bool, theme: MyTheme, /* … */ }

// Key handling while the picker is open:
//   ↑ / ↓        picker.prev() / picker.next()
//   printable    picker.push_filter(c)     Backspace  picker.pop_filter()
//   Esc          cancel  (restore the pre-picker palette)
//   Enter        keep    (persist + close)

// Every frame, *before* drawing, theme the whole app from the highlight —
// that IS the live preview, no special preview mode:
if let Some(t) = app.picker.selected_theme() {
    app.theme = MyTheme::from_palette(&t.palette);
}
// …then draw the picker on top:
frame.render_widget(ThemePicker::new(&app.picker), picker_area);

// On Enter — save so it sticks across launches:
Theme::write_choice(config_path, &app.picker.selected_theme().unwrap().name).ok();
// …and on startup:
let theme = Theme::read_choice(config_path).unwrap_or_else(Theme::default_dark);
```

The widget draws the scrollable list, the highlight, a live swatch strip
of the highlighted theme's own colours, and a key hint; it performs no
I/O and reads no clock, so it stays deterministic under snapshot tests.
[`Theme::write_choice`] / [`Theme::read_choice`] are the matching
one-call persistence pair (a saved name, or a path to a theme file).

## User-supplied themes

Users are not limited to the built-ins. A theme is just a gpui-component
`ThemeSet` JSON document — the exact format the vendored ones use — and it
loads through the same resolution path:

| API | Source |
|---|---|
| [`Theme::from_set_json`] | an in-memory JSON string |
| [`Theme::from_set_file`] | a `.json` file |
| [`Theme::load_dir`] | every `.json` in a directory (a user themes dir) |

```rust
use rstui_theme::Theme;

// Built-ins plus whatever the user dropped in their config dir.
let mut all = Theme::all();
if let Ok(user) = Theme::load_dir("~/.config/myapp/themes") {
    all.extend(user);
}
```

Unknown GUI-only keys are ignored (not rejected), and any failure is a
typed [`ThemeError`] (`Parse` / `Read`, with the offending path) — never a
panic. The kitchen-sink wires this end to end: `RSTUI_THEME` accepts
either a built-in name **or a path to a theme file**, e.g.
`RSTUI_THEME=./my-theme.json cargo run -p rstui-kitchen-sink`.

## How the port stays faithful

A gpui theme file sets only a handful of colours; the rest are *derived*
by compositing already-resolved ones, and the derivation is mode-aware.
The crate reproduces that pipeline exactly:

1. **`schema`** — gpui-component's `ThemeSet` JSON, verbatim.
2. **`shadcn`** — the named Tailwind/shadcn palette the base theme is
   authored against, vendored from gpui-component's own data (its `hex`
   fields are the source of truth, so values are byte-identical).
3. **`cascade`** — `apply_config` ported field-for-field: same
   operations, same statement order, same mode constants, same final
   alpha clamps. The order is load-bearing — each fallback reads fields
   resolved by the lines above it.
4. **`palette`** — the one lossy, irreversible step: a terminal cell has
   no alpha, so every colour is composited onto the theme background and
   reduced to an opaque 24-bit `Color`. A faint translucent row tint
   (`#CDA86911`) becomes the faint wash its author intended, not solid
   colour. Colours degrade further per terminal capability through the
   normal `rstui-core` path.

The vendored theme JSON lives in `crates/rstui-theme/themes/`
(`_default-*.json` are gpui-component's base palette + shadcn scales);
`crates/rstui-theme/tests/themes.rs` is the fidelity gate — every theme
resolves, every colour comes out opaque, and known literals + a derived
fallback + the alpha clamps land on their exact expected values.

[`Theme`]: https://docs.rs/rstui-theme
[`ThemePalette`]: https://docs.rs/rstui-theme
[`Theme::from_set_json`]: https://docs.rs/rstui-theme
[`Theme::from_set_file`]: https://docs.rs/rstui-theme
[`Theme::load_dir`]: https://docs.rs/rstui-theme
[`Theme::write_choice`]: https://docs.rs/rstui-theme
[`Theme::read_choice`]: https://docs.rs/rstui-theme
[`Theme::default_dark`]: https://docs.rs/rstui-theme
[`ThemePicker`]: https://docs.rs/rstui-theme
[`ThemePickerState`]: https://docs.rs/rstui-theme
[`ThemeError`]: https://docs.rs/rstui-theme
[`Color`]: https://docs.rs/rstui-core
[`Style`]: https://docs.rs/rstui-core
