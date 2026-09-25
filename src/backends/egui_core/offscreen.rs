//! Deterministic window-less dialog for goldens and the gallery.
//!
//! An [`OffscreenDialog`] is the same [`Dialog`] code path real windows use, with:
//! - a [`MemoryPresenter`] instead of a window surface;
//! - a frozen [`DialogClock`] advanced only by [`OffscreenDialog::render_at`];
//! - an injected, fixed appearance (no registry or portal access);
//! - a fixed height limit ([`OFFSCREEN_MAX_HEIGHT`]) instead of the monitor's.
//!
//! Each `render_at(t)` freezes the clock at `t`, applies the queued changes in call order, runs
//! exactly one frame and applies a requested resize after it (the next render has the new size).
//!
//! Everything a frame depends on is a function of the call sequence, so the same script gives the
//! same bytes (the font registry is process-global; system fallback fonts only matter for text the
//! theme's bundled/primary fonts don't cover).

use egui::{Event, Pos2};

use super::appearance::Appearance;
use super::clock::DialogClock;
use super::dialog::{Dialog, DialogContent, DialogParams};
use super::render::MemoryPresenter;
use super::theme::DialogKind;
use crate::model::{XDialogBackend, XDialogOptions, XDialogResult};

/// Max client height (logical px) of every offscreen dialog: a 1080 px high monitor × 0.9, like
/// the runtime derives from a real monitor. Fixed so renders don't depend on the machine.
const OFFSCREEN_MAX_HEIGHT: f32 = 972.0;

/// Progress state to apply.
#[derive(Clone, Debug)]
pub enum TestProgress {
    /// Determinate value in 0..=1.
    Value(f32),
    /// Indeterminate mode.
    Indeterminate,
}

/// Injected appearance (no registry/portal access in offscreen mode).
#[derive(Clone, Debug, Default)]
pub struct TestAppearance {
    /// Dark scheme.
    pub dark: bool,
    /// Windows accent palette `[L3, L2, L1, A, D1, D2, D3]` (RGB).
    pub accent_palette: Option<[[u8; 3]; 7]>,
    /// Accent base colour (RGB).
    pub accent: Option<[u8; 3]>,
}

/// A change queued for the next render.
enum Queued {
    Event(Event),
    Progress(TestProgress),
    Text(String),
}

/// Deterministic window-less dialog. Same `Dialog` code path as real windows.
pub struct OffscreenDialog {
    d: Dialog,
    // `set_progress`/`set_text` are applied with the clock time of the next render, so they are
    // queued alongside the events, in call order.
    queue: Vec<Queued>,
    last_t: f64,
}

impl OffscreenDialog {
    /// Build a dialog of an egui backend (`Fluent` or `Ubuntu`), rendered at `ppp` (in `(0, 8]`).
    /// Runs the measure pass, the keyboard on-open step and the open frame at `t = 0` (scripts
    /// should use `t > 0`).
    pub fn new(backend: XDialogBackend, look: TestAppearance, ppp: f32, kind: DialogKind, options: XDialogOptions) -> Self {
        assert!(matches!(backend, XDialogBackend::Fluent | XDialogBackend::Ubuntu), "{backend:?} is not an egui backend");
        assert!(ppp > 0.0 && ppp <= 8.0, "invalid ppp {ppp}");
        let params = DialogParams { id: 0,
                                    content: DialogContent::new(kind, options),
                                    appearance: Appearance::test(look.dark, look.accent, look.accent_palette),
                                    system_appearance: None,
                                    ppp,
                                    max_height: OFFSCREEN_MAX_HEIGHT,
                                    clock: DialogClock::frozen(0.0),
                                    sender: None };
        let mut d = Dialog::new(super::theme::new(backend), params);
        let size = d.physical_size(ppp);
        d.attach(Box::new(MemoryPresenter::new()), ppp, size);
        // Present the open frame at t = 0, like the runtime does right after creating a window.
        // Without it the first scripted event would land in the first real pass, where every
        // tween is sighted for the first time after the reset and snaps to its target (a hover
        // at t0 would show the settled look at t0 instead of starting the fade).
        d.frame().expect("memory presenter");
        OffscreenDialog { d, queue: Vec::new(), last_t: 0.0 }
    }

    /// Client size in physical px (the size the next render will have).
    pub fn size_px(&self) -> (u32, u32) {
        let [w, h] = self.d.size_px();
        (w, h)
    }

    /// Button rects in LOGICAL px `[x, y, w, h]`, indexed by API button index (from the last
    /// pass; the measure pass right after `new`).
    pub fn button_rects(&self) -> Vec<[f32; 4]> {
        self.d.button_rects()
    }

    /// Centre of button `i` in points (for pointer events), `None` for an unknown index.
    pub fn button_centre(&self, i: usize) -> Option<Pos2> {
        let r = *self.button_rects().get(i)?;
        Some(Pos2::new(r[0] + r[2] / 2.0, r[1] + r[3] / 2.0))
    }

    /// Queue an input event (points); applied at the next render, in call order.
    pub fn event(&mut self, ev: Event) {
        self.queue.push(Queued::Event(ev));
    }

    /// Queue a progress change; applied with the clock time of the next render.
    pub fn set_progress(&mut self, p: TestProgress) {
        self.queue.push(Queued::Progress(p));
    }

    /// Queue a body text change (applied at the next render; the dialog may resize after it).
    pub fn set_text(&mut self, text: &str) {
        self.queue.push(Queued::Text(text.to_owned()));
    }

    /// Advance the dialog clock to `t` seconds and render; returns `(w, h, RGBA8)` of the rendered
    /// size (normally `size_px()` before the call; after the dialog closed, its last frame). Time
    /// is monotonic: an earlier `t` than the previous render is clamped to it.
    pub fn render_at(&mut self, t: f64) -> (u32, u32, Vec<u8>) {
        let t = if t.is_finite() { t.max(self.last_t) } else { self.last_t };
        self.last_t = t;
        self.d.freeze_clock(t);
        for q in std::mem::take(&mut self.queue) {
            match q {
                Queued::Event(ev) => self.d.handle_events([ev]),
                Queued::Progress(TestProgress::Value(v)) => self.d.set_progress_value(v),
                Queued::Progress(TestProgress::Indeterminate) => self.d.set_progress_indeterminate(),
                Queued::Text(s) => self.d.set_text(&s),
            }
        }
        self.d.frame().expect("memory presenter");
        // Apply a size change like a window system: the next frame has the new size.
        if self.d.take_resize().is_some() {
            let size = self.d.physical_size(self.d.ppp());
            self.d.resized(size);
        }
        self.d.presenter().and_then(|p| p.read_rgba()).map_or((0, 0, Vec::new()), |(w, h, px)| (w, h, px.to_vec()))
    }

    /// The dialog's result, once it closed.
    pub fn result(&self) -> Option<XDialogResult> {
        self.d.result().cloned()
    }
}
