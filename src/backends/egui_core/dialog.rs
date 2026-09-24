//! One dialog: its egui context, content, keyboard policy, clock, presenter and result delivery.
//!
//! Life cycle: [`Dialog::new`] builds the egui context (fonts, styles, `core_options`), runs the
//! **measure pass** (an ordinary root-ui pass whose shapes are dropped and whose font-atlas upload
//! is kept for the first present), applies the keyboard on-open step and resets tweens. The window
//! system then creates the window at [`Dialog::desired_size`], [`Dialog::attach`]es a presenter and
//! calls [`Dialog::frame`] for every repaint. Input goes through [`Dialog::handle_event`]: pointer
//! and wheel events to egui, keys to the keyboard policy. A dialog is closed once it has delivered
//! its result ([`Dialog::is_closed`]); the owner then hides and destroys its window.
//!
//! Window-system independent: the manager (own loop / host mode) and the offscreen harness drive
//! the same code.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Pos2, Rect, Vec2, ViewportId};

use super::anim;
use super::appearance::{resolve_appearance, Appearance};
use super::clock::{DialogClock, Schedule, Wants};
use super::fonts::FontRegistry;
use super::input::{CoreInput, InputTranslator};
use super::keyboard::{focused_button, KeyAction, KeyboardState};
use super::render::{Presenter, RenderFrame};
use super::theme::{
    button_id, core_options, core_style_overrides, DialogKind, DialogUiOutput, DialogView, FrameInfo, Platform, ProgressView,
    SizeLimits, Theme, ThemeEnv, WindowStyle,
};
use crate::backends::host_types::HostEvent;
use crate::model::{XDialogIcon, XDialogOptions, XDialogResult, XDialogTheme};
use crate::{ProgressButtonCallback, ProgressDialogProxy};

/// Largest font-atlas side the software raster accepts.
const MAX_TEXTURE_SIDE: usize = 8192;

/// What the dialog shows (API strings, unchanged).
#[derive(Clone, Debug)]
pub(crate) struct DialogContent {
    pub kind: DialogKind,
    pub title: String,
    pub heading: String,
    pub body: String,
    pub icon: XDialogIcon,
    pub buttons: Vec<String>,
    /// Same length as `buttons` (test hooks only).
    pub disabled: Vec<bool>,
    pub progress: Option<ProgressView>,
}

impl DialogContent {
    /// Content of a message (`Message`) or progress (`Progress`, starting determinate at 0) dialog.
    pub(crate) fn new(kind: DialogKind, options: XDialogOptions) -> Self {
        let disabled = vec![false; options.buttons.len()];
        DialogContent { kind,
                        title: options.title,
                        heading: options.main_instruction,
                        body: options.message,
                        icon: options.icon,
                        buttons: options.buttons,
                        disabled,
                        progress: (kind == DialogKind::Progress).then_some(ProgressView::Determinate { value: 0.0, prev: 0.0, changed_at: 0.0 }) }
    }

    /// Every string the theme may draw (font coverage check).
    fn texts(&self) -> Vec<&str> {
        let mut v = vec![self.heading.as_str(), self.body.as_str()];
        v.extend(self.buttons.iter().map(String::as_str));
        v
    }
}

/// Where the dialog's result goes.
pub(crate) struct ResultSink {
    pub sender: Option<oneshot::Sender<XDialogResult>>,
    /// Progress dialogs with `show_progress_with_callback`.
    pub callback: Option<ProgressButtonCallback>,
}

impl ResultSink {
    pub(crate) fn none() -> Self {
        ResultSink { sender: None, callback: None }
    }
}

/// How the appearance is obtained.
#[derive(Clone, Debug)]
pub(crate) enum AppearanceSource {
    /// Resolved from the platform with this `XDialogTheme` override; re-resolved on
    /// `ThemeChanged` / portal changes.
    System(XDialogTheme),
    /// Injected (offscreen harness): never re-resolved.
    Fixed,
}

/// Construction parameters.
pub(crate) struct DialogParams {
    /// Request id (the `ProgressDialogProxy` id handed to callbacks).
    pub id: usize,
    pub content: DialogContent,
    pub appearance: Appearance,
    pub appearance_source: AppearanceSource,
    /// Pixels per point the window will most likely have (measure pass).
    pub ppp: f32,
    pub limits: SizeLimits,
    pub clock: DialogClock,
    pub sink: ResultSink,
}

/// Font-coverage bookkeeping.
#[derive(Clone, Copy, Debug, Default)]
struct FontState {
    /// Number of fallback faces (process registry) installed in this context; `None` before the
    /// first `set_fonts`.
    fallbacks: Option<usize>,
    /// All visible characters were covered (or are known uncoverable).
    complete: bool,
}

/// One dialog. Generic over the theme; independent of the window system.
pub(crate) struct Dialog<T: Theme> {
    id: usize,
    ctx: egui::Context,
    tokens: T::Tokens,
    style: WindowStyle,
    env: ThemeEnv,
    appearance_source: AppearanceSource,
    content: DialogContent,
    /// Last determinate target (so `prev` survives indeterminate phases).
    last_value: f32,
    limits: SizeLimits,
    clock: DialogClock,
    input: InputTranslator,
    keyboard: KeyboardState,
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
    sink: ResultSink,
    result: Option<XDialogResult>,
    closed: bool,
    schedule: Schedule,
    frames: u64,
    fonts: FontState,
    present_failed: bool,
    /// The next pass consumes an `egui::Event::PointerGone`: egui clears the pointer's hover
    /// position only on the pass after that, so one more frame follows it (no stale hover).
    pointer_gone: bool,
}

impl<T: Theme> Dialog<T> {
    /// Build the context, install fonts, run the measure pass, focus the default button.
    pub(crate) fn new(theme: &T, p: DialogParams) -> Self {
        let env = ThemeEnv { appearance: p.appearance, platform: Platform::current() };
        let tokens = theme.tokens(&env);
        let style = theme.window_style(&tokens);
        let ppp = sanitize_ppp(p.ppp);
        let last_value = match p.content.progress {
            Some(ProgressView::Determinate { value, .. }) => value,
            _ => 0.0,
        };
        let mut d = Dialog { id: p.id,
                             ctx: egui::Context::default(),
                             tokens,
                             style,
                             env,
                             appearance_source: p.appearance_source,
                             content: p.content,
                             last_value,
                             limits: p.limits,
                             clock: p.clock,
                             input: InputTranslator::new(),
                             keyboard: KeyboardState::new(theme.keyboard_policy()),
                             egui_events: Vec::new(),
                             out: DialogUiOutput::default(),
                             textures: egui::TexturesDelta::default(),
                             presenter: None,
                             ppp,
                             size_px: [0, 0],
                             window_focused: true,
                             requested: Vec2::ZERO,
                             resize: None,
                             titlebar_changed: false,
                             sink: p.sink,
                             result: None,
                             closed: false,
                             schedule: Schedule::default(),
                             frames: 0,
                             fonts: FontState::default(),
                             present_failed: false,
                             pointer_gone: false };
        d.init_context(theme, true);
        d
    }

    /// (Re)build the egui context: options, styles, fonts, measure pass, on-open keyboard step,
    /// tween reset. `open`: first build (else: context recreated for a new presenter; the focused
    /// button is kept).
    fn init_context(&mut self, theme: &T, open: bool) {
        let keep_focus = if open { None } else { focused_button(&self.ctx, &self.out) };
        self.textures.clear();
        self.ctx = egui::Context::default();
        self.ctx.options_mut(core_options);
        self.apply_style(theme);
        self.fonts = FontState::default();
        self.update_fonts(theme, true);

        // Measure pass: ordinary pass over the root ui at the largest allowed
        // size, no events; keep the atlas upload, drop the shapes and the repaint request.
        let mut full = self.run_pass(theme, true);
        self.textures.append(std::mem::take(&mut full.textures_delta));
        self.requested = self.out.desired_size;

        if open {
            let ctx = self.ctx.clone();
            let out = self.out.clone();
            self.keyboard.on_open(&ctx, &out, &self.content.disabled);
        } else if let Some(b) = keep_focus {
            self.ctx.memory_mut(|m| m.request_focus(button_id(b)));
        }
        // Tweens snap on the first real frame (the measure pass saw pre-focus targets).
        anim::reset_animations(&self.ctx);
    }

    fn apply_style(&self, theme: &T) {
        let dark = self.env.appearance.dark;
        self.ctx.options_mut(|o| o.theme_preference = if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
        let tk = &self.tokens;
        self.ctx.all_styles_mut(|s| {
                    theme.configure_style(tk, s);
                    core_style_overrides(s);
                });
    }

    /// Make sure every visible character is covered (discovering fallback faces if needed) and
    /// (re)install the font definitions when the set of fallbacks changed. Takes effect at the
    /// start of the next pass.
    fn update_fonts(&mut self, theme: &T, wait: bool) {
        let reg = FontRegistry::global();
        let primary = theme.primary_faces(reg);
        let cov = reg.ensure_coverage(&primary, &self.content.texts(), wait);
        let fallbacks = reg.fallbacks();
        if self.fonts.fallbacks != Some(fallbacks.len()) {
            self.ctx.set_fonts(build_fonts(theme, reg, &fallbacks));
            self.fonts.fallbacks = Some(fallbacks.len());
        }
        self.fonts.complete = cov.complete;
    }

    /// One egui pass (`run_ui` exactly once) building the theme into the root ui.
    fn run_pass(&mut self, theme: &T, sizing: bool) -> egui::FullOutput {
        let now = self.clock.now();
        let (focus_visible, key_pressed, scroll_request) = if sizing {
            (self.keyboard.focus_visible(), None, 0.0)
        } else {
            let ctx = self.ctx.clone();
            self.keyboard.frame_info_parts(&ctx, &self.out)
        };
        let screen = if sizing { Vec2::new(self.limits.max_width, self.limits.max_height) } else { self.client_logical() };
        let mut raw = egui::RawInput { time: Some(now),
                                       predicted_dt: 1.0 / 60.0,
                                       screen_rect: Some(Rect::from_min_size(Pos2::ZERO, screen)),
                                       max_texture_side: Some(MAX_TEXTURE_SIDE),
                                       focused: !sizing && self.window_focused,
                                       events: if sizing { Vec::new() } else { std::mem::take(&mut self.egui_events) },
                                       ..Default::default() };
        raw.viewports.entry(ViewportId::ROOT).or_default().native_pixels_per_point = Some(self.ppp);

        let c = &self.content;
        let view = DialogView { kind: c.kind,
                                title: &c.title,
                                heading: &c.heading,
                                body: &c.body,
                                icon: &c.icon,
                                buttons: &c.buttons,
                                disabled: &c.disabled,
                                progress: c.progress,
                                env: &self.env,
                                limits: self.limits,
                                frame: FrameInfo { time: now,
                                                   ppp: self.ppp,
                                                   sizing,
                                                   window_focused: self.window_focused,
                                                   focus_visible,
                                                   key_pressed,
                                                   scroll_request } };
        let tokens = &self.tokens;
        let mut out = DialogUiOutput::default();
        let full = self.ctx.run_ui(raw, |ui| out = theme.ui(tokens, &view, ui));
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
        self.ppp = sanitize_ppp(ppp);
        self.size_px = size_px;
        let want = self.physical_size(self.ppp);
        if size_px != want {
            // The window got a different size (other DPI than expected, WM constraints): ask again.
            self.resize = Some(self.out.desired_size);
        }
    }

    /// Drop the presenter (before the window is destroyed).
    pub(crate) fn detach(&mut self) -> Option<Box<dyn Presenter>> {
        self.presenter.take()
    }

    /// Replace the presenter after a failed present: the new one has no font atlas, so the egui
    /// context is recreated (full atlas upload; focus kept).
    pub(crate) fn replace_presenter(&mut self, theme: &T, presenter: Box<dyn Presenter>) {
        self.presenter = Some(presenter);
        self.present_failed = false;
        self.init_context(theme, false);
        self.schedule.asap(Instant::now());
    }

    /// The last `present` failed (surface lost); the owner may [`Dialog::replace_presenter`].
    pub(crate) fn take_present_failed(&mut self) -> bool {
        std::mem::take(&mut self.present_failed)
    }

    /// Run one frame: keyboard deadlines, one egui pass, pointer activation, resize check,
    /// tessellate + present, schedule the next frame.
    pub(crate) fn frame(&mut self, theme: &T) {
        if self.closed {
            return;
        }
        let now = self.clock.now();
        if let KeyAction::Activate(b) = self.keyboard.poll(now) {
            self.activate(b);
            if self.closed {
                return;
            }
        }
        let pointer_gone = std::mem::take(&mut self.pointer_gone);
        let mut full = self.run_pass(theme, false);
        self.textures.append(std::mem::take(&mut full.textures_delta));
        if let Some(i) = self.out.activated {
            self.activate(i);
            if self.closed {
                return;
            }
        }

        // Content changed size (set_text reflow, DPI rounding): ask the window system.
        let desired = self.out.desired_size;
        if ((desired - self.requested) * self.ppp).abs().max_elem() > 0.5 {
            self.requested = desired;
            self.resize = Some(desired);
        }

        if let Some(presenter) = self.presenter.as_mut() {
            let prims = self.ctx.tessellate(std::mem::take(&mut full.shapes), full.pixels_per_point);
            let frame = RenderFrame { prims: &prims, textures: &mut self.textures, size_px: self.size_px, ppp: full.pixels_per_point, clear: self.style.clear };
            if let Err(e) = presenter.present(frame) {
                warn!("xdialog: present failed: {e}");
                self.present_failed = true;
            }
            self.textures.clear();
        }
        self.frames += 1;

        let delay = full.viewport_output.get(&ViewportId::ROOT).map_or(Duration::MAX, |v| v.repaint_delay);
        let flash = self.keyboard.next_deadline().map(|t| Duration::from_secs_f64((t - self.clock.now()).max(0.0)));
        let wants = Wants { immediate: pointer_gone,
                            animation: delay.is_zero() || flash.is_some() && !self.clock.is_frozen(),
                            delayed: (!delay.is_zero() && delay < Duration::from_secs(3600)).then_some(delay),
                            at: flash };
        self.schedule.after_frame(Instant::now(), wants);
    }

    // ---------------------------------------------------------------------------------------------
    // Input
    // ---------------------------------------------------------------------------------------------

    /// Handle one window/input event (physical coordinates). Pointer and wheel events are queued
    /// for egui; keys run the keyboard policy now (before the next pass).
    pub(crate) fn handle_event(&mut self, theme: &T, ev: HostEvent) {
        if self.closed {
            return;
        }
        let mut core = Vec::new();
        let queued = self.egui_events.len();
        self.input.translate(ev, self.ppp, &mut core, &mut self.egui_events);
        self.pointer_gone |= self.egui_events[queued..].contains(&egui::Event::PointerGone);
        for c in core {
            match c {
                CoreInput::Resized(size) => self.size_px = size,
                CoreInput::ScaleFactor(s) => {
                    self.ppp = sanitize_ppp(s);
                    // Same logical size, new physical size.
                    self.resize = Some(self.out.desired_size);
                }
                CoreInput::ThemeChanged => {
                    if let AppearanceSource::System(xt) = &self.appearance_source {
                        let a = resolve_appearance(xt);
                        self.set_appearance(theme, a);
                    }
                }
                CoreInput::CloseRequested => self.finish(XDialogResult::WindowClosed),
                other => {
                    if let CoreInput::Focused(f) = other {
                        self.window_focused = f;
                        // Nothing reports an accent-colour change on Windows (winit has no event
                        // for it): re-read the system appearance whenever the dialog gains focus.
                        if f {
                            if let AppearanceSource::System(xt) = &self.appearance_source {
                                let a = resolve_appearance(xt);
                                self.set_appearance(theme, a);
                            }
                        }
                    }
                    let ctx = self.ctx.clone();
                    let client_h = self.client_logical().y;
                    let action = self.keyboard.on_input(&ctx, &other, &self.out, &self.content.disabled, self.clock.now(), client_h);
                    match action {
                        KeyAction::None => {}
                        KeyAction::Activate(b) => self.activate(b),
                        KeyAction::Close => self.finish(XDialogResult::WindowClosed),
                    }
                }
            }
            if self.closed {
                return;
            }
        }
        self.schedule.asap(Instant::now());
    }

    /// Activate button `i` (pointer or keyboard): message -> `ButtonPressed(i)` and close;
    /// progress with a callback -> run it (on this thread, panics caught) and close unless it
    /// returns `true`; progress without a callback -> `ButtonPressed(i)` and close.
    pub(crate) fn activate(&mut self, i: usize) {
        if self.closed || i >= self.content.buttons.len() || self.content.disabled[i] {
            return;
        }
        let keep = match self.sink.callback.as_mut() {
            Some(cb) if self.content.kind == DialogKind::Progress => {
                let proxy = ProgressDialogProxy::non_owning(self.id);
                match catch_unwind(AssertUnwindSafe(|| (cb.0)(i, &proxy))) {
                    Ok(keep) => keep,
                    Err(_) => {
                        error!("xdialog: a progress button callback panicked; closing the dialog");
                        false
                    }
                }
            }
            _ => false,
        };
        if keep {
            self.schedule.asap(Instant::now());
        } else {
            self.finish(XDialogResult::ButtonPressed(i));
        }
    }

    /// Deliver `result` (if none was delivered yet) and mark the dialog closed.
    pub(crate) fn finish(&mut self, result: XDialogResult) {
        if self.result.is_none() {
            if let Some(tx) = self.sink.sender.take() {
                let _ = tx.send(result.clone());
            }
            self.result = Some(result);
        }
        self.closed = true;
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed
    }

    /// The delivered result, once closed.
    pub(crate) fn result(&self) -> Option<&XDialogResult> {
        self.result.as_ref()
    }

    // ---------------------------------------------------------------------------------------------
    // Requests
    // ---------------------------------------------------------------------------------------------

    pub(crate) fn set_progress_value(&mut self, value: f32) {
        let value = if value.is_finite() { value.clamp(0.0, 1.0) } else { 0.0 };
        if self.content.progress.is_none() {
            return;
        }
        let prev = self.last_value;
        self.last_value = value;
        self.content.progress = Some(ProgressView::Determinate { value, prev, changed_at: self.clock.now() });
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
    pub(crate) fn set_text(&mut self, theme: &T, text: &str) {
        if self.content.body == text {
            return;
        }
        self.content.body = text.to_owned();
        self.update_fonts(theme, false);
        self.schedule.asap(Instant::now());
    }

    /// Test hooks: force a button's disabled state.
    pub(crate) fn set_disabled(&mut self, index: usize, disabled: bool) {
        if let Some(d) = self.content.disabled.get_mut(index) {
            *d = disabled;
            self.schedule.asap(Instant::now());
        }
    }

    /// A background font discovery step finished: retry an incomplete coverage check.
    pub(crate) fn refresh_fonts(&mut self, theme: &T) {
        if !self.fonts.complete || Some(FontRegistry::global().fallbacks().len()) != self.fonts.fallbacks {
            self.update_fonts(theme, false);
            self.schedule.asap(Instant::now());
        }
    }

    /// Re-resolve the platform appearance (portal change); no-op for a fixed appearance.
    pub(crate) fn refresh_appearance(&mut self, theme: &T) {
        if let AppearanceSource::System(xt) = &self.appearance_source {
            let a = resolve_appearance(xt);
            self.set_appearance(theme, a);
        }
    }

    /// Switch appearance: new tokens and styles, tweens snap (no fade between palettes).
    pub(crate) fn set_appearance(&mut self, theme: &T, a: Appearance) {
        if a == self.env.appearance {
            return;
        }
        let was_dark = self.style.dark_titlebar;
        self.env.appearance = a;
        self.tokens = theme.tokens(&self.env);
        self.style = theme.window_style(&self.tokens);
        self.apply_style(theme);
        anim::reset_animations(&self.ctx);
        self.titlebar_changed |= was_dark != self.style.dark_titlebar;
        self.schedule.asap(Instant::now());
    }

    // ---------------------------------------------------------------------------------------------
    // Accessors
    // ---------------------------------------------------------------------------------------------

    pub(crate) fn title(&self) -> &str {
        &self.content.title
    }

    /// Logical client size the content wants (measure pass / last pass).
    pub(crate) fn desired_size(&self) -> Vec2 {
        self.out.desired_size
    }

    /// `round(desired * ppp)` physical px (at least 1x1): what winit makes of a logical inner size
    /// (`LogicalSize::to_physical` rounds), so the window, the offscreen harness and the skia
    /// references (e.g. 350 x 151.2 -> 350 x 151) agree and `attach` doesn't re-request a size
    /// the window system can never give.
    pub(crate) fn physical_size(&self, ppp: f32) -> [u32; 2] {
        let s = self.out.desired_size * sanitize_ppp(ppp);
        [(s.x.round() as u32).max(1), (s.y.round() as u32).max(1)]
    }

    /// A pending window resize request (logical px), taken.
    pub(crate) fn take_resize(&mut self) -> Option<Vec2> {
        self.resize.take()
    }

    /// The dark-title-bar flag changed since the last call (appearance switch).
    pub(crate) fn take_titlebar_change(&mut self) -> Option<bool> {
        std::mem::take(&mut self.titlebar_changed).then_some(self.style.dark_titlebar)
    }

    pub(crate) fn window_style(&self) -> WindowStyle {
        self.style
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

    pub(crate) fn frames(&self) -> u64 {
        self.frames
    }

    /// Freeze (`Some(t)`) or unfreeze the dialog clock; repaints.
    pub(crate) fn freeze_clock(&mut self, t: Option<f64>) {
        self.clock.freeze(t);
        self.schedule.asap(Instant::now());
    }

    pub(crate) fn schedule_mut(&mut self) -> &mut Schedule {
        &mut self.schedule
    }

    #[cfg(test)]
    pub(crate) fn ctx(&self) -> &egui::Context {
        &self.ctx
    }

    /// The last pass's theme output (button rects, focus order).
    #[cfg(test)]
    pub(crate) fn ui_output(&self) -> &DialogUiOutput {
        &self.out
    }

    /// The presenter (offscreen harness reads its pixels).
    pub(crate) fn presenter(&self) -> Option<&dyn Presenter> {
        self.presenter.as_deref()
    }

}

impl<T: Theme> Drop for Dialog<T> {
    fn drop(&mut self) {
        // epaint debug-asserts that texture deltas are never dropped unapplied.
        self.textures.clear();
        if self.result.is_none() {
            if let Some(tx) = self.sink.sender.take() {
                let _ = tx.send(XDialogResult::WindowClosed);
            }
        }
    }
}

fn sanitize_ppp(ppp: f32) -> f32 {
    if ppp.is_finite() && ppp > 0.0 {
        ppp
    } else {
        1.0
    }
}

/// The theme's font definitions plus every process fallback appended to each text family.
fn build_fonts<T: Theme>(theme: &T, reg: &FontRegistry, fallbacks: &[super::fonts::Fallback]) -> egui::FontDefinitions {
    let mut defs = egui::FontDefinitions::empty();
    theme.install_fonts(&mut defs, reg);
    let families = theme.text_families();
    let bold_families = theme.bold_families();
    if cfg!(debug_assertions) {
        let bound = |f: &egui::FontFamily| defs.families.get(f).is_some_and(|v| v.iter().all(|n| defs.font_data.contains_key(n)) && !v.is_empty());
        for f in [egui::FontFamily::Proportional, egui::FontFamily::Monospace].iter().chain(families.iter()) {
            debug_assert!(bound(f), "theme {} must bind font family {f:?}", theme.id());
        }
    }
    let primary = defs.families.get(&egui::FontFamily::Proportional).and_then(|v| v.first()).and_then(|n| defs.font_data.get(n)).cloned();
    for fb in fallbacks {
        let data = |face: super::fonts::FaceRef| {
            let mut data = face.font_data();
            if let Some(p) = &primary {
                data.tweak.y_offset_factor = shared_baseline_offset(p, face).unwrap_or(0.0);
            }
            Arc::new(data)
        };
        defs.font_data.insert(fb.name.clone(), data(fb.face));
        let bold = fb.bold.map(|face| {
                              defs.font_data.insert(fb.bold_name(), data(face));
                              fb.bold_name()
                          });
        for fam in &families {
            let chain = defs.families.entry(fam.clone()).or_default();
            // Bold chains: the family's bold face first, its regular face for glyphs the bold lacks.
            let bold = bold.as_ref().filter(|_| bold_families.contains(fam));
            for name in bold.into_iter().chain([&fb.name]) {
                if !chain.contains(name) {
                    chain.push(name.clone());
                }
            }
        }
    }
    defs
}

/// `FontTweak::y_offset_factor` that puts a fallback face's baseline on the primary face's.
///
/// epaint centres a fallback glyph on the row by font height (`ascent + (primary_height -
/// face_height) / 2`, epaint 0.36 `text_layout.rs`), so fallback text (CJK, Hebrew, emoji) sits
/// 1-3 px off. cosmic-text and DirectWrite put every face on one shared baseline; this offset
/// moves the glyphs there: `primary_ascent - face_ascent - (primary_height - face_height) / 2`
/// in ems (plus the primary's own offset). Visual only: row heights and layout are unchanged.
fn shared_baseline_offset(primary: &egui::FontData, face: super::fonts::FaceRef) -> Option<f32> {
    use super::fonts::em_vertical_metrics;
    let (pa, ph) = em_vertical_metrics(&primary.font, primary.index)?;
    let (fa, fh) = em_vertical_metrics(face.bytes, face.index)?;
    let s = primary.tweak.scale;
    let off = s * pa + s * primary.tweak.y_offset_factor - fa - 0.5 * (s * ph - fh);
    off.is_finite().then_some(off)
}

#[cfg(test)]
pub(crate) mod tests {
    //! State-machine tests through real egui passes with a stub theme and a memory presenter.

    use super::*;
    use crate::backends::egui_core::render::MemoryPresenter;
    use crate::backends::egui_core::theme::{
        ArrowAxis, ArrowNav, ButtonInfo, ButtonInteraction, FocusVisibility, KeyboardPolicy, SpaceKey,
    };
    use crate::backends::host_types::{Key, MouseButton};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Minimal theme: optional heading line, body block, a row of 80x30 buttons at y = content.
    pub(crate) struct StubTheme {
        pub policy: KeyboardPolicy,
    }

    impl StubTheme {
        pub(crate) fn linux() -> Self {
            StubTheme { policy: KeyboardPolicy { focus_on_open: true,
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
                                                 scroll_keys: false } }
        }
    }

    impl Theme for StubTheme {
        type Tokens = bool;
        fn id(&self) -> &'static str {
            "stub"
        }
        fn keyboard_policy(&self) -> KeyboardPolicy {
            self.policy
        }
        fn tokens(&self, env: &ThemeEnv) -> bool {
            env.appearance.dark
        }
        fn window_style(&self, dark: &bool) -> WindowStyle {
            WindowStyle { clear: if *dark { egui::Color32::BLACK } else { egui::Color32::WHITE }, dark_titlebar: *dark }
        }
        fn install_fonts(&self, defs: &mut egui::FontDefinitions, _reg: &FontRegistry) {
            use super::super::fonts::bundled;
            defs.font_data.insert("r".into(), Arc::new(bundled::ubuntu_regular().font_data()));
            defs.families.insert(egui::FontFamily::Proportional, vec!["r".into()]);
            defs.families.insert(egui::FontFamily::Monospace, vec!["r".into()]);
        }
        fn text_families(&self) -> Vec<egui::FontFamily> {
            vec![egui::FontFamily::Proportional, egui::FontFamily::Monospace]
        }
        fn primary_faces(&self, _reg: &FontRegistry) -> Vec<super::super::fonts::FaceRef> {
            vec![super::super::fonts::bundled::ubuntu_regular()]
        }
        fn configure_style(&self, _tk: &bool, style: &mut egui::Style) {
            style.spacing.item_spacing = Vec2::ZERO;
        }
        fn ui(&self, tk: &bool, view: &DialogView<'_>, ui: &mut egui::Ui) -> DialogUiOutput {
            use super::super::text::{TextBlockWidget, TextCtx, TextStyle};
            let ctx = ui.ctx().clone();
            let text = TextCtx::new(&ctx);
            let style = TextStyle::new(14.0, egui::FontFamily::Proportional, 16.8);
            let body = text.layout(view.body, &style, 200.0, None);
            let color = if *tk { egui::Color32::WHITE } else { egui::Color32::BLACK };
            ui.put(Rect::from_min_size(Pos2::new(10.0, 10.0), body.size), TextBlockWidget::new(&body, color));
            let top = 20.0 + body.size.y;
            let n = view.buttons.len();
            let mut out = DialogUiOutput { arrow_order: (0..n).collect(), arrow_axis: ArrowAxis::Horizontal, default_button: n.checked_sub(1), ..Default::default() };
            for i in 0..n {
                let rect = Rect::from_min_size(Pos2::new(10.0 + 90.0 * i as f32, top), Vec2::new(80.0, 30.0));
                let st = ButtonInteraction::interact(ui, rect, i, view);
                let fill = anim::animate_color(ui.ctx(), st.response.id, if st.hovered { egui::Color32::RED } else { egui::Color32::GRAY }, anim::Transition::linear(0.15));
                ui.painter().rect_filled(rect, 0.0, fill);
                if st.activated {
                    out.activated = Some(i);
                }
                out.buttons.push(ButtonInfo { index: i, rect });
            }
            out.desired_size = Vec2::new(220.0 + 90.0 * n.saturating_sub(2) as f32, top + 40.0);
            out
        }
    }

    pub(crate) struct Rig {
        pub theme: StubTheme,
        pub d: Dialog<StubTheme>,
        pub rx: Option<oneshot::Receiver<XDialogResult>>,
    }

    impl Rig {
        pub(crate) fn new(kind: DialogKind, buttons: &[&str], callback: Option<ProgressButtonCallback>) -> Rig {
            let theme = StubTheme::linux();
            let options = XDialogOptions { title: "t".into(),
                                           main_instruction: String::new(),
                                           message: "Hello world, this is a body text.".into(),
                                           icon: XDialogIcon::None,
                                           buttons: buttons.iter().map(|s| s.to_string()).collect() };
            let (tx, rx) = oneshot::channel();
            let params = DialogParams { id: 7,
                                        content: DialogContent::new(kind, options),
                                        appearance: Appearance::default(),
                                        appearance_source: AppearanceSource::Fixed,
                                        ppp: 1.0,
                                        limits: SizeLimits { max_height: 800.0, max_width: 1000.0 },
                                        clock: DialogClock::frozen(0.0),
                                        sink: ResultSink { sender: Some(tx), callback } };
            let mut d = Dialog::new(&theme, params);
            let size = d.physical_size(1.0);
            d.attach(Box::new(MemoryPresenter::new()), 1.0, size);
            Rig { theme, d, rx: Some(rx) }
        }

        pub(crate) fn at(&mut self, t: f64) {
            self.d.freeze_clock(Some(t));
            self.d.frame(&self.theme);
        }

        pub(crate) fn ev(&mut self, ev: HostEvent) {
            self.d.handle_event(&self.theme, ev);
        }

        pub(crate) fn center(&self, i: usize) -> (f64, f64) {
            let r = self.d.button_rects()[i];
            ((r[0] + r[2] / 2.0) as f64, (r[1] + r[3] / 2.0) as f64)
        }

        pub(crate) fn result(&mut self) -> Option<XDialogResult> {
            self.rx.as_ref().and_then(|rx| rx.try_recv().ok())
        }

        pub(crate) fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
            let (w, _, px) = self.d.presenter().unwrap().read_rgba().unwrap();
            let i = ((y * w + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        }
    }

    fn key(k: Key, pressed: bool) -> HostEvent {
        HostEvent::Key { key: k, pressed, repeat: false }
    }

    #[test]
    fn measure_pass_sizes_and_first_frame_presents() {
        let mut r = Rig::new(DialogKind::Message, &["Cancel", "OK"], None);
        let want = r.d.desired_size();
        assert!(want.x == 220.0 && want.y > 60.0, "{want:?}");
        assert_eq!(r.d.size_px(), r.d.physical_size(1.0));
        r.at(0.0);
        assert!(r.d.presenter().unwrap().read_rgba().is_some());
        assert_eq!(r.d.take_resize(), None);
        // Default (last) button focused on open.
        assert_eq!(focused_button(r.d.ctx(), r.d.ui_output()), Some(1));
    }

    #[test]
    fn click_activates_message_and_hover_animates() {
        let mut r = Rig::new(DialogKind::Message, &["Cancel", "OK"], None);
        r.at(0.0);
        let (x, y) = r.center(0);
        let (px, py) = (x as u32, y as u32);
        assert_eq!(r.pixel(px, py), [160, 160, 160, 255]); // Color32::GRAY
        r.ev(HostEvent::CursorMoved { x, y });
        r.at(1.0); // hover starts (old value shown)
        r.at(1.075); // half-way
        let mid = r.pixel(px, py);
        assert!(mid[0] > 150 && mid[0] < 250 && mid[1] < 128, "{mid:?}");
        r.at(1.2);
        assert_eq!(r.pixel(px, py), [255, 0, 0, 255]);
        r.ev(HostEvent::MouseButton { button: MouseButton::Primary, pressed: true });
        r.at(1.3);
        assert!(!r.d.is_closed());
        r.ev(HostEvent::MouseButton { button: MouseButton::Primary, pressed: false });
        r.at(1.4);
        assert!(r.d.is_closed());
        assert_eq!(r.result(), Some(XDialogResult::ButtonPressed(0)));
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
        r.ev(HostEvent::CloseRequested);
        assert_eq!(r.result(), Some(XDialogResult::WindowClosed));
    }

    #[test]
    fn progress_callback_keep_open_and_panic() {
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        let cb = ProgressButtonCallback(Box::new(|i, _p| {
                                            CALLS.fetch_add(1, Ordering::SeqCst);
                                            assert_eq!(i, 0);
                                            true
                                        }));
        let mut r = Rig::new(DialogKind::Progress, &["Cancel"], Some(cb));
        r.at(0.0);
        r.ev(key(Key::Enter, true));
        assert_eq!(CALLS.load(Ordering::SeqCst), 1);
        assert!(!r.d.is_closed());

        let cb = ProgressButtonCallback(Box::new(|_, _| panic!("boom")));
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
        assert_eq!(r.d.content.progress, Some(ProgressView::Determinate { value: 0.5, prev: 0.0, changed_at: 0.0 }));
        r.at(1.0);
        r.d.set_progress_indeterminate();
        r.at(2.0);
        r.d.set_progress_indeterminate();
        assert_eq!(r.d.content.progress, Some(ProgressView::Indeterminate { since: 1.0, restarted_at: 2.0 }));
        r.d.set_progress_value(2.0);
        assert_eq!(r.d.content.progress, Some(ProgressView::Determinate { value: 1.0, prev: 0.5, changed_at: 2.0 }));
        // A longer body grows the window.
        let h = r.d.desired_size().y;
        r.d.set_text(&r.theme, &"many words ".repeat(40));
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
        drop(r);
        assert_eq!(rx.try_recv().ok(), Some(XDialogResult::WindowClosed));
    }

    #[test]
    fn scale_factor_change_requests_same_logical_size() {
        let mut r = Rig::new(DialogKind::Message, &["OK"], None);
        r.at(0.0);
        r.ev(HostEvent::ScaleFactorChanged { scale_factor: 2.0 });
        let want = r.d.take_resize().unwrap();
        assert_eq!(want, r.d.desired_size());
        assert_eq!(r.d.physical_size(2.0), [(want.x * 2.0).round() as u32, (want.y * 2.0).round() as u32]);
        r.ev(HostEvent::Resized { width: r.d.physical_size(2.0)[0], height: r.d.physical_size(2.0)[1] });
        r.at(0.1);
        assert_eq!(r.d.presenter().unwrap().read_rgba().unwrap().0, r.d.physical_size(2.0)[0]);
        // Zero size (minimised) skips drawing without error.
        r.ev(HostEvent::Resized { width: 0, height: 0 });
        r.at(0.2);
        assert!(!r.d.take_present_failed());
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
        r.d.set_text(&r.theme, "你好，世界");
        r.at(0.0);
        let id = egui::FontId::new(14.0, egui::FontFamily::Proportional);
        assert!(r.d.ctx().fonts_mut(|f| f.has_glyphs(&id, "你好世界确定")));
    }

    #[test]
    fn appearance_switch_resets_tokens() {
        let mut r = Rig::new(DialogKind::Message, &["OK"], None);
        r.at(0.0);
        assert_eq!(r.pixel(1, 1), [255, 255, 255, 255]);
        r.d.set_appearance(&r.theme, Appearance { dark: true, accent: None });
        assert_eq!(r.d.take_titlebar_change(), Some(true));
        r.at(0.1);
        assert_eq!(r.pixel(1, 1), [0, 0, 0, 255]);
    }
}
