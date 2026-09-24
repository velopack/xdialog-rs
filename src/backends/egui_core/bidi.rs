//! Bidirectional text pre-pass.
//!
//! epaint 0.36 has no bidi: it splits text into runs by font face, shapes each run with harfrust
//! (which picks RTL for Arabic/Hebrew runs, so a single word comes out right) and then places the
//! runs left to right. A right-to-left phrase therefore renders with its words in reverse order.
//!
//! `visual_line` turns one already-wrapped line (logical order) into a string that epaint, placing
//! things left to right, renders in the correct visual order:
//! - `unicode-bidi` resolves embedding levels and gives the level runs in visual order (rules L1/L2).
//! - Inside a right-to-left run, the text is cut into atoms: maximal sequences of right-to-left
//!   letters with their combining marks (kept in logical order so harfrust still joins and shapes
//!   them as one RTL word), and every other character on its own (spaces, punctuation). The atom
//!   order is reversed and mirrored brackets are swapped (rule L4).
//! - Explicit directional formatting characters are dropped (epaint draws them as nothing anyway).

use unicode_bidi::{bidi_class, BidiClass, Level, ParagraphBidiInfo};

/// Whether `text` contains anything that needs the bidi pre-pass (a right-to-left or Arabic
/// number character, or an explicit embedding/override/isolate).
pub(crate) fn needs_bidi(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(bidi_class(c),
                 BidiClass::R | BidiClass::AL | BidiClass::AN | BidiClass::RLE | BidiClass::RLO | BidiClass::RLI)
    })
}

/// Paragraph base direction (rules P2/P3: the first strong character, ignoring isolates);
/// `false` (left-to-right) when there is none.
pub(crate) fn paragraph_is_rtl(text: &str) -> bool {
    let info = ParagraphBidiInfo::new(text, None);
    info.paragraph_level.is_rtl()
}

/// Reorder one line (logical order, no `\n`) into the visual-order string described in the
/// module docs. `base_rtl` is the paragraph direction the line belongs to.
pub(crate) fn visual_line(line: &str, base_rtl: bool) -> String {
    if line.is_empty() || !needs_bidi(line) && !base_rtl {
        return strip_formatting(line);
    }
    let level = if base_rtl { Level::rtl() } else { Level::ltr() };
    let info = ParagraphBidiInfo::new(line, Some(level));
    let (levels, runs) = info.visual_runs(0..line.len());
    let mut out = String::with_capacity(line.len());
    for run in runs {
        let text = &line[run.clone()];
        if levels[run.start].is_rtl() {
            push_rtl_run(text, &mut out);
        } else {
            out.extend(text.chars().filter(|c| !is_format_char(*c)));
        }
    }
    out
}

fn push_rtl_run(text: &str, out: &mut String) {
    // Atoms as byte ranges, in logical order.
    let mut atoms: Vec<(usize, usize)> = Vec::new();
    let mut word_start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if is_format_char(c) {
            if let Some(s) = word_start.take() {
                atoms.push((s, i));
            }
            continue;
        }
        let class = bidi_class(c);
        let joins = matches!(class, BidiClass::R | BidiClass::AL)
                    || (class == BidiClass::NSM && word_start.is_some());
        if joins {
            if word_start.is_none() {
                word_start = Some(i);
            }
        } else {
            if let Some(s) = word_start.take() {
                atoms.push((s, i));
            }
            atoms.push((i, i + c.len_utf8()));
        }
    }
    if let Some(s) = word_start {
        atoms.push((s, text.len()));
    }
    for &(s, e) in atoms.iter().rev() {
        let atom = &text[s..e];
        let mut chars = atom.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => out.push(mirror(c)),
            _ => out.push_str(atom),
        }
    }
}

fn strip_formatting(s: &str) -> String {
    s.chars().filter(|c| !is_format_char(*c)).collect()
}

/// Explicit bidi formatting characters and marks (invisible; removed from the visual string).
fn is_format_char(c: char) -> bool {
    matches!(c, '\u{200E}' | '\u{200F}' | '\u{061C}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Bidi_Mirroring_Glyph for the common paired punctuation (rule L4).
fn mirror(c: char) -> char {
    const PAIRS: &[(char, char)] = &[('(', ')'),
                                     ('[', ']'),
                                     ('{', '}'),
                                     ('<', '>'),
                                     ('«', '»'),
                                     ('‹', '›'),
                                     ('⁅', '⁆'),
                                     ('⁽', '⁾'),
                                     ('₍', '₎'),
                                     ('≤', '≥'),
                                     ('≪', '≫'),
                                     ('⟨', '⟩'),
                                     ('〈', '〉'),
                                     ('《', '》'),
                                     ('「', '」'),
                                     ('『', '』'),
                                     ('【', '】'),
                                     ('〔', '〕'),
                                     ('（', '）'),
                                     ('［', '］'),
                                     ('｛', '｝'),
                                     ('＜', '＞')];
    for &(a, b) in PAIRS {
        if c == a {
            return b;
        }
        if c == b {
            return a;
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ltr_text_is_untouched() {
        assert!(!needs_bidi("Hello, world (1)"));
        assert_eq!(visual_line("Hello, world (1)", false), "Hello, world (1)");
        assert!(!paragraph_is_rtl("Hello"));
    }

    #[test]
    fn hebrew_words_reverse_but_stay_whole() {
        // "shalom olam" (two Hebrew words): words swap, letters stay in logical order for harfrust.
        let line = "שלום עולם";
        assert!(needs_bidi(line));
        assert!(paragraph_is_rtl(line));
        assert_eq!(visual_line(line, true), "עולם שלום");
    }

    #[test]
    fn mixed_hebrew_english_line() {
        // LTR paragraph with an embedded Hebrew phrase: only the phrase is reordered.
        let line = "Open שלום עולם now";
        assert_eq!(visual_line(line, false), "Open עולם שלום now");
        // RTL paragraph with an embedded English word: the English run keeps its order, runs flip.
        let line = "שלום Hello עולם";
        assert_eq!(visual_line(line, true), "עולם Hello שלום");
    }

    #[test]
    fn brackets_mirror_and_numbers_stay_ltr() {
        let line = "(שלום) 123";
        // RTL paragraph: "123" is a separate (even-level) run placed at the visual left.
        assert_eq!(visual_line(line, true), "123 (שלום)");
        // Arabic with a combining mark stays one atom.
        let ar = "مَرحبا بالعالم";
        assert_eq!(visual_line(ar, true), "بالعالم مَرحبا");
    }
}
