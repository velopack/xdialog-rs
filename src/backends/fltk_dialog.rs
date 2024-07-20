use fltk::{
    *, enums::*, frame::Frame, group::Flex, image::SvgImage, prelude::*,
    window::DoubleWindow,
};
use crate::model::*;
use crate::state::insert_result;

use super::{
    fltk_fonts::*, fltk_button::CustomButton, fltk_progress::CustomProgressBar,
    fltk_theme::{DialogTheme, get_theme_icon_svg}, fltk::Tick,
};

pub struct CustomFltkDialog
{
    id: usize,
    pad_x: i32,
    pad_y: i32,
    wind: DoubleWindow,
    root: Flex,
    data: XDialogOptions,
    progress: Option<CustomProgressBar>,
    buttons: Vec<CustomButton>,
    body_text: Frame,
}

impl CustomFltkDialog {
    pub fn new(id: usize, data: XDialogOptions, theme: &DialogTheme, has_progress: bool) -> CustomFltkDialog {
        let mut wind = DoubleWindow::new(0, 0, 50, 50, data.title.as_str()).center_screen();
        let data2 = data.clone();

        wind.set_callback(move |wnd| {
            wnd.hide();
            insert_result(id, XDialogResult::WindowClosed);
        });

        // Start Root column
        let mut flex_root_col = Flex::default().size_of_parent().column();

        // Start Icon row
        let mut flex_icon_row = Flex::default().row();
        flex_icon_row.set_margin(theme.default_content_margin);

        // Svg Icon
        let mut has_icon = true;
        if let Some(icon_data) = get_theme_icon_svg(data.icon)
        {
            let mut icon_frame = Frame::default();
            if let Ok(mut svg_img) = SvgImage::from_data(icon_data) {
                let svg2 = svg_img.clone();
                svg_img.scale(theme.main_icon_size, theme.main_icon_size, true, true);
                icon_frame.set_image(Some(svg_img));
                flex_icon_row.fixed(&mut icon_frame, theme.main_icon_size);
                wind.set_icon(Some(svg2));
                has_icon = true;
            } else {
                flex_icon_row.fixed(&mut icon_frame, 0);
            }
            icon_frame.set_align(Align::Top | Align::Center | Align::Inside);
        }

        // Start Main column
        let mut flex_main_col = Flex::default().column();
        // flex_main_col.set_spacing(theme.default_content_margin);

        // Main instruction
        let mut main_instr = Frame::default();
        main_instr.set_label(data.main_instruction.as_str());
        main_instr.set_label_size(get_main_instruction_size());
        main_instr.set_label_font(get_main_instruction_font());
        main_instr.set_label_color(theme.color_title_text);
        main_instr.set_align(Align::Left | Align::Inside | Align::Wrap);
        flex_main_col.fixed(&mut main_instr, theme.main_icon_size);

        let mut progress_option: Option<CustomProgressBar> = None;

        if has_progress {
            let mut flex_progress_col = Flex::default().column();
            flex_progress_col.set_margin(3);

            let progress_bar = CustomProgressBar::new(&theme);
            progress_option = Some(progress_bar);

            flex_progress_col.end();
            flex_main_col.fixed(&mut flex_progress_col, 11);
        }

        // Body text
        let mut body_text = Frame::default();
        body_text.set_label(data.message.as_str());
        body_text.set_label_font(get_body_font());
        body_text.set_label_size(get_body_size());
        body_text.set_align(Align::Inside | Align::Wrap | Align::TopLeft);

        // End Main column
        flex_main_col.end();

        // End Icon row
        flex_icon_row.end();

        // Start Button background
        let mut flex_button_background = Flex::default().column();
        flex_button_background.set_frame(FrameType::ThinUpBox);

        // Start Button row
        let mut flex_button_row = Flex::default().row();
        flex_button_row.set_spacing(theme.button_panel_spacing);
        flex_button_row.set_margin(theme.button_panel_margin);

        // Padding frame
        let _ = Frame::default();

        let mut btn_vec = Vec::new();

        // Buttons
        let button_iter: Vec<(usize, &String)> = if theme.button_order_reversed {
            data.buttons.iter().enumerate().rev().collect()
        } else {
            data.buttons.iter().enumerate().collect()
        };
        for (index, button_text) in button_iter {
            let mut wnd_btn_click = wind.clone();
            let mut flex_button_wrapper = Flex::default().column();
            let mut button = CustomButton::new(theme);
            button.set_label(button_text.as_str());
            button.set_label_size(get_body_size());
            button.set_label_font(get_body_font());
            button.set_callback(move |_| {
                wnd_btn_click.hide();
                insert_result(id, XDialogResult::ButtonPressed(index));
            });
            flex_button_wrapper.end();
            let (w, _) = button.measure_label();
            flex_button_row.fixed(&mut flex_button_wrapper, w + (theme.button_text_padding * 2));

            btn_vec.push(button);
        }

        // End Button row
        flex_button_row.end();

        // End Button background
        flex_button_background.end();
        flex_root_col.fixed(&mut flex_button_background, theme.button_panel_height);

        // End Root column
        flex_root_col.end();

        // End Window
        wind.end();

        // Before showing the window, try and compute the optimal window size.
        let icon_width = if has_icon { theme.main_icon_size + theme.default_content_margin } else { 0 };
        let progress_height = if has_progress { 5 + theme.default_content_margin } else { 0 };
        let pad_x = icon_width + (theme.default_content_margin * 2);
        let pad_y = (theme.default_content_margin * 2) + theme.button_panel_height + theme.main_icon_size + progress_height;

        let mut ret = Self {
            id,
            pad_x,
            pad_y,
            wind,
            data: data2,
            root: flex_root_col,
            progress: progress_option,
            buttons: btn_vec,
            body_text,
        };

        ret.auto_size();
        ret.center_screen();
        ret
    }

    pub fn auto_size(&mut self) {
        let wind_size = calculate_ideal_window_size(self.data.message.as_str(), self.pad_x, self.pad_y);
        let new_width = wind_size.0.max(self.wind.width());
        let new_height = wind_size.1.max(self.wind.height());
        self.wind.set_size(new_width, new_height);
        self.root.clone().size_of_parent();
    }

    pub fn center_screen(&mut self) {
        self.wind.clone().center_screen();
    }

    // pub fn hide(&mut self) {
    //     self.wind.clone().hide();
    // }

    pub fn show(&mut self) {
        self.wind.clone().show();
        // wind.set_on_top() - currently has bugs. https://github.com/fltk-rs/fltk-rs/issues/1573
    }

    pub fn close(&mut self) {
        self.wind.clone().hide();
        insert_result(self.id, XDialogResult::WindowClosed);
    }

    pub fn set_progress_value(&mut self, value: f32) {
        if let Some(p) = &mut self.progress {
            p.set_value(value);
        }
    }

    pub fn set_progress_indeterminate(&mut self) {
        if let Some(p) = &mut self.progress {
            p.set_indeterminate();
        }
    }

    pub fn set_body_text(&mut self, text: &str) {
        self.body_text.set_label(text);
        self.data.message = text.to_string();
        self.auto_size();
    }
}

impl Tick for CustomFltkDialog {
    fn tick(&mut self, elapsed_secs: f32) {
        if let Some(p) = &mut self.progress {
            p.tick(elapsed_secs);
        }
        for b in self.buttons.iter_mut() {
            b.tick(elapsed_secs);
        }
    }
}

fn calculate_ideal_window_size(body_text: &str, pad_x: i32, pad_y: i32) -> (i32, i32) {
    let (_, line_height) = draw::measure("A", true);

    draw::set_font(get_body_font(), get_body_size());
    let (initial_width, initial_height) = draw::measure(body_text, true);

    let window_width = if initial_width <= 600 {
        300
    } else if initial_width >= 4000 {
        600
    } else {
        // linear interpolation between 300 (at 600px) and 600 (at 4000px)
        300 + (((initial_width - 600) as f32 / 3400.0) * 300.0) as i32
    };

    // Adjust window width if the initial height is very large
    let height_threshold = 5 * line_height;
    let extra_width = if initial_height > height_threshold {
        (initial_height as f32 / height_threshold as f32 * 50.0).min(300.0) as i32
    } else {
        0
    };

    let final_window_width = (window_width + extra_width).min(600).min(initial_width + pad_y).max(350);
    let (_, wrapped_height) = draw::wrap_measure(body_text, final_window_width - pad_x, true);
    let final_window_height = (wrapped_height + pad_y + line_height).max(130);
    (final_window_width, final_window_height)
}