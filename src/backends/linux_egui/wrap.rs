//! cosmic-text style word wrapping (the skia backend used cosmic-text `Wrap::WordOrGlyph`).
//!
//! epaint's own wrapping counts the space after a word against the wrap width, so a line whose
//! words fit exactly (e.g. `long_instruction`: 258 px of text in a 259 px column) wraps one word
//! earlier than cosmic-text, where trailing whitespace hangs past the edge. Here lines are built
//! greedily from break opportunities, each candidate measured WITHOUT its trailing whitespace,
//! and a word wider than the column is broken between characters. The result is handed to
//! `TextCtx::layout_wrapped` as explicit lines (each keeping its paragraph's base direction), so
//! painting never re-wraps.
//!
//! Tabs advance to the next multiple of 8 space advances from the start of the paragraph
//! (cosmic-text `tab_width` 8, measured before wrapping); epaint's own tab is a fixed, narrower
//! advance. Left-to-right text with tabs is therefore measured and laid out here, each tab
//! becoming the leading space of the following run.

use egui::epaint::text::{LayoutJob, TextFormat};
use egui::{Color32, Vec2};

use crate::backends::egui_core::bidi;
use crate::backends::egui_core::text::{TextBlock, TextCtx, TextLine, TextStyle};

/// cosmic-text `Buffer::tab_width` default.
const TAB_WIDTH: f32 = 8.0;

/// Lay out `text` wrapped at `width` the way cosmic-text did.
pub(super) fn layout(ctx: &egui::Context, text: &TextCtx<'_>, s: &str, style: &TextStyle, width: f32) -> TextBlock {
    if s.is_empty() {
        return TextBlock::default();
    }
    if !has_tabs(s) {
        let fits = |line: &str| text.natural_width(line.trim_end(), style) <= width + 1e-3;
        // Laid out per paragraph so wrapped lines keep the paragraph's base direction.
        return text.layout_wrapped(s, style, |p| wrap_paragraph(p, &fits));
    }
    let mut lines = Vec::new();
    let mut block_w = 0.0f32;
    for para in s.split('\n') {
        let tabs = Tabs::new(text, para, style);
        let fits = |line: &str| tabs.width(text, line.trim_end(), style) <= width + 1e-3;
        for line in wrap_paragraph(para, &fits) {
            let galley = ctx.fonts_mut(|f| f.layout_job(tabs.job(line, style)));
            let w = galley.rect.width();
            block_w = block_w.max(w);
            let box_top = lines.len() as f32 * style.line_pitch;
            let half_leading = (style.line_pitch - galley.rect.height()) / 2.0;
            lines.push(TextLine { offset: Vec2::new(0.0, box_top + half_leading), width: w, box_top, galley, rtl: false });
        }
    }
    let size = Vec2::new(block_w, lines.len() as f32 * style.line_pitch);
    TextBlock { lines, size, rtl: false, truncated: false }
}

/// Unwrapped width of `s` (widest `\n` paragraph), with cosmic-text tab stops.
pub(super) fn natural_width(text: &TextCtx<'_>, s: &str, style: &TextStyle) -> f32 {
    if !has_tabs(s) {
        return text.natural_width(s, style);
    }
    s.split('\n').map(|p| Tabs::new(text, p, style).width(text, p, style)).fold(0.0, f32::max)
}

/// Text with tabs that this module lays out itself (bidi text keeps the core path).
fn has_tabs(s: &str) -> bool {
    s.contains('\t') && !bidi::needs_bidi(s)
}

/// The tab advances of one paragraph: `(byte offset of the tab, advance)`.
struct Tabs<'p> {
    para: &'p str,
    stops: Vec<(usize, f32)>,
}

impl<'p> Tabs<'p> {
    fn new(text: &TextCtx<'_>, para: &'p str, style: &TextStyle) -> Self {
        let stop = TAB_WIDTH * space_advance(style);
        let (mut x, mut last, mut stops) = (0.0f32, 0usize, Vec::new());
        for (i, _) in para.match_indices('\t') {
            x += text.natural_width(&para[last..i], style);
            let next = if stop > 0.0 { ((x / stop).floor() + 1.0) * stop } else { x };
            stops.push((i, next - x));
            x = next;
            last = i + 1;
        }
        Tabs { para, stops }
    }

    /// Runs of `line` (a subslice of the paragraph) between tabs, each with the advance of the
    /// tabs before it.
    fn runs<'l>(&self, line: &'l str) -> Vec<(f32, &'l str)> {
        let base = (line.as_ptr() as usize).saturating_sub(self.para.as_ptr() as usize);
        let mut out = Vec::new();
        let (mut lead, mut last) = (0.0f32, 0usize);
        for (i, _) in line.match_indices('\t') {
            if i > last {
                out.push((lead, &line[last..i]));
                lead = 0.0;
            }
            lead += self.stops.iter().find(|s| s.0 == base + i).map_or(0.0, |s| s.1);
            last = i + 1;
        }
        out.push((lead, &line[last..]));
        out
    }

    fn width(&self, text: &TextCtx<'_>, line: &str, style: &TextStyle) -> f32 {
        self.runs(line).into_iter().map(|(lead, run)| lead + text.natural_width(run, style)).sum()
    }

    fn job(&self, line: &str, style: &TextStyle) -> LayoutJob {
        let format = TextFormat { font_id: style.font_id(), color: Color32::PLACEHOLDER, coords: style.coords.clone(), ..Default::default() };
        let mut job = LayoutJob::default();
        for (lead, run) in self.runs(line) {
            job.append(run, lead, format.clone());
        }
        job
    }
}

/// Unhinted advance of a space, the glyph cosmic-text shapes a tab as (Ubuntu `hmtx`: 231 / 240
/// units per 1000 in Regular / Bold). epaint's hinted space rounds to 3 px at 14 px, which would
/// put the stops 2 px early.
fn space_advance(style: &TextStyle) -> f32 {
    style.size * if style.family == super::bold() { 0.240 } else { 0.231 }
}

/// Greedy wrap of one paragraph. Returned lines are subslices with trailing whitespace removed.
fn wrap_paragraph<'p>(para: &'p str, fits: &dyn Fn(&str) -> bool) -> Vec<&'p str> {
    if para.trim_end().is_empty() || fits(para) {
        return vec![para.trim_end()];
    }
    let breaks = break_opportunities(para);
    let mut lines = Vec::new();
    let mut start = 0usize;
    while start < para.len() {
        // Longest prefix ending at a break opportunity that fits.
        let mut end = None;
        for &b in breaks.iter().filter(|&&b| b > start) {
            if fits(&para[start..b]) {
                end = Some(b);
            } else {
                break;
            }
        }
        let end = match end {
            Some(e) => e,
            None => {
                // The first word alone is too wide: break it between characters (at least one).
                let word_end = breaks.iter().copied().find(|&b| b > start).unwrap_or(para.len());
                let mut cut = start;
                for (i, c) in para[start..word_end].char_indices() {
                    let next = start + i + c.len_utf8();
                    if cut > start && !fits(&para[start..next]) {
                        break;
                    }
                    cut = next;
                }
                cut
            }
        };
        lines.push(para[start..end].trim_end());
        start = end;
        // Whitespace at the start of a continuation line hangs on the previous line.
        while let Some(c) = para[start..].chars().next() {
            if c == ' ' || c == '\t' {
                start += 1;
            } else {
                break;
            }
        }
    }
    lines
}

/// Byte offsets where a line may end, following cosmic-text's `unicode-linebreak` (UAX #14) for
/// the cases that occur in dialog text: after whitespace runs (before the next word); after a
/// hyphen or slash unless a digit follows (`state-|of-|the-|art`, `https://|example.com/|path`);
/// after other break-after punctuation (`|`, `!`, `?`, `}`, `…`, U+2010/2012/2013) and on both
/// sides of an em dash; after closing/infix punctuation only before an opening bracket; around
/// ideographic characters (class ID, simplified); and at the end of the paragraph.
fn break_opportunities(para: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut prev: Option<char> = None;
    for (i, c) in para.char_indices() {
        if let Some(p) = prev {
            let after_space = p.is_whitespace() && !c.is_whitespace();
            let joined = !p.is_whitespace() && !c.is_whitespace() && !no_break_before(c) && !no_break_after(p);
            let ideographic = joined && (is_ideographic(p) || is_ideographic(c));
            let punct = joined
                        && match p {
                            // HY, SY (`HY × NU`, `SY × NU`: `4-2`, `1/2` stay whole).
                            '-' | '/' => !c.is_ascii_digit(),
                            // BA, EX, CL, IN; B2 (em dash) breaks on both sides.
                            '|' | '!' | '?' | '}' | '\u{2026}' | '\u{2010}' | '\u{2012}' | '\u{2013}' | '\u{2014}' => true,
                            // IS, CP: only before an opening bracket.
                            ',' | '.' | ':' | ';' | ')' | ']' => matches!(c, '(' | '[' | '{'),
                            _ => c == '\u{2014}',
                        };
            if after_space || ideographic || punct {
                out.push(i);
            }
        }
        prev = Some(c);
    }
    out.push(para.len());
    out
}

fn is_ideographic(c: char) -> bool {
    matches!(c as u32,
             0x2E80..=0x2FFF | 0x3040..=0x30FF | 0x3100..=0x31BF | 0x31F0..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFF60 | 0x20000..=0x3FFFF)
}

/// No break before: closing punctuation / small kana / prolonged sound mark (UAX #14 CL, CP, NS,
/// CJ), infix and exclamation marks (IS, EX), hyphens and break-after marks (HY, BA), the slash
/// (SY), quotes (QU) and the ellipsis (IN).
fn no_break_before(c: char) -> bool {
    "、。，．：；！？）〕］｝〉》」』】〙〗〟ー々〻ぁぃぅぇぉっゃゅょゎゕゖァィゥェォッャュョヮヵヶ・゛゜ヽヾゝゞ".contains(c)
    || is_quote(c)
    || matches!(c, ')' | ']' | '}' | ',' | '.' | '!' | '?' | ':' | ';' | '-' | '/' | '|' | '\u{2026}' | '\u{2010}' | '\u{2012}' | '\u{2013}')
}

/// UAX #14 QU: no break on either side.
fn is_quote(c: char) -> bool {
    matches!(c, '"' | '\'' | '\u{2018}' | '\u{2019}' | '\u{201C}' | '\u{201D}' | '\u{00AB}' | '\u{00BB}')
}

/// Opening punctuation (UAX #14 OP) and quotes (QU): no break after.
fn no_break_after(c: char) -> bool {
    "（〔［｛〈《「『【〘〖〝".contains(c) || is_quote(c) || matches!(c, '(' | '[' | '{')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_space_hangs() {
        let fits = |s: &str| s.trim_end().chars().count() <= 5;
        assert_eq!(wrap_paragraph("abcde fg", &fits), vec!["abcde", "fg"]);
        assert_eq!(wrap_paragraph("ab cd ef", &fits), vec!["ab cd", "ef"]);
        assert_eq!(wrap_paragraph("abcdefghij k", &fits), vec!["abcde", "fghij", "k"]);
        assert_eq!(wrap_paragraph("", &fits), vec![""]);
        assert_eq!(wrap_paragraph("日本語のテキストです", &fits), vec!["日本語のテ", "キストです"]);
        // A hyphenated word breaks after its hyphens; a line never starts with the hyphen.
        assert_eq!(wrap_paragraph("ab well-known", &fits), vec!["ab", "well-", "known"]);
    }

    fn pieces(s: &str) -> Vec<&str> {
        let mut last = 0;
        break_opportunities(s).into_iter()
                              .map(|b| {
                                  let p = &s[last..b];
                                  last = b;
                                  p
                              })
                              .collect()
    }

    /// The segments `unicode-linebreak` 0.1.5 (cosmic-text 0.19, the skia backend) produces.
    #[test]
    fn uax14_breaks() {
        assert_eq!(pieces("A self-contained, state-of-the-art example-with."),
                   vec!["A ", "self-", "contained, ", "state-", "of-", "the-", "art ", "example-", "with."]);
        assert_eq!(pieces("Visit https://example.com/downloads/release-notes/version-4.0.0.html for"),
                   vec!["Visit ", "https://", "example.com/", "downloads/", "release-", "notes/", "version-4.0.0.html ", "for"]);
        assert_eq!(pieces("a|b a!b a?b a}b a)b a]b a,b a.b a:b a;b a\"b a'b"),
                   vec!["a|", "b ", "a!", "b ", "a?", "b ", "a}", "b ", "a)b ", "a]b ", "a,b ", "a.b ", "a:b ", "a;b ", "a\"b ", "a'b"]);
        assert_eq!(pieces("1/2 4-2 a-(b) a,(b a(b a\u{2014}b a\u{2013}b a\u{2026}b"),
                   vec!["1/2 ", "4-2 ", "a-", "(b) ", "a,", "(b ", "a(b ", "a", "\u{2014}", "b ", "a\u{2013}", "b ", "a\u{2026}", "b"]);
        assert_eq!(pieces("a--b a//b a-/b a/-b a-\"b"), vec!["a--", "b ", "a//", "b ", "a-/", "b ", "a/-", "b ", "a-\"b"]);
        assert_eq!(pieces("Tabs\there and  double"), vec!["Tabs\t", "here ", "and  ", "double"]);
    }
}

