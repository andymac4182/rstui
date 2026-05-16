# rstui

A Rust TUI framework for building powerful terminal applications quickly.

The project is intended to learn from OpenTUI, OpenCode, Bubble Tea, ratatui,
and polished component libraries, while staying idiomatic to Rust.

## GNHF Claude Runner

Use the repo-local helper to run gnhf with Claude Code on Opus 4.7 using max
effort.

```sh
npm install -g gnhf
claude setup-token

scripts/run-gnhf-rstui.sh
```

By default this runs:

```sh
--current-branch --push
```

That keeps going until you stop it, gnhf reaches its failure limit, or you pass
your own stop condition. Pass custom gnhf flags to override the defaults:

```sh
scripts/run-gnhf-rstui.sh --worktree --max-iterations 10
```

Set `RSTUI_CLAUDE_MODEL` or `RSTUI_CLAUDE_EFFORT` to override the Claude model
or effort level for the wrapper.
