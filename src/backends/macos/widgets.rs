//! macOS theme widgets: the push button (default / standard), the progress bar
//! (NSProgressIndicator), the fallback severity icons and the overlay scroller.

use std::rc::Rc;

use super::tokens::MacTokens;
use crate::backends::draw::{Color, LineCap, Point, Rect, Size, Vec2};
use crate::backends::gui::anim::{Easing, Transition};
use crate::backends::gui::text::TextBlock;
use crate::backends::gui::theme::{button_id, ButtonInteraction, DialogView, ProgressView};
use crate::backends::gui::ui::{caps_centered, indeterminate_capsule, Id, Ui, CAPSULE_CYCLE, CAPSULE_STRETCH};
use crate::model::XDialogIcon;

/// Push button height (both styles) and Sequoia's corner radius (Tahoe's buttons are capsules).
pub(crate) const BUTTON_H: f64 = 28.0;
pub(crate) const CORNER: f64 = 6.0;
/// Fill change on pointer-over / pointer-out (pressing is instant).
const HOVER_FADE: Transition = Transition::linear(0.12);
/// Focus ring: width and gap outside the button.
const RING_W: f64 = 3.0;
const RING_GAP: f64 = 0.5;
/// Progress bar thickness (a capsule).
pub(crate) const PROGRESS_H: f64 = 6.0;
/// Determinate value changes.
const PROGRESS_VALUE: Transition = Transition::new(0.2, Easing::CubicBezier(0.25, 0.1, 0.25, 1.0));

// ------------------------------------------------------------------------------------------------
// Push button
// ------------------------------------------------------------------------------------------------

/// A push button in `rect` with the style's corners (`tk.button_radius`): the default button (accent gradient, flat on
/// Tahoe; white label) or a standard one (translucent fill). Under the pointer the fill fades to its hover shade (no hover while another
/// button is held); the pressed look shows at once while the pointer is held inside, or Space is
/// held. The focus ring shows only with keyboard focus visibility.
pub(crate) fn button(ui: &mut Ui<'_>,
                     rect: Rect,
                     index: usize,
                     label: &Rc<TextBlock>,
                     default: bool,
                     view: &DialogView<'_>,
                     tk: &MacTokens)
                     -> ButtonInteraction {
    let st = ButtonInteraction::interact(ui, rect, index, view);
    let radius = tk.button_radius;
    let pressed = st.pointer_down && st.contains_pointer || st.key_pressed;
    let fade = if pressed { Transition::INSTANT } else { HOVER_FADE };
    let id = button_id(index).with("macos.fill");
    let text = if default {
        let (top, bottom) = if pressed {
            (tk.default_pressed_top, tk.default_pressed_bottom)
        } else if st.hovered {
            (tk.default_hover_top, tk.default_hover_bottom)
        } else {
            (tk.default_top, tk.default_bottom)
        };
        let top = ui.animate(id.with("top"), top, fade);
        let bottom = ui.animate(id.with("bottom"), bottom, fade);
        if top == bottom {
            ui.fill_rect(rect, radius, top);
        } else {
            ui.fill_rect_gradient(rect, radius, top, bottom);
        }
        tk.default_text
    } else {
        let fill = if pressed {
            tk.button_pressed
        } else if st.hovered {
            tk.button_hover
        } else {
            tk.button
        };
        let fill = ui.animate(id, fill, fade);
        ui.fill_rect(rect, radius, fill);
        tk.text
    };
    if st.focus_visible {
        let d = RING_GAP + RING_W / 2.0;
        ui.stroke_rect(rect.inflate(d, d), radius + d, RING_W, tk.focus_ring);
    }
    ui.text(label, caps_centered(rect, label), text);
    st
}

// ------------------------------------------------------------------------------------------------
// Progress bar
// ------------------------------------------------------------------------------------------------

/// NSProgressIndicator (bar style) in `r` (`PROGRESS_H` tall): a capsule track with the accent
/// capsule growing from the left; indeterminate, the shared "stretchy capsule" sweeping side to
/// side (`ui::indeterminate_capsule`, the Ubuntu theme's motion) in the accent.
pub(crate) fn progress(ui: &mut Ui<'_>, r: Rect, progress: ProgressView, tk: &MacTokens) {
    let id = Id::new("macos.progress");
    let radius = r.height() / 2.0;
    ui.fill_rect(r, radius, tk.track);
    match progress {
        ProgressView::Determinate { value } => {
            let v = ui.animate(id, value.clamp(0.0, 1.0), PROGRESS_VALUE) as f64;
            if v > 0.0 {
                // Never narrower than the capsule's height, so the ends stay round.
                let w = (r.width() * v).max(r.height()).min(r.width());
                ui.fill_rect_unsnapped(Rect::from_origin_size(r.origin(), Size::new(w, r.height())), radius, tk.progress);
            }
        }
        ProgressView::Indeterminate { restarted_at, .. } => {
            ui.animate(id, 0.0f32, Transition::INSTANT);
            ui.request_smooth_frame();
            let elapsed = (ui.time() - restarted_at).max(0.0);
            if let Some(c) = indeterminate_capsule(r, elapsed, CAPSULE_CYCLE, CAPSULE_STRETCH) {
                ui.fill_rect_unsnapped(c, radius, tk.progress);
            }
        }
    }
}

// ------------------------------------------------------------------------------------------------
// Fallback severity icons
// ------------------------------------------------------------------------------------------------

/// A severity icon drawn in the `size` box at `origin`, for when the system's alert icons are
/// unavailable (any platform but macOS): the caution triangle (warning), a stop octagon (error),
/// a note disc (information). Shapes follow the macOS icon grid (content about 13/16 of the box).
pub(crate) fn icon(ui: &mut Ui<'_>, origin: Point, size: f64, icon: &XDialogIcon, tk: &MacTokens) {
    let k = size / 64.0;
    let p = |x: f64, y: f64| origin + Vec2::new(x * k, y * k);
    let bang = |ui: &mut Ui<'_>, cx: f64, top: f64, bottom: f64, dot: f64, w: f64, color: Color| {
        ui.line(p(cx, top), p(cx, bottom), w * k, LineCap::Round, color);
        ui.circle(p(cx, dot), w * 0.62 * k, color);
    };
    match icon {
        XDialogIcon::Warning => {
            // A triangle with round corners: the inner triangle plus round-capped edges.
            let pts = [p(32.0, 10.5), p(56.0, 52.0), p(8.0, 52.0)];
            rounded_polygon(ui, &pts, 5.0 * k, tk.caution_edge, 0.75 * k);
            rounded_polygon(ui, &pts, 5.0 * k, tk.caution_bottom, 0.0);
            let inner = [p(32.0, 13.0), p(53.5, 50.5), p(10.5, 50.5)];
            rounded_polygon(ui, &inner, 3.5 * k, tk.caution_top, 0.0);
            bang(ui, 32.0, 25.0, 38.5, 46.0, 5.5, tk.caution_glyph);
        }
        XDialogIcon::Error => {
            let c = p(32.0, 32.0);
            let octagon = |r: f64| -> Vec<Point> {
                (0..8).map(|i| {
                          let a = std::f64::consts::PI / 8.0 * (2 * i + 1) as f64;
                          c + Vec2::new(a.cos() * r, a.sin() * r)
                      })
                      .collect()
            };
            rounded_polygon(ui, &octagon(23.0 * k), 3.0 * k, tk.stop_edge, 0.75 * k);
            rounded_polygon(ui, &octagon(23.0 * k), 3.0 * k, tk.stop_bottom, 0.0);
            rounded_polygon(ui, &octagon(21.0 * k), 2.5 * k, tk.stop_top, 0.0);
            bang(ui, 32.0, 19.0, 35.0, 44.0, 6.0, tk.icon_glyph);
        }
        XDialogIcon::Information => {
            let c = p(32.0, 32.0);
            ui.circle(c, 26.75 * k, tk.note_edge);
            ui.circle(c, 26.0 * k, tk.note_bottom);
            ui.circle(c - Vec2::new(0.0, 1.0 * k), 24.5 * k, tk.note_top.lerp_to_gamma(tk.note_bottom, 0.45));
            ui.circle(p(32.0, 19.5), 3.6 * k, tk.icon_glyph);
            ui.line(p(32.0, 29.0), p(32.0, 45.0), 6.0 * k, LineCap::Round, tk.icon_glyph);
        }
        XDialogIcon::None | XDialogIcon::Custom => {}
    }
}

/// The polygon `pts` (convex) with its corners rounded by `r`, grown by `grow` (an edge under
/// it). `color` must be opaque (the strokes overlap).
fn rounded_polygon(ui: &mut Ui<'_>, pts: &[Point], r: f64, color: Color, grow: f64) {
    // Fill `pts` inset by `r`, then stroke the inset edges `2 r` wide with round caps: a Minkowski
    // sum with a disc of radius `r`, i.e. the same outline with round corners.
    let n = pts.len();
    let inset: Vec<Point> = (0..n).map(|i| inset_vertex(pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n], r)).collect();
    ui.polygon(&inset, color);
    let w = 2.0 * (r + grow);
    for i in 0..n {
        ui.line(inset[i], inset[(i + 1) % n], w, LineCap::Round, color);
    }
}

/// Vertex `at` of a convex polygon (neighbours `prev`, `next`) moved inwards so both its edges
/// move `r` inwards: `r / sin(half angle)` along the corner's bisector.
fn inset_vertex(prev: Point, at: Point, next: Point, r: f64) -> Point {
    let unit = |v: Vec2| {
        let len = (v.x * v.x + v.y * v.y).sqrt().max(1e-9);
        Vec2::new(v.x / len, v.y / len)
    };
    let (a, b) = (unit(prev - at), unit(next - at));
    let bisector = unit(Vec2::new(a.x + b.x, a.y + b.y));
    let cos = (a.x * b.x + a.y * b.y).clamp(-1.0, 1.0);
    let sin_half = ((1.0 - cos) / 2.0).sqrt().max(0.1);
    at + bisector * (r / sin_half)
}

// ------------------------------------------------------------------------------------------------
// Overlay scroller
// ------------------------------------------------------------------------------------------------

/// Knob width at rest and while the pointer is over the scroller or dragging it; its inset from
/// the viewport's right edge; shortest knob.
const KNOB_THIN: f64 = 6.0;
const KNOB_WIDE: f64 = 9.0;
const KNOB_INSET: f64 = 2.0;
const MIN_KNOB: f64 = 18.0;

/// The overlay scroller at the right edge of `viewport` for content `content_h` tall at
/// `offset`: a rounded knob that widens under the pointer. Dragging it scrolls; returns the new
/// offset.
pub(crate) fn scroll_bar(ui: &mut Ui<'_>, viewport: Rect, content_h: f64, offset: f64, tk: &MacTokens) -> f64 {
    let (view_h, max) = (viewport.height(), (content_h - viewport.height()).max(0.0));
    if max <= 0.0 {
        return 0.0;
    }
    let x1 = viewport.x1 - KNOB_INSET;
    let it = ui.interact(Id::new("macos.scroller"), Rect::new(x1 - KNOB_WIDE, viewport.y0, x1, viewport.y1));
    let knob_h = (view_h * view_h / content_h).max(MIN_KNOB).min(view_h - 2.0 * KNOB_INSET);
    let travel = view_h - 2.0 * KNOB_INSET - knob_h;
    let mut offset = offset;
    if it.pointer_down && travel > 0.0 {
        offset = (offset + ui.pointer_delta().y * max / travel).clamp(0.0, max);
    }
    let w = ui.animate(Id::new("macos.scroller.width"),
                       if it.hovered || it.pointer_down { KNOB_WIDE as f32 } else { KNOB_THIN as f32 },
                       Transition::linear(0.12)) as f64;
    let y = viewport.y0 + KNOB_INSET + offset / max * travel;
    ui.fill_rect_unsnapped(Rect::from_origin_size(Point::new(x1 - w, y), Size::new(w, knob_h)), w / 2.0, tk.scroll_thumb);
    offset
}
