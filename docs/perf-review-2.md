# Performance review 2 — post-fix, now repeatable

- **Date:** 2026-05-18
- **Tooling:** `cargo xtask perf` + `docs/perf-baseline.json` + `rstui-devtools` (ADR 0018)
- **Predecessor:** [`docs/perf-review.md`](perf-review.md) (the one-shot audit + ~40 landed fixes)

## What changed since review 1

Review 1 was a one-shot hand audit. Its two root-cause theses were
fixed and landed (the per-cell `Position`↔`index_of` round-trip is
gone; the build-whole-then-clip / re-parse-every-frame cluster is
windowed or caller-cached). The headline number moved exactly as
predicted: a zero-change idle frame's `buffer/diff` is **~11.5 µs**,
down from the ~40 µs review 1 opened with (a ~3.5× win that has held).

The structural change this review delivers is that **a review no
longer has to be hand-run**. ADR 0018 added:

- `rstui-devtools` — an opt-in leaf crate: a scoped-unsafe
  `CountingAllocator`, a caller-owned `PerfSession`, a
  `DevToolsAdapter` bridging the runtime, and a Chrome-DevTools-style
  `DevTools` overlay (Performance / Memory / Events / Inspect).
- An additive, pure `FrameObserver` seam in `rstui-runtime`
  (per-phase `logic`/`view`/`flush` timings, the RT-01 `produced`
  flag, input→frame latency, coalesced-event count) — zero-cost when
  unobserved, with `Harness::last_frame()` for deterministic headless
  perf tests.
- `cargo xtask perf` — runs the benches and diffs the result against
  the checked-in `docs/perf-baseline.json`, flagging regressions.
  *Not* a CI gate (ADR 0005); a one-command repeatable review.

"Do a perf review" is now: `cargo xtask perf`, read the report, audit
anything flagged, and for a live app open the `DevTools` overlay.

## Current numbers (`cargo xtask perf`, this machine)

| scenario | min | read |
|---|---|---|
| `buffer/diff/identical` | ~11.5 µs | idle frame — the review-1 win, holding (~40 µs → ~11.5 µs) |
| `buffer/diff/full` | ~13.4 µs | full repaint ≈ idle now: the diff is no longer the floor |
| `buffer/fill` / `set_str` | ~2.6 / ~3.1 µs | core stamping, unchanged |
| `layout/split/nested` | ~83→125 ns | **flagged** (see below) |
| `edit/textarea/insert` | ~41 ns | CM-3 O(1) `line_char_len` holding |
| `widget/list` / `table` / `tree` | ~33 / ~32 / ~18 µs | windowed widgets, well-behaved |
| `widget/paragraph/render` | ~56 µs | post-PG-1 |
| **`widget/markdown/render`** | **~1.48 ms** | still ~30× the next widget — the deliberate widget re-parse; the fix is caller-side caching (done in acp-client UI-1/MD-1), see below |
| `runtime/frame/idle` ≈ `changed` | ~80 µs | per-frame `view` projection of a representative 2-pane app dominates (not the now-~12 µs diff) |
| `runtime/input/mouse_move` | ~80 µs | one pointer-motion frame ≈ one normal frame — RT-01 keeps motion at *one* frame's cost, not one-per-sample |

Two things this table proves the tooling does:

1. **It catches regressions.** `layout/split/nested` is flagged
   `REGRESSED` (+~50%). It is sub-µs (83→125 ns, the cheapest
   scenario, at the noise floor) — almost certainly measurement
   jitter at that scale rather than a real defect, *but the report
   surfaces it for a human to judge* instead of it passing silently.
   That is exactly the point of `cargo xtask perf --check`: a number,
   not a vibe. (Re-run on a quiet machine; if it persists, bisect the
   recent `Layout::split` touches.)
2. **`runtime/input/mouse_move` ≈ a normal frame.** The "freeze while
   moving the mouse" class is the RT-01 saturation; this scenario
   exists so a regression of the coalesce/skip guard shows up as this
   number ballooning toward `N × frame`. It currently does not.

## Root-cause re-audit of code landed since review 1

New crates: `rstui-acp-shm` / `rstui-acp-plugin-sdk` (IPC/SDK, no
render hot path), `rstui-ai` / `rstui-jsonui` (have render paths),
`rstui-smoke` (test). The two root-cause patterns, applied to the new
code:

### Finding R2-AI-1 — `rstui-ai` measures every message's markdown every frame

**Status: fixed** — `rstui_ai::ConversationCache` (the caller-owned
UI-1/MD-1 cache); `Conversation::cache(&c)` / `Message::cache(&c)` are
opt-in and byte-identical, gate-enforced. Details under *Plan* below.

`Conversation` correctly *windows rendering* (off-screen turns are not
laid out — `conversation.rs` render loop). But the **height/scroll
math is not windowed**:

- `Message::height(width)` (`crates/rstui-ai/src/message.rs:176-181`)
  is `1 + Markdown::new(message_body_markdown(self.message)).lines(width).len()`
  — it **builds the body `String` from `parts` and runs the full
  Markdown parser+layout** on every call.
- `Conversation::turn_starts(width)`
  (`crates/rstui-ai/src/conversation.rs:169`) and the render loop call
  `Message::new(message).height(width)` for **every message, every
  frame**, to place the scroll window.

So a conversation of *N* messages pays *N* × (`message_body_markdown`
String build + a `widget/markdown/render`-class parse ≈ 1.48 ms each)
**every frame**, just to compute where the visible turns start —
unbounded in *N*, independent of how few are on screen. This is the
review-1 root-cause B ("build/parse the whole dataset every frame")
re-introduced in new code, *bounded for rendering but not for the
height pass*.

**Prescribed fix (the landed acp-client UI-1/MD-1 pattern):** a
caller-owned, `UiMessage::id`-keyed cache of the built body (and the
per-width height), invalidated on a `parts` fingerprint change. An
append-only conversation only mutates its last (streaming) message, so
older entries are immutable and cache permanently — exactly the
acp-client transcript situation that UI-1/MD-1 solved byte-identically.

### `rstui-jsonui` — clean

`UiNode::render` re-projects the parsed document to a fresh tree every
frame *by design* (ADR 0012 pure projection, no retained tree), but
the **parse is caller-owned** (the `DataModel`/parsed doc is passed
in, not re-parsed in `render`). No String-rebuild B-pattern; the cost
is document size, not re-derivation. No action.

### Root-cause A (per-cell index round-trip)

None in new code — the Tier-0 `Buffer::diff` flat-slice rewrite is the
permanent enabler and the new code uses the modern API.

## Plan

1. **R2-AI-1 fix — done.** `rstui_ai::ConversationCache`: a
   caller-owned, `UiMessage::id`-keyed per-message height memo (the
   `ScrollState` precedent — owned by the app's model, driven by the
   reducer). The reducer calls `cache.sync(&messages, width)` **once
   per `update`**; `Conversation::cache(&c)` / `Message::cache(&c)`
   then *read* it in `view`. Exactly the acp-client `refresh_md_cache`
   contract: only the last (streaming) turn is volatile, so it is
   never cached and re-measured fresh — every non-last turn is
   measured once and reused, and a miss measures fresh and returns the
   identical number. The per-frame cost drops from *O(N)* full Markdown
   re-parses to *O(1)* (just the streaming turn) plus the
   already-windowed visible renders.

   *Quantification:* this removes `N − 1` re-parses **per frame**, each
   a `widget/markdown/render`-class operation (≈ 1.48 ms on the
   baseline machine — the existing scenario *is* the per-message unit
   cost; the win is that × the non-last history × every frame). No new
   `rstui-bench` scenario: that crate is deliberately *rstui-code +
   std only* (its `Cargo.toml` / ADR 0005 — `rstui-ai` pulls
   `serde_json`, an external crate, so it cannot be a bench dep). The
   guarantee is instead **gate-enforced**, the proven acp-client
   approach: a byte-identical render test
   (`a_synced_cache_renders_byte_identical_and_measures_the_same`,
   render with vs. without the synced cache at three scroll offsets +
   equal `content_rows`/`turn_starts`) and a measurement-exactness
   test (`cache_height_equals_a_fresh_measure_for_every_non_last_message`),
   plus width-change / history-trim / empty-id / changed-fingerprint
   cases. Opt-in and additive: no `.cache(..)` ⇒ pre-cache behavior,
   unchanged.
2. **Wire `DevTools` into the kitchen-sink** (a hotkey toggle) and a
   standalone `devtools_demo`, so a downstream rstui developer has a
   worked reference for the overlay + the allocator + the observer.
3. **Re-baseline** (`cargo xtask perf --save`) after each, on a quiet
   machine, and keep this document the narrative companion to the
   machine record.
