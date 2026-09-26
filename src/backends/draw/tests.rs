//! Renderer conformance tests: every backend draws the same display lists the same way (within
//! anti-aliasing), checked through `MemorySurface`.

use std::rc::Rc;

use super::*;

#[test]
fn renderer_matches_target() {
    let expected = if cfg!(windows) {
        "d2d"
    } else if cfg!(target_os = "macos") {
        "cg"
    } else {
        "soft"
    };
    assert_eq!(RENDERER, expected);
}

const BG: Color = Color::from_rgb(0xFA, 0xFA, 0xFA);

fn text() -> Rc<Text> {
    Text::shared().expect("text system")
}

/// Render `shapes` over `BG` at `ppp` into a `size` (logical px) frame; RGBA and width.
fn render(shapes: &[Shape], size: [f64; 2], ppp: f64) -> (Vec<u8>, u32) {
    let mut s = <MemorySurface as MemoryTarget>::new(&text()).expect("memory surface");
    let size_px = [(size[0] * ppp).round() as u32, (size[1] * ppp).round() as u32];
    s.present(&Frame { shapes, size_px, ppp, clear: BG }).expect("present");
    let (w, h, px) = s.read_rgba().expect("a frame");
    assert_eq!([w, h], size_px);
    assert!(px.chunks(4).all(|p| p[3] == 255), "opaque");
    (px.to_vec(), w)
}

fn at(px: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    px[i..i + 4].try_into().unwrap()
}

const BG_PX: [u8; 4] = [0xFA, 0xFA, 0xFA, 255];

#[test]
fn opaque_rects_are_exact_and_snapped() {
    for ppp in [1.0, 1.5, 2.0] {
        let shapes = [Shape::Rect { rect: Rect::new(4.2, 4.0, 30.4, 20.0), radius: 0.0, color: Color::GRAY, snap: true }];
        let (px, w) = render(&shapes, [64.0, 48.0], ppp);
        let (x0, x1) = ((4.2 * ppp).round() as u32, (30.4 * ppp).round() as u32);
        let y = (10.0 * ppp) as u32;
        // Crisp edges: the first and last covered columns are fully grey, their neighbours clear.
        assert_eq!(at(&px, w, x0, y), [160, 160, 160, 255], "ppp {ppp}");
        assert_eq!(at(&px, w, x1 - 1, y), [160, 160, 160, 255], "ppp {ppp}");
        assert_eq!(at(&px, w, x0 - 1, y), BG_PX, "ppp {ppp}");
        assert_eq!(at(&px, w, x1, y), BG_PX, "ppp {ppp}");
    }
}

#[test]
fn circle_coverage_matches_area() {
    let r = 10.0;
    let shapes = [Shape::Circle { center: Point::new(20.3, 19.6), radius: r, color: Color::BLACK }];
    let (px, _) = render(&shapes, [40.0, 40.0], 1.0);
    let covered: f64 = px.chunks(4).map(|p| (0xFA - p[1]) as f64 / 0xFA as f64).sum();
    let expected = std::f64::consts::PI * r * r;
    assert!((covered - expected).abs() / expected < 0.04, "{covered} vs {expected}");
}

#[test]
fn one_px_stroke_is_crisp_at_fractional_scale() {
    let shapes = [Shape::Stroke { rect: Rect::new(10.0, 10.0, 30.0, 30.0), radius: 0.0, width: 1.0, color: Color::BLACK }];
    let (px, w) = render(&shapes, [40.0, 40.0], 1.5);
    let row: Vec<u8> = (0..w).map(|x| at(&px, w, x, 30)[0]).collect();
    assert!(row.iter().all(|&v| v == 0 || v == 0xFA), "{row:?}");
    assert!(row.contains(&0));
}

#[test]
fn clips_contain_and_balance() {
    let full = Rect::new(0.0, 0.0, 40.0, 40.0);
    let shapes = [Shape::PushClip(Rect::new(10.0, 10.0, 20.0, 20.0)),
                  Shape::Rect { rect: full, radius: 0.0, color: Color::BLACK, snap: true },
                  Shape::PushClip(Rect::new(30.0, 30.0, 40.0, 40.0)),
                  // Intersected with the outer clip: draws nothing.
                  Shape::Rect { rect: full, radius: 0.0, color: Color::RED, snap: true },
                  Shape::PopClip,
                  Shape::PopClip,
                  Shape::PopClip, // unbalanced: ignored
                  Shape::Rect { rect: Rect::new(30.0, 0.0, 40.0, 10.0), radius: 0.0, color: Color::BLACK, snap: true },
                  // Left open: popped at the end.
                  Shape::PushClip(Rect::new(0.0, 30.0, 5.0, 40.0))];
    let (px, w) = render(&shapes, [40.0, 40.0], 1.0);
    assert_eq!(at(&px, w, 15, 15), [0, 0, 0, 255]);
    assert_eq!(at(&px, w, 5, 5), BG_PX);
    assert_eq!(at(&px, w, 35, 35), BG_PX);
    assert_eq!(at(&px, w, 35, 5), [0, 0, 0, 255]);
    assert!(px.chunks(4).all(|p| p[0] == p[1]), "no red");
}

#[test]
fn round_caps_extend_past_the_end_butt_caps_do_not() {
    let line = |cap| [Shape::Line { from: Point::new(10.0, 20.0), to: Point::new(30.0, 20.0), width: 6.0, cap, color: Color::BLACK }];
    let (butt, w) = render(&line(LineCap::Butt), [40.0, 40.0], 1.0);
    let (round, _) = render(&line(LineCap::Round), [40.0, 40.0], 1.0);
    assert_eq!(at(&butt, w, 20, 20), [0, 0, 0, 255]);
    assert_eq!(at(&butt, w, 31, 20), BG_PX);
    assert_eq!(at(&butt, w, 8, 20), BG_PX);
    assert_eq!(at(&round, w, 31, 20), [0, 0, 0, 255]);
    assert_eq!(at(&round, w, 8, 20), [0, 0, 0, 255]);
}

#[test]
fn images_blit_one_to_one() {
    // 4x4 physical px of white with a red pixel at (1, 2), drawn at 2x into a 2x2 logical rect.
    let mut rgba = [255u8; 64];
    rgba[(2 * 4 + 1) * 4..(2 * 4 + 1) * 4 + 4].copy_from_slice(&[255, 0, 0, 255]);
    let image = Image::from_straight_rgba([4, 4], &rgba);
    let shapes = [Shape::Image { image, rect: Rect::new(5.0, 5.0, 7.0, 7.0) }];
    let (px, w) = render(&shapes, [20.0, 20.0], 2.0);
    assert_eq!(at(&px, w, 11, 12), [255, 0, 0, 255]);
    assert_eq!(at(&px, w, 10, 10), [255, 255, 255, 255]);
    assert_eq!(at(&px, w, 13, 13), [255, 255, 255, 255]);
    assert_eq!(at(&px, w, 9, 10), BG_PX);
    assert_eq!(at(&px, w, 14, 10), BG_PX);
}

fn params<'a>(family: &'a Family, max_width: Option<f64>, line_height: Option<f64>) -> TextParams<'a> {
    TextParams { family, size: 14.0, weight: Weight::REGULAR, optical_size: None, line_height, max_width, rtl: false }
}

#[test]
fn text_measures_wraps_and_honours_line_height() {
    let t = text();
    let family = t.resolve_family(&["Ubuntu", "Segoe UI"]);
    let one = t.layout("Hello world", &params(&family, None, None));
    assert!(one.size().width > 0.0 && one.line_count() == 1, "{:?}", one.size());
    let two = t.layout("Hello world", &params(&family, Some(one.size().width * 0.75), Some(20.0)));
    assert_eq!(two.line_count(), 2);
    assert!((two.size().height - 40.0).abs() < 0.5, "{:?}", two.size());
}

#[test]
fn text_colour_is_applied_at_draw_time_and_renders_are_deterministic() {
    let t = text();
    let family = t.resolve_family(&["Ubuntu", "Segoe UI"]);
    let layout = t.layout("Hello", &params(&family, None, None));
    let draw = |color| [Shape::Text { layout: layout.clone(), pos: Point::new(4.0, 4.0), color }];
    let (red, _) = render(&draw(Color::RED), [60.0, 30.0], 1.0);
    let (blue, _) = render(&draw(Color::from_rgb(0, 0, 255)), [60.0, 30.0], 1.0);
    // Red text has red-dominant ink and no blue-dominant ink, and the other way round.
    let ink = |px: &[u8], c: usize| px.chunks(4).filter(|p| p[c] > 200 && p[(c + 1) % 3] < 100).count();
    assert!(ink(&red, 0) > 10 && ink(&red, 2) == 0);
    assert!(ink(&blue, 2) > 10 && ink(&blue, 0) == 0);
    let (again, _) = render(&draw(Color::RED), [60.0, 30.0], 1.0);
    assert!(red == again, "byte-identical re-render");
}

#[test]
fn rtl_paragraphs_are_right_aligned() {
    let t = text();
    let family = t.resolve_family(&["Ubuntu", "Segoe UI"]);
    // Where the ink of a paragraph laid out in a 200 px box is: (leftmost, rightmost) column.
    let ink = |rtl: bool| {
        let layout = t.layout("Hello", &TextParams { rtl, ..params(&family, Some(200.0), None) });
        let (px, w) = render(&[Shape::Text { layout, pos: Point::new(0.0, 4.0), color: Color::BLACK }], [200.0, 30.0], 1.0);
        let cols: Vec<u32> = (0..w).filter(|&x| (0..30).any(|y| at(&px, w, x, y)[0] < 0x80)).collect();
        assert!(!cols.is_empty(), "rtl {rtl}: no ink");
        (cols[0], *cols.last().unwrap())
    };
    let (ltr, rtl) = (ink(false), ink(true));
    assert!(ltr.1 < 100, "LTR starts at the left: {ltr:?}");
    assert!(rtl.0 > 100 && rtl.1 >= 190, "RTL ends at the right edge: {rtl:?}");
}

/// Both themes' font lists resolve to a family that lays out text (the platform UI font when none
/// of the named ones exists).
#[test]
fn ui_family_resolves() {
    use crate::backends::gui::theme;
    use crate::XDialogBackend;
    let t = text();
    for backend in [XDialogBackend::Fluent, XDialogBackend::Ubuntu] {
        let family = t.resolve_family(theme::new(backend).fonts().families);
        let layout = t.layout("Hello", &params(&family, None, None));
        assert!(layout.size().width > 10.0 && layout.size().height > 10.0, "{backend:?}: {:?}", layout.size());
        assert_eq!(layout.line_count(), 1, "{backend:?}");
    }
}
