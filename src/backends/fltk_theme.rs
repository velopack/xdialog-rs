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

    todo!();
}

pub fn apply_macos_theme(app_instance: &App, dark: bool) -> DialogTheme {
    load_macos_fonts(app_instance);
    app::set_frame_type_cb(FrameType::ThinUpBox, thin_up_box_noop_cb, 0, 0, 0, 0);

    todo!();
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
    draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::BackGround2);
    draw::begin_line();
    draw::set_draw_color(Color::from_hex(0xDFDFDF));
    draw::draw_line(x, y, x + w, y);
    draw::end_line();
}

fn thin_up_box_noop_cb(x: i32, y: i32, w: i32, h: i32, _: Color) {
    // noop
}

// 
// fn up_box_windows(x: i32, y: i32, w: i32, h: i32, _: Color) {
//     draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::BackGround2);
//     draw::draw_rbox(x, y, w, h, WINDOWS_BUTTON_BORDER_RADIUS, true, Color::from_hex(0xFDFDFD));
//     draw::draw_rbox(x, y, w, h, WINDOWS_BUTTON_BORDER_RADIUS, false, Color::from_hex(0xD0D0D0));
// }
// 
// fn down_box_windows(x: i32, y: i32, w: i32, h: i32, _: Color) {
//     draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::BackGround2);
//     draw::draw_rbox(x, y, w, h, WINDOWS_BUTTON_BORDER_RADIUS, true, Color::from_hex(0xCCE4F7));
//     draw::draw_rbox(x, y, w, h, WINDOWS_BUTTON_BORDER_RADIUS, false, Color::from_hex(0x005499));
// }
// 
// fn engraved_box_windows(x: i32, y: i32, w: i32, h: i32, _: Color) {
//     draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::BackGround2);
//     draw::draw_rbox(x, y, w, h, WINDOWS_BUTTON_BORDER_RADIUS, true, Color::from_hex(0xE0EEF9));
//     draw::draw_rbox(x, y, w, h, WINDOWS_BUTTON_BORDER_RADIUS, false, Color::from_hex(0x0078D4));
// }
// 
// pub fn apply_windows_theme(app_instance: &App) -> DialogTheme {
//     load_windows_fonts(app_instance);
//     app::set_visible_focus(false);
//     app::background(255, 255, 255);
//     app::background2(0xF0, 0xF0, 0xF0);
//     app::set_color(Color::Selection, 0x00, 0x33, 0x99);
//     app::set_frame_type_cb(FrameType::ThinUpBox, thin_up_box_windows, 0, 0, 0, 0);
//     app::set_frame_type_cb(FrameType::UpBox, up_box_windows, 0, 0, 0, 0);
//     app::set_frame_type_cb(FrameType::DownBox, down_box_windows, 0, 0, 0, 0);
//     app::set_frame_type_cb(FrameType::EngravedBox, engraved_box_windows, 0, 0, 0, 0);
// 
//     DialogTheme {
//         button_panel_height: 41,
//         button_panel_margin: 10,
//         button_panel_spacing: 10,
//         button_x_padding: 24,
//         main_icon_size: 32,
//         default_content_margin: 10,
//     }
// }
// 
// fn thin_up_box_ubuntu(_: i32, _: i32, _: i32, _: i32, _: Color) {
//     // no-op
// }
// 
// fn up_box_ubuntu(x: i32, y: i32, w: i32, h: i32, _: Color) {
//     draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::BackGround2);
//     draw::draw_rbox(x, y, w, h, UBUNTU_BUTTON_BORDER_RADIUS, true, Color::from_hex(0xFFFFFF));
//     draw::draw_rbox(x, y, w, h, UBUNTU_BUTTON_BORDER_RADIUS, false, Color::from_hex(0xC7C7C7));
// }
// 
// fn down_box_ubuntu(x: i32, y: i32, w: i32, h: i32, _: Color) {
//     draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::BackGround2);
//     draw::draw_rbox(x, y, w, h, UBUNTU_BUTTON_BORDER_RADIUS, true, Color::from_hex(0xE0E0E0));
//     draw::set_line_style(LineStyle::Solid, 2);
//     draw::draw_rbox(x, y, w, h, UBUNTU_BUTTON_BORDER_RADIUS, false, Color::from_hex(0xE2997F));
//     draw::set_line_style(LineStyle::Solid, 1);
//     draw::draw_rbox(x + 1, y + 1, w - 2, h - 2, UBUNTU_BUTTON_BORDER_RADIUS, false, Color::from_hex(0xF3AA90));
// }
// 
// fn engraved_box_ubuntu(x: i32, y: i32, w: i32, h: i32, _: Color) {
//     draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::BackGround2);
//     draw::draw_rbox(x, y, w, h, UBUNTU_BUTTON_BORDER_RADIUS, true, Color::from_hex(0xF5F5F5));
//     draw::draw_rbox(x, y, w, h, UBUNTU_BUTTON_BORDER_RADIUS, false, Color::from_hex(0xC7C7C7));
// }
// 
// pub fn apply_ubuntu_theme(app_instance: &App) -> DialogTheme {
//     load_ubuntu_fonts(app_instance);
//     app::set_visible_focus(false);
//     app::background(0xFA, 0xFA, 0xFA);
//     app::background2(0xFA, 0xFA, 0xFA);
//     app::foreground(0x3D, 0x3D, 0x3D);
//     app::set_color(Color::Selection, 0x3D, 0x3D, 0x3D);
//     app::set_frame_type_cb(FrameType::ThinUpBox, thin_up_box_ubuntu, 0, 0, 0, 0);
//     app::set_frame_type_cb(FrameType::UpBox, up_box_ubuntu, 0, 0, 0, 0);
//     app::set_frame_type_cb(FrameType::DownBox, down_box_ubuntu, 0, 0, 0, 0);
//     app::set_frame_type_cb(FrameType::EngravedBox, engraved_box_ubuntu, 0, 0, 0, 0);
// 
//     DialogTheme {
//         button_panel_height: 48,
//         button_panel_spacing: 7,
//         button_panel_margin: 7,
//         button_x_padding: 24,
//         main_icon_size: 48,
//         default_content_margin: 12,
//     }
// }
// 
// fn thin_up_box_macos(_: i32, _: i32, _: i32, _: i32, _: Color) {
//     // no-op
// }
// 
// fn up_box_macos_light(x: i32, y: i32, w: i32, h: i32, _: Color) {
//     draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::BackGround2);
//     draw::draw_rbox(x, y, w, h, UBUNTU_BUTTON_BORDER_RADIUS, true, Color::from_hex(0xFFFFFF));
//     draw::draw_rbox(x, y, w, h, UBUNTU_BUTTON_BORDER_RADIUS, false, Color::from_hex(0xDCDBDA));
// }
// 
// fn up_box_macos_dark(x: i32, y: i32, w: i32, h: i32, _: Color) {
//     draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::BackGround2);
//     draw::draw_rbox(x, y, w, h, UBUNTU_BUTTON_BORDER_RADIUS, true, Color::from_hex(0x656565));
// }
// 
// fn down_box_macos(x: i32, y: i32, w: i32, h: i32, _: Color) {
//     draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::BackGround2);
//     draw::draw_rbox(x, y, w, h, UBUNTU_BUTTON_BORDER_RADIUS, true, Color::from_hex(0x146DCC));
// }
// 
// fn engraved_box_macos(x: i32, y: i32, w: i32, h: i32, _: Color) {
//     draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::BackGround2);
//     draw::draw_rbox(x, y, w, h, UBUNTU_BUTTON_BORDER_RADIUS, true, Color::from_hex(0x2482E7));
// }
// 
// pub fn apply_macos_theme(app_instance: &App, dark: bool) -> DialogTheme {
//     load_macos_fonts(app_instance);
//     app::set_visible_focus(false);
// 
//     if dark {
//         app::background(0x2A, 0x29, 0x26);
//         app::background2(0x2A, 0x29, 0x26);
//         app::foreground(0xFF, 0xFF, 0xFF);
//         app::set_color(Color::Selection, 0xFF, 0xFF, 0xFF);
//         app::set_frame_type_cb(FrameType::UpBox, up_box_macos_dark, 0, 0, 0, 0);
//     } else {
//         app::background(0xEC, 0xEB, 0xEA);
//         app::background2(0xEC, 0xEB, 0xEA);
//         app::foreground(0x00, 0x00, 0x00);
//         app::set_color(Color::Selection, 0x00, 0x00, 0x00);
//         app::set_frame_type_cb(FrameType::UpBox, up_box_macos_light, 0, 0, 0, 0);
//     }
// 
//     app::set_frame_type_cb(FrameType::ThinUpBox, thin_up_box_macos, 0, 0, 0, 0);
//     app::set_frame_type_cb(FrameType::DownBox, down_box_macos, 0, 0, 0, 0);
//     app::set_frame_type_cb(FrameType::EngravedBox, engraved_box_macos, 0, 0, 0, 0);
// 
//     DialogTheme {
//         button_panel_height: 54,
//         button_panel_spacing: 10,
//         button_panel_margin: 15,
//         button_x_padding: 24,
//         main_icon_size: 48,
//         default_content_margin: 15,
//     }
// }
