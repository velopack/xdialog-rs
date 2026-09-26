//! Input events in logical px (from winit, or injected by the offscreen harness / host test
//! hooks), and the translation of winit window events into them.

use crate::backends::draw::{Point, Vec2};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::keyboard::{Key as WinitKey, NamedKey};

/// Keys the dialogs use.
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Tab,
    Enter,
    Escape,
    Space,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    PageUp,
    PageDown,
}

/// A mouse button.
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
}

/// One input event, positions in logical px (client-relative).
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The pointer moved (inside the window).
    PointerMoved(Point),
    /// A mouse button was pressed or released at `pos`.
    PointerButton {
        /// Where.
        pos: Point,
        /// Which button.
        button: PointerButton,
        /// Pressed (`true`) or released.
        pressed: bool,
    },
    /// The pointer left the window.
    PointerGone,
    /// Wheel / touchpad scroll in logical px (positive `y`: towards the top of the content).
    Scroll(Vec2),
    /// A key was pressed or released.
    Key {
        /// Which key.
        key: Key,
        /// Pressed (`true`) or released.
        pressed: bool,
        /// An OS auto-repeat of a held key.
        repeat: bool,
        /// Shift was held.
        shift: bool,
    },
    /// The window gained or lost keyboard focus.
    WindowFocused(bool),
}

/// Where a release lands when the pointer is outside the window: far from every widget.
const OUTSIDE: Point = Point::new(-1.0e6, -1.0e6);
/// Logical px per wheel line.
const LINE: f64 = 40.0;

/// Per-window translation state (pointer position, Shift, the touch acting as the pointer).
#[derive(Default)]
pub(crate) struct WinitInput {
    pointer: Option<Point>,
    shift: bool,
    touch: Option<u64>,
}

impl WinitInput {
    /// Translate one winit event at `scale` (physical px per logical px).
    pub(crate) fn translate(&mut self, ev: &WindowEvent, scale: f64) -> Vec<Event> {
        let logical = |x: f64, y: f64| Point::new(x / scale, y / scale);
        match ev {
            WindowEvent::CursorMoved { position, .. } => {
                let p = logical(position.x, position.y);
                self.pointer = Some(p);
                vec![Event::PointerMoved(p)]
            }
            WindowEvent::CursorLeft { .. } => {
                self.pointer = None;
                vec![Event::PointerGone]
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    MouseButton::Left => PointerButton::Primary,
                    MouseButton::Right => PointerButton::Secondary,
                    MouseButton::Middle => PointerButton::Middle,
                    _ => return vec![],
                };
                vec![Event::PointerButton { pos: self.pointer.unwrap_or(OUTSIDE), button, pressed: *state == ElementState::Pressed }]
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let d = match delta {
                    MouseScrollDelta::LineDelta(x, y) => Vec2::new(*x as f64 * LINE, *y as f64 * LINE),
                    MouseScrollDelta::PixelDelta(p) => Vec2::new(p.x / scale, p.y / scale),
                };
                vec![Event::Scroll(d)]
            }
            WindowEvent::ModifiersChanged(m) => {
                self.shift = m.state().shift_key();
                vec![]
            }
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
                let WinitKey::Named(named) = &event.logical_key else { return vec![] };
                let key = match named {
                    NamedKey::Tab => Key::Tab,
                    NamedKey::Enter => Key::Enter,
                    NamedKey::Escape => Key::Escape,
                    NamedKey::Space => Key::Space,
                    NamedKey::ArrowLeft => Key::ArrowLeft,
                    NamedKey::ArrowRight => Key::ArrowRight,
                    NamedKey::ArrowUp => Key::ArrowUp,
                    NamedKey::ArrowDown => Key::ArrowDown,
                    NamedKey::Home => Key::Home,
                    NamedKey::End => Key::End,
                    NamedKey::PageUp => Key::PageUp,
                    NamedKey::PageDown => Key::PageDown,
                    _ => return vec![],
                };
                vec![Event::Key { key, pressed: event.state == ElementState::Pressed, repeat: event.repeat, shift: self.shift }]
            }
            WindowEvent::Focused(f) => vec![Event::WindowFocused(*f)],
            // The first finger acts as the primary pointer.
            WindowEvent::Touch(t) => {
                let p = logical(t.location.x, t.location.y);
                match t.phase {
                    TouchPhase::Started if self.touch.is_none() => {
                        self.touch = Some(t.id);
                        self.pointer = Some(p);
                        vec![Event::PointerMoved(p), Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true }]
                    }
                    TouchPhase::Moved if self.touch == Some(t.id) => {
                        self.pointer = Some(p);
                        vec![Event::PointerMoved(p)]
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled if self.touch == Some(t.id) => {
                        self.touch = None;
                        self.pointer = None;
                        let pos = if t.phase == TouchPhase::Ended { p } else { OUTSIDE };
                        vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: false }, Event::PointerGone]
                    }
                    _ => vec![],
                }
            }
            _ => vec![],
        }
    }
}
