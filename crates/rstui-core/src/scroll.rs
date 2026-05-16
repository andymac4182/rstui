//! Caller-owned 1-axis scroll position as model state.
//!
//! [`ScrollState`] is the scrolling-side sibling of
//! [`TextEdit`](crate::text_edit::TextEdit) /
//! [`FocusRing`](crate::focus::FocusRing): a pure value type that lives as a
//! *field in the application's model*, mutated only by `update` (a wheel
//! tick, `PageUp`, a fresh streamed chunk), and read by the pure `view` to
//! drive a viewport widget. The
//! [`ScrollView`](https://docs.rs/rstui-widgets)/`List`/`Editor` widgets are
//! pure projections of one — they read [`offset`](ScrollState::offset) and
//! never scroll themselves. Per
//! [ADR 0004](https://github.com/andymac4182/rstui/blob/main/docs/adr/0004-focus-routing-architecture.md)
//! §1 this is *forced* by rstui's pure-`view` / immediate-mode design: a
//! widget is handed only a [`Buffer`](crate::buffer::Buffer) at render time
//! with no signed coordinates, so it can neither own the scroll offset nor
//! move it on an event. The reducer owns the scroll, exactly as it owns the
//! focus and the edited text.
//!
//! Like [`focus`](crate::focus) and [`text_edit`](crate::text_edit), this
//! module is **optional**: an app may keep a bare `usize` offset of its own
//! and never name a type from here. `ScrollState` exists only to turn the
//! clamp / sticky-bottom / scroll-into-view bookkeeping every transcript or
//! log pane re-derives — and routinely gets wrong at the edges (off-by-one at
//! the end, a window larger than the content, an over-scroll past the tail) —
//! into one reusable, panic-free primitive:
//!
//! - The offset is a **line/row index** (the first content row drawn at the
//!   viewport top), never a pixel or byte. The widget maps it straight to a
//!   slice; sizes are not stored here — `content_len` and `viewport_len` are
//!   layout facts the caller passes in per call (the same caller-owned-offset
//!   model `List`/`Editor`/`ScrollView` already use, which clamp a raw offset
//!   against their own dimensions at render time).
//! - It carries a **sticky-bottom intent** ([`following`](ScrollState::following)):
//!   a streaming transcript that is pinned to the tail stays pinned as new
//!   lines arrive ([`on_content_change`](ScrollState::on_content_change)),
//!   but the moment the user scrolls up it stops auto-following and stays
//!   where they put it — the one ergonomic every chat/log UI needs and the
//!   deep-dive's #1 near-blocker for a faithful transcript.
//! - Every method is **total** — no input, including a `content_len`/
//!   `viewport_len` of `0`, a child past the end, or [`usize::MAX`] sizes,
//!   can panic or leave the offset past the last full window after a
//!   [`clamp`](ScrollState::clamp) (the iter-25 "a pure projection must be
//!   total" rule, the same guarantee [`FocusRing`](crate::focus::FocusRing)
//!   and [`TextEdit`](crate::text_edit::TextEdit) give).
//!
//! It is **one axis on purpose**: a 2-D viewport composes two
//! `ScrollState`s (one per axis), exactly as the
//! [`focus`](crate::focus) model composes ids rather than growing a 2-D
//! variant. This is **app/widget** scroll and is unrelated to terminal
//! scrollback.
//!
//! # Example
//!
//! ```
//! use rstui_core::scroll::ScrollState;
//!
//! // A chat transcript: 100 lines of history, a 20-row viewport. `new()`
//! // starts pinned to the tail (sticky-bottom), the usual transcript default.
//! let mut s = ScrollState::new();
//! assert!(s.following());
//!
//! // A streamed chunk grows the content: while following, the offset snaps
//! // to the new end with zero caller bookkeeping.
//! s.on_content_change(100, 20);
//! assert_eq!(s.offset(), 80); // 100 - 20: the last full window
//! assert!(s.at_end(100, 20));
//!
//! // The user scrolls up to read history: tail-follow stops, so further
//! // streamed lines no longer yank them back down.
//! s.scroll_by(-30, 100, 20);
//! assert_eq!(s.offset(), 50);
//! assert!(!s.following());
//! s.on_content_change(140, 20); // 40 more lines stream in…
//! assert_eq!(s.offset(), 50); // …and they stay exactly where they were.
//!
//! // Jumping back to the bottom re-arms sticky-follow.
//! s.scroll_to_end(140, 20);
//! assert_eq!(s.offset(), 120);
//! assert!(s.following());
//!
//! // Every input is total: an over-scroll parks at the end, never a panic.
//! s.set_offset(9_999);
//! s.clamp(140, 20);
//! assert_eq!(s.offset(), 120);
//! ```

/// A caller-owned one-axis scroll offset plus a sticky-bottom intent.
///
/// `ScrollState` is a **pure value type** designed to live as a field in the
/// application's model. It owns *no* terminal, runtime, or widget state:
/// `update` mutates it in response to scroll messages the app maps (a wheel
/// tick, `PageUp`/`PageDown`, `Home`/`End`, a freshly streamed chunk), and
/// the pure `view` only reads [`offset`](Self::offset) to project a viewport.
/// The framework never touches it. Compose two (one per axis) for a 2-D
/// viewport.
///
/// `content_len` (total rows of content) and `viewport_len` (visible rows)
/// are **not stored** — they are layout facts passed per call, so the same
/// state is correct across a resize with no invalidation step. The offset
/// invariant `offset <= content_len.saturating_sub(viewport_len)` holds after
/// any [`clamp`](Self::clamp) / [`scroll_by`](Self::scroll_by) /
/// [`scroll_to_end`](Self::scroll_to_end) /
/// [`on_content_change`](Self::on_content_change) / [`show`](Self::show);
/// [`set_offset`](Self::set_offset) records a *raw* request (an over-scroll)
/// that the next `clamp` reconciles, exactly as `ScrollView`/`Editor` accept
/// a raw caller offset and clamp it at render.
///
/// Every method is **total**: arbitrary input — zero `content_len` or
/// `viewport_len`, a child far past the end, [`usize::MAX`] sizes, a wildly
/// over-scrolled [`set_offset`](Self::set_offset) — is well-defined and never
/// panics.
///
/// # `Default` vs [`new`](Self::new)
///
/// `Default` is the inert all-zero state (`offset 0`, **not** following) so it
/// drops cleanly into a `#[derive(Default)]` model as a plain top-anchored
/// pane. [`new`](Self::new) instead starts **following the tail**, the right
/// default for a streaming transcript/log — the one place this primitive's
/// `new() != default()` (a deliberate, documented divergence from
/// [`FocusRing`](crate::focus::FocusRing)/[`TextEdit`](crate::text_edit::TextEdit),
/// because "follow the tail" is the useful default exactly when you reach for
/// this type).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ScrollState {
    /// First content row drawn at the viewport top.
    offset: usize,
    /// Sticky-bottom intent: keep the offset pinned to the end as content
    /// grows. Set by tail-ward navigation, cleared by scrolling away.
    follow_tail: bool,
}

impl ScrollState {
    /// A scroll pinned to the tail: `offset 0` and **following**.
    ///
    /// This is the streaming-transcript default — the offset snaps to the end
    /// on the first [`on_content_change`](Self::on_content_change) /
    /// [`clamp`](Self::clamp)-driven reconcile, so a fresh chat/log view
    /// shows the newest lines with no caller bookkeeping. Use `Default` for a
    /// plain top-anchored pane that does *not* auto-follow (see the
    /// [type docs](Self#default-vs-new)).
    #[must_use]
    pub fn new() -> Self {
        Self {
            offset: 0,
            follow_tail: true,
        }
    }

    /// The first content row drawn at the viewport top — what the projecting
    /// widget slices from (it clamps against its own dimensions at render, so
    /// a raw [`set_offset`](Self::set_offset) over-scroll is still safe).
    #[must_use]
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Whether the sticky-bottom intent is armed: a growing
    /// [`on_content_change`](Self::on_content_change) keeps the offset pinned
    /// to the end while this is `true`.
    #[must_use]
    pub fn following(&self) -> bool {
        self.follow_tail
    }

    /// The largest in-bounds offset for these sizes: `content_len -
    /// viewport_len`, or `0` when the content fits (saturating, never
    /// negative). The single definition every other method clamps to, so the
    /// end-of-scroll rule cannot drift.
    #[must_use]
    fn max_offset(content_len: usize, viewport_len: usize) -> usize {
        content_len.saturating_sub(viewport_len)
    }

    /// Requests an absolute offset (an explicit jump, e.g. a scrollbar drag
    /// mapped to a row). This is a **raw** request: the value is stored
    /// as-is — call [`clamp`](Self::clamp) (the reducer does this each frame
    /// against the current layout) to reconcile an over-scroll, exactly as
    /// `ScrollView`/`Editor` accept a raw caller offset and clamp it at
    /// render. An explicit jump is a manual action, so tail-follow is
    /// **dropped**; it re-arms only via [`scroll_to_end`](Self::scroll_to_end)
    /// or a [`scroll_by`](Self::scroll_by) that lands at the end.
    pub fn set_offset(&mut self, off: usize) {
        self.offset = off;
        self.follow_tail = false;
    }

    /// Clamps the offset into `0..=max` for these sizes (`max` =
    /// `content_len - viewport_len`, saturating). Total for any sizes,
    /// idempotent, and leaves the sticky-bottom intent untouched — call it
    /// after [`set_offset`](Self::set_offset) or whenever `content_len`/
    /// `viewport_len` may have changed (a resize) to re-establish the offset
    /// invariant.
    pub fn clamp(&mut self, content_len: usize, viewport_len: usize) {
        self.offset = self.offset.min(Self::max_offset(content_len, viewport_len));
    }

    /// Whether the offset is at (or past) the last full window for these
    /// sizes — i.e. the tail is visible. `true` when the content fits the
    /// viewport (there is nothing below to scroll to).
    #[must_use]
    pub fn at_end(&self, content_len: usize, viewport_len: usize) -> bool {
        self.offset >= Self::max_offset(content_len, viewport_len)
    }

    /// Scrolls by `delta` rows (negative = up/toward the start), saturating at
    /// both ends and clamped into bounds. Tail-follow is then set to whether
    /// the move **landed at the end**, so a wheel/PageDown that reaches the
    /// bottom re-arms sticky-follow and scrolling away from it disarms it —
    /// the expected "scroll back down to resume following" behavior.
    pub fn scroll_by(&mut self, delta: isize, content_len: usize, viewport_len: usize) {
        let magnitude = delta.unsigned_abs();
        let moved = if delta >= 0 {
            self.offset.saturating_add(magnitude)
        } else {
            self.offset.saturating_sub(magnitude)
        };
        self.offset = moved.min(Self::max_offset(content_len, viewport_len));
        self.follow_tail = self.at_end(content_len, viewport_len);
    }

    /// Jumps to the very top (`offset 0`) and **disarms** tail-follow (going
    /// to the top is an explicit "stop following, read from the start").
    pub fn scroll_to_top(&mut self) {
        self.offset = 0;
        self.follow_tail = false;
    }

    /// Jumps to the last full window for these sizes and **arms** tail-follow,
    /// so subsequent streamed content keeps the tail pinned (the "jump to
    /// bottom and stay there" action).
    pub fn scroll_to_end(&mut self, content_len: usize, viewport_len: usize) {
        self.offset = Self::max_offset(content_len, viewport_len);
        self.follow_tail = true;
    }

    /// Reconciles the offset after the content length changed (a streamed
    /// chunk arrived, lines were trimmed). Sticky-bottom: if
    /// [`following`](Self::following) the offset snaps to the new end (the
    /// tail stays pinned while streaming); otherwise it is merely clamped in
    /// place so the user's reading position is preserved and never escapes
    /// the new bounds. Call this from `update` whenever `content_len` grows
    /// or shrinks — *before* any same-frame [`clamp`](Self::clamp) with the
    /// new length, since it reads the pre-change follow intent.
    pub fn on_content_change(&mut self, content_len: usize, viewport_len: usize) {
        let max = Self::max_offset(content_len, viewport_len);
        self.offset = if self.follow_tail {
            max
        } else {
            self.offset.min(max)
        };
    }

    /// Scroll-into-view: adjusts the offset by the **smallest** amount so the
    /// child rows `[child_y, child_y + child_h)` are fully visible in a
    /// `viewport_len`-row window, then clamps into the content. If the child
    /// is taller than the window (it cannot fully fit) the window is aligned
    /// to the child's **top** (its start is shown — total and deterministic).
    /// Tail-follow is set to whether the resulting offset is at the end (so
    /// revealing the last child resumes following). All arithmetic saturates:
    /// any `child_y`/`child_h`/sizes, including [`usize::MAX`], are safe.
    pub fn show(
        &mut self,
        child_y: usize,
        child_h: usize,
        viewport_len: usize,
        content_len: usize,
    ) {
        let child_end = child_y.saturating_add(child_h);
        let want = if viewport_len == 0 || child_h >= viewport_len {
            // Cannot fit the whole child; show its start.
            child_y
        } else if child_y < self.offset {
            // Child starts above the window: pull the top up to it.
            child_y
        } else if child_end > self.offset.saturating_add(viewport_len) {
            // Child ends below the window: push the bottom down to it.
            child_end.saturating_sub(viewport_len)
        } else {
            // Already fully visible.
            self.offset
        };
        self.offset = want.min(Self::max_offset(content_len, viewport_len));
        self.follow_tail = self.at_end(content_len, viewport_len);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_follows_the_tail_but_default_is_inert() {
        let fresh = ScrollState::new();
        assert_eq!(fresh.offset(), 0);
        assert!(fresh.following());

        let default = ScrollState::default();
        assert_eq!(default.offset(), 0);
        assert!(!default.following());
        assert_ne!(fresh, default); // the deliberate `new() != default()`
    }

    #[test]
    fn max_offset_saturates_when_the_content_fits() {
        // content <= viewport => nothing to scroll, max offset 0, at the end.
        let s = ScrollState::default();
        assert!(s.at_end(5, 10));
        assert!(s.at_end(0, 0));
        // content > viewport => max is the difference.
        let mut s = ScrollState::default();
        s.set_offset(3);
        s.clamp(100, 20);
        assert_eq!(s.offset(), 3);
        assert!(!s.at_end(100, 20));
        s.set_offset(80);
        s.clamp(100, 20);
        assert!(s.at_end(100, 20));
    }

    #[test]
    fn set_offset_is_raw_and_clamp_reconciles_it() {
        let mut s = ScrollState::new();
        s.set_offset(9_999); // explicit over-scroll, stored as-is
        assert_eq!(s.offset(), 9_999);
        assert!(!s.following()); // an explicit jump drops tail-follow
        s.clamp(50, 10);
        assert_eq!(s.offset(), 40); // reconciled to the last full window
        s.clamp(50, 10); // idempotent
        assert_eq!(s.offset(), 40);
    }

    #[test]
    fn scroll_by_saturates_clamps_and_tracks_following() {
        let mut s = ScrollState::default();
        s.scroll_by(5, 100, 10);
        assert_eq!(s.offset(), 5);
        assert!(!s.following());

        // Up past the start saturates at 0, not an underflow.
        s.scroll_by(-9_999, 100, 10);
        assert_eq!(s.offset(), 0);
        assert!(!s.following());

        // Down past the end parks at the last window and re-arms following.
        s.scroll_by(isize::MAX, 100, 10);
        assert_eq!(s.offset(), 90);
        assert!(s.following());
    }

    #[test]
    fn scroll_to_top_and_end_set_the_follow_intent() {
        let mut s = ScrollState::new();
        s.scroll_to_end(200, 25);
        assert_eq!(s.offset(), 175);
        assert!(s.following());

        s.scroll_to_top();
        assert_eq!(s.offset(), 0);
        assert!(!s.following());
    }

    #[test]
    fn on_content_change_is_sticky_bottom_only_while_following() {
        // Following: streamed lines keep the tail pinned.
        let mut s = ScrollState::new();
        s.on_content_change(100, 20);
        assert_eq!(s.offset(), 80);
        s.on_content_change(140, 20); // 40 more lines stream in
        assert_eq!(s.offset(), 120);
        assert!(s.following());

        // Not following: the reading position is preserved, only clamped.
        s.scroll_by(-50, 140, 20); // user scrolls up -> stops following
        assert_eq!(s.offset(), 70);
        assert!(!s.following());
        s.on_content_change(300, 20); // lots more streams in
        assert_eq!(s.offset(), 70); // stays exactly where the user left it
        // …and a shrink (lines trimmed) clamps it back into bounds.
        s.on_content_change(40, 20);
        assert_eq!(s.offset(), 20);
    }

    #[test]
    fn show_brings_a_child_into_view_with_the_smallest_move() {
        let mut s = ScrollState::default();
        // Child below the window -> push the bottom down to it.
        s.show(50, 3, 10, 100);
        assert_eq!(s.offset(), 43); // 50 + 3 - 10
        // Child already fully visible -> no move.
        s.show(45, 2, 10, 100);
        assert_eq!(s.offset(), 43);
        // Child above the window -> pull the top up to it.
        s.show(10, 1, 10, 100);
        assert_eq!(s.offset(), 10);
        // Child taller than the window -> align to its top, show the start.
        s.show(70, 25, 10, 100);
        assert_eq!(s.offset(), 70);
        // Revealing the very last rows arms tail-follow.
        s.show(99, 1, 10, 100);
        assert_eq!(s.offset(), 90);
        assert!(s.following());
    }

    #[test]
    fn show_is_total_for_degenerate_sizes() {
        let mut s = ScrollState::new();
        s.show(usize::MAX, usize::MAX, 0, 0); // no panic, no overflow
        assert_eq!(s.offset(), 0);
        // Degenerate huge sizes saturate to a max offset of 0.
        s.show(usize::MAX, 1, usize::MAX, usize::MAX);
        assert_eq!(s.offset(), 0);
    }

    /// The totality property (the iter-25 rule, mirroring
    /// [`FocusRing`](crate::focus::FocusRing)'s and
    /// [`TextEdit`](crate::text_edit::TextEdit)'s): any sequence of any
    /// operation, over randomly-sized content/viewport (including the
    /// degenerate zeros and huge values), never panics and — once reconciled
    /// against the current layout the way the reducer does each frame
    /// ([`clamp`](ScrollState::clamp)) — always keeps
    /// `offset <= content.saturating_sub(viewport)`.
    #[test]
    fn any_sequence_of_operations_is_total_and_stays_in_bounds() {
        // Fixed-seed LCG keeps the run deterministic with no rand dep
        // (rstui-core is dependency-free) — the technique focus.rs uses.
        let mut state: u64 = 0x5371_1cef_a11d_0c75;
        let mut rng = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state
        };

        let mut s = ScrollState::new();
        for _ in 0..20_000 {
            // Sizes spanning the degenerate corners and large values.
            let pick = |r: u64| match r % 5 {
                0 => 0usize,
                1 => 1,
                2 => (r >> 8) as usize % 64,
                3 => usize::MAX,
                _ => (r >> 16) as usize % 4096,
            };
            let content = pick(rng());
            let viewport = pick(rng());

            match rng() % 9 {
                0 => s.set_offset((rng() >> 3) as usize),
                1 => s.clamp(content, viewport),
                2 => {
                    let _ = s.at_end(content, viewport);
                }
                3 => {
                    let delta = (rng() as i64 as isize).wrapping_sub(isize::MAX / 2);
                    s.scroll_by(delta, content, viewport);
                }
                4 => s.scroll_to_top(),
                5 => s.scroll_to_end(content, viewport),
                6 => s.on_content_change(content, viewport),
                7 => s.show(
                    (rng() >> 5) as usize,
                    (rng() >> 9) as usize % 256,
                    viewport,
                    content,
                ),
                _ => s = ScrollState::new(),
            }

            // Reconcile against the current layout, as the reducer/widget
            // does every frame, then assert the offset invariant holds.
            s.clamp(content, viewport);
            assert!(
                s.offset() <= content.saturating_sub(viewport),
                "offset escaped 0..=max after clamp"
            );
            // at_end agrees with the offset/max relationship.
            assert_eq!(
                s.at_end(content, viewport),
                s.offset() >= content.saturating_sub(viewport)
            );
        }
        // Reaching here proves no operation panicked for any input.
    }
}
