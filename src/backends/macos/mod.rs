//! macOS theme: the alert as NSAlert and CFUserNotification show it, built from the theme's own
//! widgets (`widgets.rs`: push button, progress bar, fallback icons, scroller), in two styles
//! ([`MacStyle`]): macOS 11 (Big Sur) to 15 (Sequoia), and macOS 26 (Tahoe).
//!
//! Layout (points, measured on Sequoia; [`Metrics::LEGACY`]):
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
//! Tahoe ([`Metrics::TAHOE`], measured on macOS 26 from CFUserNotification alerts):
//!
//! ```text
//! ╭──────────────────────────╮  260 wide, 26 corner radius (Liquid Glass)
//! │ 20                       │
//! │ 20 [icon 64]             │  left-aligned
//! │ 29 to the first baseline │
//! │ 22 Title (13 pt bold)    │  left-aligned, wraps at 216, pitch 16
//! │ 26 baseline to baseline  │
//! │ 22 Body (13 pt), left-   │  pitch 16
//! │ aligned, wrapping at 216 │
//! │ 19                       │
//! │ ( Cancel ) 8 (   OK   )  │  28 tall capsules, 110 wide; stacked: 228 wide, 6 apart
//! │ 16                       │
//! ╰──────────────────────────╯
//! ```
//!
//! With a heading and no body, the Tahoe icon and title are centred (as CFUserNotification
//! does).
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

use self::tokens::{MacTokens, BUTTON_SIZE, FONTS, TITLE_SIZE};
use self::widgets::{BUTTON_H, PROGRESS_H};
use crate::backends::draw::{Color, Point, Rect, Size};
use crate::backends::gui::appearance::Appearance;
use crate::backends::gui::text::{TextBlock, TextStyle};
use crate::backends::gui::theme::*;
use crate::backends::gui::ui::{columns, Column};

/// Window width; icon side; the button inset (sides and bottom), the same in both styles.
const WIDTH: f64 = 260.0;
const ICON_SIZE: f64 = 64.0;
const PAD: f64 = 16.0;
/// Horizontal label padding inside a button (a label wider than its button minus this is elided
/// or stacks the buttons).
const BUTTON_PAD_X: f64 = 12.0;
/// Smallest scrolling body viewport.
const MIN_VIEWPORT: f64 = 42.0;

/// Which macOS alert look the theme draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MacStyle {
    /// macOS 11 (Big Sur) to 15 (Sequoia): everything centred, 6 pt button corners, the
    /// accent-gradient default button, a vibrancy material.
    #[default]
    Legacy,
    /// macOS 26 (Tahoe): left-aligned, capsule buttons, a flat accent default button, Liquid
    /// Glass with 26 pt corners.
    Tahoe,
}

impl MacStyle {
    /// The running system's style: `Tahoe` where Liquid Glass exists (macOS 26+), `Legacy`
    /// elsewhere. Test builds honour `XDIALOG_TEST_MAC_STYLE=legacy|tahoe`.
    pub(crate) fn current() -> Self {
        if crate::backends::gui::appearance::test_env_enabled() {
            match std::env::var("XDIALOG_TEST_MAC_STYLE").ok().as_deref().map(str::trim) {
                Some(v) if v.eq_ignore_ascii_case("legacy") => return MacStyle::Legacy,
                Some(v) if v.eq_ignore_ascii_case("tahoe") => return MacStyle::Tahoe,
                _ => {}
            }
        }
        #[cfg(target_os = "macos")]
        if crate::backends::gui::platform_mac::has_liquid_glass() {
            return MacStyle::Tahoe;
        }
        MacStyle::Legacy
    }

    fn metrics(self) -> &'static Metrics {
        match self {
            MacStyle::Legacy => &Metrics::LEGACY,
            MacStyle::Tahoe => &Metrics::TAHOE,
        }
    }
}

/// The layout numbers of a style (points).
struct Metrics {
    /// Text column inset (wraps at `WIDTH - 2 * text_inset`).
    text_inset: f64,
    /// Icon distance from the top, and from the left (`None`: centred).
    icon_top: f64,
    icon_left: Option<f64>,
    /// Body text size (the title and button labels are 13 pt in both styles).
    body_size: f64,
    /// Baseline distances: icon bottom (or window top + `icon_top` without an icon) to the first
    /// baseline; title's last baseline to the body's first; last text baseline to the buttons (or
    /// to the progress bar); progress bar to the buttons.
    first_baseline: f64,
    title_to_body: f64,
    text_to_buttons: f64,
    text_to_progress: f64,
    progress_to_buttons: f64,
    /// Gap between side-by-side buttons, and between stacked ones.
    row_gap: f64,
    stack_gap: f64,
    /// Window corner radius (the material is clipped to it).
    window_radius: f64,
}

impl Metrics {
    /// Big Sur to Sequoia (measured on Sequoia): everything centred.
    const LEGACY: Metrics = Metrics { text_inset: 20.0,
                                      icon_top: 20.0,
                                      icon_left: None,
                                      body_size: 11.0,
                                      first_baseline: 33.0,
                                      title_to_body: 23.5,
                                      text_to_buttons: 19.5,
                                      text_to_progress: 14.0,
                                      progress_to_buttons: 16.0,
                                      row_gap: 8.0,
                                      stack_gap: 8.0,
                                      window_radius: 10.0 };

    /// Tahoe (measured on macOS 26.6 at 2x): the icon at (20, 20), text at x = 22 wrapping at 216
    /// (the longest unwrapped line measured 211, the shortest wrapped one 217), the first
    /// baseline 29 below the icon, 26 between the title's last baseline and the body's first, 19
    /// from the last baseline to the buttons; side-by-side buttons 110 wide (8 apart), stacked
    /// ones 34 apart (6 gap); 14 pt capsule corners; a 26 pt continuous window corner (fitted to
    /// the window's alpha). These reproduce the reference window heights (176 to 330) exactly. The
    /// progress bar distances are not in the references: Sequoia's.
    const TAHOE: Metrics = Metrics { text_inset: 22.0,
                                     icon_top: 20.0,
                                     icon_left: Some(20.0),
                                     body_size: 13.0,
                                     first_baseline: 29.0,
                                     title_to_body: 26.0,
                                     text_to_buttons: 19.0,
                                     text_to_progress: 14.0,
                                     progress_to_buttons: 16.0,
                                     row_gap: 8.0,
                                     stack_gap: 6.0,
                                     window_radius: 26.0 };
}

/// Keyboard policy (AppKit): the focus ring only after Tab, Return activates the default button,
/// Space activates the focused button on release, Tab wraps, arrows don't move focus between
/// push buttons (clamped), PageUp/PageDown/Home/End scroll the body.
pub(crate) const KEYBOARD: KeyboardPolicy = KeyboardPolicy { focus_visibility: FocusVisibility::KeyboardOnly,
                                                             arrows: ArrowNav::Clamp,
                                                             enter_falls_back_to_default: true,
                                                             enter_activates_default: true,
                                                             space: SpaceKey::ActivateOnRelease,
                                                             scroll_keys: true };

/// The macOS theme.
pub(crate) struct MacTheme {
    style: MacStyle,
    tokens: MacTokens,
    /// Body scroll offset, logical px.
    scroll: f64,
}

impl MacTheme {
    pub(crate) fn new(style: MacStyle) -> Self {
        MacTheme { style, tokens: MacTokens::new(style, &Appearance::default()), scroll: 0.0 }
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
        self.tokens = MacTokens::new(self.style, appearance);
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

    /// Legacy: the alert vibrancy; Tahoe: Liquid Glass.
    fn window_material(&self) -> WindowMaterial {
        let kind = match self.style {
            MacStyle::Legacy => MaterialKind::Vibrancy,
            MacStyle::Tahoe => MaterialKind::Glass,
        };
        WindowMaterial { kind, corner_radius: self.style.metrics().window_radius }
    }

    /// The system's alert icons (the app icon for `Information`).
    #[cfg(target_os = "macos")]
    fn system_icon(&self, icon: &crate::model::XDialogIcon) -> Option<std::sync::Arc<crate::icon::IconFile>> {
        crate::backends::gui::platform_mac::alert_icon(icon)
    }

    fn ui(&mut self, view: &DialogView<'_>, ui: &mut Ui<'_>) -> DialogUiOutput {
        let tk = self.tokens.clone();
        let m = self.style.metrics();
        let label_style = TextStyle::regular(BUTTON_SIZE);
        let (title_pitch, body_pitch) = (tokens::line_height(TITLE_SIZE), tokens::line_height(m.body_size));
        let mut out = DialogUiOutput::default();

        // ---- measure ----
        let text_w = WIDTH - 2.0 * m.text_inset;
        let heading = if view.heading.is_empty() { view.title } else { view.heading };
        // Tahoe centres the icon and title only when there is no body.
        let centred = m.icon_left.is_none() || view.body.is_empty();
        let style = |s: TextStyle| if centred { s.centered() } else { s };
        let title = (!heading.is_empty()).then(|| ui.layout(heading, &style(TextStyle::bold(TITLE_SIZE)), text_w, None));
        let body = (!view.body.is_empty()).then(|| ui.layout(view.body, &style(TextStyle::regular(m.body_size)), text_w, None));
        let n = view.buttons.len();
        let row = WIDTH - 2.0 * PAD;
        let half = (row - m.row_gap) / 2.0;
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
            _ if stacked => n as f64 * BUTTON_H + (n - 1) as f64 * m.stack_gap,
            _ => BUTTON_H,
        };

        // ---- vertical layout: baselines first, then boxes ----
        let icon = view.has_icon();
        let y = if icon { m.icon_top + ICON_SIZE } else { m.icon_top };
        // Where the next text block's first baseline goes, and the last baseline so far.
        let mut next_baseline = y + m.first_baseline - if icon { 0.0 } else { m.icon_top };
        let mut last = None;
        let title_top = title.as_ref().map(|t| {
                                          let top = next_baseline - first_baseline(t);
                                          let lb = top + last_baseline(t, title_pitch);
                                          last = Some(lb);
                                          next_baseline = lb + m.title_to_body;
                                          top
                                      });
        // The body: a viewport that scrolls when the whole dialog would exceed the height limit.
        let body_top = body.as_ref().map(|b| next_baseline - first_baseline(b));
        let tail = |text_bottom: f64| {
            // Height below the last baseline: progress bar and buttons.
            let mut h = 0.0;
            if view.progress.is_some() {
                h += m.text_to_progress + PROGRESS_H + if n > 0 { m.progress_to_buttons } else { PAD };
            } else if n > 0 {
                h += m.text_to_buttons;
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
        let anchor = last.unwrap_or(y - (m.text_to_buttons - PAD));
        let win_h = tail(anchor);

        // ---- paint ----
        // A text block's left edge: centred, or start-aligned in the text column.
        let cx = WIDTH / 2.0;
        let paint_text = |ui: &mut Ui<'_>, block: &TextBlock, top: f64| {
            let x = if centred {
                cx - block.size.width / 2.0
            } else if block.rtl {
                m.text_inset + text_w - block.size.width
            } else {
                m.text_inset
            };
            let pos = Point::new(x, top);
            ui.text(block, pos, tk.text);
            pos
        };
        if icon {
            let left = match m.icon_left {
                Some(x) if !centred => x,
                _ => cx - ICON_SIZE / 2.0,
            };
            let r = Rect::from_origin_size(Point::new(left, m.icon_top), Size::new(ICON_SIZE, ICON_SIZE));
            match view.custom_icon {
                Some(image) => ui.image(image, r),
                None => widgets::icon(ui, r.origin(), ICON_SIZE, view.icon, &tk),
            }
            out.parts.icon = Some(r);
        }
        if let (Some(t), Some(top)) = (&title, title_top) {
            let pos = paint_text(ui, t, top);
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
            let pos = paint_text(ui, b, top - self.scroll);
            if clip {
                ui.pop_clip();
            }
            out.parts.body = Some(Rect::from_origin_size(pos, b.size).intersect(viewport));
            self.scroll = widgets::scroll_bar(ui, viewport, content_h, self.scroll, &tk);
        }
        if let Some(p) = view.progress {
            let r = Rect::new(PAD, anchor + m.text_to_progress, WIDTH - PAD, anchor + m.text_to_progress + PROGRESS_H);
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
                        col.space(m.stack_gap);
                    }
                    let r = col.row(BUTTON_H);
                    let st = widgets::button(ui, r, index, &labels[index], is_default(index), view, &tk);
                    out.push_button(&st);
                }
            } else {
                // Side by side in API order; one button spans the row.
                let cells = columns(Rect::new(PAD, top, WIDTH - PAD, top + BUTTON_H), n, m.row_gap);
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
        let mut theme = MacTheme::new(MacStyle::Legacy);
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
        let mut theme = MacTheme::new(MacStyle::Legacy);
        let buttons = vec!["Cancel".to_string(), "Don't Save".to_string(), "Save".to_string()];
        let (out, _) = pass(&mut theme, &DialogView { heading: "Save?", ..view(&buttons) });
        assert_eq!(out.buttons.iter().map(|b| b.index).collect::<Vec<_>>(), vec![2, 1, 0]);
        assert!(out.buttons[0].rect.y1 < out.buttons[1].rect.y0);
        assert_eq!(out.buttons[0].rect.width(), WIDTH - 2.0 * PAD);
    }

    /// Without a heading the window title is the alert's title.
    #[test]
    fn title_stands_in_for_the_heading() {
        let mut theme = MacTheme::new(MacStyle::Legacy);
        let buttons = vec!["OK".to_string()];
        let (out, _) = pass(&mut theme, &DialogView { title: "App", body: "Body.", ..view(&buttons) });
        assert!(out.parts.heading.is_some() && out.parts.body.is_some());
    }

    /// A body taller than the height limit scrolls instead of growing the window.
    #[test]
    fn long_body_scrolls() {
        let mut theme = MacTheme::new(MacStyle::Legacy);
        let buttons = vec!["OK".to_string()];
        let body = "A long line of body text. ".repeat(200);
        let (out, _) = pass(&mut theme, &DialogView { heading: "Long", body: &body, max_height: 400.0, ..view(&buttons) });
        assert!(out.desired_size.height <= 401.0, "{:?}", out.desired_size);
    }

    /// Tahoe: the icon at (20, 20), title and body start-aligned at x = 22 (wrapping at 216),
    /// side-by-side buttons 110 wide 8 apart, capsule corners.
    #[test]
    fn tahoe_is_left_aligned() {
        let mut theme = MacTheme::new(MacStyle::Tahoe);
        let buttons = vec!["Cancel".to_string(), "OK".to_string()];
        let v = DialogView { heading: "Quit?",
                             body: "Unsaved changes will be lost.",
                             icon: &crate::model::XDialogIcon::Warning,
                             ..view(&buttons) };
        let (out, shapes) = pass(&mut theme, &v);
        assert_eq!(out.parts.icon, Some(Rect::new(20.0, 20.0, 84.0, 84.0)));
        assert_eq!(out.parts.heading.map(|r| r.x0), Some(22.0));
        assert_eq!(out.parts.body.map(|r| r.x0), Some(22.0));
        let (a, b) = (out.buttons[0].rect, out.buttons[1].rect);
        assert_eq!((a.x0, a.width(), b.x0, b.width()), (16.0, 110.0, 134.0, 110.0));
        assert_eq!(a.height(), BUTTON_H);
        let capsule = |r: Rect| {
            shapes.iter()
                  .any(|s| matches!(s, crate::backends::draw::Shape::Rect { rect, radius, .. } if *rect == r && *radius == BUTTON_H / 2.0))
        };
        assert!(capsule(a) && capsule(b), "capsule buttons");
    }

    /// Tahoe with a heading and no body: icon and title centred (as CFUserNotification does).
    #[test]
    fn tahoe_title_only_is_centred() {
        let mut theme = MacTheme::new(MacStyle::Tahoe);
        let buttons = vec!["OK".to_string()];
        let v = DialogView { heading: "Hello", icon: &crate::model::XDialogIcon::Error, ..view(&buttons) };
        let (out, _) = pass(&mut theme, &v);
        assert_eq!(out.parts.icon, Some(Rect::new(98.0, 20.0, 162.0, 84.0)));
        let h = out.parts.heading.expect("heading");
        assert!((h.center().x - WIDTH / 2.0).abs() < 0.01, "{h:?}");
        assert_eq!((out.buttons[0].rect.x0, out.buttons[0].rect.width()), (PAD, WIDTH - 2.0 * PAD));
    }

    /// Tahoe stacks three buttons 34 apart (6 gap), the default on top; Sequoia 36 (8 gap).
    #[test]
    fn stacked_button_pitch_per_style() {
        let buttons = vec!["Cancel".to_string(), "Don't Save".to_string(), "Save".to_string()];
        for (style, pitch) in [(MacStyle::Tahoe, 34.0), (MacStyle::Legacy, 36.0)] {
            let mut theme = MacTheme::new(style);
            let (out, _) = pass(&mut theme, &DialogView { heading: "Save?", ..view(&buttons) });
            assert_eq!(out.buttons.iter().map(|b| b.index).collect::<Vec<_>>(), vec![2, 1, 0]);
            assert_eq!(out.buttons[1].rect.y0 - out.buttons[0].rect.y0, pitch, "{style:?}");
            assert_eq!(out.buttons[0].rect.width(), WIDTH - 2.0 * PAD);
        }
    }
}
