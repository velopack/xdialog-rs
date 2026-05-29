//! The bundled UI font (Ubuntu) and the family name it registers under.
//!
//! The raw bytes are loaded into the cosmic-text font database as the **primary** family (see
//! [`super::text`]); cosmic-text falls back to the installed system fonts per-glyph for anything
//! Ubuntu doesn't cover (CJK, Arabic, color emoji, …), so the bundled font stays deterministic
//! while still rendering arbitrary Unicode.

pub static FONT_REGULAR_DATA: &[u8] = include_bytes!("fonts/Ubuntu-Regular.ttf");
pub static FONT_BOLD_DATA: &[u8] = include_bytes!("fonts/Ubuntu-Bold.ttf");

/// The family name the bundled Ubuntu faces register under in the font database.
pub const UI_FONT_FAMILY: &str = "Ubuntu";
