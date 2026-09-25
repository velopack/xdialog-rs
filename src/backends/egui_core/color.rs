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
