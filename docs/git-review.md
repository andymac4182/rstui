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

## Layout & views

It is built to be flexible without any new widgets — every option is plain
caller-owned state the pure `view` reads:

- **Visual commit tree** — by default the history pane is the real
  `git log --graph` DAG: `git` draws the lanes/merges, commit rows stay
  selectable, pure connector rows (`|/`, `|\`) are shown but skipped by
  navigation. `\` toggles it off for a flat list.
- **Side-by-side diff** — `s` flips the [`Diff`](widgets/rich-rendering.md#diff)
  between unified (`≡`) and split (`◫`).
- **Re-orientable split** — `t` moves the history pane between the left
  (a tall commit column) and the top (a wide commit strip).
- **Resizable split** — `-` / `=` grow/shrink the history pane against the
  diff (15–75 % of the body; `Layout` clamps it total at any size).
- **Mouse** — **drag the pane border** to resize (the same divider-drag the
  kitchen-sink uses), **click** a commit to select it, and the **wheel**
  scrolls the patch or steps the history depending on which pane the pointer
  is over. The reducer hit-tests the geometry `view` recorded into a
  `Cell<Geom>` — the canonical rstui mouse pattern (a real terminal does not
  always send an initial resize, so a guessed size mis-places every click).
- **Filter** — `/` narrows the history to commits whose
  sha/subject/author/date match (case-insensitive); `Enter` keeps it, `Esc`
  clears. While filtered the list is flat (the DAG of a subset is
  meaningless).

## Keys

| Key(s) | Action |
|---|---|
| `[` / `]`, `p` / `n` | Previous / next commit |
| `j` / `k`, `↑` / `↓` | Move selection (history focus) or scroll the patch (diff focus) |
| `g` / `G` | Newest / oldest commit |
| `Tab` | Switch focus: history ⇄ patch |
| `s` | Toggle side-by-side ⇄ unified diff |
| `t` | Move the history pane: left ⇄ top |
| `-` / `=` | Resize the history / diff split |
| `\` | Toggle the visual commit tree (`git log --graph`) |
| `/` | Filter commits (`Enter` keep · `Esc` clear) |
| `e` | Edit the selected commit's first changed file |
| `Ctrl-S` | Save the edited file to the working tree (Edit mode) |
| `Esc` | Leave Edit mode / close help (in Review, `Esc`/`q` quits) |
| `Ctrl-K` | Keymap settings panel (the [`KeymapView`](widgets/overlays-and-control.md#keymapview) widget) |
| `?` | Help overlay · `q` Quit · `Ctrl-C` always quits |

## Customisable keymap

Every **command** above (filter, focus, the diff/layout toggles, edit,
help, quit, the keymap panel) is a semantic `Action` resolved through the
shared [`rstui-keymap`](keymaps.md) engine (ADR 0015), so all of them are
remappable three ways:

- **In-app:** `Ctrl-K` opens the `KeymapView` settings panel — select a
  row, press `r`/`Enter` to **capture a new key**, `x` to disable, `Esc`
  to close. The help/footer re-derive from the live map.
- **A config file / env:** `RSTUI_KEYMAP=/path/to/keymap` loads an
  `id = keys` override file (e.g. `git.split = ctrl+b`); see
  [keymaps.md](keymaps.md). Mirrors `RSTUI_THEME` — no rebuild, no panel.

Pane-relative **motions** (`j`/`k`, `g`/`G`, `↑`/`↓`, `[`/`]`, page,
`Home`) stay raw screen keys *by design*: ADR 0015 keeps the keymap
shell-level, and `Chord` folds letter case (so vim's `g`/`G` could not be
distinct actions) — the same boundary the kitchen sink draws for its
arrows/typing. Text entry (the editor, the filter input) is likewise raw.

## Testing

`crates/rstui-git-review/tests/harness.rs` drives the exact app through the
deterministic `Harness`: it boots against *this* repository's real history,
proves navigation/focus/help/tiny-terminal never panic or quit, and proves
the edit→save round-trip end to end against a throwaway fixture repo
(`git init` → edit → `Ctrl-S` → assert the working-tree bytes changed). The
new layout/views are covered on fixture repos too: the `git --graph` art
renders, `/` narrows the history and `Esc` restores it, `s` flips the
side-by-side marker, and the orientation/resize/graph toggles never panic.
See [Testing](testing.md) for the layered suite model.
