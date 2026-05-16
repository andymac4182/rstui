RSTUI Stream 3: Rich document rendering.

You are one of five parallel gnhf streams working on rstui. Stay in your lane so the branches merge cleanly.

Other streams:
- Stream 1 owns the general concrete widget catalog and third-party widget authoring.
- Stream 2 owns full-screen runtime/crossterm/app-shell behavior.
- Stream 4 owns plugin host/runtime work around secure-exec.
- Stream 5 owns quality/DX infrastructure, benchmarks, profiling, kitchen sink, checks, and CI/dev workflows.

Your primary ownership:
- rich-rendering widgets/modules under `crates/rstui-widgets/**` or a clearly justified rich-rendering crate if dependencies require it
- examples that demonstrate markdown, links, tables, Mermaid, or diffs
- focused text/core primitives only when needed by rich rendering

Goal:
Build the track for markdown rendering, clickable links/link activation, markdown tables, Mermaid diagrams rendered into terminal-friendly ASCII/Unicode output, and text diff rendering.

References:
- Use `npx opensrc@latest path github:modem-dev/hunk` for diff UI/API ideas.
- Use OpenTUI and OpenCode as product references for document-heavy terminal UIs when useful.

Important direction:
- These are future-completeness capabilities, so ship meaningful vertical slices rather than pretending the whole renderer is done at once.
- Fit capabilities into the normal widget/component model.
- Keep interactive pieces integrated with focus/input/event handling.
- Make output testable through headless snapshots.
- Avoid broad runtime/backend/plugin/benchmark work except for small integration needs.
- Maintain the vague-name ban.

Useful next areas:
- a `Markdown` or `MarkdownView` initial widget with headings/emphasis/code/list support
- markdown table rendering as a focused slice
- link span model and activation event shape
- text diff widget/reference implementation inspired by hunk
- Mermaid AST/input placeholder plus terminal rendering plan, then a narrow renderable subset

Validation:
Run the relevant cargo gates before success. Prefer `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-features`, and any existing xtask checks.
