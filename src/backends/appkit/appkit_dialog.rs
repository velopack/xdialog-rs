use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2::MainThreadMarker;
use objc2_app_kit::*;
use objc2_foundation::*;

use crate::model::*;

// Layout constants
const WINDOW_MIN_WIDTH: f64 = 350.0;
const WINDOW_MAX_WIDTH: f64 = 500.0;
const WINDOW_PADDING: f64 = 20.0;
const ICON_SIZE: f64 = 64.0;
const ICON_PROGRESS_SIZE: f64 = 48.0;
const BUTTON_HEIGHT: f64 = 24.0;
const BUTTON_MIN_WIDTH: f64 = 80.0;
const BUTTON_SPACING: f64 = 8.0;
const BUTTON_PANEL_HEIGHT: f64 = 52.0;
const TEXT_SPACING: f64 = 8.0;
const PROGRESS_HEIGHT: f64 = 20.0;
const TITLE_FONT_SIZE: f64 = 13.0;
const BODY_FONT_SIZE: f64 = 11.0;

pub struct AppKitDialog {
    #[allow(dead_code)]
    id: usize,
    window: Retained<NSWindow>,
    title_field: Option<Retained<NSTextField>>,
    body_field: Option<Retained<NSTextField>>,
    progress: Option<Retained<NSProgressIndicator>>,
    icon_view: Option<Retained<NSImageView>>,
    buttons: Vec<Retained<NSButton>>,
    has_progress: bool,
    options: XDialogOptions,
}

impl AppKitDialog {
    pub fn new(
        id: usize,
        options: XDialogOptions,
        has_progress: bool,
        handler: &AnyObject,
    ) -> Self {
        let mtm = unsafe { MainThreadMarker::new_unchecked() };
        let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable;

        let window = unsafe {
            let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(400.0, 200.0));
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc::<NSWindow>(),
                rect,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };

        window.setTitle(&NSString::from_str(&options.title));
        unsafe { window.setReleasedWhenClosed(false) };

        let content_view = window.contentView().unwrap();

        // Icon
        let icon_image = get_icon_image(&options.icon);
        let icon_view = icon_image.map(|image| {
            let size = if has_progress { ICON_PROGRESS_SIZE } else { ICON_SIZE };
            let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(size, size));
            let iv = NSImageView::initWithFrame(mtm.alloc::<NSImageView>(), frame);
            iv.setImage(Some(&image));
            iv.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
            content_view.addSubview(&iv);
            iv
        });

        // Title
        let title_field = if !options.main_instruction.is_empty() {
            let field = create_label(&options.main_instruction, true, mtm);
            content_view.addSubview(&field);
            Some(field)
        } else {
            None
        };

        // Progress bar
        let progress = if has_progress {
            let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(100.0, PROGRESS_HEIGHT));
            let p = NSProgressIndicator::initWithFrame(
                mtm.alloc::<NSProgressIndicator>(),
                frame,
            );
            p.setStyle(NSProgressIndicatorStyle::Bar);
            p.setMinValue(0.0);
            p.setMaxValue(1.0);
            p.setDoubleValue(0.0);
            p.setIndeterminate(false);
            content_view.addSubview(&p);
            Some(p)
        } else {
            None
        };

        // Body text
        let body_field = if !options.message.is_empty() {
            let field = create_label(&options.message, false, mtm);
            content_view.addSubview(&field);
            Some(field)
        } else {
            None
        };

        // Buttons (iterate in reverse so rightmost button is last/default)
        let button_count = options.buttons.len();
        let mut buttons = Vec::new();
        for (index, button_text) in options.buttons.iter().enumerate().rev() {
            let btn = NSButton::initWithFrame(
                mtm.alloc::<NSButton>(),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(BUTTON_MIN_WIDTH, BUTTON_HEIGHT)),
            );
            #[allow(deprecated)]
            btn.setBezelStyle(NSBezelStyle::Rounded);
            btn.setTitle(&NSString::from_str(button_text));

            let tag = ((id << 16) | index) as isize;
            btn.setTag(tag);

            unsafe { btn.setTarget(Some(handler)) };
            unsafe { btn.setAction(Some(sel!(buttonClicked:))) };

            // Last button (highest index) is the default/return key
            if index == button_count - 1 {
                btn.setKeyEquivalent(&NSString::from_str("\r"));
            }
            // First button gets escape key (if more than one button)
            if index == 0 && button_count > 1 {
                btn.setKeyEquivalent(&NSString::from_str("\u{1b}"));
            }

            content_view.addSubview(&btn);
            buttons.push(btn);
        }
        buttons.reverse(); // put back in original order

        let mut dialog = Self {
            id,
            window,
            title_field,
            body_field,
            progress,
            icon_view,
            buttons,
            has_progress,
            options,
        };

        dialog.layout();
        dialog
    }

    fn layout(&mut self) {
        let title_font = NSFont::boldSystemFontOfSize(TITLE_FONT_SIZE);
        let body_font = NSFont::systemFontOfSize(BODY_FONT_SIZE);

        let has_icon = self.icon_view.is_some();
        let icon_size = if self.has_progress { ICON_PROGRESS_SIZE } else { ICON_SIZE };

        let mut content_width = WINDOW_MIN_WIDTH - WINDOW_PADDING * 2.0;

        // For progress with icon, text is narrower
        let text_area_width = if self.has_progress && has_icon {
            content_width - icon_size - WINDOW_PADDING
        } else {
            content_width
        };

        // Measure text
        let title_height = if !self.options.main_instruction.is_empty() {
            measure_text_height(&self.options.main_instruction, &title_font, text_area_width)
        } else {
            0.0
        };
        let body_height = if !self.options.message.is_empty() {
            measure_text_height(&self.options.message, &body_font, text_area_width)
        } else {
            0.0
        };

        // Widen if text is very tall
        if title_height + body_height > 150.0 {
            content_width = (content_width + 100.0).min(WINDOW_MAX_WIDTH - WINDOW_PADDING * 2.0);
        }

        let text_area_width = if self.has_progress && has_icon {
            content_width - icon_size - WINDOW_PADDING
        } else {
            content_width
        };

        // Re-measure at final width
        let title_height = if !self.options.main_instruction.is_empty() {
            measure_text_height(&self.options.main_instruction, &title_font, text_area_width)
        } else {
            0.0
        };
        let body_height = if !self.options.message.is_empty() {
            measure_text_height(&self.options.message, &body_font, text_area_width)
        } else {
            0.0
        };

        // Compute total height (macOS origin is bottom-left)
        let mut total_height = WINDOW_PADDING;

        if !self.has_progress && has_icon {
            total_height += icon_size + TEXT_SPACING;
        }
        if title_height > 0.0 {
            total_height += title_height + TEXT_SPACING;
        }
        if self.has_progress {
            total_height += PROGRESS_HEIGHT + TEXT_SPACING;
        }
        if body_height > 0.0 {
            total_height += body_height + TEXT_SPACING;
        }
        if !self.options.buttons.is_empty() {
            total_height += BUTTON_PANEL_HEIGHT;
        }
        total_height += WINDOW_PADDING;

        // Ensure minimum height for progress icon
        if self.has_progress && has_icon {
            let min_h = icon_size + WINDOW_PADDING * 2.0
                + if !self.options.buttons.is_empty() { BUTTON_PANEL_HEIGHT } else { 0.0 };
            if total_height < min_h {
                total_height = min_h;
            }
        }

        let window_width = content_width + WINDOW_PADDING * 2.0;

        // Resize window (preserve top-left position)
        let frame = self.window.frame();
        let new_frame = NSRect::new(
            NSPoint::new(frame.origin.x, frame.origin.y + frame.size.height - total_height),
            NSSize::new(window_width, total_height),
        );
        self.window.setFrame_display(new_frame, true);

        // Position subviews top-down
        let mut y = total_height - WINDOW_PADDING;

        // Message dialog: icon centered above text
        if !self.has_progress && has_icon {
            if let Some(ref iv) = self.icon_view {
                let icon_x = (window_width - icon_size) / 2.0;
                y -= icon_size;
                iv.setFrame(NSRect::new(
                    NSPoint::new(icon_x, y),
                    NSSize::new(icon_size, icon_size),
                ));
                y -= TEXT_SPACING;
            }
        }

        // Progress dialog: icon on left
        let text_x = if self.has_progress && has_icon {
            if let Some(ref iv) = self.icon_view {
                let icon_y = y - icon_size;
                iv.setFrame(NSRect::new(
                    NSPoint::new(WINDOW_PADDING, icon_y),
                    NSSize::new(icon_size, icon_size),
                ));
            }
            WINDOW_PADDING + icon_size + WINDOW_PADDING
        } else {
            WINDOW_PADDING
        };

        // Title
        if let Some(ref tf) = self.title_field {
            y -= title_height;
            tf.setFrame(NSRect::new(
                NSPoint::new(text_x, y),
                NSSize::new(text_area_width, title_height),
            ));
            y -= TEXT_SPACING;
        }

        // Progress bar
        if let Some(ref p) = self.progress {
            y -= PROGRESS_HEIGHT;
            p.setFrame(NSRect::new(
                NSPoint::new(text_x, y),
                NSSize::new(text_area_width, PROGRESS_HEIGHT),
            ));
            y -= TEXT_SPACING;
        }

        // Body text
        if let Some(ref bf) = self.body_field {
            y -= body_height;
            bf.setFrame(NSRect::new(
                NSPoint::new(text_x, y),
                NSSize::new(text_area_width, body_height),
            ));
        }

        // Buttons - right-aligned at bottom
        if !self.buttons.is_empty() {
            let button_y = WINDOW_PADDING;
            let mut btn_x = window_width - WINDOW_PADDING;

            for btn in self.buttons.iter().rev() {
                let title_width = measure_button_width(btn);
                let btn_width = (title_width + 30.0).max(BUTTON_MIN_WIDTH);
                btn_x -= btn_width;
                btn.setFrame(NSRect::new(
                    NSPoint::new(btn_x, button_y),
                    NSSize::new(btn_width, BUTTON_HEIGHT),
                ));
                btn_x -= BUTTON_SPACING;
            }
        }
    }

    pub fn show(&self) {
        self.window.center();
        self.window.makeKeyAndOrderFront(None);
        unsafe {
            let mtm = MainThreadMarker::new_unchecked();
            let app = NSApplication::sharedApplication(mtm);
            #[allow(deprecated)]
            app.activateIgnoringOtherApps(true);
        }
    }

    pub fn is_visible(&self) -> bool {
        self.window.isVisible()
    }

    pub fn close(&self) {
        self.window.orderOut(None);
    }

    pub fn set_progress_value(&self, value: f32) {
        if let Some(ref p) = self.progress {
            if p.isIndeterminate() {
                p.setIndeterminate(false);
                unsafe { p.stopAnimation(None) };
            }
            p.setDoubleValue(value as f64);
        }
    }

    pub fn set_progress_indeterminate(&self) {
        if let Some(ref p) = self.progress {
            p.setIndeterminate(true);
            unsafe { p.startAnimation(None) };
        }
    }

    pub fn set_body_text(&mut self, text: &str) {
        self.options.message = text.to_string();

        if let Some(ref bf) = self.body_field {
            bf.setStringValue(&NSString::from_str(text));
        } else if !text.is_empty() {
            let mtm = unsafe { MainThreadMarker::new_unchecked() };
            let field = create_label(text, false, mtm);
            self.window.contentView().unwrap().addSubview(&field);
            self.body_field = Some(field);
        }

        self.layout();
    }
}

fn get_icon_image(icon: &XDialogIcon) -> Option<Retained<NSImage>> {
    let name = match icon {
        XDialogIcon::None => return None,
        XDialogIcon::Error | XDialogIcon::Warning => "NSCaution",
        XDialogIcon::Information => "NSInfo",
    };
    NSImage::imageNamed(&NSString::from_str(name))
}

fn create_label(text: &str, bold: bool, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(100.0, 20.0));
    let field = NSTextField::initWithFrame(mtm.alloc::<NSTextField>(), frame);

    field.setStringValue(&NSString::from_str(text));
    field.setBezeled(false);
    field.setDrawsBackground(false);
    field.setEditable(false);
    field.setSelectable(false);

    if bold {
        field.setFont(Some(&NSFont::boldSystemFontOfSize(TITLE_FONT_SIZE)));
    } else {
        field.setFont(Some(&NSFont::systemFontOfSize(BODY_FONT_SIZE)));
    }

    // Enable word wrapping
    unsafe {
        let cell: Option<Retained<AnyObject>> = msg_send![&field, cell];
        if let Some(cell) = cell {
            let () = msg_send![&*cell, setWraps: true];
            let () = msg_send![&*cell, setLineBreakMode: 0u64]; // NSLineBreakByWordWrapping
        }
    }

    field
}

fn measure_text_height(text: &str, font: &NSFont, width: f64) -> f64 {
    unsafe {
        let ns_string = NSString::from_str(text);
        let font_key = NSFontAttributeName;
        let font_obj: Retained<AnyObject> = msg_send![font, self];
        let attrs = NSDictionary::from_slices(&[font_key], &[&*font_obj]);
        let rect = ns_string.boundingRectWithSize_options_attributes_context(
            NSSize::new(width, f64::MAX),
            NSStringDrawingOptions::UsesLineFragmentOrigin
                | NSStringDrawingOptions::UsesFontLeading,
            Some(&attrs),
            None,
        );
        rect.size.height.ceil()
    }
}

fn measure_button_width(btn: &NSButton) -> f64 {
    unsafe {
        let title: Retained<NSString> = msg_send![btn, title];
        let font = NSFont::systemFontOfSize(BODY_FONT_SIZE);
        let font_obj: Retained<AnyObject> = msg_send![&*font, self];
        let attrs = NSDictionary::from_slices(&[NSFontAttributeName], &[&*font_obj]);
        let size = title.sizeWithAttributes(Some(&attrs));
        size.width
    }
}
