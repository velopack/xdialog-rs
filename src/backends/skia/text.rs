//! Text layout, measurement and rasterization for the software backend.
//!
//! Built on **cosmic-text**: the bundled Ubuntu font is the primary family, and cosmic-text
//! performs per-glyph fallback to the system fonts loaded by [`FontSystem::new`] for anything
//! Ubuntu lacks (CJK, Arabic, color emoji, …). Complex-script shaping (rustybuzz) and color-emoji
//! rasterization (swash) come for free. Callers select the face with a `bold` flag rather than
//! passing a font handle.

use std::sync::{LazyLock, Mutex};

use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache, Weight};
use tiny_skia::PixmapMut;

use super::font::{FONT_BOLD_DATA, FONT_REGULAR_DATA, UI_FONT_FAMILY};

/// Line height as a multiple of the font size (close to the previous fontdue line spacing).
const LINE_HEIGHT_SCALE: f32 = 1.2;

struct FontContext {
    font_system: FontSystem,
    swash_cache: SwashCache,
}

/// Shared font context: the installed system fonts (for fallback) plus the bundled Ubuntu faces
/// (the primary family).
///
/// Behind a `Mutex` because cosmic-text needs `&mut FontSystem`/`&mut SwashCache` for both layout
/// and rasterization. Dialog painting is infrequent, so lock contention is irrelevant; the lock
/// is never held across a call that could re-enter.
static FONT_CONTEXT: LazyLock<Mutex<FontContext>> = LazyLock::new(|| {
    let mut font_system = FontSystem::new(); // discovers and loads the installed system fonts
    let db = font_system.db_mut();
    db.load_font_data(FONT_REGULAR_DATA.to_vec());
    db.load_font_data(FONT_BOLD_DATA.to_vec());
    Mutex::new(FontContext {
        font_system,
        swash_cache: SwashCache::new(),
    })
});

fn attrs(bold: bool) -> Attrs<'static> {
    Attrs::new()
        .family(Family::Name(UI_FONT_FAMILY))
        .weight(if bold { Weight::BOLD } else { Weight::NORMAL })
}

/// A shaped, wrapped paragraph ready to render. Owns a cosmic-text [`Buffer`].
pub struct TextLayout {
    buffer: Buffer,
    pub total_width: f32,
    pub total_height: f32,
    #[allow(dead_code)]
    pub line_height: f32,
}

/// Lay out `text` at `size`, wrapping at `max_width` (pass `f32::INFINITY` for no wrapping).
pub fn layout_text(text: &str, bold: bool, size: f32, max_width: f32) -> TextLayout {
    let mut ctx = FONT_CONTEXT.lock().unwrap();
    let ctx = &mut *ctx;

    let line_height = size * LINE_HEIGHT_SCALE;
    let metrics = Metrics::new(size, line_height);

    let mut buffer = Buffer::new(&mut ctx.font_system, metrics);
    let width_opt = if max_width.is_finite() { Some(max_width) } else { None };
    buffer.set_size(&mut ctx.font_system, width_opt, None);
    buffer.set_text(&mut ctx.font_system, text, attrs(bold), Shaping::Advanced);
    buffer.shape_until_scroll(&mut ctx.font_system, false);

    let mut total_width: f32 = 0.0;
    let mut line_count: u32 = 0;
    for run in buffer.layout_runs() {
        total_width = total_width.max(run.line_w);
        line_count += 1;
    }
    // An empty string still occupies a single line.
    let total_height = line_count.max(1) as f32 * line_height;

    TextLayout {
        buffer,
        total_width,
        total_height,
        line_height,
    }
}

/// Measure the width of a single (unwrapped) line of text.
pub fn measure_text_width(text: &str, bold: bool, size: f32) -> f32 {
    layout_text(text, bold, size, f32::INFINITY).total_width
}

/// Render a laid-out paragraph onto `pixmap` with its top-left at (`x`, `y`).
///
/// `color` is the text color; color-emoji glyphs ignore it and use their own pixels. The pixmap is
/// assumed opaque (dialogs are), so glyphs are alpha-blended over the existing pixels and the
/// destination alpha is kept at 255.
pub fn render_text(
    pixmap: &mut PixmapMut,
    layout: &TextLayout,
    color: (u8, u8, u8),
    x: f32,
    y: f32,
) {
    let mut ctx = FONT_CONTEXT.lock().unwrap();
    let ctx = &mut *ctx;

    let (cr, cg, cb) = color;
    let text_color = Color::rgb(cr, cg, cb);

    let pm_w = pixmap.width() as i32;
    let pm_h = pixmap.height() as i32;
    let ox = x.round() as i32;
    let oy = y.round() as i32;
    let data = pixmap.data_mut();

    layout
        .buffer
        .draw(&mut ctx.font_system, &mut ctx.swash_cache, text_color, |gx, gy, w, h, px| {
            // cosmic-text emits straight (non-premultiplied) coverage/color: `Mask` glyphs carry
            // the text color with coverage in alpha; `Color` (emoji) glyphs carry their own RGBA.
            let a = px.a();
            if a == 0 {
                return;
            }
            let (sr, sg, sb) = (px.r(), px.g(), px.b());
            for dy in 0..h as i32 {
                for dx in 0..w as i32 {
                    let xx = ox + gx + dx;
                    let yy = oy + gy + dy;
                    if xx < 0 || yy < 0 || xx >= pm_w || yy >= pm_h {
                        continue;
                    }
                    let idx = (yy as usize * pm_w as usize + xx as usize) * 4;
                    if a == 255 {
                        data[idx] = sr;
                        data[idx + 1] = sg;
                        data[idx + 2] = sb;
                        data[idx + 3] = 255;
                    } else {
                        let af = a as f32 / 255.0;
                        let inv = 1.0 - af;
                        data[idx] = (sr as f32 * af + data[idx] as f32 * inv) as u8;
                        data[idx + 1] = (sg as f32 * af + data[idx + 1] as f32 * inv) as u8;
                        data[idx + 2] = (sb as f32 * af + data[idx + 2] as f32 * inv) as u8;
                        data[idx + 3] = 255;
                    }
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::Pixmap;

    /// Renders a multi-script sample and asserts glyphs actually rasterize. Latin + Cyrillic are
    /// covered by the bundled Ubuntu font (so this passes anywhere); CJK/Arabic/emoji exercise the
    /// system-font fallback and are confirmed visually via the dumped PNG.
    #[test]
    fn renders_multiscript_with_fallback() {
        let mut pixmap = Pixmap::new(960, 80).unwrap();
        for px in pixmap.data_mut().chunks_exact_mut(4) {
            px.copy_from_slice(&[255, 255, 255, 255]); // white background
        }

        let sample = "Ubuntu Ąćé | Кириллица | 日本語 中文 한국어 | العربية | 😀🎉🌍";
        let layout = layout_text(sample, false, 32.0, 940.0);
        assert!(layout.total_width > 0.0, "layout produced zero width");

        {
            let mut pm = pixmap.as_mut();
            render_text(&mut pm, &layout, (0, 0, 0), 10.0, 10.0);
        }

        let drawn = pixmap
            .data()
            .chunks_exact(4)
            .filter(|p| p[0] < 250 || p[1] < 250 || p[2] < 250)
            .count();
        eprintln!("rendered non-background pixels: {drawn}");
        let _ = pixmap.save_png("target/skia_unicode_check.png");
        assert!(drawn > 1000, "expected substantial glyph coverage, got {drawn}");
    }
}
