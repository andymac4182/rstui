Build rstui: an idiomatic Rust TUI framework that lets developers build powerful terminal applications quickly.

Continue from the existing code and .gnhf notes. Each iteration should make one small, commit-ready improvement toward a production-quality Rust TUI framework. Use ultrathink-level care for architecture, component APIs, rendering boundaries, and plugin security, while keeping each implementation slice reviewable.

Reference these upstream projects with npx opensrc when useful:
npx opensrc@latest path github:anomalyco/opentui
npx opensrc@latest path github:anomalyco/opencode
npx opensrc@latest path github:charmbracelet/bubbletea
npx opensrc@latest path github:ratatui/ratatui
npx opensrc@latest path github:longbridge/gpui-component
npx opensrc@latest path github:rivet-dev/secure-exec
npx opensrc@latest path github:earendil-works/pi

Project direction:
1. Build a Rust-native TUI framework inspired by the architecture of OpenTUI, the real application needs of OpenCode, the update/view/event loop ergonomics of Bubble Tea, the terminal rendering ecosystem of ratatui, and the breadth and polish of gpui-component.
2. Make rstui capable of supporting rich application UIs: layout, styling, themes, focus, keyboard/mouse events, async tasks, state updates, rendering, testing, and reusable components.
3. Grow a full component set over time: text, labels, input, textarea, select, checkbox, radio, buttons, lists, tables, trees, tabs, split panes, modals, command palette, status bars, spinners, progress, notifications, forms, markdown/code rendering, logs, and inspector/debug widgets.
4. Design for a plugin system using rivet-dev/secure-exec so rstui apps can support powerful but permissioned plugins, learning from OpenCode and pi. Treat plugin permissions, capabilities, process isolation, IO boundaries, and testability as first-class design constraints.
5. Make third-party component and widget authoring a first-class design goal. Public APIs should be composable, documented, and easy for application developers and agents to use when building custom TUIs.
6. Provide an agent-friendly iteration loop for building TUIs: deterministic examples, snapshots, headless render output, and eventually a kitchen-sink/demo app that makes visual and interaction regressions easy to inspect.
7. If the repository is still empty, first create a minimal Rust 2024 workspace foundation with README, CI, crate layout, and a tiny runnable example. Keep the foundation small and useful.
8. Plan the workspace around real boundaries: core runtime, renderer/layout/style primitives, components, examples, and plugin host/runtime. Introduce crates when there is enough real API surface to justify the boundary; avoid empty placeholder crates.
9. Use current stable crates where they clearly help, but do not add dependencies speculatively. Prefer simple, testable Rust APIs and keep public names coherent.
10. Add focused tests or examples for each new public capability. For behavior that is hard to test directly, add a small deterministic model or snapshot-style check.
11. Run the appropriate validation before reporting success. For Rust code, prefer cargo fmt --all --check, cargo clippy --all-targets --all-features -- -D warnings, and cargo test --all-features once those commands exist.
12. Update notes.md with the slice completed, upstream facts learned, and the next likely surface to build.

Do not copy code blindly from the reference projects. Use them to understand proven API shapes and architecture, then implement idiomatic Rust that fits rstui.
