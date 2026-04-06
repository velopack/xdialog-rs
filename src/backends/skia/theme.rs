#[derive(Debug, Clone)]
pub struct SkiaButtonStyle {
    pub border_color: (u8, u8, u8),
    pub background_color: (u8, u8, u8),
    pub text_color: (u8, u8, u8),
    pub border_radius: i32,
    pub border_width: i32,
}

#[derive(Debug, Clone)]
pub struct SkiaTheme {
    pub button_panel_height: i32,
    pub button_panel_spacing: i32,
    pub button_panel_margin: i32,
    pub button_text_padding: i32,
    pub button_order_reversed: bool,

    pub main_icon_size: i32,
    pub default_content_margin: i32,
    pub content_margin_top: i32,
    pub content_margin_bottom: i32,

    pub color_background: (u8, u8, u8),
    pub color_background_alt: (u8, u8, u8),
    pub color_body_text: (u8, u8, u8),
    pub color_title_text: (u8, u8, u8),
    pub color_progress_background: (u8, u8, u8),
    pub color_progress_foreground: (u8, u8, u8),

    pub style_button_inactive: SkiaButtonStyle,
    pub style_button_hover: SkiaButtonStyle,
    pub style_button_pressed: SkiaButtonStyle,
    pub style_button_focused: SkiaButtonStyle,
}

pub fn get_theme() -> SkiaTheme {
    // Ubuntu theme - matching fltk_theme.rs apply_ubuntu_theme()
    SkiaTheme {
        button_panel_height: 48,
        button_panel_spacing: 7,
        button_panel_margin: 7,
        button_text_padding: 24,
        button_order_reversed: false,

        main_icon_size: 48,
        default_content_margin: 12,
        content_margin_top: 5,
        content_margin_bottom: 16,

        color_background: (0xFA, 0xFA, 0xFA),
        color_background_alt: (0xFA, 0xFA, 0xFA),
        color_body_text: (0x3D, 0x3D, 0x3D),
        color_title_text: (0x3D, 0x3D, 0x3D),
        // Pre-computed: FLTK Color::from_hex(0xE2997F).lighter() ≈ (0xFF, 0xC1, 0xA7)
        color_progress_background: (0xFF, 0xC1, 0xA7),
        color_progress_foreground: (0xE2, 0x99, 0x7F),

        style_button_inactive: SkiaButtonStyle {
            border_color: (0xC7, 0xC7, 0xC7),
            background_color: (0xFF, 0xFF, 0xFF),
            text_color: (0x3D, 0x3D, 0x3D),
            border_radius: 6,
            border_width: 2,
        },

        style_button_hover: SkiaButtonStyle {
            border_color: (0xF3, 0xAA, 0x90),
            background_color: (0xF5, 0xF5, 0xF5),
            text_color: (0x3D, 0x3D, 0x3D),
            border_radius: 6,
            border_width: 2,
        },

        style_button_pressed: SkiaButtonStyle {
            border_color: (0xE2, 0x99, 0x7F),
            background_color: (0xE0, 0xE0, 0xE0),
            text_color: (0x3D, 0x3D, 0x3D),
            border_radius: 6,
            border_width: 2,
        },

        style_button_focused: SkiaButtonStyle {
            border_color: (0xE2, 0x99, 0x7F),
            background_color: (0xFF, 0xFF, 0xFF),
            text_color: (0x3D, 0x3D, 0x3D),
            border_radius: 6,
            border_width: 2,
        },
    }
}
