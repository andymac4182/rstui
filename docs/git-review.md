# git-review — the worked code-review + editing app

`rstui-git-review` is a full-screen TUI that steps through a repository's git
history, renders each commit's patch with syntax highlighting, and edits the
working tree — built **entirely from existing widgets**, with `git` reached
only as a [`Cmd`](runtime.md)-seam subprocess.

It is the worked proof of the thesis in
[code-review-and-editing.md](code-review-and-editing.md): a review/editing
tool needs *no new framework widgets*. The roadmap's "git invocation is a
`Cmd`/app concern, not a widget" and "the review shell reuses existing
widgets" are demonstrated here, not just asserted.

## Run it

```sh
cargo run -p rstui-git-review                 # the repo in the current dir
cargo run -p rstui-git-review -- path/to/repo # another working tree
cargo run -p rstui-git-review -- . -- main~20..  # restrict the history range
```

Outside a git repository, or with no `git` on `PATH`, it shows a single
explanatory panel instead of crashing — the totality rule applies to apps too.

## What it composes (no new widgets)

| Concern | Widget | State it projects |
|---|---|---|
| Commit history | [`List`](widgets/core-set.md#list) | the `git log` rows + `selected` index |
| The commit's patch | [`Diff`](widgets/rich-rendering.md#diff) | `git show -p <sha>` text + a scroll offset |
| Editing a file | [`Editor`](widgets/core-set.md#editor) over a caller-owned [`TextArea`](core-reference.md) | the open working-tree file |
| Line numbers | [`LineNumberGutter`](widgets/rich-rendering.md#linenumbergutter) | the editor's row count |
| Chrome | [`StatusBar`](widgets/core-set.md#statusbar), [`HelpOverlay`](widgets/overlays-and-control.md#helpoverlay) | repo · branch · mode · keys |

The list scroll and the editor viewport are **pure functions of the selection
and the caret** computed in `view` (no stored offsets, no interior
mutability); only the diff's vertical scroll is genuine independent user
state. `git` runs in a `Cmd::perform` closure off the render loop, so a slow
`git show` never freezes the UI; the headless `Harness` runs it inline, so the
tests are deterministic.

## Keys

| Key(s) | Action |
|---|---|
| `[` / `]`, `p` / `n` | Previous / next commit |
| `j` / `k`, `↑` / `↓` | Move selection (list focus) or scroll the patch (diff focus) |
| `g` / `G` | Newest / oldest commit |
| `Tab` | Switch focus: commit list ⇄ patch |
| `e` | Edit the selected commit's first changed file |
| `Ctrl-S` | Save the edited file to the working tree (Edit mode) |
| `Esc` | Leave Edit mode / close help (in Review, `Esc`/`q` quits) |
| `?` | Help overlay · `q` Quit · `Ctrl-C` always quits |

## Testing

`crates/rstui-git-review/tests/harness.rs` drives the exact app through the
deterministic `Harness`: it boots against *this* repository's real history,
proves navigation/focus/help/tiny-terminal never panic or quit, and proves
the edit→save round-trip end to end against a throwaway fixture repo
(`git init` → edit → `Ctrl-S` → assert the working-tree bytes changed). See
[Testing](testing.md) for the layered suite model.
