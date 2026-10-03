//! One dialog: content, theme, input state, keyboard policy, clock, drawing target and result
//! delivery.
//!
//! Life cycle: [`Dialog::new`] runs the measure pass ([`Theme::ui`]), applies the keyboard
//! on-open step and resets tweens. The owner (the runtime with a winit window, or the offscreen
//! harness) then sizes its surface to [`Dialog::desired_size`], [`Dialog::attach`]es a drawing
//! [`Target`] and calls [`Dialog::frame`] for every repaint. Input goes through
//! [`Dialog::handle_events`] (logical px): pointer presses are hit-tested against the last pass's
//! widget rects, keys run the keyboard policy; both can activate a button right away. A dialog is
//! closed once it has delivered its result ([`Dialog::is_closed`]); the owner then hides and drops
//! its window.
//!
//! The result goes to the caller only once the dialog is shown ([`Dialog::set_sender`]); a dialog
//! dropped before it delivered a result sends `WindowClosed`.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use accesskit::TreeUpdate;

use super::a11y::{self, Request};
use super::appearance::{resolve_appearance, Appearance};
use super::clock::{DialogClock, Schedule};
use super::input::{Event, PointerButton};
use super::keyboard::{has_button, KeyAction, KeyboardState};
use super::theme::{button_id, DialogKind, DialogUiOutput, DialogView, ProgressView, Theme};
use super::ui::{Id, Ui, UiState};
#[cfg(any(test, feature = "_test-hooks"))]
use crate::backends::draw::MemorySurface;
use crate::backends::draw::{Color, DrawError, Frame, Image, Point, Rect, Shape, Size, Surface, Text, WindowSurface};
use crate::icon::IconFile;
use crate::model::{ResultSender, XDialogIcon, XDialogOptions, XDialogResult, XDialogTheme};
use crate::{ProgressButtonCallback, ProgressDialogProxy};

/// What the dialog shows (the API strings, with `\n` line breaks).
#[derive(Clone, Debug)]
pub(crate) struct DialogContent {
    pub kind: DialogKind,
    pub title: String,
    pub heading: String,
    pub body: String,
    pub icon: XDialogIcon,
    /// `options.icon_source`, loaded (`None`: none, or it couldn't be loaded).
    pub icon_file: Option<Arc<IconFile>>,
    pub buttons: Vec<String>,
    pub progress: Option<ProgressView>,
}

impl DialogContent {
    /// Content of a message (`Message`) or progress (`Progress`, starting determinate at 0) dialog.
    /// Reads the icon, if any.
    pub(crate) fn new(kind: DialogKind, options: XDialogOptions) -> Self {
        let icon_file = options.icon_source.as_ref().and_then(|source| match IconFile::load(source) {
                                                        Ok(f) => Some(Arc::new(f)),
                                                        Err(e) => {
                                                            warn!("xdialog: ignoring the icon: {e}");
                                                            None
                                                        }
                                                    });
        DialogContent { kind,
                        title: lf_newlines(options.title),
                        heading: lf_newlines(options.main_instruction),
                        body: lf_newlines(options.message),
                        icon: options.icon,
                        icon_file,
                        buttons: options.buttons.into_iter().map(lf_newlines).collect(),
                        progress: (kind == DialogKind::Progress).then_some(ProgressView::Determinate { value: 0.0 }) }
    }

    /// Every string the theme may draw (font coverage check).
    fn texts(&self) -> Vec<&str> {
        let mut v = vec![self.title.as_str(), self.heading.as_str(), self.body.as_str()];
        v.extend(self.buttons.iter().map(String::as_str));
        v
    }
}

/// `text` with `\r\n` and `\r` line breaks as `\n` (what the themes, the elision and the
/// accessibility tree see).
fn lf_newlines(text: String) -> String {
    if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text
    }
}

pub(crate) struct DialogParams {
    /// Request id (the `ProgressDialogProxy` id handed to callbacks).
    pub id: usize,
    pub content: DialogContent,
    pub appearance: Appearance,
    /// `Some`: the appearance is re-resolved from the platform with this override on
    /// `ThemeChanged` / portal changes. `None`: injected (offscreen harness), never re-resolved.
    pub system_appearance: Option<XDialogTheme>,
    /// Physical px per logical px the window will most likely have.
    pub ppp: f64,
    /// Max client height, logical px ([`DialogView::max_height`]).
    pub max_height: f64,
    pub clock: DialogClock,
    /// Where the result goes (`None`: set later with [`Dialog::set_sender`], or the offscreen
    /// harness).
    pub sender: Option<ResultSender>,
    /// How long opening waits for fallback fonts when the theme's family lacks characters.
    pub font_wait: Duration,
    /// The text system the dialog lays out with (the runtime's, or this thread's shared one).
    pub text: Rc<Text>,
}

/// Where a dialog's frames go.
pub(crate) enum Target {
    Window(WindowSurface),
    #[cfg(any(test, feature = "_test-hooks"))]
    Memory(MemorySurface),
}

impl Target {
    fn present(&mut self, frame: &Frame<'_>) -> Result<(), DrawError> {
        match self {
            Target::Window(s) => s.present(frame),
            #[cfg(any(test, feature = "_test-hooks"))]
            Target::Memory(s) => s.present(frame),
        }
    }
}

/// One dialog (no window: see the module docs).
pub(crate) struct Dialog {
    id: usize,
    theme: Box<dyn Theme>,
    appearance: Appearance,
    system_appearance: Option<XDialogTheme>,
    content: DialogContent,
    max_height: f64,
    clock: DialogClock,
    keyboard: KeyboardState,
    st: UiState,
    /// The last pass's theme output, drawing and interactive rects.
    out: DialogUiOutput,
    shapes: Vec<Shape>,
    hits: Vec<(Id, Rect)>,
    /// The drawing last presented to the target, and its size, scale and clear colour (`None`:
    /// nothing presented yet, or presenting failed).
    presented: Vec<Shape>,
    presented_with: Option<([u32; 2], f64, Color)>,
    target: Option<Target>,
    ppp: f64,
    /// Client size in physical px.
    size_px: [u32; 2],
    /// The logical size last requested from (or created by) the window system.
    requested: Size,
    resize: Option<Size>,
    titlebar_changed: bool,
    sender: Option<ResultSender>,
    /// Progress dialogs with `show_progress_with_callback`.
    callback: Option<ProgressButtonCallback>,
    result: Option<XDialogResult>,
    schedule: Schedule,
    /// Frames presented (host test hooks).
    #[cfg(any(test, all(feature = "winit-host", feature = "_test-hooks")))]
    frames: u64,
    /// Every visible character has a face (or the system scan has been merged).
    fonts_complete: bool,
    /// The custom icon's side in physical px and its image (`None`: no frame decoded), rendered
    /// again when the scale changes.
    icon: Option<(u32, Option<Image>)>,
    /// The window has a behind-window material: frames clear with the theme's translucent colour.
    translucent: bool,
}

impl Dialog {
    /// Check fonts, run the measure pass ([`Theme::ui`]), focus the default button.
    pub(crate) fn new(mut theme: Box<dyn Theme>, p: DialogParams) -> Self {
        theme.set_appearance(&p.appearance);
        let mut d = Dialog { id: p.id,
                             keyboard: KeyboardState::new(theme.keyboard_policy()),
                             theme,
                             appearance: p.appearance,
                             system_appearance: p.system_appearance,
                             content: p.content,
                             max_height: p.max_height,
                             clock: p.clock,
                             st: UiState::new(p.text),
                             out: DialogUiOutput::default(),
                             shapes: Vec::new(),
                             hits: Vec::new(),
                             presented: Vec::new(),
                             presented_with: None,
                             target: None,
                             ppp: p.ppp,
                             size_px: [0, 0],
                             requested: Size::ZERO,
                             resize: None,
                             titlebar_changed: false,
                             sender: p.sender,
                             callback: None,
                             result: None,
                             schedule: Schedule::default(),
                             #[cfg(any(test, all(feature = "winit-host", feature = "_test-hooks")))]
                             frames: 0,
                             fonts_complete: false,
                             icon: None,
                             translucent: false };
        d.update_fonts(p.font_wait);
        d.run_pass(true);
        d.requested = d.out.desired_size;
        d.keyboard.on_open(&mut d.st.focus, &d.out);
        // Tweens snap on the first real frame (the measure pass saw pre-focus targets).
        d.st.tweens.reset();
        d
    }

    /// Make fallback faces available for every visible character (waiting up to `wait`).
    fn update_fonts(&mut self, wait: Duration) {
        self.st.texts.begin_pass(self.theme.fonts());
        self.fonts_complete = self.st.texts.prepare_fonts(&self.content.texts(), wait);
    }

    /// The icon's image at the current scale (rendered on first use and when the scale changes):
    /// `Custom`'s icon file, or the theme's system image of a severity icon.
    fn update_icon(&mut self) {
        let file = match self.content.icon {
            XDialogIcon::None => return,
            XDialogIcon::Custom => self.content.icon_file.clone(),
            ref severity => self.theme.system_icon(severity),
        };
        let Some(file) = file else { return };
        let px = (self.theme.icon_size() * self.ppp).round().max(1.0) as u32;
        if self.icon.as_ref().is_none_or(|(size, _)| *size != px) {
            let image = file.render(px).map(|img| Image::from_straight_rgba([img.size, img.size], &img.rgba));
            self.icon = Some((px, image));
        }
    }

    /// One theme pass; stores the output (its buttons from the pass's `interact` calls), drawing
    /// and interactive rects.
    fn run_pass(&mut self, sizing: bool) -> super::clock::Wants {
        let frame = if sizing { Default::default() } else { self.keyboard.frame_info(self.st.focus, &self.out) };
        self.update_icon();
        self.st.texts.begin_pass(self.theme.fonts());
        let c = &self.content;
        let view = DialogView { title: &c.title,
                                heading: &c.heading,
                                body: &c.body,
                                icon: &c.icon,
                                custom_icon: self.icon.as_ref().and_then(|(_, img)| img.as_ref()),
                                buttons: &c.buttons,
                                progress: c.progress,
                                max_height: self.max_height,
                                frame };
        let mut ui = Ui::with_buffers(&mut self.st, self.clock.now(), std::mem::take(&mut self.shapes), std::mem::take(&mut self.hits));
        self.out = self.theme.ui(&view, &mut ui);
        let pass = ui.finish();
        self.out.buttons = pass.buttons;
        self.shapes = pass.shapes;
        self.hits = pass.hits;
        self.st.end_pass();
        pass.wants
    }

    // ---------------------------------------------------------------------------------------------
    // Window attachment and frames
    // ---------------------------------------------------------------------------------------------

    /// Attach the drawing target and the window's metrics (physical client size).
    pub(crate) fn attach(&mut self, target: Target, ppp: f64, size_px: [u32; 2]) {
        self.target = Some(target);
        self.presented_with = None;
        self.ppp = ppp;
        self.size_px = size_px;
        if size_px != self.physical_size(ppp) {
            // The window got a different size (other DPI than expected, WM constraints): ask again.
            self.resize = Some(self.out.desired_size);
        }
    }

    /// The progress button callback (handed over only once nothing can fail any more).
    pub(crate) fn set_callback(&mut self, callback: Option<ProgressButtonCallback>) {
        self.callback = callback;
    }

    /// Run one frame: one theme pass, resize check, draw + present, schedule the next frame. A
    /// frame the schedule asked for is not presented when it would draw what is already on
    /// screen; one the window system asked for always is (its content may be gone).
    /// `Err`: presenting failed (the frame is otherwise complete).
    pub(crate) fn frame(&mut self) -> Result<(), DrawError> {
        self.frame_with(|| {})
    }

    /// [`Dialog::frame`], calling `before_present` right before presenting (not when the frame is
    /// skipped).
    pub(crate) fn frame_with(&mut self, before_present: impl FnOnce()) -> Result<(), DrawError> {
        if self.is_closed() {
            return Ok(());
        }
        let wants = self.run_pass(false);

        // Content changed size (set_text reflow, fonts): ask the window system.
        let desired = self.out.desired_size;
        if ((desired.width - self.requested.width).abs().max((desired.height - self.requested.height).abs())) * self.ppp > 0.5 {
            self.requested = desired;
            self.resize = Some(desired);
        }

        let mut presented = Ok(());
        let clear = self.clear_color();
        let with = (self.size_px, self.ppp, clear);
        let unchanged = self.schedule.self_scheduled() && self.presented_with == Some(with) && self.presented == self.shapes;
        if let Some(target) = self.target.as_mut().filter(|_| !unchanged) {
            before_present();
            presented = target.present(&Frame { shapes: &self.shapes, size_px: self.size_px, ppp: self.ppp, clear });
            self.presented_with = presented.is_ok().then_some(with);
            // The next pass records into the old buffer.
            std::mem::swap(&mut self.presented, &mut self.shapes);
            #[cfg(any(test, all(feature = "winit-host", feature = "_test-hooks")))]
            {
                self.frames += 1;
            }
        }
        self.schedule.after_frame(Instant::now(), wants);
        presented
    }

    // ---------------------------------------------------------------------------------------------
    // Input
    // ---------------------------------------------------------------------------------------------

    /// Handle input events (logical px): see the module docs. Only the primary button presses
    /// widgets.
    pub(crate) fn handle_events(&mut self, events: impl IntoIterator<Item = Event>) {
        for ev in events {
            if self.is_closed() {
                return;
            }
            match ev {
                Event::Key { .. } => self.keyboard_event(&ev),
                Event::PointerMoved(p) => {
                    self.st.pointer = Some(p);
                    self.st.moved = true;
                }
                Event::PointerGone => {
                    self.st.pointer = None;
                    self.st.gone = true;
                }
                Event::PointerButton { pos, button: PointerButton::Primary, pressed: true } => {
                    self.st.pointer = Some(pos);
                    // Drags count from the press, not from where the last pass saw the pointer.
                    self.st.last_pointer = Some(pos);
                    let hit = self.hits.iter().rev().find(|(_, r)| r.contains(pos)).map(|(id, _)| *id);
                    self.st.pressed = hit;
                    // Buttons take focus on press.
                    if let Some(i) = hit.and_then(|id| self.button_of(id)) {
                        self.st.focus = Some(i);
                    }
                    self.keyboard_event(&ev);
                }
                Event::PointerButton { pos, button: PointerButton::Primary, pressed: false } => {
                    // Released inside the button the press started on: a click.
                    let Some(id) = self.st.pressed.take() else { continue };
                    let inside = self.hits.iter().any(|(i, r)| *i == id && r.contains(pos));
                    if let Some(b) = self.button_of(id).filter(|_| inside) {
                        self.activate(b);
                    }
                }
                Event::PointerButton { .. } => {}
                Event::Scroll(d) => self.keyboard.wheel(-d.y),
                // Only changes count: X11 reports a new window's focus state some time after it
                // is shown, and a press made meanwhile must not be cancelled by it.
                Event::WindowFocused(focused) if focused == self.st.window_focused => {}
                Event::WindowFocused(focused) => {
                    self.st.window_focused = focused;
                    if focused {
                        // Nothing reports an accent-colour change on Windows: re-read the system
                        // appearance whenever the dialog gains focus.
                        self.refresh_appearance();
                    } else {
                        // The release may never arrive (capture lost): drop the held press.
                        self.cancel_press();
                    }
                    self.keyboard_event(&ev);
                }
            }
        }
        self.invalidate();
    }

    /// Whether `pos` (logical px) is on an interactive widget of the last pass.
    pub(crate) fn hits_widget(&self, pos: Point) -> bool {
        self.hits.iter().any(|(_, r)| r.contains(pos))
    }

    /// The API index of the button with widget id `id`.
    fn button_of(&self, id: Id) -> Option<usize> {
        self.out.buttons.iter().map(|b| b.index).find(|&i| button_id(i) == id)
    }

    /// Drop a held primary press without activating anything; the pointer counts as gone until it
    /// moves again.
    pub(crate) fn cancel_press(&mut self) {
        if self.st.pressed.take().is_some() {
            self.st.pointer = None;
            self.st.gone = true;
        }
    }

    /// New client size in physical px (a zero side = minimised).
    pub(crate) fn resized(&mut self, size_px: [u32; 2]) {
        if size_px != self.size_px {
            self.size_px = size_px;
            self.invalidate();
        }
    }

    /// New scale factor; the window system keeps the logical size and reports the new physical
    /// size with the next [`Dialog::resized`].
    pub(crate) fn scale_changed(&mut self, ppp: f64) {
        self.ppp = ppp;
        self.invalidate();
    }

    /// State changed: repaint.
    fn invalidate(&mut self) {
        self.schedule.asap(Instant::now());
    }

    fn keyboard_event(&mut self, ev: &Event) {
        let client_h = self.client_logical().height;
        match self.keyboard.on_event(ev, &mut self.st.focus, &self.out, client_h) {
            KeyAction::None => {}
            KeyAction::Activate(b) => self.activate(b),
            KeyAction::Close => self.finish(XDialogResult::WindowClosed),
        }
    }

    /// An assistive technology's request: Click and Focus on a button take the keyboard's paths
    /// (activation; focus that shows and cancels a held Space).
    pub(crate) fn a11y_request(&mut self, r: Request) {
        match r {
            Request::Click(b) => self.activate(b),
            Request::Focus(b) if has_button(&self.out, b) => self.keyboard.focus(&mut self.st.focus, b),
            Request::Focus(_) => {}
        }
        self.invalidate();
    }

    /// Activate button `i` (pointer, keyboard or screen reader): message -> `ButtonPressed(i)` and
    /// close; progress with a callback -> run it (on this thread, panics caught) and close unless
    /// it returns `true`; progress without a callback -> `ButtonPressed(i)` and close.
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
            self.invalidate();
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
        self.invalidate();
    }

    pub(crate) fn set_progress_indeterminate(&mut self) {
        let now = self.clock.now();
        self.content.progress = match self.content.progress {
            None => return,
            Some(ProgressView::Indeterminate { since, .. }) => Some(ProgressView::Indeterminate { since, restarted_at: now }),
            Some(ProgressView::Determinate { .. }) => Some(ProgressView::Indeterminate { since: now, restarted_at: now }),
        };
        self.invalidate();
    }

    /// New body text (`set_text`): the next pass relayouts and may resize. Never waits for the
    /// system font scan (this runs on the shared UI thread, possibly many times a second): an
    /// incomplete check is retried by [`Dialog::refresh_fonts`] when the scan ends.
    pub(crate) fn set_text(&mut self, text: &str) {
        let text = lf_newlines(text.to_owned());
        if self.content.body == text {
            return;
        }
        self.content.body = text;
        self.update_fonts(Duration::ZERO);
        self.invalidate();
    }

    /// The system font scan finished: lay out again with the new faces.
    pub(crate) fn refresh_fonts(&mut self) {
        if !self.fonts_complete {
            self.update_fonts(Duration::ZERO);
        }
        self.invalidate();
    }

    /// Re-resolve the platform appearance; no-op for a fixed (injected) appearance.
    pub(crate) fn refresh_appearance(&mut self) {
        if let Some(xt) = self.system_appearance {
            let a = resolve_appearance(xt);
            self.set_appearance(a);
        }
    }

    /// Switch appearance: new tokens, tweens snap (no fade between palettes).
    pub(crate) fn set_appearance(&mut self, a: Appearance) {
        if a == self.appearance {
            return;
        }
        self.titlebar_changed |= a.dark != self.appearance.dark;
        self.appearance = a;
        self.theme.set_appearance(&a);
        self.st.tweens.reset();
        self.invalidate();
    }

    // ---------------------------------------------------------------------------------------------
    // Accessors
    // ---------------------------------------------------------------------------------------------

    /// The colour frames are cleared with (see [`Theme::translucent_clear`]).
    fn clear_color(&self) -> Color {
        self.translucent.then(|| self.theme.translucent_clear()).flatten().unwrap_or_else(|| self.theme.clear_color())
    }

    /// Whether the theme has a translucent look (its window should get a behind-window material).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn wants_translucency(&self) -> bool {
        self.theme.translucent_clear().is_some()
    }

    /// The behind-window material the theme asks for (with [`Dialog::wants_translucency`]).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn window_material(&self) -> super::theme::WindowMaterial {
        self.theme.window_material()
    }

    /// The window got a behind-window material (call before the first frame).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn set_translucent(&mut self, translucent: bool) {
        self.translucent = translucent;
    }

    pub(crate) fn title(&self) -> &str {
        &self.content.title
    }

    /// The loaded `options.icon_source` (the window icon).
    pub(crate) fn icon_file(&self) -> Option<&IconFile> {
        self.content.icon_file.as_deref()
    }

    /// Logical client size the content wants (measure pass / last pass).
    pub(crate) fn desired_size(&self) -> Size {
        self.out.desired_size
    }

    /// Client size in logical px (the requested size until the window reports one).
    fn client_logical(&self) -> Size {
        if self.size_px[0] == 0 || self.size_px[1] == 0 {
            self.requested
        } else {
            Size::new(self.size_px[0] as f64 / self.ppp, self.size_px[1] as f64 / self.ppp)
        }
    }

    /// `round(desired * ppp)` physical px (at least 1x1): what winit makes of a logical inner size
    /// (`LogicalSize::to_physical` rounds), so the window and the offscreen harness agree and
    /// `attach` doesn't re-request a size the window system can never give.
    pub(crate) fn physical_size(&self, ppp: f64) -> [u32; 2] {
        let s = self.out.desired_size * ppp;
        [(s.width.round() as u32).max(1), (s.height.round() as u32).max(1)]
    }

    /// A pending window resize request (logical px), taken.
    pub(crate) fn take_resize(&mut self) -> Option<Size> {
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

    /// The accessibility tree of the last pass.
    pub(crate) fn a11y_tree(&self) -> TreeUpdate {
        a11y::tree(&a11y::TreeSource { content: &self.content, out: &self.out, focus: self.st.focus, ppp: self.ppp })
    }
}

/// Introspection for the offscreen harness, the host test hooks and unit tests.
#[cfg(any(test, feature = "_test-hooks"))]
impl Dialog {
    /// The delivered result, once closed.
    #[cfg(feature = "_test-hooks")]
    pub(crate) fn result(&self) -> Option<&XDialogResult> {
        self.result.as_ref()
    }

    #[cfg(feature = "_test-hooks")]
    pub(crate) fn ppp(&self) -> f64 {
        self.ppp
    }

    pub(crate) fn size_px(&self) -> [u32; 2] {
        self.size_px
    }

    /// Button rects in LOGICAL px `[x, y, w, h]`, indexed by API button index (zeros for a
    /// button the theme didn't lay out).
    pub(crate) fn button_rects(&self) -> Vec<[f64; 4]> {
        let mut v = vec![[0.0; 4]; self.content.buttons.len()];
        for b in &self.out.buttons {
            if let Some(slot) = v.get_mut(b.index) {
                *slot = [b.rect.x0, b.rect.y0, b.rect.width(), b.rect.height()];
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
        self.invalidate();
    }

    /// The last frame of a memory target as opaque RGBA8 `(width, height, pixels)`.
    pub(crate) fn read_rgba(&self) -> Option<(u32, u32, &[u8])> {
        use crate::backends::draw::MemoryTarget;
        match self.target.as_ref()? {
            Target::Memory(m) => m.read_rgba(),
            Target::Window(_) => None,
        }
    }
}

impl Drop for Dialog {
    fn drop(&mut self) {
        if let Some(tx) = self.sender.take() {
            tx.send(XDialogResult::WindowClosed);
        }
    }
}

#[cfg(test)]
mod tests {
    //! State-machine tests through real passes with a stub theme and a memory target.

    use super::*;
    use crate::backends::draw::{Color, MemoryTarget, Point, TextSystem};
    use crate::backends::gui::anim::Transition;
    use crate::backends::gui::input::Key;
    use crate::backends::gui::text::{TextStyle, ThemeFonts};
    use crate::backends::gui::theme::{ButtonInteraction, KeyboardPolicy};
    use crate::model::DialogReply;
    use crate::oneshot;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Minimal theme: body block, a row of 80x30 buttons below it.
    struct StubTheme {
        dark: bool,
    }

    impl Theme for StubTheme {
        fn set_appearance(&mut self, appearance: &Appearance) {
            self.dark = appearance.dark;
        }

        fn keyboard_policy(&self) -> KeyboardPolicy {
            crate::backends::ubuntu::KEYBOARD
        }

        fn fonts(&self) -> &'static ThemeFonts {
            &crate::backends::ubuntu::FONTS
        }

        fn icon_size(&self) -> f64 {
            48.0
        }

        fn clear_color(&self) -> Color {
            if self.dark {
                Color::BLACK
            } else {
                Color::WHITE
            }
        }

        fn ui(&mut self, view: &DialogView<'_>, ui: &mut Ui<'_>) -> DialogUiOutput {
            let body = ui.layout(view.body, &TextStyle::regular(14.0), 200.0, None);
            let color = if self.dark { Color::WHITE } else { Color::BLACK };
            ui.text(&body, Point::new(10.0, 10.0), color);
            let top = 20.0 + body.size.height;
            let mut out = DialogUiOutput::default();
            for i in 0..view.buttons.len() {
                let rect = Rect::from_origin_size(Point::new(10.0 + 90.0 * i as f64, top), Size::new(80.0, 30.0));
                let st = ButtonInteraction::interact(ui, rect, i, view);
                let fill =
                    ui.animate(button_id(i), if st.hovered { Color::from_rgb(255, 0, 0) } else { Color::GRAY }, Transition::linear(0.15));
                ui.fill_rect(rect, 0.0, fill);
            }
            out.desired_size = Size::new(220.0 + 90.0 * view.buttons.len().saturating_sub(2) as f64, top + 40.0);
            out
        }
    }

    type Answer = Result<XDialogResult, crate::XDialogError>;

    struct Rig {
        d: Dialog,
        rx: Option<oneshot::Receiver<Answer>>,
    }

    impl Rig {
        fn new(kind: DialogKind, buttons: &[&str], callback: Option<ProgressButtonCallback>) -> Rig {
            let options = XDialogOptions { title: "t".into(),
                                           main_instruction: String::new(),
                                           message: "Hello world, this is a body text.".into(),
                                           icon: XDialogIcon::None,
                                           buttons: buttons.iter().map(|s| s.to_string()).collect(),
                                           ..Default::default() };
            let (tx, rx) = oneshot::channel::<Answer>();
            let text = Text::shared().expect("text system");
            let params = DialogParams { id: 7,
                                        content: DialogContent::new(kind, options),
                                        appearance: Appearance::default(),
                                        system_appearance: None,
                                        ppp: 1.0,
                                        max_height: 800.0,
                                        clock: DialogClock::frozen(0.0),
                                        sender: Some(DialogReply::Message(tx).opened()),
                                        font_wait: Duration::ZERO,
                                        text: text.clone() };
            let mut d = Dialog::new(Box::new(StubTheme { dark: false }), params);
            let size = d.physical_size(1.0);
            d.attach(Target::Memory(MemorySurface::new(&text).expect("memory surface")), 1.0, size);
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

        fn center(&self, i: usize) -> Point {
            let r = self.d.button_rects()[i];
            Point::new(r[0] + r[2] / 2.0, r[1] + r[3] / 2.0)
        }

        fn result(&mut self) -> Option<XDialogResult> {
            self.rx.as_ref().and_then(|rx| rx.try_recv().ok()).and_then(Result::ok)
        }

        fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
            let (w, _, px) = self.d.read_rgba().unwrap();
            let i = ((y * w + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        }
    }

    fn key(key: Key, pressed: bool) -> Event {
        Event::Key { key, pressed, repeat: false, shift: false }
    }

    fn primary(pos: Point, pressed: bool) -> Event {
        Event::PointerButton { pos, button: PointerButton::Primary, pressed }
    }

    #[test]
    fn measure_pass_sizes_and_first_frame_presents() {
        let mut r = Rig::new(DialogKind::Message, &["Cancel", "OK"], None);
        let want = r.d.desired_size();
        assert!(want.width == 220.0 && want.height > 60.0, "{want:?}");
        assert_eq!(r.d.size_px(), r.d.physical_size(1.0));
        r.at(0.0);
        assert!(r.d.read_rgba().is_some());
        assert_eq!(r.d.frames(), 1);
        assert_eq!(r.d.take_resize(), None);
        // Default (last) button focused on open.
        assert_eq!(r.d.st.focus, Some(1));
    }

    /// `ButtonInteraction::interact` alone puts a button into the pass's `buttons` (keyboard
    /// order, accessibility), in call order, with its rect.
    #[test]
    fn interact_records_the_buttons_in_call_order() {
        let mut r = Rig::new(DialogKind::Message, &["A", "B", "C"], None);
        r.at(0.0);
        assert_eq!(r.d.out.buttons.iter().map(|b| b.index).collect::<Vec<_>>(), vec![0, 1, 2]);
        for b in &r.d.out.buttons {
            assert_eq!((b.rect.x0, b.rect.width(), b.rect.height()), (10.0 + 90.0 * b.index as f64, 80.0, 30.0));
        }
        assert_eq!(r.d.a11y_tree().nodes.iter().filter(|(_, n)| n.role() == accesskit::Role::Button).count(), 3);
    }

    #[test]
    fn click_activates_message_and_hover_animates() {
        let mut r = Rig::new(DialogKind::Message, &["Cancel", "OK"], None);
        r.at(0.0);
        let c = r.center(0);
        let (px, py) = (c.x as u32, c.y as u32);
        assert_eq!(r.pixel(px, py), [160, 160, 160, 255]);
        r.ev(Event::PointerMoved(c));
        r.at(1.0); // hover starts (old value shown)
        r.at(1.075); // half-way
        let mid = r.pixel(px, py);
        assert!(mid[0] > 150 && mid[0] < 250 && mid[1] < 128, "{mid:?}");
        r.at(1.2);
        assert_eq!(r.pixel(px, py), [255, 0, 0, 255]);
        // Settled: no further frames are scheduled (static dialogs don't repaint).
        assert_eq!(r.d.schedule.next, None);
        r.ev(primary(c, true));
        r.at(1.3);
        assert!(!r.d.is_closed());
        r.ev(primary(c, false));
        assert!(r.d.is_closed());
        assert_eq!(r.result(), Some(XDialogResult::ButtonPressed(0)));
    }

    /// A move and a press in one batch: drag deltas count from the press, not from the pointer
    /// the last pass saw.
    #[test]
    fn press_anchors_drags() {
        let mut r = Rig::new(DialogKind::Message, &["Cancel", "OK"], None);
        r.ev(Event::PointerMoved(Point::new(5.0, 5.0)));
        r.at(0.0);
        let c = r.center(0);
        r.d.handle_events([Event::PointerMoved(c), primary(c, true)]);
        assert_eq!(r.d.st.last_pointer, Some(c));
    }

    #[test]
    fn release_outside_and_focus_loss_cancel() {
        let mut r = Rig::new(DialogKind::Message, &["Cancel", "OK"], None);
        r.at(0.0);
        let c = r.center(0);
        r.ev(Event::PointerMoved(c));
        r.ev(primary(c, true));
        assert_eq!(r.d.st.focus, Some(0), "a press focuses the button");
        r.ev(primary(Point::new(2.0, 2.0), false));
        assert!(!r.d.is_closed(), "released outside");

        r.ev(primary(c, true));
        r.at(0.1);
        r.ev(Event::WindowFocused(false));
        assert_eq!(r.d.st.pointer, None);
        // The real release that may follow does not click.
        r.ev(primary(c, false));
        r.at(0.2);
        assert!(!r.d.is_closed());
        // Secondary buttons never press widgets.
        r.ev(Event::PointerButton { pos: c, button: PointerButton::Secondary, pressed: true });
        r.ev(Event::PointerButton { pos: c, button: PointerButton::Secondary, pressed: false });
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
    }

    /// Pins the current behaviour: Escape closes a progress dialog without running its callback.
    #[test]
    fn escape_on_progress_with_callback() {
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        let cb: ProgressButtonCallback = Box::new(|_, _| {
            CALLS.fetch_add(1, Ordering::SeqCst);
            false
        });
        let mut r = Rig::new(DialogKind::Progress, &["Cancel"], Some(cb));
        r.at(0.0);
        r.ev(key(Key::Escape, true));
        assert!(r.d.is_closed());
        assert_eq!(CALLS.load(Ordering::SeqCst), 0);
        assert_eq!(r.result(), Some(XDialogResult::WindowClosed));
    }

    #[test]
    fn unchanged_scheduled_frames_are_not_presented() {
        let mut r = Rig::new(DialogKind::Message, &["Cancel", "OK"], None);
        r.at(0.0);
        assert_eq!((r.d.frames(), r.d.schedule.next), (1, None));
        // The same size again schedules nothing.
        r.d.resized(r.d.size_px());
        assert_eq!(r.d.schedule.next, None);
        // A pointer move over the background: a frame is scheduled, and draws what is on screen.
        r.ev(Event::PointerMoved(Point::new(2.0, 2.0)));
        r.d.schedule_mut().fired();
        r.d.frame().unwrap();
        assert_eq!(r.d.frames(), 1);
        // Redraws the window system asks for always present.
        r.d.frame().unwrap();
        assert_eq!(r.d.frames(), 2);
        // A hover fade starts at the old colour, then presents.
        r.ev(Event::PointerMoved(r.center(0)));
        for (t, frames) in [(0.1, 2), (0.175, 3)] {
            r.d.freeze_clock(t);
            r.d.schedule_mut().fired();
            r.d.frame().unwrap();
            assert_eq!(r.d.frames(), frames, "at {t}");
        }
    }

    #[test]
    fn line_breaks_are_newlines() {
        let options = XDialogOptions { title: "a\r\nb".into(),
                                       main_instruction: "c\rd".into(),
                                       message: "e\r\nf\rg".into(),
                                       buttons: vec!["O\r\nK".into()],
                                       ..Default::default() };
        let c = DialogContent::new(DialogKind::Message, options);
        assert_eq!([&c.title, &c.heading, &c.body, &c.buttons[0]], ["a\nb", "c\nd", "e\nf\ng", "O\nK"]);
        let mut r = Rig::new(DialogKind::Progress, &[], None);
        r.d.set_text("x\r\ny");
        assert_eq!(r.d.content.body, "x\ny");
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
        let h = r.d.desired_size().height;
        r.d.set_text(&"many words ".repeat(40));
        r.at(3.0);
        assert!(r.d.desired_size().height > h);
        assert_eq!(r.d.take_resize(), Some(r.d.desired_size()));
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
        assert_eq!(rx.try_recv().ok().and_then(Result::ok), Some(XDialogResult::WindowClosed));
    }

    #[test]
    fn scale_factor_change_keeps_logical_size() {
        let mut r = Rig::new(DialogKind::Message, &["OK"], None);
        r.at(0.0);
        r.d.scale_changed(2.0);
        let want = r.d.physical_size(2.0);
        r.d.resized(want);
        r.at(0.1);
        assert_eq!(r.d.take_resize(), None, "same logical size: no resize request");
        assert_eq!(r.d.read_rgba().unwrap().0, want[0]);
        // Zero size (minimised) skips drawing without error.
        r.d.resized([0, 0]);
        r.at(0.2);
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

    #[test]
    fn screen_reader_click_and_focus() {
        let mut r = Rig::new(DialogKind::Message, &["A", "B"], None);
        r.at(0.0);
        r.d.a11y_request(Request::Focus(0));
        assert_eq!(r.d.st.focus, Some(0));
        r.d.a11y_request(Request::Focus(5));
        assert_eq!(r.d.st.focus, Some(0), "unknown buttons are ignored");
        r.d.a11y_request(Request::Click(1));
        assert_eq!(r.result(), Some(XDialogResult::ButtonPressed(1)));
    }

    #[test]
    fn long_press_still_clicks_and_one_batch_press_release_activates() {
        let mut r = Rig::new(DialogKind::Message, &["Cancel", "OK"], None);
        r.at(0.0);
        let c = r.center(0);
        r.ev(Event::PointerMoved(c));
        r.ev(primary(c, true));
        r.at(0.1);
        r.at(5.0);
        r.ev(primary(c, false));
        assert_eq!(r.result(), Some(XDialogResult::ButtonPressed(0)), "no maximum click duration");

        let mut r = Rig::new(DialogKind::Message, &["Cancel", "OK"], None);
        r.at(0.0);
        let c = r.center(0);
        r.d.handle_events([Event::PointerMoved(c), primary(c, true), primary(c, false)]);
        assert_eq!(r.d.st.focus, Some(0), "the press focused the button");
        assert_eq!(r.result(), Some(XDialogResult::ButtonPressed(0)));
    }

    #[test]
    fn release_outside_neither_activates_nor_hovers() {
        let mut r = Rig::new(DialogKind::Message, &["Cancel", "OK"], None);
        r.at(0.0);
        let c = r.center(0);
        let (px, py) = (c.x as u32, c.y as u32);
        r.ev(Event::PointerMoved(c));
        r.ev(primary(c, true));
        r.at(1.0);
        r.at(1.5);
        assert_eq!(r.pixel(px, py), [255, 0, 0, 255], "hovered while pressed inside");
        let off = Point::new(200.0, 5.0);
        r.ev(Event::PointerMoved(off));
        r.ev(primary(off, false));
        r.at(2.0);
        r.at(2.5);
        assert!(!r.d.is_closed());
        assert_eq!(r.pixel(px, py), [160, 160, 160, 255], "not hovered after the release outside");
        assert_eq!(r.d.st.focus, Some(0), "the press still focused it");
    }

    /// A real theme's dialog at `ppp`, one frame presented.
    fn themed(backend: crate::XDialogBackend, kind: DialogKind, options: XDialogOptions, ppp: f64) -> (Dialog, oneshot::Receiver<Answer>) {
        let (tx, rx) = oneshot::channel::<Answer>();
        let text = Text::shared().expect("text system");
        let params = DialogParams { id: 3,
                                    content: DialogContent::new(kind, options),
                                    appearance: Appearance::default(),
                                    system_appearance: None,
                                    ppp,
                                    max_height: 800.0,
                                    clock: DialogClock::frozen(0.0),
                                    sender: Some(DialogReply::Message(tx).opened()),
                                    font_wait: Duration::ZERO,
                                    text: text.clone() };
        let mut d = Dialog::new(super::super::theme::with_style(backend, crate::backends::macos::MacStyle::Legacy), params);
        let size = d.physical_size(ppp);
        d.attach(Target::Memory(MemorySurface::new(&text).expect("memory surface")), ppp, size);
        d.frame().unwrap();
        (d, rx)
    }

    fn a11y_options(buttons: &[&str]) -> XDialogOptions {
        XDialogOptions { title: "Title".into(),
                         main_instruction: "Heading".into(),
                         message: "Body text".into(),
                         icon: XDialogIcon::Warning,
                         buttons: buttons.iter().map(|s| s.to_string()).collect(),
                         ..Default::default() }
    }

    /// An AccessKit request for button `button`.
    fn request(action: accesskit::Action, button: usize) -> Request {
        let req = accesskit::ActionRequest { action,
                                             target_tree: accesskit::TreeId::ROOT,
                                             target_node: a11y::button_node(button),
                                             data: None };
        Request::from_action(&req).expect("a button request")
    }

    #[test]
    fn accessibility_tree_and_actions() {
        use accesskit::{Action, Role};
        for backend in [crate::XDialogBackend::Fluent, crate::XDialogBackend::Ubuntu] {
            let (mut d, rx) = themed(backend, DialogKind::Message, a11y_options(&["No", "Yes"]), 1.0);
            let t = d.a11y_tree();
            let with_role = |role: Role| t.nodes.iter().filter(move |(_, n)| n.role() == role).map(|(_, n)| n);
            let root = with_role(Role::AlertDialog).next().expect("dialog root");
            assert_eq!(root.label(), Some("Title"), "{backend:?}");
            assert_eq!(root.description(), Some("Heading\nBody text"));
            let mut buttons: Vec<_> = with_role(Role::Button).filter_map(|n| n.label()).collect();
            buttons.sort();
            assert_eq!(buttons, ["No", "Yes"], "{backend:?}");
            assert!(with_role(Role::Button).all(|n| n.supports_action(Action::Click) && n.supports_action(Action::Focus)));
            assert_eq!(with_role(Role::DefaultButton).count(), 0, "{backend:?}");
            let labels: Vec<_> = with_role(Role::Label).filter_map(|n| n.value()).collect();
            assert!(labels.contains(&"Heading") && labels.contains(&"Body text"), "{backend:?}: {labels:?}");
            // The icon reads as its severity word.
            assert!(labels.contains(&"Warning") && with_role(Role::Image).next().is_none(), "{backend:?}: {labels:?}");
            // Every node has bounds; the default button has focus.
            assert!(t.nodes.iter().all(|(_, n)| n.bounds().is_some()), "{backend:?}");
            assert_eq!(t.focus, a11y::button_node(1));

            // Screen-reader requests take the keyboard paths.
            d.a11y_request(request(Action::Focus, 0));
            d.frame().unwrap();
            assert_eq!(d.a11y_tree().focus, a11y::button_node(0), "{backend:?}");
            d.a11y_request(request(Action::Click, 0));
            assert!(matches!(rx.try_recv(), Ok(Ok(XDialogResult::ButtonPressed(0)))), "{backend:?}");
        }
    }

    #[test]
    fn accessibility_progress() {
        use accesskit::Role;
        for backend in [crate::XDialogBackend::Fluent, crate::XDialogBackend::Ubuntu] {
            let (mut d, _rx) = themed(backend, DialogKind::Progress, a11y_options(&["Cancel"]), 1.0);
            d.set_progress_value(0.42);
            d.frame().unwrap();
            let t = d.a11y_tree();
            assert!(t.nodes.iter().any(|(_, n)| n.role() == Role::Dialog));
            let bar = t.nodes.iter().map(|(_, n)| n).find(|n| n.role() == Role::ProgressIndicator).expect("progress bar");
            assert_eq!((bar.numeric_value(), bar.min_numeric_value(), bar.max_numeric_value(), bar.value()),
                       (Some(42.0), Some(0.0), Some(100.0), Some("42%")),
                       "{backend:?}");
            d.set_progress_indeterminate();
            d.frame().unwrap();
            let t = d.a11y_tree();
            let bar = t.nodes.iter().map(|(_, n)| n).find(|n| n.role() == Role::ProgressIndicator).expect("progress bar");
            assert_eq!((bar.numeric_value(), bar.value()), (None, None), "{backend:?}");
        }
    }

    /// A real theme's `Custom` icon dialog at `ppp`, one frame presented.
    fn custom_icon(backend: crate::XDialogBackend, icon_source: Option<crate::XDialogIconSource>, ppp: f64) -> Dialog {
        let options = XDialogOptions { title: "Title".into(),
                                       message: "A body text wide enough to set the window width.".into(),
                                       icon: XDialogIcon::Custom,
                                       icon_source,
                                       buttons: vec!["OK".into()],
                                       ..Default::default() };
        themed(backend, DialogKind::Message, options, ppp).0
    }

    #[test]
    fn custom_icon_shows_the_icon_source() {
        use crate::XDialogIconSource::{Bytes, File};
        let dir = std::env::temp_dir().join(format!("xdialog-icon-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ico = dir.join("app.ico");
        std::fs::write(&ico, crate::icon::tests::ico_bytes(16, 256)).unwrap();
        for backend in [crate::XDialogBackend::Fluent, crate::XDialogBackend::Ubuntu] {
            let without = custom_icon(backend, None, 1.0);
            let missing = custom_icon(backend, Some(File(dir.join("missing.ico"))), 1.0);
            let garbage = custom_icon(backend, Some(Bytes(b"garbage".as_slice().into())), 1.0);
            // No file, or one that can't be read: laid out as without an icon.
            assert_eq!(without.desired_size(), garbage.desired_size(), "{backend:?}");
            assert_eq!(without.desired_size(), missing.desired_size(), "{backend:?}");
            assert!(without.icon_file().is_none() && without.icon.is_none());

            for (ppp, source) in [(1.0, File(ico.clone())), (2.0, Bytes(crate::icon::tests::ico_bytes(16, 256).into()))] {
                let d = custom_icon(backend, Some(source), ppp);
                assert_ne!(d.desired_size(), without.desired_size(), "{backend:?}: room for the icon");
                // Rendered at one texel per physical pixel.
                let size = (d.theme.icon_size() * ppp).round() as u32;
                assert_eq!(d.icon.as_ref().map(|(px, img)| (*px, img.as_ref().map(Image::size))), Some((size, Some([size; 2]))));
                // Some pixel of the window is the icon's red.
                let (_, _, px) = d.read_rgba().unwrap();
                let red = px.as_chunks::<4>().0.iter().any(|p| p[0] > 240 && p[1] < 16 && p[2] < 16);
                assert!(red, "{backend:?} at {ppp}x: no red icon pixel");
                // No accessibility node for a custom icon (decorative).
                assert!(d.a11y_tree().nodes.iter().all(|(_, n)| n.value() != Some("Custom")), "{backend:?}");
                assert_eq!(d.a11y_tree().nodes.len(), 3, "{backend:?}: root, body, button");
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
