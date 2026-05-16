# rstui GNHF Streams

These scripts start parallel Claude/GNHF streams in separate git worktrees.
Each stream uses `--worktree --push` by default so the main checkout stays
clean and the streams do not compete for the same working tree.

Run from the repo root:

```sh
scripts/gnhf-streams/run-widgets.sh
scripts/gnhf-streams/run-fullscreen-runtime.sh
scripts/gnhf-streams/run-rich-rendering.sh
scripts/gnhf-streams/run-plugins.sh
scripts/gnhf-streams/run-quality-dx.sh
```

Each script accepts normal gnhf flags. Passing any flags replaces the default,
so include `--worktree --push` yourself if you override:

```sh
scripts/gnhf-streams/run-widgets.sh --worktree --push --max-tokens 50000000
```

The prompt files in `scripts/gnhf-streams/prompts/` define ownership boundaries
so the streams avoid overlapping work.
