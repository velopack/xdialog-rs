use std::sync::LazyLock;

static FONT_REGULAR_DATA: &[u8] = include_bytes!("fonts/Ubuntu-Regular.ttf");
static FONT_BOLD_DATA: &[u8] = include_bytes!("fonts/Ubuntu-Bold.ttf");

pub static FONT_REGULAR: LazyLock<fontdue::Font> = LazyLock::new(|| {
    fontdue::Font::from_bytes(FONT_REGULAR_DATA, fontdue::FontSettings::default()).unwrap()
});

pub static FONT_BOLD: LazyLock<fontdue::Font> = LazyLock::new(|| {
    fontdue::Font::from_bytes(FONT_BOLD_DATA, fontdue::FontSettings::default()).unwrap()
});
