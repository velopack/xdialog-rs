//! The Theme contract.
//!
//! A theme builds the whole dialog every frame through a [`Ui`]: it lays out with plain
//! arithmetic (plus the small helpers in `ui.rs`), paints with the `Ui` painting primitives (the
//! `draw` layer's shapes, colours and text layouts) and gets hover/press/focus for its buttons
//! from [`ButtonInteraction`]. Core owns everything else:
//!
//! - the **measure pass** (see [`Theme::ui`]): the window is created at the size the theme reports
//!   before it exists (no flicker);
//! - input: pointer hit testing against the last pass's widget rects, focus, activation, and the
//!   **keyboard policy** ([`KeyboardPolicy`]) the theme picks; widgets learn what they need through
//!   [`FrameInfo`] and [`ButtonInteraction`];
//! - tweens, frame pacing, text layout caching, the drawing backend and accessibility.

use super::appearance::Appearance;
pub(crate) use super::text::ThemeFonts;
pub(crate) use super::ui::{Id, Ui};
use crate::backends::draw::{Color, Image, Rect, Size};
use crate::backends::fluent::FluentTheme;
use crate::backends::ubuntu::UbuntuTheme;
use crate::model::{XDialogBackend, XDialogIcon};

// ------------------------------------------------------------------------------------------------
// What to show (core -> theme)
// ------------------------------------------------------------------------------------------------

/// Which kind of dialog to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogKind {
    /// A message dialog (`show_message*`).
    Message,
    /// A progress dialog (`show_progress*`).
    Progress,
}

/// Progress bar state. Times are dialog-clock seconds ([`Ui::time`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ProgressView {
    /// `value` in 0..=1 (the latest target). Animate towards it with a tween (stable id).
    Determinate { value: f32 },
    /// Indeterminate mode. `since` = time the bar entered indeterminate mode (unchanged by further
    /// `set_indeterminate` calls); `restarted_at` = time of the latest `set_indeterminate` call.
    Indeterminate { since: f64, restarted_at: f64 },
}

/// Per-frame state from core's keyboard policy (core -> widgets).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct FrameInfo {
    /// Whether the focused widget should show its focus visual right now, per
    /// [`KeyboardPolicy::focus_visibility`] ([`ButtonInteraction::focus_visible`] combines it).
    pub focus_visible: bool,
    /// API index of the button that shows the keyboard-pressed look (Space held on it,
    /// [`SpaceKey::ActivateOnRelease`]).
    pub key_pressed: Option<usize>,
    /// Keyboard scroll request for the body viewport (logical px; positive = reveal content
    /// further down): PageUp/PageDown/Home/End when [`KeyboardPolicy::scroll_keys`]. A theme with
    /// a scrolling body applies it; otherwise it ignores it.
    pub scroll_request: f64,
    /// Mouse wheel scroll since the last pass (same units and sign as `scroll_request`). A theme
    /// applies it only while the pointer is over its scrolling viewport, as a native scroll view.
    pub wheel_request: f64,
}

/// The dialog content handed to [`Theme::ui`]. All strings are the raw API strings.
#[derive(Clone, Copy)]
pub(crate) struct DialogView<'a> {
    /// `options.main_instruction` ("" = none).
    pub heading: &'a str,
    /// `options.message`, or the latest `set_text` for progress dialogs ("" = none).
    pub body: &'a str,
    pub icon: &'a XDialogIcon,
    /// `XDialogIcon::Custom`: the icon image, rendered at [`Theme::icon_size`] × the scale (1 texel
    /// : 1 physical px). `None`: no icon file, or it could not be loaded.
    pub custom_icon: Option<&'a Image>,
    /// Button labels in API order. Every button index in this contract is an index into this slice.
    pub buttons: &'a [String],
    /// `Some` for progress dialogs.
    pub progress: Option<ProgressView>,
    /// Max client height, logical px (primary monitor height * 0.9, or 800 when no monitor is
    /// known). A theme may ignore it or scroll the body.
    pub max_height: f64,
    pub frame: FrameInfo,
}

impl DialogView<'_> {
    /// Whether the dialog shows an icon: a severity icon, or `Custom` with its image loaded.
    pub(crate) fn has_icon(&self) -> bool {
        match self.icon {
            XDialogIcon::None => false,
            XDialogIcon::Custom => self.custom_icon.is_some(),
            _ => true,
        }
    }
}

// ------------------------------------------------------------------------------------------------
// What the theme built (theme -> core)
// ------------------------------------------------------------------------------------------------

/// One focusable button as laid out this pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ButtonInfo {
    /// API index (into `view.buttons`).
    pub index: usize,
    /// Widget rect, logical px.
    pub rect: Rect,
}

/// Where the theme put the non-interactive parts (accessibility bounds).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Parts {
    pub heading: Option<Rect>,
    pub body: Option<Rect>,
    pub icon: Option<Rect>,
    pub progress: Option<Rect>,
}

/// Result of one [`Theme::ui`] call.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DialogUiOutput {
    /// Logical client size the content needs. From the measure pass it becomes the initial
    /// window size; in later passes, a change (e.g. `set_text` reflowed the body) makes core
    /// `request_inner_size` (top-left kept). Must be independent of the current window size.
    pub desired_size: Size,
    /// Every button in on-screen order, left to right: the Tab order (Tab moves forward through
    /// this Vec, Shift+Tab backward) and the Left/Right arrow order.
    pub buttons: Vec<ButtonInfo>,
    pub parts: Parts,
}

impl DialogUiOutput {
    /// Record a button laid out this pass (Tab order = call order).
    pub(crate) fn push_button(&mut self, b: &ButtonInteraction) {
        self.buttons.push(ButtonInfo { index: b.index, rect: b.rect });
    }
}

// ------------------------------------------------------------------------------------------------
// Keyboard policy
// ------------------------------------------------------------------------------------------------

/// When [`FrameInfo::focus_visible`] is true. Visible on open in both cases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FocusVisibility {
    /// Whenever something is focused (the theme itself may hide it while hovering).
    Always,
    /// Only after keyboard navigation (":focus-visible", WinUI FocusState.Keyboard). A pointer
    /// press anywhere in the window hides it until the next Tab/arrow key.
    KeyboardOnly,
}

/// Left/Right arrow focus navigation along [`DialogUiOutput::buttons`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArrowNav {
    /// Stop at the ends (WinUI).
    Clamp,
    /// Wrap around (ubuntu).
    Wrap,
}

/// What Space does on the focused button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SpaceKey {
    /// Activate on key press (ubuntu).
    ActivateOnPress,
    /// Press shows the pressed look ([`FrameInfo::key_pressed`]); release activates if focus did
    /// not move meanwhile (WinUI). Focus loss or Escape cancels.
    ActivateOnRelease,
}

/// Keyboard / focus / activation policy. Core implements the state machine; the theme picks.
///
/// Fixed for every theme: the default button (the highest API index laid out) is focused on open,
/// Tab/Shift+Tab move through [`DialogUiOutput::buttons`] wrapping, navigation keys auto-repeat,
/// Enter (not repeat) activates the focused button, Escape (not repeat) closes, Up/Down never
/// navigate. Each theme's choice is its `KEYBOARD` constant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct KeyboardPolicy {
    pub focus_visibility: FocusVisibility,
    pub arrows: ArrowNav,
    /// Enter with nothing focused activates the default button.
    pub enter_falls_back_to_default: bool,
    pub space: SpaceKey,
    /// PageUp/PageDown/Home/End (and Up/Down) produce [`FrameInfo::scroll_request`].
    pub scroll_keys: bool,
}

// ------------------------------------------------------------------------------------------------
// The trait
// ------------------------------------------------------------------------------------------------

/// Implemented by `ubuntu::UbuntuTheme` and `fluent::FluentTheme`. Each theme stores its own
/// colours/metrics ("tokens") for the current appearance, and any per-dialog widget state.
pub(crate) trait Theme {
    /// Recompute the tokens for an appearance. Called on open and on appearance change (core then
    /// snaps every tween).
    fn set_appearance(&mut self, appearance: &Appearance);

    fn keyboard_policy(&self) -> KeyboardPolicy;

    /// The families (first available), weights and metrics of the theme's text.
    fn fonts(&self) -> &'static ThemeFonts;

    /// Side of the dialog icon, logical px (the size core renders [`DialogView::custom_icon`] at).
    fn icon_size(&self) -> f64;

    /// The window background (core clears every frame with it).
    fn clear_color(&self) -> Color;

    /// Build the whole dialog into `ui` (origin top-left of the client area). Buttons MUST use
    /// [`ButtonInteraction::interact`], so core can hit-test and focus them.
    ///
    /// The first call is the **measure pass**: an ordinary pass without input and before
    /// anything has focus, whose drawing core discards; the window is then created at
    /// [`DialogUiOutput::desired_size`]. It must build the same layout and report the same
    /// `desired_size` as a real pass; tweens are reset after it (they snap on the first real
    /// frame).
    fn ui(&mut self, view: &DialogView<'_>, ui: &mut Ui<'_>) -> DialogUiOutput;
}

/// The theme of a drawn backend (`Fluent`, anything else: `Ubuntu`), with default tokens until
/// [`Theme::set_appearance`].
pub(crate) fn new(backend: XDialogBackend) -> Box<dyn Theme> {
    match backend {
        XDialogBackend::Fluent => Box::new(FluentTheme::new()),
        _ => Box::new(UbuntuTheme::new()),
    }
}

// ------------------------------------------------------------------------------------------------
// Button interaction (shared by every theme's button widget)
// ------------------------------------------------------------------------------------------------

/// The id of the button with API index `index` (core's hit testing and focus use it).
pub(crate) fn button_id(index: usize) -> Id {
    Id::new("xdialog.button").with(index)
}

/// Everything a button widget needs to pick its look.
///
/// Mapping hints:
/// - ubuntu: hover = `contains_pointer` (geometric, also while another button is held); pressed
///   look = `pointer_down || key_pressed` (kept while dragged off).
/// - WinUI: hover = `hovered` (no hover on other buttons while one is held, like pointer
///   capture); pressed = `pointer_down && contains_pointer || key_pressed`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ButtonInteraction {
    /// API index of the button.
    pub index: usize,
    pub rect: Rect,
    /// The pointer is inside the rect.
    pub contains_pointer: bool,
    /// Inside, and no other widget is being pressed.
    pub hovered: bool,
    /// A pointer press started on this button and is still held (inside or not).
    pub pointer_down: bool,
    /// Keyboard pressed look ([`FrameInfo::key_pressed`]).
    pub key_pressed: bool,
    /// Has keyboard focus (also while the window is inactive).
    pub focused: bool,
    /// Focused, [`FrameInfo::focus_visible`] and the window is active.
    pub focus_visible: bool,
}

impl ButtonInteraction {
    /// Register button `index` at `rect` for this pass and derive its states.
    pub(crate) fn interact(ui: &mut Ui<'_>, rect: Rect, index: usize, view: &DialogView<'_>) -> Self {
        let it = ui.interact(button_id(index), rect);
        let focused = ui.focused_button() == Some(index);
        ButtonInteraction { index,
                            rect,
                            contains_pointer: it.contains_pointer,
                            hovered: it.hovered,
                            pointer_down: it.pointer_down,
                            key_pressed: view.frame.key_pressed == Some(index),
                            focused,
                            focus_visible: focused && view.frame.focus_visible && ui.window_focused() }
    }
}

/// Shared helpers for theme and core tests.
#[cfg(all(test, draw_soft))]
pub(crate) mod test_support {
    use super::*;
    use crate::backends::draw::{Shape, Text, TextSystem};
    use crate::backends::gui::ui::UiState;

    /// A message dialog view with `buttons`, no heading/body/icon and default frame info.
    pub(crate) fn view(buttons: &[String]) -> DialogView<'_> {
        DialogView { heading: "",
                     body: "",
                     icon: &XDialogIcon::None,
                     custom_icon: None,
                     buttons,
                     progress: None,
                     max_height: 800.0,
                     frame: FrameInfo::default() }
    }

    /// Widget state over this thread's text system.
    pub(crate) fn state() -> UiState {
        UiState::new(Text::shared().expect("text system"))
    }

    /// One pass of `theme` over `view` at t = 0 without input; returns the output and the drawing.
    pub(crate) fn pass(theme: &mut dyn Theme, view: &DialogView<'_>) -> (DialogUiOutput, Vec<Shape>) {
        pass_with(theme, view, &mut state(), 0.0)
    }

    /// One pass with the given input/widget state at dialog time `t`.
    pub(crate) fn pass_with(theme: &mut dyn Theme, view: &DialogView<'_>, st: &mut UiState, t: f64) -> (DialogUiOutput, Vec<Shape>) {
        st.texts.begin_pass(theme.fonts());
        let mut ui = Ui::new(st, t);
        let out = theme.ui(view, &mut ui);
        let shapes = std::mem::take(&mut ui.shapes);
        st.end_pass();
        (out, shapes)
    }
}
