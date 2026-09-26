//! Text on cosmic-text: shaping, bidi, line breaking and font fallback.
//!
//! A [`Layout`] is a shared, immutable, colour-free shaped paragraph ([`Shaped`]: positioned
//! glyph runs). [`Text`] memoizes shaping, so re-laying out the same paragraph returns the same
//! layout (and id): the canvas's text raster cache, keyed by that id, survives across frames and
//! colour fades.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use cosmic_text::{Align, Attrs, Buffer, Metrics, Shaping, Wrap};
use vello_cpu::Glyph;

use super::fonts::{self, Face, BUNDLED_FAMILY};
use crate::backends::draw::{DrawError, Size, TextParams};

/// Glyphs of one face and size; positions are logical px from the layout's top-left, `y` on the
/// baseline.
#[derive(Clone, Debug)]
pub(crate) struct GlyphRun {
    pub face: Face,
    pub font_size: f32,
    pub glyphs: Vec<Glyph>,
}

/// A shaped paragraph.
#[derive(Debug)]
pub(crate) struct Shaped {
    id: u64,
    pub(crate) size: Size,
    lines: usize,
    baseline: f64,
    cap_height: f64,
    pub(crate) runs: Vec<GlyphRun>,
    /// The largest font size (raster padding).
    pub(crate) max_font_size: f32,
}

/// A shaped, wrapped paragraph (cheap to clone).
#[derive(Clone, Debug)]
pub(crate) struct Layout(pub(crate) Rc<Shaped>);

impl crate::backends::draw::TextLayout for Layout {
    fn size(&self) -> Size {
        self.0.size
    }

    fn line_count(&self) -> usize {
        self.0.lines
    }

    fn first_baseline(&self) -> f64 {
        self.0.baseline
    }

    fn cap_height(&self) -> f64 {
        self.0.cap_height
    }

    fn id(&self) -> u64 {
        self.0.id
    }
}

/// A font family name the database has.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Family(Rc<str>);

/// What shaping depends on.
#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    text: Box<str>,
    family: Rc<str>,
    size: u64,
    weight: u16,
    line_height: Option<u64>,
    max_width: Option<u64>,
    rtl: bool,
    generation: u64,
}

/// Memoized layouts before the memo is cleared.
const MEMO_LIMIT: usize = 256;

/// The text service (one per thread; the font database behind it is process-wide).
pub(crate) struct Text {
    memo: RefCell<HashMap<Key, Layout>>,
}

impl crate::backends::draw::TextSystem for Text {
    type Family = Family;
    type Layout = Layout;

    fn shared() -> Result<Rc<Self>, DrawError> {
        thread_local! {
            static SHARED: OnceCell<Rc<Text>> = const { OnceCell::new() };
        }
        Ok(SHARED.with(|s| s.get_or_init(|| Rc::new(Text { memo: RefCell::new(HashMap::new()) })).clone()))
    }

    /// The first candidate the database has. "system-ui", "sans-serif", the Segoe UI families
    /// and nothing known map to the bundled Ubuntu, so every look renders the same on every
    /// machine (the goldens).
    fn resolve_family(&self, candidates: &[&str]) -> Family {
        let fonts = fonts::lock();
        for &name in candidates {
            if matches!(name, "system-ui" | "sans-serif") || name.starts_with("Segoe UI") {
                break;
            }
            if fonts.has_family(name) {
                return Family(name.into());
            }
        }
        Family(BUNDLED_FAMILY.into())
    }

    fn layout(&self, text: &str, p: &TextParams<'_>) -> Layout {
        let key = Key { text: text.into(),
                        family: p.family.0.clone(),
                        size: p.size.to_bits(),
                        weight: p.weight.0,
                        line_height: p.line_height.map(f64::to_bits),
                        max_width: p.max_width.map(f64::to_bits),
                        rtl: p.rtl,
                        generation: fonts::lock().generation };
        if let Some(l) = self.memo.borrow().get(&key) {
            return l.clone();
        }
        let layout = Layout(Rc::new(shape(text, p)));
        let mut memo = self.memo.borrow_mut();
        if memo.len() >= MEMO_LIMIT {
            memo.clear();
        }
        memo.insert(key, layout.clone());
        layout
    }

    fn prepare_fonts(&self, family: &Family, texts: &[&str], wait: Duration) -> bool {
        fonts::prepare_fonts(&family.0, texts, wait)
    }

    fn generation(&self) -> u64 {
        fonts::lock().generation
    }
}

/// Shape and lay out one paragraph. The baseline of every line sits the family's ascent below
/// the line top; with a `line_height`, shifted by half its difference to ascent + descent.
fn shape(text: &str, p: &TextParams<'_>) -> Shaped {
    struct Raw {
        font: cosmic_text::fontdb::ID,
        weight: u16,
        size: f32,
        glyph: Glyph,
    }
    let family: &str = &p.family.0;
    let size = p.size as f32;
    let mut f = fonts::lock();
    let vm = f.vmetrics(family, p.weight.0);
    let line_height = p.line_height.map_or(size * vm.line_height, |lh| lh as f32);
    let ascent = match p.line_height {
        Some(lh) => size * vm.ascent + (lh as f32 - size * (vm.ascent + vm.descent)) / 2.0,
        None => size * vm.ascent,
    };
    let mut raw: Vec<Raw> = Vec::new();
    let (mut width, mut lines) = (0.0f64, 0usize);
    {
        let mut buffer = Buffer::new_empty(Metrics::new(size, line_height));
        let mut b = buffer.borrow_with(&mut f.system);
        b.set_size(p.max_width.map(|w| w.max(0.0) as f32), None);
        b.set_wrap(if p.max_width.is_some() { Wrap::WordOrGlyph } else { Wrap::None });
        let attrs = Attrs::new().family(cosmic_text::Family::Name(family)).weight(cosmic_text::Weight(p.weight.0));
        b.set_text(text, &attrs, Shaping::Advanced, Some(if p.rtl { Align::Right } else { Align::Left }));
        for run in b.layout_runs() {
            lines += 1;
            let baseline = run.line_top + ascent;
            // Extent of all glyphs and of the visible ones (whitespace excluded).
            let (mut all_l, mut all_r, mut ink_l, mut ink_r) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
            for g in run.glyphs {
                all_l = all_l.min(g.x);
                all_r = all_r.max(g.x + g.w);
                if !run.text.get(g.start..g.end).is_some_and(|s| s.chars().all(char::is_whitespace)) {
                    ink_l = ink_l.min(g.x);
                    ink_r = ink_r.max(g.x + g.w);
                }
                raw.push(Raw { font: g.font_id,
                               weight: g.font_weight.0,
                               size: g.font_size,
                               glyph: Glyph { id: g.glyph_id as u32,
                                              x: g.x + g.font_size * g.x_offset,
                                              y: baseline + g.y - g.font_size * g.y_offset } });
            }
            // Leading whitespace counts, trailing whitespace (the far end of the line in the
            // paragraph's direction) does not.
            if ink_r >= ink_l {
                let w = if p.rtl { all_r - ink_l } else { ink_r - all_l };
                width = width.max(w as f64);
            }
        }
    }
    let mut runs: Vec<GlyphRun> = Vec::new();
    let mut last = None;
    for r in raw {
        let key = (r.font, r.weight, r.size.to_bits());
        if last != Some(key) {
            let Some(face) = f.face(r.font, r.weight) else { continue };
            runs.push(GlyphRun { face, font_size: r.size, glyphs: Vec::new() });
            last = Some(key);
        }
        runs.last_mut().expect("pushed above").glyphs.push(r.glyph);
    }
    let lines = lines.max(1);
    let max_font_size = runs.iter().map(|r| r.font_size).fold(size, f32::max);
    Shaped { id: crate::backends::draw::next_id(),
             size: Size::new(width, lines as f64 * line_height as f64),
             lines,
             baseline: ascent as f64,
             cap_height: (size * vm.cap_height) as f64,
             runs,
             max_font_size }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::draw::{TextLayout, TextSystem, Weight};

    fn params(family: &Family, max_width: Option<f64>, line_height: Option<f64>, rtl: bool) -> TextParams<'_> {
        TextParams { family, size: 14.0, weight: Weight::REGULAR, optical_size: None, line_height, max_width, rtl }
    }

    #[test]
    fn families_resolve_to_the_bundled_font() {
        let t = Text::shared().unwrap();
        let ubuntu = Family(BUNDLED_FAMILY.into());
        assert_eq!(t.resolve_family(&["Segoe UI Variable Text", "Segoe UI", "Ubuntu"]), ubuntu);
        assert_eq!(t.resolve_family(&["system-ui"]), ubuntu);
        assert_eq!(t.resolve_family(&["No Such Family"]), ubuntu);
        assert_eq!(t.resolve_family(&["Ubuntu", "Segoe UI"]), ubuntu);
    }

    #[test]
    fn wraps_measures_and_memoizes() {
        let t = Text::shared().unwrap();
        let fam = t.resolve_family(&["Ubuntu"]);
        let text = "The quick brown fox jumps over the lazy dog, again and again and again.";
        let natural = t.layout(text, &params(&fam, None, None, false));
        let wrapped = t.layout(text, &params(&fam, Some(150.0), None, false));
        assert_eq!(natural.line_count(), 1);
        assert!(natural.size().width > 150.0);
        assert!(wrapped.line_count() >= 3 && wrapped.size().width <= 150.0, "{:?}", wrapped.size());
        // Ubuntu's natural line height (ascent + descent + gap) is about 1.149 em.
        assert!((natural.size().height / 14.0 - 1.149).abs() < 0.01, "{:?}", natural.size());
        assert_eq!(t.layout("", &params(&fam, None, None, false)).size().height, natural.size().height);
        // Same parameters: the same layout (id), unless the system font scan (started by other
        // tests) finished in between; other parameters: another.
        let generation = t.generation();
        let (a, b) = (t.layout(text, &params(&fam, None, None, false)), t.layout(text, &params(&fam, None, None, false)));
        if t.generation() == generation {
            assert_eq!(a.id(), b.id());
        }
        assert_ne!(wrapped.id(), natural.id());
    }

    #[test]
    fn trailing_whitespace_is_not_measured() {
        let t = Text::shared().unwrap();
        let fam = t.resolve_family(&["Ubuntu"]);
        let a = t.layout("Hello", &params(&fam, None, None, false)).size().width;
        let b = t.layout("Hello   ", &params(&fam, None, None, false)).size().width;
        assert!(a > 0.0 && (a - b).abs() < 1e-6, "{a} {b}");
    }

    #[test]
    fn line_height_is_the_pitch_and_leading_is_split() {
        let t = Text::shared().unwrap();
        let fam = t.resolve_family(&["Ubuntu"]);
        let natural = t.layout("Hi", &params(&fam, None, None, false));
        let tall = t.layout("Hi", &params(&fam, None, Some(30.0), false));
        let two = t.layout("Hello world", &params(&fam, Some(40.0), Some(30.0), false));
        assert_eq!(tall.size().height, 30.0);
        assert_eq!(two.line_count(), 2);
        assert_eq!(two.size().height, 60.0);
        // Natural: the baseline at the ascent. Explicit: half the extra space above the glyphs.
        let y = |l: &Layout| l.0.runs[0].glyphs[0].y;
        let m = fonts::lock().vmetrics("Ubuntu", 400);
        assert!((y(&natural) - 14.0 * 0.932).abs() < 0.2, "{}", y(&natural));
        let expected = 14.0 * m.ascent + (30.0 - 14.0 * (m.ascent + m.descent)) / 2.0;
        assert!((y(&tall) - expected).abs() < 0.01, "{} {expected}", y(&tall));
    }

    #[test]
    fn rtl_paragraphs_are_right_aligned() {
        let t = Text::shared().unwrap();
        let fam = t.resolve_family(&["Ubuntu"]);
        let l = t.layout("abc", &params(&fam, Some(200.0), None, true));
        let x0 = l.0.runs[0].glyphs.iter().map(|g| g.x).fold(f32::MAX, f32::min);
        assert!(x0 > 150.0, "{x0}");
        let l = t.layout("abc", &params(&fam, Some(200.0), None, false));
        let x0 = l.0.runs[0].glyphs.iter().map(|g| g.x).fold(f32::MAX, f32::min);
        assert!(x0 < 1.0, "{x0}");
    }
}
