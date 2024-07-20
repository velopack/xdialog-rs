use fltk::{app, draw};
use fltk::app::App;
use fltk::enums::{Color, FrameType};

use crate::model::{XDialogIcon, XDialogTheme};
use super::fltk_fonts::*;

#[derive(Debug, Clone)]
pub struct DialogButtonStyle {
    pub color_button_border: Color,
    pub color_button_text: Color,
    pub color_button_background: Color,
    pub border_radius: i32,
    pub border_width: i32,
}

#[derive(Debug, Clone)]
pub struct DialogTheme {
    pub button_panel_height: i32,
    pub button_panel_spacing: i32,
    pub button_panel_margin: i32,
    pub button_text_padding: i32,
    pub button_order_reversed: bool,

    pub main_icon_size: i32,
    pub default_content_margin: i32,

    pub color_background: Color,
    pub color_background_alt: Color,
    pub color_body_text: Color,
    pub color_title_text: Color,
    pub color_progress_background: Color,
    pub color_progress_foreground: Color,

    pub style_button_inactive: DialogButtonStyle,
    pub style_button_hover: DialogButtonStyle,
    pub style_button_pressed: DialogButtonStyle,
    pub style_button_focused: DialogButtonStyle,
}

pub fn apply_theme(app_instance: &App, theme: XDialogTheme) -> DialogTheme {
    let theme = match theme {
        XDialogTheme::SystemDefault => {
            let mode = dark_light::detect();
            let is_dark = mode == dark_light::Mode::Dark;
            if cfg!(target_os = "windows") {
                apply_windows_theme(app_instance)
            } else if cfg!(target_os = "macos") {
                apply_macos_theme(app_instance, is_dark)
            } else {
                apply_ubuntu_theme(app_instance)
            }
        }
        XDialogTheme::Windows => apply_windows_theme(app_instance),
        XDialogTheme::Ubuntu => apply_ubuntu_theme(app_instance),
        XDialogTheme::MacOSLight => apply_macos_theme(app_instance, false),
        XDialogTheme::MacOSDark => apply_macos_theme(app_instance, true)
    };

    let bg = theme.color_background.to_rgb();
    let bg_alt = theme.color_background_alt.to_rgb();
    let fg = theme.color_body_text.to_rgb();
    app::background(bg.0, bg.1, bg.2);
    app::background2(bg_alt.0, bg_alt.1, bg_alt.2);
    app::foreground(fg.0, fg.1, fg.2);
    app::set_visible_focus(false);
    theme
}

pub fn apply_windows_theme(app_instance: &App) -> DialogTheme {
    load_windows_fonts(app_instance);
    app::set_frame_type_cb(FrameType::ThinUpBox, thin_up_box_windows_cb, 0, 0, 0, 0);
    DialogTheme {
        button_panel_height: 41,
        button_panel_margin: 10,
        button_panel_spacing: 10,
        button_text_padding: 24,
        button_order_reversed: true,

        main_icon_size: 32,
        default_content_margin: 10,

        color_background: Color::from_hex(0xFFFFFF),
        color_background_alt: Color::from_hex(0xF0F0F0),
        color_body_text: Color::from_hex(0x000000),
        color_title_text: Color::from_hex(0x003399),
        color_progress_background: Color::from_hex(0xA7CAED),
        color_progress_foreground: Color::from_hex(0x1976D2),

        style_button_inactive: DialogButtonStyle {
            color_button_border: Color::from_hex(0xD0D0D0),
            color_button_background: Color::from_hex(0xFDFDFD),
            color_button_text: Color::from_hex(0x000000),
            border_radius: 3,
            border_width: 1,
        },

        style_button_hover: DialogButtonStyle {
            color_button_border: Color::from_hex(0x0078D4),
            color_button_background: Color::from_hex(0xE0EEF9),
            color_button_text: Color::from_hex(0x000000),
            border_radius: 3,
            border_width: 1,
        },

        style_button_pressed: DialogButtonStyle {
            color_button_border: Color::from_hex(0x005499),
            color_button_background: Color::from_hex(0xCCE4F7),
            color_button_text: Color::from_hex(0x000000),
            border_radius: 3,
            border_width: 1,
        },

        style_button_focused: DialogButtonStyle {
            color_button_border: Color::from_hex(0x0078D4),
            color_button_background: Color::from_hex(0xFDFDFD),
            color_button_text: Color::from_hex(0x000000),
            border_radius: 3,
            border_width: 1,
        },
    }
}

pub fn apply_ubuntu_theme(app_instance: &App) -> DialogTheme {
    load_ubuntu_fonts(app_instance);
    app::set_frame_type_cb(FrameType::ThinUpBox, thin_up_box_noop_cb, 0, 0, 0, 0);
    DialogTheme {
        button_panel_height: 48,
        button_panel_spacing: 7,
        button_panel_margin: 7,
        button_text_padding: 24,
        button_order_reversed: false,

        main_icon_size: 48,
        default_content_margin: 12,

        color_background: Color::from_hex(0xFAFAFA),
        color_background_alt: Color::from_hex(0xFAFAFA),
        color_body_text: Color::from_hex(0x3D3D3D),
        color_title_text: Color::from_hex(0x3D3D3D),
        color_progress_background: Color::from_hex(0xE2997F).lighter(),
        color_progress_foreground: Color::from_hex(0xE2997F),

        style_button_inactive: DialogButtonStyle {
            color_button_border: Color::from_hex(0xC7C7C7),
            color_button_background: Color::from_hex(0xFFFFFF),
            color_button_text: Color::from_hex(0x3D3D3D),
            border_radius: 6,
            border_width: 2,
        },

        style_button_hover: DialogButtonStyle {
            color_button_border: Color::from_hex(0xF3AA90),
            color_button_background: Color::from_hex(0xF5F5F5),
            color_button_text: Color::from_hex(0x3D3D3D),
            border_radius: 6,
            border_width: 2,
        },

        style_button_pressed: DialogButtonStyle {
            color_button_border: Color::from_hex(0xE2997F),
            color_button_background: Color::from_hex(0xE0E0E0),
            color_button_text: Color::from_hex(0x3D3D3D),
            border_radius: 6,
            border_width: 2,
        },

        style_button_focused: DialogButtonStyle {
            color_button_border: Color::from_hex(0xE2997F),
            color_button_background: Color::from_hex(0xFFFFFF),
            color_button_text: Color::from_hex(0x3D3D3D),
            border_radius: 6,
            border_width: 2,
        },
    }
}

pub fn apply_macos_theme(app_instance: &App, dark: bool) -> DialogTheme {
    load_macos_fonts(app_instance);
    app::set_frame_type_cb(FrameType::ThinUpBox, thin_up_box_noop_cb, 0, 0, 0, 0);
    let mut light_theme = DialogTheme {
        button_panel_height: 54,
        button_panel_spacing: 10,
        button_panel_margin: 15,
        button_text_padding: 24,
        button_order_reversed: false,

        main_icon_size: 48,
        default_content_margin: 15,

        color_background: Color::from_hex(0xECEDEB),
        color_background_alt: Color::from_hex(0xECEDEB),
        color_body_text: Color::from_hex(0x242424),
        color_title_text: Color::from_hex(0x242424),
        color_progress_background: Color::from_hex(0x027BFF).lighter(),
        color_progress_foreground: Color::from_hex(0x027BFF),

        style_button_inactive: DialogButtonStyle {
            color_button_border: Color::from_hex(0xD5D6D5),
            color_button_background: Color::from_hex(0xFFFFFF),
            color_button_text: Color::from_hex(0x242424),
            border_radius: 6,
            border_width: 1,
        },

        style_button_hover: DialogButtonStyle {
            color_button_border: Color::TransparentBg,
            color_button_background: Color::from_hex(0x027BFF),
            color_button_text: Color::from_hex(0xFFFFFF),
            border_radius: 6,
            border_width: 0,
        },

        style_button_pressed: DialogButtonStyle {
            color_button_border: Color::TransparentBg,
            color_button_background: Color::from_hex(0x027BFF).darker(),
            color_button_text: Color::from_hex(0xFFFFFF),
            border_radius: 6,
            border_width: 0,
        },

        style_button_focused: DialogButtonStyle {
            color_button_border: Color::TransparentBg,
            color_button_background: Color::from_hex(0x2891FF),
            color_button_text: Color::from_hex(0xFFFFFF),
            border_radius: 6,
            border_width: 0,
        },
    };

    if dark {
        light_theme.color_background = Color::from_hex(0x2A2926);
        light_theme.color_background_alt = Color::from_hex(0x2A2926);
        light_theme.color_body_text = Color::from_hex(0xFFFFFF);
        light_theme.color_title_text = Color::from_hex(0xFFFFFF);
        light_theme.color_progress_background = Color::from_hex(0x027BFF).darker();

        light_theme.style_button_inactive.color_button_border = Color::from_hex(0x656565);
        light_theme.style_button_inactive.color_button_background = Color::from_hex(0x656565);
        light_theme.style_button_inactive.color_button_text = Color::from_hex(0xFFFFFF);
        light_theme.style_button_inactive.border_width = 0;
    }

    light_theme
}

pub fn get_theme_icon_svg(icon: XDialogIcon) -> Option<&'static str>
{
    match icon {
        XDialogIcon::None => None,
        XDialogIcon::Error => Some(crate::images::IMAGE_ERROR_SVG),
        XDialogIcon::Warning => Some(crate::images::IMAGE_WARNING_SVG),
        XDialogIcon::Question => Some(crate::images::IMAGE_INFO_SVG),
        XDialogIcon::Information => Some(crate::images::IMAGE_INFO_SVG),
    }
}

fn thin_up_box_windows_cb(x: i32, y: i32, w: i32, h: i32, _: Color) {
    draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::Background2);
    draw::begin_line();
    draw::set_draw_color(Color::from_hex(0xDFDFDF));
    draw::draw_line(x, y, x + w, y);
    draw::end_line();
}

fn thin_up_box_noop_cb(_: i32, _: i32, _: i32, _: i32, _: Color) {
    // noop
}