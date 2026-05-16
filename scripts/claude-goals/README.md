# rstui Claude Goal Streams

These prompts are designed to be pasted into Claude Code after starting
`/goal`. They split rstui into five parallel streams. Each stream creates its
own worktree, works in larger coherent slices, commits validated work, then
serializes merge-back to `main` with a local lock so multiple Claude runs do
not merge at the same time.

Start from a clean `main` checkout:

```sh
cd /Users/andrewmcclenaghan/dev/andymac4182/rstui
git status --short
git checkout main
git pull --ff-only origin main
```

Then open five Claude Code sessions and run `/goal` in each one, pasting one
prompt per session:

```sh
pbcopy < scripts/claude-goals/widgets.md
pbcopy < scripts/claude-goals/fullscreen-runtime.md
pbcopy < scripts/claude-goals/rich-rendering.md
pbcopy < scripts/claude-goals/plugins.md
pbcopy < scripts/claude-goals/quality-dx.md
```

The prompts intentionally repeat the shared rules. That keeps each Claude run
self-contained if it is launched in a separate terminal, resumed later, or run
from a different working directory.

Important operating notes:

- Each stream owns a clear area and should avoid broad edits outside that area.
- Each stream merges back to `main` after each validated slice, not only at the
  end of the night.
- Merge-back uses `/tmp/rstui-main-merge.lock`; if a Claude run crashes while
  holding it, inspect the lock before removing it.
- If `main` is dirty, a merge conflict is ambiguous, or validation cannot be
  made green, the stream should stop and report rather than forcing broken
  state into `main`.
- No stream should use `git reset --hard`, force push, or delete another
  stream's worktree.
