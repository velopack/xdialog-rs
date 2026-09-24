//! Offscreen gallery + compare driver for the egui themes.
//!
//! ```text
//! cargo run --release --example egui_gallery --features _test-hooks,fluent-egui,linux-egui -- \
//!     [--theme linux|fluent|all] [--out <dir>] [--refs <dir> [--compare]] [--filter <substr>] [--list] [--strict]
//! ```
//!
//! Renders every variant of `linux.rs` (1:1 with the skia reference captures) / `fluent.rs` (the WinUI
//! reference variants) with the deterministic offscreen renderer: injected clock,
//! pointer, keyboard focus and appearance, light and dark. Per capture it writes
//! `<out>/<theme>/<variant><suffix>.png`. With `--refs` + `--compare` it also writes
//! `__ref.png` (cropped reference), `__diff.png` (heat map), `__side.png` (ours | ref | diff, 2×),
//! a contact sheet of the static captures (`sheet.png`) and `<out>/<theme>/report.csv` (size delta,
//! % pixels over 10/255, CIE76 ΔE, ink bbox + bands, colour probes). Probes with
//! explicit colours are evaluated even without references. `--strict` exits with 1 when a probe
//! fails.
//!
//! Default `--refs`: `$XDIALOG_REFS` if set. Reference paths in the variant lists are relative to
//! it (`skia/...`, `winui/...`); the reference captures are not part of this repository.
//!
//! The driver is `main.rs` + `model.rs` + `compare.rs`; the variant lists are `linux.rs` and
//! `fluent.rs`.

mod compare;
mod fluent;
mod linux;
mod model;

use std::path::{Path, PathBuf};
use std::time::Instant;

pub use model::*;
pub use xdialog::__test::{HostEvent, Key, TestAppearance, TestKind, TestProgress};
pub use xdialog::{XDialogIcon, XDialogOptions};

use compare::{Img, Row};

struct Args {
    themes: Vec<&'static str>,
    out: PathBuf,
    refs: Option<PathBuf>,
    compare: bool,
    filter: Option<String>,
    list: bool,
    strict: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args { themes: vec!["linux"],
                       out: PathBuf::from("target/egui_gallery"),
                       refs: std::env::var_os("XDIALOG_REFS").map(PathBuf::from),
                       compare: false,
                       filter: None,
                       list: false,
                       strict: false };
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
            "--refs" => a.refs = Some(PathBuf::from(val()?)),
            "--compare" => a.compare = true,
            "--filter" => a.filter = Some(val()?),
            "--list" => a.list = true,
            "--strict" => a.strict = true,
            "-h" | "--help" => {
                println!("{}", include_str!("main.rs").lines().take_while(|l| l.starts_with("//!")).map(|l| l.trim_start_matches("//!")).collect::<Vec<_>>().join("\n"));
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if a.compare && a.refs.is_none() {
        return Err("--compare needs --refs <dir> (or XDIALOG_REFS)".into());
    }
    Ok(a)
}

pub fn variants_for(theme: &str) -> Vec<Variant> {
    match theme {
        "fluent" => fluent::variants(),
        _ => linux::variants(),
    }
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("egui_gallery: {e}");
            std::process::exit(2);
        }
    };
    let mut failures = 0usize;
    for theme in &args.themes {
        let variants: Vec<Variant> =
            variants_for(theme).into_iter().filter(|v| args.filter.as_deref().is_none_or(|f| v.name.contains(f))).collect();
        if args.list {
            for v in &variants {
                println!("{theme}\t{}\t{} captures\t{}", v.name, v.captures.len(), v.reference.as_ref().map_or("-".into(), |r| r.path.clone()));
            }
            continue;
        }
        match run_theme(theme, &variants, &args) {
            Ok(f) => failures += f,
            Err(e) => {
                eprintln!("egui_gallery: {theme}: {e}");
                std::process::exit(1);
            }
        }
    }
    if args.strict && failures > 0 {
        std::process::exit(1);
    }
}

fn run_theme(theme: &str, variants: &[Variant], args: &Args) -> Result<usize, String> {
    let dir = args.out.join(theme);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let t0 = Instant::now();
    let mut rows: Vec<Row> = Vec::new();
    let mut sheet: Vec<Img> = Vec::new();
    let mut frames_total = 0usize;
    for v in variants {
        let frames = run_variant(theme, v)?;
        frames_total += frames.len();
        for f in &frames {
            let ours = Img::from_frame(f);
            ours.save(&dir.join(format!("{}{}.png", v.name, f.suffix)))?;
            let row = compare_frame(theme, v, f, &ours, &dir, args)?;
            if f.suffix.is_empty() {
                sheet.push(side_image(&ours, &row, args, v, f)?);
            }
            rows.push(row);
        }
    }
    let failures: usize = rows.iter().map(Row::probe_failures).sum();
    let probes: usize = rows.iter().map(|r| r.probes.len()).sum();
    let mut csv = String::from(compare::CSV_HEADER);
    csv.push('\n');
    for r in &rows {
        csv.push_str(&r.csv());
        csv.push('\n');
    }
    std::fs::write(dir.join("report.csv"), csv).map_err(|e| e.to_string())?;
    if !sheet.is_empty() {
        compare::stack(&sheet).save(&dir.join("sheet.png"))?;
    }
    println!("{theme}: {} variants, {frames_total} frames, {probes} probes ({failures} failed) in {:.1}s -> {}",
             variants.len(),
             t0.elapsed().as_secs_f64(),
             dir.display());
    for r in rows.iter().filter(|r| r.probe_failures() > 0) {
        for p in r.probes.iter().filter(|p| !p.pass) {
            println!("  FAIL {}{} {}: {}", r.variant, r.frame, p.label, p.detail);
        }
    }
    Ok(failures)
}

/// The side-by-side image of a capture (ours | ref | diff when compared, else ours alone at 1×).
fn side_image(ours: &Img, row: &Row, args: &Args, v: &Variant, f: &Frame) -> Result<Img, String> {
    if args.compare && row.reference.is_some() {
        let (Some(refs), Some(spec)) = (&args.refs, &v.reference) else { return Ok(ours.clone()) };
        let (_, r) = compare::load_ref(refs, spec, &f.suffix)?;
        let (_, heat) = compare::diff(ours, &r);
        Ok(compare::side_by_side(ours, Some(&r), Some(&heat)))
    } else {
        Ok(ours.clone())
    }
}

/// Metrics + probe results of one capture; writes the `__ref/__diff/__side` images.
fn compare_frame(theme: &str, v: &Variant, f: &Frame, ours: &Img, dir: &Path, args: &Args) -> Result<Row, String> {
    let mut row = Row { theme: theme.into(), variant: v.name.clone(), frame: f.suffix.clone(), t: f.t, ours: (f.w, f.h), ink_ours: compare::ink(ours), ..Default::default() };

    // Reference loading (cached per suffix for the probe neighbours).
    let mut cache: std::collections::HashMap<String, Option<Img>> = Default::default();
    let refs = args.refs.clone();
    let spec = v.reference.clone();
    let mut load = |suffix: &str| -> Option<Img> {
        cache.entry(suffix.to_owned())
             .or_insert_with(|| match (&refs, &spec) {
                 (Some(r), Some(s)) => compare::load_ref(r, s, suffix).ok().map(|(_, i)| i),
                 _ => None,
             })
             .clone()
    };

    if args.compare {
        if let (Some(refs), Some(spec)) = (&args.refs, &v.reference) {
            match compare::load_ref(refs, spec, &f.suffix) {
                Ok((name, r)) => {
                    let (d, heat) = compare::diff(ours, &r);
                    let stem = format!("{}{}", v.name, f.suffix);
                    r.save(&dir.join(format!("{stem}__ref.png")))?;
                    heat.save(&dir.join(format!("{stem}__diff.png")))?;
                    compare::side_by_side(ours, Some(&r), Some(&heat)).save(&dir.join(format!("{stem}__side.png")))?;
                    row.ink_ref = Some(compare::ink(&r));
                    row.reference = Some((name, r.w, r.h));
                    row.diff = Some(d);
                }
                Err(e) => eprintln!("  {}{}: reference: {e}", v.name, f.suffix),
            }
        }
    }
    row.probes = compare::run_probes(v, f, &mut load);
    Ok(row)
}
