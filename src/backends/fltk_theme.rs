use fltk::{app, draw};
use fltk::enums::{Color, FrameType};
use crate::model::MessageBoxIcon;
use super::fltk_fonts::*;

const BUTTON_BORDER_RADIUS: i32 = 5;

fn thin_up_box_windows(x: i32, y: i32, w: i32, h: i32, _: Color) {
    draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::BackGround2);
    draw::begin_line();
    draw::set_draw_color(Color::from_hex(0xDFDFDF));
    draw::draw_line(x, y, x + w, y);
    draw::end_line();
}

fn up_box_windows(x: i32, y: i32, w: i32, h: i32, _: Color) {
    draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::BackGround2);
    draw::draw_rbox(x, y, w, h, BUTTON_BORDER_RADIUS, true, Color::from_hex(0xFDFDFD));
    draw::draw_rbox(x, y, w, h, BUTTON_BORDER_RADIUS, false, Color::from_hex(0xD0D0D0));
}

fn down_box_windows(x: i32, y: i32, w: i32, h: i32, _: Color) {
    draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::BackGround2);
    draw::draw_rbox(x, y, w, h, BUTTON_BORDER_RADIUS, true, Color::from_hex(0xCCE4F7));
    draw::draw_rbox(x, y, w, h, BUTTON_BORDER_RADIUS, false, Color::from_hex(0x005499));
}

fn engraved_box_windows(x: i32, y: i32, w: i32, h: i32, _: Color) {
    draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::BackGround2);
    draw::draw_rbox(x, y, w, h, BUTTON_BORDER_RADIUS, true, Color::from_hex(0xE0EEF9));
    draw::draw_rbox(x, y, w, h, BUTTON_BORDER_RADIUS, false, Color::from_hex(0x0078D4));
}

pub fn apply_windows_theme() {
    load_windows_fonts();
    app::set_visible_focus(false);
    app::background(255, 255, 255);
    app::background2(0xF0, 0xF0, 0xF0);
    app::set_frame_type_cb(FrameType::ThinUpBox, thin_up_box_windows, 0, 0, 0, 0);
    app::set_frame_type_cb(FrameType::UpBox, up_box_windows, 0, 0, 0, 0);
    app::set_frame_type_cb(FrameType::DownBox, down_box_windows, 0, 0, 0, 0);
    app::set_frame_type_cb(FrameType::EngravedBox, engraved_box_windows, 0, 0, 0, 0);
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
