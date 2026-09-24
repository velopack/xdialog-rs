//! Gallery variant model + deterministic script runner.
//!
//! Shared by the gallery driver (`main.rs`), the offscreen goldens (`tests/egui_offscreen.rs`), the
//! render bench (`benches/render.rs`) and the live harness (`examples/egui_live.rs`), which include
//! this file with `#[path]`. `linux.rs` / `fluent.rs` build `Vec<Variant>` from it.
//!
//! Coordinates are LOGICAL px (client-relative) unless stated otherwise; the runner converts to
//! physical px with the variant's `ppp`.
#![allow(dead_code)]

use xdialog::__test::{HostEvent, Key, MouseButton, OffscreenDialog, TestAppearance, TestKind, TestProgress};
use xdialog::{XDialogIcon, XDialogOptions};

/// A scripted state change applied at a dialog-clock time.
#[derive(Clone, Debug)]
pub enum Action {
    /// Inject a raw input/window event (physical px).
    Event(HostEvent),
    /// Move the pointer to the centre of the button with this API index.
    HoverButton(usize),
    /// Move the pointer to the button's centre and press (hold) the primary button.
    PressButton(usize),
    /// Release the primary button (wherever the pointer is).
    Release,
    /// Move the pointer to a logical point; negative coordinates count from the right/bottom edge.
    MoveTo(f32, f32),
    /// The pointer left the window.
    Leave,
    /// Press and release a key.
    Key(Key),
    /// Press a key (held).
    KeyDown(Key),
    /// Release a key.
    KeyUp(Key),
    /// Window focus change.
    WindowFocus(bool),
    /// Change progress.
    Progress(TestProgress),
    /// Replace the body text.
    SetText(String),
    /// Force a button's disabled state.
    Disable(usize, bool),
}

/// How to cut the dialog client out of a reference PNG.
#[derive(Clone, Debug)]
pub enum Crop {
    /// Use the whole image.
    Full,
    /// Fixed rect `[x, y, w, h]` in reference pixels.
    Rect([u32; 4]),
    /// Auto-detect the client as the bounding box of the pixels of this colour (RGB, ±3).
    AutoByColor([u8; 3]),
}

/// A reference image to compare against.
#[derive(Clone, Debug)]
pub struct RefSpec {
    /// Path relative to the `--refs` directory. `{suffix}` is replaced by the capture suffix.
    pub path: String,
    pub crop: Crop,
    /// Per-capture overrides `(suffix, path)` (reference sequences with irregular file names).
    pub frames: Vec<(String, String)>,
}

impl RefSpec {
    pub fn new(path: impl Into<String>, crop: Crop) -> Self {
        RefSpec { path: path.into(), crop, frames: Vec::new() }
    }

    /// Reference path for a capture suffix.
    pub fn path_for(&self, suffix: &str) -> String {
        match self.frames.iter().find(|(s, _)| s == suffix) {
            Some((_, p)) => p.clone(),
            None => self.path.replace("{suffix}", suffix),
        }
    }
}

/// What a probe position is relative to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    /// Client area; negative coordinates count from the right/bottom edge.
    Client,
    /// A button rect (API index); x/y from its left/top edge, negative from its right/bottom.
    Button(usize),
    /// A button rect; x from its left edge (negative: right), y from its vertical centre.
    ButtonMid(usize),
}

/// A colour probe (the primary pass/fail signal of the compare report).
#[derive(Clone, Debug)]
pub struct Probe {
    /// Short label for the report.
    pub label: String,
    /// Logical px relative to `anchor`.
    pub at: (f32, f32),
    pub anchor: Anchor,
    /// Expected RGB. `None`: compare with the reference image's pixel at the same position.
    pub expect: Option<[u8; 3]>,
    /// Max abs difference per channel.
    pub tol: u8,
    /// Only for the capture with this suffix (`None`: every capture). For animation frames
    /// (`_fNN`), a probe also passes when it matches frame NN±1 (reference timing jitter) or, for
    /// reference-sampled probes, lies between frames NN-1 and NN+1 (a dropped reference tick).
    pub only: Option<String>,
}

impl Probe {
    pub fn new(label: impl Into<String>, anchor: Anchor, at: (f32, f32), expect: [u8; 3], tol: u8) -> Self {
        Probe { label: label.into(), at, anchor, expect: Some(expect), tol, only: None }
    }

    /// Compare with the reference image at the same point.
    pub fn vs_ref(label: impl Into<String>, anchor: Anchor, at: (f32, f32), tol: u8) -> Self {
        Probe { label: label.into(), at, anchor, expect: None, tol, only: None }
    }

    pub fn only(mut self, suffix: impl Into<String>) -> Self {
        self.only = Some(suffix.into());
        self
    }
}

/// One gallery entry.
#[derive(Clone, Debug)]
pub struct Variant {
    pub name: String,
    pub kind: TestKind,
    pub options: XDialogOptions,
    pub appearance: TestAppearance,
    pub ppp: f32,
    /// `(t seconds, action)`, applied in order (stable for equal times).
    pub script: Vec<(f64, Action)>,
    /// `(t seconds, file-name suffix)` frames to capture.
    pub captures: Vec<(f64, String)>,
    pub reference: Option<RefSpec>,
    pub probes: Vec<Probe>,
    /// Included in the offscreen goldens (`tests/egui_offscreen.rs`).
    pub golden: bool,
}

impl Variant {
    /// A message dialog captured once at `t = 1.0` (suffix "").
    pub fn message(name: impl Into<String>, options: XDialogOptions, appearance: TestAppearance) -> Self {
        Variant { name: name.into(),
                  kind: TestKind::Message,
                  options,
                  appearance,
                  ppp: 1.0,
                  script: Vec::new(),
                  captures: Vec::new(),
                  reference: None,
                  probes: Vec::new(),
                  golden: false }
    }

    /// A progress dialog (determinate 0 until scripted).
    pub fn progress(name: impl Into<String>, options: XDialogOptions, appearance: TestAppearance) -> Self {
        Variant { kind: TestKind::Progress, ..Variant::message(name, options, appearance) }
    }

    pub fn at(mut self, t: f64, a: Action) -> Self {
        self.script.push((t, a));
        self
    }

    pub fn capture(mut self, t: f64, suffix: impl Into<String>) -> Self {
        self.captures.push((t, suffix.into()));
        self
    }

    /// `n` captures `t0 + k·dt` with suffixes `_f00.._f{n-1}`.
    pub fn burst(mut self, t0: f64, n: usize, dt: f64) -> Self {
        for k in 0..n {
            self.captures.push((t0 + k as f64 * dt, format!("_f{k:02}")));
        }
        self
    }

    pub fn reference(mut self, r: RefSpec) -> Self {
        self.reference = Some(r);
        self
    }

    pub fn probe(mut self, p: Probe) -> Self {
        self.probes.push(p);
        self
    }

    pub fn probes(mut self, p: impl IntoIterator<Item = Probe>) -> Self {
        self.probes.extend(p);
        self
    }

    pub fn ppp(mut self, ppp: f32) -> Self {
        self.ppp = ppp;
        self
    }

    pub fn golden(mut self) -> Self {
        self.golden = true;
        self
    }

    /// Time of the last script action or capture.
    pub fn end_time(&self) -> f64 {
        self.script.iter().map(|s| s.0).chain(self.captures.iter().map(|c| c.0)).fold(0.0, f64::max)
    }
}

/// Dialog options shorthand.
pub fn opts(title: &str, heading: &str, body: &str, icon: XDialogIcon, buttons: &[&str]) -> XDialogOptions {
    XDialogOptions { title: title.into(),
                     main_instruction: heading.into(),
                     message: body.into(),
                     icon,
                     buttons: buttons.iter().map(|s| s.to_string()).collect() }
}

/// Light/dark appearance without an accent.
pub fn look(dark: bool) -> TestAppearance {
    TestAppearance { dark, accent_palette: None, accent: None }
}

/// "light" / "dark".
pub fn theme_word(dark: bool) -> &'static str {
    if dark {
        "dark"
    } else {
        "light"
    }
}

/// One captured frame.
#[derive(Clone, Debug)]
pub struct Frame {
    pub suffix: String,
    pub t: f64,
    pub w: u32,
    pub h: u32,
    /// RGBA8, `w * h * 4`.
    pub rgba: Vec<u8>,
    pub ppp: f32,
    /// Logical `[x, y, w, h]` by API button index at capture time.
    pub button_rects: Vec<[f32; 4]>,
}

impl Frame {
    /// RGB at a physical pixel (clamped).
    pub fn rgb(&self, x: i64, y: i64) -> [u8; 3] {
        let x = x.clamp(0, self.w as i64 - 1) as usize;
        let y = y.clamp(0, self.h as i64 - 1) as usize;
        let i = (y * self.w as usize + x) * 4;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2]]
    }

    /// Resolve a probe to a physical pixel of this frame (`None`: the button doesn't exist).
    pub fn probe_px(&self, p: &Probe) -> Option<(i64, i64)> {
        let (cw, ch) = (self.w as f32 / self.ppp, self.h as f32 / self.ppp);
        let from = |v: f32, lo: f32, hi: f32| if v < 0.0 { hi + v } else { lo + v };
        let (x, y) = match p.anchor {
            Anchor::Client => (from(p.at.0, 0.0, cw), from(p.at.1, 0.0, ch)),
            Anchor::Button(i) | Anchor::ButtonMid(i) => {
                let r = *self.button_rects.get(i)?;
                let x = from(p.at.0, r[0], r[0] + r[2]);
                let y = match p.anchor {
                    Anchor::ButtonMid(_) => r[1] + r[3] / 2.0 + p.at.1,
                    _ => from(p.at.1, r[1], r[1] + r[3]),
                };
                (x, y)
            }
        };
        Some(((x * self.ppp).floor() as i64, (y * self.ppp).floor() as i64))
    }
}

/// Physical centre of button `i` (from the last pass).
fn button_centre(d: &OffscreenDialog, i: usize) -> Result<(f64, f64), String> {
    let rects = d.button_rects();
    let r = rects.get(i).ok_or_else(|| format!("no button {i} (have {})", rects.len()))?;
    let ppp = d.ppp() as f64;
    Ok(((r[0] + r[2] / 2.0) as f64 * ppp, (r[1] + r[3] / 2.0) as f64 * ppp))
}

fn apply(d: &mut OffscreenDialog, a: &Action) -> Result<(), String> {
    let key = |key: Key, pressed: bool| HostEvent::Key { key, pressed, repeat: false };
    let button = |pressed: bool| HostEvent::MouseButton { button: MouseButton::Primary, pressed };
    match a {
        Action::Event(ev) => d.event(*ev),
        Action::HoverButton(i) => {
            let (x, y) = button_centre(d, *i)?;
            d.event(HostEvent::CursorMoved { x, y });
        }
        Action::PressButton(i) => {
            let (x, y) = button_centre(d, *i)?;
            d.event(HostEvent::CursorMoved { x, y });
            d.event(button(true));
        }
        Action::Release => d.event(button(false)),
        Action::MoveTo(x, y) => {
            let (w, h) = d.size_px();
            let ppp = d.ppp() as f64;
            let fx = if *x < 0.0 { w as f64 + *x as f64 * ppp } else { *x as f64 * ppp };
            let fy = if *y < 0.0 { h as f64 + *y as f64 * ppp } else { *y as f64 * ppp };
            d.event(HostEvent::CursorMoved { x: fx, y: fy });
        }
        Action::Leave => d.event(HostEvent::CursorLeft),
        Action::Key(k) => {
            d.event(key(*k, true));
            d.event(key(*k, false));
        }
        Action::KeyDown(k) => d.event(key(*k, true)),
        Action::KeyUp(k) => d.event(key(*k, false)),
        Action::WindowFocus(f) => d.event(HostEvent::Focused(*f)),
        Action::Progress(p) => d.set_progress(p.clone()),
        Action::SetText(s) => d.set_text(s),
        Action::Disable(i, dis) => d.force_disabled(*i, *dis),
    }
    Ok(())
}

/// Run a variant's script offscreen and return its captures, in capture order.
///
/// Time line: every distinct script/capture time `t` (ascending) is one `render_at(t)` with all
/// actions at `t` applied first; a capture at `t` sees them. Same variant -> same bytes.
pub fn run_variant(theme: &str, v: &Variant) -> Result<Vec<Frame>, String> {
    let mut d = OffscreenDialog::new(theme, v.appearance.clone(), v.ppp, v.kind.clone(), v.options.clone())?;
    let mut times: Vec<f64> = v.script.iter().map(|s| s.0).chain(v.captures.iter().map(|c| c.0)).collect();
    times.sort_by(f64::total_cmp);
    times.dedup();
    let mut frames: Vec<Option<Frame>> = vec![None; v.captures.len()];
    for &t in &times {
        for (_, a) in v.script.iter().filter(|s| s.0 == t) {
            apply(&mut d, a).map_err(|e| format!("{} at t={t}: {e}", v.name))?;
        }
        let rgba = d.render_at(t);
        let (w, h) = d.last_size();
        if rgba.len() != (w * h * 4) as usize || w == 0 {
            return Err(format!("{}: empty render at t={t}", v.name));
        }
        let rects = d.button_rects();
        for (i, (ct, suffix)) in v.captures.iter().enumerate() {
            if *ct == t {
                frames[i] = Some(Frame { suffix: suffix.clone(), t, w, h, rgba: rgba.clone(), ppp: v.ppp, button_rects: rects.clone() });
            }
        }
    }
    Ok(frames.into_iter().flatten().collect())
}
