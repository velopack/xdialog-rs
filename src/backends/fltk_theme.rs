use fltk::{app, draw};
use fltk::app::App;
use fltk::draw::LineStyle;
use fltk::enums::{Color, FrameType};

use crate::model::{MessageBoxIcon, XDialogTheme};
use super::fltk_fonts::*;

pub fn apply_theme(app_instance: &App, theme: XDialogTheme) -> DialogSpacing {
    match theme {
        XDialogTheme::SystemDefault => {
            if cfg!(target_os = "windows") {
                apply_windows_theme(app_instance)
            } else {
                apply_ubuntu_theme(app_instance)
            }
        }
        XDialogTheme::Windows => apply_windows_theme(app_instance),
        XDialogTheme::Ubuntu => apply_ubuntu_theme(app_instance),
        XDialogTheme::MacOS => todo!("macOS theme not implemented"),
    }
}

const WINDOWS_BUTTON_BORDER_RADIUS: i32 = 4;
const UBUNTU_BUTTON_BORDER_RADIUS: i32 = 6;

#[derive(Debug, Clone)]
pub struct DialogSpacing {
    pub button_panel_height: i32,
    pub button_spacing: i32,
    pub button_x_padding: i32,
    pub icon_size: i32,
    pub content_margin: i32,
}

fn thin_up_box_windows(x: i32, y: i32, w: i32, h: i32, _: Color) {
    draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::BackGround2);
    draw::begin_line();
    draw::set_draw_color(Color::from_hex(0xDFDFDF));
    draw::draw_line(x, y, x + w, y);
    draw::end_line();
}

fn up_box_windows(x: i32, y: i32, w: i32, h: i32, _: Color) {
    draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::BackGround2);
    draw::draw_rbox(x, y, w, h, WINDOWS_BUTTON_BORDER_RADIUS, true, Color::from_hex(0xFDFDFD));
    draw::draw_rbox(x, y, w, h, WINDOWS_BUTTON_BORDER_RADIUS, false, Color::from_hex(0xD0D0D0));
}

fn down_box_windows(x: i32, y: i32, w: i32, h: i32, _: Color) {
    draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::BackGround2);
    draw::draw_rbox(x, y, w, h, WINDOWS_BUTTON_BORDER_RADIUS, true, Color::from_hex(0xCCE4F7));
    draw::draw_rbox(x, y, w, h, WINDOWS_BUTTON_BORDER_RADIUS, false, Color::from_hex(0x005499));
}

fn engraved_box_windows(x: i32, y: i32, w: i32, h: i32, _: Color) {
    draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::BackGround2);
    draw::draw_rbox(x, y, w, h, WINDOWS_BUTTON_BORDER_RADIUS, true, Color::from_hex(0xE0EEF9));
    draw::draw_rbox(x, y, w, h, WINDOWS_BUTTON_BORDER_RADIUS, false, Color::from_hex(0x0078D4));
}

pub fn apply_windows_theme(app_instance: &App) -> DialogSpacing {
    load_windows_fonts(app_instance);
    app::set_visible_focus(false);
    app::background(255, 255, 255);
    app::background2(0xF0, 0xF0, 0xF0);
    app::set_color(Color::Selection, 0x00, 0x33, 0x99);
    app::set_frame_type_cb(FrameType::ThinUpBox, thin_up_box_windows, 0, 0, 0, 0);
    app::set_frame_type_cb(FrameType::UpBox, up_box_windows, 0, 0, 0, 0);
    app::set_frame_type_cb(FrameType::DownBox, down_box_windows, 0, 0, 0, 0);
    app::set_frame_type_cb(FrameType::EngravedBox, engraved_box_windows, 0, 0, 0, 0);

    DialogSpacing {
        button_panel_height: 41,
        button_spacing: 10,
        button_x_padding: 24,
        icon_size: 32,
        content_margin: 10,
    }
}

fn thin_up_box_ubuntu(_: i32, _: i32, _: i32, _: i32, _: Color) {
    // no-op
}

fn up_box_ubuntu(x: i32, y: i32, w: i32, h: i32, _: Color) {
    draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::BackGround2);
    draw::draw_rbox(x, y, w, h, UBUNTU_BUTTON_BORDER_RADIUS, true, Color::from_hex(0xFFFFFF));
    draw::draw_rbox(x, y, w, h, UBUNTU_BUTTON_BORDER_RADIUS, false, Color::from_hex(0xC7C7C7));
}

fn down_box_ubuntu(x: i32, y: i32, w: i32, h: i32, _: Color) {
    draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::BackGround2);
    draw::draw_rbox(x, y, w, h, UBUNTU_BUTTON_BORDER_RADIUS, true, Color::from_hex(0xE0E0E0));
    draw::set_line_style(LineStyle::Solid, 2);
    draw::draw_rbox(x, y, w, h, UBUNTU_BUTTON_BORDER_RADIUS, false, Color::from_hex(0xE2997F));
    draw::set_line_style(LineStyle::Solid, 1);
    draw::draw_rbox(x + 1, y + 1, w - 2, h - 2, UBUNTU_BUTTON_BORDER_RADIUS, false, Color::from_hex(0xF3AA90));
}

fn engraved_box_ubuntu(x: i32, y: i32, w: i32, h: i32, _: Color) {
    draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::BackGround2);
    draw::draw_rbox(x, y, w, h, UBUNTU_BUTTON_BORDER_RADIUS, true, Color::from_hex(0xF5F5F5));
    draw::draw_rbox(x, y, w, h, UBUNTU_BUTTON_BORDER_RADIUS, false, Color::from_hex(0xC7C7C7));
}

pub fn apply_ubuntu_theme(app_instance: &App) -> DialogSpacing {
    load_ubuntu_fonts(app_instance);
    app::set_visible_focus(false);
    app::background(0xFA, 0xFA, 0xFA);
    app::background2(0xFA, 0xFA, 0xFA);
    app::foreground(0x3D, 0x3D, 0x3D);
    app::set_color(Color::Selection, 0x3D, 0x3D, 0x3D);
    app::set_frame_type_cb(FrameType::ThinUpBox, thin_up_box_ubuntu, 0, 0, 0, 0);
    app::set_frame_type_cb(FrameType::UpBox, up_box_ubuntu, 0, 0, 0, 0);
    app::set_frame_type_cb(FrameType::DownBox, down_box_ubuntu, 0, 0, 0, 0);
    app::set_frame_type_cb(FrameType::EngravedBox, engraved_box_ubuntu, 0, 0, 0, 0);

    DialogSpacing {
        button_panel_height: 48,
        button_spacing: 7,
        button_x_padding: 24,
        icon_size: 48,
        content_margin: 12,
    }
}

pub fn get_theme_icon_svg(icon: MessageBoxIcon) -> Option<&'static str>
{
    match icon {
        MessageBoxIcon::None => None,
        MessageBoxIcon::Error => Some(crate::images::IMAGE_ERROR_SVG),
        MessageBoxIcon::Warning => Some(crate::images::IMAGE_WARNING_SVG),
        MessageBoxIcon::Question => Some(crate::images::IMAGE_INFO_SVG),
        MessageBoxIcon::Information => Some(crate::images::IMAGE_INFO_SVG),
    }
}
