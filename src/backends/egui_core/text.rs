//! Text layout for themes.
//!
//! Paragraphs are wrapped first, then every wrapped line is laid out as its own galley and placed
//! manually at `k * line_pitch` (optionally rounded), with the half-leading offset applied. This
//! gives exact control over line pitch (skia: `1.2 * size`; Fluent: fractional DirectWrite pitch),
//! which epaint's own row placement (rounded row heights) cannot express.
//!
//! Left-to-right text is wrapped by epaint itself. Text that needs bidi (see `bidi.rs`) is wrapped
//! here in logical order at word boundaries (measuring each candidate line in visual order), then
//! every line is reordered into visual order before it is laid out. Lines of right-to-left
//! paragraphs are right-aligned within the block.
//!
//! egui caches galleys by layout job, so repeated layouts of the same strings are cheap.

use std::sync::Arc;

use egui::epaint::text::{LayoutJob, TextFormat, VariationCoords};
use egui::{Color32, FontFamily, FontId, Galley, Painter, Pos2, Vec2};
#[cfg(test)]
use egui::{Response, Sense, Ui, Widget};

use super::bidi;

const ELLIPSIS: char = '\u{2026}';

/// How to lay out a run of text.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextStyle {
    /// Font size in logical px.
    pub size: f32,
    /// Font family (must be bound by `Theme::install_fonts`), e.g. `Proportional` or `Name("bold")`.
    pub family: FontFamily,
    /// Variable-font coordinates (e.g. `wght`, `opsz`); empty for static faces.
    pub coords: VariationCoords,
    /// Distance between consecutive line boxes in logical px (skia: `1.2 * size`).
    pub line_pitch: f32,
    /// `true`: line box `k` starts at `round(k * line_pitch)` (Fluent/DirectWrite); `false`: at
    /// `k * line_pitch` exactly (skia/cosmic-text).
    pub round_line_tops: bool,
}

impl TextStyle {
    /// A static-face style with the given pitch.
    pub(crate) fn new(size: f32, family: FontFamily, line_pitch: f32) -> Self {
        TextStyle { size, family, coords: VariationCoords::default(), line_pitch, round_line_tops: false }
    }

    pub(crate) fn font_id(&self) -> FontId {
        FontId::new(self.size, self.family.clone())
    }

    fn format(&self) -> TextFormat {
        TextFormat { font_id: self.font_id(), color: Color32::PLACEHOLDER, coords: self.coords.clone(), ..Default::default() }
    }
}

/// One laid-out line.
#[derive(Clone, Debug)]
pub(crate) struct TextLine {
    /// Galley of this line only (visual order), laid out with `Color32::PLACEHOLDER` (colour chosen
    /// at paint).
    pub galley: Arc<Galley>,
    /// Top-left of the galley relative to the block's top-left, half-leading already applied
    /// (the galley's row is vertically centred in its `line_pitch` line box). For right-to-left
    /// lines `TextBlock::paint` additionally shifts the line right by `block.size.x - width`.
    pub offset: Vec2,
    /// Advance width of the line in logical px (trailing whitespace of wrapped lines excluded).
    pub width: f32,
    /// Top of this line's line box relative to the block top.
    pub box_top: f32,
    /// The line belongs to a right-to-left paragraph (painted right-aligned in the block).
    pub rtl: bool,
}

/// A laid-out paragraph.
#[derive(Clone, Debug, Default)]
pub(crate) struct TextBlock {
    pub lines: Vec<TextLine>,
    /// `x` = widest line, `y` = `lines * line_pitch` (rounded when `round_line_tops`). Empty text
    /// gives `Vec2::ZERO` and no lines.
    pub size: Vec2,
    /// The text's base direction (first strong character) is right-to-left.
    #[cfg_attr(not(test), allow(dead_code))] // informational; only the tests read it
    pub rtl: bool,
    /// Lines were cut by `max_lines` (the last line ends with an ellipsis).
    #[cfg_attr(not(test), allow(dead_code))] // informational; only the tests read it
    pub truncated: bool,
}

impl TextBlock {
    pub(crate) fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Paint all lines with their top-left block corner at `top_left` in `color`. Lines of
    /// left-to-right paragraphs are left-aligned, lines of right-to-left paragraphs right-aligned
    /// within `size.x`.
    pub(crate) fn paint(&self, painter: &Painter, top_left: Pos2, color: Color32) {
        for line in &self.lines {
            let mut pos = top_left + line.offset;
            if line.rtl {
                pos.x += self.size.x - line.width;
            }
            painter.galley(pos, line.galley.clone(), color);
        }
    }
}

/// A [`TextBlock`] as an `egui::Widget`: allocates exactly `block.size` in the current layout
/// and paints the block at the allocated top-left (nothing is painted in invisible/sizing uis).
#[cfg(test)]
pub(crate) struct TextBlockWidget<'a> {
    pub block: &'a TextBlock,
    pub color: Color32,
}

#[cfg(test)]
impl<'a> TextBlockWidget<'a> {
    pub(crate) fn new(block: &'a TextBlock, color: Color32) -> Self {
        TextBlockWidget { block, color }
    }
}

#[cfg(test)]
impl Widget for TextBlockWidget<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let (rect, response) = ui.allocate_exact_size(self.block.size, Sense::hover());
        if ui.is_rect_visible(rect) {
            self.block.paint(ui.painter(), rect.min, self.color);
        }
        response
    }
}

/// Text layout context for themes (`TextCtx::new(ui.ctx())`). Only valid inside an egui pass (fonts exist).
pub(crate) struct TextCtx<'a> {
    ctx: &'a egui::Context,
}

impl<'a> TextCtx<'a> {
    pub(crate) fn new(ctx: &'a egui::Context) -> Self {
        TextCtx { ctx }
    }

    /// Width of the widest line when only explicit `\n` break lines (no wrapping).
    pub(crate) fn natural_width(&self, text: &str, style: &TextStyle) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        if bidi::needs_bidi(text) {
            return text.split('\n')
                       .map(|p| self.line_width(&bidi::visual_line(p, bidi::paragraph_is_rtl(p)), style))
                       .fold(0.0, f32::max);
        }
        let job = LayoutJob::single_section(text.to_owned(), style.format());
        let galley = self.ctx.fonts_mut(|f| f.layout_job(job));
        galley.rows.iter().map(|r| r.row.size.x).fold(0.0, f32::max)
    }

    /// Wrap `text` at `wrap_width` (logical px; `f32::INFINITY` = no wrapping) into at most
    /// `max_lines` lines (ellipsis on the last one when cut), one galley per line.
    pub(crate) fn layout(&self, text: &str, style: &TextStyle, wrap_width: f32, max_lines: Option<usize>) -> TextBlock {
        if text.is_empty() {
            return TextBlock::default();
        }
        let max_lines = max_lines.map(|n| n.max(1));
        let (rows, truncated) = if bidi::needs_bidi(text) {
            self.wrap_bidi(text, style, wrap_width, max_lines)
        } else {
            self.wrap_ltr(text, style, wrap_width, max_lines)
        };
        self.build(text, rows, truncated, style)
    }

    /// Lay out `text` with line breaks chosen by `wrap` (called once per `\n` paragraph; returns
    /// the paragraph's lines in logical order). Each line keeps its PARAGRAPH's base direction:
    /// a wrapped line of a right-to-left paragraph that starts with a Latin word stays
    /// right-to-left (re-classifying each line on its own would flip it).
    pub(crate) fn layout_wrapped<'t>(&self, text: &'t str, style: &TextStyle, wrap: impl Fn(&'t str) -> Vec<&'t str>) -> TextBlock {
        if text.is_empty() {
            return TextBlock::default();
        }
        let rows: Vec<(String, bool)> = if bidi::needs_bidi(text) {
            let block_rtl = bidi::paragraph_is_rtl(text);
            text.split('\n')
                .flat_map(|para| {
                    let rtl = paragraph_rtl(para, block_rtl);
                    wrap(para).into_iter().map(move |line| (bidi::visual_line(line, rtl), rtl))
                })
                .collect()
        } else {
            text.split('\n').flat_map(|para| wrap(para).into_iter().map(|line| (line.to_owned(), false))).collect()
        };
        self.build(text, rows, false, style)
    }

    /// One galley per `(visual line, rtl)` row.
    fn build(&self, text: &str, rows: Vec<(String, bool)>, truncated: bool, style: &TextStyle) -> TextBlock {
        let mut lines = Vec::with_capacity(rows.len());
        let mut width = 0.0f32;
        for (k, (visual, rtl)) in rows.into_iter().enumerate() {
            let galley = self.ctx.fonts_mut(|f| f.layout_job(LayoutJob::single_section(visual, style.format())));
            let w = galley.rect.width();
            width = width.max(w);
            let top = k as f32 * style.line_pitch;
            let box_top = if style.round_line_tops { top.round() } else { top };
            let half_leading = (style.line_pitch - galley.rect.height()) / 2.0;
            lines.push(TextLine { offset: Vec2::new(0.0, box_top + half_leading), width: w, box_top, galley, rtl });
        }
        let h = lines.len() as f32 * style.line_pitch;
        let h = if style.round_line_tops { h.round() } else { h };
        TextBlock { lines, size: Vec2::new(width, h), rtl: bidi::paragraph_is_rtl(text), truncated }
    }

    /// Width of one unwrapped line.
    fn line_width(&self, line: &str, style: &TextStyle) -> f32 {
        if line.is_empty() {
            return 0.0;
        }
        let job = LayoutJob::single_section(line.to_owned(), style.format());
        self.ctx.fonts_mut(|f| f.layout_job(job)).rect.width()
    }

    /// Left-to-right text: epaint wraps (and elides). Returns (line text, rtl=false) per row.
    fn wrap_ltr(&self, text: &str, style: &TextStyle, wrap_width: f32, max_lines: Option<usize>) -> (Vec<(String, bool)>, bool) {
        let mut job = LayoutJob::single_section(text.to_owned(), style.format());
        job.wrap.max_width = wrap_width;
        if let Some(n) = max_lines {
            job.wrap.max_rows = n;
            job.wrap.overflow_character = Some(ELLIPSIS);
        }
        self.ctx.fonts_mut(|f| {
                    let g = f.layout_job(job);
                    let rows = g.rows
                                .iter()
                                .map(|r| {
                                    let t = r.row.text();
                                    // A row that ends at a wrap point keeps its breaking space; it
                                    // doesn't count towards the line width.
                                    let t = if r.ends_with_newline { t } else { t.trim_end().to_owned() };
                                    (t, false)
                                })
                                .collect();
                    (rows, g.elided)
                })
    }

    /// Text with right-to-left content: greedy word wrap in logical order, measured in visual
    /// order, then each line reordered. Returns (visual line, paragraph rtl) per line.
    fn wrap_bidi(&self, text: &str, style: &TextStyle, wrap_width: f32, max_lines: Option<usize>) -> (Vec<(String, bool)>, bool) {
        let block_rtl = bidi::paragraph_is_rtl(text);
        let mut out: Vec<(String, bool)> = Vec::new();
        let paragraphs: Vec<&str> = text.split('\n').collect();
        for (pi, para) in paragraphs.iter().enumerate() {
            let rtl = paragraph_rtl(para, block_rtl);
            let fits = |s: &str| self.line_width(&bidi::visual_line(s, rtl), style) <= wrap_width + 0.01;
            let logical = wrap_logical(para, &fits);
            for (li, line) in logical.iter().enumerate() {
                if max_lines.is_some_and(|n| out.len() + 1 == n) {
                    let more = li + 1 < logical.len() || pi + 1 < paragraphs.len();
                    if more {
                        // Cut here: everything that remains goes on this last line, elided.
                        let rest_start = line_offset(para, line);
                        let rest = &para[rest_start..];
                        let cut = elide(rest, &|s: &str| fits(s));
                        out.push((bidi::visual_line(&cut, rtl), rtl));
                        return (out, true);
                    }
                }
                out.push((bidi::visual_line(line, rtl), rtl));
            }
        }
        (out, false)
    }
}

/// Base direction of one `\n` paragraph: its own first strong character, or the block's direction
/// when it has none (blank lines, digits only).
fn paragraph_rtl(para: &str, block_rtl: bool) -> bool {
    if para.chars().any(|c| !c.is_whitespace()) {
        bidi::paragraph_is_rtl(para) || (block_rtl && !has_strong(para))
    } else {
        block_rtl
    }
}

/// Whether `s` has a strong directional character.
fn has_strong(s: &str) -> bool {
    use unicode_bidi::{bidi_class, BidiClass};
    s.chars().any(|c| matches!(bidi_class(c), BidiClass::L | BidiClass::R | BidiClass::AL))
}

/// Byte offset of `line` (a subslice of `para`) inside `para`.
fn line_offset(para: &str, line: &str) -> usize {
    (line.as_ptr() as usize).saturating_sub(para.as_ptr() as usize).min(para.len())
}

/// Greedy word wrap of one paragraph in logical order. Break opportunities are after whitespace;
/// a word wider than the line is broken between characters. Returned lines are subslices of
/// `para` with trailing whitespace removed.
fn wrap_logical<'p>(para: &'p str, fits: &dyn Fn(&str) -> bool) -> Vec<&'p str> {
    if para.is_empty() {
        return vec![para];
    }
    // Word boundaries: positions where a non-space follows a space.
    let mut breaks: Vec<usize> = Vec::new();
    let mut prev_space = false;
    for (i, c) in para.char_indices() {
        if prev_space && !c.is_whitespace() {
            breaks.push(i);
        }
        prev_space = c.is_whitespace();
    }
    breaks.push(para.len());

    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut bi = 0usize;
    while start < para.len() {
        // Extend the line word by word while it fits.
        let mut end = None;
        while bi < breaks.len() {
            let candidate = para[start..breaks[bi]].trim_end();
            if end.is_some() && !fits(candidate) {
                break;
            }
            if end.is_none() && !fits(candidate) {
                // The first word alone doesn't fit: break it between characters.
                let word_end = breaks[bi];
                let mut cut = start;
                for (i, c) in para[start..word_end].char_indices() {
                    let next = start + i + c.len_utf8();
                    if cut > start && !fits(&para[start..next]) {
                        break;
                    }
                    cut = next;
                }
                end = Some(cut);
                if cut >= word_end {
                    bi += 1;
                }
                break;
            }
            end = Some(breaks[bi]);
            bi += 1;
        }
        let e = end.unwrap_or(para.len()).max(start + para[start..].chars().next().map_or(1, char::len_utf8));
        lines.push(para[start..e].trim_end());
        start = e;
        while bi < breaks.len() && breaks[bi] <= start {
            bi += 1;
        }
    }
    lines
}

/// Shorten `text` from the end until `text + …` fits.
fn elide(text: &str, fits: &dyn Fn(&str) -> bool) -> String {
    let trimmed = text.trim_end();
    let mut ends: Vec<usize> = trimmed.char_indices().map(|(i, c)| i + c.len_utf8()).collect();
    while let Some(end) = ends.pop() {
        let candidate = format!("{}{ELLIPSIS}", trimmed[..end].trim_end());
        if fits(&candidate) {
            return candidate;
        }
    }
    ELLIPSIS.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::egui_core::fonts::bundled;

    fn with_ctx(f: impl FnMut(&TextCtx<'_>)) {
        let mut f = f;
        let ctx = egui::Context::default();
        let mut defs = egui::FontDefinitions::empty();
        defs.font_data.insert("r".into(), Arc::new(bundled::ubuntu_regular().font_data()));
        defs.families.insert(FontFamily::Proportional, vec!["r".into()]);
        defs.families.insert(FontFamily::Monospace, vec!["r".into()]);
        ctx.set_fonts(defs);
        let mut full = ctx.run_ui(egui::RawInput::default(), |ui| f(&TextCtx::new(ui.ctx())));
        full.textures_delta.clear();
    }

    #[test]
    fn layout_wraps_and_pitches_lines() {
        let mut out = None;
        with_ctx(|t| {
            let style = TextStyle::new(14.0, FontFamily::Proportional, 16.8);
            let text = "The quick brown fox jumps over the lazy dog, again and again and again.";
            let natural = t.natural_width(text, &style);
            let block = t.layout(text, &style, 150.0, None);
            let cut = t.layout(text, &style, 150.0, Some(2));
            out = Some((natural, block.line_count(), block.size, cut.line_count(), cut.truncated, block.lines[0].width));
        });
        let (natural, lines, size, cut_lines, truncated, first_w) = out.unwrap();
        assert!(natural > 150.0);
        assert!(lines >= 3, "{lines}");
        assert!((size.y - lines as f32 * 16.8).abs() < 1e-3);
        assert!(size.x <= 150.0 + 1.0);
        assert!(first_w <= 150.0);
        assert_eq!(cut_lines, 2);
        assert!(truncated);
    }

    #[test]
    fn wrapped_lines_keep_the_paragraph_direction() {
        let mut out = Vec::new();
        with_ctx(|t| {
            let style = TextStyle::new(14.0, FontFamily::Proportional, 16.8);
            // A Hebrew paragraph whose second line starts with Latin, then an English paragraph
            // whose second line starts with Hebrew.
            let text = "שלום עולם Windows 11 שלום\nHello world שלום עולם";
            // Break after the second word.
            let block = t.layout_wrapped(text, &style, |p| {
                             let cut = p.match_indices(' ').nth(1).map_or(p.len(), |(i, _)| i);
                             vec![&p[..cut], p[cut..].trim_start()]
                         });
            out = block.lines.iter().map(|l| l.rtl).collect();
        });
        // "Windows 11 שלום" stays right-to-left; "שלום עולם" stays left-to-right.
        assert_eq!(out, vec![true, true, false, false]);
    }

    #[test]
    fn explicit_newlines_and_rtl_lines() {
        let mut out = None;
        with_ctx(|t| {
            let style = TextStyle::new(14.0, FontFamily::Proportional, 16.8);
            let ltr = t.layout("one\ntwo\n\nfour", &style, f32::INFINITY, None);
            // Hebrew is not in Ubuntu (tofu), but the layout path still runs and reorders.
            let rtl = t.layout("שלום עולם שלום עולם שלום עולם שלום עולם", &style, 80.0, None);
            let rtl_cut = t.layout("שלום עולם שלום עולם שלום עולם שלום עולם", &style, 80.0, Some(2));
            out = Some((ltr.line_count(), ltr.rtl, rtl.rtl, rtl.line_count(), rtl.lines.iter().all(|l| l.rtl), rtl_cut.line_count(), rtl_cut.truncated));
        });
        let (ltr_lines, ltr_rtl, rtl, rtl_lines, all_rtl, cut_lines, cut) = out.unwrap();
        assert_eq!(ltr_lines, 4);
        assert!(!ltr_rtl);
        assert!(rtl && all_rtl);
        assert!(rtl_lines >= 2, "{rtl_lines}");
        assert_eq!((cut_lines, cut), (2, true));
    }

    #[test]
    fn wrap_logical_breaks_words_and_long_tokens() {
        let fits = |s: &str| s.chars().count() <= 5;
        assert_eq!(wrap_logical("ab cd ef", &fits), vec!["ab cd", "ef"]);
        assert_eq!(wrap_logical("abcdefghij k", &fits), vec!["abcde", "fghij", "k"]);
        assert_eq!(wrap_logical("", &fits), vec![""]);
        assert_eq!(elide("abcdefgh", &fits), "abcd\u{2026}");
    }
}
