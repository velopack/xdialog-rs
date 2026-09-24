//! Compare tooling for the gallery: reference loading/cropping, per-image
//! metrics (size delta, % pixels over threshold, CIE76 ΔE, text-ink bbox and ink bands, colour
//! probes), side-by-side / diff-heat-map images and the `report.csv` row format. Also included by
//! `examples/egui_live.rs` (which uses only part of it).
#![allow(dead_code)]

use std::path::Path;

use super::model::{Anchor, Frame, Probe, RefSpec, Variant};
use super::Crop;

/// Per-channel threshold for "different" pixels (10/255).
pub const THRESHOLD: u8 = 10;
/// A pixel is ink when its max channel difference to the background exceeds this.
const INK: u8 = 40;

/// An RGBA8 image.
#[derive(Clone, Debug)]
pub struct Img {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl Img {
    pub fn new(w: u32, h: u32, fill: [u8; 4]) -> Self {
        Img { w, h, px: fill.iter().copied().cycle().take((w * h * 4) as usize).collect() }
    }

    pub fn from_frame(f: &Frame) -> Self {
        Img { w: f.w, h: f.h, px: f.rgba.clone() }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let img = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?.to_rgba8();
        Ok(Img { w: img.width(), h: img.height(), px: img.into_raw() })
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        image::save_buffer(path, &self.px, self.w, self.h, image::ColorType::Rgba8).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn rgb(&self, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * self.w + x) * 4) as usize;
        [self.px[i], self.px[i + 1], self.px[i + 2]]
    }

    pub fn rgb_clamped(&self, x: i64, y: i64) -> [u8; 3] {
        self.rgb(x.clamp(0, self.w as i64 - 1) as u32, y.clamp(0, self.h as i64 - 1) as u32)
    }

    fn put(&mut self, x: u32, y: u32, c: [u8; 4]) {
        if x < self.w && y < self.h {
            let i = ((y * self.w + x) * 4) as usize;
            self.px[i..i + 4].copy_from_slice(&c);
        }
    }

    pub fn crop(&self, x: u32, y: u32, w: u32, h: u32) -> Img {
        let (x, y) = (x.min(self.w), y.min(self.h));
        let (w, h) = (w.min(self.w - x), h.min(self.h - y));
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for row in y..y + h {
            let i = ((row * self.w + x) * 4) as usize;
            out.extend_from_slice(&self.px[i..i + (w * 4) as usize]);
        }
        Img { w, h, px: out }
    }

    /// Nearest-neighbour upscale.
    pub fn zoom(&self, k: u32) -> Img {
        let mut out = Img::new(self.w * k, self.h * k, [0; 4]);
        for y in 0..out.h {
            for x in 0..out.w {
                let i = (((y / k) * self.w + x / k) * 4) as usize;
                out.put(x, y, [self.px[i], self.px[i + 1], self.px[i + 2], self.px[i + 3]]);
            }
        }
        out
    }

    /// Copy `src` into `self` at `(x, y)` (clipped).
    pub fn blit(&mut self, src: &Img, x: u32, y: u32) {
        for sy in 0..src.h {
            for sx in 0..src.w {
                let i = ((sy * src.w + sx) * 4) as usize;
                self.put(x + sx, y + sy, [src.px[i], src.px[i + 1], src.px[i + 2], src.px[i + 3]]);
            }
        }
    }
}

fn maxdiff(a: [u8; 3], b: [u8; 3]) -> u8 {
    (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap_or(0)
}

/// Load and crop a reference.
pub fn load_ref(refs: &Path, spec: &RefSpec, suffix: &str) -> Result<(String, Img), String> {
    let rel = spec.path_for(suffix);
    let img = Img::load(&refs.join(&rel))?;
    let img = match &spec.crop {
        Crop::Full => img,
        Crop::Rect([x, y, w, h]) => img.crop(*x, *y, *w, *h),
        Crop::AutoByColor(c) => {
            let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
            for y in 0..img.h {
                for x in 0..img.w {
                    if maxdiff(img.rgb(x, y), *c) <= 3 {
                        x0 = x0.min(x);
                        y0 = y0.min(y);
                        x1 = x1.max(x);
                        y1 = y1.max(y);
                    }
                }
            }
            if x0 > x1 {
                return Err(format!("{rel}: colour {c:?} not found for auto-crop"));
            }
            img.crop(x0, y0, x1 - x0 + 1, y1 - y0 + 1)
        }
    };
    Ok((rel, img))
}

// ---------------------------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------------------------

fn srgb_to_lab(c: [u8; 3]) -> [f64; 3] {
    let lin = |v: u8| {
        let v = v as f64 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (lin(c[0]), lin(c[1]), lin(c[2]));
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f64| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// CIE76 ΔE between two sRGB colours.
pub fn delta_e(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (a, b) = (srgb_to_lab(a), srgb_to_lab(b));
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Most common colour on the image border (the background).
pub fn background(img: &Img) -> [u8; 3] {
    let mut counts: std::collections::HashMap<[u8; 3], u32> = Default::default();
    let mut add = |x: u32, y: u32| *counts.entry(img.rgb(x, y)).or_default() += 1;
    for x in 0..img.w {
        add(x, 0);
        add(x, img.h - 1);
    }
    for y in 0..img.h {
        add(0, y);
        add(img.w - 1, y);
    }
    counts.into_iter().max_by_key(|(c, n)| (*n, *c)).map(|(c, _)| c).unwrap_or([0; 3])
}

/// Ink: bbox `[x0, y0, x1, y1]` (inclusive) of pixels far from the background, plus horizontal
/// bands (row ranges containing ink, gaps of one row merged). Bands approximate text lines,
/// icons, bars and button rows.
#[derive(Clone, Debug, Default)]
pub struct Ink {
    pub bbox: Option<[u32; 4]>,
    pub bands: Vec<(u32, u32)>,
}

pub fn ink(img: &Img) -> Ink {
    let bg = background(img);
    let mut bbox: Option<[u32; 4]> = None;
    let mut rows = vec![false; img.h as usize];
    for y in 0..img.h {
        for x in 0..img.w {
            if maxdiff(img.rgb(x, y), bg) > INK {
                rows[y as usize] = true;
                let b = bbox.get_or_insert([x, y, x, y]);
                b[0] = b[0].min(x);
                b[1] = b[1].min(y);
                b[2] = b[2].max(x);
                b[3] = b[3].max(y);
            }
        }
    }
    let mut bands: Vec<(u32, u32)> = Vec::new();
    for (y, on) in rows.iter().enumerate() {
        if !on {
            continue;
        }
        let y = y as u32;
        match bands.last_mut() {
            Some(b) if y <= b.1 + 2 => b.1 = y,
            _ => bands.push((y, y)),
        }
    }
    Ink { bbox, bands }
}

/// Pixel comparison of two images aligned at the top-left.
#[derive(Clone, Debug, Default)]
pub struct Diff {
    /// % of the union area whose max channel difference exceeds [`THRESHOLD`] (non-overlapping
    /// area counts as different).
    pub diff_pct: f64,
    pub mean_de: f64,
    pub max_de: f64,
}

pub fn diff(a: &Img, b: &Img) -> (Diff, Img) {
    let (w, h) = (a.w.max(b.w), a.h.max(b.h));
    let mut heat = Img::new(w, h, [40, 0, 40, 255]);
    let (ow, oh) = (a.w.min(b.w), a.h.min(b.h));
    let mut over = 0u64;
    let mut sum_de = 0.0;
    let mut max_de: f64 = 0.0;
    for y in 0..oh {
        for x in 0..ow {
            let (ca, cb) = (a.rgb(x, y), b.rgb(x, y));
            let d = maxdiff(ca, cb);
            let de = delta_e(ca, cb);
            sum_de += de;
            max_de = max_de.max(de);
            if d > THRESHOLD {
                over += 1;
            }
            // Faded luma of ours, red overlay by difference.
            let l = ((ca[0] as u32 * 299 + ca[1] as u32 * 587 + ca[2] as u32 * 114) / 1000) as f32;
            let base = (l * 0.35 + 40.0) as u8;
            let k = (d as f32 / 48.0).min(1.0);
            let r = (base as f32 * (1.0 - k) + 255.0 * k) as u8;
            let gb = (base as f32 * (1.0 - k)) as u8;
            heat.put(x, y, [r, gb, gb, 255]);
        }
    }
    let area = (w as u64 * h as u64).max(1);
    let non_overlap = area - ow as u64 * oh as u64;
    let n = (ow as u64 * oh as u64).max(1) as f64;
    (Diff { diff_pct: (over + non_overlap) as f64 * 100.0 / area as f64, mean_de: sum_de / n, max_de }, heat)
}

/// `[ours | ref | diff]` at 2× zoom on a grey sheet.
pub fn side_by_side(ours: &Img, reference: Option<&Img>, heat: Option<&Img>) -> Img {
    let parts: Vec<Img> = [Some(ours), reference, heat].into_iter().flatten().map(|i| i.zoom(2)).collect();
    let gap = 8;
    let w = parts.iter().map(|p| p.w).sum::<u32>() + gap * (parts.len() as u32 + 1);
    let h = parts.iter().map(|p| p.h).max().unwrap_or(0) + 2 * gap;
    let mut sheet = Img::new(w, h, [128, 128, 128, 255]);
    let mut x = gap;
    for p in &parts {
        sheet.blit(p, x, gap);
        x += p.w + gap;
    }
    sheet
}

/// Stack images vertically (a contact sheet), 1×, with gaps.
pub fn stack(rows: &[Img]) -> Img {
    let gap = 6;
    let w = rows.iter().map(|r| r.w).max().unwrap_or(1) + 2 * gap;
    let h = rows.iter().map(|r| r.h + gap).sum::<u32>() + gap;
    let mut sheet = Img::new(w, h, [96, 96, 96, 255]);
    let mut y = gap;
    for r in rows {
        sheet.blit(r, gap, y);
        y += r.h + gap;
    }
    sheet
}

// ---------------------------------------------------------------------------------------------
// Probes
// ---------------------------------------------------------------------------------------------

/// Result of one probe on one capture (`got`/`expect` for callers that want the raw colours).
#[derive(Clone, Debug)]
pub struct ProbeResult {
    pub label: String,
    pub pass: bool,
    pub got: [u8; 3],
    pub expect: Option<[u8; 3]>,
    pub detail: String,
}

/// `_fNN` -> NN.
pub fn frame_index(suffix: &str) -> Option<usize> {
    suffix.strip_prefix("_f").and_then(|n| n.parse().ok())
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// Evaluate the variant's probes on capture `f`. `load_ref(suffix)` returns a reference capture
/// (already cropped) for `expect: None` probes and neighbour frames. Client-anchored positions
/// are resolved against the reference's own size (so bottom/right-anchored probes stay on the
/// same element when the sizes differ); button-anchored ones use our button rects.
pub fn run_probes(v: &Variant, f: &Frame, load_ref: &mut dyn FnMut(&str) -> Option<Img>) -> Vec<ProbeResult> {
    let mut ref_at = |sfx: &str, p: &Probe, ours: (i64, i64)| -> Option<[u8; 3]> {
        let img = load_ref(sfx)?;
        let (x, y) = match p.anchor {
            Anchor::Client => {
                let proxy = Frame { suffix: String::new(), t: 0.0, w: img.w, h: img.h, rgba: Vec::new(), ppp: f.ppp, button_rects: Vec::new() };
                proxy.probe_px(p)?
            }
            _ => ours,
        };
        Some(img.rgb_clamped(x, y))
    };
    let mut out = Vec::new();
    for p in v.probes.iter().filter(|p| p.only.as_deref().is_none_or(|s| s == f.suffix)) {
        let Some((x, y)) = f.probe_px(p) else {
            out.push(ProbeResult { label: p.label.clone(), pass: false, got: [0; 3], expect: p.expect, detail: "no such button".into() });
            continue;
        };
        let got = f.rgb(x, y);
        // Candidate expectations: this frame, then frames NN±1 (timing jitter of the references).
        let mut candidates: Vec<(String, [u8; 3])> = Vec::new();
        let neighbours: Vec<String> = match frame_index(&f.suffix) {
            Some(k) => {
                let mut v = vec![f.suffix.clone(), format!("_f{:02}", k + 1)];
                if k > 0 {
                    v.push(format!("_f{:02}", k - 1));
                }
                v
            }
            None => vec![f.suffix.clone()],
        };
        for (i, sfx) in neighbours.iter().enumerate() {
            let e = match p.expect {
                None => ref_at(sfx, p, (x, y)),
                Some(e) if i == 0 => Some(e),
                // An explicit per-frame expectation of the neighbouring frame (same position).
                Some(_) => v.probes
                            .iter()
                            .find(|q| q.only.as_deref() == Some(sfx.as_str()) && q.anchor == p.anchor && q.at == p.at)
                            .and_then(|q| q.expect),
            };
            if let Some(e) = e {
                candidates.push((sfx.clone(), e));
            }
        }
        if p.expect.is_none() && candidates.is_empty() {
            // Reference-sampled probe without a reference (no `--refs`, or missing file): skipped.
            continue;
        }
        let hit = candidates.iter().find(|(_, e)| maxdiff(got, *e) <= p.tol);
        // A reference frame can be a repeat of its predecessor (a dropped animation tick) while
        // ours shows the true in-between value: also pass when `got` lies between the colours of
        // frames NN-1 and NN+1 (per channel, +-tol), i.e. somewhere on the captured transition.
        let between = || -> Option<String> {
            let (lo, hi) = (candidates.iter().find(|(s, _)| frame_index(s) == frame_index(&f.suffix).map(|k| k + 1))?,
                            candidates.iter().find(|(s, _)| frame_index(s).is_some_and(|n| Some(n + 1) == frame_index(&f.suffix)))?);
            let inside = (0..3).all(|c| {
                                   let (a, b) = (lo.1[c].min(hi.1[c]), lo.1[c].max(hi.1[c]));
                                   (a.saturating_sub(p.tol)..=b.saturating_add(p.tol)).contains(&got[c])
                               });
            inside.then(|| format!("between {} and {}", hi.0, lo.0))
        };
        let expect = candidates.first().map(|c| c.1);
        let (pass, detail) = match (hit, expect) {
            (Some((s, _)), _) if *s == f.suffix => (true, String::new()),
            (Some((s, _)), _) => (true, format!("matched {s}")),
            (None, Some(e)) => match between() {
                Some(d) => (true, d),
                None => (false, format!("got {} want {} (tol {})", hex(got), hex(e), p.tol)),
            },
            (None, None) => (false, "no reference".into()),
        };
        out.push(ProbeResult { label: p.label.clone(), pass, got, expect, detail });
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------------------------

pub const CSV_HEADER: &str = "theme,variant,frame,t,ours_w,ours_h,ref,ref_w,ref_h,dw,dh,diff_pct,mean_de,max_de,ink_ours,ink_ref,ink_dx0,\
                              ink_dy0,ink_dx1,ink_dy1,bands_ours,bands_ref,first_band_dy,probes_pass,probes_total,probe_failures";

/// One report row.
#[derive(Clone, Debug, Default)]
pub struct Row {
    pub theme: String,
    pub variant: String,
    pub frame: String,
    pub t: f64,
    pub ours: (u32, u32),
    pub reference: Option<(String, u32, u32)>,
    pub diff: Option<Diff>,
    pub ink_ours: Ink,
    pub ink_ref: Option<Ink>,
    pub probes: Vec<ProbeResult>,
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

fn bbox_str(b: Option<[u32; 4]>) -> String {
    b.map_or(String::new(), |b| format!("{} {} {} {}", b[0], b[1], b[2], b[3]))
}

fn bands_str(b: &[(u32, u32)]) -> String {
    b.iter().map(|(a, z)| format!("{a}-{z}")).collect::<Vec<_>>().join(" ")
}

impl Row {
    pub fn probe_failures(&self) -> usize {
        self.probes.iter().filter(|p| !p.pass).count()
    }

    pub fn csv(&self) -> String {
        let (rname, rw, rh) = self.reference.clone().unwrap_or_default();
        let has_ref = self.reference.is_some();
        let d = self.diff.clone().unwrap_or_default();
        let ir = self.ink_ref.clone().unwrap_or_default();
        let delta = |i: usize| match (self.ink_ours.bbox, ir.bbox) {
            (Some(a), Some(b)) => (a[i] as i64 - b[i] as i64).to_string(),
            _ => String::new(),
        };
        let first_dy = match (self.ink_ours.bands.first(), ir.bands.first()) {
            (Some(a), Some(b)) => (a.0 as i64 - b.0 as i64).to_string(),
            _ => String::new(),
        };
        let fails: Vec<String> = self.probes.iter().filter(|p| !p.pass).map(|p| format!("{}: {}", p.label, p.detail)).collect();
        let opt = |s: String| if has_ref { s } else { String::new() };
        [self.theme.clone(),
         self.variant.clone(),
         self.frame.clone(),
         format!("{:.3}", self.t),
         self.ours.0.to_string(),
         self.ours.1.to_string(),
         rname,
         opt(rw.to_string()),
         opt(rh.to_string()),
         opt((self.ours.0 as i64 - rw as i64).to_string()),
         opt((self.ours.1 as i64 - rh as i64).to_string()),
         opt(format!("{:.2}", d.diff_pct)),
         opt(format!("{:.2}", d.mean_de)),
         opt(format!("{:.1}", d.max_de)),
         bbox_str(self.ink_ours.bbox),
         bbox_str(ir.bbox),
         delta(0),
         delta(1),
         delta(2),
         delta(3),
         bands_str(&self.ink_ours.bands),
         bands_str(&ir.bands),
         first_dy,
         self.probes.iter().filter(|p| p.pass).count().to_string(),
         self.probes.len().to_string(),
         fails.join("; ")].iter()
                          .map(|s| csv_field(s))
                          .collect::<Vec<_>>()
                          .join(",")
    }
}
