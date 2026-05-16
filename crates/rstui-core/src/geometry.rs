//! Integer screen geometry.
//!
//! Terminals address cells with non-negative integer coordinates, so every
//! geometric type here is built on [`u16`]. [`Rect`] is the workhorse: it is
//! constructed through [`Rect::new`], which clamps the rectangle so that its
//! right and bottom edges can never overflow a `u16`. That invariant lets the
//! accessor methods stay panic-free without resorting to wrapping arithmetic.

/// A point in terminal cell coordinates.
///
/// The origin `(0, 0)` is the top-left cell; `x` grows rightwards and `y`
/// grows downwards, matching how terminals address the screen.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Position {
    /// Column, counted from the left edge.
    pub x: u16,
    /// Row, counted from the top edge.
    pub y: u16,
}

impl Position {
    /// The top-left cell.
    pub const ORIGIN: Self = Self { x: 0, y: 0 };

    /// Creates a position at `(x, y)`.
    #[must_use]
    pub const fn new(x: u16, y: u16) -> Self {
        Self { x, y }
    }
}

impl From<(u16, u16)> for Position {
    fn from((x, y): (u16, u16)) -> Self {
        Self { x, y }
    }
}

/// A width/height pair measured in terminal cells.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Size {
    /// Number of columns.
    pub width: u16,
    /// Number of rows.
    pub height: u16,
}

impl Size {
    /// Creates a size of `width` columns by `height` rows.
    #[must_use]
    pub const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }

    /// The number of cells this size covers.
    ///
    /// Returned as a [`u32`] because a full `u16` × `u16` grid does not fit in
    /// a `u16`.
    #[must_use]
    pub const fn area(self) -> u32 {
        self.width as u32 * self.height as u32
    }

    /// Returns `true` if either dimension is zero, so the size covers no cells.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

impl From<(u16, u16)> for Size {
    fn from((width, height): (u16, u16)) -> Self {
        Self { width, height }
    }
}

/// A symmetric inset applied when shrinking a [`Rect`] with [`Rect::inner`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Margin {
    /// Cells removed from both the left and right edges.
    pub horizontal: u16,
    /// Cells removed from both the top and bottom edges.
    pub vertical: u16,
}

impl Margin {
    /// Creates a margin with the given horizontal and vertical insets.
    #[must_use]
    pub const fn new(horizontal: u16, vertical: u16) -> Self {
        Self {
            horizontal,
            vertical,
        }
    }
}

/// An axis-aligned rectangle of terminal cells.
///
/// A `Rect` is half-open: it covers columns `x..x + width` and rows
/// `y..y + height`. [`Rect::new`] clamps `width`/`height` so that
/// [`Rect::right`] and [`Rect::bottom`] always fit in a `u16`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rect {
    /// Left edge (inclusive).
    pub x: u16,
    /// Top edge (inclusive).
    pub y: u16,
    /// Width in cells.
    pub width: u16,
    /// Height in cells.
    pub height: u16,
}

impl Rect {
    /// The empty rectangle at the origin.
    pub const ZERO: Self = Self {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    };

    /// Creates a rectangle at `(x, y)` with the given size.
    ///
    /// `width` and `height` are reduced if necessary so that `x + width` and
    /// `y + height` stay within `u16::MAX`. This keeps every edge accessor
    /// total and panic-free.
    #[must_use]
    pub const fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        let width = if x as u32 + width as u32 > u16::MAX as u32 {
            u16::MAX - x
        } else {
            width
        };
        let height = if y as u32 + height as u32 > u16::MAX as u32 {
            u16::MAX - y
        } else {
            height
        };
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Creates a rectangle at the origin with the given size.
    #[must_use]
    pub const fn from_size(size: Size) -> Self {
        Self::new(0, 0, size.width, size.height)
    }

    /// The number of cells the rectangle covers.
    #[must_use]
    pub const fn area(self) -> u32 {
        self.width as u32 * self.height as u32
    }

    /// Returns `true` if the rectangle covers no cells.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Left edge column (inclusive).
    #[must_use]
    pub const fn left(self) -> u16 {
        self.x
    }

    /// Right edge column (exclusive).
    ///
    /// Cannot overflow because [`Rect::new`] clamps the width.
    #[must_use]
    pub const fn right(self) -> u16 {
        self.x.saturating_add(self.width)
    }

    /// Top edge row (inclusive).
    #[must_use]
    pub const fn top(self) -> u16 {
        self.y
    }

    /// Bottom edge row (exclusive).
    ///
    /// Cannot overflow because [`Rect::new`] clamps the height.
    #[must_use]
    pub const fn bottom(self) -> u16 {
        self.y.saturating_add(self.height)
    }

    /// The top-left corner.
    #[must_use]
    pub const fn position(self) -> Position {
        Position::new(self.x, self.y)
    }

    /// The size of the rectangle.
    #[must_use]
    pub const fn size(self) -> Size {
        Size::new(self.width, self.height)
    }

    /// Returns `true` if `position` lies inside the rectangle.
    #[must_use]
    pub const fn contains(self, position: Position) -> bool {
        position.x >= self.left()
            && position.x < self.right()
            && position.y >= self.top()
            && position.y < self.bottom()
    }

    /// Returns `true` if the two rectangles share at least one cell.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.left() < other.right()
            && self.right() > other.left()
            && self.top() < other.bottom()
            && self.bottom() > other.top()
    }

    /// The largest rectangle contained in both `self` and `other`.
    ///
    /// Returns a zero-area rectangle when they do not overlap.
    #[must_use]
    pub fn intersection(self, other: Self) -> Self {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = self.right().min(other.right());
        let y2 = self.bottom().min(other.bottom());
        Self::new(x1, y1, x2.saturating_sub(x1), y2.saturating_sub(y1))
    }

    /// The smallest rectangle containing both `self` and `other`.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        let x1 = self.x.min(other.x);
        let y1 = self.y.min(other.y);
        let x2 = self.right().max(other.right());
        let y2 = self.bottom().max(other.bottom());
        Self::new(x1, y1, x2 - x1, y2 - y1)
    }

    /// Shrinks the rectangle inward by `margin` on each axis.
    ///
    /// If the margin is larger than the rectangle, a zero-area rectangle
    /// centered on the original is returned rather than wrapping.
    #[must_use]
    pub fn inner(self, margin: Margin) -> Self {
        let doubled_h = margin.horizontal.saturating_mul(2);
        let doubled_v = margin.vertical.saturating_mul(2);
        if self.width < doubled_h || self.height < doubled_v {
            Self::new(
                self.x.saturating_add(self.width / 2),
                self.y.saturating_add(self.height / 2),
                0,
                0,
            )
        } else {
            Self::new(
                self.x + margin.horizontal,
                self.y + margin.vertical,
                self.width - doubled_h,
                self.height - doubled_v,
            )
        }
    }

    /// Returns an iterator over every [`Position`] in the rectangle, in
    /// row-major (left-to-right, top-to-bottom) order.
    pub fn positions(self) -> impl Iterator<Item = Position> {
        (self.top()..self.bottom())
            .flat_map(move |y| (self.left()..self.right()).map(move |x| Position::new(x, y)))
    }
}

impl From<(Position, Size)> for Rect {
    fn from((position, size): (Position, Size)) -> Self {
        Self::new(position.x, position.y, size.width, size.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_area_uses_u32_to_avoid_overflow() {
        assert_eq!(Size::new(u16::MAX, u16::MAX).area(), 4_294_836_225);
        assert!(Size::new(0, 10).is_empty());
        assert!(!Size::new(3, 4).is_empty());
    }

    #[test]
    fn new_clamps_so_edges_never_overflow() {
        let r = Rect::new(u16::MAX - 1, 0, 50, 10);
        assert_eq!(r.right(), u16::MAX);
        assert_eq!(r.width, 1);

        let r = Rect::new(0, u16::MAX - 2, 10, 100);
        assert_eq!(r.bottom(), u16::MAX);
        assert_eq!(r.height, 2);
    }

    #[test]
    fn edges_and_accessors() {
        let r = Rect::new(2, 3, 10, 5);
        assert_eq!((r.left(), r.right(), r.top(), r.bottom()), (2, 12, 3, 8));
        assert_eq!(r.position(), Position::new(2, 3));
        assert_eq!(r.size(), Size::new(10, 5));
        assert_eq!(r.area(), 50);
    }

    #[test]
    fn contains_is_half_open() {
        let r = Rect::new(0, 0, 4, 4);
        assert!(r.contains(Position::new(0, 0)));
        assert!(r.contains(Position::new(3, 3)));
        assert!(!r.contains(Position::new(4, 0)));
        assert!(!r.contains(Position::new(0, 4)));
    }

    #[test]
    fn intersection_and_intersects_agree() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 10, 10);
        assert!(a.intersects(b));
        assert_eq!(a.intersection(b), Rect::new(5, 5, 5, 5));

        let c = Rect::new(100, 100, 2, 2);
        assert!(!a.intersects(c));
        assert!(a.intersection(c).is_empty());
    }

    #[test]
    fn union_is_the_bounding_box() {
        let a = Rect::new(0, 0, 2, 2);
        let b = Rect::new(10, 10, 2, 2);
        assert_eq!(a.union(b), Rect::new(0, 0, 12, 12));
    }

    #[test]
    fn inner_shrinks_and_collapses_gracefully() {
        let r = Rect::new(0, 0, 10, 10);
        assert_eq!(r.inner(Margin::new(2, 1)), Rect::new(2, 1, 6, 8));

        let collapsed = r.inner(Margin::new(100, 100));
        assert!(collapsed.is_empty());
        assert!(r.contains(collapsed.position()));
    }

    #[test]
    fn positions_iterates_row_major() {
        let r = Rect::new(1, 1, 2, 2);
        let visited: Vec<_> = r.positions().collect();
        assert_eq!(
            visited,
            vec![
                Position::new(1, 1),
                Position::new(2, 1),
                Position::new(1, 2),
                Position::new(2, 2),
            ]
        );
    }
}
