//! `rstui-code` — the code-editing widget set for the rstui TUI framework.
//!
//! `rstui-core` owns the [`Widget`](rstui_core::Widget) trait and the
//! dependency-free primitives; `rstui-widgets` is the general widget set. This
//! crate is where the **code-editing** widgets live — the [`Editor`], the
//! unified [`Diff`], the language-aware [`syntax`] overlay, the symbol
//! [`Outline`], the multi-file [`Changeset`] splitter, and the
//! [`LineNumberGutter`] — together with a **first-class tree-sitter** back
//! end ([`treesitter`]). It is a separate crate so the universally-depended-on
//! `rstui-core`/`rstui-widgets` stay tree-sitter-free; only `rstui-code`
//! consumers pull tree-sitter
//! (see [ADR 0024](https://github.com/andymac4182/rstui/blob/main/docs/adr/0024-code-widget-crate-and-treesitter-exemption.md),
//! which supersedes [ADR 0023](https://github.com/andymac4182/rstui/blob/main/docs/adr/0023-treesitter-tier1-excluded-leaf-crate.md)).
//!
//! Every widget here follows the exact pure-projection pattern the rest of
//! the framework follows — depend on `rstui-core` (and `rstui-widgets` for
//! the framing [`Block`](rstui_widgets::Block) /
//! [`Extmark`](rstui_widgets::Extmark) overlay), implement
//! [`rstui_core::Widget`], stamp glyphs through the public `Buffer`
//! contract, and snapshot-test against
//! [`TestBackend`](rstui_core::TestBackend). The state — the document, the
//! scroll, the syntax overlay, the selection, the symbol list — is always
//! caller-owned data the widget only *reads* per cell.
//!
//! - [`editor`]: [`Editor`] — a multi-line text-entry widget, the
//!   [`Input`](rstui_widgets::Input) dual for documents; a pure projection of
//!   a borrowed caller-owned [`TextArea`](rstui_core::TextArea) model plus
//!   caller-owned 2D `scroll` and `focused`, with a rendered (not terminal)
//!   2D caret, an optional borrowed syntax overlay and selection. The reducer
//!   owns the edit and the scroll; the widget only reads.
//! - [`diff`]: [`Diff`] / [`DiffLayout`] / [`DiffTheme`] — a unified-diff
//!   view (hunk headers, +/- gutters, line numbers, word-level intra-line
//!   highlight, an optional language-aware syntax tint under the diff
//!   colours), the document analogue of `Paragraph` for code-review panes.
//! - [`syntax`]: [`Language`] / [`LexState`] / [`SyntaxStyles`] /
//!   [`line_overlay`] — the dependency-free, language-aware lexical tinter
//!   shared by [`Diff`] and [`Editor`], carrying an end-of-line [`LexState`]
//!   so multi-line strings/comments colour correctly. Colours come from the
//!   caller's theme.
//! - [`outline`]: [`Outline`] / [`Symbol`] / [`SymbolKind`] — the symbol
//!   model and a dependency-free per-[`Language`] heuristic
//!   scanner ([`Outline::scan`]); a *model + scanner*, projected through the
//!   existing `Tree`/`List`.
//! - [`changeset`]: [`Changeset`] / [`DiffFile`] / [`FileStatus`] /
//!   [`HunkRef`] — the multi-file *splitter + index* over a multi-file
//!   unified patch, feeding single-file slices back to [`Diff`].
//! - [`line_number_gutter`]: [`LineNumberGutter`] — a pure layout widget
//!   drawing a numeric (+ optional per-row sign) gutter and exposing the
//!   inner content [`Rect`](rstui_core::Rect) (the
//!   [`Block::inner`](rstui_widgets::Block::inner) pattern), for
//!   code/diff/editor panes.
//! - [`treesitter`]: [`Analyzer`] / [`TsLanguage`] — the first-class
//!   tree-sitter back end (ADR 0022 Tier-1): one real parse → *both* the
//!   per-character syntax overlay [`Editor::syntax`](editor::Editor) reads
//!   *and* the symbol [`Outline`] the panel projects, a drop-in better
//!   producer of the same shapes the dependency-free [`syntax`]/[`outline`]
//!   floor feeds. Per-language, default-on, behind a Cargo feature each.
//!
//! # Example
//!
//! ```
//! use rstui_core::{Buffer, Position, Rect, TextArea, Widget};
//! use rstui_code::Editor;
//!
//! let doc = TextArea::from_value("fn main() {}\n");
//! let mut buf = Buffer::empty(Rect::new(0, 0, 14, 2));
//! Editor::new(&doc).render(buf.area(), &mut buf);
//!
//! assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, 'f');
//! ```

pub mod changeset;
pub mod diff;
pub mod editor;
pub mod line_number_gutter;
pub mod outline;
pub mod syntax;
pub mod treesitter;

pub use changeset::{Changeset, DiffFile, FileStatus, HunkRef};
pub use diff::{Diff, DiffLayout, DiffTheme};
pub use editor::Editor;
pub use line_number_gutter::LineNumberGutter;
pub use outline::{Outline, Symbol, SymbolKind};
pub use syntax::{Language, LexState, SyntaxStyles, line_overlay};
pub use treesitter::{Analyzer, TsLanguage};
