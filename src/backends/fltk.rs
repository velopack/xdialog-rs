use std::sync::mpsc::Receiver;
use std::thread;

use fltk::{
    *, app, button::Button, enums::*,
    frame::Frame, group::Flex, image::SvgImage, prelude::*, window::Window,
};
use fltk::window::DoubleWindow;
use fltk_theme::{ThemeType, WidgetTheme};

use crate::model::{DialogMessageRequest, MessageBoxData, MessageBoxResult};
use crate::state::insert_result;
use super::fltk_fonts::*;

fn down_box_windows(x: i32, y: i32, w: i32, h: i32, c: Color) {
    draw::draw_box(FrameType::FlatBox, x, y, w, h, Color::from_hex(0xF0F0F0));
    draw::begin_line();
    draw::set_draw_color(Color::from_hex(0xDFDFDF));
    draw::draw_line(x, y, x + w, y);
    draw::end_line();
}

pub fn run_fltk_backend(main: fn() -> u16, receiver: Receiver<DialogMessageRequest>)
{
    #[cfg(windows)]
    {
        app::App::default();
        let widget_theme = WidgetTheme::new(ThemeType::Metro);
        widget_theme.apply();
        app::set_frame_type_cb(FrameType::PlasticDownBox, down_box_windows, 0, 0, 0, 0);
    }

    #[cfg(target_os = "linux")]
    app::App::default().with_scheme(app::Scheme::Gtk);

    load_fonts();

    let t = thread::spawn(move || {
        main();
    });

    loop {
        if let Err(e) = app::wait_for(0.1) {
            error!("xdialog event loop fatal error: {:?}", e);
            break;
        }

        if t.is_finished() {
            app::quit();
            break;
        }

        // TODO: clean up finished message box windows with window::Window::delete(hWnd);

        loop {
            // read all messages until there are no more queued
            let message = receiver.try_recv().unwrap_or(DialogMessageRequest::None);
            if message == DialogMessageRequest::None {
                break;
            }

            match message {
                DialogMessageRequest::ShowMessageBox(id, data) => {
                    create_messagebox(id, data);
                }
                DialogMessageRequest::ExitEventLoop => {
                    app::quit();
                    break;
                }
                _ => debug!("Unhandled xdialog message type: {:?}", message),
            }
        }
    }
}

fn create_messagebox(id: usize, data: MessageBoxData) -> DoubleWindow {
    let mut wind = Window::new(0, 0, 400, 300, data.title.as_str()).center_screen();
    wind.set_color(Color::White);

    // Start Root column
    let mut flex_root_col = Flex::default().size_of_parent().column();

    // Start Icon row
    let mut flex_icon_row = Flex::default().row();
    flex_icon_row.set_margin(10);

    // Svg Icon
    let icon_data = crate::images::IMAGE_INFO_SVG;
    let mut icon_frame = Frame::default();
    if let Ok(mut svg_img) = SvgImage::from_data(icon_data) {
        let svg2 = svg_img.clone();
        svg_img.scale(36, 36, true, true);
        icon_frame.set_image(Some(svg_img));
        flex_icon_row.fixed(&mut icon_frame, 36);
        wind.set_icon(Some(svg2));
    } else {
        flex_icon_row.fixed(&mut icon_frame, 0);
    }
    icon_frame.set_align(Align::Top | Align::Center | Align::Inside);

    // Start Main column
    let mut flex_main_col = Flex::default().column();
    flex_main_col.set_spacing(10);

    // Main instruction
    let mut main_instr = Frame::default();
    main_instr.set_label(data.main_instruction.as_str());
    main_instr.set_label_size(get_main_instruction_size());
    main_instr.set_label_font(get_main_instruction_font());
    main_instr.set_label_color(Color::from_hex(0x003399));
    main_instr.set_align(Align::Left | Align::Inside);
    flex_main_col.fixed(&mut main_instr, 32);

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
    flex_button_background.set_frame(FrameType::PlasticDownBox);

    // Start Button row
    let mut flex_button_row = Flex::default().row();
    flex_button_row.set_spacing(10);
    flex_button_row.set_margin(10);

    // Padding frame
    let _ = Frame::default();

    // Buttons
    for (index, button_text) in data.buttons.iter().enumerate() {
        let mut button = Button::default();
        button.set_label(button_text.as_str());
        let (w, _) = button.measure_label();
        flex_button_row.fixed(&mut button, w + 40);

        // handle hover cursor events
        let mut wnd_btn_hover = wind.clone();
        button.handle(move |_, event| {
            if event == Event::Enter {
                wnd_btn_hover.set_cursor(Cursor::Hand);
            }
            if event == Event::Leave {
                wnd_btn_hover.set_cursor(Cursor::Arrow);
            }
            false
        });

        let mut wnd_btn_click = wind.clone();
        button.set_callback(move |_| {
            wnd_btn_click.hide();
            insert_result(id, MessageBoxResult::ButtonPressed(index));
        });
    }


    // End Button row
    flex_button_row.end();

    // End Button background
    flex_button_background.end();
    flex_root_col.fixed(&mut flex_button_background, 42);

    // End Root column
    flex_root_col.end();

    // End Window
    wind.end();

    // Before showing the window, try and compute the optimal window size.
    let wind_size = calculate_ideal_window_size(data.message.as_str());
    wind.set_size(wind_size.0, wind_size.1);
    let mut wind = wind.center_screen();
    flex_root_col.size_of_parent();

    // Show window
    wind.show();
    wind
}

fn calculate_ideal_window_size(body_text: &str) -> (i32, i32) {
    let (_, line_height) = draw::measure("A", true);

    draw::set_font(get_body_font(), get_body_size());
    let (initial_width, initial_height) = draw::measure(body_text, true);

    let window_width = if initial_width <= 600 {
        300
    } else if initial_width >= 4000 {
        600
    } else {
        // New linear interpolation between 300 (at 600px) and 600 (at 4000px)
        300 + (((initial_width - 600) as f32 / 3400.0) * 300.0) as i32
    };

    // Adjust window width if the initial height is very large
    // For instance, increase width by a percentage if height exceeds a certain number of line heights
    let height_threshold = 5 * line_height;  // Arbitrary threshold: adjust based on your UI needs
    let extra_width = if initial_height > height_threshold {
        (initial_height as f32 / height_threshold as f32 * 50.0).min(300.0) as i32
    } else {
        0
    };

    let final_window_width = (window_width + extra_width).min(600).min(initial_width + 100).max(300);
    let (_, wrapped_height) = draw::wrap_measure(body_text, final_window_width - 70, true);
    let final_window_height = (wrapped_height + 100 + line_height).max(130);
    (final_window_width, final_window_height)
}