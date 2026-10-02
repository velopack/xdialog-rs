//! Text on CoreText: fonts, line breaking with `CTTypesetter`, one `CTLine` per line.
//!
//! Bidi, the font cascade (fallback) and colour emoji (sbix) are CoreText's. The colour is not part
//! of a layout (`kCTForegroundColorFromContextAttributeName`): the canvas sets the context's fill
//! colour before drawing the lines.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::ptr::{self, NonNull};
use std::rc::Rc;

use objc2_core_foundation::{CFAttributedString, CFBoolean, CFDictionary, CFNumber, CFRange, CFRetained, CFString, CFType, CGFloat};
use objc2_core_graphics::CGColorSpace;
use objc2_core_text::{
    kCTFontAttributeName, kCTFontFamilyNameAttribute, kCTFontTraitsAttribute, kCTFontWeightTrait,
    kCTForegroundColorFromContextAttributeName, kCTParagraphStyleAttributeName, CTFont, CTFontDescriptor, CTFontSymbolicTraits,
    CTFontUIFontType, CTLine, CTParagraphStyle, CTParagraphStyleSetting, CTParagraphStyleSpecifier, CTTypesetter, CTWritingDirection,
};

use crate::backends::draw::{DrawError, Size, TextParams};

/// A font family: the system UI font (SF) or one the system has by name.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Family {
    /// `CTFontCreateUIFontForLanguage(kCTFontUIFontSystem)`.
    SystemUi,
    /// A family name `resolve_family` found installed.
    Named(Rc<str>),
}

/// One laid out line: drawn with its origin (on the baseline) at `(x, baseline)` from the layout's
/// top-left, logical px.
pub(crate) struct Line {
    pub(crate) line: CFRetained<CTLine>,
    pub(crate) x: f64,
    pub(crate) baseline: f64,
}

/// A laid out paragraph.
pub(crate) struct CtLayout {
    id: u64,
    size: Size,
    line_count: usize,
    baseline: f64,
    cap_height: f64,
    pub(crate) lines: Vec<Line>,
}

/// A laid out paragraph (cheap to clone).
#[derive(Clone)]
pub(crate) struct Layout(pub(crate) Rc<CtLayout>);

impl crate::backends::draw::TextLayout for Layout {
    fn size(&self) -> Size {
        self.0.size
    }

    fn line_count(&self) -> usize {
        self.0.line_count
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

/// Fonts by (family, weight, size bits).
type FontKey = (Family, u16, u64);

/// The text service (one per thread).
pub(crate) struct Text {
    fonts: RefCell<HashMap<FontKey, CFRetained<CTFont>>>,
    /// Device RGB, shared by every surface of this thread.
    pub(crate) color_space: CFRetained<CGColorSpace>,
}

/// Fonts cached before the cache is cleared (sizes animate rarely; this is a safety net).
const FONT_LIMIT: usize = 64;

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
                  let color_space =
                      CGColorSpace::new_device_rgb().ok_or_else(|| DrawError::Backend("cg: CGColorSpaceCreateDeviceRGB failed".into()))?;
                  Ok(s.get_or_init(|| Rc::new(Text { fonts: RefCell::new(HashMap::new()), color_space })).clone())
              })
    }

    /// The first candidate installed; "system-ui", "sans-serif" and nothing found are the system
    /// UI font (SF). Segoe UI and Ubuntu are normally missing on macOS, so both looks use SF.
    fn resolve_family(&self, candidates: &[&str]) -> Family {
        for &name in candidates {
            if matches!(name, "system-ui" | "sans-serif") {
                break;
            }
            if family_installed(name) {
                return Family::Named(name.into());
            }
        }
        Family::SystemUi
    }

    fn layout(&self, text: &str, p: &TextParams<'_>) -> Layout {
        let font = self.font(p.family, p.weight.0, p.size);
        Layout(Rc::new(lay_out(text, &font, p)))
    }
}

impl Text {
    /// The font for a family, weight and size (cached).
    fn font(&self, family: &Family, weight: u16, size: f64) -> CFRetained<CTFont> {
        // CoreText takes size 0 as "the default size" (12 pt).
        let size = if size > 0.0 { size } else { 0.01 };
        let key = (family.clone(), weight, size.to_bits());
        if let Some(f) = self.fonts.borrow().get(&key) {
            return f.clone();
        }
        let font = new_font(family, weight, size);
        let mut fonts = self.fonts.borrow_mut();
        if fonts.len() >= FONT_LIMIT {
            fonts.clear();
        }
        fonts.insert(key, font.clone());
        font
    }
}

/// `kCTFontWeightTrait` (-1.0 ..= 1.0) for a CSS weight: AppKit's `NSFontWeight*` values
/// (400 regular 0.0, 500 medium 0.23, 600 semibold 0.3, 700 bold 0.4), linear in between.
fn weight_trait(weight: u16) -> f64 {
    const TABLE: [(f64, f64); 9] = [(100.0, -0.8),
                                    (200.0, -0.6),
                                    (300.0, -0.4),
                                    (400.0, 0.0),
                                    (500.0, 0.23),
                                    (600.0, 0.3),
                                    (700.0, 0.4),
                                    (800.0, 0.56),
                                    (900.0, 0.62)];
    let w = (weight as f64).clamp(100.0, 900.0);
    let i = TABLE.iter().position(|&(x, _)| x >= w).unwrap_or(TABLE.len() - 1).max(1);
    let ((x0, y0), (x1, y1)) = (TABLE[i - 1], TABLE[i]);
    y0 + (y1 - y0) * (w - x0) / (x1 - x0)
}

/// `{ kCTFontTraitsAttribute: { kCTFontWeightTrait: weight } }`, plus the family name if given.
fn descriptor_attributes(family: Option<&CFString>, weight: u16) -> CFRetained<CFDictionary<CFString, CFType>> {
    let w = CFNumber::new_f64(weight_trait(weight));
    // SAFETY: the `kCT…` keys are immutable CFString constants exported by CoreText.
    let (weight_key, traits_key, family_key) = unsafe { (kCTFontWeightTrait, kCTFontTraitsAttribute, kCTFontFamilyNameAttribute) };
    let traits = CFDictionary::<CFString, CFNumber>::from_slices(&[weight_key], &[&w]);
    let traits: &CFType = &traits;
    match family {
        Some(name) => {
            let name: &CFType = name;
            CFDictionary::from_slices(&[traits_key, family_key], &[traits, name])
        }
        None => CFDictionary::from_slices(&[traits_key], &[traits]),
    }
}

/// Whether a font family named `name` is installed (the best match for the name is in it).
fn family_installed(name: &str) -> bool {
    let name = CFString::from_str(name);
    // SAFETY: the key is an immutable CFString constant exported by CoreText; the dictionary maps
    // it to a CFString as `kCTFontFamilyNameAttribute` requires; `None` = no mandatory attributes.
    unsafe {
        let attrs = CFDictionary::<CFString, CFType>::from_slices(&[kCTFontFamilyNameAttribute], &[&*name]);
        let desc = CTFontDescriptor::with_attributes(attrs.as_opaque());
        let Some(found) = desc.matching_font_descriptor(None) else { return false };
        found.attribute(kCTFontFamilyNameAttribute)
             .and_then(|f| f.downcast::<CFString>().ok())
             .is_some_and(|f| f.to_string().eq_ignore_ascii_case(&name.to_string()))
    }
}

/// A new font: the family at the size, as close to `weight` as the family has.
fn new_font(family: &Family, weight: u16, size: f64) -> CFRetained<CTFont> {
    // SAFETY: plain CoreText object creation; every matrix argument is null (identity), the
    // dictionaries hold the value types their keys require (see `descriptor_attributes`).
    unsafe {
        let system = || {
            CTFont::new_ui_font_for_language(CTFontUIFontType::System, size, None)
                .unwrap_or_else(|| CTFont::with_name(&CFString::from_static_str("Helvetica"), size, ptr::null()))
        };
        match family {
            Family::Named(name) => {
                let name = CFString::from_str(name);
                let desc = CTFontDescriptor::with_attributes(descriptor_attributes(Some(&name), weight).as_opaque());
                let font = CTFont::with_font_descriptor(&desc, size, ptr::null());
                if font.family_name().to_string().eq_ignore_ascii_case(&name.to_string()) {
                    return font;
                }
                // Uninstalled since `resolve_family`: the system font.
                with_weight(system(), weight, size)
            }
            Family::SystemUi => with_weight(system(), weight, size),
        }
    }
}

/// `base` (the system font) in `weight`: a descriptor copy with the weight trait. If that lands
/// in another family or weight (the system font's descriptor did not take the trait), the
/// symbolic bold trait for weights from 600, else `base` itself.
///
/// # Safety
/// Plain CoreText calls; see `new_font`.
unsafe fn with_weight(base: CFRetained<CTFont>, weight: u16, size: f64) -> CFRetained<CTFont> {
    if weight == 400 {
        return base;
    }
    let wanted = weight_trait(weight);
    let desc = unsafe { base.font_descriptor().copy_with_attributes(descriptor_attributes(None, weight).as_opaque()) };
    let font = unsafe { CTFont::with_font_descriptor(&desc, size, ptr::null()) };
    let same_family = unsafe { font.family_name().to_string() == base.family_name().to_string() };
    if same_family && unsafe { weight_of(&font) }.is_none_or(|w| (w - wanted).abs() < 0.1) {
        return font;
    }
    if weight >= 600 {
        let bold = CTFontSymbolicTraits::BoldTrait;
        if let Some(b) = unsafe { base.copy_with_symbolic_traits(size, ptr::null(), bold, bold) } {
            return b;
        }
    }
    if same_family {
        font
    } else {
        base
    }
}

/// The font's `kCTFontWeightTrait`.
///
/// # Safety
/// Plain CoreText calls.
unsafe fn weight_of(font: &CTFont) -> Option<f64> {
    // SAFETY: CTFontCopyTraits returns a dictionary with CFString keys and CFType values.
    let traits = unsafe { CFRetained::cast_unchecked::<CFDictionary<CFString, CFType>>(font.traits()) };
    let w = traits.get(unsafe { kCTFontWeightTrait })?.downcast::<CFNumber>().ok()?;
    w.as_f64().or_else(|| w.as_f32().map(f64::from))
}

/// The attributes of a paragraph: the font, the colour from the context, the base direction.
fn attributes(font: &CTFont, rtl: bool) -> CFRetained<CFDictionary<CFString, CFType>> {
    let direction = if rtl { CTWritingDirection::RightToLeft } else { CTWritingDirection::LeftToRight };
    let setting = CTParagraphStyleSetting { spec: CTParagraphStyleSpecifier::BaseWritingDirection,
                                            valueSize: size_of::<CTWritingDirection>(),
                                            value: NonNull::from(&direction).cast() };
    // SAFETY: one setting whose value points at a live `CTWritingDirection` (int8_t, as the
    // specifier requires; CoreText copies it); the keys are immutable CoreText constants, and each
    // maps to the value type it requires (CTFont, CFBoolean, CTParagraphStyle).
    unsafe {
        let style = CTParagraphStyle::new(&setting, 1);
        let values: [&CFType; 3] = [font, CFBoolean::new(true), &style];
        CFDictionary::from_slices(&[kCTFontAttributeName, kCTForegroundColorFromContextAttributeName, kCTParagraphStyleAttributeName],
                                  &values)
    }
}

/// Lay out one paragraph: lines broken at `max_width` (whitespace, else clusters), each start-
/// aligned in the paragraph's direction (or centred); baselines the font's ascent below each line top
/// (with a `line_height`, shifted by half its difference to ascent + descent).
fn lay_out(text: &str, font: &CTFont, p: &TextParams<'_>) -> CtLayout {
    // SAFETY: CoreText calls on objects created here; every `CFRange` passed lies inside the
    // attributed string (`start < len`, `1 <= n <= len - start`); null metric out-pointers are
    // allowed by `CTLineGetTypographicBounds`.
    unsafe {
        let (ascent, descent, leading) = (font.ascent(), font.descent(), font.leading());
        let pitch = p.line_height.unwrap_or(ascent + descent + leading);
        let ascent = match p.line_height {
            Some(lh) => ascent + (lh - ascent - descent) / 2.0,
            None => ascent,
        };
        let cap = font.cap_height();
        let string = CFString::from_str(text);
        let attrs = attributes(font, p.rtl);
        let Some(astring) = CFAttributedString::new(None, Some(&string), Some(attrs.as_opaque())) else {
            return empty_layout(pitch, ascent, cap);
        };
        let len = astring.length();
        if len <= 0 {
            return empty_layout(pitch, ascent, cap);
        }
        let typesetter = CTTypesetter::with_attributed_string(&astring);
        let break_width = p.max_width.map_or(1e7, |w| w.max(0.0));
        // (line, width without trailing whitespace)
        let mut lines: Vec<(CFRetained<CTLine>, f64)> = Vec::new();
        let mut start = 0;
        while start < len {
            let mut n = typesetter.suggest_line_break(start, break_width);
            if n <= 0 {
                n = typesetter.suggest_cluster_break(start, break_width);
            }
            let n = if n <= 0 { len - start } else { n.min(len - start) };
            let line = typesetter.line(CFRange::new(start, n));
            let width =
                (line.typographic_bounds(ptr::null_mut(), ptr::null_mut(), ptr::null_mut()) - line.trailing_whitespace_width()).max(0.0);
            lines.push((line, width));
            start += n;
        }
        if lines.is_empty() {
            return empty_layout(pitch, ascent, cap);
        }
        let width = lines.iter().map(|l| l.1).fold(0.0, f64::max);
        let flush_width = p.max_width.unwrap_or(width);
        let flush: CGFloat = if p.center {
            0.5
        } else if p.rtl {
            1.0
        } else {
            0.0
        };
        let line_count = lines.len();
        let lines = lines.into_iter()
                         .enumerate()
                         .map(|(i, (line, _))| {
                             let x = line.pen_offset_for_flush(flush, flush_width);
                             Line { line, x, baseline: i as f64 * pitch + ascent }
                         })
                         .collect();
        CtLayout { id: crate::backends::draw::next_id(),
                   size: Size::new(width, line_count as f64 * pitch),
                   line_count,
                   baseline: ascent,
                   cap_height: cap,
                   lines }
    }
}

/// The layout of an empty paragraph: one empty line.
fn empty_layout(pitch: f64, baseline: f64, cap_height: f64) -> CtLayout {
    CtLayout { id: crate::backends::draw::next_id(), size: Size::new(0.0, pitch), line_count: 1, baseline, cap_height, lines: Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::draw::{TextLayout, TextSystem, Weight};

    fn params(family: &Family, max_width: Option<f64>, line_height: Option<f64>, rtl: bool) -> TextParams<'_> {
        TextParams { family, size: 14.0, weight: Weight::REGULAR, optical_size: None, line_height, max_width, rtl, center: false }
    }

    #[test]
    fn weights_map_to_appkit_traits() {
        assert_eq!(weight_trait(400), 0.0);
        assert_eq!(weight_trait(600), 0.3);
        assert_eq!(weight_trait(700), 0.4);
        assert!((weight_trait(450) - 0.115).abs() < 1e-9);
        assert_eq!(weight_trait(0), -0.8);
        assert_eq!(weight_trait(1000), 0.62);
    }

    #[test]
    fn families_resolve_to_the_system_font_when_missing() {
        let t = Text::shared().unwrap();
        assert_eq!(t.resolve_family(&["No Such Family xdialog"]), Family::SystemUi);
        assert_eq!(t.resolve_family(&["system-ui", "Helvetica"]), Family::SystemUi);
        assert_eq!(t.resolve_family(&["Helvetica"]), Family::Named("Helvetica".into()));
    }

    #[test]
    fn wraps_and_measures() {
        let t = Text::shared().unwrap();
        let fam = Family::SystemUi;
        let text = "The quick brown fox jumps over the lazy dog, again and again and again.";
        let natural = t.layout(text, &params(&fam, None, None, false));
        let wrapped = t.layout(text, &params(&fam, Some(150.0), None, false));
        assert_eq!(natural.line_count(), 1);
        assert!(natural.size().width > 150.0);
        assert!(wrapped.line_count() >= 3 && wrapped.size().width <= 150.0, "{:?}", wrapped.size());
        assert_eq!(t.layout("", &params(&fam, None, None, false)).size().height, natural.size().height);
        // An over-long word is broken.
        let long = t.layout("Supercalifragilisticexpialidocious", &params(&fam, Some(40.0), None, false));
        assert!(long.line_count() >= 3 && long.size().width <= 40.0, "{} {:?}", long.line_count(), long.size());
    }

    #[test]
    fn trailing_whitespace_is_not_measured() {
        let t = Text::shared().unwrap();
        let a = t.layout("Hello", &params(&Family::SystemUi, None, None, false)).size().width;
        let b = t.layout("Hello   ", &params(&Family::SystemUi, None, None, false)).size().width;
        assert!(a > 0.0 && (a - b).abs() < 1e-6, "{a} {b}");
    }

    #[test]
    fn line_height_is_the_pitch_and_leading_is_split() {
        let t = Text::shared().unwrap();
        let fam = Family::SystemUi;
        let natural = t.layout("Hi", &params(&fam, None, None, false));
        let tall = t.layout("Hi", &params(&fam, None, Some(30.0), false));
        let two = t.layout("Hello world", &params(&fam, Some(40.0), Some(30.0), false));
        assert_eq!(tall.size().height, 30.0);
        assert_eq!(two.line_count(), 2);
        assert_eq!(two.size().height, 60.0);
        let font = t.font(&fam, 400, 14.0);
        // SAFETY: metric getters on a live font.
        let (ascent, descent) = unsafe { (font.ascent(), font.descent()) };
        let expected = ascent + (30.0 - ascent - descent) / 2.0;
        assert!((tall.0.lines[0].baseline - expected).abs() < 1e-9, "{} {expected}", tall.0.lines[0].baseline);
        assert!(natural.0.lines[0].baseline < tall.0.lines[0].baseline);
        assert_eq!(two.0.lines[1].baseline - two.0.lines[0].baseline, 30.0);
    }

    #[test]
    fn rtl_lines_are_right_aligned() {
        let t = Text::shared().unwrap();
        let fam = Family::SystemUi;
        let rtl = t.layout("abc", &params(&fam, Some(200.0), None, true));
        let ltr = t.layout("abc", &params(&fam, Some(200.0), None, false));
        assert!(rtl.0.lines[0].x > 150.0, "{}", rtl.0.lines[0].x);
        assert!(ltr.0.lines[0].x.abs() < 0.5, "{}", ltr.0.lines[0].x);
    }

    #[test]
    fn bold_is_wider() {
        let t = Text::shared().unwrap();
        let fam = Family::SystemUi;
        let regular = t.layout("Hello world", &params(&fam, None, None, false)).size().width;
        let bold = t.layout("Hello world", &TextParams { weight: Weight::BOLD, ..params(&fam, None, None, false) }).size().width;
        assert!(bold > regular, "{bold} {regular}");
    }
}
