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

For unattended overnight runs, prefer the launchers. They copy the compact
goal condition to the clipboard, create an explicit git worktree under
`/Users/andrewmcclenaghan/dev/andymac4182/rstui-stream-worktrees`, launch
Claude from inside that worktree under tmux, keep macOS awake with
`caffeinate`, use Opus 4.7 with max effort, and bypass tool permission prompts:

```sh
scripts/claude-goals/run-widgets.sh
scripts/claude-goals/run-fullscreen-runtime.sh
scripts/claude-goals/run-rich-rendering.sh
scripts/claude-goals/run-plugins.sh
scripts/claude-goals/run-quality-dx.sh
```

The relevant Claude switches are:

- `--dangerously-skip-permissions`: bypass tool permission prompts.
- `--model claude-opus-4-7 --effort max`: use the requested model and thinking
  effort.

The launchers intentionally do not use Claude's built-in `--worktree` or
`--add-dir`. They create the git worktree themselves, `cd` into it, then start
Claude there. Claude should edit inside its current working directory only. The
main checkout is only for the serialized git merge-back step.

The detailed briefs in `scripts/claude-goals/*.md` intentionally repeat the
shared rules. That keeps each Claude run self-contained after it reads the file
from disk.

Each stream is also told to use Claude Code agent teams/background agents for
parallel work inside its ownership boundary: one agent can inspect references,
one can implement a focused slice, and another can verify tests/docs while the
main session coordinates integration and merge-back.

Important operating notes:

- Each stream owns a clear area and should avoid broad edits outside that area.
- Each stream merges back to `main` after each validated slice, not only at the
  end of the night.
- Validation is the single command `cargo run -p xtask -- ci` (fmt,
  lint-names, clippy, **doc**, test — exactly CI's gates). Run it on the
  *merged* `main` checkout, not just the stream worktree, before pushing;
  never push a red `main`. The partial fmt/clippy/test list omits the
  rustdoc `doc` gate. Full checklist: `docs/merging.md`.
- Merge-back uses `/tmp/rstui-main-merge.lock`. A stale lock whose
  `owner.pid` is a dead process may be cleared so a crashed stream cannot
  block everyone; a lock held by a live PID must be waited on.
- If `main` is dirty, a merge conflict is ambiguous, or validation cannot be
  made green, the stream should stop and report rather than forcing broken
  state into `main`.
- No stream should use `git reset --hard`, force push, or delete another
  stream's worktree.
