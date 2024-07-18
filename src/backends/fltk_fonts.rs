use std::sync::OnceLock;
use fltk::enums::Font;
use font_kit::family_name::FamilyName;
use font_kit::handle::Handle;
use font_kit::properties::{Properties, Stretch, Weight};

static MAIN_FONT_NAME: OnceLock<String> = OnceLock::new();
static BODY_FONT_NAME: OnceLock<String> = OnceLock::new();

pub fn get_main_instruction_font() -> Font {
    if let Some(f) = MAIN_FONT_NAME.get() {
        return Font::by_name(f);
    }
    Font::Helvetica
}

pub fn get_main_instruction_size() -> i32 {
    16
}

pub fn get_body_font() -> Font {
    if let Some(f) = BODY_FONT_NAME.get() {
        return Font::by_name(f);
    }
    Font::Helvetica
}

pub fn get_body_size() -> i32 {
    12
}

pub fn load_fonts() {
    let font_source = font_kit::source::SystemSource::new();

    let font_properties_thin = Properties {
        weight: Weight::THIN,
        stretch: Stretch::NORMAL,
        style: font_kit::properties::Style::Normal,
    };

    let font_properties_regular = Properties {
        weight: Weight::NORMAL,
        stretch: Stretch::NORMAL,
        style: font_kit::properties::Style::Normal,
    };

    let font_families = [
        FamilyName::Title("Segoe UI Variable".to_string()),
        FamilyName::Title("Segoe UI".to_string()),
    ];

    if let Ok(font_handle) = font_source.select_best_match(&font_families, &font_properties_thin)
    {
        match font_handle {
            Handle::Path { path, .. } => {
                if let Ok(font) = Font::load_font(path) {
                    let _ = MAIN_FONT_NAME.set(font);
                }
            }
            _ => {}
        }
    }

    if let Ok(font_handle) = font_source.select_best_match(&font_families, &font_properties_regular)
    {
        match font_handle {
            Handle::Path { path, .. } => {
                if let Ok(font) = Font::load_font(path) {
                    let _ = BODY_FONT_NAME.set(font);
                }
            }
            _ => {}
        }
    }

    // fails if already set (fine)
    let _ = MAIN_FONT_NAME.set(Font::Helvetica.get_name());
    let _ = BODY_FONT_NAME.set(Font::Helvetica.get_name());
}