//! macOS alert colour tokens, light and dark, for both styles ([`MacStyle`]).
//!
//! Colours were sampled from Sequoia and Tahoe alerts (CFUserNotification / NSAlert) and match
//! AppKit's semantic colours where one exists (`labelColor`, `controlAccentColor`, `systemYellow`,
//! ...). Tahoe captures are Display P3: the values here are converted to sRGB (greys are the same
//! in both; the default button's P3 #3478F6 is sRGB #007AFF).
//! The alert window is vibrant: on macOS [`MacTokens::bg`] is only the fallback the frame is
//! cleared with when the behind-window material is unavailable. Everything drawn on top of it is
//! translucent, as AppKit's control fills are, so it reads the same over the material.
//!
//! Fonts ([`FONTS`]): the system UI font (SF Pro) on macOS: 13 pt bold title, 13 pt button
//! labels, and an 11 pt (Sequoia) or 13 pt (Tahoe) body. Elsewhere (renders of this look on Windows and Linux) SF Pro when installed,
//! else Segoe UI, else the platform UI font.

use crate::backends::draw::color::{argb, rgb};
use crate::backends::draw::{Color, Weight};
use crate::backends::gui::appearance::{Accent, Appearance};
use crate::backends::gui::text::ThemeFonts;

use super::widgets::{BUTTON_H, CORNER};
use super::MacStyle;

/// `systemBlue` (the default `controlAccentColor`), light and dark.
const BLUE_LIGHT: u32 = 0x007AFF;
const BLUE_DARK: u32 = 0x0A84FF;

#[derive(Clone, Debug)]
pub(crate) struct MacTokens {
    /// Opaque window background (the alert material over an average desktop).
    pub bg: Color,
    /// `labelColor`: title, body and standard button labels.
    pub text: Color,
    /// Default (accent) push button: vertical gradient (flat on Tahoe), under the pointer, and
    /// while pressed.
    pub default_top: Color,
    pub default_bottom: Color,
    pub default_hover_top: Color,
    pub default_hover_bottom: Color,
    pub default_pressed_top: Color,
    pub default_pressed_bottom: Color,
    /// Label on the accent fill (`alternateSelectedControlTextColor`).
    pub default_text: Color,
    /// Push button corner radius: 6 on Sequoia, a capsule (half the 28 height) on Tahoe.
    pub button_radius: f64,
    /// Standard push button fill (translucent), under the pointer, and while pressed.
    pub button: Color,
    pub button_hover: Color,
    pub button_pressed: Color,
    /// `keyboardFocusIndicatorColor`: the focus ring (translucent accent).
    pub focus_ring: Color,
    /// Progress bar track (translucent) and fill (accent).
    pub track: Color,
    pub progress: Color,
    /// Overlay scroller knob.
    pub scroll_thumb: Color,
    /// Fallback severity icons (drawn when the system's icon files are unavailable).
    /// Each shape: an edge, the body (`*_bottom`) and a lighter inner face (`*_top`).
    pub caution_edge: Color,
    pub caution_top: Color,
    pub caution_bottom: Color,
    pub caution_glyph: Color,
    pub stop_edge: Color,
    pub stop_top: Color,
    pub stop_bottom: Color,
    pub note_edge: Color,
    pub note_top: Color,
    pub note_bottom: Color,
    pub icon_glyph: Color,
}

impl MacTokens {
    pub(crate) fn new(style: MacStyle, appearance: &Appearance) -> Self {
        match style {
            MacStyle::Legacy => Self::legacy(appearance),
            MacStyle::Tahoe => Self::tahoe(appearance),
        }
    }

    /// Big Sur to Sequoia.
    fn legacy(appearance: &Appearance) -> Self {
        let dark = appearance.dark;
        let accent = appearance.accent.map_or(rgb(if dark { BLUE_DARK } else { BLUE_LIGHT }), |a| a.base);
        let t = |light: u32, dark_v: u32| argb(if dark { dark_v } else { light });
        let (black, white) = (Color::BLACK, Color::WHITE);
        // The accent button: light, a gradient from the accent lightened 14 % (top) to the accent;
        // dark, the accent dimmed (NSButton's dark bezel), lighter at the top.
        let (top, bottom) = if dark {
            (accent.lerp_to_gamma(black, 0.12).lerp_to_gamma(white, 0.08), accent.lerp_to_gamma(black, 0.2).lerp_to_gamma(white, 0.06))
        } else {
            (accent.lerp_to_gamma(white, 0.14), accent)
        };
        let pressed = if dark { 0.25 } else { 0.18 };
        // Under the pointer: the accent a little lighter (dark) or deeper (light).
        let hover = |c: Color| if dark { c.lerp_to_gamma(white, 0.1) } else { c.lerp_to_gamma(black, 0.08) };
        MacTokens { bg: t(0xFFE3E3E3, 0xFF292929),
                    text: t(0xD9000000, 0xD9FFFFFF),
                    default_top: top,
                    default_bottom: bottom,
                    default_hover_top: hover(top),
                    default_hover_bottom: hover(bottom),
                    default_pressed_top: top.lerp_to_gamma(black, pressed),
                    default_pressed_bottom: bottom.lerp_to_gamma(black, pressed),
                    default_text: white,
                    button_radius: CORNER,
                    button: t(0x1F000000, 0x47FFFFFF),
                    button_hover: t(0x2B000000, 0x59FFFFFF),
                    button_pressed: t(0x38000000, 0x6BFFFFFF),
                    focus_ring: Color::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 0x80),
                    track: t(0x1A000000, 0x26FFFFFF),
                    progress: accent,
                    scroll_thumb: t(0x73000000, 0x80FFFFFF),
                    caution_edge: rgb(0xD69A00),
                    caution_top: rgb(0xFFD83B),
                    caution_bottom: rgb(0xF5B400),
                    caution_glyph: argb(0xE0000000),
                    stop_edge: rgb(0xB8231C),
                    stop_top: rgb(0xFF5F57),
                    stop_bottom: rgb(0xE0342B),
                    note_edge: accent.lerp_to_gamma(black, 0.25),
                    note_top: accent.lerp_to_gamma(white, 0.25),
                    note_bottom: accent,
                    icon_glyph: white }
    }

    /// Tahoe (sampled on macOS 26.6): the default button is the accent, flat, in both modes
    /// (sRGB #007AFF with the blue accent: `controlAccentColor` resolves to the light shade in
    /// dark mode too); standard buttons a flat translucent fill (white 7.5 % dark, black 7.3 %
    /// light: #212121 over #0F0F0F, #A6A6A6 over #B3B3B3); text `labelColor` at 85 %. The glass
    /// is flat but shows the backdrop (#0F0F0F / #B3B3B3 over a dark window): `bg` is an opaque
    /// stand-in over an average desktop.
    fn tahoe(appearance: &Appearance) -> Self {
        let dark = appearance.dark;
        let accent = appearance.accent.map_or(rgb(BLUE_LIGHT), |a| a.base);
        let t = |light: u32, dark_v: u32| argb(if dark { dark_v } else { light });
        let (black, white) = (Color::BLACK, Color::WHITE);
        let hover = if dark { accent.lerp_to_gamma(white, 0.1) } else { accent.lerp_to_gamma(black, 0.08) };
        let pressed = accent.lerp_to_gamma(black, if dark { 0.25 } else { 0.18 });
        // Text, focus ring, scroller and fallback icons as on Sequoia (with this accent).
        let base = Self::legacy(&Appearance { dark, accent: Some(Accent { base: accent, win_palette: None }) });
        MacTokens { bg: t(0xFFEFEFEF, 0xFF1C1C1C),
                    default_top: accent,
                    default_bottom: accent,
                    default_hover_top: hover,
                    default_hover_bottom: hover,
                    default_pressed_top: pressed,
                    default_pressed_bottom: pressed,
                    button_radius: BUTTON_H / 2.0,
                    button: t(0x13000000, 0x13FFFFFF),
                    button_hover: t(0x1F000000, 0x24FFFFFF),
                    button_pressed: t(0x2E000000, 0x38FFFFFF),
                    track: t(0x14000000, 0x1FFFFFFF),
                    ..base }
    }
}

// ------------------------------------------------------------------------------------------------
// Fonts
// ------------------------------------------------------------------------------------------------

/// Title and button label size (NSAlert: bold system font 13 pt). The body size is per style
/// (`Metrics::body_size`): small system font 11 pt on Sequoia, 13 pt on Tahoe.
pub(crate) const TITLE_SIZE: f64 = 13.0;
pub(crate) const BUTTON_SIZE: f64 = 13.0;

/// Line pitch: 16 pt for 13 pt text, 14 pt for 11 pt text (measured on Sequoia and Tahoe).
pub(crate) fn line_height(size: f64) -> f64 {
    size + 3.0
}

/// The system UI font (SF Pro) on macOS; elsewhere SF Pro when installed, else Segoe UI.
#[cfg(target_os = "macos")]
const FAMILIES: &[&str] = &["system-ui"];
#[cfg(not(target_os = "macos"))]
const FAMILIES: &[&str] = &["SF Pro Text", "SF Pro", "Segoe UI Variable Text", "Segoe UI"];

pub(crate) static FONTS: ThemeFonts = ThemeFonts { families: FAMILIES,
                                                   regular: Weight::REGULAR,
                                                   bold: Weight::BOLD,
                                                   line_height: |size| Some(line_height(size)),
                                                   opsz: |_| None };

#[cfg(test)]
mod tests {
    use super::*;

    /// The default accent reproduces the sampled Sequoia button colours (within a few levels).
    #[test]
    fn default_button_matches_sequoia() {
        let close = |c: Color, want: u32| {
            let w = rgb(want);
            let d = [c.r().abs_diff(w.r()), c.g().abs_diff(w.g()), c.b().abs_diff(w.b())];
            assert!(d.iter().all(|&x| x <= 12), "{c:?} vs {w:?}");
        };
        let light = MacTokens::new(MacStyle::Legacy, &Appearance { dark: false, accent: None });
        close(light.default_top, 0x2391FF);
        close(light.default_bottom, 0x007AFF);
        let dark = MacTokens::new(MacStyle::Legacy, &Appearance { dark: true, accent: None });
        close(dark.default_top, 0x2179E2);
        close(dark.default_bottom, 0x1D6BC9);
    }

    /// Tahoe's default button is flat sRGB #007AFF in both modes; standard buttons are 7-8 %
    /// translucent fills (sampled over #0F0F0F / #B3B3B3); a custom accent tints the default.
    #[test]
    fn default_button_matches_tahoe() {
        for dark in [false, true] {
            let tk = MacTokens::new(MacStyle::Tahoe, &Appearance { dark, accent: None });
            assert_eq!((tk.default_top, tk.default_bottom), (rgb(0x007AFF), rgb(0x007AFF)));
            assert_eq!(tk.default_text, Color::WHITE);
            let (over, want) = if dark { (0x0F, 0x21) } else { (0xB3, 0xA6) };
            let ink = if dark { 255.0 } else { 0.0 };
            let a = tk.button.a() as f64 / 255.0;
            let got = over as f64 * (1.0 - a) + ink * a;
            assert!((got - want as f64).abs() <= 1.5, "dark {dark}: {got} vs {want}");
        }
        let purple = Accent { base: rgb(0xA550A7), win_palette: None };
        let tk = MacTokens::new(MacStyle::Tahoe, &Appearance { dark: true, accent: Some(purple) });
        assert_eq!(tk.default_top, rgb(0xA550A7));
    }
}
