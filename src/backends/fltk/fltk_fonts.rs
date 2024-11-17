use std::sync::OnceLock;

use fltk::app::App;
use fltk::enums::Font;
use font_kit::family_name::FamilyName;
use font_kit::handle::Handle;
use font_kit::properties::{Properties, Stretch, Weight};

static MAIN_FONT_NAME: OnceLock<String> = OnceLock::new();
static MAIN_FONT_SIZE: OnceLock<i32> = OnceLock::new();
static BODY_FONT_NAME: OnceLock<String> = OnceLock::new();
static BODY_FONT_SIZE: OnceLock<i32> = OnceLock::new();

pub fn get_main_instruction_font() -> Font {
    if let Some(f) = MAIN_FONT_NAME.get() {
        return Font::by_name(f);
    }
    Font::Helvetica
}

pub fn get_main_instruction_size() -> i32 {
    if let Some(s) = MAIN_FONT_SIZE.get() {
        return s.to_owned();
    }
    16
}

pub fn get_body_font() -> Font {
    if let Some(f) = BODY_FONT_NAME.get() {
        return Font::by_name(f);
    }
    Font::Helvetica
}

pub fn get_body_size() -> i32 {
    if let Some(s) = BODY_FONT_SIZE.get() {
        return s.to_owned();
    }
    12
}

fn try_load_font<S: font_kit::source::Source>(
    app: &App,
    font_source: &S,
    font_families: &[FamilyName],
    font_properties_thin: &Properties,
    target: &OnceLock<String>,
) {
    if let Ok(font_handle) = font_source.select_best_match(&font_families, &font_properties_thin) {
        match font_handle {
            Handle::Path { path, .. } => {
                if let Ok(font) = app.load_font(path) {
                    let _ = target.set(font);
                }
            }
            _ => {}
        }
    }

    if target.get().is_none() {
        let _ = target.set(Font::Helvetica.get_name());
    }
}

pub fn load_windows_fonts(app: &App) {
    let font_source = font_kit::source::SystemSource::new();

    let font_properties_main = Properties {
        weight: Weight::THIN,
        stretch: Stretch::NORMAL,
        style: font_kit::properties::Style::Normal,
    };

    let font_properties_body = Properties {
        weight: Weight::NORMAL,
        stretch: Stretch::NORMAL,
        style: font_kit::properties::Style::Normal,
    };

    let font_families = [
        FamilyName::Title("Segoe UI Variable".to_string()),
        FamilyName::Title("Segoe UI".to_string()),
    ];

    try_load_font(
        app,
        &font_source,
        &font_families,
        &font_properties_main,
        &MAIN_FONT_NAME,
    );
    let _ = MAIN_FONT_SIZE.set(16);
    try_load_font(
        app,
        &font_source,
        &font_families,
        &font_properties_body,
        &BODY_FONT_NAME,
    );
    let _ = BODY_FONT_SIZE.set(12);
}

pub fn load_ubuntu_fonts(app: &App) {
    let font_source = font_kit::source::SystemSource::new();

    let font_properties_main = Properties {
        weight: Weight::BOLD,
        stretch: Stretch::NORMAL,
        style: font_kit::properties::Style::Normal,
    };

    let font_properties_body = Properties {
        weight: Weight::NORMAL,
        stretch: Stretch::NORMAL,
        style: font_kit::properties::Style::Normal,
    };

    let font_families = [
        FamilyName::Title("Ubuntu".to_string()),
        FamilyName::Title("Open Sans".to_string()),
    ];

    try_load_font(
        app,
        &font_source,
        &font_families,
        &font_properties_main,
        &MAIN_FONT_NAME,
    );
    let _ = MAIN_FONT_SIZE.set(18);
    try_load_font(
        app,
        &font_source,
        &font_families,
        &font_properties_body,
        &BODY_FONT_NAME,
    );
    let _ = BODY_FONT_SIZE.set(15);
}

pub fn load_macos_fonts(app: &App) {
    let font_source = font_kit::source::SystemSource::new();

    let font_properties_main = Properties {
        weight: Weight::EXTRA_BOLD,
        stretch: Stretch::NORMAL,
        style: font_kit::properties::Style::Normal,
    };

    let font_properties_body = Properties {
        weight: Weight::NORMAL,
        stretch: Stretch::NORMAL,
        style: font_kit::properties::Style::Normal,
    };

    let font_families = [
        FamilyName::Title("Helvetica Neue".to_string()),
        FamilyName::Title("Helvetica".to_string()),
    ];

    try_load_font(
        app,
        &font_source,
        &font_families,
        &font_properties_main,
        &MAIN_FONT_NAME,
    );
    try_load_font(
        app,
        &font_source,
        &font_families,
        &font_properties_body,
        &BODY_FONT_NAME,
    );
    // let _ = MAIN_FONT_NAME.set(Font::HelveticaBold.get_name());
    // let _ = BODY_FONT_NAME.set(Font::Helvetica.get_name());
    let _ = MAIN_FONT_SIZE.set(15);
    let _ = BODY_FONT_SIZE.set(12);
}
