//! Fluent (WinUI 3) colour tokens, light and dark.
//!
//! WinUI brushes are mostly translucent and get composited over whatever lies behind them. The
//! surfaces behind every element are fixed here (content area, button bar), so fills and borders
//! are pre-composited into OPAQUE colours (they interpolate cleanly). Text and focus colours stay
//! translucent and blend over the surface when painted.
//!
//! Fonts: WinUI resolves `XamlAutoFontFamily` to **Segoe UI Variable** (`SegUIVar.ttf`, axes
//! `wght` and `opsz`): body/buttons `wght 400`, the 20 px SemiBold title `wght 600`, each with the
//! automatic optical size, i.e. the same bytes with different coordinates. Fallbacks: `segoeui.ttf`
//! + `seguisb.ttf` (Windows 10), then [`ui_fonts`] (other platforms).

use std::sync::OnceLock;

use egui::epaint::text::VariationCoords;
use egui::Color32;

use crate::backends::egui_core::appearance::Appearance;
use crate::backends::egui_core::color::{argb, rgb};
use crate::backends::egui_core::fonts::{ui_fonts, FontRegistry, ThemeFace, ThemeFonts};

/// Windows 11 default (blue) accent palette `[L3, L2, L1, A, D1, D2, D3]`: the fallback when the
/// system provides no accent.
const DEFAULT_PALETTE: [u32; 7] = [0x99EBFF, 0x4CC2FF, 0x0091F8, 0x0078D4, 0x0067C0, 0x003E92, 0x001A68];

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
    /// No border at all (accent pressed).
    pub borderless: bool,
    /// Label colour (may be translucent).
    pub text: Color32,
}

#[derive(Clone, Debug)]
pub(crate) struct FluentTokens {
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
    pub acc_rest: ButtonColors,
    pub acc_hover: ButtonColors,
    pub acc_pressed: ButtonColors,
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

/// The accent palette for `appearance`: the system's (Windows registry), one derived from an
/// accent colour without a palette (portal, test override), else the Windows 11 default blue.
fn palette(appearance: &Appearance) -> [Color32; 7] {
    appearance.accent.map_or(DEFAULT_PALETTE.map(rgb), |a| a.win_palette.unwrap_or_else(|| derive_palette(a.base)))
}

/// Approximate `[L3, L2, L1, A, D1, D2, D3]` from a single accent colour (only used when an
/// accent comes without the Windows palette, i.e. `XDIALOG_TEST_ACCENT` alone).
fn derive_palette(base: Color32) -> [Color32; 7] {
    let (w, b) = (Color32::WHITE, Color32::BLACK);
    let mix = |to: Color32, t: f32| base.lerp_to_gamma(to, t);
    [mix(w, 0.6), mix(w, 0.4), mix(w, 0.2), base, mix(b, 0.15), mix(b, 0.4), mix(b, 0.65)]
}

impl FluentTokens {
    pub(crate) fn new(appearance: &Appearance) -> Self {
        Self::build(appearance.dark, &palette(appearance))
    }

    fn build(dark: bool, p: &[Color32; 7]) -> Self {
        // WinUI `Common_themeresources_any.xaml`, Light | Dark.
        let t = |light: u32, dark_v: u32| argb(if dark { dark_v } else { light });
        let base = t(0xFFF3F3F3, 0xFF202020); // SolidBackgroundFillColorBase
        let layer_alt = t(0xFFFFFFFF, 0x0DFFFFFF); // LayerFillColorAlt
        let card_stroke = t(0x0F000000, 0x19000000); // CardStrokeColorDefault
        let text = t(0xE4000000, 0xFFFFFFFF); // TextFillColorPrimary
        let text_2 = t(0x9E000000, 0xC5FFFFFF); // TextFillColorSecondary
        let ctl_fill = t(0xB3FFFFFF, 0x0FFFFFFF); // ControlFillColorDefault
        let ctl_fill_2 = t(0x80F9F9F9, 0x15FFFFFF); // ControlFillColorSecondary
        let ctl_fill_3 = t(0x4DF9F9F9, 0x08FFFFFF); // ControlFillColorTertiary
        let ctl_stroke = t(0x0F000000, 0x12FFFFFF); // ControlStrokeColorDefault
        let ctl_stroke_2 = t(0x29000000, 0x18FFFFFF); // ControlStrokeColorSecondary
        let on_acc_stroke = argb(0x14FFFFFF); // ControlStrokeColorOnAccentDefault
        let on_acc_stroke_2 = t(0x66000000, 0x23000000); // ControlStrokeColorOnAccentSecondary
        let on_acc_text = t(0xFFFFFFFF, 0xFF000000); // TextOnAccentFillColorPrimary
        let on_acc_text_2 = t(0xB3FFFFFF, 0x80000000); // TextOnAccentFillColorSecondary
        let strong_stroke = t(0x72000000, 0x8BFFFFFF); // ControlStrongStrokeColorDefault

        // AccentFillColorDefault = Dark1 (light) / Light2 (dark); Secondary/Tertiary = 0.9/0.8
        // brush opacity, composited over the bar.
        let accent = if dark { p[L2] } else { p[D1] };
        let bar = base;
        let stroke_bar = bar.blend(ctl_stroke);
        let std = |fill: Color32, elevated: bool, text: Color32| ButtonColors { fill: bar.blend(fill),
                                                                                stroke: stroke_bar,
                                                                                stroke_2: if elevated { bar.blend(ctl_stroke_2) } else { stroke_bar },
                                                                                borderless: false,
                                                                                text };
        let acc = |fill: Color32, bordered: bool, text: Color32| {
            let fill = bar.blend(fill);
            ButtonColors { fill, stroke: fill.blend(on_acc_stroke), stroke_2: fill.blend(on_acc_stroke_2), borderless: !bordered, text }
        };
        FluentTokens { content_bg: base.blend(layer_alt),
                       bar_bg: base,
                       separator: base.blend(card_stroke),
                       text,
                       std_rest: std(ctl_fill, true, text),
                       std_hover: std(ctl_fill_2, true, text),
                       std_pressed: std(ctl_fill_3, false, text_2),
                       acc_rest: acc(accent, true, on_acc_text),
                       acc_hover: acc(bar.lerp_to_gamma(accent, 0.9), true, on_acc_text),
                       acc_pressed: acc(bar.lerp_to_gamma(accent, 0.8), false, on_acc_text_2),
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

// ------------------------------------------------------------------------------------------------
// Fonts
// ------------------------------------------------------------------------------------------------

/// Line height / font size of Segoe UI (`(ascent + descent) / unitsPerEm` = (2210 + 514) / 2048):
/// 14 px -> 18.62, 20 px -> 26.6.
pub(crate) const LINE_RATIO: f32 = 2724.0 / 2048.0;

/// Body / button font size and title font size (ControlContentThemeFontSize, ContentDialog title).
pub(crate) const BODY_SIZE: f32 = 14.0;
pub(crate) const TITLE_SIZE: f32 = 20.0;

/// Optical size for a font size in DIPs (automatic `opsz` = size in points).
fn opsz(size_px: f32) -> f32 {
    size_px * 0.75
}

/// The body (regular) and title (bold) faces, resolved once per process (the files are read once
/// and kept by the registry).
pub(crate) fn theme_fonts(reg: &FontRegistry) -> ThemeFonts {
    static FONTS: OnceLock<ThemeFonts> = OnceLock::new();
    FONTS.get_or_init(|| {
             if let Some(face) = reg.windows_font("SegUIVar.ttf", 0) {
                 let at = |w: f32, size: f32| ThemeFace { face, coords: VariationCoords::new([(b"wght", w), (b"opsz", opsz(size))]) };
                 return ThemeFonts { regular: at(400.0, BODY_SIZE), bold: at(600.0, TITLE_SIZE) };
             }
             match (reg.windows_font("segoeui.ttf", 0), reg.windows_font("seguisb.ttf", 0)) {
                 (Some(regular), Some(semibold)) => ThemeFonts::new(regular, semibold),
                 _ => ui_fonts(),
             }
         })
         .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PURPLE: [[u8; 3]; 7] = [[0xF0, 0xC0, 0xF4], [0xDB, 0x9E, 0xE5], [0xB7, 0x63, 0xCB], [0xA9, 0x4D, 0xC1], [0x8E, 0x3A, 0xA7], [0x69, 0x27, 0x82], [0x40, 0x0E, 0x59]];

    fn tokens(dark: bool) -> FluentTokens {
        FluentTokens::new(&Appearance::test(dark, None, Some(PURPLE)))
    }

    /// The pre-composited colours match the WinUI 3 solid colours (within 1 per channel).
    #[test]
    fn composites_match_winui() {
        let l = tokens(false);
        for (c, want) in [(l.content_bg, 0xFFFFFF),
                          (l.separator, 0xE5E5E5),
                          (l.std_rest.fill, 0xFBFBFB),
                          (l.std_rest.stroke, 0xE5E5E5),
                          (l.std_rest.stroke_2, 0xCCCCCC),
                          (l.std_hover.fill, 0xF6F6F6),
                          (l.std_pressed.fill, 0xF5F5F5),
                          (l.acc_rest.fill, 0x8E3AA7),
                          (l.acc_rest.stroke, 0x9749AE),
                          (l.acc_rest.stroke_2, 0x552364),
                          (l.acc_hover.fill, 0x984DAF),
                          (l.acc_hover.stroke, 0xA05BB5),
                          (l.acc_pressed.fill, 0xA25FB6)]
        {
            assert_close(c, want);
        }
        let d = tokens(true);
        for (c, want) in [(d.content_bg, 0x2B2B2B),
                          (d.separator, 0x1D1D1D),
                          (d.std_rest.fill, 0x2D2D2D),
                          (d.std_rest.stroke, 0x303030),
                          (d.std_rest.stroke_2, 0x353535),
                          (d.std_hover.fill, 0x323232),
                          (d.std_pressed.fill, 0x272727),
                          (d.acc_rest.fill, 0xDB9EE5),
                          (d.acc_rest.stroke, 0xDEA6E7),
                          (d.acc_rest.stroke_2, 0xBD88C6),
                          (d.acc_hover.fill, 0xC891D1),
                          (d.acc_pressed.fill, 0xB584BD)]
        {
            assert_close(c, want);
        }
    }

    fn assert_close(c: Color32, want: u32) {
        let w = rgb(want);
        let d = [c.r().abs_diff(w.r()), c.g().abs_diff(w.g()), c.b().abs_diff(w.b())];
        assert!(d.iter().all(|&x| x <= 1), "{c:?} vs {w:?}");
    }

    #[test]
    fn default_palette_is_windows_blue() {
        assert_eq!(FluentTokens::new(&Appearance { dark: false, accent: None }).acc_rest.fill, rgb(0x0067C0));
        assert_eq!(FluentTokens::new(&Appearance { dark: true, accent: None }).acc_rest.fill, rgb(0x4CC2FF));
    }
}
