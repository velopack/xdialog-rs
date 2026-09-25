//! Gallery variant model + deterministic script runner.
//!
//! Shared by the gallery driver (`main.rs`) and the offscreen tests (`tests/egui_offscreen.rs`),
//! which include this file with `#[path]`. `ubuntu.rs` / `fluent.rs` build `Vec<Variant>` from it.
//!
//! Coordinates are LOGICAL px / points (client-relative), like the `egui::Event`s the runner
//! builds.
#![allow(dead_code)]

use xdialog::__test::egui::{Event, Key, PointerButton, Pos2};
use xdialog::__test::{OffscreenDialog, TestAppearance, TestKind, TestProgress};
use xdialog::{XDialogBackend, XDialogIcon, XDialogOptions};

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
    /// Move the pointer to a logical point.
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
                     icon_source: None,
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

/// Lower-case name of an egui backend (`"ubuntu"` / `"fluent"`): the gallery and golden dir.
pub fn theme_name(backend: XDialogBackend) -> String {
    format!("{backend:?}").to_lowercase()
}

/// A primary button press/release at `pos`.
pub fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Default::default() }
}

/// A key press or release.
pub fn key(key: Key, pressed: bool) -> Event {
    Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Default::default() }
}

/// Where a release happens after the pointer left the window: far outside every widget.
const OUTSIDE: Pos2 = Pos2::new(-1.0e6, -1.0e6);

/// Pointer state of a script: where the pointer is (`None`: outside the window) and whether the
/// primary button is held.
#[derive(Default)]
struct Pointer {
    pos: Option<Pos2>,
    down: bool,
}

/// Centre of button `i` in points (from the last pass).
fn button_centre(d: &OffscreenDialog, i: usize) -> Result<Pos2, String> {
    d.button_centre(i).ok_or_else(|| format!("no button {i} (have {})", d.button_rects().len()))
}

fn move_to(d: &mut OffscreenDialog, p: &mut Pointer, pos: Pos2) {
    p.pos = Some(pos);
    d.event(Event::PointerMoved(pos));
}

fn apply(d: &mut OffscreenDialog, p: &mut Pointer, a: &Action) -> Result<(), String> {
    match a {
        Action::HoverButton(i) => move_to(d, p, button_centre(d, *i)?),
        Action::PressButton(i) => {
            let c = button_centre(d, *i)?;
            move_to(d, p, c);
            p.down = true;
            d.event(button(c, true));
        }
        Action::Press => {
            if let Some(pos) = p.pos {
                p.down = true;
                d.event(button(pos, true));
            }
        }
        Action::Release => {
            if std::mem::take(&mut p.down) {
                match p.pos {
                    Some(pos) => d.event(button(pos, false)),
                    None => {
                        d.event(button(OUTSIDE, false));
                        d.event(Event::PointerGone);
                    }
                }
            }
        }
        Action::MoveTo(x, y) => move_to(d, p, Pos2::new(*x, *y)),
        Action::Leave => {
            p.pos = None;
            d.event(Event::PointerGone);
        }
        Action::Key(k) => {
            d.event(key(*k, true));
            d.event(key(*k, false));
        }
        Action::WindowFocus(f) => {
            if !f {
                p.down = false;
            }
            d.event(Event::WindowFocused(*f));
        }
        Action::Progress(pr) => d.set_progress(pr.clone()),
        Action::SetText(s) => d.set_text(s),
    }
    Ok(())
}

/// Run a variant's script offscreen and return its captures, in capture order.
///
/// Time line: every distinct script/capture time `t` (ascending) is one `render_at(t)` with all
/// actions at `t` applied first; a capture at `t` sees them. Same variant -> same bytes.
pub fn run_variant(backend: XDialogBackend, v: &Variant) -> Result<Vec<Frame>, String> {
    let mut d = OffscreenDialog::new(backend, v.appearance.clone(), v.ppp, v.kind, v.options.clone());
    let mut pointer = Pointer::default();
    let mut times: Vec<f64> = v.script.iter().map(|s| s.0).chain(v.captures.iter().map(|c| c.0)).collect();
    times.sort_by(f64::total_cmp);
    times.dedup();
    let mut frames: Vec<Option<Frame>> = vec![None; v.captures.len()];
    for &t in &times {
        for (_, a) in v.script.iter().filter(|s| s.0 == t) {
            apply(&mut d, &mut pointer, a).map_err(|e| format!("{} at t={t}: {e}", v.name))?;
        }
        let (w, h, rgba) = d.render_at(t);
        if w == 0 {
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
