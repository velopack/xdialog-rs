use fltk::enums::Font;

static MAIN_FONT: Font = Font::HelveticaBold;
static MAIN_FONT_SIZE: i32 = 16;
static BODY_FONT: Font = Font::Helvetica;
static BODY_FONT_SIZE: i32 = 12;

pub fn get_main_instruction_font() -> Font {
    MAIN_FONT
}

pub fn get_main_instruction_size() -> i32 {
    MAIN_FONT_SIZE
}

pub fn get_body_font() -> Font {
    BODY_FONT
}

pub fn get_body_size() -> i32 {
    BODY_FONT_SIZE
}
