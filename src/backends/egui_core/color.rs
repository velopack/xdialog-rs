//! Colour helpers shared by the themes (egui `Color32`; compositing and interpolation use
//! `Color32::blend` / `lerp_to_gamma`).

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse() {
        assert_eq!(parse_hex("#2A7DE3"), Some(rgb(0x2A7DE3)));
        assert_eq!(parse_hex("80FFFFFF"), Some(argb(0x80FFFFFF)));
        assert_eq!(parse_hex("zz"), None);
    }
}
