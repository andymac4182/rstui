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

## User-supplied themes

The on-disk format is gpui-component's `ThemeSet` JSON, unchanged. Load a
user file with [`Theme::from_set_json`]; it resolves through the exact
same path as the built-ins (unknown GUI-only keys are ignored, not
rejected, and a malformed file is a typed `ThemeError`, not a panic).

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
[`Color`]: https://docs.rs/rstui-core
[`Style`]: https://docs.rs/rstui-core
