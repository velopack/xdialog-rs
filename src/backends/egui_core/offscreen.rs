//! Deterministic window-less dialog for goldens, the gallery and benches.
//!
//! An [`OffscreenDialog`] is the same [`Dialog`] code path real windows use, with:
//! - a [`MemoryPresenter`] instead of a window surface;
//! - a frozen [`DialogClock`] advanced only by [`OffscreenDialog::render_at`];
//! - an injected, fixed appearance ([`AppearanceSource::Fixed`], no registry or portal access);
//! - fixed size limits ([`OFFSCREEN_LIMITS`]) instead of the monitor's.
//!
//! `new` runs the measure pass, the keyboard on-open step and the tween reset (all inside
//! `Dialog::new`), attaches the presenter at the measured size and presents the open frame at
//! `t = 0` (exactly like a real window, which presents right after creation). Each
//! `render_at(t)` freezes the clock at `t`, applies the queued changes in call order (pointer and
//! wheel events are queued for egui, keys run the keyboard policy, progress/text are stamped with
//! `t`), runs exactly one egui pass and presents it. A resize the content asked for is applied
//! after the frame, like a window system would (the next render has the new size).
//!
//! Everything a frame depends on is a function of the call sequence, so the same script gives the
//! same bytes (the font registry is process-global; system fallback fonts only matter for text the
//! theme's bundled/primary fonts don't cover).

use std::sync::atomic::{AtomicUsize, Ordering};

use super::appearance::Appearance;
use super::clock::DialogClock;
use super::dialog::{AppearanceSource, Dialog, DialogContent, DialogParams, ResultSink};
use super::render::MemoryPresenter;
use super::testhooks::api::{rect_centre, HostEvent, TestAppearance, TestKind, TestProgress};
use super::theme::{DialogKind, SizeLimits, Theme};
use crate::model::{XDialogOptions, XDialogResult};

/// Size limits (logical px) of every offscreen dialog: a 1080 px high monitor × 0.9, like the own
/// loop derives from a real monitor. Fixed so renders don't depend on the machine.
pub(crate) const OFFSCREEN_LIMITS: SizeLimits = SizeLimits { max_height: 972.0 };

/// Offscreen dialog ids start high so they never collide with request ids in logs.
static NEXT_ID: AtomicUsize = AtomicUsize::new(1 << 30);

/// A change queued for the next render.
enum Queued {
    Event(HostEvent),
    Progress(TestProgress),
    Text(String),
}

/// Object-safe view of a `Dialog<T>` plus its theme.
trait OffscreenDyn {
    fn dialog_size_px(&self) -> [u32; 2];
    fn button_rects(&self) -> Vec<[f32; 4]>;
    fn freeze(&mut self, t: f64);
    fn apply(&mut self, q: Queued);
    fn render(&mut self);
    fn rgba(&self) -> Option<(u32, u32, &[u8])>;
    fn result(&self) -> Option<XDialogResult>;
}

struct Inner<T: Theme> {
    theme: T,
    d: Dialog<T>,
}

impl<T: Theme> Inner<T> {
    fn new(theme: T, appearance: Appearance, ppp: f32, kind: DialogKind, options: XDialogOptions) -> Self {
        let params = DialogParams { id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
                                    content: DialogContent::new(kind, options),
                                    appearance,
                                    appearance_source: AppearanceSource::Fixed,
                                    ppp,
                                    limits: OFFSCREEN_LIMITS,
                                    clock: DialogClock::frozen(0.0),
                                    sink: ResultSink::none() };
        let mut d = Dialog::new(&theme, params);
        let size = d.physical_size(ppp);
        d.attach(Box::new(MemoryPresenter::new()), ppp, size);
        // `attach` compares against the same size: no resize is pending.
        let _ = d.take_resize();
        // Present the open frame at t = 0, like the manager does right after creating a window.
        // Without it the first scripted event would land in the first real pass, where every
        // tween is sighted for the first time after the reset and snaps to its target (a hover
        // at t0 would show the settled look at t0 instead of starting the fade).
        d.frame(&theme);
        Inner { theme, d }
    }
}

impl<T: Theme> OffscreenDyn for Inner<T> {
    fn dialog_size_px(&self) -> [u32; 2] {
        self.d.size_px()
    }

    fn button_rects(&self) -> Vec<[f32; 4]> {
        self.d.button_rects()
    }

    fn freeze(&mut self, t: f64) {
        self.d.freeze_clock(t);
    }

    fn apply(&mut self, q: Queued) {
        match q {
            Queued::Event(ev) => self.d.handle_event(&self.theme, ev),
            Queued::Progress(TestProgress::Value(v)) => self.d.set_progress_value(v),
            Queued::Progress(TestProgress::Indeterminate) => self.d.set_progress_indeterminate(),
            Queued::Text(s) => self.d.set_text(&self.theme, &s),
        }
    }

    fn render(&mut self) {
        self.d.frame(&self.theme);
        // Apply a size change like a window system: the next frame has the new size.
        if let Some(_logical) = self.d.take_resize() {
            let [width, height] = self.d.physical_size(self.d.ppp());
            self.d.handle_event(&self.theme, HostEvent::Resized { width, height });
        }
        let _ = self.d.take_titlebar_change();
    }

    fn rgba(&self) -> Option<(u32, u32, &[u8])> {
        self.d.presenter()?.read_rgba()
    }

    fn result(&self) -> Option<XDialogResult> {
        self.d.result().cloned()
    }
}

/// Deterministic window-less dialog. Same `Dialog<T>` code path as real windows.
pub struct OffscreenDialog {
    inner: Box<dyn OffscreenDyn>,
    // `set_progress`/`set_text` are applied with the clock time of the next render, so they are
    // queued alongside the events, in call order.
    queue: Vec<Queued>,
    ppp: f32,
    last_t: f64,
    /// Last rendered image (kept when the dialog closed and stops presenting).
    last: Vec<u8>,
    last_size: (u32, u32),
}

impl OffscreenDialog {
    /// Build a dialog with theme `"linux"` or `"fluent"`, rendered at `ppp`. Runs the measure
    /// pass, the keyboard on-open step and the open frame at `t = 0` (scripts should use `t > 0`).
    pub fn new(theme: &str, look: TestAppearance, ppp: f32, kind: TestKind, options: XDialogOptions) -> Result<Self, String> {
        if !(ppp.is_finite() && ppp > 0.0 && ppp <= 8.0) {
            return Err(format!("invalid ppp {ppp}"));
        }
        let appearance = Appearance::test(look.dark, look.accent, look.accent_palette);
        let kind = match kind {
            TestKind::Message => DialogKind::Message,
            TestKind::Progress => DialogKind::Progress,
        };
        let inner: Box<dyn OffscreenDyn> = match theme {
            #[cfg(xd_theme_linux)]
            "linux" => Box::new(Inner::new(crate::backends::linux_egui::LinuxTheme::new(), appearance, ppp, kind, options)),
            #[cfg(xd_theme_fluent)]
            "fluent" => Box::new(Inner::new(crate::backends::fluent_egui::FluentTheme::new(), appearance, ppp, kind, options)),
            other => return Err(format!("theme {other:?} is not compiled into this build (features linux-egui / fluent-egui)")),
        };
        Ok(OffscreenDialog { inner, queue: Vec::new(), ppp, last_t: 0.0, last: Vec::new(), last_size: (0, 0) })
    }

    /// Client size in physical px (the size the next render will have).
    pub fn size_px(&self) -> (u32, u32) {
        let [w, h] = self.inner.dialog_size_px();
        (w, h)
    }

    /// Pixels per point this dialog renders at.
    pub fn ppp(&self) -> f32 {
        self.ppp
    }

    /// Button rects in LOGICAL px `[x, y, w, h]`, indexed by API button index (from the last
    /// pass; the measure pass right after `new`).
    pub fn button_rects(&self) -> Vec<[f32; 4]> {
        self.inner.button_rects()
    }

    /// Centre of button `i` in physical px (for pointer events), `None` for an unknown index.
    pub fn button_centre_px(&self, i: usize) -> Option<(f64, f64)> {
        let (x, y) = rect_centre(*self.button_rects().get(i)?);
        Some((x * self.ppp as f64, y * self.ppp as f64))
    }

    /// Queue an event (physical px coordinates); applied at the next render.
    pub fn event(&mut self, ev: HostEvent) {
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

    /// Advance the dialog clock to `t` seconds and render; returns RGBA8 of the rendered size
    /// (see [`OffscreenDialog::last_size`]; normally `size_px()` before the call). Time is
    /// monotonic: an earlier `t` than the previous render is clamped to it.
    pub fn render_at(&mut self, t: f64) -> Vec<u8> {
        let t = if t.is_finite() { t.max(self.last_t) } else { self.last_t };
        self.last_t = t;
        self.inner.freeze(t);
        for q in std::mem::take(&mut self.queue) {
            self.inner.apply(q);
        }
        self.inner.render();
        if let Some((w, h, px)) = self.inner.rgba() {
            self.last.clear();
            self.last.extend_from_slice(px);
            self.last_size = (w, h);
        }
        self.last.clone()
    }

    /// Size (physical px) of the image the last `render_at` returned.
    pub fn last_size(&self) -> (u32, u32) {
        self.last_size
    }

    /// The dialog's result, once it closed.
    pub fn result(&self) -> Option<XDialogResult> {
        self.inner.result()
    }
}
