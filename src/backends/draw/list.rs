//! The display list themes record ([`Shape`], logical px) and its replay onto a [`Canvas`] with
//! the shared pixel snapping.
//!
//! Replay snaps rects and strokes to whole physical pixels (crisp edges at any scale) unless they
//! were recorded unsnapped (moving progress bar ends), and places text and images on whole pixels.
//! Canvases map logical px to physical px by `ppp` themselves and never snap.

use std::rc::Rc;

use super::{Canvas, Color, Image, Layout, Point, Rect};

/// How the ends of a [`Shape::Line`] look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LineCap {
    /// Ends exactly at the end points.
    Butt,
    /// A half-disc past each end point.
    Round,
}

/// One drawing command, logical px.
#[derive(Clone)]
pub(crate) enum Shape {
    /// Filled (rounded) rect. `snap: false`: drawn where it is (progress pills glide sub-pixel).
    Rect {
        rect: Rect,
        radius: f64,
        color: Color,
        snap: bool,
    },
    /// Filled (rounded) rect with a vertical gradient (`top` to `bottom`), edges snapped.
    Gradient {
        rect: Rect,
        radius: f64,
        top: Color,
        bottom: Color,
    },
    /// Filled closed polygon (never snapped).
    Polygon {
        points: Rc<[Point]>,
        color: Color,
    },
    /// Stroke of `width` centred on the (rounded) rect's edge.
    Stroke {
        rect: Rect,
        radius: f64,
        width: f64,
        color: Color,
    },
    Circle {
        center: Point,
        radius: f64,
        color: Color,
    },
    Line {
        from: Point,
        to: Point,
        width: f64,
        cap: LineCap,
        color: Color,
    },
    /// A text layout with its top-left at `pos`.
    Text {
        layout: Layout,
        pos: Point,
        color: Color,
    },
    Image {
        image: Image,
        rect: Rect,
    },
    /// Clip what follows (until the matching `PopClip`) to a rect.
    PushClip(Rect),
    PopClip,
}

/// One frame to present.
pub(crate) struct Frame<'a> {
    pub shapes: &'a [Shape],
    /// Physical px.
    pub size_px: [u32; 2],
    /// Physical px per logical px.
    pub ppp: f64,
    /// Window background: opaque, except on a translucent surface (macOS vibrancy).
    pub clear: Color,
}

/// `clear(frame.clear)`, then every shape with the snapping rules applied. Clips are balanced: a
/// `PopClip` with nothing open is ignored and clips still open at the end are popped.
pub(crate) fn replay<C: Canvas>(canvas: &mut C, frame: &Frame<'_>) {
    let ppp = frame.ppp;
    let s = |v: f64| (v * ppp).round() / ppp;
    let snap = |r: &Rect| Rect::new(s(r.x0), s(r.y0), s(r.x1), s(r.y1));
    canvas.clear(frame.clear);
    let mut clips = 0usize;
    for shape in frame.shapes {
        match shape {
            Shape::Rect { rect, radius, color, snap: sn } => {
                let r = if *sn { snap(rect) } else { *rect };
                canvas.fill_rect(r, *radius, *color);
            }
            Shape::Gradient { rect, radius, top, bottom } => canvas.fill_rect_gradient(snap(rect), *radius, *top, *bottom),
            Shape::Polygon { points, color } => canvas.fill_polygon(points, *color),
            Shape::Stroke { rect, radius, width, color } => {
                // Whole physical pixels wide, placed so both stroke edges land on pixel edges.
                let w = (width * ppp).round().max(1.0);
                let e = |v: f64| ((v * ppp - w / 2.0).round() + w / 2.0) / ppp;
                let r = Rect::new(e(rect.x0), e(rect.y0), e(rect.x1), e(rect.y1));
                canvas.stroke_rect(r, *radius, w / ppp, *color);
            }
            Shape::Circle { center, radius, color } => canvas.fill_circle(*center, *radius, *color),
            Shape::Line { from, to, width, cap, color } => canvas.line(*from, *to, *width, *cap, *color),
            Shape::Text { layout, pos, color } => canvas.draw_text(layout, Point::new(s(pos.x), s(pos.y)), *color),
            Shape::Image { image, rect } => {
                // Origin snapped, size rounded on its own: the physical size is `round(size * ppp)`,
                // the size the image was rendered at (1 texel : 1 px), wherever the origin lands.
                let (x0, y0) = (s(rect.x0), s(rect.y0));
                let (w, h) = ((rect.width() * ppp).round() / ppp, (rect.height() * ppp).round() / ppp);
                canvas.draw_image(image, Rect::new(x0, y0, x0 + w, y0 + h));
            }
            Shape::PushClip(rect) => {
                canvas.push_clip(snap(rect));
                clips += 1;
            }
            Shape::PopClip => {
                if clips > 0 {
                    canvas.pop_clip();
                    clips -= 1;
                }
            }
        }
    }
    for _ in 0..clips {
        canvas.pop_clip();
    }
}

/// Whether a frame of `size_px` has nothing to draw (minimised, not laid out yet).
pub(crate) fn is_zero_size(size_px: [u32; 2]) -> bool {
    size_px[0] == 0 || size_px[1] == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records the calls replay makes.
    #[derive(Default)]
    struct Rec(Vec<String>);

    impl Canvas for Rec {
        fn clear(&mut self, color: Color) {
            self.0.push(format!("clear {:?}", color.to_array()));
        }

        fn fill_rect(&mut self, r: Rect, radius: f64, _: Color) {
            self.0.push(format!("fill {} {} {} {} r{radius}", r.x0, r.y0, r.x1, r.y1));
        }

        fn stroke_rect(&mut self, r: Rect, _: f64, width: f64, _: Color) {
            self.0.push(format!("stroke {} {} {} {} w{width}", r.x0, r.y0, r.x1, r.y1));
        }

        fn fill_circle(&mut self, c: Point, radius: f64, _: Color) {
            self.0.push(format!("circle {} {} r{radius}", c.x, c.y));
        }

        fn line(&mut self, a: Point, b: Point, width: f64, cap: LineCap, _: Color) {
            self.0.push(format!("line {} {} {} {} w{width} {cap:?}", a.x, a.y, b.x, b.y));
        }

        fn fill_rect_gradient(&mut self, r: Rect, radius: f64, _: Color, _: Color) {
            self.0.push(format!("gradient {} {} {} {} r{radius}", r.x0, r.y0, r.x1, r.y1));
        }

        fn fill_polygon(&mut self, points: &[Point], _: Color) {
            self.0.push(format!("polygon {}", points.len()));
        }

        fn push_clip(&mut self, r: Rect) {
            self.0.push(format!("clip {} {} {} {}", r.x0, r.y0, r.x1, r.y1));
        }

        fn pop_clip(&mut self) {
            self.0.push("pop".into());
        }

        fn draw_text(&mut self, _: &Layout, p: Point, _: Color) {
            self.0.push(format!("text {} {}", p.x, p.y));
        }

        fn draw_image(&mut self, _: &Image, r: Rect) {
            self.0.push(format!("image {} {} {} {}", r.x0, r.y0, r.x1, r.y1));
        }
    }

    fn replay_at(shapes: &[Shape], ppp: f64) -> Vec<String> {
        let mut rec = Rec::default();
        replay(&mut rec, &Frame { shapes, size_px: [100, 100], ppp, clear: Color::WHITE });
        rec.0
    }

    #[test]
    fn rects_snap_unless_asked_not_to() {
        let rect = Rect::new(1.2, 1.4, 10.6, 10.9);
        let calls = replay_at(&[Shape::Rect { rect, radius: 2.0, color: Color::BLACK, snap: true },
                                Shape::Rect { rect, radius: 0.0, color: Color::BLACK, snap: false }],
                              2.0);
        assert_eq!(calls, ["clear [255, 255, 255, 255]", "fill 1 1.5 10.5 11 r2", "fill 1.2 1.4 10.6 10.9 r0"]);
    }

    #[test]
    fn strokes_are_whole_pixels_with_edges_on_pixel_edges() {
        // 1 logical px at 1.5x: 1.5 physical px rounds to 2, centred between pixel edges.
        let calls =
            replay_at(&[Shape::Stroke { rect: Rect::new(10.0, 10.0, 30.0, 30.0), radius: 0.0, width: 1.0, color: Color::BLACK }], 1.5);
        let e = |v: f64| ((v * 1.5 - 1.0f64).round() + 1.0) / 1.5;
        assert_eq!(calls[1], format!("stroke {} {} {} {} w{}", e(10.0), e(10.0), e(30.0), e(30.0), 2.0 / 1.5));
        // Both edges of the stroke land on whole physical pixels.
        for v in [e(10.0), e(30.0)] {
            let (lo, hi) = (v * 1.5 - 1.0, v * 1.5 + 1.0);
            assert!((lo - lo.round()).abs() < 1e-9 && (hi - hi.round()).abs() < 1e-9, "{lo} {hi}");
        }
        // Hairlines never vanish.
        let calls = replay_at(&[Shape::Stroke { rect: Rect::new(0.0, 0.0, 5.0, 5.0), radius: 0.0, width: 0.1, color: Color::BLACK }], 1.0);
        assert!(calls[1].ends_with("w1"), "{calls:?}");
    }

    #[test]
    fn images_and_clips_snap_circles_and_lines_do_not() {
        let img = Image::from_straight_rgba([1, 1], &[0, 0, 0, 255]);
        let calls = replay_at(&[Shape::Image { image: img, rect: Rect::new(0.3, 0.3, 4.7, 4.7) },
                                Shape::PushClip(Rect::new(0.4, 0.6, 9.4, 9.6)),
                                Shape::Circle { center: Point::new(3.3, 3.3), radius: 2.5, color: Color::BLACK },
                                Shape::Line { from: Point::new(0.3, 0.3),
                                              to: Point::new(5.3, 0.3),
                                              width: 1.5,
                                              cap: LineCap::Round,
                                              color: Color::BLACK },
                                Shape::PopClip],
                              1.0);
        assert_eq!(calls[1..], ["image 0 0 4 4", "clip 0 1 9 10", "circle 3.3 3.3 r2.5", "line 0.3 0.3 5.3 0.3 w1.5 Round", "pop"]);
    }

    #[test]
    fn images_keep_their_rounded_physical_size() {
        // 48 px at 1.1x from x 25: snapping both edges would give 80 - 28 = 52 px, not round(52.8).
        let img = Image::from_straight_rgba([1, 1], &[0, 0, 0, 255]);
        let ppp = 1.1;
        let calls = replay_at(&[Shape::Image { image: img, rect: Rect::new(25.0, 25.0, 73.0, 73.0) }], ppp);
        let v: Vec<f64> = calls[1].split(' ').skip(1).map(|v| v.parse().unwrap()).collect();
        assert!(((v[0] * ppp) - (v[0] * ppp).round()).abs() < 1e-9, "{calls:?}");
        assert!(((v[2] - v[0]) * ppp - 53.0).abs() < 1e-9, "{calls:?}");
        assert!(((v[3] - v[1]) * ppp - 53.0).abs() < 1e-9, "{calls:?}");
    }

    #[test]
    fn clips_are_balanced() {
        let r = Rect::new(0.0, 0.0, 5.0, 5.0);
        let calls = replay_at(&[Shape::PopClip, Shape::PushClip(r), Shape::PushClip(r), Shape::PopClip], 1.0);
        assert_eq!(calls[1..], ["clip 0 0 5 5", "clip 0 0 5 5", "pop", "pop"]);
    }

    #[test]
    fn zero_size() {
        assert!(is_zero_size([0, 10]));
        assert!(is_zero_size([10, 0]));
        assert!(!is_zero_size([1, 1]));
    }
}
