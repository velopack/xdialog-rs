//! Translation of the version-neutral [`HostEvent`]s into core input and egui events. One place
//! used by every mode: the own winit loop (after its winit -> `HostEvent`
//! translation), host mode and the test injection hooks.
//!
//! Widget-based themes: pointer and wheel events go to egui, which does all hit
//! testing, hover, press and click detection for the theme widgets. Keys never go to egui (so its
//! built-in Tab/arrow/Escape focus navigation and Enter/Space fake clicks never run); they drive
//! core's keyboard policy (`keyboard.rs`). Core still sees pointer presses (focus-visibility
//! modality) and focus changes.

use egui::{Pos2, Vec2};

use crate::backends::host_types::{HostEvent, Key, Modifiers, MouseButton, ScrollDelta, TouchPhase};

/// Input as consumed by the dialog state machine. Positions are logical px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CoreInput {
    PointerMoved(Pos2),
    /// The pointer left the window (or a touch ended): no hover, pointer capture ends.
    PointerLeft,
    /// A button press/release at the current pointer position.
    PointerButton { button: MouseButton, pressed: bool },
    /// The primary pointer interaction was cancelled (touch `Cancelled`): no click.
    PointerCancel,
    Key { key: Key, pressed: bool, repeat: bool },
    Modifiers(Modifiers),
    Focused(bool),
    /// New client size in physical px (a zero side = minimised).
    Resized([u32; 2]),
    ScaleFactor(f32),
    ThemeChanged,
    CloseRequested,
}

/// Per-window translation state.
#[derive(Clone, Debug, Default)]
pub(crate) struct InputTranslator {
    /// Last pointer position (logical px), `None` when outside the window.
    pointer: Option<Pos2>,
    modifiers: Modifiers,
    /// The primary touch contact currently mapped to the pointer.
    touch: Option<u64>,
    /// egui believes the primary button is held (a press was forwarded, no release yet).
    egui_primary_down: bool,
}

/// Where a cancelled press is released: far outside every widget, so the release can't activate
/// anything (`ButtonInteraction::activated` requires the pointer inside the button).
const CANCEL_POS: Pos2 = Pos2::new(-1.0e6, -1.0e6);

impl InputTranslator {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Current pointer position in logical px.
    #[cfg(test)]
    pub(crate) fn pointer(&self) -> Option<Pos2> {
        self.pointer
    }

    /// Translate one event. `ppp` = physical px per logical px (host coordinates are physical).
    pub(crate) fn translate(&mut self, ev: HostEvent, ppp: f32, core: &mut Vec<CoreInput>, egui_out: &mut Vec<egui::Event>) {
        let ppp = if ppp > 0.0 { ppp } else { 1.0 };
        let to_logical = |x: f64, y: f64| Pos2::new((x / ppp as f64) as f32, (y / ppp as f64) as f32);
        match ev {
            HostEvent::Resized { width, height } => core.push(CoreInput::Resized([width, height])),
            HostEvent::ScaleFactorChanged { scale_factor } => {
                if scale_factor.is_finite() && scale_factor > 0.0 {
                    core.push(CoreInput::ScaleFactor(scale_factor as f32));
                }
            }
            HostEvent::CursorMoved { x, y } => {
                if self.touch.is_some() {
                    return; // the touch contact owns the pointer
                }
                self.move_to(to_logical(x, y), core, egui_out);
            }
            HostEvent::CursorLeft => {
                if self.touch.is_some() {
                    return;
                }
                self.leave(core, egui_out);
            }
            HostEvent::MouseButton { button, pressed } => {
                if self.touch.is_some() {
                    return;
                }
                self.button(button, pressed, core, egui_out);
            }
            HostEvent::MouseWheel { delta } => {
                let (unit, d) = match delta {
                    ScrollDelta::Lines { x, y } => (egui::MouseWheelUnit::Line, Vec2::new(x, y)),
                    ScrollDelta::Pixels { x, y } => (egui::MouseWheelUnit::Point, Vec2::new((x / ppp as f64) as f32, (y / ppp as f64) as f32)),
                };
                if d != Vec2::ZERO && d.is_finite() {
                    egui_out.push(egui::Event::MouseWheel { unit, delta: d, phase: egui::TouchPhase::Move, modifiers: egui_modifiers(self.modifiers) });
                }
            }
            HostEvent::Touch { id, phase, x, y } => {
                let pos = to_logical(x, y);
                match phase {
                    TouchPhase::Started => {
                        if self.touch.is_none() {
                            self.touch = Some(id);
                            self.move_to(pos, core, egui_out);
                            self.button(MouseButton::Primary, true, core, egui_out);
                        }
                    }
                    TouchPhase::Moved => {
                        if self.touch == Some(id) {
                            self.move_to(pos, core, egui_out);
                        }
                    }
                    TouchPhase::Ended => {
                        if self.touch == Some(id) {
                            self.move_to(pos, core, egui_out);
                            self.button(MouseButton::Primary, false, core, egui_out);
                            self.touch = None;
                            self.leave(core, egui_out);
                        }
                    }
                    TouchPhase::Cancelled => {
                        if self.touch == Some(id) {
                            self.touch = None;
                            self.cancel_egui_press(egui_out);
                            core.push(CoreInput::PointerCancel);
                            self.leave(core, egui_out);
                        }
                    }
                }
            }
            HostEvent::Key { key, pressed, repeat } => core.push(CoreInput::Key { key, pressed, repeat }),
            HostEvent::Modifiers(m) => {
                self.modifiers = m;
                core.push(CoreInput::Modifiers(m));
            }
            HostEvent::Focused(f) => {
                if !f {
                    // Hosts filter synthetic key releases on focus loss: drop every press/capture.
                    // egui never clears held pointer buttons by itself and the release may never
                    // arrive (capture lost): that would leave a pressed look and a stale
                    // `potential_click_id` (the next press on another button would not click).
                    self.cancel_egui_press(egui_out);
                    if self.touch.take().is_some() {
                        self.leave(core, egui_out);
                    }
                    core.push(CoreInput::PointerCancel);
                }
                egui_out.push(egui::Event::WindowFocused(f));
                core.push(CoreInput::Focused(f));
            }
            HostEvent::ThemeChanged => core.push(CoreInput::ThemeChanged),
            HostEvent::CloseRequested => core.push(CoreInput::CloseRequested),
        }
    }

    fn move_to(&mut self, pos: Pos2, core: &mut Vec<CoreInput>, egui_out: &mut Vec<egui::Event>) {
        self.pointer = Some(pos);
        egui_out.push(egui::Event::PointerMoved(pos));
        core.push(CoreInput::PointerMoved(pos));
    }

    fn leave(&mut self, core: &mut Vec<CoreInput>, egui_out: &mut Vec<egui::Event>) {
        self.pointer = None;
        egui_out.push(egui::Event::PointerGone);
        core.push(CoreInput::PointerLeft);
    }

    /// Only the primary button reaches egui (skia and WinUI buttons ignore the others). In egui a
    /// secondary press would start a press on the widget under it (`is_pointer_button_down_on`,
    /// pressed look) and block primary presses until released. Core still sees every button.
    fn button(&mut self, button: MouseButton, pressed: bool, core: &mut Vec<CoreInput>, egui_out: &mut Vec<egui::Event>) {
        if button == MouseButton::Primary {
            let modifiers = egui_modifiers(self.modifiers);
            if pressed {
                if let Some(pos) = self.pointer {
                    egui_out.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers });
                    self.egui_primary_down = true;
                }
            } else if self.egui_primary_down {
                // A release after the pointer left the window happens outside every widget.
                self.egui_primary_down = false;
                let pos = self.pointer.unwrap_or(CANCEL_POS);
                egui_out.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers });
                if self.pointer.is_none() {
                    egui_out.push(egui::Event::PointerGone);
                }
            }
        }
        core.push(CoreInput::PointerButton { button, pressed });
    }

    /// Release a primary press egui still holds far away from every widget (so it can't activate
    /// anything), then report the pointer gone for this pass; the next move restores hover.
    fn cancel_egui_press(&mut self, egui_out: &mut Vec<egui::Event>) {
        if self.egui_primary_down {
            self.egui_primary_down = false;
            egui_out.push(egui::Event::PointerButton { pos: CANCEL_POS,
                                                       button: egui::PointerButton::Primary,
                                                       pressed: false,
                                                       modifiers: egui_modifiers(self.modifiers) });
            egui_out.push(egui::Event::PointerGone);
        }
    }
}

/// egui modifier state from ours.
pub(crate) fn egui_modifiers(m: Modifiers) -> egui::Modifiers {
    egui::Modifiers { alt: m.alt(), ctrl: m.ctrl(), shift: m.shift(), mac_cmd: false, command: m.ctrl() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(t: &mut InputTranslator, ev: HostEvent, ppp: f32) -> (Vec<CoreInput>, Vec<egui::Event>) {
        let (mut c, mut e) = (Vec::new(), Vec::new());
        t.translate(ev, ppp, &mut c, &mut e);
        (c, e)
    }

    #[test]
    fn cursor_is_logical_and_leave_clears() {
        let mut t = InputTranslator::new();
        let (c, e) = run(&mut t, HostEvent::CursorMoved { x: 30.0, y: 15.0 }, 1.5);
        assert_eq!(c, vec![CoreInput::PointerMoved(Pos2::new(20.0, 10.0))]);
        assert_eq!(e, vec![egui::Event::PointerMoved(Pos2::new(20.0, 10.0))]);
        let (c, e) = run(&mut t, HostEvent::CursorLeft, 1.5);
        assert_eq!(c, vec![CoreInput::PointerLeft]);
        assert_eq!(e, vec![egui::Event::PointerGone]);
        assert_eq!(t.pointer(), None);
    }

    #[test]
    fn primary_touch_maps_to_pointer() {
        let mut t = InputTranslator::new();
        let (c, _) = run(&mut t, HostEvent::Touch { id: 7, phase: TouchPhase::Started, x: 10.0, y: 10.0 }, 1.0);
        assert_eq!(c, vec![CoreInput::PointerMoved(Pos2::new(10.0, 10.0)), CoreInput::PointerButton { button: MouseButton::Primary, pressed: true }]);
        // A second finger is ignored.
        let (c, _) = run(&mut t, HostEvent::Touch { id: 8, phase: TouchPhase::Started, x: 50.0, y: 50.0 }, 1.0);
        assert!(c.is_empty());
        // Mouse events are ignored while the touch owns the pointer.
        let (c, _) = run(&mut t, HostEvent::CursorMoved { x: 1.0, y: 1.0 }, 1.0);
        assert!(c.is_empty());
        let (c, e) = run(&mut t, HostEvent::Touch { id: 7, phase: TouchPhase::Ended, x: 12.0, y: 10.0 }, 1.0);
        assert_eq!(c.last(), Some(&CoreInput::PointerLeft));
        assert!(c.contains(&CoreInput::PointerButton { button: MouseButton::Primary, pressed: false }));
        assert_eq!(e.last(), Some(&egui::Event::PointerGone));
        // Cancel path.
        run(&mut t, HostEvent::Touch { id: 9, phase: TouchPhase::Started, x: 1.0, y: 1.0 }, 1.0);
        let (c, _) = run(&mut t, HostEvent::Touch { id: 9, phase: TouchPhase::Cancelled, x: 1.0, y: 1.0 }, 1.0);
        assert_eq!(c, vec![CoreInput::PointerCancel, CoreInput::PointerLeft]);
    }

    #[test]
    fn only_primary_reaches_egui_and_cancel_releases_outside() {
        let mut t = InputTranslator::new();
        run(&mut t, HostEvent::CursorMoved { x: 10.0, y: 10.0 }, 1.0);
        let (c, e) = run(&mut t, HostEvent::MouseButton { button: MouseButton::Secondary, pressed: true }, 1.0);
        assert_eq!(c, vec![CoreInput::PointerButton { button: MouseButton::Secondary, pressed: true }]);
        assert!(e.is_empty());
        let (_, e) = run(&mut t, HostEvent::MouseButton { button: MouseButton::Primary, pressed: true }, 1.0);
        assert!(matches!(e[..], [egui::Event::PointerButton { pressed: true, button: egui::PointerButton::Primary, .. }]));
        // Focus loss releases the held press far outside, then the pointer is gone.
        let (_, e) = run(&mut t, HostEvent::Focused(false), 1.0);
        assert!(matches!(e[..], [egui::Event::PointerButton { pressed: false, pos, .. }, egui::Event::PointerGone, egui::Event::WindowFocused(false)] if pos == CANCEL_POS));
        // The real release that may follow is not forwarded twice.
        let (_, e) = run(&mut t, HostEvent::MouseButton { button: MouseButton::Primary, pressed: false }, 1.0);
        assert!(e.is_empty());
        // Press, leave the window, release outside: released at CANCEL_POS.
        run(&mut t, HostEvent::CursorMoved { x: 10.0, y: 10.0 }, 1.0);
        run(&mut t, HostEvent::MouseButton { button: MouseButton::Primary, pressed: true }, 1.0);
        run(&mut t, HostEvent::CursorLeft, 1.0);
        let (_, e) = run(&mut t, HostEvent::MouseButton { button: MouseButton::Primary, pressed: false }, 1.0);
        assert!(matches!(e[..], [egui::Event::PointerButton { pressed: false, pos, .. }, egui::Event::PointerGone] if pos == CANCEL_POS));
    }

    #[test]
    fn wheel_goes_to_egui_and_focus_loss_cancels() {
        let mut t = InputTranslator::new();
        let (c, e) = run(&mut t, HostEvent::MouseWheel { delta: ScrollDelta::Lines { x: 0.0, y: -1.0 } }, 2.0);
        assert!(c.is_empty());
        assert!(matches!(e[..], [egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta, .. }] if delta == Vec2::new(0.0, -1.0)));
        let (_, e) = run(&mut t, HostEvent::MouseWheel { delta: ScrollDelta::Pixels { x: 0.0, y: 20.0 } }, 2.0);
        assert!(matches!(e[..], [egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta, .. }] if delta == Vec2::new(0.0, 10.0)));
        let (c, e) = run(&mut t, HostEvent::Focused(false), 1.0);
        assert_eq!(c, vec![CoreInput::PointerCancel, CoreInput::Focused(false)]);
        assert_eq!(e, vec![egui::Event::WindowFocused(false)]);
    }
}
