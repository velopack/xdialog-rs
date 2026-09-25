//! Offscreen gallery for the egui themes.
//!
//! ```text
//! cargo run --release --example egui_gallery --features _test-hooks -- \
//!     [--theme ubuntu|fluent|all] [--out <dir>] [--filter <substr>]
//! ```
//!
//! Renders every variant of `ubuntu.rs` / `fluent.rs` with the deterministic offscreen renderer
//! (injected clock, pointer, keyboard focus and appearance, light and dark) and writes
//! `<out>/<theme>/<variant><suffix>.png` per capture plus `<out>/<theme>/sheet.png`, a contact
//! sheet of the stills (captures without a suffix). Review the images by eye.
//!
//! The driver is `main.rs` + `model.rs`; the variant lists are `ubuntu.rs` and `fluent.rs`.

mod fluent;
mod ubuntu;
mod model;

use std::path::PathBuf;
use std::time::Instant;

use image::RgbaImage;

pub use model::*;
pub use xdialog::__test::egui::Key;
pub use xdialog::__test::{TestAppearance, TestKind, TestProgress};
pub use xdialog::{XDialogBackend, XDialogIcon, XDialogOptions};

type Theme = (XDialogBackend, fn() -> Vec<Variant>);

/// The gallery themes and their variant lists.
const THEMES: [Theme; 2] = [(XDialogBackend::Ubuntu, ubuntu::variants), (XDialogBackend::Fluent, fluent::variants)];

struct Args {
    themes: Vec<Theme>,
    out: PathBuf,
    filter: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args { themes: vec![THEMES[0]], out: PathBuf::from("target/egui_gallery"), filter: None };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = || it.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--theme" => {
                let t = val()?;
                a.themes = THEMES.into_iter().filter(|(b, _)| t == "all" || theme_name(*b) == t).collect();
                if a.themes.is_empty() {
                    return Err(format!("unknown theme {t}"));
                }
            }
            "--out" => a.out = PathBuf::from(val()?),
            "--filter" => a.filter = Some(val()?),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(a)
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("egui_gallery: {e}");
            std::process::exit(2);
        }
    };
    for &(backend, variants) in &args.themes {
        let variants: Vec<Variant> = variants().into_iter().filter(|v| args.filter.as_deref().is_none_or(|f| v.name.contains(f))).collect();
        if let Err(e) = run_theme(backend, &variants, &args) {
            eprintln!("egui_gallery: {}: {e}", theme_name(backend));
            std::process::exit(1);
        }
    }
}

fn run_theme(backend: XDialogBackend, variants: &[Variant], args: &Args) -> Result<(), String> {
    let theme = theme_name(backend);
    let dir = args.out.join(&theme);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let t0 = Instant::now();
    let (mut frames_total, mut stills) = (0usize, Vec::new());
    for v in variants {
        for f in run_variant(backend, v)? {
            frames_total += 1;
            let img = RgbaImage::from_raw(f.w, f.h, f.rgba).ok_or("bad frame buffer")?;
            let path = dir.join(format!("{}{}.png", v.name, f.suffix));
            img.save(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            if f.suffix.is_empty() {
                stills.push(img);
            }
        }
    }
    if !stills.is_empty() {
        let path = dir.join("sheet.png");
        contact_sheet(&stills).save(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    println!("{theme}: {} variants, {frames_total} frames in {:.1}s -> {}", variants.len(), t0.elapsed().as_secs_f64(), dir.display());
    Ok(())
}

/// Shelf-packs the images left to right into rows at most `MAX_W` wide, on a mid-grey background.
fn contact_sheet(images: &[RgbaImage]) -> RgbaImage {
    const MAX_W: u32 = 1800;
    const GAP: u32 = 8;
    let mut placed = Vec::with_capacity(images.len());
    let (mut x, mut y, mut row_h, mut width) = (GAP, GAP, 0, 0);
    for img in images {
        if x > GAP && x + img.width() + GAP > MAX_W {
            (x, y, row_h) = (GAP, y + row_h + GAP, 0);
        }
        placed.push((x, y));
        x += img.width() + GAP;
        row_h = row_h.max(img.height());
        width = width.max(x);
    }
    let mut sheet = RgbaImage::from_pixel(width, y + row_h + GAP, image::Rgba([0x80, 0x80, 0x80, 0xFF]));
    for (img, &(x, y)) in images.iter().zip(&placed) {
        image::imageops::overlay(&mut sheet, img, x as i64, y as i64);
    }
    sheet
}
