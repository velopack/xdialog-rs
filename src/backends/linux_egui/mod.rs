//! Linux theme: a faithful egui port of the look of the former skia backend (xdialog 3.x; the skia
//! sources at `4473a9e`, `src/backends/skia/`).
//!
//! Widget-based: [`LinuxTheme::ui`] runs skia's layout algorithm
//! (`dialog.rs:534-658`) every pass and adds the theme's own widgets at the computed rects:
//! `icons::SkiaIcon` (1:1 procedural icons), `widgets::SkiaLabel` (cosmic-text style wrapping,
//! `1.2 x size` line pitch, pixel-rounded origin), `widgets::SkiaProgress` (300 ms OutCubic
//! value animation, 3 s stretchy indeterminate capsule) and `widgets::SkiaButton` (outlined,
//! quadratic corners, 150 ms linear colour fades). Hover/press/click come from egui, focus from
//! egui focus memory driven by core's keyboard policy (Tab/arrows wrap, Enter/Space activate on
//! press, Escape closes).
//!
//! Deliberate deviations from skia:
//! - Pointer leaving the window: skia ignored `CursorLeft`, so a hovered button stayed
//!   hovered (blue) until the next move inside the window. Core turns `CursorLeft` into egui's
//!   `PointerGone`, so the button fades back to idle over 150 ms, and the hover-driven focus-border
//!   suppression is lifted at the same time (the focused button shows its ring again).
//! - Window focus: none. skia drew the focus ring regardless of window activation, and so does
//!   this theme (it reads egui focus memory, which survives `RawInput::focused == false`).

use std::sync::Arc;

use egui::{Event, FontFamily, Id, Pos2, Rect, Ui, Vec2};

use crate::backends::egui_core::fonts::{bundled, FaceRef, FontRegistry};
use crate::backends::egui_core::text::{TextBlock, TextCtx, TextStyle};
use crate::backends::egui_core::theme::*;
use crate::model::XDialogIcon;

mod icons;
mod tokens;
mod widgets;
mod wrap;

pub(crate) use tokens::LinuxTokens;
use tokens::*;

const REGULAR: &str = "ubuntu-regular";
const BOLD: &str = "ubuntu-bold";

/// Text rasterization to approximate skia's text compositing. skia blended glyph coverage in
/// LINEAR light (text.rs:160-236), egui blends in sRGB space; for the theme's text/background
/// pairs that is equivalent to remapping coverage with a power curve (fitted over
/// `#3D3D3D on #FAFAFA` and `#EEEEEE on #2D2D2D`). cosmic-text rasterized with swash hinting on (measured:
/// hinted egui glyphs halve the text error vs unhinted).
const LIGHT_TEXT_GAMMA: f32 = 1.67;
const DARK_TEXT_GAMMA: f32 = 0.57;

/// The Linux (skia-look) theme.
pub(crate) struct LinuxTheme;

impl LinuxTheme {
    // Linux with neither built-in winit nor `winit-host` compiles the theme but has no entry point
    // that runs it (every dialog fails with `NoBackendAvailable`).
    #[cfg_attr(not(any(xd_own_loop, xd_winit_host, xd_test_hooks)), allow(dead_code))]
    pub(crate) fn new() -> Self {
        LinuxTheme
    }
}

fn bold() -> FontFamily {
    FontFamily::Name(Arc::from(BOLD))
}

fn title_style() -> TextStyle {
    TextStyle::new(TITLE_SIZE, bold(), TITLE_SIZE * LINE_HEIGHT_SCALE)
}

fn body_style() -> TextStyle {
    TextStyle::new(BODY_SIZE, FontFamily::Proportional, BODY_SIZE * LINE_HEIGHT_SCALE)
}

/// Height of a skia text block: `max(lines, 1) * line_height` (text.rs:77).
fn block_height(block: &TextBlock, style: &TextStyle) -> f32 {
    block.line_count().max(1) as f32 * style.line_pitch
}

/// skia's focus-border suppression (dialog.rs:247-281): hovering any button hides the focused
/// button's focus border. skia re-evaluates it only on `CursorMoved`, and any focus move
/// (`set_focused(true)`, keyboard or press) shows the border again until the pointer moves.
fn focus_suppressed(ui: &Ui, button_count: usize) -> bool {
    let ctx = ui.ctx();
    let key = Id::new("linux.focus_suppressed");
    let focused = ctx.memory(|m| m.focused());
    let (mut suppressed, last_focus) = ctx.data(|d| d.get_temp::<(bool, Option<Id>)>(key)).unwrap_or((false, focused));
    if focused != last_focus {
        suppressed = false;
    }
    // `PointerGone`: core turns `CursorLeft` into it (a documented deviation, see the module
    // docs); the hovered button fades back to idle, so the focus border must return. (The button
    // responses read here are the previous pass's, which still contain the pointer.)
    let (moved, gone) = ui.input(|i| {
                              (i.events.iter().any(|e| matches!(e, Event::PointerMoved(_))), i.events.iter().any(|e| matches!(e, Event::PointerGone)))
                          });
    if gone {
        suppressed = false;
    } else if moved {
        suppressed = any_button_contains_pointer(ctx, button_count);
    }
    ctx.data_mut(|d| d.insert_temp(key, (suppressed, focused)));
    suppressed
}

impl Theme for LinuxTheme {
    type Tokens = LinuxTokens;

    fn id(&self) -> &'static str {
        "linux"
    }

    /// skia `dialog.rs:304-337`: Tab/Shift+Tab and Left/Right move focus with wrapping (also on
    /// key repeat), Enter/Space activate the focused button on press, Escape closes; initial
    /// focus = the last (right-most, default) button, always drawn.
    fn keyboard_policy(&self) -> KeyboardPolicy {
        KeyboardPolicy { focus_on_open: true,
                         focus_visibility: FocusVisibility::Always,
                         tab: true,
                         arrows: ArrowNav::Wrap,
                         nav_repeat: true,
                         home_end: false,
                         enter: true,
                         enter_falls_back_to_default: false,
                         space: SpaceKey::ActivateOnPress,
                         activate_flash: None,
                         escape_closes: true,
                         scroll_keys: false }
    }

    fn tokens(&self, env: &ThemeEnv) -> LinuxTokens {
        LinuxTokens::resolve(env)
    }

    fn window_style(&self, tk: &LinuxTokens) -> WindowStyle {
        WindowStyle { clear: tk.bg, dark_titlebar: tk.dark }
    }

    fn install_fonts(&self, defs: &mut egui::FontDefinitions, _reg: &FontRegistry) {
        defs.font_data.insert(REGULAR.into(), Arc::new(bundled::ubuntu_regular().font_data()));
        defs.font_data.insert(BOLD.into(), Arc::new(bundled::ubuntu_bold().font_data()));
        defs.families.insert(FontFamily::Proportional, vec![REGULAR.into()]);
        defs.families.insert(FontFamily::Monospace, vec![REGULAR.into()]);
        defs.families.insert(bold(), vec![BOLD.into()]);
    }

    fn text_families(&self) -> Vec<FontFamily> {
        vec![FontFamily::Proportional, FontFamily::Monospace, bold()]
    }

    fn bold_families(&self) -> Vec<FontFamily> {
        vec![bold()]
    }

    fn primary_faces(&self, _reg: &FontRegistry) -> Vec<FaceRef> {
        vec![bundled::ubuntu_regular(), bundled::ubuntu_bold()]
    }

    fn configure_style(&self, tk: &LinuxTokens, style: &mut egui::Style) {
        style.spacing.item_spacing = Vec2::ZERO;
        let opts = &mut style.visuals.text_options;
        opts.font_hinting = true;
        opts.subpixel_binning = true;
        opts.color_transfer_function = egui::epaint::FontColorTransferFunction::Gamma(if tk.dark { DARK_TEXT_GAMMA } else { LIGHT_TEXT_GAMMA });
    }

    fn ui(&self, tk: &LinuxTokens, view: &DialogView<'_>, ui: &mut Ui) -> DialogUiOutput {
        let ctx = ui.ctx().clone();
        let text = TextCtx::new(&ctx);
        let (title_style, body_style) = (title_style(), body_style());
        ui.spacing_mut().item_spacing = Vec2::ZERO;

        // 1. Natural width of the content (unwrapped title/body; progress measures 0, buttons
        //    don't count) -> window width.
        let natural = wrap::natural_width(&text, view.heading, &title_style).max(wrap::natural_width(&text, view.body, &body_style));
        let win_w = window_width(natural);
        let has_icon = *view.icon != XDialogIcon::None;
        let text_x = if has_icon { MARGIN + ICON_SIZE + MARGIN } else { MARGIN };
        let col_w = win_w - text_x - MARGIN;

        // 2. Stack title / progress / body at text_x from y = 16, each followed by a 16 gap
        //    (the trailing gap is kept).
        let mut y = MARGIN;
        let mut place = |h: f32| {
            let r = Rect::from_min_size(Pos2::new(text_x, y), Vec2::new(col_w, h));
            y += h + MARGIN;
            r
        };
        let title = (!view.heading.is_empty()).then(|| {
                                                   let b = wrap::layout(&ctx, &text, view.heading, &title_style, col_w);
                                                   let r = place(block_height(&b, &title_style));
                                                   (b, r)
                                               });
        let progress = view.progress.map(|p| (p, place(PROGRESS_H)));
        let body = (!view.body.is_empty()).then(|| {
                                              let b = wrap::layout(&ctx, &text, view.body, &body_style, col_w);
                                              let r = place(block_height(&b, &body_style));
                                              (b, r)
                                          });
        // 3. Content height: at least the icon column (16 + 48 + 16); the icon is top-aligned.
        let content_h = if has_icon { y.max(MARGIN + ICON_SIZE + MARGIN) } else { y };

        // Paint order = skia z-order: icon, title, progress, body, buttons.
        if has_icon {
            ui.add(icons::SkiaIcon { icon: view.icon, rect: Rect::from_min_size(Pos2::new(MARGIN, MARGIN), Vec2::splat(ICON_SIZE)) });
        }
        if let Some((b, r)) = &title {
            ui.add(widgets::SkiaLabel { block: b, rect: *r, color: tk.title_text });
        }
        if let Some((p, r)) = progress {
            ui.add(widgets::SkiaProgress { progress: p, rect: r, tk });
        }
        if let Some((b, r)) = &body {
            ui.add(widgets::SkiaLabel { block: b, rect: *r, color: tk.body_text });
        }

        // 4. Footer (only with buttons): buttons right-aligned in API order, 7 px apart, 7 px from
        //    the right edge and the footer top. They may overflow to the left (skia doesn't stop it).
        let n = view.buttons.len();
        let mut out = DialogUiOutput { arrow_order: (0..n).collect(),
                                       arrow_axis: ArrowAxis::Horizontal,
                                       // skia focuses the LAST focusable component on open.
                                       default_button: n.checked_sub(1),
                                       ..Default::default() };
        if n > 0 {
            let labels: Vec<TextBlock> = view.buttons.iter().map(|l| text.layout(l, &body_style, f32::INFINITY, None)).collect();
            let sizes: Vec<Vec2> = labels.iter().map(widgets::button_size).collect();
            let total = sizes.iter().map(|s| s.x).sum::<f32>() + BUTTON_GAP * (n - 1) as f32;
            let mut x = win_w - FOOTER_MARGIN - total;
            let suppressed = focus_suppressed(ui, n);
            for (i, (label, size)) in labels.iter().zip(&sizes).enumerate() {
                let rect = Rect::from_min_size(Pos2::new(x, content_h + FOOTER_MARGIN), *size);
                x += size.x + BUTTON_GAP;
                let st = widgets::SkiaButton { index: i, rect, label, view, tk, focus_suppressed: suppressed }.show(ui);
                if st.activated && out.activated.is_none() {
                    out.activated = Some(i);
                }
                out.buttons.push(ButtonInfo { index: i, rect });
            }
        }
        // 5. Window = content + footer (48) when there are buttons.
        out.desired_size = Vec2::new(win_w, content_h + if n == 0 { 0.0 } else { FOOTER_H });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::egui_core::anim::{self, Lerp, Transition};
    use crate::backends::egui_core::appearance::Appearance;

    struct Harness {
        ctx: egui::Context,
        theme: LinuxTheme,
        tk: LinuxTokens,
        env: ThemeEnv,
        /// `RawInput::focused` (window activation) for the next passes.
        window_focused: std::cell::Cell<bool>,
    }

    struct Content {
        heading: &'static str,
        body: &'static str,
        icon: XDialogIcon,
        buttons: Vec<String>,
        progress: Option<ProgressView>,
    }

    impl Default for Content {
        fn default() -> Self {
            Content { heading: "Operation complete",
                      body: "The operation completed successfully and everything is fine.",
                      icon: XDialogIcon::Information,
                      buttons: vec!["Cancel".to_string(), "OK".to_string()],
                      progress: None }
        }
    }

    impl Harness {
        fn new() -> Self {
            let theme = LinuxTheme::new();
            let env = ThemeEnv { appearance: Appearance::default(), platform: Platform::current() };
            let tk = theme.tokens(&env);
            let ctx = egui::Context::default();
            let mut defs = egui::FontDefinitions::empty();
            theme.install_fonts(&mut defs, FontRegistry::global());
            ctx.set_fonts(defs);
            ctx.options_mut(core_options);
            ctx.all_styles_mut(|s| {
                   theme.configure_style(&tk, s);
                   core_style_overrides(s);
               });
            Harness { ctx, theme, tk, env, window_focused: std::cell::Cell::new(true) }
        }

        fn pass(&self, t: f64, events: Vec<egui::Event>) -> DialogUiOutput {
            self.pass_with(&Content::default(), t, events, false, |_| ()).0
        }

        /// One pass; `probe` runs inside the pass right after the theme.
        fn pass_with<R>(&self, c: &Content, t: f64, events: Vec<egui::Event>, sizing: bool, probe: impl FnOnce(&egui::Context) -> R) -> (DialogUiOutput, R) {
            let disabled = vec![false; c.buttons.len()];
            let view = DialogView { kind: if c.progress.is_some() { DialogKind::Progress } else { DialogKind::Message },
                                    title: "t",
                                    heading: c.heading,
                                    body: c.body,
                                    icon: &c.icon,
                                    buttons: &c.buttons,
                                    disabled: &disabled,
                                    progress: c.progress,
                                    env: &self.env,
                                    limits: SizeLimits { max_height: 800.0, max_width: 4096.0 },
                                    frame: FrameInfo { time: t,
                                                       ppp: 1.0,
                                                       sizing,
                                                       window_focused: true,
                                                       focus_visible: true,
                                                       key_pressed: None,
                                                       scroll_request: 0.0 } };
            // The measure pass runs at the largest allowed client size.
            let screen = if sizing { Vec2::new(4096.0, 800.0) } else { Vec2::new(350.0, 200.0) };
            let raw = egui::RawInput { time: Some(t),
                                       screen_rect: Some(Rect::from_min_size(Pos2::ZERO, screen)),
                                       focused: !sizing && self.window_focused.get(),
                                       events,
                                       ..Default::default() };
            let mut out = DialogUiOutput::default();
            let mut probe = Some(probe);
            let mut probed = None;
            let mut full = self.ctx.run_ui(raw, |ui| {
                                       out = self.theme.ui(&self.tk, &view, ui);
                                       probed = probe.take().map(|p| p(ui.ctx()));
                                   });
            full.textures_delta.clear();
            (out, probed.unwrap())
        }
    }

    /// Core order: measure pass, on-open focus, `reset_animations`, first real
    /// frame. Unlike the snap core sets up, skia pre-rendered every button at its idle look and
    /// faded the default button's focus look in over 150 ms after the window appeared.
    #[test]
    fn focus_look_fades_in_on_open() {
        let h = Harness::new();
        let c = Content::default();
        let (measured, _) = h.pass_with(&c, 0.0, vec![], true, |_| ());
        h.ctx.memory_mut(|m| m.request_focus(button_id(measured.default_button.unwrap())));
        anim::reset_animations(&h.ctx);
        let (idle, focused) = (h.tk.idle, h.tk.focused);
        let shown = |t| h.pass_with(&c, t, vec![], false, |ctx| anim::animate(ctx, button_id(1), focused, Transition::linear(0.15))).1;
        let (real, first) = h.pass_with(&c, 0.0, vec![], false, |ctx| anim::animate(ctx, button_id(1), focused, Transition::linear(0.15)));
        assert_eq!(measured, real, "measure pass must build the same layout");
        assert_eq!(first, idle);
        assert_eq!(shown(0.075), ButtonLook::lerp(&idle, &focused, 0.5));
        assert_eq!(shown(0.2), focused);
        // A relayout or a later appearance change doesn't fade again.
        anim::reset_animations(&h.ctx);
        assert_eq!(shown(0.3), focused);
    }

    #[test]
    fn layout_matches_skia_size() {
        let h = Harness::new();
        let out = h.pass(0.0, vec![]);
        assert_eq!(out.buttons.iter().map(|b| b.index).collect::<Vec<_>>(), vec![0, 1]);
        assert!((out.buttons[1].rect.right() - 343.0).abs() < 1e-3, "{:?}", out.buttons);
        assert!((out.buttons[0].rect.right() + 7.0 - out.buttons[1].rect.left()).abs() < 1e-3);
        assert_eq!(out.default_button, Some(1));
        assert_eq!(out.desired_size.x, 350.0);
        // 16 + 21.6 + 16 + 2 * 16.8 + 16 + 48 = 151.2 exactly (rects are not egui-rounded).
        assert!((out.desired_size.y - 151.2).abs() < 1e-3, "{:?}", out.desired_size);
        assert!((out.buttons[1].rect.top() - 110.2).abs() < 1e-3);
    }

    #[test]
    fn layout_variants() {
        let h = Harness::new();
        // No icon, no heading, no buttons: 16 + 16.8 + 16.
        let c = Content { heading: "", body: "Solving string theory...", icon: XDialogIcon::None, buttons: vec![], ..Default::default() };
        let out = h.pass_with(&c, 0.0, vec![], false, |_| ()).0;
        assert!((out.desired_size.y - 48.8).abs() < 1e-3, "{:?}", out.desired_size);
        assert!(out.buttons.is_empty() && out.default_button.is_none());
        // Progress with icon: 16 + 21.6 + 16 + 6 + 16 + 16.8 + 16 = 108.4 (skia 350 x 108).
        let c = Content { heading: "Downloading updates",
                          body: "Downloading package 1 of 3...",
                          buttons: vec![],
                          progress: Some(ProgressView::Determinate { value: 0.0, prev: 0.0, changed_at: 0.0 }),
                          ..Default::default() };
        let out = h.pass_with(&c, 0.0, vec![], false, |_| ()).0;
        assert!((out.desired_size.y - 108.4).abs() < 1e-3, "{:?}", out.desired_size);
        // Icon column minimum: 80 (+ 48 footer).
        let c = Content { heading: "", body: "x", ..Default::default() };
        let out = h.pass_with(&c, 0.0, vec![], false, |_| ()).0;
        assert!((out.desired_size.y - 128.0).abs() < 1e-3, "{:?}", out.desired_size);
    }

    /// skia (cosmic-text) wraps after hyphens and slashes: `qa_hyphen` / `qa_url` are 350 x 168
    /// with 3 body lines, and a tab advances to the next 8-space stop.
    #[test]
    fn wrap_breaks_and_tabs_like_cosmic_text() {
        let h = Harness::new();
        for body in ["A self-contained, well-known, state-of-the-art example-with-hyphens-everywhere-in-it-to-force-breaks.",
                     "Visit https://example.com/downloads/latest/release-notes/version-4.0.0.html for the details."]
        {
            let c = Content { heading: "Hyphens", body, buttons: vec!["OK".into()], ..Default::default() };
            let out = h.pass_with(&c, 0.0, vec![], false, |_| ()).0;
            assert_eq!(out.desired_size.round(), Vec2::new(350.0, 168.0), "{body}");
        }
        let (w, here) = h.pass_with(&Content::default(), 0.0, vec![], false, |ctx| {
                             let text = TextCtx::new(ctx);
                             (wrap::natural_width(&text, "Tabs\there", &body_style()), text.natural_width("here", &body_style()))
                         })
                         .1;
        let stop = 8.0 * 0.231 * BODY_SIZE;
        assert!((w - (2.0 * stop + here)).abs() < 1e-3, "{w} {here}");
    }

    /// cosmic-text lets trailing whitespace hang: a 258 px line fits a 259 px column (skia
    /// `long_instruction`: 355 x 221, 5 title lines).
    #[test]
    fn wrap_like_cosmic_text() {
        let h = Harness::new();
        let c = Content { heading: "This is v. long main instruction which will almost certainly need to wrap into several lines and I need to make sure that the dialog sizes correctly",
                          body: "This is a very small dialog message!",
                          icon: XDialogIcon::Error,
                          buttons: vec!["OK".into()],
                          progress: None };
        let out = h.pass_with(&c, 0.0, vec![], false, |_| ()).0;
        assert_eq!(out.desired_size.x.round(), 355.0);
        assert_eq!(out.desired_size.y.round(), 221.0, "{:?}", out.desired_size);
    }

    #[test]
    fn capsule_timeline() {
        use widgets::capsule_pos;
        assert_eq!(capsule_pos(0.0), 0.0);
        assert!((capsule_pos(0.2) - 0.5).abs() < 1e-6);
        assert_eq!(capsule_pos(0.45), 1.0);
        assert!((capsule_pos(0.7) - 0.5).abs() < 1e-6);
        assert_eq!(capsule_pos(0.95), 0.0);
    }

    #[test]
    fn pointer_click_activates_through_egui() {
        let h = Harness::new();
        let out = h.pass(0.0, vec![]);
        let c = out.buttons[0].rect.center();
        let press = |pressed| egui::Event::PointerButton { pos: c, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        assert_eq!(h.pass(0.1, vec![egui::Event::PointerMoved(c)]).activated, None);
        assert_eq!(h.pass(0.2, vec![press(true)]).activated, None);
        // The press moved egui focus to the button.
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(button_id(0)));
        // A long hold still clicks (max_click_duration = inf).
        assert_eq!(h.pass(5.0, vec![press(false)]).activated, Some(0));
        // Press, drag off, release outside: no activation.
        let off = Pos2::new(5.0, 5.0);
        h.pass(5.1, vec![press(true)]);
        let out = h.pass(5.2, vec![egui::Event::PointerMoved(off),
                                   egui::Event::PointerButton { pos: off,
                                                                button: egui::PointerButton::Primary,
                                                                pressed: false,
                                                                modifiers: Default::default() }]);
        assert_eq!(out.activated, None);
    }

    /// Hovering a button hides the focus border of the focused one; a focus move shows it again
    /// until the pointer moves (skia `set_focused`).
    #[test]
    fn focus_border_suppression() {
        let h = Harness::new();
        let out = h.pass(0.0, vec![]);
        h.ctx.memory_mut(|m| m.request_focus(button_id(1)));
        let key = Id::new("linux.focus_suppressed");
        let probe = |t, ev| h.pass_with(&Content::default(), t, ev, false, |ctx| ctx.data(|d| d.get_temp::<(bool, Option<Id>)>(key))).1.unwrap().0;
        assert!(!probe(0.1, vec![]));
        let over = out.buttons[0].rect.center();
        h.pass(0.2, vec![egui::Event::PointerMoved(over)]);
        assert!(probe(0.3, vec![egui::Event::PointerMoved(over)]));
        // Keyboard focus move (core) while still hovering: visible again.
        h.ctx.memory_mut(|m| m.request_focus(button_id(0)));
        assert!(!probe(0.4, vec![]));
        assert!(probe(0.5, vec![egui::Event::PointerMoved(over + Vec2::new(1.0, 0.0))]));
        assert!(!probe(0.6, vec![egui::Event::PointerMoved(Pos2::new(5.0, 5.0))]));
        // The pointer leaves the window while over a button (core: `CursorLeft` -> `PointerGone`):
        // the hovered button fades to idle, so the focus border must come back.
        assert!(probe(0.7, vec![egui::Event::PointerMoved(over)]));
        assert!(!probe(0.8, vec![egui::Event::PointerGone]));
    }

    /// skia drew the default button's focus ring whether or not the window was active.
    #[test]
    fn focus_ring_kept_while_window_inactive() {
        let h = Harness::new();
        let c = Content::default();
        h.pass(0.0, vec![]);
        h.ctx.memory_mut(|m| m.request_focus(button_id(1)));
        let focused = h.tk.focused;
        let shown = |t| h.pass_with(&c, t, vec![], false, |ctx| anim::animate(ctx, button_id(1), focused, Transition::linear(0.15))).1;
        shown(1.0);
        assert_eq!(shown(1.5), focused);
        h.window_focused.set(false);
        assert_eq!(shown(1.6), focused);
        assert_eq!(shown(2.5), focused);
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(button_id(1)));
    }
}
