//! Design tokens of the Ubuntu theme: metrics (logical px) and the light / dark colours, with the
//! desktop accent colour applied to the progress bar and the hover / pressed / focused buttons.

use egui::Color32;

use crate::backends::egui_core::appearance::Appearance;
use crate::backends::egui_core::color::rgb;

/// Outer padding, vertical gap between stacked items and icon/text gap.
pub(super) const MARGIN: f32 = 16.0;
/// Icon size.
pub(super) const ICON_SIZE: f32 = 48.0;
/// Footer strip height.
pub(super) const FOOTER_H: f32 = 48.0;
/// Button inset from the footer top/bottom and the right edge.
pub(super) const FOOTER_MARGIN: f32 = 7.0;
/// Gap between buttons.
pub(super) const BUTTON_GAP: f32 = 7.0;
/// Horizontal label padding on each side.
pub(super) const BUTTON_PAD_X: f32 = 24.0;
/// Button height (`48 - 2 * 7`).
pub(super) const BUTTON_H: f32 = FOOTER_H - 2.0 * FOOTER_MARGIN;
pub(super) const BUTTON_RADIUS: f32 = 6.0;
pub(super) const BUTTON_BORDER: f32 = 2.0;
/// Progress bar height.
pub(super) const PROGRESS_H: f32 = 6.0;
/// Determinate track/bar corner radius (logical; not a pill).
pub(super) const PROGRESS_RADIUS: f32 = 2.0;
/// Window width bounds.
pub(super) const MIN_WIDTH: f32 = 350.0;
pub(super) const MAX_WIDTH: f32 = 600.0;
/// Title (main instruction): Ubuntu Bold 18. Body and button labels: Ubuntu Regular 14.
pub(super) const TITLE_SIZE: f32 = 18.0;
pub(super) const BODY_SIZE: f32 = 14.0;
/// Line height as a multiple of the font size.
pub(super) const LINE_HEIGHT_SCALE: f32 = 1.2;

/// Button colours of one state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ButtonLook {
    pub border: Color32,
    pub fill: Color32,
    pub text: Color32,
}

/// Resolved colours for one appearance.
#[derive(Clone, Debug)]
pub(crate) struct UbuntuTokens {
    /// Window background.
    pub bg: Color32,
    pub title_text: Color32,
    pub body_text: Color32,
    pub progress_bg: Color32,
    pub progress_fg: Color32,
    pub idle: ButtonLook,
    pub hover: ButtonLook,
    pub pressed: ButtonLook,
    pub focused: ButtonLook,
}

const fn look(border: u32, fill: u32, text: u32) -> ButtonLook {
    ButtonLook { border: rgb(border), fill: rgb(fill), text: rgb(text) }
}

impl UbuntuTokens {
    /// Light or dark palette plus the accent. The accent is honoured only when it comes from the
    /// desktop portal (a no-op off Linux) or a test override.
    pub(crate) fn resolve(appearance: &Appearance) -> UbuntuTokens {
        let mut tk = if appearance.dark {
            UbuntuTokens { bg: rgb(0x2D2D2D),
                          title_text: rgb(0xFFFFFF),
                          body_text: rgb(0xEEEEEE),
                          progress_bg: rgb(0x4A4A4A),
                          progress_fg: rgb(0x2A7DE3),
                          idle: look(0x5A5A5A, 0x3B3B3B, 0xEEEEEE),
                          hover: look(0x2A7DE3, 0x2A7DE3, 0xFFFFFF),
                          pressed: look(0x1E5FAF, 0x1E5FAF, 0xFFFFFF),
                          focused: look(0x2A7DE3, 0x3B3B3B, 0xEEEEEE) }
        } else {
            UbuntuTokens { bg: rgb(0xFAFAFA),
                          title_text: rgb(0x3D3D3D),
                          body_text: rgb(0x3D3D3D),
                          progress_bg: rgb(0xADCEF7),
                          progress_fg: rgb(0x2A7DE3),
                          idle: look(0xC7C7C7, 0xFFFFFF, 0x3D3D3D),
                          hover: look(0x2A7DE3, 0x2A7DE3, 0xFFFFFF),
                          pressed: look(0x1E5FAF, 0x1E5FAF, 0xFFFFFF),
                          focused: look(0x2A7DE3, 0xFFFFFF, 0x3D3D3D) }
        };
        if let Some(accent) = appearance.accent {
            tk.apply_accent(accent.base);
        }
        tk
    }

    /// Accent overlay: progress, hover / pressed buttons and the focus border.
    fn apply_accent(&mut self, accent: Color32) {
        let pressed = accent.lerp_to_gamma(Color32::BLACK, 0.25);
        self.progress_fg = accent;
        // blend(accent, bg, 0.65): 35% accent + 65% background.
        self.progress_bg = accent.lerp_to_gamma(self.bg, 0.65);
        self.hover = ButtonLook { border: accent, fill: accent, text: contrasting_text(accent) };
        self.pressed = ButtonLook { border: pressed, fill: pressed, text: contrasting_text(pressed) };
        self.focused.border = accent;
    }
}

/// Text colour on an accent fill: Rec. 601 luma > 150 -> `#1A1A1A`, else white.
fn contrasting_text(c: Color32) -> Color32 {
    if c.intensity() * 255.0 > 150.0 {
        rgb(0x1A1A1A)
    } else {
        Color32::WHITE
    }
}

/// Window width for the natural text width: 300 up to 600, growing linearly to 600 at 4000,
/// clamped to `MIN_WIDTH..=MAX_WIDTH`.
pub(super) fn window_width(natural: f32) -> f32 {
    (300.0 + (natural - 600.0).max(0.0) * (300.0 / 3400.0)).clamp(MIN_WIDTH, MAX_WIDTH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_rule() {
        assert_eq!(window_width(0.0), 350.0);
        assert_eq!(window_width(1100.0), 350.0);
        assert_eq!(window_width(4000.0), 600.0);
        assert!((window_width(1167.0) - 350.0).abs() < 0.1);
        assert!(window_width(2000.0) > 400.0);
    }

    #[test]
    fn accent_overlay() {
        let tk = UbuntuTokens::resolve(&Appearance::test(false, Some([0xE9, 0x54, 0x20]), None));
        assert_eq!(tk.progress_fg, rgb(0xE95420));
        assert_eq!(tk.pressed.fill, Color32::from_rgb(0xAF, 0x3F, 0x18));
        assert_eq!(tk.progress_bg, Color32::from_rgb(((0xE9 as f32) * 0.35 + 250.0 * 0.65).round() as u8,
                                                     ((0x54 as f32) * 0.35 + 250.0 * 0.65).round() as u8,
                                                     ((0x20 as f32) * 0.35 + 250.0 * 0.65).round() as u8));
        assert_eq!(tk.hover.text, Color32::WHITE);
        assert_eq!(tk.focused.fill, rgb(0xFFFFFF));
        let base = UbuntuTokens::resolve(&Appearance::default());
        assert_eq!(base.progress_fg, rgb(0x2A7DE3));
    }
}
