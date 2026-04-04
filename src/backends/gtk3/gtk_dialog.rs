use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;

use crate::model::*;
use crate::state::insert_result;

pub struct GtkDialog {
    window: gtk::Window,
    progress_bar: Option<gtk::ProgressBar>,
    content_label: gtk::Label,
    is_indeterminate: Rc<Cell<bool>>,
}

impl GtkDialog {
    pub fn new(id: usize, options: XDialogOptions, has_progress: bool) -> Self {
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title(&options.title);
        window.set_default_size(420, -1);
        window.set_resizable(false);
        window.set_position(gtk::WindowPosition::Center);
        window.set_keep_above(true);

        // Root vertical box
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 12);
        vbox.set_margin_start(18);
        vbox.set_margin_end(18);
        vbox.set_margin_top(18);
        vbox.set_margin_bottom(18);

        // Header area: icon + text side-by-side
        let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 12);

        let icon_name = match options.icon {
            XDialogIcon::Error => Some("dialog-error"),
            XDialogIcon::Warning => Some("dialog-warning"),
            XDialogIcon::Information => Some("dialog-information"),
            XDialogIcon::None => None,
        };
        if let Some(name) = icon_name {
            let image = gtk::Image::from_icon_name(Some(name), gtk::IconSize::Dialog);
            image.set_valign(gtk::Align::Start);
            hbox.pack_start(&image, false, false, 0);
        }

        let text_box = gtk::Box::new(gtk::Orientation::Vertical, 6);

        // Main instruction: bold, larger text
        if !options.main_instruction.is_empty() {
            let label = gtk::Label::new(None);
            label.set_markup(&format!(
                "<span size='large' weight='bold'>{}</span>",
                glib::markup_escape_text(&options.main_instruction)
            ));
            label.set_xalign(0.0);
            label.set_line_wrap(true);
            label.set_max_width_chars(50);
            label.set_selectable(true);
            label.set_can_focus(false);
            text_box.pack_start(&label, false, false, 0);
        }

        // Body message
        let content_label = gtk::Label::new(None);
        if !options.message.is_empty() {
            content_label.set_text(&options.message);
        }
        content_label.set_xalign(0.0);
        content_label.set_line_wrap(true);
        content_label.set_max_width_chars(50);
        content_label.set_selectable(true);
        content_label.set_can_focus(false);
        text_box.pack_start(&content_label, false, false, 0);

        hbox.pack_start(&text_box, true, true, 0);
        vbox.pack_start(&hbox, true, true, 0);

        // Progress bar (optional)
        let progress_bar = if has_progress {
            let pb = gtk::ProgressBar::new();
            pb.set_show_text(false);
            vbox.pack_start(&pb, false, false, 0);
            Some(pb)
        } else {
            None
        };

        // Separator before buttons
        if !options.buttons.is_empty() {
            let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
            vbox.pack_start(&sep, false, false, 0);
        }

        // Buttons
        if !options.buttons.is_empty() {
            let button_box = gtk::ButtonBox::new(gtk::Orientation::Horizontal);
            button_box.set_layout(gtk::ButtonBoxStyle::End);
            button_box.set_spacing(6);

            for (idx, text) in options.buttons.iter().enumerate() {
                let button = gtk::Button::with_label(text);
                if idx == 0 {
                    button.style_context().add_class("suggested-action");
                }
                let win = window.clone();
                button.connect_clicked(move |_| {
                    insert_result(id, XDialogResult::ButtonPressed(idx));
                    unsafe { win.destroy(); }
                });
                button_box.pack_start(&button, false, false, 0);
            }

            vbox.pack_start(&button_box, false, false, 0);
        }

        window.add(&vbox);

        // Handle window close via X button
        window.connect_delete_event(move |win, _| {
            insert_result(id, XDialogResult::WindowClosed);
            unsafe { win.destroy(); }
            glib::Propagation::Stop
        });

        window.show_all();

        let is_indeterminate = Rc::new(Cell::new(false));

        GtkDialog {
            window,
            progress_bar,
            content_label,
            is_indeterminate,
        }
    }

    pub fn set_progress_value(&self, value: f32) {
        if let Some(ref pb) = self.progress_bar {
            pb.set_fraction(value as f64);
            self.is_indeterminate.set(false);
        }
    }

    pub fn set_progress_indeterminate(&self) {
        self.is_indeterminate.set(true);
    }

    pub fn set_progress_text(&self, text: &str) {
        self.content_label.set_text(text);
    }

    pub fn pulse_if_indeterminate(&self) {
        if self.is_indeterminate.get() {
            if let Some(ref pb) = self.progress_bar {
                pb.pulse();
            }
        }
    }

    pub fn close(&self, id: usize) {
        insert_result(id, XDialogResult::WindowClosed);
        unsafe { self.window.destroy(); }
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
    }

    pub fn destroy(self) {
        unsafe { self.window.destroy(); }
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
    }
}
