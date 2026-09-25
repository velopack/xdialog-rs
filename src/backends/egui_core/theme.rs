//! The Theme contract, widget-based.
//!
//! A theme builds the whole dialog every frame with normal egui layout and its OWN widgets
//! (`impl egui::Widget`): it paints them and gets hover/press/focus from egui (`Response` + egui
//! focus memory). Two pieces are shared by core:
//!
//! - the **measure pass** (see [`Theme::ui`]): the window is created at the size the theme reports
//!   before it exists (no flicker);
//! - the **keyboard policy** ([`KeyboardPolicy`]): keys never reach egui; core moves egui focus
//!   (`Memory::request_focus(button_id(i))`), activates buttons and closes on Escape according to
//!   the policy the theme picks, and tells widgets what they need through [`FrameInfo`].

use egui::{Color32, Id, Rect, Response, Sense, Vec2};

use super::appearance::Appearance;
pub(crate) use super::fonts::ThemeFonts;
use crate::backends::egui_fluent::FluentTheme;
use crate::backends::egui_ubuntu::UbuntuTheme;
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

/// Progress bar state. Times are dialog-clock seconds (= `ui.input(|i| i.time)`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ProgressView {
    /// `value` in 0..=1 (the latest target). Animate towards it with a widget-local tween
    /// (`anim::animate` with a stable id).
    Determinate { value: f32 },
    /// Indeterminate mode. `since` = time the bar entered indeterminate mode (unchanged by further
    /// `set_indeterminate` calls); `restarted_at` = time of the latest `set_indeterminate` call.
    Indeterminate { since: f64, restarted_at: f64 },
}

/// Per-frame state that does not come from egui (core -> widgets).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct FrameInfo {
    /// Whether the focused widget should show its focus visual right now, per
    /// [`KeyboardPolicy::focus_visibility`]. Widgets combine it with `response.has_focus()`;
    /// [`ButtonInteraction::focus_visible`] does that.
    pub focus_visible: bool,
    /// API index of the button that shows the keyboard-pressed look (Space held on it,
    /// [`SpaceKey::ActivateOnRelease`]).
    pub key_pressed: Option<usize>,
    /// Keyboard scroll request for the body viewport (logical px; positive = reveal content
    /// further down), from PageUp/PageDown/Home/End when [`KeyboardPolicy::scroll_keys`]. A theme
    /// with a body `ScrollArea` applies it; otherwise it ignores it.
    pub scroll_request: f32,
}

/// The dialog content handed to [`Theme::ui`]. All strings are the raw API strings.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DialogView<'a> {
    /// `options.main_instruction` ("" = none).
    pub heading: &'a str,
    /// `options.message`, or the latest `set_text` for progress dialogs ("" = none).
    pub body: &'a str,
    pub icon: &'a XDialogIcon,
    /// The image of [`XDialogIcon::Custom`], [`Theme::icon_size`] logical px square (drawn at
    /// exactly one texel per physical pixel); `None` without a usable icon source.
    pub custom_icon: Option<egui::TextureId>,
    /// Button labels in API order. Every button index in this contract is an index into this slice.
    pub buttons: &'a [String],
    /// `Some` for progress dialogs.
    pub progress: Option<ProgressView>,
    /// Max client height, logical px (primary monitor height * 0.9, or 800 when no monitor is
    /// known). A theme may ignore it or put the body in an `egui::ScrollArea`.
    pub max_height: f32,
    pub frame: FrameInfo,
}

impl DialogView<'_> {
    /// Whether an icon is shown: any severity icon, or `Custom` when its image loaded.
    pub(crate) fn has_icon(&self) -> bool {
        match self.icon {
            XDialogIcon::None => false,
            XDialogIcon::Custom => self.custom_icon.is_some(),
            _ => true,
        }
    }

    /// Paint the custom icon image into `rect` (nothing if there is none).
    pub(crate) fn paint_custom_icon(&self, painter: &egui::Painter, rect: Rect) {
        if let Some(tex) = self.custom_icon {
            painter.image(tex, rect, Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
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

/// Result of one [`Theme::ui`] call.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DialogUiOutput {
    /// Logical client size the content needs. From the measure pass it becomes the initial
    /// window size; in later passes, a change (e.g. `set_text` reflowed the body) makes core
    /// `request_inner_size` (top-left kept). Must be independent of the current window size.
    pub desired_size: Vec2,
    /// Every button in on-screen order, left to right: the Tab order (Tab moves forward through
    /// this Vec, Shift+Tab backward) and the Left/Right arrow order.
    pub buttons: Vec<ButtonInfo>,
    /// A button activated by the POINTER this pass ([`ButtonInteraction::activated`]). Keyboard
    /// activation is decided by core and never reported here.
    pub activated: Option<usize>,
}

impl DialogUiOutput {
    /// Record a button laid out this pass (Tab order = call order) and its pointer activation.
    pub(crate) fn push_button(&mut self, b: &ButtonInteraction) {
        if b.activated && self.activated.is_none() {
            self.activated = Some(b.index);
        }
        self.buttons.push(ButtonInfo { index: b.index, rect: b.response.rect });
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

/// Implemented by `egui_ubuntu::UbuntuTheme` and `egui_fluent::FluentTheme`. Each theme stores
/// its own colours/metrics ("tokens") for the current appearance.
pub(crate) trait Theme {
    /// Recompute the tokens for an appearance. Called on open and on appearance change (core then
    /// calls `anim::reset_animations`).
    fn set_appearance(&mut self, appearance: &Appearance);

    fn keyboard_policy(&self) -> KeyboardPolicy;

    /// Side of the dialog icon, logical px (the size core renders [`DialogView::custom_icon`] at).
    fn icon_size(&self) -> f32;

    /// The theme's regular and bold faces. Core binds `Proportional`/`Monospace` to the regular
    /// face and [`super::fonts::bold_family`] to the bold one, and appends fallback faces.
    fn fonts(&self) -> ThemeFonts;

    /// Adjust egui style (spacing, scroll bars, egui widget visuals if the theme
    /// uses stock widgets such as `ScrollArea`). Called when the tokens change, for both the light
    /// and dark style of the context. `visuals.panel_fill` is the window background (core clears
    /// the buffer with it). Core applies [`core_style_overrides`] AFTER this call.
    fn configure_style(&self, style: &mut egui::Style);

    /// Build the whole dialog into `ui` (the root ui: covers the client rect, origin top-left,
    /// no margin). Use egui layout and the theme's own widgets. Buttons MUST use
    /// [`ButtonInteraction::interact`] (or at least [`button_id`] + `Sense::click()`), so core's
    /// keyboard policy can focus them.
    ///
    /// The first call is the **measure pass**: an ordinary pass over the root ui (a wide screen of
    /// the allowed height) without events and before anything has focus, whose shapes core
    /// discards (its font-atlas upload is kept for the first present); the window is then created
    /// at [`DialogUiOutput::desired_size`]. It must build the same layout and report the same
    /// `desired_size` as a real pass; tweens are reset after it (they snap on the first real
    /// frame).
    fn ui(&self, view: &DialogView<'_>, ui: &mut egui::Ui) -> DialogUiOutput;
}

/// The theme of an egui backend (`Fluent`, anything else: `Ubuntu`), with default tokens until
/// [`Theme::set_appearance`].
pub(crate) fn new(backend: XDialogBackend) -> Box<dyn Theme> {
    match backend {
        XDialogBackend::Fluent => Box::new(FluentTheme::new()),
        _ => Box::new(UbuntuTheme::new()),
    }
}

// ------------------------------------------------------------------------------------------------
// Core-mandated egui settings (applied by core; documented here because widget semantics rely on them)
// ------------------------------------------------------------------------------------------------

/// Style settings core forces on every style after [`Theme::configure_style`]: no default item
/// spacing (themes lay out explicitly), `window_fill = panel_fill`, exact-rect hit testing
/// (`interact_radius = 0`), no selectable labels, instant programmatic scrolling (a keyboard
/// scroll request lands on the same frame).
pub(crate) fn core_style_overrides(style: &mut egui::Style) {
    style.spacing.item_spacing = Vec2::ZERO;
    style.visuals.window_fill = style.visuals.panel_fill;
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
/// - `surrender_focus_on = Never`: clicking empty space keeps focus (ubuntu, WinUI).
/// - no keyboard zoom.
pub(crate) fn core_options(opts: &mut egui::Options) {
    opts.max_passes = std::num::NonZeroUsize::MIN;
    opts.zoom_with_keyboard = false;
    opts.input_options.max_click_dist = f32::INFINITY;
    opts.input_options.max_click_duration = f64::INFINITY;
    opts.input_options.surrender_focus_on = egui::SurrenderFocusOn::Never;
}

/// Apply the theme's style (plus [`core_style_overrides`]) to both styles of `ctx` and
/// select the light or dark one.
pub(crate) fn install_style(ctx: &egui::Context, theme: &dyn Theme, dark: bool) {
    ctx.options_mut(|o| o.theme_preference = if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
    ctx.all_styles_mut(|s| {
           theme.configure_style(s);
           core_style_overrides(s);
       });
}

// ------------------------------------------------------------------------------------------------
// Button interaction helper (shared by every theme's button widget)
// ------------------------------------------------------------------------------------------------

/// The egui id of the button with API index `index`. Core's keyboard policy focuses buttons by
/// this id, so theme button widgets must interact with exactly this id.
pub(crate) fn button_id(index: usize) -> Id {
    Id::new("xdialog.button").with(index)
}

/// A filled rounded rect that is not snapped to whole pixels, so a moving end (progress bars)
/// glides instead of stepping a pixel at a time.
pub(crate) fn unsnapped_rect(rect: Rect, radius: f32, color: Color32) -> egui::epaint::RectShape {
    egui::epaint::RectShape::filled(rect, radius, color).with_round_to_pixels(false)
}

/// Everything a button widget needs to pick its look, from egui + core's keyboard state.
///
/// Mapping hints:
/// - ubuntu: hover = `contains_pointer` (geometric, also while another button is held); pressed
///   look = `pointer_down || key_pressed` (kept while dragged off).
/// - WinUI: hover = `hovered` (egui suppresses hover of other widgets while one is held, like
///   pointer capture); pressed = `pointer_down && contains_pointer || key_pressed`.
#[derive(Clone, Debug)]
pub(crate) struct ButtonInteraction {
    pub response: Response,
    /// API index of the button.
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
    /// Has egui keyboard focus and [`FrameInfo::focus_visible`].
    pub focus_visible: bool,
    /// Released inside after a press on it this pass (pointer activation); recorded by
    /// [`DialogUiOutput::push_button`].
    pub activated: bool,
}

impl ButtonInteraction {
    /// Interact with the (already allocated) button `rect` for API index `index`: senses clicks
    /// with [`button_id`], moves focus to the button on a primary press, and derives the states.
    pub(crate) fn interact(ui: &egui::Ui, rect: Rect, index: usize, view: &DialogView<'_>) -> Self {
        let response = ui.interact(rect, button_id(index), Sense::click());
        let label = view.buttons.get(index).map_or("", String::as_str);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
        let pointer_down = response.is_pointer_button_down_on();
        let contains_pointer = response.contains_pointer();
        let activated = response.clicked() && contains_pointer;
        // Ubuntu/WinUI move focus to a button on press. A press and release delivered in the same
        // pass never shows `is_pointer_button_down_on` (false on the release pass), so a click
        // this pass also moves focus (only primary presses reach egui, see `Dialog::handle_events`).
        let pressed_here = pointer_down && ui.input(|i| i.pointer.primary_pressed());
        if (pressed_here || activated) && !response.has_focus() {
            response.request_focus();
        }
        ButtonInteraction { index,
                            contains_pointer,
                            hovered: response.hovered() && contains_pointer,
                            pointer_down,
                            key_pressed: view.frame.key_pressed == Some(index),
                            focus_visible: response.has_focus() && view.frame.focus_visible,
                            activated,
                            response }
    }
}

/// Shared helpers for theme and core tests.
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

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

    /// A context with core options, the theme's fonts and its (light) style installed.
    pub(crate) fn theme_ctx(theme: &dyn Theme) -> egui::Context {
        let ctx = egui::Context::default();
        ctx.options_mut(core_options);
        ctx.set_fonts(super::super::fonts::font_definitions(&theme.fonts(), &[]));
        install_style(&ctx, theme, false);
        ctx
    }

    /// One pass of `theme` over `view` (textures dropped); `probe` runs inside the pass right after
    /// the theme.
    pub(crate) fn run_ui<R>(ctx: &egui::Context,
                            theme: &dyn Theme,
                            view: &DialogView<'_>,
                            raw: egui::RawInput,
                            probe: impl FnOnce(&egui::Context) -> R)
                            -> (DialogUiOutput, R) {
        let (mut probe, mut out) = (Some(probe), None);
        ctx.run_ui(raw, |ui| out = Some((theme.ui(view, ui), probe.take().expect("one pass")(ui.ctx())))).textures_delta.clear();
        out.expect("one pass")
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::view;
    use super::*;
    use egui::{Event, PointerButton, Pos2};

    /// One pass with a single button at `rect`; returns its interaction.
    fn pass(ctx: &egui::Context, t: f64, events: Vec<Event>) -> ButtonInteraction {
        let buttons = vec!["OK".to_string()];
        let view = view(&buttons);
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
        assert!(st.pointer_down && st.response.has_focus() && st.hovered);
        let st = pass(&ctx, 0.3, vec![Event::PointerMoved(off)]);
        assert!(st.pointer_down && !st.contains_pointer && !st.hovered);
        let st = pass(&ctx, 0.4, vec![button(off, false)]);
        // egui reports the click (press started here) and hovers the clicked widget on the
        // release pass; the helper must not.
        assert!(st.response.clicked() && st.response.hovered());
        assert!(!st.activated && !st.hovered && !st.pointer_down && st.response.has_focus());
    }

    #[test]
    fn long_press_still_clicks() {
        let ctx = ctx();
        let c = rect().center();
        pass(&ctx, 0.0, vec![]);
        pass(&ctx, 0.1, vec![Event::PointerMoved(c), button(c, true)]);
        assert!(pass(&ctx, 5.0, vec![button(c, false)]).activated, "max_click_duration = inf");
    }

    #[test]
    fn press_and_release_in_one_pass_activates_and_focuses() {
        let ctx = ctx();
        let c = rect().center();
        pass(&ctx, 0.0, vec![]);
        let st = pass(&ctx, 0.1, vec![Event::PointerMoved(c), button(c, true), button(c, false)]);
        assert!(st.activated && st.response.has_focus());
        let mut out = DialogUiOutput::default();
        out.push_button(&st);
        assert_eq!((out.activated, out.buttons), (Some(0), vec![ButtonInfo { index: 0, rect: rect() }]));
    }
}
