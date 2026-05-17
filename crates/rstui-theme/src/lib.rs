//! `rstui-theme` — semantic colour themes for rstui, a faithful port of every
//! [gpui-component](https://github.com/longbridge/gpui-component) theme.
//!
//! # What this gives you
//!
//! gpui-component ships ~20 polished theme *sets* (Catppuccin, Tokyo Night,
//! Gruvbox, Solarized, Ayu, Everforest, …), most with light and dark
//! variants. Every one of them is vendored here verbatim and resolved into a
//! terminal-ready [`ThemePalette`]: ~110 semantic colours (background,
//! border, primary, danger, list-selection, scrollbar, the ANSI base set, …)
//! plus the [`Style`](rstui_core::Style) constructors rstui widgets consume.
//!
//! ```
//! use rstui_theme::Theme;
//!
//! // Every built-in theme; each light/dark variant is its own entry.
//! let all = Theme::all();
//! assert!(all.iter().any(|t| t.name.contains("Catppuccin")));
//!
//! let t = Theme::by_name("Tokyo Night").or_else(|| Some(Theme::default_dark())).unwrap();
//! let list_style = t.palette.selection();        // -> rstui_core::Style
//! let _ = (list_style, t.palette.background);    // wire into widget builders
//! ```
//!
//! # Bring your own themes
//!
//! The crate is published as `rstui-theme` — depend on it and you get the
//! whole built-in catalogue. Users are not limited to it: a theme is just a
//! gpui-component `ThemeSet` JSON document (the exact format the built-ins
//! use), loaded through the same resolution path —
//!
//! - [`Theme::from_set_json`] — from an in-memory string,
//! - [`Theme::from_set_file`] — from a `.json` file,
//! - [`Theme::load_dir`] — every `.json` in a user themes directory,
//!
//! so an app can offer `Theme::all()` plus whatever the user dropped in their
//! config dir. The kitchen-sink wires this through `RSTUI_THEME`, which
//! accepts either a built-in name or a path to a theme file.
//!
//! # How the port stays faithful
//!
//! A gpui theme file sets only a handful of colours; the rest are *derived*
//! by compositing already-resolved ones, and the derivation is mode-aware.
//! This crate reproduces that pipeline exactly:
//!
//! - [`schema`] — gpui-component's `ThemeSet` JSON, verbatim (unknown GUI-only
//!   keys ignored, not rejected).
//! - [`shadcn`] — the named Tailwind/shadcn palette the *base* theme is
//!   authored against, vendored from gpui-component's own data.
//! - [`cascade`] — `apply_config` ported field-for-field: same operations,
//!   same order, same mode constants, same final alpha clamps.
//! - [`palette`] — the one lossy step: composite every colour onto the
//!   background and reduce to an opaque terminal [`Color`](rstui_core::Color),
//!   because a cell has no alpha. A faint translucent row tint becomes the
//!   faint wash its author intended, not solid colour.
//!
//! # Scope
//!
//! This crate is data + `Style` constructors only. It deliberately does *not*
//! change the [`Widget`](rstui_core) render contract or rewrite widgets:
//! rstui's convention (and the kitchen-sink's own theme) is for the app to
//! thread theme-derived [`Style`](rstui_core::Style)s into widget builders at
//! the call site, which is exactly what [`ThemePalette`]'s methods produce.
//! See `examples/theme_gallery.rs` for the end-to-end wiring.

// Public modules (rstui-core's convention): each is documented and the
// architecture cross-links above resolve to it. The flat re-exports below are
// the names most callers use.
pub mod cascade;
pub mod hsla;
pub mod palette;
pub mod picker;
pub mod registry;
pub mod schema;
pub mod shadcn;

pub use cascade::ThemeColor;
pub use hsla::{Hsla, Rgba};
pub use palette::ThemePalette;
pub use picker::{ThemePicker, ThemePickerState};
pub use registry::{Theme, ThemeError};
pub use schema::{ThemeColorConfig, ThemeConfig, ThemeMode, ThemeSet};
pub use shadcn::try_parse_color;
