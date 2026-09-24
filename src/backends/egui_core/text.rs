//! Text layout for themes: one egui galley per text block.
//!
//! A block is a single `LayoutJob` (font and line height from [`TextStyle`]; epaint puts the extra
//! leading below the glyphs) that epaint wraps, elides and places. Text that needs bidi (see
//! `bidi.rs`) is wrapped here in logical order at word boundaries (measuring each candidate line in
//! visual order), every line is reordered into visual order, and the lines are joined into one job
//! with one section per line; lines of right-to-left paragraphs are right-aligned within the block
//! (section leading space).
//!
//! egui caches galleys by layout job, so repeated layouts of the same strings are cheap.

use std::sync::Arc;

use egui::epaint::text::{LayoutJob, TextFormat};
use egui::{Color32, FontFamily, FontId, Galley, Painter, Pos2, Response, Sense, Ui, Vec2, Widget};

use super::bidi;

const ELLIPSIS: char = '\u{2026}';

/// How to lay out a run of text.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextStyle {
    /// Font size in logical px.
    pub size: f32,
    /// Font family, e.g. `Proportional` or [`super::fonts::bold_family`].
    pub family: FontFamily,
    /// Height of one line in logical px.
    pub line_height: f32,
}

impl TextStyle {
    /// The theme's regular face (`Proportional`).
    pub(crate) fn regular(size: f32, line_height: f32) -> Self {
        TextStyle { size, family: FontFamily::Proportional, line_height }
    }

    /// The theme's bold face ([`super::fonts::bold_family`]).
    pub(crate) fn bold(size: f32, line_height: f32) -> Self {
        TextStyle { size, family: super::fonts::bold_family(), line_height }
    }

    fn format(&self) -> TextFormat {
        TextFormat { font_id: FontId::new(self.size, self.family.clone()),
                     color: Color32::PLACEHOLDER,
                     line_height: Some(self.line_height),
                     ..Default::default() }
    }

    fn job(&self, text: String) -> LayoutJob {
        LayoutJob::single_section(text, self.format())
    }
}

/// A laid-out paragraph: one galley, laid out with `Color32::PLACEHOLDER` (colour chosen at paint).
#[derive(Clone, Debug)]
pub(crate) struct TextBlock {
    pub galley: Arc<Galley>,
    /// `x` = widest line (trailing whitespace excluded), `y` = total line height. Empty text gives
    /// `Vec2::ZERO`.
    pub size: Vec2,
    /// The text starts right-to-left (the block is right-aligned by [`TextBlockWidget`]).
    rtl: bool,
}

impl TextBlock {
    pub(crate) fn is_empty(&self) -> bool {
        self.size == Vec2::ZERO
    }

    /// The text starts right-to-left.
    pub(crate) fn is_rtl(&self) -> bool {
        self.rtl
    }

    /// Paint the block with its top-left corner at `top_left` in `color`.
    pub(crate) fn paint(&self, painter: &Painter, top_left: Pos2, color: Color32) {
        painter.galley(top_left - self.galley.rect.min.to_vec2(), self.galley.clone(), color);
    }
}

/// A [`TextBlock`] as an `egui::Widget`: allocates `width` (default: the block width) x the block
/// height and paints the block there, right-aligned when it is right-to-left.
pub(crate) struct TextBlockWidget<'a> {
    pub block: &'a TextBlock,
    pub color: Color32,
    pub width: Option<f32>,
}

impl Widget for TextBlockWidget<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let size = Vec2::new(self.width.unwrap_or(self.block.size.x), self.block.size.y);
        let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
        if ui.is_rect_visible(rect) {
            let x = if self.block.is_rtl() { rect.right() - self.block.size.x } else { rect.left() };
            self.block.paint(ui.painter(), Pos2::new(x, rect.top()), self.color);
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
        self.layout(text, style, f32::INFINITY, None).size.x
    }

    /// Wrap `text` at `wrap_width` (logical px; `f32::INFINITY` = no wrapping) into at most
    /// `max_lines` lines (ellipsis on the last one when cut).
    pub(crate) fn layout(&self, text: &str, style: &TextStyle, wrap_width: f32, max_lines: Option<usize>) -> TextBlock {
        let max_lines = max_lines.map(|n| n.max(1));
        let needs_bidi = bidi::needs_bidi(text);
        let job = if needs_bidi {
            let lines = self.wrap_bidi(text, style, wrap_width, max_lines);
            let widths: Vec<f32> = lines.iter().map(|(line, _)| self.line_width(line, style)).collect();
            let block_w = widths.iter().copied().fold(0.0, f32::max);
            let mut job = LayoutJob::default();
            for (i, ((line, rtl), w)) in lines.iter().zip(widths).enumerate() {
                let line = if i + 1 < lines.len() { format!("{line}\n") } else { line.clone() };
                job.append(&line, if *rtl { block_w - w } else { 0.0 }, style.format());
            }
            job
        } else {
            let mut job = style.job(text.to_owned());
            job.wrap.max_width = wrap_width;
            if let Some(n) = max_lines {
                job.wrap.max_rows = n;
                job.wrap.overflow_character = Some(ELLIPSIS);
            }
            job
        };
        let galley = self.ctx.fonts_mut(|f| f.layout_job(job));
        let size = if text.is_empty() { Vec2::ZERO } else { Vec2::new(text_width(&galley), galley.rect.height()) };
        TextBlock { galley, size, rtl: needs_bidi && bidi::paragraph_is_rtl(text) }
    }

    /// Width of one unwrapped line.
    fn line_width(&self, line: &str, style: &TextStyle) -> f32 {
        if line.is_empty() {
            return 0.0;
        }
        self.ctx.fonts_mut(|f| f.layout_job(style.job(line.to_owned()))).rect.width()
    }

    /// Text with right-to-left content: greedy word wrap in logical order, measured in visual
    /// order, then each line reordered. Returns the visual lines, each with whether its paragraph
    /// is right-to-left.
    fn wrap_bidi(&self, text: &str, style: &TextStyle, wrap_width: f32, max_lines: Option<usize>) -> Vec<(String, bool)> {
        let block_rtl = bidi::paragraph_is_rtl(text);
        let mut out: Vec<(String, bool)> = Vec::new();
        let paragraphs: Vec<&str> = text.split('\n').collect();
        for (pi, para) in paragraphs.iter().enumerate() {
            let rtl = paragraph_rtl(para, block_rtl);
            let fits = |s: &str| self.line_width(&bidi::visual_line(s, rtl), style) <= wrap_width + 0.01;
            let logical = wrap_logical(para, &fits);
            for (li, line) in logical.iter().enumerate() {
                if max_lines.is_some_and(|n| out.len() + 1 == n) && (li + 1 < logical.len() || pi + 1 < paragraphs.len()) {
                    // Cut here: everything that remains goes on this last line, elided.
                    let rest = &para[line_offset(para, line)..];
                    out.push((bidi::visual_line(&elide(rest, &fits), rtl), rtl));
                    return out;
                }
                out.push((bidi::visual_line(line, rtl), rtl));
            }
        }
        out
    }
}

/// Width of the widest row of `galley` without its trailing whitespace (a wrapped row keeps the
/// space it broke at).
fn text_width(galley: &Galley) -> f32 {
    galley.rows
          .iter()
          .map(|r| r.row.glyphs.iter().rev().find(|g| !g.chr.is_whitespace()).map_or(0.0, |g| r.pos.x + g.max_x()))
          .fold(0.0, f32::max)
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
    fn layout_wraps_and_elides() {
        let mut out = None;
        with_ctx(|t| {
            let style = TextStyle::regular(14.0, 17.0);
            let text = "The quick brown fox jumps over the lazy dog, again and again and again.";
            let natural = t.natural_width(text, &style);
            let block = t.layout(text, &style, 150.0, None);
            let cut = t.layout(text, &style, 150.0, Some(2));
            out = Some((natural, block.galley.rows.len(), block.size, cut.galley.rows.len(), cut.galley.elided));
        });
        let (natural, lines, size, cut_lines, elided) = out.unwrap();
        assert!(natural > 150.0);
        assert!(lines >= 3, "{lines}");
        assert!((size.y - lines as f32 * 17.0).abs() < 1e-3, "{size:?}");
        assert!(size.x <= 150.0);
        assert_eq!(cut_lines, 2);
        assert!(elided);
    }

    #[test]
    fn explicit_newlines_and_rtl_lines() {
        let mut out = None;
        with_ctx(|t| {
            let style = TextStyle::regular(14.0, 17.0);
            let ltr = t.layout("one\ntwo\n\nfour", &style, f32::INFINITY, None);
            // Hebrew is not in Ubuntu (tofu), but the layout path still runs and reorders.
            let text = "שלום עולם שלום עולם שלום עולם שלום עולם";
            let rtl = t.layout(text, &style, 80.0, None);
            let rtl_cut = t.layout(text, &style, 80.0, Some(2));
            out = Some((ltr.galley.rows.len(),
                        ltr.is_rtl(),
                        rtl.is_rtl(),
                        rtl.galley.rows.len(),
                        rtl.size.x,
                        rtl_cut.galley.rows.len(),
                        rtl_cut.galley.text().contains(ELLIPSIS)));
        });
        let (ltr_lines, ltr_rtl, rtl, rtl_lines, rtl_w, cut_lines, cut) = out.unwrap();
        assert_eq!(ltr_lines, 4);
        assert!(!ltr_rtl);
        assert!(rtl);
        assert!(rtl_lines >= 2 && rtl_w <= 80.0 + 0.5, "{rtl_lines} {rtl_w}");
        assert_eq!((cut_lines, cut), (2, true));
    }

    /// Each paragraph is aligned by its own direction: an RTL paragraph after an LTR one is
    /// right-aligned within the block, the LTR one stays left.
    #[test]
    fn mixed_paragraphs_align_by_their_own_direction() {
        let mut out = None;
        with_ctx(|t| {
            let style = TextStyle::regular(14.0, 17.0);
            let block = t.layout("This is an English paragraph.\n\u{05e9}\u{05dc}\u{05d5}\u{05dd}", &style, f32::INFINITY, None);
            let rows: Vec<(f32, f32)> = block.galley.rows.iter().map(|r| (r.pos.x + r.row.glyphs[0].pos.x, r.pos.x + r.row.glyphs.last().unwrap().max_x())).collect();
            out = Some((block.is_rtl(), block.size.x, rows));
        });
        let (rtl, w, rows) = out.unwrap();
        assert!(!rtl);
        assert!(rows[0].0.abs() < 0.5, "{rows:?}");
        assert!(rows[1].0 > 10.0 && (rows[1].1 - w).abs() < 0.5, "{rows:?} {w}");
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
