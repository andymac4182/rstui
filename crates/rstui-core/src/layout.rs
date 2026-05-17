//! Splitting a [`Rect`] into sub-regions with constraints.
//!
//! Every multi-region UI starts here: a [`Layout`] takes an area, a
//! [`Direction`], and a list of [`Constraint`]s, and divides the area into one
//! contiguous [`Rect`] per constraint. The segments always tile the available
//! span exactly — there are no gap cells the caller has to reason about — which
//! is the property that makes layouts compose cleanly with
//! [`Buffer`](crate::Buffer) drawing.
//!
//! ```text
//! Layout::horizontal([Length(10), Fill(1)]).split(area)
//! ┌──────────┬───────────────────────────────────────┐
//! │ Length 10│                Fill 1                  │
//! └──────────┴───────────────────────────────────────┘
//! ```
//!
//! # Determinism over a constraint solver
//!
//! ratatui resolves constraints with a Cassowary (`kasuari`) solver and `f32`
//! arithmetic. `rstui-core` is dependency-free and prizes byte-for-byte
//! reproducibility, so this module is instead a small, fully specified
//! *divider*: integer-only math (no floats, no platform-dependent rounding) and
//! a documented priority order rather than soft/hard constraint weights. The
//! [`Constraint`] vocabulary matches ratatui's so existing knowledge transfers,
//! but the resolution rule is rstui's own and is spelled out on [`Layout`].
//!
//! Constraint *alignment* modes (ratatui's `Flex`: centering or spreading
//! segments inside leftover space) are a deliberately deferred surface; v1
//! always feeds leftover space into [`Fill`](Constraint::Fill) /
//! [`Min`](Constraint::Min) / [`Max`](Constraint::Max) constraints, or, when
//! there are none, into the last segment.
//!
//! # Example
//!
//! ```
//! use rstui_core::{Constraint, Layout, Rect};
//!
//! let area = Rect::new(0, 0, 40, 10);
//! let [header, body, footer] = Layout::vertical([
//!     Constraint::Length(1),
//!     Constraint::Fill(1),
//!     Constraint::Length(1),
//! ])
//! .areas(area);
//!
//! assert_eq!(header, Rect::new(0, 0, 40, 1));
//! assert_eq!(body, Rect::new(0, 1, 40, 8));
//! assert_eq!(footer, Rect::new(0, 9, 40, 1));
//! ```

use crate::geometry::{Margin, Rect};

/// Horizontal placement of content within an available span.
///
/// A primitive shared by the [`text`](crate::text) model ([`Line`](crate::Line)
/// and [`Text`](crate::Text) alignment) and by widgets (a framing block's
/// title, a paragraph's rows). It lives in `layout` because placement within a
/// span is a layout concern, not a property of any one widget — matching
/// ratatui, where `Alignment` is also a core layout type.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Alignment {
    /// Flush with the start of the span.
    #[default]
    Left,
    /// Centered, with any odd remainder biased toward the start.
    Center,
    /// Flush with the end of the span.
    Right,
}

/// The axis along which a [`Layout`] arranges its segments.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Segments are placed side by side, left to right.
    Horizontal,
    /// Segments are stacked top to bottom. This is the default, matching the
    /// way most terminal UIs are composed.
    #[default]
    Vertical,
}

impl Direction {
    /// The perpendicular direction (`Horizontal` ↔ `Vertical`).
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Horizontal => Self::Vertical,
            Self::Vertical => Self::Horizontal,
        }
    }
}

/// How much of a [`Layout`]'s span a single segment should occupy.
///
/// The variants mirror ratatui's so existing layouts read the same; the
/// resolution semantics are rstui's own deterministic divider, documented on
/// [`Layout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Constraint {
    /// An exact number of cells. Does not grow or shrink unless the area is
    /// too small to fit every fixed constraint.
    Length(u16),
    /// A percentage `0..=100` of the area, rounded to the nearest cell.
    Percentage(u16),
    /// A `numerator / denominator` fraction of the area, rounded to the
    /// nearest cell. A zero denominator is treated as `1`.
    Ratio(u32, u32),
    /// At least this many cells; absorbs leftover space (weight 1) above the
    /// floor.
    Min(u16),
    /// At most this many cells; absorbs leftover space (weight 1) up to the
    /// ceiling.
    Max(u16),
    /// Takes a share of the leftover space proportional to this weight,
    /// relative to the other `Fill`/`Min`/`Max` segments.
    Fill(u16),
}

impl Constraint {
    /// Resolves this constraint against a single available `length`,
    /// independent of any sibling constraints.
    ///
    /// This is the per-axis sizing primitive (handy for one-off clamping); the
    /// full multi-segment behavior is [`Layout::split`].
    #[must_use]
    pub fn apply(self, length: u16) -> u16 {
        match self {
            Self::Length(v) | Self::Fill(v) => length.min(v),
            Self::Max(m) => length.min(m),
            Self::Min(m) => length.max(m),
            Self::Percentage(p) => {
                let cells = div_round(u64::from(p) * u64::from(length), 100);
                length.min(cells as u16)
            }
            Self::Ratio(num, den) => {
                let den = u64::from(den.max(1));
                let cells = div_round(u64::from(num) * u64::from(length), den);
                length.min(cells.min(u64::from(length)) as u16)
            }
        }
    }

    /// `[Length(v); _]` from an iterator of lengths.
    pub fn from_lengths<I: IntoIterator<Item = u16>>(lengths: I) -> Vec<Self> {
        lengths.into_iter().map(Self::Length).collect()
    }

    /// `[Percentage(v); _]` from an iterator of percentages.
    pub fn from_percentages<I: IntoIterator<Item = u16>>(percentages: I) -> Vec<Self> {
        percentages.into_iter().map(Self::Percentage).collect()
    }

    /// `[Ratio(n, d); _]` from an iterator of `(numerator, denominator)` pairs.
    pub fn from_ratios<I: IntoIterator<Item = (u32, u32)>>(ratios: I) -> Vec<Self> {
        ratios.into_iter().map(|(n, d)| Self::Ratio(n, d)).collect()
    }

    /// `[Fill(v); _]` from an iterator of fill weights.
    pub fn from_fills<I: IntoIterator<Item = u16>>(fills: I) -> Vec<Self> {
        fills.into_iter().map(Self::Fill).collect()
    }

    /// `[Min(v); _]` from an iterator of minimums.
    pub fn from_mins<I: IntoIterator<Item = u16>>(mins: I) -> Vec<Self> {
        mins.into_iter().map(Self::Min).collect()
    }

    /// `[Max(v); _]` from an iterator of maximums.
    pub fn from_maxes<I: IntoIterator<Item = u16>>(maxes: I) -> Vec<Self> {
        maxes.into_iter().map(Self::Max).collect()
    }
}

impl From<u16> for Constraint {
    /// A bare `u16` is a [`Constraint::Length`], so `Layout::vertical([1, 3])`
    /// reads naturally.
    fn from(length: u16) -> Self {
        Self::Length(length)
    }
}

/// A reusable recipe for dividing a [`Rect`] into contiguous sub-regions.
///
/// # Resolution algorithm
///
/// Given an area, [`split`](Self::split) first shrinks it by the configured
/// margins, then divides the main-axis span (`width` for
/// [`Horizontal`](Direction::Horizontal), `height` for
/// [`Vertical`](Direction::Vertical)):
///
/// 1. Inter-segment [`spacing`](Self::spacing) is reserved up front:
///    `available = span − spacing × (segments − 1)`.
/// 2. **Fixed** constraints ([`Length`](Constraint::Length),
///    [`Percentage`](Constraint::Percentage), [`Ratio`](Constraint::Ratio))
///    request a size computed from `available`. [`Min`](Constraint::Min)
///    contributes its value as a floor.
/// 3. If those requests fit, the **leftover** is split among the **flexible**
///    constraints ([`Fill`](Constraint::Fill) by its weight,
///    [`Min`](Constraint::Min)/[`Max`](Constraint::Max) with weight 1),
///    water-filling so [`Max`](Constraint::Max) ceilings are never exceeded.
///    With no flexible constraints, the leftover is added to the **last**
///    segment so the area is always fully tiled.
/// 4. If the fixed requests overflow `available`, every segment is scaled down
///    proportionally to fit.
/// 5. Any integer-rounding remainder is given to the last segment, so the
///    segment sizes plus spacing sum to the span exactly.
///
/// The result is always one [`Rect`] per constraint, laid end to end with
/// `spacing` cells between them, fully contained in the area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    direction: Direction,
    constraints: Vec<Constraint>,
    horizontal_margin: u16,
    vertical_margin: u16,
    spacing: u16,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            direction: Direction::Vertical,
            constraints: Vec::new(),
            horizontal_margin: 0,
            vertical_margin: 0,
            spacing: 0,
        }
    }
}

impl Layout {
    /// Creates a layout with the given direction and constraints.
    ///
    /// Items are anything `Into<Constraint>`, so `u16` lengths work directly.
    pub fn new<I, C>(direction: Direction, constraints: I) -> Self
    where
        I: IntoIterator<Item = C>,
        C: Into<Constraint>,
    {
        Self {
            direction,
            constraints: constraints.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    /// A top-to-bottom layout with the given constraints.
    pub fn vertical<I, C>(constraints: I) -> Self
    where
        I: IntoIterator<Item = C>,
        C: Into<Constraint>,
    {
        Self::new(Direction::Vertical, constraints)
    }

    /// A left-to-right layout with the given constraints.
    pub fn horizontal<I, C>(constraints: I) -> Self
    where
        I: IntoIterator<Item = C>,
        C: Into<Constraint>,
    {
        Self::new(Direction::Horizontal, constraints)
    }

    /// Sets the split direction.
    #[must_use]
    pub fn direction(mut self, direction: Direction) -> Self {
        self.direction = direction;
        self
    }

    /// Replaces the constraint list.
    #[must_use]
    pub fn constraints<I, C>(mut self, constraints: I) -> Self
    where
        I: IntoIterator<Item = C>,
        C: Into<Constraint>,
    {
        self.constraints = constraints.into_iter().map(Into::into).collect();
        self
    }

    /// Insets the area by `margin` cells on all four sides before splitting.
    #[must_use]
    pub fn margin(mut self, margin: u16) -> Self {
        self.horizontal_margin = margin;
        self.vertical_margin = margin;
        self
    }

    /// Insets the area by `margin` cells on the left and right edges.
    #[must_use]
    pub fn horizontal_margin(mut self, margin: u16) -> Self {
        self.horizontal_margin = margin;
        self
    }

    /// Insets the area by `margin` cells on the top and bottom edges.
    #[must_use]
    pub fn vertical_margin(mut self, margin: u16) -> Self {
        self.vertical_margin = margin;
        self
    }

    /// Reserves `spacing` empty cells between adjacent segments.
    #[must_use]
    pub fn spacing(mut self, spacing: u16) -> Self {
        self.spacing = spacing;
        self
    }

    /// Divides `area` into one [`Rect`] per constraint.
    ///
    /// The rectangles are contiguous along the layout direction (with
    /// [`spacing`](Self::spacing) gaps), span the full cross-axis, and are
    /// entirely contained in `area` after margins.
    ///
    /// This allocates a fresh `Vec` every call. A widget that splits the same
    /// container every frame should keep one `Vec<Rect>` and call
    /// [`split_into`](Self::split_into) instead, which reuses that buffer.
    #[must_use]
    pub fn split(&self, area: Rect) -> Vec<Rect> {
        let mut rects = Vec::new();
        self.split_into(area, &mut rects);
        rects
    }

    /// Like [`split`](Self::split) but writes the segments into a caller-owned
    /// `out`, clearing it first.
    ///
    /// `out` is `clear`ed (length reset to 0, capacity kept) and then refilled,
    /// so a caller that keeps one `Vec<Rect>` across frames and passes it here
    /// every frame performs **no heap allocation** once the buffer has grown to
    /// the segment count. This is the per-frame path the layout-heavy widgets
    /// (table, grid, form, flow, split-pane) take; [`split`](Self::split) is
    /// the convenience wrapper for one-off callers and remains byte-identical.
    pub fn split_into(&self, area: Rect, out: &mut Vec<Rect>) {
        out.clear();
        out.reserve(self.constraints.len());
        self.for_each_rect(area, |rect| out.push(rect));
    }

    /// Solves the layout and invokes `emit` once per segment, in order. The
    /// single owner of the size→geometry math: [`split_into`](Self::split_into)
    /// pushes into a `Vec`, [`areas`](Self::areas) writes into a stack array,
    /// and neither path allocates here (the scratch is stack-backed for the
    /// small-N common case via `ScratchBuf`).
    fn for_each_rect(&self, area: Rect, mut emit: impl FnMut(Rect)) {
        let n = self.constraints.len();
        if n == 0 {
            return;
        }

        let inner = area.inner(Margin::new(self.horizontal_margin, self.vertical_margin));
        let span = match self.direction {
            Direction::Horizontal => inner.width,
            Direction::Vertical => inner.height,
        };

        // Scratch-free for the small-N common case (layouts are 2–5
        // constraints): `solve` borrows a stack array sliced to `n` and only
        // touches the heap when `n` exceeds the inline capacity.
        let mut sizes = ScratchBuf::new(n);
        solve(span, &self.constraints, self.spacing, sizes.as_mut_slice());

        let mut cursor = match self.direction {
            Direction::Horizontal => inner.x,
            Direction::Vertical => inner.y,
        };
        for (i, &size) in sizes.as_slice().iter().enumerate() {
            let rect = match self.direction {
                Direction::Horizontal => Rect::new(cursor, inner.y, size, inner.height),
                Direction::Vertical => Rect::new(inner.x, cursor, inner.width, size),
            };
            emit(rect);
            cursor = cursor.saturating_add(size);
            if i + 1 < n {
                cursor = cursor.saturating_add(self.spacing);
            }
        }
    }

    /// Like [`split`](Self::split) but returns a fixed-size array, so callers
    /// can destructure: `let [a, b] = layout.areas(area);`.
    ///
    /// # Panics
    ///
    /// Panics if `N` does not equal the number of constraints.
    #[must_use]
    pub fn areas<const N: usize>(&self, area: Rect) -> [Rect; N] {
        let count = self.constraints.len();
        assert_eq!(
            count, N,
            "Layout::areas::<{N}> called with {count} constraint(s)",
        );
        let mut out = [Rect::ZERO; N];
        let mut i = 0;
        self.for_each_rect(area, |rect| {
            out[i] = rect;
            i += 1;
        });
        out
    }
}

/// Integer division rounding half away from zero (all inputs are non-negative).
fn div_round(num: u64, den: u64) -> u64 {
    debug_assert!(den != 0);
    (num + den / 2) / den
}

/// Inline capacity for [`solve`]'s per-segment scratch. Real layouts are 2–5
/// constraints (header/body/footer, sidebar/content/aside, a form's rows); 16
/// covers every layout in the codebase with zero heap traffic and keeps the
/// stack frame small (a `[u64; 16]` is 128 B). Larger layouts fall back to a
/// one-off `Vec`, which is correct but no longer the hot path.
const SCRATCH_INLINE: usize = 16;

/// A length-`n` scratch slice that lives on the stack while `n <=
/// SCRATCH_INLINE` and spills to a single heap `Vec` only for the rare large
/// layout. `T: Copy + Default` so the inline array can be value-initialized
/// with no `unsafe` and no per-element drop. Used both for [`solve`]'s
/// per-segment work arrays and for the size output buffer.
enum ScratchBuf<T: Copy + Default> {
    /// Stack-backed: a fixed array plus the live length (`<= SCRATCH_INLINE`).
    Inline {
        data: [T; SCRATCH_INLINE],
        len: usize,
    },
    /// Heap fallback for `n > SCRATCH_INLINE`; a single allocation.
    Spill(Vec<T>),
}

impl<T: Copy + Default> ScratchBuf<T> {
    /// A zero-initialized buffer of length `n` (stack if it fits inline).
    fn new(n: usize) -> Self {
        if n <= SCRATCH_INLINE {
            Self::Inline {
                data: [T::default(); SCRATCH_INLINE],
                len: n,
            }
        } else {
            Self::Spill(vec![T::default(); n])
        }
    }

    /// A buffer of length `n` with every element set to `value`.
    fn filled(n: usize, value: T) -> Self {
        if n <= SCRATCH_INLINE {
            Self::Inline {
                data: [value; SCRATCH_INLINE],
                len: n,
            }
        } else {
            Self::Spill(vec![value; n])
        }
    }

    #[inline]
    fn as_slice(&self) -> &[T] {
        match self {
            Self::Inline { data, len } => &data[..*len],
            Self::Spill(v) => v.as_slice(),
        }
    }

    #[inline]
    fn as_mut_slice(&mut self) -> &mut [T] {
        match self {
            Self::Inline { data, len } => &mut data[..*len],
            Self::Spill(v) => v.as_mut_slice(),
        }
    }
}

/// Per-axis core: resolves `constraints` against a `total` span (including
/// `spacing` between segments) into one size per constraint, always summing to
/// the span exactly. See [`Layout`] for the documented algorithm.
///
/// Priority is `Min` floors → fixed requests (`Length`/`Percentage`/`Ratio`) →
/// flexible growth (`Fill`/`Min`/`Max`), so the canonical
/// `[Percentage(100), Min(20)]` reserve-the-sidebar idiom resolves the way
/// ratatui's solver does even though this is plain integer arithmetic.
fn solve(total: u16, constraints: &[Constraint], spacing: u16, out: &mut [u16]) {
    let n = constraints.len();
    debug_assert_eq!(
        out.len(),
        n,
        "solve out buffer must be one slot per constraint"
    );
    let gaps = u64::from(spacing) * (n as u64 - 1);
    let available = u64::from(total).saturating_sub(gaps);
    if available == 0 {
        out.fill(0);
        return;
    }
    let mut size_buf = ScratchBuf::<u64>::new(n);
    let size = size_buf.as_mut_slice();

    // Per-segment: a hard floor (Min), a fixed request
    // (Length/Percentage/Ratio), a flex weight, and an upper ceiling.
    // Stack-backed for the small-N common case; see `ScratchBuf`.
    let mut floor_buf = ScratchBuf::<u64>::new(n);
    let mut req_buf = ScratchBuf::<u64>::new(n);
    let mut weight_buf = ScratchBuf::<u64>::new(n);
    let mut ceil_buf = ScratchBuf::<u64>::filled(n, u64::MAX);
    let floor = floor_buf.as_mut_slice();
    let req = req_buf.as_mut_slice();
    let weight = weight_buf.as_mut_slice();
    let ceil = ceil_buf.as_mut_slice();
    for (i, c) in constraints.iter().enumerate() {
        match *c {
            Constraint::Length(v) => req[i] = u64::from(v),
            Constraint::Percentage(p) => {
                req[i] = div_round(u64::from(p) * available, 100).min(available);
            }
            Constraint::Ratio(num, den) => {
                req[i] =
                    div_round(u64::from(num) * available, u64::from(den.max(1))).min(available);
            }
            Constraint::Min(v) => {
                floor[i] = u64::from(v);
                weight[i] = 1;
            }
            Constraint::Max(v) => {
                weight[i] = 1;
                ceil[i] = u64::from(v);
            }
            Constraint::Fill(w) => weight[i] = u64::from(w),
        }
    }

    let floor_sum: u64 = floor.iter().sum();
    if floor_sum >= available {
        // The reserved minimums alone overflow the span: honor them in
        // proportion and leave nothing for anyone else.
        for i in 0..n {
            size[i] = div_round(floor[i] * available, floor_sum);
        }
    } else {
        size.copy_from_slice(floor);
        let budget = available - floor_sum;
        let req_sum: u64 = req.iter().sum();
        if req_sum > budget {
            // Fixed requests overflow the post-floor budget: scale them down
            // proportionally; flexible segments get only their floor.
            for i in 0..n {
                if req[i] > 0 {
                    size[i] = div_round(req[i] * budget, req_sum);
                }
            }
        } else {
            for i in 0..n {
                size[i] += req[i];
            }
            let mut leftover = budget - req_sum;
            let total_weight: u64 = weight.iter().sum();
            if total_weight == 0 {
                // No flexible segment to absorb slack: grow the last one so
                // the span is still fully tiled.
                size[n - 1] += leftover;
            } else {
                // Water-fill the leftover by weight, freezing any Max segment
                // that reaches its ceiling and redistributing the excess.
                let mut frozen_buf = ScratchBuf::<bool>::new(n);
                let frozen = frozen_buf.as_mut_slice();
                while leftover > 0 {
                    let active: u64 = (0..n)
                        .filter(|&i| weight[i] > 0 && !frozen[i])
                        .map(|i| weight[i])
                        .sum();
                    if active == 0 {
                        break;
                    }
                    let mut progressed = false;
                    let mut spent = 0u64;
                    for i in 0..n {
                        if weight[i] == 0 || frozen[i] {
                            continue;
                        }
                        let mut grant = div_round(leftover * weight[i], active);
                        if grant == 0 {
                            grant = 1; // forward progress on tiny leftovers
                        }
                        let headroom = ceil[i].saturating_sub(size[i]);
                        let grant = grant.min(headroom).min(leftover - spent);
                        if grant > 0 {
                            size[i] += grant;
                            spent += grant;
                            progressed = true;
                        }
                        if size[i] >= ceil[i] {
                            frozen[i] = true;
                        }
                        if spent == leftover {
                            break;
                        }
                    }
                    leftover -= spent;
                    if !progressed {
                        break; // every flexible segment is at its ceiling
                    }
                }
            }
        }
    }

    // Hand any integer-rounding remainder to the last segment so the sizes
    // plus spacing sum to the span exactly.
    let assigned: u64 = size.iter().sum();
    match available.cmp(&assigned) {
        std::cmp::Ordering::Greater => size[n - 1] += available - assigned,
        std::cmp::Ordering::Less => {
            size[n - 1] = size[n - 1].saturating_sub(assigned - available);
        }
        std::cmp::Ordering::Equal => {}
    }

    for (slot, &s) in out.iter_mut().zip(size.iter()) {
        *slot = s.min(u64::from(u16::MAX)) as u16;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The invariant every split must hold: one rect per constraint, all
    /// inside the area, contiguous along the direction with `spacing` gaps.
    fn assert_tiles(layout: &Layout, area: Rect) -> Vec<Rect> {
        let rects = layout.split(area);
        assert_eq!(rects.len(), layout.constraints.len());
        // `split_into` must be byte-identical to `split` for every case the
        // suite exercises (and `split` is now a wrapper over it).
        let mut reused = vec![Rect::new(9, 9, 9, 9)]; // pre-dirtied buffer
        layout.split_into(area, &mut reused);
        assert_eq!(reused, rects, "split_into != split");
        let inner = area.inner(Margin::new(
            layout.horizontal_margin,
            layout.vertical_margin,
        ));
        for (i, w) in rects.windows(2).enumerate() {
            let (a, b) = (w[0], w[1]);
            match layout.direction {
                Direction::Vertical => {
                    assert_eq!(a.x, inner.x, "seg {i} x");
                    assert_eq!(a.width, inner.width, "seg {i} width");
                    assert_eq!(a.bottom() + layout.spacing, b.y, "seg {i} not contiguous");
                }
                Direction::Horizontal => {
                    assert_eq!(a.y, inner.y, "seg {i} y");
                    assert_eq!(a.height, inner.height, "seg {i} height");
                    assert_eq!(a.right() + layout.spacing, b.x, "seg {i} not contiguous");
                }
            }
        }
        if let (Some(first), Some(last)) = (rects.first(), rects.last()) {
            match layout.direction {
                Direction::Vertical => {
                    assert_eq!(first.y, inner.y);
                    assert!(last.bottom() <= inner.bottom());
                }
                Direction::Horizontal => {
                    assert_eq!(first.x, inner.x);
                    assert!(last.right() <= inner.right());
                }
            }
        }
        rects
    }

    #[test]
    fn empty_constraints_split_to_nothing() {
        assert!(
            Layout::vertical(Vec::<Constraint>::new())
                .split(Rect::new(0, 0, 10, 10))
                .is_empty()
        );
    }

    #[test]
    fn length_plus_fill_is_a_header_body_split() {
        let r = assert_tiles(
            &Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]),
            Rect::new(0, 0, 40, 10),
        );
        assert_eq!(r, vec![Rect::new(0, 0, 40, 1), Rect::new(0, 1, 40, 9)]);
    }

    #[test]
    fn fill_weights_split_proportionally_and_tile_exactly() {
        let r = assert_tiles(
            &Layout::horizontal([
                Constraint::Fill(1),
                Constraint::Fill(2),
                Constraint::Fill(1),
            ]),
            Rect::new(0, 0, 40, 3),
        );
        let widths: Vec<_> = r.iter().map(|x| x.width).collect();
        assert_eq!(widths, vec![10, 20, 10]);
        assert_eq!(widths.iter().sum::<u16>(), 40);
    }

    #[test]
    fn last_segment_absorbs_slack_when_no_flex_constraint() {
        // Two fixed lengths under-fill; the last grows so the area is tiled.
        let r = assert_tiles(
            &Layout::horizontal([Constraint::Length(10), Constraint::Length(10)]),
            Rect::new(0, 0, 50, 2),
        );
        assert_eq!(r, vec![Rect::new(0, 0, 10, 2), Rect::new(10, 0, 40, 2)]);
    }

    #[test]
    fn percentages_round_and_remainder_goes_to_last() {
        let r = assert_tiles(
            &Layout::horizontal([
                Constraint::Percentage(33),
                Constraint::Percentage(33),
                Constraint::Percentage(33),
            ]),
            Rect::new(0, 0, 100, 1),
        );
        let widths: Vec<_> = r.iter().map(|x| x.width).collect();
        // 33 + 33 + (100 - 66) so the span is fully tiled.
        assert_eq!(widths, vec![33, 33, 34]);
    }

    #[test]
    fn overflowing_fixed_requests_scale_down_to_fit() {
        let r = assert_tiles(
            &Layout::horizontal([Constraint::Length(80), Constraint::Length(80)]),
            Rect::new(0, 0, 100, 1),
        );
        let widths: Vec<_> = r.iter().map(|x| x.width).collect();
        assert_eq!(widths.iter().sum::<u16>(), 100);
        assert_eq!(widths, vec![50, 50]);
    }

    #[test]
    fn max_ceiling_is_never_exceeded_and_excess_redistributes() {
        // Sidebar pattern: a capped column next to a flexible body, with the
        // capped column listed *last* to exercise ceiling-aware remainder.
        let r = assert_tiles(
            &Layout::horizontal([Constraint::Fill(1), Constraint::Max(20)]),
            Rect::new(0, 0, 100, 1),
        );
        let widths: Vec<_> = r.iter().map(|x| x.width).collect();
        assert_eq!(widths, vec![80, 20]);
    }

    #[test]
    fn min_floor_is_respected_when_space_allows() {
        let r = assert_tiles(
            &Layout::horizontal([Constraint::Percentage(100), Constraint::Min(20)]),
            Rect::new(0, 0, 50, 1),
        );
        let widths: Vec<_> = r.iter().map(|x| x.width).collect();
        assert!(widths[1] >= 20, "Min(20) got {}", widths[1]);
        assert_eq!(widths.iter().sum::<u16>(), 50);
    }

    #[test]
    fn spacing_is_reserved_between_segments() {
        let layout = Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).spacing(2);
        let r = assert_tiles(&layout, Rect::new(0, 0, 100, 1));
        assert_eq!(r[0], Rect::new(0, 0, 49, 1));
        assert_eq!(r[1], Rect::new(51, 0, 49, 1));
        // Two 49-cell segments + a 2-cell gap exactly fills 100.
        assert_eq!(r[0].width + 2 + r[1].width, 100);
    }

    #[test]
    fn margins_inset_the_area_before_splitting() {
        let r = Layout::vertical([Constraint::Fill(1)])
            .margin(2)
            .split(Rect::new(0, 0, 20, 20));
        assert_eq!(r, vec![Rect::new(2, 2, 16, 16)]);
    }

    #[test]
    fn areas_destructures_into_a_fixed_array() {
        let area = Rect::new(0, 0, 30, 9);
        let [a, b, c] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Fill(1),
            Constraint::Length(2),
        ])
        .areas(area);
        assert_eq!(a, Rect::new(0, 0, 30, 2));
        assert_eq!(b, Rect::new(0, 2, 30, 5));
        assert_eq!(c, Rect::new(0, 7, 30, 2));
    }

    #[test]
    #[should_panic(expected = "Layout::areas::<2>")]
    fn areas_panics_on_constraint_count_mismatch() {
        let _: [Rect; 2] = Layout::vertical([Constraint::Fill(1)]).areas(Rect::new(0, 0, 4, 4));
    }

    #[test]
    fn zero_area_yields_zero_sized_but_well_formed_segments() {
        let r = Layout::vertical([Constraint::Fill(1), Constraint::Fill(1)])
            .split(Rect::new(3, 4, 0, 0));
        assert_eq!(r.len(), 2);
        for rect in r {
            assert!(rect.is_empty());
        }
    }

    #[test]
    fn constraint_apply_matches_documented_semantics() {
        assert_eq!(Constraint::Length(8).apply(20), 8);
        assert_eq!(Constraint::Length(40).apply(20), 20);
        assert_eq!(Constraint::Percentage(50).apply(20), 10);
        assert_eq!(Constraint::Ratio(1, 3).apply(30), 10);
        assert_eq!(Constraint::Ratio(1, 0).apply(7), 7); // den 0 → treated as 1
        assert_eq!(Constraint::Min(25).apply(20), 25);
        assert_eq!(Constraint::Max(5).apply(20), 5);
    }

    #[test]
    fn constraint_constructors_and_u16_conversion() {
        assert_eq!(
            Constraint::from_lengths([1, 2]),
            vec![Constraint::Length(1), Constraint::Length(2)]
        );
        assert_eq!(Constraint::from_fills([3]), vec![Constraint::Fill(3)]);
        assert_eq!(
            Constraint::from_ratios([(1, 2)]),
            vec![Constraint::Ratio(1, 2)]
        );
        let c: Constraint = 7u16.into();
        assert_eq!(c, Constraint::Length(7));
        // u16 items flow through Layout::new via Into<Constraint>.
        let r = Layout::horizontal([4u16, 6u16]).split(Rect::new(0, 0, 10, 1));
        assert_eq!(r, vec![Rect::new(0, 0, 4, 1), Rect::new(4, 0, 6, 1)]);
    }

    #[test]
    fn direction_default_and_opposite() {
        assert_eq!(Direction::default(), Direction::Vertical);
        assert_eq!(Direction::Horizontal.opposite(), Direction::Vertical);
        assert_eq!(Direction::Vertical.opposite(), Direction::Horizontal);
    }

    #[test]
    fn split_into_clears_a_dirty_buffer_and_matches_split() {
        let layout = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ]);
        let area = Rect::new(0, 0, 40, 10);
        // A buffer that already holds stale, longer content must be replaced
        // wholesale — not appended to, not partially overwritten.
        let mut buf = vec![Rect::new(7, 7, 7, 7); 9];
        layout.split_into(area, &mut buf);
        assert_eq!(buf, layout.split(area));
        assert_eq!(buf.len(), 3);
    }

    #[test]
    fn split_into_reuses_capacity_without_reallocating() {
        // The whole point of the API: a widget that keeps one buffer across
        // frames must not hit the allocator once it has grown to fit.
        let layout = Layout::horizontal([Constraint::Fill(1), Constraint::Min(20)]);
        let area = Rect::new(0, 0, 100, 4);
        let mut buf = Vec::new();
        layout.split_into(area, &mut buf);
        let ptr = buf.as_ptr();
        let cap = buf.capacity();
        for _ in 0..32 {
            layout.split_into(area, &mut buf);
        }
        assert_eq!(buf.as_ptr(), ptr, "buffer was reallocated across frames");
        assert_eq!(buf.capacity(), cap);
        assert_eq!(buf, layout.split(area));
    }

    #[test]
    fn split_into_empty_constraints_clears_to_nothing() {
        let mut buf = vec![Rect::new(1, 2, 3, 4)];
        Layout::vertical(Vec::<Constraint>::new()).split_into(Rect::new(0, 0, 10, 10), &mut buf);
        assert!(buf.is_empty());
    }

    #[test]
    fn large_n_layout_spills_scratch_to_heap_but_stays_correct() {
        // 20 > SCRATCH_INLINE (16): exercises the `ScratchBuf::Spill` path in
        // `solve`; the result must still tile exactly like any other layout.
        let n = 20usize;
        let layout = Layout::horizontal(vec![Constraint::Fill(1); n]);
        let r = assert_tiles(&layout, Rect::new(0, 0, 200, 1));
        assert_eq!(r.len(), n);
        assert_eq!(r.iter().map(|x| x.width).sum::<u16>(), 200);
        // Equal weights over 200 cells → 10 each, exactly tiled.
        for rect in &r {
            assert_eq!(rect.width, 10);
        }
    }
}
