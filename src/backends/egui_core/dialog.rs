//! One dialog: its egui context, content, keyboard policy, clock, presenter and result delivery.
//!
//! Life cycle: [`Dialog::new`] builds the egui context (fonts, styles, `core_options`), runs the
//! measure pass ([`Theme::ui`]), applies the keyboard on-open step and resets tweens. The owner
//! (the runtime with a winit window, or the offscreen harness) then sizes its surface to
//! [`Dialog::desired_size`], [`Dialog::attach`]es a presenter and calls [`Dialog::frame`] for every
//! repaint. Input goes through [`Dialog::handle_events`]
//! (`egui::Event`s in points, from egui-winit or the offscreen harness): pointer and wheel events
//! to egui, keys to the keyboard policy. A dialog is closed once it has delivered its result
//! ([`Dialog::is_closed`]); the owner then hides and drops its window.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Duration, Instant};

use egui::{Event, PointerButton, Pos2, Rect, Vec2, ViewportId};

use super::anim;
use super::appearance::{resolve_appearance, Appearance};
use super::clock::{DialogClock, Schedule, Wants};
use super::fonts::{font_definitions, FontRegistry};
use super::keyboard::{KeyAction, KeyboardState};
use super::render::{Presenter, RenderFrame};
use super::theme::{core_options, install_style, DialogKind, DialogUiOutput, DialogView, ProgressView, Theme};
use crate::model::{ResultSender, XDialogIcon, XDialogOptions, XDialogResult, XDialogTheme};
use crate::{ProgressButtonCallback, ProgressDialogProxy};

/// Largest font-atlas side the software raster accepts.
pub(crate) const MAX_TEXTURE_SIDE: usize = 8192;

/// Screen width (logical px) of the measure pass. Wide enough for any dialog: themes apply their
/// own width rules.
const MEASURE_WIDTH: f32 = 4096.0;

/// What the dialog shows (API strings, unchanged).
#[derive(Clone, Debug)]
pub(crate) struct DialogContent {
    pub title: String,
    pub heading: String,
    pub body: String,
    pub icon: XDialogIcon,
    pub buttons: Vec<String>,
    pub progress: Option<ProgressView>,
}

impl DialogContent {
    /// Content of a message (`Message`) or progress (`Progress`, starting determinate at 0) dialog.
    pub(crate) fn new(kind: DialogKind, options: XDialogOptions) -> Self {
        DialogContent { title: options.title,
                        heading: options.main_instruction,
                        body: options.message,
                        icon: options.icon,
                        buttons: options.buttons,
                        progress: (kind == DialogKind::Progress).then_some(ProgressView::Determinate { value: 0.0 }) }
    }

    /// Every string the theme may draw (font coverage check).
    fn texts(&self) -> Vec<&str> {
        let mut v = vec![self.heading.as_str(), self.body.as_str()];
        v.extend(self.buttons.iter().map(String::as_str));
        v
    }
}

/// Construction parameters.
pub(crate) struct DialogParams {
    /// Request id (the `ProgressDialogProxy` id handed to callbacks).
    pub id: usize,
    pub content: DialogContent,
    pub appearance: Appearance,
    /// `Some`: the appearance is re-resolved from the platform with this override on
    /// `ThemeChanged` / portal changes. `None`: injected (offscreen harness), never re-resolved.
    pub system_appearance: Option<XDialogTheme>,
    /// Pixels per point the window will most likely have (measure pass).
    pub ppp: f32,
    /// Max client height, logical px ([`DialogView::max_height`]).
    pub max_height: f32,
    pub clock: DialogClock,
    /// Where the result goes (`None`: offscreen harness).
    pub sender: Option<ResultSender>,
}

/// Where a cancelled press is released: far outside every widget, so the release can't activate
/// anything (`ButtonInteraction::activated` requires the pointer inside the button).
const CANCEL_POS: Pos2 = Pos2::new(-1.0e6, -1.0e6);

/// Font-coverage bookkeeping.
#[derive(Clone, Copy, Debug, Default)]
struct FontState {
    /// Number of fallback faces (process registry) installed in this context; `None` before the
    /// first `set_fonts`.
    fallbacks: Option<usize>,
    /// All visible characters were covered (or are known uncoverable).
    complete: bool,
}

/// One dialog (no window: see the module docs).
pub(crate) struct Dialog {
    id: usize,
    ctx: egui::Context,
    theme: Box<dyn Theme>,
    appearance: Appearance,
    system_appearance: Option<XDialogTheme>,
    content: DialogContent,
    max_height: f32,
    clock: DialogClock,
    keyboard: KeyboardState,
    /// egui holds a primary press (a press was forwarded, no release yet).
    primary_down: bool,
    /// egui events queued for the next pass.
    egui_events: Vec<egui::Event>,
    /// The last pass's theme output.
    out: DialogUiOutput,
    /// Texture changes not presented yet (the measure pass's atlas upload, frames without a
    /// presenter).
    textures: egui::TexturesDelta,
    presenter: Option<Box<dyn Presenter>>,
    ppp: f32,
    /// Client size in physical px.
    size_px: [u32; 2],
    window_focused: bool,
    /// The logical size last requested from (or created by) the window system.
    requested: Vec2,
    resize: Option<Vec2>,
    titlebar_changed: bool,
    sender: Option<ResultSender>,
    /// Progress dialogs with `show_progress_with_callback` (set by `attach`).
    callback: Option<ProgressButtonCallback>,
    result: Option<XDialogResult>,
    schedule: Schedule,
    /// Frames presented (host test hooks).
    #[cfg(any(test, all(feature = "winit-host", feature = "_test-hooks")))]
    frames: u64,
    fonts: FontState,
    /// The next pass consumes an `egui::Event::PointerGone`: egui clears the pointer's hover
    /// position only on the pass after that, so one more frame follows it (no stale hover).
    pointer_gone: bool,
}

impl Dialog {
    /// Build the context, install fonts, run the measure pass ([`Theme::ui`]), focus the default
    /// button.
    pub(crate) fn new(mut theme: Box<dyn Theme>, p: DialogParams) -> Self {
        theme.set_appearance(&p.appearance);
        let mut d = Dialog { id: p.id,
                             ctx: egui::Context::default(),
                             keyboard: KeyboardState::new(theme.keyboard_policy()),
                             theme,
                             appearance: p.appearance,
                             system_appearance: p.system_appearance,
                             content: p.content,
                             max_height: p.max_height,
                             clock: p.clock,
                             primary_down: false,
                             egui_events: Vec::new(),
                             out: DialogUiOutput::default(),
                             textures: egui::TexturesDelta::default(),
                             presenter: None,
                             ppp: p.ppp,
                             size_px: [0, 0],
                             window_focused: true,
                             requested: Vec2::ZERO,
                             resize: None,
                             titlebar_changed: false,
                             sender: p.sender,
                             callback: None,
                             result: None,
                             schedule: Schedule::default(),
                             #[cfg(any(test, all(feature = "winit-host", feature = "_test-hooks")))]
                             frames: 0,
                             fonts: FontState::default(),
                             pointer_gone: false };
        d.ctx.options_mut(core_options);
        install_style(&d.ctx, &*d.theme, d.appearance.dark);
        d.update_fonts(true);

        // Measure pass: keep the atlas upload, drop the shapes and the repaint request.
        let mut full = d.run_pass(true);
        d.textures.append(std::mem::take(&mut full.textures_delta));
        d.requested = d.out.desired_size;

        let (ctx, out) = (d.ctx.clone(), d.out.clone());
        d.keyboard.on_open(&ctx, &out);
        // Tweens snap on the first real frame (the measure pass saw pre-focus targets).
        anim::reset_animations(&d.ctx);
        d
    }

    /// Make sure every visible character is covered (discovering fallback faces if needed) and
    /// (re)install the font definitions when the set of fallbacks changed. Takes effect at the
    /// start of the next pass.
    fn update_fonts(&mut self, wait: bool) {
        let reg = FontRegistry::global();
        let fonts = self.theme.fonts();
        self.fonts.complete = reg.ensure_coverage(&fonts, &self.content.texts(), wait);
        let fallbacks = reg.fallbacks();
        if self.fonts.fallbacks != Some(fallbacks.len()) {
            self.ctx.set_fonts(font_definitions(&fonts, &fallbacks));
            self.fonts.fallbacks = Some(fallbacks.len());
        }
    }

    /// One egui pass (`run_ui` exactly once) building the theme into the root ui.
    fn run_pass(&mut self, sizing: bool) -> egui::FullOutput {
        let now = self.clock.now();
        let frame = self.keyboard.frame_info(&self.ctx, &self.out);
        let screen = if sizing { Vec2::new(MEASURE_WIDTH, self.max_height) } else { self.client_logical() };
        let mut raw = egui::RawInput { time: Some(now),
                                       predicted_dt: 1.0 / 60.0,
                                       screen_rect: Some(Rect::from_min_size(Pos2::ZERO, screen)),
                                       max_texture_side: Some(MAX_TEXTURE_SIDE),
                                       focused: !sizing && self.window_focused,
                                       events: if sizing { Vec::new() } else { std::mem::take(&mut self.egui_events) },
                                       ..Default::default() };
        raw.viewports.entry(ViewportId::ROOT).or_default().native_pixels_per_point = Some(self.ppp);

        let c = &self.content;
        let view = DialogView { heading: &c.heading,
                                body: &c.body,
                                icon: &c.icon,
                                buttons: &c.buttons,
                                progress: c.progress,
                                max_height: self.max_height,
                                frame };
        let theme = &self.theme;
        let mut out = DialogUiOutput::default();
        let full = self.ctx.run_ui(raw, |ui| out = theme.ui(&view, ui));
        self.out = out;
        full
    }

    /// Client size in logical px (the requested size until the window reports one).
    fn client_logical(&self) -> Vec2 {
        if self.size_px[0] == 0 || self.size_px[1] == 0 {
            self.requested
        } else {
            Vec2::new(self.size_px[0] as f32, self.size_px[1] as f32) / self.ppp
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Window attachment and frames
    // ---------------------------------------------------------------------------------------------

    /// Attach the window's presenter and metrics (physical client size).
    pub(crate) fn attach(&mut self, presenter: Box<dyn Presenter>, ppp: f32, size_px: [u32; 2]) {
        self.presenter = Some(presenter);
        self.ppp = ppp;
        self.size_px = size_px;
        let want = self.physical_size(self.ppp);
        if size_px != want {
            // The window got a different size (other DPI than expected, WM constraints): ask again.
            self.resize = Some(self.out.desired_size);
        }
    }

    /// The progress button callback (handed over only once nothing can fail any more).
    pub(crate) fn set_callback(&mut self, callback: Option<ProgressButtonCallback>) {
        self.callback = callback;
    }

    /// Run one frame: one egui pass, pointer activation, resize check, tessellate + present,
    /// schedule the next frame. `Err`: presenting failed (the frame is otherwise complete).
    pub(crate) fn frame(&mut self) -> Result<(), String> {
        if self.is_closed() {
            return Ok(());
        }
        let pointer_gone = std::mem::take(&mut self.pointer_gone);
        let mut full = self.run_pass(false);
        self.textures.append(std::mem::take(&mut full.textures_delta));
        if let Some(i) = self.out.activated {
            self.activate(i);
            if self.is_closed() {
                return Ok(());
            }
        }

        // Content changed size (set_text reflow, DPI rounding): ask the window system.
        let desired = self.out.desired_size;
        if ((desired - self.requested) * self.ppp).abs().max_elem() > 0.5 {
            self.requested = desired;
            self.resize = Some(desired);
        }

        let mut presented = Ok(());
        if let Some(presenter) = self.presenter.as_mut() {
            let prims = self.ctx.tessellate(std::mem::take(&mut full.shapes), full.pixels_per_point);
            let clear = self.ctx.global_style().visuals.panel_fill;
            let frame = RenderFrame { prims: &prims, textures: &self.textures, size_px: self.size_px, ppp: full.pixels_per_point, clear };
            presented = presenter.present(frame);
            self.textures.clear();
        }
        #[cfg(any(test, all(feature = "winit-host", feature = "_test-hooks")))]
        {
            self.frames += 1;
        }

        let delay = full.viewport_output.get(&ViewportId::ROOT).map_or(Duration::MAX, |v| v.repaint_delay);
        let smooth = super::anim::take_smooth_request(&self.ctx);
        let wants = Wants { immediate: pointer_gone, delay: (delay < Duration::from_secs(3600)).then_some(delay), smooth };
        self.schedule.after_frame(Instant::now(), wants);
        presented
    }

    // ---------------------------------------------------------------------------------------------
    // Input
    // ---------------------------------------------------------------------------------------------

    /// Handle input events (points; from egui-winit or the offscreen harness). Pointer and wheel
    /// events are queued for egui; keys never reach egui, they run the keyboard policy now (before
    /// the next pass). Only the primary button reaches egui (in egui a secondary press would start
    /// a press on the widget under it and block primary presses until released).
    pub(crate) fn handle_events(&mut self, events: impl IntoIterator<Item = Event>) {
        for ev in events {
            if self.is_closed() {
                return;
            }
            match ev {
                Event::Key { .. } => self.keyboard_event(&ev),
                // egui-winit already emulates the primary touch with pointer events.
                Event::Touch { phase, .. } => {
                    if phase == egui::TouchPhase::Cancel {
                        self.cancel_press();
                    }
                }
                Event::PointerButton { button: PointerButton::Primary, pressed: true, .. } => {
                    self.primary_down = true;
                    self.keyboard_event(&ev);
                    self.egui_events.push(ev);
                }
                // A release is forwarded once per forwarded press (not again after a cancel).
                Event::PointerButton { button: PointerButton::Primary, pressed: false, .. } => {
                    if std::mem::take(&mut self.primary_down) {
                        self.egui_events.push(ev);
                    }
                }
                // Only changes count: X11 reports a new window's focus state some time after it
                // is shown, and a press made meanwhile must not be cancelled by it.
                Event::WindowFocused(focused) if focused == self.window_focused => {}
                Event::WindowFocused(focused) => {
                    self.window_focused = focused;
                    if focused {
                        // Nothing reports an accent-colour change on Windows: re-read the system
                        // appearance whenever the dialog gains focus.
                        self.refresh_appearance();
                    } else {
                        // The release may never arrive (capture lost): drop the held press.
                        self.cancel_press();
                    }
                    self.keyboard_event(&ev);
                    self.egui_events.push(ev);
                }
                Event::PointerGone => {
                    self.pointer_gone = true;
                    self.egui_events.push(ev);
                }
                Event::PointerMoved(_) | Event::MouseWheel { .. } | Event::ModifiersChanged(_) => self.egui_events.push(ev),
                _ => {}
            }
        }
        self.schedule.asap(Instant::now());
    }

    /// Release a primary press egui still holds far away from every widget (so it can't activate
    /// anything), then report the pointer gone for this pass; the next move restores hover.
    pub(crate) fn cancel_press(&mut self) {
        if std::mem::take(&mut self.primary_down) {
            self.egui_events.push(Event::PointerButton { pos: CANCEL_POS, button: PointerButton::Primary, pressed: false, modifiers: Default::default() });
            self.egui_events.push(Event::PointerGone);
            self.pointer_gone = true;
        }
    }

    /// New client size in physical px (a zero side = minimised).
    pub(crate) fn resized(&mut self, size_px: [u32; 2]) {
        self.size_px = size_px;
        self.schedule.asap(Instant::now());
    }

    /// New scale factor; the window system keeps the logical size and reports the new physical
    /// size with the next [`Dialog::resized`].
    pub(crate) fn scale_changed(&mut self, ppp: f32) {
        self.ppp = ppp;
        self.schedule.asap(Instant::now());
    }

    /// Run the keyboard policy for one event and apply its action.
    fn keyboard_event(&mut self, ev: &Event) {
        let ctx = self.ctx.clone();
        let client_h = self.client_logical().y;
        match self.keyboard.on_event(&ctx, ev, &self.out, client_h) {
            KeyAction::None => {}
            KeyAction::Activate(b) => self.activate(b),
            KeyAction::Close => self.finish(XDialogResult::WindowClosed),
        }
    }

    /// Activate button `i` (pointer or keyboard): message -> `ButtonPressed(i)` and close;
    /// progress with a callback -> run it (on this thread, panics caught) and close unless it
    /// returns `true`; progress without a callback -> `ButtonPressed(i)` and close.
    pub(crate) fn activate(&mut self, i: usize) {
        if self.is_closed() || i >= self.content.buttons.len() {
            return;
        }
        let keep = match self.callback.as_mut() {
            Some(cb) => {
                let proxy = ProgressDialogProxy::non_owning(self.id);
                match catch_unwind(AssertUnwindSafe(|| cb(i, &proxy))) {
                    Ok(keep) => keep,
                    Err(_) => {
                        error!("xdialog: a progress button callback panicked; closing the dialog");
                        false
                    }
                }
            }
            None => false,
        };
        if keep {
            self.schedule.asap(Instant::now());
        } else {
            self.finish(XDialogResult::ButtonPressed(i));
        }
    }

    /// Where the result goes, once the dialog is shown (until then a failure is the caller's error,
    /// not a result).
    pub(crate) fn set_sender(&mut self, sender: ResultSender) {
        self.sender = Some(sender);
    }

    /// Deliver `result` (if none was delivered yet): the dialog is closed from now on.
    pub(crate) fn finish(&mut self, result: XDialogResult) {
        if self.result.is_none() {
            if let Some(tx) = self.sender.take() {
                tx.send(result.clone());
            }
            self.result = Some(result);
        }
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.result.is_some()
    }

    // ---------------------------------------------------------------------------------------------
    // Requests
    // ---------------------------------------------------------------------------------------------

    pub(crate) fn set_progress_value(&mut self, value: f32) {
        let value = if value.is_finite() { value.clamp(0.0, 1.0) } else { 0.0 };
        if self.content.progress.is_none() {
            return;
        }
        self.content.progress = Some(ProgressView::Determinate { value });
        self.schedule.asap(Instant::now());
    }

    pub(crate) fn set_progress_indeterminate(&mut self) {
        let now = self.clock.now();
        self.content.progress = match self.content.progress {
            None => return,
            Some(ProgressView::Indeterminate { since, .. }) => Some(ProgressView::Indeterminate { since, restarted_at: now }),
            Some(ProgressView::Determinate { .. }) => Some(ProgressView::Indeterminate { since: now, restarted_at: now }),
        };
        self.schedule.asap(Instant::now());
    }

    /// New body text (`set_text`): fonts are checked; the next pass relayouts and may resize.
    /// Never waits for the system-font scan (this runs on the shared UI thread, possibly many times
    /// a second): an incomplete check is retried by [`Dialog::refresh_fonts`] when the scan ends.
    pub(crate) fn set_text(&mut self, text: &str) {
        if self.content.body == text {
            return;
        }
        self.content.body = text.to_owned();
        self.update_fonts(false);
        self.schedule.asap(Instant::now());
    }

    /// A background font discovery step finished: retry an incomplete coverage check.
    pub(crate) fn refresh_fonts(&mut self) {
        if !self.fonts.complete || Some(FontRegistry::global().fallbacks().len()) != self.fonts.fallbacks {
            self.update_fonts(false);
            self.schedule.asap(Instant::now());
        }
    }

    /// Re-resolve the platform appearance (portal change); no-op for a fixed appearance.
    pub(crate) fn refresh_appearance(&mut self) {
        if let Some(xt) = self.system_appearance {
            let a = resolve_appearance(xt);
            self.set_appearance(a);
        }
    }

    /// Switch appearance: new tokens and styles, tweens snap (no fade between palettes).
    pub(crate) fn set_appearance(&mut self, a: Appearance) {
        if a == self.appearance {
            return;
        }
        self.titlebar_changed |= a.dark != self.appearance.dark;
        self.appearance = a;
        self.theme.set_appearance(&a);
        install_style(&self.ctx, &*self.theme, a.dark);
        anim::reset_animations(&self.ctx);
        self.schedule.asap(Instant::now());
    }

    // ---------------------------------------------------------------------------------------------
    // Accessors
    // ---------------------------------------------------------------------------------------------

    /// The egui context (egui-winit's input translation reads its zoom factor).
    pub(crate) fn ctx(&self) -> &egui::Context {
        &self.ctx
    }

    pub(crate) fn title(&self) -> &str {
        &self.content.title
    }

    /// Logical client size the content wants (measure pass / last pass).
    pub(crate) fn desired_size(&self) -> Vec2 {
        self.out.desired_size
    }

    /// `round(desired * ppp)` physical px (at least 1x1): what winit makes of a logical inner size
    /// (`LogicalSize::to_physical` rounds), so the window and the offscreen harness agree and
    /// `attach` doesn't re-request a size the window system can never give.
    pub(crate) fn physical_size(&self, ppp: f32) -> [u32; 2] {
        let s = self.out.desired_size * ppp;
        [(s.x.round() as u32).max(1), (s.y.round() as u32).max(1)]
    }

    /// A pending window resize request (logical px), taken.
    pub(crate) fn take_resize(&mut self) -> Option<Vec2> {
        self.resize.take()
    }

    /// The dark-title-bar flag changed since the last call (appearance switch).
    pub(crate) fn take_titlebar_change(&mut self) -> Option<bool> {
        std::mem::take(&mut self.titlebar_changed).then_some(self.appearance.dark)
    }

    /// Dark title bar (DWMWA_USE_IMMERSIVE_DARK_MODE / winit `with_theme(Dark)`).
    pub(crate) fn dark_titlebar(&self) -> bool {
        self.appearance.dark
    }

    pub(crate) fn schedule_mut(&mut self) -> &mut Schedule {
        &mut self.schedule
    }
}

/// Introspection for the offscreen harness, the host test hooks and unit tests.
#[cfg(any(test, feature = "_test-hooks"))]
impl Dialog {
    /// The delivered result, once closed.
    pub(crate) fn result(&self) -> Option<&XDialogResult> {
        self.result.as_ref()
    }

    pub(crate) fn ppp(&self) -> f32 {
        self.ppp
    }

    pub(crate) fn size_px(&self) -> [u32; 2] {
        self.size_px
    }

    /// Button rects in LOGICAL px `[x, y, w, h]`, indexed by API button index (zeros for a
    /// button the theme didn't lay out).
    pub(crate) fn button_rects(&self) -> Vec<[f32; 4]> {
        let mut v = vec![[0.0; 4]; self.content.buttons.len()];
        for b in &self.out.buttons {
            if let Some(slot) = v.get_mut(b.index) {
                *slot = [b.rect.min.x, b.rect.min.y, b.rect.width(), b.rect.height()];
            }
        }
        v
    }

    #[cfg(any(test, feature = "winit-host"))]
    pub(crate) fn frames(&self) -> u64 {
        self.frames
    }

    /// Fix the dialog clock at `t` seconds; repaints.
    pub(crate) fn freeze_clock(&mut self, t: f64) {
        self.clock.freeze(t);
        self.schedule.asap(Instant::now());
    }

    /// The presenter (offscreen harness reads its pixels).
    pub(crate) fn presenter(&self) -> Option<&dyn Presenter> {
        self.presenter.as_deref()
    }
}

impl Drop for Dialog {
    fn drop(&mut self) {
        // epaint debug-asserts that texture deltas are never dropped unapplied.
        self.textures.clear();
        if let Some(tx) = self.sender.take() {
            tx.send(XDialogResult::WindowClosed);
        }
    }
}

#[cfg(test)]
mod tests {
    //! State-machine tests through real egui passes with a stub theme and a memory presenter.

    use super::*;
    use crate::backends::egui_core::render::MemoryPresenter;
    use crate::backends::egui_core::fonts::{bundled, ThemeFonts};
    use crate::backends::egui_core::keyboard::focused_button;
    use crate::backends::egui_core::text::{layout, TextBlockWidget, TextStyle};
    use crate::backends::egui_core::theme::{ButtonInteraction, KeyboardPolicy};
    use egui::Key;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Minimal theme: optional heading line, body block, a row of 80x30 buttons at y = content.
    struct StubTheme {
        dark: bool,
    }

    impl Theme for StubTheme {
        fn set_appearance(&mut self, appearance: &Appearance) {
            self.dark = appearance.dark;
        }
        fn keyboard_policy(&self) -> KeyboardPolicy {
            crate::backends::egui_ubuntu::KEYBOARD
        }
        fn fonts(&self) -> ThemeFonts {
            ThemeFonts::new(bundled::UBUNTU_REGULAR, bundled::UBUNTU_BOLD)
        }
        fn configure_style(&self, style: &mut egui::Style) {
            style.visuals.panel_fill = if self.dark { egui::Color32::BLACK } else { egui::Color32::WHITE };
        }
        fn ui(&self, view: &DialogView<'_>, ui: &mut egui::Ui) -> DialogUiOutput {
            let ctx = ui.ctx().clone();
            // Whole physical pixels at 1x, 1.5x and 2x, so the layout is the same at every tested scale.
            let style = TextStyle::regular(14.0, 16.0);
            let body = layout(&ctx, view.body, &style, 200.0, None);
            let color = if self.dark { egui::Color32::WHITE } else { egui::Color32::BLACK };
            ui.put(Rect::from_min_size(Pos2::new(10.0, 10.0), body.size), TextBlockWidget { block: &body, color, width: body.size.x });
            let top = 20.0 + body.size.y;
            let n = view.buttons.len();
            let mut out = DialogUiOutput::default();
            for i in 0..n {
                let rect = Rect::from_min_size(Pos2::new(10.0 + 90.0 * i as f32, top), Vec2::new(80.0, 30.0));
                let st = ButtonInteraction::interact(ui, rect, i, view);
                let fill = anim::animate(ui.ctx(), st.response.id, if st.hovered { egui::Color32::RED } else { egui::Color32::GRAY }, anim::Transition::linear(0.15));
                ui.painter().rect_filled(rect, 0.0, fill);
                out.push_button(&st);
            }
            out.desired_size = Vec2::new(220.0 + 90.0 * n.saturating_sub(2) as f32, top + 40.0);
            out
        }
    }

    struct Rig {
        d: Dialog,
        rx: Option<crate::oneshot::Receiver<Result<XDialogResult, crate::XDialogError>>>,
    }

    impl Rig {
        fn new(kind: DialogKind, buttons: &[&str], callback: Option<ProgressButtonCallback>) -> Rig {
            let options = XDialogOptions { title: "t".into(),
                                           main_instruction: String::new(),
                                           message: "Hello world, this is a body text.".into(),
                                           icon: XDialogIcon::None,
                                           buttons: buttons.iter().map(|s| s.to_string()).collect() };
            let (tx, rx) = crate::oneshot::channel();
            let params = DialogParams { id: 7,
                                        content: DialogContent::new(kind, options),
                                        appearance: Appearance::default(),
                                        system_appearance: None,
                                        ppp: 1.0,
                                        max_height: 800.0,
                                        clock: DialogClock::frozen(0.0),
                                        sender: Some(crate::model::DialogReply::Message(tx).opened()) };
            let mut d = Dialog::new(Box::new(StubTheme { dark: false }), params);
            let size = d.physical_size(1.0);
            d.attach(Box::new(MemoryPresenter::new()), 1.0, size);
            d.set_callback(callback);
            Rig { d, rx: Some(rx) }
        }

        fn at(&mut self, t: f64) {
            self.d.freeze_clock(t);
            self.d.frame().unwrap();
        }

        fn ev(&mut self, ev: Event) {
            self.d.handle_events([ev]);
        }

        fn center(&self, i: usize) -> Pos2 {
            let r = self.d.button_rects()[i];
            Pos2::new(r[0] + r[2] / 2.0, r[1] + r[3] / 2.0)
        }

        fn result(&mut self) -> Option<XDialogResult> {
            self.rx.as_ref().and_then(|rx| rx.try_recv().ok()?.ok())
        }

        fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
            let (w, _, px) = self.d.presenter().unwrap().read_rgba().unwrap();
            let i = ((y * w + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        }
    }

    fn key(key: Key, pressed: bool) -> Event {
        Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Default::default() }
    }

    fn primary(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Default::default() }
    }

    #[test]
    fn measure_pass_sizes_and_first_frame_presents() {
        let mut r = Rig::new(DialogKind::Message, &["Cancel", "OK"], None);
        let want = r.d.desired_size();
        assert!(want.x == 220.0 && want.y > 60.0, "{want:?}");
        assert_eq!(r.d.size_px(), r.d.physical_size(1.0));
        r.at(0.0);
        assert!(r.d.presenter().unwrap().read_rgba().is_some());
        assert_eq!(r.d.frames(), 1);
        assert_eq!(r.d.take_resize(), None);
        // Default (last) button focused on open.
        assert_eq!(focused_button(&r.d.ctx, &r.d.out), Some(1));
    }

    #[test]
    fn click_activates_message_and_hover_animates() {
        let mut r = Rig::new(DialogKind::Message, &["Cancel", "OK"], None);
        r.at(0.0);
        let c = r.center(0);
        let (px, py) = (c.x as u32, c.y as u32);
        assert_eq!(r.pixel(px, py), [160, 160, 160, 255]); // Color32::GRAY
        r.ev(Event::PointerMoved(c));
        r.at(1.0); // hover starts (old value shown)
        r.at(1.075); // half-way
        let mid = r.pixel(px, py);
        assert!(mid[0] > 150 && mid[0] < 250 && mid[1] < 128, "{mid:?}");
        r.at(1.2);
        assert_eq!(r.pixel(px, py), [255, 0, 0, 255]);
        r.ev(primary(c, true));
        r.at(1.3);
        assert!(!r.d.is_closed());
        r.ev(primary(c, false));
        r.at(1.4);
        assert!(r.d.is_closed());
        assert_eq!(r.result(), Some(XDialogResult::ButtonPressed(0)));
    }

    #[test]
    fn focus_loss_cancels_a_held_press() {
        let mut r = Rig::new(DialogKind::Message, &["Cancel", "OK"], None);
        r.at(0.0);
        let c = r.center(0);
        r.ev(Event::PointerMoved(c));
        r.ev(primary(c, true));
        r.at(0.1);
        r.ev(Event::WindowFocused(false));
        // The real release that may follow is not forwarded a second time.
        r.ev(primary(c, false));
        r.at(0.2);
        assert!(!r.d.is_closed());
        // Secondary buttons never reach egui.
        r.ev(Event::PointerButton { pos: c, button: PointerButton::Secondary, pressed: true, modifiers: Default::default() });
        r.ev(Event::PointerButton { pos: c, button: PointerButton::Secondary, pressed: false, modifiers: Default::default() });
        r.at(0.3);
        assert!(!r.d.is_closed());
    }

    #[test]
    fn keyboard_activation_and_escape() {
        let mut r = Rig::new(DialogKind::Message, &["A", "B", "C"], None);
        r.at(0.0);
        r.ev(key(Key::Tab, true));
        r.at(0.1);
        r.ev(key(Key::Enter, true));
        assert_eq!(r.result(), Some(XDialogResult::ButtonPressed(0)));

        let mut r = Rig::new(DialogKind::Message, &["A", "B"], None);
        r.at(0.0);
        r.ev(key(Key::Escape, true));
        assert!(r.d.is_closed());
        assert_eq!(r.result(), Some(XDialogResult::WindowClosed));

        let mut r = Rig::new(DialogKind::Message, &[], None);
        r.at(0.0);
        r.d.finish(XDialogResult::WindowClosed);
        assert_eq!(r.result(), Some(XDialogResult::WindowClosed));
    }

    #[test]
    fn progress_callback_keep_open_and_panic() {
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        let cb: ProgressButtonCallback = Box::new(|i, _p| {
            CALLS.fetch_add(1, Ordering::SeqCst);
            assert_eq!(i, 0);
            true
        });
        let mut r = Rig::new(DialogKind::Progress, &["Cancel"], Some(cb));
        r.at(0.0);
        r.ev(key(Key::Enter, true));
        assert_eq!(CALLS.load(Ordering::SeqCst), 1);
        assert!(!r.d.is_closed());

        let cb: ProgressButtonCallback = Box::new(|_, _| panic!("boom"));
        let mut r = Rig::new(DialogKind::Progress, &["Cancel"], Some(cb));
        r.at(0.0);
        r.ev(key(Key::Space, true));
        assert!(r.d.is_closed());
        assert_eq!(r.result(), Some(XDialogResult::ButtonPressed(0)));

        // Without a callback, a button click closes with ButtonPressed.
        let mut r = Rig::new(DialogKind::Progress, &["Hide"], None);
        r.at(0.0);
        r.ev(key(Key::Enter, true));
        assert_eq!(r.result(), Some(XDialogResult::ButtonPressed(0)));
    }

    #[test]
    fn progress_requests_update_view() {
        let mut r = Rig::new(DialogKind::Progress, &[], None);
        r.at(0.0);
        r.d.set_progress_value(0.5);
        assert_eq!(r.d.content.progress, Some(ProgressView::Determinate { value: 0.5 }));
        r.at(1.0);
        r.d.set_progress_indeterminate();
        r.at(2.0);
        r.d.set_progress_indeterminate();
        assert_eq!(r.d.content.progress, Some(ProgressView::Indeterminate { since: 1.0, restarted_at: 2.0 }));
        r.d.set_progress_value(2.0);
        assert_eq!(r.d.content.progress, Some(ProgressView::Determinate { value: 1.0 }));
        // A longer body grows the window.
        let h = r.d.desired_size().y;
        r.d.set_text(&"many words ".repeat(40));
        r.at(3.0);
        assert!(r.d.desired_size().y > h);
        let want = r.d.take_resize().expect("resize requested");
        assert_eq!(want, r.d.desired_size());
        // Message dialogs ignore progress requests.
        let mut m = Rig::new(DialogKind::Message, &["OK"], None);
        m.d.set_progress_value(0.3);
        assert_eq!(m.d.content.progress, None);
    }

    #[test]
    fn drop_before_result_sends_window_closed() {
        let mut r = Rig::new(DialogKind::Message, &["OK"], None);
        let rx = r.rx.take().unwrap();
        assert_eq!(r.d.result(), None);
        drop(r);
        assert!(matches!(rx.try_recv(), Ok(Ok(XDialogResult::WindowClosed))));
    }

    #[test]
    fn scale_factor_change_keeps_logical_size() {
        let mut r = Rig::new(DialogKind::Message, &["OK"], None);
        r.at(0.0);
        r.d.scale_changed(2.0);
        assert_eq!(r.d.ppp(), 2.0);
        let want = r.d.physical_size(2.0);
        r.d.resized(want);
        r.at(0.1);
        assert_eq!(r.d.take_resize(), None, "same logical size: no resize request");
        assert_eq!(r.d.presenter().unwrap().read_rgba().unwrap().0, want[0]);
        // Zero size (minimised) skips drawing without error.
        r.d.resized([0, 0]);
        r.at(0.2);
    }

    /// Characters the theme's faces lack get a system fallback face before the measure pass.
    #[cfg(windows)]
    #[test]
    fn cjk_body_gets_a_fallback_face() {
        let reg = FontRegistry::global();
        if reg.windows_font("msyh.ttc", 0).is_none() && reg.windows_font("YuGothM.ttc", 0).is_none() {
            return; // no CJK fonts installed
        }
        let mut r = Rig::new(DialogKind::Message, &["确定"], None);
        r.d.set_text("你好，世界");
        r.at(0.0);
        let id = egui::FontId::new(14.0, egui::FontFamily::Proportional);
        assert!(r.d.ctx.fonts_mut(|f| f.has_glyphs(&id, "你好世界确定")));
    }

    #[test]
    fn appearance_switch_resets_tokens() {
        let mut r = Rig::new(DialogKind::Message, &["OK"], None);
        r.at(0.0);
        assert_eq!(r.pixel(1, 1), [255, 255, 255, 255]);
        r.d.set_appearance(Appearance { dark: true, accent: None });
        assert_eq!(r.d.take_titlebar_change(), Some(true));
        r.at(0.1);
        assert_eq!(r.pixel(1, 1), [0, 0, 0, 255]);
    }
}
