//! Renderer tests: exact pixels for trivially predictable shapes, analytic coverage for
//! antialiased ones, cross-checks between the raster's SIMD / optimised paths, presenter
//! contracts (zero-size skip, delta hygiene, alpha masking). The dialogs' pixels are pinned by the
//! offscreen goldens (`tests/egui_offscreen.rs`).

use egui::{
    pos2, vec2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Pos2, Rect, Stroke, StrokeKind, TexturesDelta, Vec2,
};

use super::raster::{color::available_instrs, ColorFieldOrder, EguiSoftwareRender};
use super::software::{mask_alpha, MemoryPresenter};
use super::{Presenter, RenderFrame};
use crate::backends::egui_core::fonts::bundled::UBUNTU_REGULAR;

const CLEAR: Color32 = Color32::from_rgb(0xFA, 0xFA, 0xFA);

fn ctx_with_font() -> egui::Context {
    let ctx = egui::Context::default();
    let mut defs = FontDefinitions::empty();
    defs.font_data.insert("ubuntu".into(), std::sync::Arc::new(FontData::from_static(UBUNTU_REGULAR.bytes)));
    defs.families.insert(FontFamily::Proportional, vec!["ubuntu".into()]);
    defs.families.insert(FontFamily::Monospace, vec!["ubuntu".into()]);
    ctx.set_fonts(defs);
    ctx
}

/// One egui pass painting through `paint`; returns tessellated primitives and the texture delta.
fn frame(ctx: &egui::Context,
         size_pts: Vec2,
         ppp: f32,
         mut paint: impl FnMut(&egui::Painter))
         -> (Vec<egui::ClippedPrimitive>, TexturesDelta) {
    let mut raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size_pts)), time: Some(0.0), ..Default::default() };
    raw.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(ppp);
    let out = ctx.run_ui(raw, |ui| paint(ui.painter()));
    let prims = ctx.tessellate(out.shapes, out.pixels_per_point);
    (prims, out.textures_delta)
}

fn size_px(size_pts: Vec2, ppp: f32) -> [u32; 2] {
    [(size_pts.x * ppp).round() as u32, (size_pts.y * ppp).round() as u32]
}

/// Render one frame with a fresh presenter; returns RGBA8.
fn render_with(raster: EguiSoftwareRender, size_pts: Vec2, ppp: f32, paint: impl FnMut(&egui::Painter)) -> Image {
    let ctx = ctx_with_font();
    let (prims, mut textures) = frame(&ctx, size_pts, ppp, paint);
    let mut p = MemoryPresenter::with_raster(raster);
    let size = size_px(size_pts, ppp);
    p.present(RenderFrame { prims: &prims, textures: &textures, size_px: size, ppp, clear: CLEAR }).unwrap();
    textures.clear();
    let (w, h, px) = p.read_rgba().unwrap();
    assert_eq!([w, h], size);
    Image { w, h, px: px.to_vec() }
}

fn render(size_pts: Vec2, ppp: f32, paint: impl FnMut(&egui::Painter)) -> Image {
    render_with(EguiSoftwareRender::new(ColorFieldOrder::Bgra), size_pts, ppp, paint)
}

#[derive(Clone)]
struct Image {
    w: u32,
    h: u32,
    px: Vec<u8>,
}

impl Image {
    fn at(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.w + x) * 4) as usize;
        self.px[i..i + 4].try_into().unwrap()
    }

    /// (max per-channel difference, number of pixels differing by more than `tol`)
    fn diff(&self, other: &Image, tol: u8) -> (u8, usize) {
        assert_eq!((self.w, self.h), (other.w, other.h), "image size");
        let mut max = 0;
        let mut over = 0;
        for (a, b) in self.px.as_chunks::<4>().0.iter().zip(other.px.as_chunks::<4>().0) {
            let d = a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap();
            max = max.max(d);
            if d > tol {
                over += 1;
            }
        }
        (max, over)
    }
}

// ---------------------------------------------------------------------------------------------

#[test]
fn opaque_rects_are_exact() {
    for ppp in [1.0f32, 2.0] {
        let red = Color32::from_rgb(200, 30, 40);
        let blue = Color32::from_rgb(10, 90, 220);
        let img = render(vec2(64.0, 48.0), ppp, |p| {
            p.rect_filled(Rect::from_min_max(pos2(4.0, 4.0), pos2(30.0, 20.0)), 0.0, red);
            // Overlapping, later shape wins (painter order).
            p.rect_filled(Rect::from_min_max(pos2(20.0, 10.0), pos2(50.0, 40.0)), 0.0, blue);
        });
        let s = ppp as u32;
        // Interior pixels (1 physical px away from any edge, which egui feathers).
        for y in (4 * s + 1)..(10 * s - 1) {
            for x in (4 * s + 1)..(30 * s - 1) {
                assert_eq!(img.at(x, y), [200, 30, 40, 255], "red @ {x},{y} ppp {ppp}");
            }
        }
        for y in (10 * s + 1)..(40 * s - 1) {
            for x in (20 * s + 1)..(50 * s - 1) {
                assert_eq!(img.at(x, y), [10, 90, 220, 255], "blue @ {x},{y} ppp {ppp}");
            }
        }
        // Far from all shapes: the clear colour, untouched.
        for (x, y) in [(0, 0), (60 * s, 2 * s), (2 * s, 46 * s), (60 * s, 46 * s), (10 * s, 30 * s)] {
            assert_eq!(img.at(x, y), [0xFA, 0xFA, 0xFA, 255], "clear @ {x},{y} ppp {ppp}");
        }
    }
}

#[test]
fn clear_colour_fills_empty_frame() {
    let img = render(vec2(7.0, 5.0), 1.0, |_| {});
    assert!(img.px.as_chunks::<4>().0.iter().all(|p| *p == [0xFA, 0xFA, 0xFA, 255]));
}

#[test]
fn antialiased_circle_coverage_matches_area() {
    // Black disc on white: total darkness == covered area. Checks the triangle rasterizer,
    // feathering and blending together against the analytic answer.
    for (ppp, r) in [(1.0f32, 10.0f32), (1.5, 13.3), (2.0, 7.25)] {
        let size = vec2(40.0, 40.0);
        let ctx = ctx_with_font();
        let (prims, mut textures) = frame(&ctx, size, ppp, |p| {
            p.circle_filled(pos2(20.3, 19.6), r, Color32::BLACK);
        });
        let mut p = MemoryPresenter::new();
        let px = size_px(size, ppp);
        p.present(RenderFrame { prims: &prims, textures: &textures, size_px: px, ppp, clear: Color32::WHITE }).unwrap();
        textures.clear();
        let (_, _, rgba) = p.read_rgba().unwrap();
        let covered: f64 = rgba.as_chunks::<4>().0.iter().map(|p| (255 - p[1]) as f64 / 255.0).sum();
        let expected = std::f64::consts::PI * (r * ppp) as f64 * (r * ppp) as f64;
        let rel = (covered - expected).abs() / expected;
        assert!(rel < 0.02, "ppp {ppp} r {r}: covered {covered:.1} px² vs {expected:.1} px² ({:.2}%)", rel * 100.0);
    }
}

#[test]
fn memory_presenter_is_rgba_and_opaque() {
    let img = render(vec2(8.0, 8.0), 1.0, |p| {
        p.rect_filled(Rect::from_min_max(pos2(0.0, 0.0), pos2(8.0, 8.0)), 0.0, Color32::from_rgb(255, 0, 0));
        // Translucent overlay: output alpha must still be 255.
        p.rect_filled(Rect::from_min_max(pos2(4.0, 0.0), pos2(8.0, 8.0)), 0.0, Color32::from_rgba_unmultiplied(0, 0, 255, 128));
    });
    assert_eq!(img.at(1, 4), [255, 0, 0, 255]);
    let [r, g, b, a] = img.at(6, 4);
    assert!((125..=129).contains(&r) && g == 0 && (126..=130).contains(&b) && a == 255, "{:?}", [r, g, b, a]);
}

#[test]
fn softbuffer_words_have_zero_top_byte() {
    // [B, G, R, A] bytes as the raster writes them into the softbuffer buffer.
    let bytes: [[u8; 4]; 2] = [[0x33, 0x22, 0x11, 0xFF], [0x03, 0x02, 0x01, 0x80]];
    let mut words: Vec<u32> = bytes.iter().map(|b| u32::from_ne_bytes(*b)).collect();
    mask_alpha(&mut words);
    assert_eq!(words, [0x0011_2233, 0x0001_0203]);
}

#[test]
fn zero_size_frame_applies_textures() {
    let ctx = ctx_with_font();
    let size = vec2(120.0, 30.0);
    let text = |p: &egui::Painter| {
        p.text(pos2(4.0, 4.0), egui::Align2::LEFT_TOP, "Hello", FontId::proportional(16.0), Color32::BLACK);
    };
    let mut p = MemoryPresenter::new();

    // First pass: the full font atlas arrives while the window is still zero-sized.
    let (prims, mut textures) = frame(&ctx, size, 1.0, text);
    assert!(!textures.set.is_empty(), "first frame uploads the font atlas");
    for zero in [[0, 30], [120, 0], [0, 0]] {
        p.present(RenderFrame { prims: &prims, textures: &textures, size_px: zero, ppp: 1.0, clear: CLEAR }).unwrap();
    }
    textures.clear();
    assert!(p.read_rgba().is_none(), "nothing presented yet");

    // Second pass carries no (full) atlas; the text must still render from the atlas applied above.
    let (prims, mut textures) = frame(&ctx, size, 1.0, text);
    assert!(textures.set.iter().all(|(_, d)| d.iter().all(|d| d.pos.is_some())), "no full re-upload");
    p.present(RenderFrame { prims: &prims, textures: &textures, size_px: [120, 30], ppp: 1.0, clear: CLEAR }).unwrap();
    textures.clear();
    let (_, _, rgba) = p.read_rgba().unwrap();
    let dark = rgba.as_chunks::<4>().0.iter().filter(|p| p[0] < 100).count();
    assert!(dark > 30, "text drawn from the previously applied atlas ({dark} dark px)");
}

fn scene(p: &egui::Painter) {
    let bg = Rect::from_min_max(pos2(6.0, 6.0), pos2(234.0, 154.0));
    p.rect(bg, CornerRadius::same(8), Color32::WHITE, Stroke::new(1.0, Color32::from_gray(200)), StrokeKind::Inside);
    // Button-like outlined rounded rect + default ring.
    let b = Rect::from_min_size(pos2(140.0, 116.0), vec2(84.0, 28.0));
    p.rect(b,
           CornerRadius::same(6),
           Color32::from_rgb(0xF4, 0xF6, 0xFA),
           Stroke::new(1.0, Color32::from_rgb(0xB0, 0xB6, 0xC0)),
           StrokeKind::Inside);
    p.rect_stroke(b.expand(2.0), CornerRadius::same(8), Stroke::new(2.0, Color32::from_rgb(0x1A, 0x73, 0xE8)), StrokeKind::Outside);
    // Icon-like disc with a glyph built from primitives.
    p.circle_filled(pos2(30.0, 34.0), 16.0, Color32::from_rgb(0x1A, 0x73, 0xE8));
    p.circle_filled(pos2(30.0, 26.0), 2.2, Color32::WHITE);
    p.line_segment([pos2(30.0, 31.0), pos2(30.0, 43.0)], Stroke::new(3.0, Color32::WHITE));
    // Translucent overlap and a diagonal hairline.
    p.circle_filled(pos2(200.0, 40.0), 18.0, Color32::from_rgba_unmultiplied(220, 40, 40, 140));
    p.circle_stroke(pos2(186.0, 52.0), 14.0, Stroke::new(1.5, Color32::from_rgb(20, 140, 60)));
    p.line_segment([pos2(56.0, 140.0), pos2(120.0, 118.0)], Stroke::new(1.0, Color32::from_gray(40)));
    // Vertex-coloured mesh (gradient) = the varying-colour triangle path.
    let mut mesh = egui::Mesh::default();
    let g = Rect::from_min_size(pos2(16.0, 116.0), vec2(36.0, 28.0));
    mesh.colored_vertex(g.left_top(), Color32::from_rgb(255, 0, 0));
    mesh.colored_vertex(g.right_top(), Color32::from_rgb(0, 255, 0));
    mesh.colored_vertex(g.right_bottom(), Color32::from_rgb(0, 0, 255));
    mesh.colored_vertex(g.left_bottom(), Color32::from_rgba_premultiplied(0, 0, 0, 0));
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(mesh);
    // Text (font atlas sampling) and a clipped shape.
    p.text(pos2(56.0, 18.0), egui::Align2::LEFT_TOP, "Heading", FontId::proportional(18.0), Color32::from_gray(20));
    p.text(pos2(56.0, 44.0), egui::Align2::LEFT_TOP, "Body text, fractional ppp. gjqy", FontId::proportional(13.0), Color32::from_gray(60));
    p.text(pos2(152.0, 122.0), egui::Align2::LEFT_TOP, "OK", FontId::proportional(14.0), Color32::from_gray(20));
    let clipped = p.with_clip_rect(Rect::from_min_max(pos2(56.0, 70.0), pos2(140.0, 100.0)));
    clipped.circle_filled(pos2(98.0, 100.0), 26.0, Color32::from_rgb(0xE8, 0xA0, 0x1A));
}

const SCENE: Vec2 = vec2(240.0, 160.0);

#[test]
fn simd_paths_match_generic() {
    let impls = available_instrs();
    let generic = *impls.last().unwrap();
    for ppp in [1.0, 1.5] {
        let reference = render_with(EguiSoftwareRender::new(ColorFieldOrder::Bgra).with_simd_impl(generic), SCENE, ppp, scene);
        for &imp in impls {
            let img = render_with(EguiSoftwareRender::new(ColorFieldOrder::Bgra).with_simd_impl(imp), SCENE, ppp, scene);
            let (max, over) = img.diff(&reference, 1);
            assert!(over == 0, "{imp} vs {generic} @ppp {ppp}: max diff {max}, {over} px > 1");
        }
    }
}

#[test]
fn optimised_paths_match_plain_triangles() {
    for ppp in [1.0, 1.5] {
        let plain = render_with(EguiSoftwareRender::new(ColorFieldOrder::Bgra).with_allow_raster_opt(false)
                                                                              .with_convert_tris_to_rects(false),
                                SCENE,
                                ppp,
                                scene);
        let fast = render(SCENE, ppp, scene);
        let (max, over) = fast.diff(&plain, 2);
        let total = (fast.w * fast.h) as usize;
        assert!(over * 1000 <= total, "ppp {ppp}: max diff {max}, {over}/{total} px > 2");
    }
}
