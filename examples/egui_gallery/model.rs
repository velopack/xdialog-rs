//! Gallery variant model + deterministic script runner.
//!
//! Shared by the gallery driver (`main.rs`) and the offscreen tests (`tests/egui_offscreen.rs`),
//! which include this file with `#[path]`. `linux.rs` / `fluent.rs` build `Vec<Variant>` from it.
//!
//! Coordinates are LOGICAL px (client-relative); the runner converts to physical px with the
//! variant's `ppp`.
#![allow(dead_code)]

use xdialog::__test::{HostEvent, Key, MouseButton, OffscreenDialog, TestAppearance, TestKind, TestProgress};
use xdialog::{XDialogIcon, XDialogOptions};

/// A scripted state change applied at a dialog-clock time.
#[derive(Clone, Debug)]
pub enum Action {
    /// Move the pointer to the centre of the button with this API index.
    HoverButton(usize),
    /// Move the pointer to the button's centre and press (hold) the primary button.
    PressButton(usize),
    /// Press (hold) the primary button wherever the pointer is.
    Press,
    /// Release the primary button (wherever the pointer is).
    Release,
    /// Move the pointer to a logical point; negative coordinates count from the right/bottom edge.
    MoveTo(f32, f32),
    /// The pointer left the window.
    Leave,
    /// Press and release a key.
    Key(Key),
    /// Window focus change.
    WindowFocus(bool),
    /// Change progress.
    Progress(TestProgress),
    /// Replace the body text.
    SetText(String),
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
    /// Included in the offscreen goldens (`tests/egui_offscreen.rs`).
    pub golden: bool,
}

impl Variant {
    /// A message dialog without captures.
    pub fn message(name: impl Into<String>, options: XDialogOptions, appearance: TestAppearance) -> Self {
        Variant { name: name.into(),
                  kind: TestKind::Message,
                  options,
                  appearance,
                  ppp: 1.0,
                  script: Vec::new(),
                  captures: Vec::new(),
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

    pub fn ppp(mut self, ppp: f32) -> Self {
        self.ppp = ppp;
        self
    }

    pub fn golden(mut self) -> Self {
        self.golden = true;
        self
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
    pub w: u32,
    pub h: u32,
    /// RGBA8, `w * h * 4`.
    pub rgba: Vec<u8>,
}

/// Physical centre of button `i` (from the last pass).
fn button_centre(d: &OffscreenDialog, i: usize) -> Result<(f64, f64), String> {
    d.button_centre_px(i).ok_or_else(|| format!("no button {i} (have {})", d.button_rects().len()))
}

fn apply(d: &mut OffscreenDialog, a: &Action) -> Result<(), String> {
    let button = |pressed: bool| HostEvent::MouseButton { button: MouseButton::Primary, pressed };
    match a {
        Action::HoverButton(i) => {
            let (x, y) = button_centre(d, *i)?;
            d.event(HostEvent::CursorMoved { x, y });
        }
        Action::PressButton(i) => {
            let (x, y) = button_centre(d, *i)?;
            d.event(HostEvent::CursorMoved { x, y });
            d.event(button(true));
        }
        Action::Press => d.event(button(true)),
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
            d.event(HostEvent::Key { key: *k, pressed: true, repeat: false });
            d.event(HostEvent::Key { key: *k, pressed: false, repeat: false });
        }
        Action::WindowFocus(f) => d.event(HostEvent::Focused(*f)),
        Action::Progress(p) => d.set_progress(p.clone()),
        Action::SetText(s) => d.set_text(s),
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
        for (i, (ct, suffix)) in v.captures.iter().enumerate() {
            if *ct == t {
                frames[i] = Some(Frame { suffix: suffix.clone(), w, h, rgba: rgba.clone() });
            }
        }
    }
    Ok(frames.into_iter().flatten().collect())
}
