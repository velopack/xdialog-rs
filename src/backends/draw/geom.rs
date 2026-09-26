//! Geometry in logical px (f64): the subset of kurbo's API the dialogs use, with kurbo's semantics.

/// A position.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    /// Horizontal, growing to the right.
    pub x: f64,
    /// Vertical, growing downwards.
    pub y: f64,
}

/// A displacement.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    /// Horizontal component.
    pub x: f64,
    /// Vertical component.
    pub y: f64,
}

/// A width and height.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

/// An axis-aligned rect from `(x0, y0)` (top-left) to `(x1, y1)` (bottom-right).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Point {
    /// The origin.
    pub const ZERO: Point = Point::new(0.0, 0.0);

    /// The point at `(x, y)`.
    pub const fn new(x: f64, y: f64) -> Self {
        Point { x, y }
    }

    /// The displacement from the origin to this point.
    pub fn to_vec2(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

impl Vec2 {
    /// No displacement.
    pub const ZERO: Vec2 = Vec2::new(0.0, 0.0);

    /// A displacement by `(x, y)`.
    pub const fn new(x: f64, y: f64) -> Self {
        Vec2 { x, y }
    }
}

impl Size {
    pub const ZERO: Size = Size::new(0.0, 0.0);

    pub const fn new(width: f64, height: f64) -> Self {
        Size { width, height }
    }
}

impl Rect {
    pub const ZERO: Rect = Rect::new(0.0, 0.0, 0.0, 0.0);

    pub const fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Rect { x0, y0, x1, y1 }
    }

    pub fn from_origin_size(origin: Point, size: Size) -> Self {
        Rect::new(origin.x, origin.y, origin.x + size.width, origin.y + size.height)
    }

    /// The rect spanned by two corners (normalised: `x0 <= x1`, `y0 <= y1`).
    pub fn from_points(a: Point, b: Point) -> Self {
        Rect::new(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y))
    }

    pub fn from_center_size(center: Point, size: Size) -> Self {
        let (hw, hh) = (size.width / 2.0, size.height / 2.0);
        Rect::new(center.x - hw, center.y - hh, center.x + hw, center.y + hh)
    }

    pub fn origin(&self) -> Point {
        Point::new(self.x0, self.y0)
    }

    #[allow(dead_code)] // kurbo parity
    pub fn size(&self) -> Size {
        Size::new(self.width(), self.height())
    }

    pub fn center(&self) -> Point {
        Point::new(0.5 * (self.x0 + self.x1), 0.5 * (self.y0 + self.y1))
    }

    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    /// Half-open: the top and left edges are inside, the bottom and right edges are not.
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x0 && p.x < self.x1 && p.y >= self.y0 && p.y < self.y1
    }

    /// Grown by `dx` on the left and right and `dy` on the top and bottom (negative: shrunk).
    pub fn inflate(&self, dx: f64, dy: f64) -> Rect {
        Rect::new(self.x0 - dx, self.y0 - dy, self.x1 + dx, self.y1 + dy)
    }

    /// Grown by `d` on every side (kurbo's sign: positive grows, negative shrinks).
    pub fn inset(&self, d: f64) -> Rect {
        self.inflate(d, d)
    }

    /// The overlap; when there is none, a zero-area rect at the overlap edge.
    pub fn intersect(&self, other: Rect) -> Rect {
        let x0 = self.x0.max(other.x0);
        let y0 = self.y0.max(other.y0);
        let x1 = self.x1.min(other.x1);
        let y1 = self.y1.min(other.y1);
        Rect::new(x0, y0, x1.max(x0), y1.max(y0))
    }

    /// Whether the area is zero (or negative).
    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    /// Every coordinate multiplied by `k` (logical -> physical px).
    pub fn scale(&self, k: f64) -> Rect {
        Rect::new(self.x0 * k, self.y0 * k, self.x1 * k, self.y1 * k)
    }
}

impl core::ops::Add<Vec2> for Point {
    type Output = Point;

    fn add(self, v: Vec2) -> Point {
        Point::new(self.x + v.x, self.y + v.y)
    }
}

impl core::ops::Sub<Vec2> for Point {
    type Output = Point;

    fn sub(self, v: Vec2) -> Point {
        Point::new(self.x - v.x, self.y - v.y)
    }
}

impl core::ops::Sub<Point> for Point {
    type Output = Vec2;

    fn sub(self, p: Point) -> Vec2 {
        Vec2::new(self.x - p.x, self.y - p.y)
    }
}

impl core::ops::Mul<f64> for Size {
    type Output = Size;

    fn mul(self, k: f64) -> Size {
        Size::new(self.width * k, self.height * k)
    }
}

impl core::ops::Mul<f64> for Vec2 {
    type Output = Vec2;

    fn mul(self, k: f64) -> Vec2 {
        Vec2::new(self.x * k, self.y * k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_constructors_and_accessors() {
        let r = Rect::from_origin_size(Point::new(1.0, 2.0), Size::new(10.0, 4.0));
        assert_eq!(r, Rect::new(1.0, 2.0, 11.0, 6.0));
        assert_eq!(r.origin(), Point::new(1.0, 2.0));
        assert_eq!(r.size(), Size::new(10.0, 4.0));
        assert_eq!(r.center(), Point::new(6.0, 4.0));
        assert_eq!(Rect::from_center_size(r.center(), r.size()), r);
        assert_eq!(Rect::from_points(Point::new(11.0, 2.0), Point::new(1.0, 6.0)), r);
        assert_eq!(r.scale(1.5), Rect::new(1.5, 3.0, 16.5, 9.0));
    }

    #[test]
    fn contains_is_half_open() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(r.contains(Point::new(0.0, 0.0)));
        assert!(r.contains(Point::new(9.99, 9.99)));
        assert!(!r.contains(Point::new(10.0, 5.0)));
        assert!(!r.contains(Point::new(5.0, 10.0)));
        assert!(!r.contains(Point::new(-0.01, 5.0)));
    }

    #[test]
    fn inflate_inset_and_intersect() {
        let r = Rect::new(10.0, 10.0, 20.0, 30.0);
        assert_eq!(r.inflate(1.0, 2.0), Rect::new(9.0, 8.0, 21.0, 32.0));
        assert_eq!(r.inflate(-1.0, -2.0), Rect::new(11.0, 12.0, 19.0, 28.0));
        assert_eq!(r.inset(2.0), Rect::new(8.0, 8.0, 22.0, 32.0));
        assert_eq!(r.inset(-2.0), Rect::new(12.0, 12.0, 18.0, 28.0));
        assert_eq!(r.intersect(Rect::new(15.0, 0.0, 40.0, 20.0)), Rect::new(15.0, 10.0, 20.0, 20.0));
        let none = r.intersect(Rect::new(25.0, 35.0, 40.0, 40.0));
        assert!(none.is_empty());
        assert_eq!(none, Rect::new(25.0, 35.0, 25.0, 35.0));
        assert!(!r.is_empty());
        assert!(Rect::ZERO.is_empty());
    }

    #[test]
    fn operators() {
        let p = Point::new(1.0, 2.0);
        let v = Vec2::new(3.0, 4.0);
        assert_eq!(p + v, Point::new(4.0, 6.0));
        assert_eq!(p - v, Point::new(-2.0, -2.0));
        assert_eq!(Point::new(4.0, 6.0) - p, v);
        assert_eq!(p.to_vec2(), Vec2::new(1.0, 2.0));
        assert_eq!(v * 2.0, Vec2::new(6.0, 8.0));
        assert_eq!(Size::new(2.0, 3.0) * 1.5, Size::new(3.0, 4.5));
    }
}
