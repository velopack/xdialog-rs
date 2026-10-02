//! macOS theme: the alert of macOS 11 (Big Sur) to 15 (Sequoia), as NSAlert and
//! CFUserNotification show it, built from the theme's own widgets (`widgets.rs`: push button,
//! progress bar, fallback icons, scroller).
//!
//! Layout (points, measured on Sequoia):
//!
//! ```text
//! ┌──────────────────────────┐  260 wide, no title bar (the window's is hidden)
//! │ 20                       │
//! │         [icon 64]        │  app / severity / custom icon, centred
//! │ 33 to the first baseline │
//! │    Title (13 pt bold)    │  centred, wraps at 220, pitch 16
//! │ 23.5 baseline to baseline│
//! │  Body (11 pt), centred,  │  pitch 14
//! │  wrapping at 220         │
//! │ 14   ▬▬▬▬▬▬▬▬▬▬▬▬▬▬      │  progress bar (6), then 16
//! │ 19.5                     │
//! │ [ Cancel ] 8 [   OK   ]  │  28 tall, 16 from the sides and the bottom
//! └──────────────────────────┘
//! ```
//!
//! Buttons: two side by side in API order (the default, last API index, on the right) when both
//! labels fit in half the row; otherwise (and always with three or more) stacked full width,
//! default on top (reversed API order). The default button is the accent button in message
//! dialogs whatever has focus, and Return activates it (AppKit's key equivalent); Space activates
//! the focused button, whose focus ring only appears after Tab. Without a heading the window
//! title is the alert's title (as CFUserNotification does). A body too tall for the screen
//! scrolls.

mod tokens;
mod widgets;

use std::rc::Rc;

use self::tokens::{MacTokens, BODY_SIZE, BUTTON_SIZE, FONTS, TITLE_SIZE};
use self::widgets::{BUTTON_H, PROGRESS_H};
use crate::backends::draw::{Color, Point, Rect, Size};
use crate::backends::gui::appearance::Appearance;
use crate::backends::gui::text::{TextBlock, TextStyle};
use crate::backends::gui::theme::*;
use crate::backends::gui::ui::{columns, Column};

/// Window width; text column inset (wraps at `WIDTH - 2 * TEXT_INSET`); button inset (sides and
/// bottom).
const WIDTH: f64 = 260.0;
const TEXT_INSET: f64 = 20.0;
const PAD: f64 = 16.0;
/// Icon side and its distance from the top.
const ICON_SIZE: f64 = 64.0;
const ICON_TOP: f64 = 20.0;
/// Baseline distances: icon bottom (or window top + `ICON_TOP` without an icon) to the first
/// baseline; title's last baseline to the body's first; last text baseline to the buttons (or
/// to the progress bar).
const FIRST_BASELINE: f64 = 33.0;
const TITLE_TO_BODY: f64 = 23.5;
const TEXT_TO_BUTTONS: f64 = 19.5;
const TEXT_TO_PROGRESS: f64 = 14.0;
const PROGRESS_TO_BUTTONS: f64 = 16.0;
/// Gap between buttons (side by side and stacked).
const BUTTON_GAP: f64 = 8.0;
/// Horizontal label padding inside a button (a label wider than its button minus this is elided
/// or stacks the buttons).
const BUTTON_PAD_X: f64 = 12.0;
/// Smallest scrolling body viewport.
const MIN_VIEWPORT: f64 = 42.0;

/// Keyboard policy (AppKit): the focus ring only after Tab, Return activates the default button,
/// Space activates the focused button on release, Tab wraps, arrows don't move focus between
/// push buttons (clamped), PageUp/PageDown/Home/End scroll the body.
pub(crate) const KEYBOARD: KeyboardPolicy = KeyboardPolicy { focus_visibility: FocusVisibility::KeyboardNavOnly,
                                                             arrows: ArrowNav::Clamp,
                                                             enter_falls_back_to_default: true,
                                                             enter_activates_default: true,
                                                             space: SpaceKey::ActivateOnRelease,
                                                             scroll_keys: true };

/// The macOS theme.
pub(crate) struct MacTheme {
    tokens: MacTokens,
    /// Body scroll offset, logical px.
    scroll: f64,
}

impl MacTheme {
    pub(crate) fn new() -> Self {
        MacTheme { tokens: MacTokens::new(&Appearance::default()), scroll: 0.0 }
    }
}

/// The last baseline of `block` below its top, for a block of `pitch` spaced lines.
fn last_baseline(block: &TextBlock, pitch: f64) -> f64 {
    first_baseline(block) + (block.lines.max(1) - 1) as f64 * pitch
}

fn first_baseline(block: &TextBlock) -> f64 {
    block.paras.first().map_or(0.0, |(l, at)| at.y + crate::backends::draw::TextLayout::first_baseline(l))
}

impl Theme for MacTheme {
    fn set_appearance(&mut self, appearance: &Appearance) {
        self.tokens = MacTokens::new(appearance);
    }

    fn keyboard_policy(&self) -> KeyboardPolicy {
        KEYBOARD
    }

    fn fonts(&self) -> &'static ThemeFonts {
        &FONTS
    }

    fn icon_size(&self) -> f64 {
        ICON_SIZE
    }

    fn clear_color(&self) -> Color {
        self.tokens.bg
    }

    /// The alert material shows through untinted.
    fn translucent_clear(&self) -> Option<Color> {
        Some(Color::TRANSPARENT)
    }

    /// The system's alert icons (the app icon for `Information`).
    #[cfg(target_os = "macos")]
    fn system_icon(&self, icon: &crate::model::XDialogIcon) -> Option<std::sync::Arc<crate::icon::IconFile>> {
        crate::backends::gui::platform_mac::alert_icon(icon)
    }

    fn ui(&mut self, view: &DialogView<'_>, ui: &mut Ui<'_>) -> DialogUiOutput {
        let tk = self.tokens.clone();
        let title_style = TextStyle::bold(TITLE_SIZE).centered();
        let body_style = TextStyle::regular(BODY_SIZE).centered();
        let label_style = TextStyle::regular(BUTTON_SIZE);
        let (title_pitch, body_pitch) = (tokens::line_height(TITLE_SIZE), tokens::line_height(BODY_SIZE));
        let mut out = DialogUiOutput::default();

        // ---- measure ----
        let text_w = WIDTH - 2.0 * TEXT_INSET;
        let heading = if view.heading.is_empty() { view.title } else { view.heading };
        let title = (!heading.is_empty()).then(|| ui.layout(heading, &title_style, text_w, None));
        let body = (!view.body.is_empty()).then(|| ui.layout(view.body, &body_style, text_w, None));
        let n = view.buttons.len();
        let row = WIDTH - 2.0 * PAD;
        let half = (row - BUTTON_GAP) / 2.0;
        let labels: Vec<Rc<TextBlock>> = view.buttons.iter().map(|b| ui.layout(b, &label_style, f64::INFINITY, Some(1))).collect();
        let stacked = n > 2 || labels.iter().any(|l| l.size.width + 2.0 * BUTTON_PAD_X > half);
        // Labels wider than their button are elided to it.
        let label_max = if stacked || n == 1 { row } else { half } - 2.0 * BUTTON_PAD_X;
        let labels: Vec<Rc<TextBlock>> =
            labels.into_iter()
                  .zip(view.buttons)
                  .map(|(l, b)| if l.size.width > label_max { ui.layout(b, &label_style, label_max, Some(1)) } else { l })
                  .collect();
        let buttons_h = match n {
            0 => 0.0,
            _ if stacked => n as f64 * BUTTON_H + (n - 1) as f64 * BUTTON_GAP,
            _ => BUTTON_H,
        };

        // ---- vertical layout: baselines first, then boxes ----
        let icon = view.has_icon();
        let y = if icon { ICON_TOP + ICON_SIZE } else { ICON_TOP };
        // Where the next text block's first baseline goes, and the last baseline so far.
        let mut next_baseline = y + FIRST_BASELINE - if icon { 0.0 } else { ICON_TOP };
        let mut last = None;
        let title_top = title.as_ref().map(|t| {
                                          let top = next_baseline - first_baseline(t);
                                          let lb = top + last_baseline(t, title_pitch);
                                          last = Some(lb);
                                          next_baseline = lb + TITLE_TO_BODY;
                                          top
                                      });
        // The body: a viewport that scrolls when the whole dialog would exceed the height limit.
        let body_top = body.as_ref().map(|b| next_baseline - first_baseline(b));
        let tail = |text_bottom: f64| {
            // Height below the last baseline: progress bar and buttons.
            let mut h = 0.0;
            if view.progress.is_some() {
                h += TEXT_TO_PROGRESS + PROGRESS_H + if n > 0 { PROGRESS_TO_BUTTONS } else { PAD };
            } else if n > 0 {
                h += TEXT_TO_BUTTONS;
            } else {
                h += PAD;
            }
            text_bottom + h + if n > 0 { buttons_h + PAD } else { 0.0 }
        };
        let (mut viewport_h, mut content_h) = (0.0, 0.0);
        if let (Some(b), Some(top)) = (&body, body_top) {
            content_h = b.size.height;
            let lb_in = last_baseline(b, body_pitch);
            let natural = tail(top + lb_in);
            let excess = (natural - view.max_height).max(0.0);
            viewport_h = (content_h - excess).max(MIN_VIEWPORT.min(content_h));
            // The last visible baseline keeps its distance to what follows.
            last = Some(top + lb_in - (content_h - viewport_h));
        }
        // What the progress bar and buttons hang from: the last visible baseline, or without any
        // text a point placing them `PAD` below the icon (or the top).
        let anchor = last.unwrap_or(y - (TEXT_TO_BUTTONS - PAD));
        let win_h = tail(anchor);

        // ---- paint ----
        let cx = WIDTH / 2.0;
        if icon {
            let r = Rect::from_origin_size(Point::new(cx - ICON_SIZE / 2.0, ICON_TOP), Size::new(ICON_SIZE, ICON_SIZE));
            match view.custom_icon {
                Some(image) => ui.image(image, r),
                None => widgets::icon(ui, r.origin(), ICON_SIZE, view.icon, &tk),
            }
            out.parts.icon = Some(r);
        }
        if let (Some(t), Some(top)) = (&title, title_top) {
            let pos = Point::new(cx - t.size.width / 2.0, top);
            ui.text(t, pos, tk.text);
            out.parts.heading = Some(Rect::from_origin_size(pos, t.size));
        }
        if let (Some(b), Some(top)) = (&body, body_top) {
            let viewport = Rect::new(PAD / 2.0, top, WIDTH - PAD / 2.0, top + viewport_h);
            let max_scroll = (content_h - viewport_h).max(0.0);
            let wheel = if ui.pointer().is_some_and(|p| viewport.contains(p)) { view.frame.wheel_request } else { 0.0 };
            self.scroll = (self.scroll + view.frame.scroll_request + wheel).clamp(0.0, max_scroll);
            let clip = max_scroll > 0.0;
            if clip {
                ui.push_clip(viewport);
            }
            let pos = Point::new(cx - b.size.width / 2.0, top - self.scroll);
            ui.text(b, pos, tk.text);
            if clip {
                ui.pop_clip();
            }
            out.parts.body = Some(Rect::from_origin_size(pos, b.size).intersect(viewport));
            self.scroll = widgets::scroll_bar(ui, viewport, content_h, self.scroll, &tk);
        }
        if let Some(p) = view.progress {
            let r = Rect::new(PAD, anchor + TEXT_TO_PROGRESS, WIDTH - PAD, anchor + TEXT_TO_PROGRESS + PROGRESS_H);
            widgets::progress(ui, r, p, &tk);
            out.parts.progress = Some(r);
        }

        // ---- buttons ----
        if n > 0 {
            let top = win_h - PAD - buttons_h;
            let default_button = n - 1;
            let is_default = |i: usize| view.progress.is_none() && i == default_button;
            if stacked {
                // Default on top, then the others in reversed API order.
                let mut col = Column::new(PAD, WIDTH - PAD, top);
                for k in 0..n {
                    let index = n - 1 - k;
                    if k > 0 {
                        col.space(BUTTON_GAP);
                    }
                    let r = col.row(BUTTON_H);
                    let st = widgets::button(ui, r, index, &labels[index], is_default(index), view, &tk);
                    out.push_button(&st);
                }
            } else {
                // Side by side in API order; one button spans the row.
                let cells = columns(Rect::new(PAD, top, WIDTH - PAD, top + BUTTON_H), n, BUTTON_GAP);
                for (index, cell) in cells.into_iter().enumerate() {
                    let st = widgets::button(ui, cell, index, &labels[index], is_default(index), view, &tk);
                    out.push_button(&st);
                }
            }
        }
        out.desired_size = Size::new(WIDTH, win_h.ceil());
        out
    }
}

#[cfg(all(test, draw_soft))]
mod tests {
    use super::*;
    use crate::backends::gui::theme::test_support::{pass, view};

    /// Two short buttons sit side by side in API order, the default on the right; the window is
    /// 260 wide with the buttons 16 from its edges and bottom.
    #[test]
    fn two_buttons_side_by_side() {
        let mut theme = MacTheme::new();
        let buttons = vec!["Cancel".to_string(), "OK".to_string()];
        let v = DialogView { heading: "Quit?", body: "Unsaved changes will be lost.", ..view(&buttons) };
        let (out, _) = pass(&mut theme, &v);
        assert_eq!(out.buttons.iter().map(|b| b.index).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(out.desired_size.width, WIDTH);
        assert_eq!(out.buttons[0].rect.x0, PAD);
        assert_eq!(out.buttons[1].rect.x1, WIDTH - PAD);
        assert_eq!(out.buttons[0].rect.y0, out.buttons[1].rect.y0);
        assert!((out.buttons[1].rect.y1 - (out.desired_size.height - PAD)).abs() < 1.0);
    }

    /// Three buttons stack, the default on top.
    #[test]
    fn three_buttons_stack_default_first() {
        let mut theme = MacTheme::new();
        let buttons = vec!["Cancel".to_string(), "Don't Save".to_string(), "Save".to_string()];
        let (out, _) = pass(&mut theme, &DialogView { heading: "Save?", ..view(&buttons) });
        assert_eq!(out.buttons.iter().map(|b| b.index).collect::<Vec<_>>(), vec![2, 1, 0]);
        assert!(out.buttons[0].rect.y1 < out.buttons[1].rect.y0);
        assert_eq!(out.buttons[0].rect.width(), WIDTH - 2.0 * PAD);
    }

    /// Without a heading the window title is the alert's title.
    #[test]
    fn title_stands_in_for_the_heading() {
        let mut theme = MacTheme::new();
        let buttons = vec!["OK".to_string()];
        let (out, _) = pass(&mut theme, &DialogView { title: "App", body: "Body.", ..view(&buttons) });
        assert!(out.parts.heading.is_some() && out.parts.body.is_some());
    }

    /// A body taller than the height limit scrolls instead of growing the window.
    #[test]
    fn long_body_scrolls() {
        let mut theme = MacTheme::new();
        let buttons = vec!["OK".to_string()];
        let body = "A long line of body text. ".repeat(200);
        let (out, _) = pass(&mut theme, &DialogView { heading: "Long", body: &body, max_height: 400.0, ..view(&buttons) });
        assert!(out.desired_size.height <= 401.0, "{:?}", out.desired_size);
    }
}
