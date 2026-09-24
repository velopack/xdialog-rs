//! Colours and metrics of the skia look (skia `theme.rs`).

use egui::Color32;

use crate::backends::egui_core::appearance::AccentSource;
use crate::backends::egui_core::color::{luma601, mix, rgb, scale_rgb};
use crate::backends::egui_core::theme::ThemeEnv;

/// Outer padding, vertical gap between stacked items and icon/text gap (`default_content_margin`).
pub(super) const MARGIN: f32 = 16.0;
/// `main_icon_size`.
pub(super) const ICON_SIZE: f32 = 48.0;
/// `button_panel_height` (footer strip).
pub(super) const FOOTER_H: f32 = 48.0;
/// `button_panel_margin`: button inset from the footer top/bottom and the right edge.
pub(super) const FOOTER_MARGIN: f32 = 7.0;
/// `button_panel_spacing`: gap between buttons.
pub(super) const BUTTON_GAP: f32 = 7.0;
/// `button_text_padding`: horizontal label padding on each side.
pub(super) const BUTTON_PAD_X: f32 = 24.0;
/// Button height (`48 - 2 * 7`).
pub(super) const BUTTON_H: f32 = FOOTER_H - 2.0 * FOOTER_MARGIN;
pub(super) const BUTTON_RADIUS: f32 = 6.0;
pub(super) const BUTTON_BORDER: f32 = 2.0;
/// `PROGRESS_HEIGHT`.
pub(super) const PROGRESS_H: f32 = 6.0;
/// Determinate track/bar corner radius (logical; not a pill).
pub(super) const PROGRESS_RADIUS: f32 = 2.0;
/// Window width bounds (`dialog.rs` MIN_WIDTH / MAX_WIDTH).
pub(super) const MIN_WIDTH: f32 = 350.0;
pub(super) const MAX_WIDTH: f32 = 600.0;
/// Title (main instruction): Ubuntu Bold 18. Body and button labels: Ubuntu Regular 14.
pub(super) const TITLE_SIZE: f32 = 18.0;
pub(super) const BODY_SIZE: f32 = 14.0;
/// cosmic-text line height as a multiple of the font size (`LINE_HEIGHT_SCALE`).
pub(super) const LINE_HEIGHT_SCALE: f32 = 1.2;

/// Button colours of one state (skia `ButtonStyle` minus the constant radius/width).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ButtonLook {
    pub border: Color32,
    pub fill: Color32,
    pub text: Color32,
}

/// Resolved skia theme (`ubuntu_light` / `ubuntu_dark`, plus `apply_accent`).
#[derive(Clone, Debug)]
pub(crate) struct LinuxTokens {
    pub dark: bool,
    /// `color_background` (window).
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

impl LinuxTokens {
    /// skia `get_theme` + `apply_accent`. The accent is honoured only when it comes from the
    /// desktop portal (skia's only source; a no-op off Linux) or a test override.
    pub(crate) fn resolve(env: &ThemeEnv) -> LinuxTokens {
        let dark = env.appearance.dark;
        let mut tk = if dark {
            LinuxTokens { dark,
                          bg: rgb(0x2D2D2D),
                          title_text: rgb(0xFFFFFF),
                          body_text: rgb(0xEEEEEE),
                          progress_bg: rgb(0x4A4A4A),
                          progress_fg: rgb(0x2A7DE3),
                          idle: look(0x5A5A5A, 0x3B3B3B, 0xEEEEEE),
                          hover: look(0x2A7DE3, 0x2A7DE3, 0xFFFFFF),
                          pressed: look(0x1E5FAF, 0x1E5FAF, 0xFFFFFF),
                          focused: look(0x2A7DE3, 0x3B3B3B, 0xEEEEEE) }
        } else {
            LinuxTokens { dark,
                          bg: rgb(0xFAFAFA),
                          title_text: rgb(0x3D3D3D),
                          body_text: rgb(0x3D3D3D),
                          progress_bg: rgb(0xADCEF7),
                          progress_fg: rgb(0x2A7DE3),
                          idle: look(0xC7C7C7, 0xFFFFFF, 0x3D3D3D),
                          hover: look(0x2A7DE3, 0x2A7DE3, 0xFFFFFF),
                          pressed: look(0x1E5FAF, 0x1E5FAF, 0xFFFFFF),
                          focused: look(0x2A7DE3, 0xFFFFFF, 0x3D3D3D) }
        };
        if let Some(accent) = env.appearance.accent.filter(|a| matches!(a.source, AccentSource::Portal | AccentSource::Test)) {
            tk.apply_accent(accent.base);
        }
        tk
    }

    /// skia `apply_accent` (theme.rs:161-179).
    fn apply_accent(&mut self, accent: Color32) {
        let accent = Color32::from_rgb(accent.r(), accent.g(), accent.b());
        let pressed = scale_rgb(accent, 0.75);
        self.progress_fg = accent;
        // blend(accent, bg, 0.65): 35% accent + 65% background.
        self.progress_bg = mix(accent, self.bg, 0.65);
        self.hover = ButtonLook { border: accent, fill: accent, text: contrasting_text(accent) };
        self.pressed = ButtonLook { border: pressed, fill: pressed, text: contrasting_text(pressed) };
        self.focused.border = accent;
    }
}

/// skia `contrasting_text`: Rec. 601 luma > 150 -> `#1A1A1A`, else white.
fn contrasting_text(c: Color32) -> Color32 {
    if luma601(c) > 150.0 {
        rgb(0x1A1A1A)
    } else {
        Color32::WHITE
    }
}

/// skia `clamp_window_width(natural)` then clamped to `MIN_WIDTH..=MAX_WIDTH` (dialog.rs:649-658).
pub(super) fn window_width(natural: f32) -> f32 {
    let w = if natural <= 600.0 {
        300.0
    } else if natural >= 4000.0 {
        600.0
    } else {
        300.0 + (natural - 600.0) / 3400.0 * 300.0
    };
    w.clamp(MIN_WIDTH, MAX_WIDTH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::egui_core::appearance::Appearance;
    use crate::backends::egui_core::theme::Platform;

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
        let env = ThemeEnv { appearance: Appearance::test(false, Some([0xE9, 0x54, 0x20]), None), platform: Platform::Linux };
        let tk = LinuxTokens::resolve(&env);
        assert_eq!(tk.progress_fg, rgb(0xE95420));
        assert_eq!(tk.pressed.fill, Color32::from_rgb(0xAF, 0x3F, 0x18));
        assert_eq!(tk.progress_bg, Color32::from_rgb(((0xE9 as f32) * 0.35 + 250.0 * 0.65).round() as u8,
                                                     ((0x54 as f32) * 0.35 + 250.0 * 0.65).round() as u8,
                                                     ((0x20 as f32) * 0.35 + 250.0 * 0.65).round() as u8));
        assert_eq!(tk.hover.text, Color32::WHITE);
        assert_eq!(tk.focused.fill, rgb(0xFFFFFF));
        let base = LinuxTokens::resolve(&ThemeEnv { appearance: Appearance::default(), platform: Platform::Linux });
        assert_eq!(base.progress_fg, rgb(0x2A7DE3));
    }
}
