use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;

use crate::model::{XDialogIcon, XDialogOptions, XDialogResult};

pub struct GtkDialog {
    window: gtk4::Window,
    progress_bar: Option<gtk4::ProgressBar>,
    content_label: gtk4::Label,
    is_indeterminate: Rc<Cell<bool>>,
    result_sender: Rc<RefCell<Option<oneshot::Sender<XDialogResult>>>>,
}

impl GtkDialog {
    pub fn new(options: XDialogOptions, has_progress: bool, result_sender: oneshot::Sender<XDialogResult>) -> Self {
        let result_sender = Rc::new(RefCell::new(Some(result_sender)));
        let window = gtk4::Window::new();
        window.set_title(Some(&options.title));
        window.set_default_size(420, -1);
        window.set_resizable(false);
        window.set_modal(true);

        // Root vertical box
        let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        vbox.set_margin_start(18);
        vbox.set_margin_end(18);
        vbox.set_margin_top(18);
        vbox.set_margin_bottom(18);

        // Header area: icon + text side-by-side
        let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);

        let icon_name = match options.icon {
            XDialogIcon::Error => Some("dialog-error"),
            XDialogIcon::Warning => Some("dialog-warning"),
            XDialogIcon::Information => Some("dialog-information"),
            XDialogIcon::None => None,
        };
        if let Some(name) = icon_name {
            let image = gtk4::Image::from_icon_name(name);
            image.set_icon_size(gtk4::IconSize::Large);
            image.set_valign(gtk4::Align::Start);
            hbox.append(&image);
        }

        let text_box = gtk4::Box::new(gtk4::Orientation::Vertical, 6);

        // Main instruction: bold, larger text
        if !options.main_instruction.is_empty() {
            let label = gtk4::Label::new(None);
            label.set_markup(&format!(
                "<span size='large' weight='bold'>{}</span>",
                gtk4::glib::markup_escape_text(&options.main_instruction)
            ));
            label.set_xalign(0.0);
            label.set_wrap(true);
            label.set_max_width_chars(50);
            label.set_selectable(true);
            label.set_focusable(false);
            text_box.append(&label);
        }

        // Body message
        let content_label = gtk4::Label::new(None);
        if !options.message.is_empty() {
            content_label.set_text(&options.message);
        }
        content_label.set_xalign(0.0);
        content_label.set_wrap(true);
        content_label.set_max_width_chars(50);
        content_label.set_selectable(true);
        content_label.set_focusable(false);

        text_box.set_hexpand(true);
        text_box.set_vexpand(true);
        text_box.append(&content_label);

        hbox.append(&text_box);

        hbox.set_hexpand(true);
        hbox.set_vexpand(true);
        vbox.append(&hbox);

        // Progress bar (optional)
        let progress_bar = if has_progress {
            let pb = gtk4::ProgressBar::new();
            pb.set_show_text(false);
            vbox.append(&pb);
            Some(pb)
        } else {
            None
        };

        // Separator before buttons
        if !options.buttons.is_empty() {
            let sep = gtk4::Separator::new(gtk4::Orientation::Horizontal);
            vbox.append(&sep);
        }

        // Buttons
        if !options.buttons.is_empty() {
            let button_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
            button_box.set_halign(gtk4::Align::End);

            let last_idx = options.buttons.len() - 1;
            let mut default_button = None;
            for (idx, text) in options.buttons.iter().enumerate() {
                let button = gtk4::Button::with_label(text);
                if idx == last_idx {
                    button.add_css_class("suggested-action");
                    default_button = Some(button.clone());
                }
                let win = window.clone();
                let rs = result_sender.clone();
                button.connect_clicked(move |_| {
                    if let Some(sender) = rs.borrow_mut().take() {
                        let _ = sender.send(XDialogResult::ButtonPressed(idx));
                    }
                    win.destroy();
                });
                button_box.append(&button);
            }

            vbox.append(&button_box);

            window.set_child(Some(&vbox));

            // Set default button after widget hierarchy is established
            if let Some(ref btn) = default_button {
                window.set_default_widget(Some(btn));
                btn.grab_focus();
            }
        } else {
            window.set_child(Some(&vbox));
        }

        // Handle window close via X button
        let rs = result_sender.clone();
        window.connect_close_request(move |win| {
            if let Some(sender) = rs.borrow_mut().take() {
                let _ = sender.send(XDialogResult::WindowClosed);
            }
            win.destroy();
            gtk4::glib::Propagation::Stop
        });

        window.present();

        let is_indeterminate = Rc::new(Cell::new(false));

        GtkDialog {
            window,
            progress_bar,
            content_label,
            is_indeterminate,
            result_sender,
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

    pub fn close(&self) {
        if let Some(sender) = self.result_sender.borrow_mut().take() {
            let _ = sender.send(XDialogResult::WindowClosed);
        }
        self.window.destroy();
        while gtk4::glib::MainContext::default().iteration(false) {}
    }

    pub fn destroy(self) {
        self.window.destroy();
        while gtk4::glib::MainContext::default().iteration(false) {}
    }
}
