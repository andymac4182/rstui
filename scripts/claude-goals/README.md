# rstui Claude Goal Streams

These prompts split rstui into five parallel Claude Code streams. The full
stream briefs are intentionally detailed and live one directory up. Claude
`/goal` conditions are limited to 4000 characters, so paste the compact files
from `goal-conditions/`; each compact goal tells Claude to read and execute the
full local brief.

Start from a clean `main` checkout:

```sh
cd /Users/andrewmcclenaghan/dev/andymac4182/rstui
git status --short
git checkout main
git pull --ff-only origin main
```

Then open five Claude Code sessions and run `/goal` in each one, pasting one
compact goal condition per session:

```sh
pbcopy < scripts/claude-goals/goal-conditions/widgets.md
pbcopy < scripts/claude-goals/goal-conditions/fullscreen-runtime.md
pbcopy < scripts/claude-goals/goal-conditions/rich-rendering.md
pbcopy < scripts/claude-goals/goal-conditions/plugins.md
pbcopy < scripts/claude-goals/goal-conditions/quality-dx.md
```

The detailed briefs in `scripts/claude-goals/*.md` intentionally repeat the
shared rules. That keeps each Claude run self-contained after it reads the file
from disk.

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
