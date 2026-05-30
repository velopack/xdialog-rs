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

/// A single memoized shape: the inputs it was shaped from plus the resulting [`TextLayout`].
struct CacheEntry {
    text: String,
    bold: bool,
    size: f32,
    max_width: f32,
    layout: TextLayout,
}

/// Number of shaped layouts kept per component. A label is shaped at up to three distinct keys per
/// frame — the natural-width measure (`max_width == ∞`), the wrapped-width measure, and the
/// physical-size paint — so the cache must hold all three at once for a relayout to be free.
const CACHE_CAP: usize = 4;

/// Memoizes shaped [`TextLayout`]s, re-shaping only when no cached entry matches the requested
/// `(text, weight, size, wrap-width)`.
///
/// Shaping (cosmic-text / rustybuzz) is by far the most expensive part of painting text, and a
/// component's text is constant across the repaints driven by its animations — a button's label
/// doesn't change as its colours fade on hover/focus. The same layout is also requested repeatedly
/// across relayouts: a label is measured (at two widths) *and* painted, and an unchanged title is
/// re-measured on every body-text update. A small LRU keyed on the exact inputs lets all of those
/// reuse the shaped layout instead of re-shaping; entries are rebuilt only on a real change (new
/// text) or a relayout/DPI change (new size or width).
#[derive(Default)]
pub struct CachedLayout {
    /// Least-recently-used at the front, most-recently-used at the back.
    entries: Vec<CacheEntry>,
}

impl CachedLayout {
    /// Return the shaped layout for these inputs, reshaping only on a cache miss. Keys on the exact
    /// `(size, max_width)` values; during an animation, or across relayouts with unchanged text,
    /// the caller passes bit-identical values so this hits.
    pub fn get(&mut self, text: &str, bold: bool, size: f32, max_width: f32) -> &TextLayout {
        if let Some(idx) = self
            .entries
            .iter()
            .position(|e| e.text == text && e.bold == bold && e.size == size && e.max_width == max_width)
        {
            // Promote to most-recently-used so the live working set survives eviction.
            if idx != self.entries.len() - 1 {
                let e = self.entries.remove(idx);
                self.entries.push(e);
            }
            return &self.entries.last().unwrap().layout;
        }

        let layout = layout_text(text, bold, size, max_width);
        if self.entries.len() >= CACHE_CAP {
            self.entries.remove(0); // evict least-recently-used
        }
        self.entries.push(CacheEntry {
            text: text.to_string(),
            bold,
            size,
            max_width,
            layout,
        });
        &self.entries.last().unwrap().layout
    }
}

/// Render a laid-out paragraph onto `pixmap` with its top-left at (`x`, `y`).
///
/// `color` is the text color; color-emoji glyphs ignore it and use their own pixels. The pixmap is
/// assumed opaque (dialogs are), so glyphs are alpha-blended over the existing pixels and the
/// destination alpha is kept at 255.
/// Alpha-blend one straight (non-premultiplied) source sample over an opaque destination pixel at
/// byte offset `idx`, keeping the destination alpha at 255. Fully opaque samples are a plain copy.
#[inline(always)]
fn blend_px(data: &mut [u8], idx: usize, sr: u8, sg: u8, sb: u8, a: u8) {
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
    let pm_w_us = pm_w as usize;
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
            let gox = ox + gx;
            let goy = oy + gy;
            let (gw, gh) = (w as i32, h as i32);

            if gox >= 0 && goy >= 0 && gox + gw <= pm_w && goy + gh <= pm_h {
                // Fast path: the whole glyph cell is inside the pixmap (the common case for dialog
                // text). Skip the per-pixel bounds test and advance the row index by a full stride
                // instead of recomputing `y * width + x` for every texel.
                for dy in 0..gh {
                    let mut idx = ((goy + dy) as usize * pm_w_us + gox as usize) * 4;
                    for _ in 0..gw {
                        blend_px(data, idx, sr, sg, sb, a);
                        idx += 4;
                    }
                }
            } else {
                // Slow path: the glyph straddles a pixmap edge; clip per pixel.
                for dy in 0..gh {
                    let yy = goy + dy;
                    if yy < 0 || yy >= pm_h {
                        continue;
                    }
                    for dx in 0..gw {
                        let xx = gox + dx;
                        if xx < 0 || xx >= pm_w {
                            continue;
                        }
                        blend_px(data, (yy as usize * pm_w_us + xx as usize) * 4, sr, sg, sb, a);
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

    /// The cache must return a layout matching the *current* inputs — never a stale one after the
    /// text, size or wrap width changes — while matching `layout_text` exactly on a hit.
    #[test]
    fn cached_layout_tracks_inputs() {
        let mut cache = CachedLayout::default();

        let a = cache.get("Hello", false, 16.0, f32::INFINITY).total_width;
        // Same inputs → identical result (served from cache).
        let a2 = cache.get("Hello", false, 16.0, f32::INFINITY).total_width;
        assert_eq!(a, a2);
        // …and identical to a fresh, uncached shape of the same inputs.
        assert_eq!(a, layout_text("Hello", false, 16.0, f32::INFINITY).total_width);

        // Longer text must reshape (and be wider), not return the stale "Hello" width.
        let b = cache.get("Hello, world!", false, 16.0, f32::INFINITY).total_width;
        assert_eq!(b, layout_text("Hello, world!", false, 16.0, f32::INFINITY).total_width);
        assert!(b > a, "longer text should be wider: {b} vs {a}");

        // Larger font must reshape (and grow).
        let c = cache.get("Hello, world!", false, 32.0, f32::INFINITY).total_width;
        assert!(c > b, "larger size should be wider: {c} vs {b}");
    }
}
