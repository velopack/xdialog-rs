//! Offscreen gallery for the egui themes.
//!
//! ```text
//! cargo run --release --example egui_gallery --features _test-hooks,fluent-egui,linux-egui -- \
//!     [--theme linux|fluent|all] [--out <dir>] [--filter <substr>] [--list]
//! ```
//!
//! Renders every variant of `linux.rs` / `fluent.rs` with the deterministic offscreen renderer
//! (injected clock, pointer, keyboard focus and appearance, light and dark) and writes
//! `<out>/<theme>/<variant><suffix>.png` per capture plus `<out>/<theme>/sheet.png`, a contact
//! sheet of the stills (captures without a suffix). Review the images by eye.
//!
//! The driver is `main.rs` + `model.rs`; the variant lists are `linux.rs` and `fluent.rs`.

mod fluent;
mod linux;
mod model;

use std::path::PathBuf;
use std::time::Instant;

use image::RgbaImage;

pub use model::*;
pub use xdialog::__test::{Key, TestAppearance, TestKind, TestProgress};
pub use xdialog::{XDialogIcon, XDialogOptions};

struct Args {
    themes: Vec<&'static str>,
    out: PathBuf,
    filter: Option<String>,
    list: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args { themes: vec!["linux"], out: PathBuf::from("target/egui_gallery"), filter: None, list: false };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = || it.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--theme" => {
                a.themes = match val()?.as_str() {
                    "linux" => vec!["linux"],
                    "fluent" => vec!["fluent"],
                    "all" => vec!["linux", "fluent"],
                    other => return Err(format!("unknown theme {other}")),
                }
            }
            "--out" => a.out = PathBuf::from(val()?),
            "--filter" => a.filter = Some(val()?),
            "--list" => a.list = true,
            "-h" | "--help" => {
                println!("{}", include_str!("main.rs").lines().take_while(|l| l.starts_with("//!")).map(|l| l.trim_start_matches("//!")).collect::<Vec<_>>().join("\n"));
                std::process::exit(0);
            }
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
    for &theme in &args.themes {
        let all = if theme == "fluent" { fluent::variants() } else { linux::variants() };
        let variants: Vec<Variant> = all.into_iter().filter(|v| args.filter.as_deref().is_none_or(|f| v.name.contains(f))).collect();
        if args.list {
            for v in &variants {
                println!("{theme}\t{}\t{} captures", v.name, v.captures.len());
            }
            continue;
        }
        if let Err(e) = run_theme(theme, &variants, &args) {
            eprintln!("egui_gallery: {theme}: {e}");
            std::process::exit(1);
        }
    }
}

fn run_theme(theme: &str, variants: &[Variant], args: &Args) -> Result<(), String> {
    let dir = args.out.join(theme);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let t0 = Instant::now();
    let (mut frames_total, mut stills) = (0usize, Vec::new());
    for v in variants {
        for f in run_variant(theme, v)? {
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
