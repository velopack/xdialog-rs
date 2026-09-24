//! Fluent theme fonts.
//!
//! WinUI resolves `XamlAutoFontFamily` to **Segoe UI Variable** (`SegUIVar.ttf`, axes `wght` and
//! `opsz`). Preference order:
//! 1. `SegUIVar.ttf` (Windows 11): body/buttons `wght 400`, title `wght 600`, each with the optical
//!    size DirectWrite picks automatically for the font size;
//! 2. `segoeui.ttf` + `seguisb.ttf` (Windows 10);
//! 3. the bundled Ubuntu Regular/Bold (Linux builds of the fluent theme, broken installs).
//!
//! Two egui families: `Proportional` (= Monospace) for body and button text, and
//! `Name("fluent.title")` for the 20 px SemiBold title. With the variable font both are the same
//! bytes with different `FontTweak::coords`.

use std::sync::{Arc, OnceLock};

use egui::epaint::text::{FontTweak, VariationCoords};
use egui::{FontData, FontDefinitions, FontFamily};

use crate::backends::egui_core::fonts::{bundled, FaceRef, FontRegistry};

/// Family of the title text.
pub(crate) fn title_family() -> FontFamily {
    FontFamily::Name("fluent.title".into())
}

/// Line box pitch / font size of Segoe UI (DirectWrite default line spacing =
/// `(usWinAscent + usWinDescent) / unitsPerEm` = (2210 + 514) / 2048): 14 px -> 18.62, 20 px -> 26.6.
pub(crate) const LINE_RATIO: f32 = 2724.0 / 2048.0;

/// Body / button font size and title font size (ControlContentThemeFontSize, ContentDialog title).
pub(crate) const BODY_SIZE: f32 = 14.0;
pub(crate) const TITLE_SIZE: f32 = 20.0;

/// Optical size DirectWrite derives for a font size in DIPs (automatic `opsz` = size in points).
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
                if std::env::var_os("XDIALOG_FLUENT_BUNDLED_FONT").is_some() {
                    return FaceSet::Bundled;
                }
                if let Some(f) = reg.windows_font("SegUIVar.ttf", 0) {
                    return FaceSet::Variable(f);
                }
                match (reg.windows_font("segoeui.ttf", 0), reg.windows_font("seguisb.ttf", 0)) {
                    (Some(regular), Some(semibold)) => FaceSet::Static { regular, semibold },
                    _ => FaceSet::Bundled,
                }
            })
    }

    pub(crate) fn faces(&self) -> Vec<FaceRef> {
        match *self {
            FaceSet::Variable(f) => vec![f],
            FaceSet::Static { regular, semibold } => vec![regular, semibold],
            FaceSet::Bundled => vec![bundled::ubuntu_regular(), bundled::ubuntu_bold()],
        }
    }

    /// Add the faces and bind `Proportional`, `Monospace` and the title family.
    pub(crate) fn install(&self, defs: &mut FontDefinitions) {
        let (body, title) = match *self {
            FaceSet::Variable(f) => {
                let coords = |w: f32, size: f32| VariationCoords::new([(b"wght", w), (b"opsz", opsz(size))]);
                (with_tweak(f.font_data(), coords(400.0, BODY_SIZE)), with_tweak(f.font_data(), coords(600.0, TITLE_SIZE)))
            }
            FaceSet::Static { regular, semibold } => (regular.font_data(), semibold.font_data()),
            FaceSet::Bundled => (bundled::ubuntu_regular().font_data(), bundled::ubuntu_bold().font_data()),
        };
        defs.font_data.insert("fluent.body".into(), Arc::new(body));
        defs.font_data.insert("fluent.title".into(), Arc::new(title));
        defs.families.insert(FontFamily::Proportional, vec!["fluent.body".into()]);
        defs.families.insert(FontFamily::Monospace, vec!["fluent.body".into()]);
        defs.families.insert(title_family(), vec!["fluent.title".into()]);
    }
}

fn with_tweak(mut data: FontData, coords: VariationCoords) -> FontData {
    data.tweak = FontTweak { coords, ..data.tweak.clone() };
    data
}
