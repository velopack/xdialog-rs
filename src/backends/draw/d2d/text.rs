//! Text on DirectWrite: system fonts, shaping, font fallback, bidi, line breaking and colour
//! fonts are all DirectWrite's.
//!
//! [`Text`] holds the factories (shared by every surface of the thread: Direct2D caches glyphs
//! per factory, and a warm factory draws a colour-emoji dialog's first frame in ~10 ms instead of
//! ~150 ms) and a cache of text formats. A [`Layout`] is an immutable, colour-free
//! `IDWriteTextLayout` plus its measurements.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use windows::core::{w, Interface, BOOL, PCWSTR};
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;

use super::backend;
use crate::backends::draw::{next_id, DrawError, Size, TextParams};

/// A resolved font family (cheap to clone).
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Family(Rc<FamilyInner>);

#[derive(PartialEq, Debug)]
struct FamilyInner {
    /// NUL-terminated family name in the system (weight-stretch-style) collection.
    name: Vec<u16>,
    /// Its index there (`None`: not installed; DirectWrite falls back per character).
    index: Option<u32>,
    /// NUL-terminated name in the typographic collection (the variable-font path: optical size).
    typo_name: Option<Vec<u16>>,
    /// Distinct per (name, typographic name) in its `Text`: the format cache key.
    key: u32,
}

/// A laid-out paragraph.
pub(crate) struct DwLayout {
    id: u64,
    /// `None`: DirectWrite failed (logged); nothing is drawn.
    pub(crate) layout: Option<IDWriteTextLayout>,
    size: Size,
    lines: usize,
    baseline: f64,
    cap_height: f64,
}

/// A shaped, wrapped paragraph (cheap to clone).
#[derive(Clone)]
pub(crate) struct Layout(pub(crate) Rc<DwLayout>);

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

/// What a text format depends on (no `String`: the family is its collection index).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct FormatKey {
    family: u32,
    weight: u16,
    size: u64,
    opsz: Option<u64>,
    line_height: Option<u64>,
    rtl: bool,
    wrap: bool,
}

/// The per-thread Direct2D / DirectWrite service.
pub(crate) struct Text {
    pub(crate) d2d: ID2D1Factory1,
    dwrite: IDWriteFactory,
    dwrite6: Option<IDWriteFactory6>,
    fonts: IDWriteFontCollection,
    typo: Option<IDWriteFontCollection2>,
    /// Fixed rendering parameters: windows and WIC renders look the same whatever the user's
    /// ClearType tuning.
    pub(crate) params: IDWriteRenderingParams,
    formats: RefCell<HashMap<FormatKey, IDWriteTextFormat>>,
    /// The families resolved so far (their `key` is the position).
    families: RefCell<Vec<Family>>,
}

/// `s` as a NUL-terminated UTF-16 string.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

impl Text {
    fn new() -> Result<Self, DrawError> {
        // SAFETY: factory creation and queries with out-pointers to locals.
        unsafe {
            let d2d: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).map_err(backend("D2D1CreateFactory"))?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).map_err(backend("DWriteCreateFactory"))?;
            let dwrite6 = dwrite.cast::<IDWriteFactory6>().ok();
            let mut fonts = None;
            dwrite.GetSystemFontCollection(&mut fonts, false).map_err(backend("GetSystemFontCollection"))?;
            let fonts = fonts.ok_or_else(|| DrawError::Backend("d2d: no system font collection".into()))?;
            let typo = dwrite6.as_ref().and_then(|f| f.GetSystemFontCollection(false, DWRITE_FONT_FAMILY_MODEL_TYPOGRAPHIC).ok());
            let params =
                dwrite.CreateCustomRenderingParams(1.8, 0.5, 0.0, DWRITE_PIXEL_GEOMETRY_FLAT, DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC)
                      .map_err(backend("CreateCustomRenderingParams"))?;
            Ok(Text { d2d,
                      dwrite,
                      dwrite6,
                      fonts,
                      typo,
                      params,
                      formats: RefCell::new(HashMap::new()),
                      families: RefCell::new(Vec::new()) })
        }
    }

    /// The index of family `name` (NUL-terminated) in `collection`.
    fn find(collection: &IDWriteFontCollection, name: &[u16]) -> Option<u32> {
        let (mut index, mut exists) = (0u32, BOOL(0));
        // SAFETY: `name` is NUL-terminated; out-pointers to locals.
        unsafe { collection.FindFamilyName(PCWSTR(name.as_ptr()), &mut index, &mut exists).ok()? };
        exists.as_bool().then_some(index)
    }

    /// The family's (ascent, descent, cap height) in em, from its first font matching `weight`.
    /// Segoe UI's when unknown.
    fn vmetrics_em(&self, family: &Family, weight: u16) -> (f32, f32, f32) {
        let metrics = || -> windows::core::Result<DWRITE_FONT_METRICS> {
            let index = family.0.index.ok_or_else(windows::core::Error::empty)?;
            let mut m = DWRITE_FONT_METRICS::default();
            // SAFETY: plain DirectWrite queries; `m` is a local out-parameter.
            unsafe {
                let font =
                    self.fonts
                        .GetFontFamily(index)?
                        .GetFirstMatchingFont(DWRITE_FONT_WEIGHT(weight as i32), DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL)?;
                font.GetMetrics(&mut m);
            }
            Ok(m)
        };
        match metrics() {
            Ok(m) if m.designUnitsPerEm > 0 => {
                let upem = m.designUnitsPerEm as f32;
                (m.ascent as f32 / upem, m.descent as f32 / upem, m.capHeight as f32 / upem)
            }
            _ => (2210.0 / 2048.0, 514.0 / 2048.0, 1434.0 / 2048.0),
        }
    }

    /// The (cached) text format for `p`.
    fn format(&self, p: &TextParams<'_>) -> windows::core::Result<IDWriteTextFormat> {
        let family = &p.family.0;
        let opsz = p.optical_size.filter(|_| self.dwrite6.is_some() && self.typo.is_some() && family.typo_name.is_some());
        let key = FormatKey { family: family.key,
                              weight: p.weight.0,
                              size: p.size.to_bits(),
                              opsz: opsz.map(f64::to_bits),
                              line_height: p.line_height.map(f64::to_bits),
                              rtl: p.rtl,
                              wrap: p.max_width.is_some() };
        if let Some(f) = self.formats.borrow().get(&key) {
            return Ok(f.clone());
        }
        let size = p.size as f32;
        // SAFETY: NUL-terminated names that outlive the calls; DirectWrite copies what it keeps.
        let format: IDWriteTextFormat = unsafe {
            match (opsz, &self.dwrite6, &self.typo, &family.typo_name) {
                (Some(opsz), Some(f6), Some(typo), Some(name)) => {
                    let axes = [DWRITE_FONT_AXIS_VALUE { axisTag: DWRITE_FONT_AXIS_TAG_WEIGHT, value: p.weight.0 as f32 },
                                DWRITE_FONT_AXIS_VALUE { axisTag: DWRITE_FONT_AXIS_TAG_OPTICAL_SIZE, value: opsz as f32 }];
                    f6.CreateTextFormat(PCWSTR(name.as_ptr()), typo, &axes, size, w!(""))?.cast()?
                }
                _ => self.dwrite.CreateTextFormat(PCWSTR(family.name.as_ptr()),
                                                   &self.fonts,
                                                   DWRITE_FONT_WEIGHT(p.weight.0 as i32),
                                                   DWRITE_FONT_STYLE_NORMAL,
                                                   DWRITE_FONT_STRETCH_NORMAL,
                                                   size,
                                                   w!(""))?,
            }
        };
        // SAFETY: plain setters on the new format.
        unsafe {
            if p.rtl {
                format.SetReadingDirection(DWRITE_READING_DIRECTION_RIGHT_TO_LEFT)?;
            }
            format.SetWordWrapping(if p.max_width.is_some() {
                                       DWRITE_WORD_WRAPPING_EMERGENCY_BREAK
                                   } else {
                                       DWRITE_WORD_WRAPPING_NO_WRAP
                                   })?;
            if let Some(lh) = p.line_height {
                // Uniform pitch; the difference to ascent + descent split above and below.
                let (ascent, descent, _) = self.vmetrics_em(p.family, p.weight.0);
                let baseline = ascent * size + (lh as f32 - (ascent + descent) * size) / 2.0;
                format.SetLineSpacing(DWRITE_LINE_SPACING_METHOD_UNIFORM, lh as f32, baseline)?;
            }
        }
        self.formats.borrow_mut().insert(key, format.clone());
        Ok(format)
    }

    fn try_layout(&self, text: &str, p: &TextParams<'_>) -> windows::core::Result<(IDWriteTextLayout, DWRITE_TEXT_METRICS, f64)> {
        let format = self.format(p)?;
        let utf16: Vec<u16> = text.encode_utf16().collect();
        let width = p.max_width.map_or(0.0, |w| w.max(0.0) as f32);
        // SAFETY: DirectWrite copies the string; out-pointers to locals.
        unsafe {
            let layout = self.dwrite.CreateTextLayout(&utf16, &format, width, f32::MAX)?;
            let mut m = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut m)?;
            if p.rtl && p.max_width.is_none() {
                // Unwrapped right-to-left lines align to the box's right edge: make the box the
                // widest line (trailing whitespace, on the left, hangs outside).
                layout.SetMaxWidth(m.width)?;
                layout.GetMetrics(&mut m)?;
            }
            let mut lines = vec![DWRITE_LINE_METRICS::default(); m.lineCount.max(1) as usize];
            let mut count = 0u32;
            layout.GetLineMetrics(Some(&mut lines), &mut count)?;
            Ok((layout, m, lines[0].baseline as f64))
        }
    }
}

impl crate::backends::draw::TextSystem for Text {
    type Family = Family;
    type Layout = Layout;

    fn shared() -> Result<Rc<Self>, DrawError> {
        thread_local! {
            static SHARED: OnceCell<Rc<Text>> = const { OnceCell::new() };
        }
        SHARED.with(|s| {
                  if let Some(t) = s.get() {
                      return Ok(t.clone());
                  }
                  let t = Rc::new(Text::new()?);
                  // Never released: a thread-local destructor runs under the loader lock, and
                  // releasing a factory that drove a hardware HWND target (its Direct3D device)
                  // there deadlocks the exiting thread. One set of factories per thread.
                  std::mem::forget(t.clone());
                  Ok(s.get_or_init(|| t).clone())
              })
    }

    /// The first candidate installed; "system-ui" / "sans-serif" and no match are Segoe UI.
    fn resolve_family(&self, candidates: &[&str]) -> Family {
        let names = || {
            candidates.iter()
                      .map(|&c| if matches!(c, "system-ui" | "sans-serif") { "Segoe UI" } else { c })
                      .chain(Some("Segoe UI"))
                      .map(wide)
        };
        let (name, index) =
            names().find_map(|n| Self::find(&self.fonts, &n).map(|i| (n, Some(i)))).unwrap_or_else(|| (wide("Segoe UI"), None));
        let typo_name = self.typo.as_ref().and_then(|typo| names().find(|n| Self::find(typo, n).is_some()));
        let mut families = self.families.borrow_mut();
        if let Some(f) = families.iter().find(|f| f.0.name == name && f.0.typo_name == typo_name) {
            return f.clone();
        }
        let family = Family(Rc::new(FamilyInner { name, index, typo_name, key: families.len() as u32 }));
        families.push(family.clone());
        family
    }

    fn layout(&self, text: &str, p: &TextParams<'_>) -> Layout {
        let (layout, size, lines, baseline) = match self.try_layout(text, p) {
            Ok((l, m, b)) => (Some(l), Size::new(m.width as f64, m.height as f64), (m.lineCount as usize).max(1), b),
            Err(e) => {
                warn!("xdialog: DirectWrite could not lay out text: {e}");
                (None, Size::ZERO, 1, 0.0)
            }
        };
        let cap_height = self.vmetrics_em(p.family, p.weight.0).2 as f64 * p.size;
        Layout(Rc::new(DwLayout { id: next_id(), layout, size, lines, baseline, cap_height }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::draw::{TextLayout, TextSystem, Weight};

    fn params(family: &Family, max_width: Option<f64>, line_height: Option<f64>, opsz: Option<f64>) -> TextParams<'_> {
        TextParams { family, size: 14.0, weight: Weight::REGULAR, optical_size: opsz, line_height, max_width, rtl: false }
    }

    #[test]
    fn families_resolve() {
        let t = Text::shared().unwrap();
        let segoe = t.resolve_family(&["Segoe UI"]);
        assert!(segoe.0.index.is_some());
        assert_eq!(t.resolve_family(&["No Such Family xdialog", "Segoe UI"]), segoe);
        assert_eq!(t.resolve_family(&["system-ui"]), segoe);
        assert_eq!(t.resolve_family(&["No Such Family xdialog"]), segoe);
    }

    #[test]
    fn measures_wraps_and_spaces_lines() {
        let t = Text::shared().unwrap();
        let fam = t.resolve_family(&["Segoe UI"]);
        let text = "The quick brown fox jumps over the lazy dog, again and again. 你好 👋";
        let natural = t.layout(text, &params(&fam, None, None, None));
        let wrapped = t.layout(text, &params(&fam, Some(150.0), None, None));
        assert_eq!(natural.line_count(), 1);
        assert!(natural.size().width > 150.0);
        assert!(wrapped.line_count() >= 3 && wrapped.size().width <= 150.0, "{:?}", wrapped.size());
        // Segoe UI: (2210 + 514) / 2048 em per line.
        assert!((natural.size().height / 14.0 - 1.33).abs() < 0.01, "{:?}", natural.size());
        let spaced = t.layout("Hello world", &params(&fam, Some(40.0), Some(30.0), None));
        assert_eq!(spaced.line_count(), 2);
        assert!((spaced.size().height - 60.0).abs() < 0.01, "{:?}", spaced.size());
        assert_ne!(natural.id(), t.layout(text, &params(&fam, None, None, None)).id());
    }

    #[test]
    fn trailing_whitespace_is_not_measured() {
        let t = Text::shared().unwrap();
        let fam = t.resolve_family(&["Segoe UI"]);
        let a = t.layout("Hello", &params(&fam, None, None, None)).size().width;
        let b = t.layout("Hello   ", &params(&fam, None, None, None)).size().width;
        assert!(a > 0.0 && (a - b).abs() < 1e-3, "{a} {b}");
    }

    /// A right-to-left paragraph in a wide box measures its own width, not the box's.
    #[test]
    fn rtl_width_is_the_text_width() {
        let t = Text::shared().unwrap();
        let fam = t.resolve_family(&["Segoe UI"]);
        let text = "\u{05e9}\u{05dc}\u{05d5}\u{05dd} \u{05e2}\u{05d5}\u{05dc}\u{05dd}";
        let natural = t.layout(text, &TextParams { rtl: true, ..params(&fam, None, None, None) });
        let boxed = t.layout(text, &TextParams { rtl: true, ..params(&fam, Some(300.0), None, None) });
        assert!(natural.size().width > 20.0 && natural.size().width < 150.0, "{:?}", natural.size());
        assert!((boxed.size().width - natural.size().width).abs() < 0.5, "{:?} {:?}", boxed.size(), natural.size());
    }

    /// Fluent's variable-font path (optical size) lays out like the plain one, roughly.
    #[test]
    fn optical_size_path() {
        let t = Text::shared().unwrap();
        let fam = t.resolve_family(crate::backends::gui::theme::new(crate::XDialogBackend::Fluent).fonts().families);
        let plain = t.layout("Hello world", &params(&fam, None, None, None));
        let opsz = t.layout("Hello world", &params(&fam, None, None, Some(10.5)));
        assert!(opsz.0.layout.is_some() && opsz.size().width > 40.0, "{:?}", opsz.size());
        assert!((opsz.size().width / plain.size().width - 1.0).abs() < 0.2, "{:?} {:?}", opsz.size(), plain.size());
    }
}
