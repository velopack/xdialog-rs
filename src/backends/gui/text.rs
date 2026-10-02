//! Text for themes, measured and drawn with the drawing backend's layouts (so layout matches what
//! the backend draws).
//!
//! A [`TextBlock`] is one layout per `\n` paragraph, stacked. Each paragraph is laid out in its
//! own direction: a right-to-left paragraph (first strong character) is laid out at its own width
//! with its lines right-aligned, and right-aligned within the block. A centred style
//! ([`TextStyle::centered`]) centres every line and paragraph in the block instead. The block itself records
//! whether it starts right-to-left, so painters can right-align it in its column. `max_lines`
//! elides the text (with `…`) until it fits.
//!
//! Layouts carry no colour (it is chosen when a block is painted), so a block is laid out once
//! and painted in any colour. [`TextCache`] keeps blocks and paragraph layouts across passes.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::time::Duration;

use crate::backends::draw::{Family, Layout, Point, Size, Text, TextLayout, TextParams, TextSystem, Weight};

const ELLIPSIS: char = '\u{2026}';

/// How to lay out a run of text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextStyle {
    /// Font size in logical px.
    pub size: f64,
    /// The theme's bold weight instead of its regular one.
    pub bold: bool,
    /// Every line centred in the block (instead of start-aligned in its paragraph's direction).
    pub center: bool,
}

impl TextStyle {
    pub(crate) const fn regular(size: f64) -> Self {
        TextStyle { size, bold: false, center: false }
    }

    pub(crate) const fn bold(size: f64) -> Self {
        TextStyle { size, bold: true, center: false }
    }

    /// The same style with centred lines.
    pub(crate) const fn centered(self) -> Self {
        TextStyle { center: true, ..self }
    }

    /// `bold` and `center` as one cache key word.
    fn flags(&self) -> u64 {
        self.bold as u64 | (self.center as u64) << 1
    }
}

/// The font family a theme renders with (the first of `families` the platform has, else its UI
/// font), the weights of its regular and bold text, and its metrics per font size (logical px).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ThemeFonts {
    pub families: &'static [&'static str],
    pub regular: Weight,
    pub bold: Weight,
    /// Uniform line pitch (`None`: the font's natural one).
    pub line_height: fn(f64) -> Option<f64>,
    /// Variable-font optical size in pt (`None`: the font's default).
    pub opsz: fn(f64) -> Option<f64>,
}

/// A laid-out paragraph stack (colour chosen at paint time).
#[derive(Clone, Default)]
pub(crate) struct TextBlock {
    /// Widest line x total height, rounded up to whole logical px. Empty text: zero.
    pub size: Size,
    /// The text starts right-to-left (painters right-align the block in its column).
    pub rtl: bool,
    pub lines: usize,
    /// Paragraph layouts and their top-left within the block.
    pub(crate) paras: Vec<(Layout, Point)>,
}

impl TextBlock {
    pub(crate) fn is_empty(&self) -> bool {
        self.paras.is_empty()
    }

    /// The middle of the first line's capitals below the block's top: centring this (rather
    /// than the line box) centres a label optically whatever the font's ascent and descent.
    pub(crate) fn cap_center(&self) -> f64 {
        match self.paras.first() {
            Some((l, pos)) => pos.y + l.first_baseline() - l.cap_height() / 2.0,
            None => self.size.height / 2.0,
        }
    }
}

/// Whether `text` starts right-to-left (its first strong character).
pub(crate) fn starts_rtl(text: &str) -> bool {
    unicode_bidi::get_base_direction(text) == unicode_bidi::Direction::Rtl
}

/// Text layouts of one dialog, cached across passes (layouts used in the last pass are kept).
pub(crate) struct TextCache {
    text: Rc<Text>,
    fonts: Option<&'static ThemeFonts>,
    family: Option<Family>,
    generation: u64,
    blocks: Cache<Rc<TextBlock>>,
    paras: Cache<Layout>,
}

/// A cached value and what it was made from (the key's hash is only a hash).
struct Entry<V> {
    text: Box<str>,
    bits: [u64; 4],
    value: V,
}

/// A two-generation cache keyed by text plus four words of style bits: entries not used in a
/// pass are dropped after the next one. A hit allocates nothing.
struct Cache<V> {
    prev: HashMap<u64, Entry<V>>,
    cur: HashMap<u64, Entry<V>>,
}

impl<V: Clone> Cache<V> {
    fn new() -> Self {
        Cache { prev: HashMap::new(), cur: HashMap::new() }
    }

    fn hash(text: &str, bits: &[u64; 4]) -> u64 {
        let mut h = DefaultHasher::new();
        text.hash(&mut h);
        bits.hash(&mut h);
        h.finish()
    }

    fn get(&mut self, text: &str, bits: [u64; 4]) -> Option<V> {
        let k = Self::hash(text, &bits);
        let hit = |e: &Entry<V>| &*e.text == text && e.bits == bits;
        if let Some(e) = self.cur.get(&k).filter(|e| hit(e)) {
            return Some(e.value.clone());
        }
        let e = self.prev.remove(&k).filter(|e| hit(e))?;
        let value = e.value.clone();
        self.cur.insert(k, e);
        Some(value)
    }

    fn insert(&mut self, text: &str, bits: [u64; 4], value: V) {
        self.cur.insert(Self::hash(text, &bits), Entry { text: text.into(), bits, value });
    }

    fn end_pass(&mut self) {
        self.prev = std::mem::take(&mut self.cur);
    }

    fn clear(&mut self) {
        self.prev.clear();
        self.cur.clear();
    }
}

impl TextCache {
    pub(crate) fn new(text: Rc<Text>) -> Self {
        TextCache { text, fonts: None, family: None, generation: 0, blocks: Cache::new(), paras: Cache::new() }
    }

    /// Start of a pass in `fonts` (a font change, or new fonts on the system, drops every cached
    /// layout).
    pub(crate) fn begin_pass(&mut self, fonts: &'static ThemeFonts) {
        let generation = self.text.generation();
        if !self.fonts.is_some_and(|f| std::ptr::eq(f, fonts)) || self.generation != generation {
            self.family = Some(self.text.resolve_family(fonts.families));
            self.fonts = Some(fonts);
            self.generation = generation;
            self.blocks.clear();
            self.paras.clear();
        }
    }

    /// End of a pass: forget layouts not used in it.
    pub(crate) fn end_pass(&mut self) {
        self.blocks.end_pass();
        self.paras.end_pass();
    }

    /// Make fallback faces covering `texts` available (see `TextSystem::prepare_fonts`); `false`:
    /// still loading. Call after `begin_pass`.
    pub(crate) fn prepare_fonts(&self, texts: &[&str], wait: Duration) -> bool {
        self.family.as_ref().is_none_or(|f| self.text.prepare_fonts(f, texts, wait))
    }

    /// One paragraph layout (cached).
    fn para(&mut self, text: &str, style: &TextStyle, max_width: Option<f64>, rtl: bool) -> Layout {
        let bits = [style.size.to_bits(), style.flags(), max_width.map_or(u64::MAX, f64::to_bits), rtl as u64];
        if let Some(l) = self.paras.get(text, bits) {
            return l;
        }
        let fonts = self.fonts.expect("begin_pass first");
        let family = self.family.as_ref().expect("begin_pass first");
        let params = TextParams { family,
                                  size: style.size,
                                  weight: if style.bold { fonts.bold } else { fonts.regular },
                                  optical_size: (fonts.opsz)(style.size),
                                  line_height: (fonts.line_height)(style.size),
                                  max_width,
                                  rtl,
                                  center: style.center };
        let layout = self.text.layout(text, &params);
        self.paras.insert(text, bits, layout.clone());
        layout
    }

    /// Lay out `text`, wrapped at `wrap_width` (logical px; `f64::INFINITY` = no wrapping), into
    /// at most `max_lines` lines (elided). Cached.
    pub(crate) fn layout(&mut self, text: &str, style: &TextStyle, wrap_width: f64, max_lines: Option<usize>) -> Rc<TextBlock> {
        let bits = [style.size.to_bits(), style.flags(), wrap_width.to_bits(), max_lines.map_or(u64::MAX, |n| n as u64)];
        if let Some(b) = self.blocks.get(text, bits) {
            return b;
        }
        let block = Rc::new(self.build(text, style, wrap_width, max_lines));
        self.blocks.insert(text, bits, block.clone());
        block
    }

    fn build(&mut self, text: &str, style: &TextStyle, wrap_width: f64, max_lines: Option<usize>) -> TextBlock {
        if text.is_empty() {
            return TextBlock::default();
        }
        let block = self.stack(text, style, wrap_width);
        let Some(n) = max_lines.map(|n| n.max(1)).filter(|n| block.lines > *n) else { return block };
        // Elide: the longest prefix that fits in `n` lines with an ellipsis.
        let ends: Vec<usize> = text.char_indices().map(|(i, c)| i + c.len_utf8()).collect();
        let (mut lo, mut hi) = (0usize, ends.len());
        let mut best = self.stack(&ELLIPSIS.to_string(), style, wrap_width);
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            let candidate = format!("{}{ELLIPSIS}", text[..ends[mid - 1]].trim_end());
            let b = self.stack(&candidate, style, wrap_width);
            if b.lines <= n {
                best = b;
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        best
    }

    /// The paragraphs of `text` stacked (see the module docs).
    fn stack(&mut self, text: &str, style: &TextStyle, wrap_width: f64) -> TextBlock {
        let wrap = wrap_width.is_finite().then_some(wrap_width);
        let mut paras = Vec::new();
        let (mut y, mut width, mut lines) = (0.0f64, 0.0f64, 0usize);
        for para in text.split('\n') {
            let rtl = starts_rtl(para);
            let mut layout = self.para(para, style, wrap, rtl);
            let mut w = layout.size().width;
            if rtl || style.center {
                // Lines right-aligned (or centred) within the paragraph's own width.
                let own = w.ceil() + 1.0;
                layout = self.para(para, style, Some(own), rtl);
                w = own;
            }
            let size = layout.size();
            width = width.max(size.width);
            lines += layout.line_count().max(1);
            paras.push((layout, y, rtl, w));
            y += size.height;
        }
        let block_w = paras.iter().map(|p| p.3).fold(width, f64::max);
        let rtl = paras.first().is_some_and(|p| p.2);
        let x = |rtl: bool, w: f64| {
            if style.center {
                ((block_w - w) / 2.0).round()
            } else if rtl {
                block_w - w
            } else {
                0.0
            }
        };
        let paras = paras.into_iter().map(|(layout, y, rtl, w)| (layout, Point::new(x(rtl, w), y))).collect();
        TextBlock { size: Size::new(block_w.ceil(), y.ceil()), rtl, lines, paras }
    }
}

#[cfg(test)]
mod tests {
    //! TEMP (WP1): ignored off the software backend while d2d and cg are stubs; WP2 / WP3 remove
    //! the `cfg_attr(not(draw_soft), ignore)`s.

    use super::*;
    use crate::backends::ubuntu::FONTS;

    /// A cache in the Ubuntu theme's fonts (line pitch 1.2 × size), ready for a pass.
    fn cache() -> TextCache {
        let mut c = TextCache::new(Text::shared().expect("text system"));
        c.begin_pass(&FONTS);
        c
    }

    const STYLE: TextStyle = TextStyle::regular(14.0);
    const PITCH: f64 = 14.0 * 1.2;

    #[test]
    fn layout_wraps_and_elides() {
        let mut c = cache();
        let text = "The quick brown fox jumps over the lazy dog, again and again and again.";
        let natural = c.layout(text, &STYLE, f64::INFINITY, None);
        assert_eq!(natural.lines, 1);
        assert!(natural.size.width > 150.0, "{:?}", natural.size);
        let block = c.layout(text, &STYLE, 150.0, None);
        assert!(block.lines >= 3, "{}", block.lines);
        assert!((block.size.height - (block.lines as f64 * PITCH).ceil()).abs() < 1.0, "{:?}", block.size);
        assert!(block.size.width <= 150.0, "{:?}", block.size);
        let cut = c.layout(text, &STYLE, 150.0, Some(2));
        assert_eq!(cut.lines, 2);
        assert!(cut.size.width <= 150.0 && cut.size.height < block.size.height, "{:?}", cut.size);
        // One line: a prefix plus the ellipsis, narrower than the text.
        let one = c.layout(text, &STYLE, 150.0, Some(1));
        assert_eq!(one.lines, 1);
        assert!(one.size.width <= 150.0);
        // Cached: the same block again.
        assert!(Rc::ptr_eq(&cut, &c.layout(text, &STYLE, 150.0, Some(2))));
    }

    #[test]
    fn explicit_newlines_and_rtl_lines() {
        let mut c = cache();
        let ltr = c.layout("one\ntwo\n\nfour", &STYLE, f64::INFINITY, None);
        assert_eq!((ltr.lines, ltr.paras.len(), ltr.rtl), (4, 4, false));
        assert!((ltr.size.height - (4.0 * PITCH).ceil()).abs() < 1.0, "{:?}", ltr.size);
        // Hebrew may have no face here (tofu): the layout path still runs.
        let text = "שלום עולם שלום עולם שלום עולם שלום עולם";
        let rtl = c.layout(text, &STYLE, 80.0, None);
        assert!(rtl.rtl);
        assert!(rtl.lines >= 2 && rtl.size.width <= 80.0 + 1.0, "{} {:?}", rtl.lines, rtl.size);
        let cut = c.layout(text, &STYLE, 80.0, Some(2));
        assert!(cut.rtl && cut.lines == 2, "{}", cut.lines);
    }

    /// Each paragraph is aligned by its own direction: an RTL paragraph after an LTR one is
    /// right-aligned within the block, the LTR one stays left.
    #[test]
    fn mixed_paragraphs_align_by_their_own_direction() {
        let mut c = cache();
        let block = c.layout("This is an English paragraph.\n\u{05e9}\u{05dc}\u{05d5}\u{05dd}", &STYLE, f64::INFINITY, None);
        assert!(!block.rtl);
        let (ltr, rtl) = (&block.paras[0], &block.paras[1]);
        assert_eq!(ltr.1.x, 0.0);
        assert!(rtl.1.x > 10.0, "{:?}", rtl.1);
        // The RTL paragraph's box ends at the block's right edge. Its ink is up to 3 px short of it:
        // it is laid out at `ceil(width) + 1`, and the block's width is rounded up.
        let gap = block.size.width - (rtl.1.x + rtl.0.size().width);
        assert!((0.0..3.0).contains(&gap), "{gap}: {:?} {:?} {:?}", rtl.1, rtl.0.size(), block.size);
        assert!(!starts_rtl("abc \u{05e9}") && starts_rtl("\u{05e9} abc") && !starts_rtl("123"));
    }

    #[test]
    fn empty_text_is_an_empty_block() {
        let mut c = cache();
        let b = c.layout("", &STYLE, 100.0, Some(1));
        assert!(b.is_empty() && b.size == Size::ZERO && b.lines == 0);
    }
}
