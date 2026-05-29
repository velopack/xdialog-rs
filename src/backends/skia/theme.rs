use super::desktop::{ColorScheme, DesktopAppearance};

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

/// Build the theme for the given desktop appearance: pick the light or dark base, then overlay
/// the desktop accent color (if any) onto the interactive elements.
pub fn get_theme(appearance: &DesktopAppearance) -> SkiaTheme {
    let mut theme = match appearance.color_scheme {
        ColorScheme::Dark => ubuntu_dark(),
        // NoPreference falls back to the light Ubuntu theme (historical default).
        ColorScheme::Light | ColorScheme::NoPreference => ubuntu_light(),
    };
    if let Some(accent) = appearance.accent_color {
        apply_accent(&mut theme, accent);
    }
    theme
}

/// The original hard-coded Ubuntu light theme - matching fltk_theme.rs apply_ubuntu_theme().
fn ubuntu_light() -> SkiaTheme {
    SkiaTheme {
        button_panel_height: 48,
        button_panel_spacing: 7,
        button_panel_margin: 7,
        button_text_padding: 24,
        button_order_reversed: false,

        main_icon_size: 48,
        default_content_margin: 16,

        color_background: (0xFA, 0xFA, 0xFA),
        color_background_alt: (0xFA, 0xFA, 0xFA),
        color_body_text: (0x3D, 0x3D, 0x3D),
        color_title_text: (0x3D, 0x3D, 0x3D),
        color_progress_background: (173, 206, 247),
        color_progress_foreground: (42, 125, 227),

        style_button_inactive: SkiaButtonStyle {
            border_color: (0xC7, 0xC7, 0xC7),
            background_color: (0xFF, 0xFF, 0xFF),
            text_color: (0x3D, 0x3D, 0x3D),
            border_radius: 6,
            border_width: 2,
        },

        style_button_hover: SkiaButtonStyle {
            border_color: (42, 125, 227),
            background_color: (42, 125, 227),
            text_color: (0xFF, 0xFF, 0xFF),
            border_radius: 6,
            border_width: 2,
        },

        style_button_pressed: SkiaButtonStyle {
            border_color: (30, 95, 175),
            background_color: (30, 95, 175),
            text_color: (0xFF, 0xFF, 0xFF),
            border_radius: 6,
            border_width: 2,
        },

        style_button_focused: SkiaButtonStyle {
            border_color: (42, 125, 227),
            background_color: (0xFF, 0xFF, 0xFF),
            text_color: (0x3D, 0x3D, 0x3D),
            border_radius: 6,
            border_width: 2,
        },
    }
}

/// A dark counterpart to [`ubuntu_light`], in the spirit of Adwaita/Ubuntu dark. Layout metrics
/// match the light theme; only colors change. The accent blue mirrors the light theme and is
/// overridden by [`apply_accent`] when the desktop provides one.
fn ubuntu_dark() -> SkiaTheme {
    SkiaTheme {
        button_panel_height: 48,
        button_panel_spacing: 7,
        button_panel_margin: 7,
        button_text_padding: 24,
        button_order_reversed: false,

        main_icon_size: 48,
        default_content_margin: 16,

        color_background: (0x2D, 0x2D, 0x2D),
        color_background_alt: (0x2D, 0x2D, 0x2D),
        color_body_text: (0xEE, 0xEE, 0xEE),
        color_title_text: (0xFF, 0xFF, 0xFF),
        color_progress_background: (0x4A, 0x4A, 0x4A),
        color_progress_foreground: (42, 125, 227),

        style_button_inactive: SkiaButtonStyle {
            border_color: (0x5A, 0x5A, 0x5A),
            background_color: (0x3B, 0x3B, 0x3B),
            text_color: (0xEE, 0xEE, 0xEE),
            border_radius: 6,
            border_width: 2,
        },

        style_button_hover: SkiaButtonStyle {
            border_color: (42, 125, 227),
            background_color: (42, 125, 227),
            text_color: (0xFF, 0xFF, 0xFF),
            border_radius: 6,
            border_width: 2,
        },

        style_button_pressed: SkiaButtonStyle {
            border_color: (30, 95, 175),
            background_color: (30, 95, 175),
            text_color: (0xFF, 0xFF, 0xFF),
            border_radius: 6,
            border_width: 2,
        },

        style_button_focused: SkiaButtonStyle {
            border_color: (42, 125, 227),
            background_color: (0x3B, 0x3B, 0x3B),
            text_color: (0xEE, 0xEE, 0xEE),
            border_radius: 6,
            border_width: 2,
        },
    }
}

/// Overlay the desktop's accent color onto the interactive elements (hover/pressed/focused
/// buttons and the progress bar), deriving pressed/track shades from it and choosing readable
/// text. Everything else (backgrounds, inactive buttons, body text) keeps the base theme.
fn apply_accent(theme: &mut SkiaTheme, accent: (u8, u8, u8)) {
    let pressed = darken(accent, 0.75);
    let on_accent = contrasting_text(accent);
    let on_pressed = contrasting_text(pressed);

    theme.color_progress_foreground = accent;
    theme.color_progress_background = blend(accent, theme.color_background, 0.65);

    theme.style_button_hover.border_color = accent;
    theme.style_button_hover.background_color = accent;
    theme.style_button_hover.text_color = on_accent;

    theme.style_button_pressed.border_color = pressed;
    theme.style_button_pressed.background_color = pressed;
    theme.style_button_pressed.text_color = on_pressed;

    // Focused keeps the base background; only the accent outline changes.
    theme.style_button_focused.border_color = accent;
}

/// Scale each channel toward black by `factor` (e.g. 0.75 = 25% darker).
fn darken((r, g, b): (u8, u8, u8), factor: f32) -> (u8, u8, u8) {
    let f = |c: u8| (c as f32 * factor).round().clamp(0.0, 255.0) as u8;
    (f(r), f(g), f(b))
}

/// Linearly mix `from` toward `to`, where `t` is the weight of `to` (0.0 = `from`, 1.0 = `to`).
fn blend(from: (u8, u8, u8), to: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let mix = |a: u8, b: u8| (a as f32 * (1.0 - t) + b as f32 * t).round().clamp(0.0, 255.0) as u8;
    (mix(from.0, to.0), mix(from.1, to.1), mix(from.2, to.2))
}

/// Pick black or white text for legibility on the given background using perceived luminance.
fn contrasting_text((r, g, b): (u8, u8, u8)) -> (u8, u8, u8) {
    let luminance = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
    if luminance > 150.0 {
        (0x1A, 0x1A, 0x1A)
    } else {
        (0xFF, 0xFF, 0xFF)
    }
}
