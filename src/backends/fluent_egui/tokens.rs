//! Fluent (WinUI 3) colour tokens, light and dark.
//!
//! WinUI brushes are mostly translucent and get composited over whatever lies behind them. The
//! surfaces behind every element are fixed here (content area, button bar), so fills and borders
//! are pre-composited into OPAQUE colours (they match the measured reference pixels exactly and
//! interpolate cleanly). Text and focus colours stay translucent: glyph anti-aliasing then blends
//! the same way DirectWrite's grayscale AA does over the surface.

use egui::Color32;

use crate::backends::egui_core::appearance::AccentSource;
use crate::backends::egui_core::color::{argb, mix, over, rgb};
use crate::backends::egui_core::theme::ThemeEnv;

/// Windows 11 default (blue) accent palette `[L3, L2, L1, A, D1, D2, D3]`: the fallback
/// when neither the registry nor a test override provides one (e.g. the fluent theme on Linux).
pub(crate) const DEFAULT_PALETTE: [u32; 7] = [0x99EBFF, 0x4CC2FF, 0x0091F8, 0x0078D4, 0x0067C0, 0x003E92, 0x001A68];

const L2: usize = 1;
const A: usize = 3;
const D1: usize = 4;

/// Colours of one button look (all opaque, pre-composited over the button bar).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ButtonColors {
    pub fill: Color32,
    /// Border on three sides (standard: sides + the non-elevated edge).
    pub stroke: Color32,
    /// The "elevation" edge: bottom (light standard, accent in both themes) or top (dark
    /// standard). Equal to `stroke` for flat borders.
    pub stroke_2: Color32,
    /// No border at all (accent pressed / disabled).
    pub borderless: bool,
    /// Label colour (may be translucent).
    pub text: Color32,
}

#[derive(Clone, Debug)]
pub(crate) struct FluentTokens {
    pub dark: bool,
    /// Top (content) area: LayerFillColorAlt over SolidBackgroundFillColorBase.
    pub content_bg: Color32,
    /// Button bar and window clear colour: SolidBackgroundFillColorBase.
    pub bar_bg: Color32,
    /// 1 px separator at the bottom of the content area (CardStrokeColorDefault over the base).
    pub separator: Color32,
    /// TextFillColorPrimary (translucent in light mode).
    pub text: Color32,
    pub std_rest: ButtonColors,
    pub std_hover: ButtonColors,
    pub std_pressed: ButtonColors,
    pub std_disabled: ButtonColors,
    pub acc_rest: ButtonColors,
    pub acc_hover: ButtonColors,
    pub acc_pressed: ButtonColors,
    pub acc_disabled: ButtonColors,
    /// Standard elevation edge is the top edge (dark) instead of the bottom edge (light).
    pub std_elevation_top: bool,
    /// FocusStrokeColorOuter (2 px) / FocusStrokeColorInner (1 px).
    pub focus_outer: Color32,
    pub focus_inner: Color32,
    /// ProgressBar track (ControlStrongStrokeColorDefault) and indicator (AccentFillColorDefault).
    pub progress_track: Color32,
    pub progress_fill: Color32,
    /// Severity circles (InfoBar icon background glyph) and the symbol colour (TextFillColorInverse).
    pub sev_info: Color32,
    pub sev_warning: Color32,
    pub sev_error: Color32,
    pub sev_glyph: Color32,
    /// Scroll bar thumb (ControlStrongFillColorDefault).
    pub scroll_thumb: Color32,
}

/// The accent palette the fluent theme uses for `env`: the Windows registry or a test override,
/// else the Windows 11 default blue. A test accent without a palette gets a derived one.
pub(crate) fn palette(env: &ThemeEnv) -> [Color32; 7] {
    match env.appearance.accent {
        Some(a) if matches!(a.source, AccentSource::WindowsRegistry | AccentSource::Test) => a.win_palette.unwrap_or_else(|| derive_palette(a.base)),
        _ => DEFAULT_PALETTE.map(rgb),
    }
}

/// Approximate `[L3, L2, L1, A, D1, D2, D3]` from a single accent colour (only used when an
/// accent comes without the Windows palette, i.e. `XDIALOG_TEST_ACCENT` alone).
fn derive_palette(base: Color32) -> [Color32; 7] {
    let (w, b) = (Color32::WHITE, Color32::BLACK);
    [mix(base, w, 0.6), mix(base, w, 0.4), mix(base, w, 0.2), base, mix(base, b, 0.15), mix(base, b, 0.4), mix(base, b, 0.65)]
}

/// An opaque colour drawn with brush `Opacity = a` over `bg` (exact float opacity; an 8-bit
/// alpha would be off by one against the captures).
fn opacity(c: Color32, a: f32, bg: Color32) -> Color32 {
    let ch = |f: u8, b: u8| (f as f32 * a + b as f32 * (1.0 - a)).round() as u8;
    Color32::from_rgb(ch(c.r(), bg.r()), ch(c.g(), bg.g()), ch(c.b(), bg.b()))
}

impl FluentTokens {
    pub(crate) fn new(env: &ThemeEnv) -> Self {
        Self::build(env.appearance.dark, &palette(env))
    }

    fn build(dark: bool, p: &[Color32; 7]) -> Self {
        // WinUI `Common_themeresources_any.xaml`, Light | Dark.
        let t = |light: u32, dark_v: u32| argb(if dark { dark_v } else { light });
        let base = t(0xFFF3F3F3, 0xFF202020); // SolidBackgroundFillColorBase
        let layer_alt = t(0xFFFFFFFF, 0x0DFFFFFF); // LayerFillColorAlt
        let card_stroke = t(0x0F000000, 0x19000000); // CardStrokeColorDefault
        let text = t(0xE4000000, 0xFFFFFFFF); // TextFillColorPrimary
        let text_2 = t(0x9E000000, 0xC5FFFFFF); // TextFillColorSecondary
        let text_dis = t(0x5C000000, 0x5DFFFFFF); // TextFillColorDisabled
        let ctl_fill = t(0xB3FFFFFF, 0x0FFFFFFF); // ControlFillColorDefault
        let ctl_fill_2 = t(0x80F9F9F9, 0x15FFFFFF); // ControlFillColorSecondary
        let ctl_fill_3 = t(0x4DF9F9F9, 0x08FFFFFF); // ControlFillColorTertiary
        let ctl_fill_dis = t(0x4DF9F9F9, 0x0BFFFFFF); // ControlFillColorDisabled
        let ctl_stroke = t(0x0F000000, 0x12FFFFFF); // ControlStrokeColorDefault
        let ctl_stroke_2 = t(0x29000000, 0x18FFFFFF); // ControlStrokeColorSecondary
        let on_acc_stroke = argb(0x14FFFFFF); // ControlStrokeColorOnAccentDefault
        let on_acc_stroke_2 = t(0x66000000, 0x23000000); // ControlStrokeColorOnAccentSecondary
        let acc_dis = t(0x37000000, 0x28FFFFFF); // AccentFillColorDisabled
        let on_acc_text = t(0xFFFFFFFF, 0xFF000000); // TextOnAccentFillColorPrimary
        let on_acc_text_2 = t(0xB3FFFFFF, 0x80000000); // TextOnAccentFillColorSecondary
        let on_acc_text_dis = t(0xFFFFFFFF, 0x87FFFFFF); // TextOnAccentFillColorDisabled
        let strong_stroke = t(0x72000000, 0x8BFFFFFF); // ControlStrongStrokeColorDefault

        // AccentFillColorDefault = Dark1 (light) / Light2 (dark); Secondary/Tertiary = 0.9/0.8
        // brush opacity, composited over the bar.
        let accent = if dark { p[L2] } else { p[D1] };
        let bar = base;
        let stroke_bar = over(ctl_stroke, bar);
        let std = |fill: Color32, elevated: bool, text: Color32| ButtonColors { fill: over(fill, bar),
                                                                                stroke: stroke_bar,
                                                                                stroke_2: if elevated { over(ctl_stroke_2, bar) } else { stroke_bar },
                                                                                borderless: false,
                                                                                text };
        let acc = |fill: Color32, bordered: bool, text: Color32| {
            let fill = over(fill, bar);
            ButtonColors { fill, stroke: over(on_acc_stroke, fill), stroke_2: over(on_acc_stroke_2, fill), borderless: !bordered, text }
        };
        FluentTokens { dark,
                       content_bg: over(layer_alt, base),
                       bar_bg: base,
                       separator: over(card_stroke, base),
                       text,
                       std_rest: std(ctl_fill, true, text),
                       std_hover: std(ctl_fill_2, true, text),
                       std_pressed: std(ctl_fill_3, false, text_2),
                       std_disabled: std(ctl_fill_dis, false, text_dis),
                       acc_rest: acc(accent, true, on_acc_text),
                       acc_hover: acc(opacity(accent, 0.9, bar), true, on_acc_text),
                       acc_pressed: acc(opacity(accent, 0.8, bar), false, on_acc_text_2),
                       acc_disabled: acc(acc_dis, false, on_acc_text_dis),
                       std_elevation_top: dark,
                       focus_outer: t(0xE4000000, 0xFFFFFFFF),
                       focus_inner: t(0xB3FFFFFF, 0xB3000000),
                       progress_track: strong_stroke,
                       progress_fill: accent,
                       // SystemFillColorAttention = SystemAccentColor (light) / Light2 (dark).
                       sev_info: if dark { p[L2] } else { p[A] },
                       sev_warning: t(0xFF9D5D00, 0xFFFCE100),
                       sev_error: t(0xFFC42B1C, 0xFFFF99A4),
                       sev_glyph: t(0xFFFFFFFF, 0xE4000000),
                       scroll_thumb: t(0x72000000, 0x8BFFFFFF) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::egui_core::appearance::Appearance;
    use crate::backends::egui_core::theme::Platform;

    const PURPLE: [[u8; 3]; 7] = [[0xF0, 0xC0, 0xF4], [0xDB, 0x9E, 0xE5], [0xB7, 0x63, 0xCB], [0xA9, 0x4D, 0xC1], [0x8E, 0x3A, 0xA7], [0x69, 0x27, 0x82], [0x40, 0x0E, 0x59]];

    fn tokens(dark: bool) -> FluentTokens {
        FluentTokens::new(&ThemeEnv { appearance: Appearance::test(dark, None, Some(PURPLE)), platform: Platform::current() })
    }

    /// The pre-composited colours equal the pixels measured in the WinUI captures.
    #[test]
    fn composites_match_measured_references() {
        let l = tokens(false);
        assert_eq!(l.content_bg, rgb(0xFFFFFF));
        assert_eq!(l.separator, rgb(0xE5E5E5));
        assert_eq!((l.std_rest.fill, l.std_rest.stroke, l.std_rest.stroke_2), (rgb(0xFBFBFB), rgb(0xE5E5E5), rgb(0xCCCCCC)));
        assert_eq!(l.std_hover.fill, rgb(0xF6F6F6));
        assert_eq!(l.std_pressed.fill, rgb(0xF5F5F5));
        assert_eq!((l.acc_rest.fill, l.acc_rest.stroke, l.acc_rest.stroke_2), (rgb(0x8E3AA7), rgb(0x9749AE), rgb(0x552364)));
        assert_close(l.acc_hover.fill, 0x984DAF);
        assert_close(l.acc_hover.stroke, 0xA05BB5);
        assert_close(l.acc_pressed.fill, 0xA25FB6);
        assert_eq!(l.acc_disabled.fill, rgb(0xBFBFBF));
        let d = tokens(true);
        assert_eq!(d.content_bg, rgb(0x2B2B2B));
        assert_eq!(d.separator, rgb(0x1D1D1D));
        assert_eq!((d.std_rest.fill, d.std_rest.stroke, d.std_rest.stroke_2), (rgb(0x2D2D2D), rgb(0x303030), rgb(0x353535)));
        assert_eq!(d.std_hover.fill, rgb(0x323232));
        assert_eq!(d.std_pressed.fill, rgb(0x272727));
        assert_eq!(d.std_disabled.fill, rgb(0x2A2A2A));
        assert_eq!((d.acc_rest.fill, d.acc_rest.stroke, d.acc_rest.stroke_2), (rgb(0xDB9EE5), rgb(0xDEA6E7), rgb(0xBD88C6)));
        // The compositor's opacity rounding isn't reproducible bit-exactly: within 1.
        assert_close(d.acc_hover.fill, 0xC891D1);
        assert_close(d.acc_pressed.fill, 0xB584BD);
        assert_eq!(d.acc_disabled.fill, rgb(0x434343));
    }

    fn assert_close(c: Color32, want: u32) {
        let w = rgb(want);
        let d = [c.r().abs_diff(w.r()), c.g().abs_diff(w.g()), c.b().abs_diff(w.b())];
        assert!(d.iter().all(|&x| x <= 1), "{c:?} vs {w:?}");
    }

    #[test]
    fn default_palette_is_windows_blue() {
        let env = ThemeEnv { appearance: Appearance { dark: false, accent: None }, platform: Platform::current() };
        assert_eq!(FluentTokens::new(&env).acc_rest.fill, rgb(0x0067C0));
        let env = ThemeEnv { appearance: Appearance { dark: true, accent: None }, platform: Platform::current() };
        assert_eq!(FluentTokens::new(&env).acc_rest.fill, rgb(0x4CC2FF));
    }
}
