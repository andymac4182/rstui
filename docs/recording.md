# Recording

Every GIF/MP4 in this documentation is produced by [VHS][vhs] from a `.tape`
script — so the media is **reproducible, reviewable in a diff, and never
hand-captured**. Regenerating it is one command. There is also a VHS-driven
end-to-end smoke that asserts the *real* crossterm binary still works.

[vhs]: https://github.com/charmbracelet/vhs

## Toolchain

```sh
brew install vhs ttyd ffmpeg
```

- `vhs` drives a headless terminal and renders the recording.
- `ttyd` is the terminal VHS attaches to.
- `ffmpeg` is only needed for the `.mp4` outputs (GIFs work without it).

In this environment the headless Chrome VHS uses cannot sandbox, so VHS must
run with `VHS_NO_SANDBOX=true`. Both entry points below set it for you.

## One command

```sh
# from the repo root, either of:
cargo xtask record [all|widgets|gallery|kitchen-sink|e2e] [--check]
scripts/record-demos.sh [all|widgets|gallery|kitchen-sink|e2e] [--check]
```

| Target | What it produces |
|--------|------------------|
| `widgets` | one GIF per `rstui-widgets` example → `docs/widgets/media/<name>.gif` |
| `gallery` | the flagship hero GIF → `docs/widgets/media/gallery.gif` |
| `kitchen-sink` | the showcase at ~80×24 / ~120×40 / ~160×50 / ~200×60 → `docs/media/` |
| `e2e` | drives the real `rstui-kitchen-sink` binary, captures terminal text |
| `e2e --check` | the above **plus** a regression assertion (see below) |
| `all` *(default)* | every target above |

`record` pre-builds all examples and the kitchen sink first, so recordings
have no compile noise and the tapes run fast. It is **not** a `cargo xtask ci`
gate — it needs the VHS toolchain, which is not a CI dependency (the same
posture as `bench`; ADR 0005).

## How tapes are organised

```
vhs/
  common.tape                 # the single source of truth for shared Set directives
  gallery.tape                # the hero recording
  kitchen-sink/
    _tour.tape                # the shared scripted walkthrough (Sourced)
    kitchen-sink-80x24.tape   # Output + size, then Sources _tour.tape
    kitchen-sink-120x40.tape
    kitchen-sink-160x50.tape
    kitchen-sink-200x60.tape
  e2e/
    kitchen-sink-smoke.tape     # drives the real binary; Output is a .txt capture
    kitchen-sink-smoke.expect   # required substrings in the final frame
    kitchen-sink-dataviz.tape   # palette-drives the real binary through the
    kitchen-sink-dataviz.expect # observability/data-viz screens
```

- Every tape `Source vhs/common.tape`, so theme/font/padding are defined once.
- Per-widget tapes are **generated** by `record` from the live example list
  (into `target/vhs/`, not committed) — so they can never go stale when a
  widget is added or renamed. Only the hand-tuned tapes are committed.
- VHS is always invoked with cwd = repo root, so `Source` and `Output` paths
  are repo-relative and identical for humans and the xtask task.

## The end-to-end regression gate

`record e2e --check` is the third testing layer (see [Testing](testing.md)).
It drives the **real `rstui-kitchen-sink` crossterm binary** through a fixed
keystroke script, captures the terminal text VHS produces, and asserts every
marker in the sibling `.expect` file is present in the final frame.

This catches what the headless `Harness` cannot — crossterm event
translation, the panic-safe lifecycle, real terminal sizing — end to end. The
markers are header/footer/screen literals that are stable regardless of the
animation tick counter (kept in lock-step with the kitchen-sink test suite).

```sh
cargo xtask record e2e --check    # exit non-zero if a marker is missing
```

Add a new e2e scenario by dropping a `vhs/e2e/<name>.tape` whose `Output` is
`target/vhs/e2e/<name>.txt` plus a `vhs/e2e/<name>.expect` listing the
required substrings.

## Keeping media current

When a widget or screen changes, the recording must be regenerated and the
docs re-checked. That whole responsibility is encoded in the
[`rstui-docs` skill](../.claude/skills/rstui-docs/SKILL.md) — it is the
maintenance contract for everything on this site.
