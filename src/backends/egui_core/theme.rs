//! The Theme contract, widget-based.
//!
//! A theme builds the whole dialog every frame with normal egui layout and its OWN widgets
//! (`impl egui::Widget`): it paints them precisely and gets hover/press/focus from egui
//! (`Response` + egui focus memory). Core is plumbing (event loop, windows, input translation,
//! fonts/bidi, presenter, appearance/DPI, requests, test hooks) plus two thin shared pieces:
//!
//! - the **measure pass**: before the window exists core runs [`Theme::ui`] once in an ordinary
//!   pass whose shapes it discards ([`FrameInfo::sizing`]) and creates the window at
//!   [`DialogUiOutput::desired_size`] (no flicker);
//! - the **keyboard policy** ([`KeyboardPolicy`]): keys never reach egui; core moves egui focus
//!   (`Memory::request_focus(button_id(i))`), activates buttons and closes on Escape according to
//!   the policy the theme picks, and tells widgets what they need through [`FrameInfo`].
//!
//! Data flow per frame (core, `dialog.rs`): input -> `RawInput { time: dialog clock, .. }` ->
//! keyboard policy (may move egui focus / activate / close) -> `ctx.run_ui(raw, |ui| theme.ui(..))`
//! (exactly one pass, `max_passes = 1`) -> [`DialogUiOutput`] (pointer activation, focus order,
//! desired size) -> tessellate + present -> schedule from egui's `repaint_delay`.
//!
//! FROZEN for the parallel work packages: changes must be additive and go through the orchestrator.

use std::time::Duration;

use egui::{Color32, Id, Rect, Response, Sense, Vec2};

use super::appearance::Appearance;
use super::fonts::{FaceRef, FontRegistry};
use crate::model::XDialogIcon;

// ------------------------------------------------------------------------------------------------
// Environment
// ------------------------------------------------------------------------------------------------

/// The OS the dialog is running on (not the theme's look).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Platform {
    Linux,
    Windows,
}

impl Platform {
    /// The platform this binary was compiled for.
    pub(crate) const fn current() -> Platform {
        if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Linux
        }
    }
}

/// Everything a theme may base its tokens on.
#[derive(Clone, Debug)]
pub(crate) struct ThemeEnv {
    /// Resolved light/dark + accent (XDialogTheme override and test overrides already applied).
    pub appearance: Appearance,
    #[allow(dead_code)] // theme API surface: no theme branches on the platform yet
    pub platform: Platform,
}

// ------------------------------------------------------------------------------------------------
// What to show (core -> theme)
// ------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DialogKind {
    Message,
    Progress,
}

/// Progress bar state. Times are dialog-clock seconds (= `ui.input(|i| i.time)`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ProgressView {
    /// `value` in 0..=1 (the latest target). `prev` is the previous determinate target (equal to
    /// `value` when there was none) and `changed_at` the time of the latest `set_value`.
    ///
    /// For skia's "300 ms OutCubic from the currently displayed bar end" use
    /// `anim::animate_f32(ctx, id, value, Transition::new(0.3, Easing::OutCubic))`: the tween
    /// remembers the displayed value across indeterminate phases as long as the widget id is stable.
    /// skia freezes a running value tween while indeterminate: call `anim::stop::<f32>(ctx, id)`
    /// in the indeterminate branch to get exactly that.
    Determinate { value: f32, prev: f32, changed_at: f64 },
    /// Indeterminate mode. `since` = time the bar entered indeterminate mode (unchanged by further
    /// `set_indeterminate` calls; WinUI phase origin: `IsIndeterminate = true` again is a no-op).
    /// `restarted_at` = time of the latest `set_indeterminate` call (skia resets its capsule phase
    /// on every call, `progress.rs:81-85`); equals `since` after the first call.
    Indeterminate { since: f64, restarted_at: f64 },
}

/// Size limits for the dialog content (logical px).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SizeLimits {
    /// Max client height (monitor work area * 0.9, or 800 when unknown, e.g. host mode). A theme
    /// may ignore it (skia had no height cap) or put the body in an `egui::ScrollArea`.
    pub max_height: f32,
    /// Max client width (work area width * 0.9, or 4096). Themes normally have their own, smaller
    /// width rules.
    pub max_width: f32,
}

/// Per-frame state that does not come from egui (core -> widgets).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FrameInfo {
    /// Dialog clock in seconds. Identical to `ui.input(|i| i.time)` (core feeds it as
    /// `RawInput::time`), repeated here for convenience.
    pub time: f64,
    /// Physical pixels per logical pixel (= `ui.ctx().pixels_per_point()`).
    pub ppp: f32,
    /// `true` only in the measure pass: an ordinary pass over the root ui (NOT an
    /// egui `sizing_pass`/invisible ui, so layout and widget registration are identical to a real
    /// pass) without events, whose shapes core discards. The theme must build the same layout and
    /// report the same `desired_size` it would in a real pass. Core calls
    /// `anim::reset_animations` after it, so tweens snap on the first real frame.
    pub sizing: bool,
    /// The window has keyboard focus (false in backdrop).
    pub window_focused: bool,
    /// Whether the focused widget should show its focus visual right now, per
    /// [`KeyboardPolicy::focus_visibility`] (keyboard modality / focus-on-open). Widgets combine it
    /// with `response.has_focus()`; [`ButtonInteraction::focus_visible`] does that.
    pub focus_visible: bool,
    /// API index of the button that shows the keyboard-pressed look: Space held on it
    /// ([`SpaceKey::ActivateOnRelease`]) or a running [`KeyboardPolicy::activate_flash`].
    pub key_pressed: Option<usize>,
    /// Keyboard scroll request for the body viewport (logical px; positive = reveal content
    /// further down), from PageUp/PageDown/Home/End when [`KeyboardPolicy::scroll_keys`]. A theme
    /// with a body `ScrollArea` applies it inside the area with
    /// `ui.scroll_with_delta(Vec2::new(0.0, -scroll_request))`; otherwise it ignores it.
    pub scroll_request: f32,
}

/// The dialog content handed to [`Theme::ui`]. All strings are the raw API strings.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DialogView<'a> {
    pub kind: DialogKind,
    /// Window title (title bar). Themes don't paint it.
    #[allow(dead_code)] // theme API surface: no theme reads it yet
    pub title: &'a str,
    /// `options.main_instruction` ("" = none).
    pub heading: &'a str,
    /// `options.message`, or the latest `set_text` for progress dialogs ("" = none).
    pub body: &'a str,
    pub icon: &'a XDialogIcon,
    /// Button labels in API order. Every button index in this contract is an index into this slice.
    pub buttons: &'a [String],
    /// Same length as `buttons`. Never set by the public API today; test hooks can force it.
    /// [`ButtonInteraction::interact`] makes a disabled button non-interactive and unfocusable.
    pub disabled: &'a [bool],
    /// `Some` for progress dialogs.
    pub progress: Option<ProgressView>,
    #[allow(dead_code)] // theme API surface: no theme reads it yet
    pub env: &'a ThemeEnv,
    pub limits: SizeLimits,
    pub frame: FrameInfo,
}

impl DialogView<'_> {
    /// Whether button `index` is disabled.
    pub(crate) fn is_disabled(&self, index: usize) -> bool {
        self.disabled.get(index).copied().unwrap_or(false)
    }
}

// ------------------------------------------------------------------------------------------------
// What the theme built (theme -> core)
// ------------------------------------------------------------------------------------------------

/// One focusable button as laid out this pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ButtonInfo {
    /// API index (into `view.buttons`). Its egui id MUST be [`button_id`]`(index)`.
    pub index: usize,
    /// Widget rect, logical px (test hooks report it; core never hit-tests it).
    pub rect: Rect,
}

/// Arrow-key axis for [`KeyboardPolicy::arrows`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ArrowAxis {
    /// Left/Right move through `arrow_order`; Up/Down do nothing.
    #[default]
    Horizontal,
    /// Up/Down move through `arrow_order` (stacked buttons); Left/Right do nothing.
    #[allow(dead_code)] // theme API surface: no theme uses it yet
    Vertical,
}

/// Result of one [`Theme::ui`] call.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DialogUiOutput {
    /// Logical client size the content needs. From the measure pass it becomes the initial
    /// window size; in later passes, a change (e.g. `set_text` reflowed the body) makes core
    /// `request_inner_size` (top-left kept). Must be independent of the current window size.
    pub desired_size: Vec2,
    /// Every button in Tab order (Tab moves forward through this Vec, Shift+Tab backward). Core
    /// skips disabled buttons (`view.is_disabled`) when navigating.
    pub buttons: Vec<ButtonInfo>,
    /// API indices in on-screen order along `arrow_axis` (left to right / top to bottom).
    pub arrow_order: Vec<usize>,
    pub arrow_axis: ArrowAxis,
    /// API index of the default button: initial focus ([`KeyboardPolicy::focus_on_open`]) and the
    /// Enter target when nothing is focused ([`KeyboardPolicy::enter_falls_back_to_default`]).
    pub default_button: Option<usize>,
    /// A button activated by the POINTER this pass ([`ButtonInteraction::activated`]). Keyboard
    /// activation is decided by core and never reported here.
    pub activated: Option<usize>,
}

// ------------------------------------------------------------------------------------------------
// Window style and keyboard policy
// ------------------------------------------------------------------------------------------------

/// Window-level style.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WindowStyle {
    /// Buffer clear colour / window background.
    pub clear: Color32,
    /// DWMWA_USE_IMMERSIVE_DARK_MODE / winit `with_theme(Dark)`.
    pub dark_titlebar: bool,
}

/// When [`FrameInfo::focus_visible`] is true.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FocusVisibility {
    /// Whenever something is focused (skia: the focused button always shows its focus look,
    /// including after a pointer press moved focus; the theme itself hides it while hovering).
    Always,
    /// Only after keyboard navigation (":focus-visible", WinUI FocusState.Keyboard). A pointer
    /// press anywhere in the window hides it until the next Tab/arrow key. `on_open`: visible on
    /// open (WinUI ContentDialog shows it on the default button before any pointer input).
    KeyboardOnly { on_open: bool },
}

/// Arrow-key focus navigation along [`DialogUiOutput::arrow_order`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArrowNav {
    Off,
    /// Stop at the ends (WinUI).
    Clamp,
    /// Wrap around (skia).
    Wrap,
}

/// What Space does on the focused button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SpaceKey {
    #[allow(dead_code)] // theme API surface: no theme uses it yet
    Ignore,
    /// Activate on key press (skia).
    ActivateOnPress,
    /// Press shows the pressed look ([`FrameInfo::key_pressed`]); release activates if focus did
    /// not move meanwhile (WinUI). Focus loss or Escape cancels.
    ActivateOnRelease,
}

/// Keyboard / focus / activation policy. Core implements the state machine; the theme picks.
///
/// | field | linux (skia look) | fluent (WinUI) |
/// |---|---|---|
/// | focus_on_open | true (last = default button) | true (default button) |
/// | focus_visibility | Always | KeyboardOnly { on_open: true } |
/// | tab | true (wraps) | true (wraps) |
/// | arrows | Wrap | Clamp |
/// | nav_repeat | true | true |
/// | home_end | false | false |
/// | enter | true (on press, not repeat) | true (on press, not repeat) |
/// | enter_falls_back_to_default | false | true |
/// | space | ActivateOnPress | ActivateOnRelease |
/// | activate_flash | None | None |
/// | escape_closes | true | true |
/// | scroll_keys | false | true |
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct KeyboardPolicy {
    /// After the measure pass, focus [`DialogUiOutput::default_button`].
    pub focus_on_open: bool,
    pub focus_visibility: FocusVisibility,
    /// Tab / Shift+Tab move focus through [`DialogUiOutput::buttons`], wrapping.
    pub tab: bool,
    pub arrows: ArrowNav,
    /// Tab/arrow auto-repeat moves focus (activation keys and Escape always ignore repeat).
    pub nav_repeat: bool,
    /// Home/End focus the first/last button of `arrow_order` (takes priority over `scroll_keys`).
    pub home_end: bool,
    /// Enter (key press, not repeat) activates the focused button.
    pub enter: bool,
    /// Enter with nothing focused activates the default button.
    pub enter_falls_back_to_default: bool,
    pub space: SpaceKey,
    /// `Some(d)`: on key activation show the pressed look ([`FrameInfo::key_pressed`]) for `d`,
    /// THEN deliver the activation (GTK ACTIVATE_TIMEOUT). `None`: deliver immediately.
    pub activate_flash: Option<Duration>,
    /// Escape (not repeat) acts as CloseRequested -> `WindowClosed`. Also works with no buttons.
    pub escape_closes: bool,
    /// PageUp/PageDown (and Up/Down when there are no buttons) produce
    /// [`FrameInfo::scroll_request`].
    pub scroll_keys: bool,
}

// ------------------------------------------------------------------------------------------------
// The trait
// ------------------------------------------------------------------------------------------------

/// Implemented by `linux_egui::LinuxTheme` and `fluent_egui::FluentTheme`.
pub(crate) trait Theme: Send + Sync + 'static {
    type Tokens: Clone + Send + 'static;

    /// Stable id used by `XDIALOG_BACKEND` and the harnesses ("linux" | "fluent").
    fn id(&self) -> &'static str;

    fn keyboard_policy(&self) -> KeyboardPolicy;

    /// Resolve colours/metrics for an environment. Called on open and on appearance change
    /// (core then calls `anim::reset_animations`).
    fn tokens(&self, env: &ThemeEnv) -> Self::Tokens;

    fn window_style(&self, tk: &Self::Tokens) -> WindowStyle;

    /// Add the theme's primary faces and family chains. Fallback fonts are appended by core.
    /// CONTRACT: must bind `Proportional`, `Monospace` AND every `Name(..)` family the theme uses
    /// (epaint panics with "FontFamily::.. is not bound to any fonts" otherwise; egui's
    /// default_fonts are off). Core asserts this in debug builds right after the call.
    fn install_fonts(&self, defs: &mut egui::FontDefinitions, reg: &FontRegistry);

    /// Families that core must extend with fallback faces (e.g. Proportional + Name("bold")).
    fn text_families(&self) -> Vec<egui::FontFamily>;

    /// The subset of [`Theme::text_families`] rendered bold (weight 600 or more): core puts each
    /// fallback family's bold face ahead of its regular face in these chains, so e.g. a CJK heading
    /// is bold too. Default: none.
    fn bold_families(&self) -> Vec<egui::FontFamily> {
        Vec::new()
    }

    /// Font faces that back `text_families`, for core's glyph-coverage check.
    fn primary_faces(&self, reg: &FontRegistry) -> Vec<FaceRef>;

    /// Adjust egui style (text options via `style.visuals.text_options`, spacing, scroll bars,
    /// egui widget visuals if the theme uses stock widgets such as `ScrollArea`). Called when
    /// tokens change, for both the light and dark style of the context. Core applies
    /// [`core_style_overrides`] AFTER this call.
    fn configure_style(&self, tk: &Self::Tokens, style: &mut egui::Style);

    /// Build the whole dialog into `ui` (the root ui: covers the client rect, origin top-left,
    /// no margin; the buffer was cleared to `window_style().clear`). Use egui layout and the
    /// theme's own widgets. Buttons MUST use [`ButtonInteraction::interact`] (or at least
    /// [`button_id`] + `Sense::click()`), so core's keyboard policy can focus them.
    fn ui(&self, tk: &Self::Tokens, view: &DialogView<'_>, ui: &mut egui::Ui) -> DialogUiOutput;
}

// ------------------------------------------------------------------------------------------------
// Core-mandated egui settings (applied by core; documented here because widget semantics rely on them)
// ------------------------------------------------------------------------------------------------

/// Style settings core forces on every style after [`Theme::configure_style`]:
/// exact-rect hit testing (`interact_radius = 0`), no selectable labels, instant programmatic
/// scrolling (`ui.scroll_with_delta` for [`FrameInfo::scroll_request`] lands on the same frame).
pub(crate) fn core_style_overrides(style: &mut egui::Style) {
    style.scroll_animation = egui::style::ScrollAnimation::none();
    style.interaction.interact_radius = 0.0;
    style.interaction.selectable_labels = false;
    style.interaction.multi_widget_text_select = false;
}

/// Context options core forces:
/// - `max_passes = 1`: a discarded pass would re-run the theme with empty input and
///   `time += predicted_dt`, losing clicks and determinism.
/// - clicks: `max_click_dist`/`max_click_duration` infinite, so a long or wobbly press still clicks
///   (the widget additionally requires the release to be inside, see [`ButtonInteraction`]).
/// - `surrender_focus_on = Never`: clicking empty space keeps focus (skia, WinUI).
/// - no keyboard zoom.
pub(crate) fn core_options(opts: &mut egui::Options) {
    opts.max_passes = std::num::NonZeroUsize::MIN;
    opts.zoom_with_keyboard = false;
    opts.input_options.max_click_dist = f32::INFINITY;
    opts.input_options.max_click_duration = f64::INFINITY;
    opts.input_options.surrender_focus_on = egui::SurrenderFocusOn::Never;
}

// ------------------------------------------------------------------------------------------------
// Button interaction helper (shared by every theme's button widget)
// ------------------------------------------------------------------------------------------------

/// The egui id of the button with API index `index`. Core's keyboard policy focuses buttons by
/// this id, so theme button widgets must interact with exactly this id.
pub(crate) fn button_id(index: usize) -> Id {
    Id::new("xdialog.button").with(index)
}

/// Whether the pointer is geometrically inside any of the first `count` buttons, callable BEFORE
/// the buttons are added this pass (egui hit-tests the current pointer against the previous
/// pass's widget rects at the start of the pass; `ctx.read_response` exposes that). Used for
/// skia's "focus look suppressed while any button is hovered".
pub(crate) fn any_button_contains_pointer(ctx: &egui::Context, count: usize) -> bool {
    (0..count).any(|i| ctx.read_response(button_id(i)).is_some_and(|r| r.contains_pointer()))
}

/// Everything a button widget needs to pick its look, from egui + core's keyboard state.
///
/// Mapping hints:
/// - skia: hover = `contains_pointer` (geometric, also while another button is held); pressed
///   look = `pointer_down || key_pressed` (kept while dragged off); focus look =
///   `focus_visible && !any_contains_pointer` (the row widget knows all buttons).
/// - WinUI: hover = `hovered` (egui suppresses hover of other widgets while one is held, like
///   pointer capture); pressed = `pointer_down && contains_pointer || key_pressed`.
#[derive(Clone, Debug)]
pub(crate) struct ButtonInteraction {
    pub response: Response,
    #[allow(dead_code)] // theme API surface: no theme reads it yet
    pub index: usize,
    /// Pointer is geometrically inside the rect (exact rect, previous-pass layout).
    pub contains_pointer: bool,
    /// egui hover AND `contains_pointer`: inside AND no other widget is being pressed. (egui alone
    /// also reports the clicked widget as hovered on the release pass even when released outside,
    /// which would flash a one-frame hover tween; the `contains_pointer` term removes that.)
    pub hovered: bool,
    /// A pointer press started on this button and is still held (inside or not).
    pub pointer_down: bool,
    /// Keyboard pressed look ([`FrameInfo::key_pressed`]).
    pub key_pressed: bool,
    /// Has egui keyboard focus.
    #[allow(dead_code)] // theme API surface: no theme reads it yet
    pub focused: bool,
    /// `focused && view.frame.focus_visible`.
    pub focus_visible: bool,
    /// Released inside after a press on it this pass (pointer activation). Report it in
    /// [`DialogUiOutput::activated`].
    pub activated: bool,
    pub disabled: bool,
}

impl ButtonInteraction {
    /// Interact with the (already allocated) button `rect` for API index `index`: senses clicks
    /// with [`button_id`], moves focus to the button on a primary press, and derives the states.
    pub(crate) fn interact(ui: &egui::Ui, rect: Rect, index: usize, view: &DialogView<'_>) -> Self {
        let disabled = view.is_disabled(index) || !ui.is_enabled();
        let sense = if disabled { Sense::hover() } else { Sense::click() };
        let response = ui.interact(rect, button_id(index), sense);
        let pointer_down = !disabled && response.is_pointer_button_down_on();
        let contains_pointer = response.contains_pointer();
        let activated = !disabled && response.clicked() && contains_pointer;
        // skia/WinUI move focus to a button on press. A press and release delivered in the same
        // pass never shows `is_pointer_button_down_on` (false on the release pass), so a click
        // this pass also moves focus (only primary presses reach egui, see `input.rs`).
        let pressed_here = pointer_down && ui.input(|i| i.pointer.primary_pressed());
        if (pressed_here || activated) && !response.has_focus() {
            response.request_focus();
        }
        let focused = response.has_focus();
        ButtonInteraction { index,
                            contains_pointer,
                            hovered: !disabled && response.hovered() && contains_pointer,
                            pointer_down,
                            key_pressed: !disabled && view.frame.key_pressed == Some(index),
                            focused,
                            focus_visible: focused && view.frame.focus_visible,
                            activated,
                            disabled,
                            response }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, PointerButton, Pos2};

    /// One pass with a single button at `rect`; returns its interaction.
    fn pass(ctx: &egui::Context, t: f64, events: Vec<Event>) -> ButtonInteraction {
        let env = ThemeEnv { appearance: Appearance::default(), platform: Platform::current() };
        let icon = XDialogIcon::None;
        let buttons = vec!["OK".to_string()];
        let disabled = vec![false];
        let view = DialogView { kind: DialogKind::Message,
                                title: "",
                                heading: "",
                                body: "",
                                icon: &icon,
                                buttons: &buttons,
                                disabled: &disabled,
                                progress: None,
                                env: &env,
                                limits: SizeLimits { max_height: 800.0, max_width: 4096.0 },
                                frame: FrameInfo { time: t,
                                                   ppp: 1.0,
                                                   sizing: false,
                                                   window_focused: true,
                                                   focus_visible: false,
                                                   key_pressed: None,
                                                   scroll_request: 0.0 } };
        let raw = egui::RawInput { time: Some(t),
                                   screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(200.0, 100.0))),
                                   events,
                                   ..Default::default() };
        let mut out = None;
        let mut full = ctx.run_ui(raw, |ui| out = Some(ButtonInteraction::interact(ui, rect(), 0, &view)));
        full.textures_delta.clear();
        out.unwrap()
    }

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(80.0, 30.0))
    }

    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Default::default() }
    }

    fn ctx() -> egui::Context {
        let ctx = egui::Context::default();
        ctx.options_mut(core_options);
        ctx.all_styles_mut(core_style_overrides);
        ctx
    }

    #[test]
    fn release_outside_neither_activates_nor_hovers() {
        let ctx = ctx();
        let c = rect().center();
        let off = Pos2::new(150.0, 80.0);
        pass(&ctx, 0.0, vec![]);
        pass(&ctx, 0.1, vec![Event::PointerMoved(c)]);
        let st = pass(&ctx, 0.2, vec![button(c, true)]);
        assert!(st.pointer_down && st.focused && st.hovered);
        let st = pass(&ctx, 0.3, vec![Event::PointerMoved(off)]);
        assert!(st.pointer_down && !st.contains_pointer && !st.hovered);
        let st = pass(&ctx, 0.4, vec![button(off, false)]);
        // egui reports the click (press started here) and hovers the clicked widget on the
        // release pass; the helper must not.
        assert!(st.response.clicked() && st.response.hovered());
        assert!(!st.activated && !st.hovered && !st.pointer_down && st.focused);
    }

    #[test]
    fn press_and_release_in_one_pass_activates_and_focuses() {
        let ctx = ctx();
        let c = rect().center();
        pass(&ctx, 0.0, vec![]);
        let st = pass(&ctx, 0.1, vec![Event::PointerMoved(c), button(c, true), button(c, false)]);
        assert!(st.activated && st.focused);
    }
}
