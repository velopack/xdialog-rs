//! Fluent theme fonts.
//!
//! WinUI resolves `XamlAutoFontFamily` to **Segoe UI Variable** (`SegUIVar.ttf`, axes `wght` and
//! `opsz`). Preference order:
//! 1. `SegUIVar.ttf` (Windows 11): body/buttons `wght 400`, title `wght 600`, each with the
//!    automatic optical size for the font size;
//! 2. `segoeui.ttf` + `seguisb.ttf` (Windows 10);
//! 3. the bundled Ubuntu Regular/Bold (Linux builds of the fluent theme, broken installs).
//!
//! The regular face renders body and button text, the bold face the 20 px SemiBold title. With
//! the variable font both are the same bytes with different `FontTweak::coords`.

use std::sync::OnceLock;

use egui::epaint::text::VariationCoords;

use crate::backends::egui_core::fonts::{bundled, FaceRef, FontRegistry, ThemeFace, ThemeFonts};

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

/// The faces the theme renders with.
#[derive(Clone, Copy, Debug)]
pub(crate) enum FaceSet {
    /// Segoe UI Variable (one file, weights through `wght`).
    Variable(FaceRef),
    /// Static Segoe UI Regular + Semibold.
    Static { regular: FaceRef, semibold: FaceRef },
    /// Bundled Ubuntu Regular + Bold.
    Bundled,
}

impl FaceSet {
    /// Resolved once per process (the files are read once and kept by the registry).
    pub(crate) fn get(reg: &FontRegistry) -> FaceSet {
        static SET: OnceLock<FaceSet> = OnceLock::new();
        *SET.get_or_init(|| {
                if let Some(f) = reg.windows_font("SegUIVar.ttf", 0) {
                    return FaceSet::Variable(f);
                }
                match (reg.windows_font("segoeui.ttf", 0), reg.windows_font("seguisb.ttf", 0)) {
                    (Some(regular), Some(semibold)) => FaceSet::Static { regular, semibold },
                    _ => FaceSet::Bundled,
                }
            })
    }

    /// The body (regular) and title (bold) faces.
    pub(crate) fn fonts(&self) -> ThemeFonts {
        match *self {
            FaceSet::Variable(face) => {
                let at = |w: f32, size: f32| ThemeFace { face, coords: VariationCoords::new([(b"wght", w), (b"opsz", opsz(size))]) };
                ThemeFonts { regular: at(400.0, BODY_SIZE), bold: at(600.0, TITLE_SIZE) }
            }
            FaceSet::Static { regular, semibold } => ThemeFonts::new(regular, semibold),
            FaceSet::Bundled => ThemeFonts::new(bundled::ubuntu_regular(), bundled::ubuntu_bold()),
        }
    }
}
