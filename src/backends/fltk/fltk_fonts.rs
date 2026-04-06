use std::sync::OnceLock;

use fltk::app::App;
use fltk::enums::Font;
use font_kit::family_name::FamilyName;
use font_kit::handle::Handle;
use font_kit::properties::{Properties, Stretch, Weight};

// Embedded fallback fonts (Inter, SIL Open Font License 1.1)
static FALLBACK_REGULAR: &[u8] = include_bytes!("fonts/Inter-Regular.ttf");
static FALLBACK_BOLD: &[u8] = include_bytes!("fonts/Inter-Bold.ttf");

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

/// Try to load a system font via fontconfig (dynamically loaded).
/// Returns true if a font was successfully loaded into `target`.
fn try_load_system_font(
    app: &App,
    font_families: &[FamilyName],
    font_properties: &Properties,
    target: &OnceLock<String>,
) -> bool {
    // fontconfig is loaded via dlopen, so SystemSource::new() may panic
    // if libfontconfig.so is not available on the system
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let source = font_kit::source::SystemSource::new();
        source.select_best_match(font_families, font_properties)
    }));

    if let Ok(Ok(Handle::Path { path, .. })) = result {
        if let Ok(font) = app.load_font(path) {
            let _ = target.set(font);
            return true;
        }
    }
    false
}

/// Load an embedded fallback font by writing it to a temp file and loading via FLTK.
fn load_embedded_font(app: &App, font_data: &[u8], name: &str, target: &OnceLock<String>) -> bool {
    let temp_path = std::env::temp_dir().join(format!("xdialog_{name}.ttf"));
    if std::fs::write(&temp_path, font_data).is_ok() {
        if let Ok(font_name) = app.load_font(&temp_path) {
            let _ = target.set(font_name);
            let _ = std::fs::remove_file(&temp_path);
            return true;
        }
    }
    false
}

fn try_load_font(
    app: &App,
    font_families: &[FamilyName],
    font_properties: &Properties,
    target: &OnceLock<String>,
    fallback_data: &[u8],
    fallback_name: &str,
) {
    if try_load_system_font(app, font_families, font_properties, target) {
        return;
    }

    if load_embedded_font(app, fallback_data, fallback_name, target) {
        return;
    }

    let _ = target.set(Font::Helvetica.get_name());
}

pub fn load_windows_fonts(app: &App) {
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
        &font_families,
        &font_properties_main,
        &MAIN_FONT_NAME,
        FALLBACK_BOLD,
        "inter_bold",
    );
    let _ = MAIN_FONT_SIZE.set(16);
    try_load_font(
        app,
        &font_families,
        &font_properties_body,
        &BODY_FONT_NAME,
        FALLBACK_REGULAR,
        "inter_regular",
    );
    let _ = BODY_FONT_SIZE.set(12);
}

pub fn load_ubuntu_fonts(app: &App) {
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
        &font_families,
        &font_properties_main,
        &MAIN_FONT_NAME,
        FALLBACK_BOLD,
        "inter_bold",
    );
    let _ = MAIN_FONT_SIZE.set(18);
    try_load_font(
        app,
        &font_families,
        &font_properties_body,
        &BODY_FONT_NAME,
        FALLBACK_REGULAR,
        "inter_regular",
    );
    let _ = BODY_FONT_SIZE.set(15);
}

pub fn load_macos_fonts(app: &App) {
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
        &font_families,
        &font_properties_main,
        &MAIN_FONT_NAME,
        FALLBACK_BOLD,
        "inter_bold",
    );
    try_load_font(
        app,
        &font_families,
        &font_properties_body,
        &BODY_FONT_NAME,
        FALLBACK_REGULAR,
        "inter_regular",
    );
    let _ = MAIN_FONT_SIZE.set(15);
    let _ = BODY_FONT_SIZE.set(12);
}
