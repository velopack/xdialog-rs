//! Colour helpers shared by the themes. All inputs/outputs are egui `Color32` (which stores
//! premultiplied alpha); the helpers work on straight (unmultiplied) sRGB(A) channels.

use egui::Color32;

/// Opaque colour from `0xRRGGBB`.
pub(crate) const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Colour from `0xAARRGGBB` with straight alpha (the notation of WinUI resource tables).
pub(crate) fn argb(hex: u32) -> Color32 {
    Color32::from_rgba_unmultiplied((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, (hex >> 24) as u8)
}

/// Parse `RRGGBB`, `#RRGGBB` or `#AARRGGBB` (straight alpha).
pub(crate) fn parse_hex(s: &str) -> Option<Color32> {
    let h = s.trim().trim_start_matches('#');
    if !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match h.len() {
        6 => u32::from_str_radix(h, 16).ok().map(rgb),
        8 => u32::from_str_radix(h, 16).ok().map(argb),
        _ => None,
    }
}

/// Straight-alpha channels `[r, g, b, a]`.
pub(crate) fn unmultiplied(c: Color32) -> [u8; 4] {
    c.to_srgba_unmultiplied()
}

/// `fg` (straight alpha) composited over `bg` in sRGB space (as DWM/XAML/GTK composite). The
/// result has `bg`'s alpha when `bg` is opaque (the usual case).
pub(crate) fn over(fg: Color32, bg: Color32) -> Color32 {
    let [fr, fg_, fb, fa] = unmultiplied(fg);
    let [br, bg_, bb, ba] = unmultiplied(bg);
    let a = fa as f32 / 255.0;
    let ch = |f: u8, b: u8| (f as f32 * a + b as f32 * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
    let out_a = (fa as f32 + ba as f32 * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(ch(fr, br), ch(fg_, bg_), ch(fb, bb), out_a)
}

/// Linear interpolation per straight-alpha sRGB(A) channel: `a*(1-t) + b*t`, rounded.
/// `mix(a, b, 0.35)` = 65% `a` + 35% `b`.
pub(crate) fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let [ar, ag, ab, aa] = unmultiplied(a);
    let [br, bg, bb, ba] = unmultiplied(b);
    let ch = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(ch(ar, br), ch(ag, bg), ch(ab, bb), ch(aa, ba))
}

/// Same RGB with straight alpha `alpha` (0..=1).
pub(crate) fn with_alpha(c: Color32, alpha: f32) -> Color32 {
    let [r, g, b, _] = unmultiplied(c);
    Color32::from_rgba_unmultiplied(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Multiply each RGB channel by `factor`, rounded (skia `darken`).
pub(crate) fn scale_rgb(c: Color32, factor: f32) -> Color32 {
    let [r, g, b, a] = unmultiplied(c);
    let ch = |x: u8| (x as f32 * factor).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(ch(r), ch(g), ch(b), a)
}

/// Rec. 601 luma `0.299R + 0.587G + 0.114B` in 0..=255 (skia `contrasting_text`).
pub(crate) fn luma601(c: Color32) -> f32 {
    let [r, g, b, _] = unmultiplied(c);
    0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn over_composites() {
        // Fluent dark content_bg: #0DFFFFFF over #202020 = #2B2B2B
        assert_eq!(over(argb(0x0DFFFFFF), rgb(0x202020)), rgb(0x2B2B2B));
    }

    #[test]
    fn mix_and_parse() {
        // skia progress track with accent: 35% accent + 65% bg
        let c = mix(rgb(0xFAFAFA), rgb(0x2A7DE3), 0.35);
        assert_eq!(unmultiplied(c)[3], 255);
        assert_eq!(parse_hex("#2A7DE3"), Some(rgb(0x2A7DE3)));
        assert_eq!(parse_hex("zz"), None);
    }
}
