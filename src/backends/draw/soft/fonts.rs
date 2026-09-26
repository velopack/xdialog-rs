//! Fonts: one process-wide cosmic-text [`FontSystem`] (shaping, bidi, wrapping, per-character
//! fallback) and the faces glyph runs are drawn with.
//!
//! - The database starts with the bundled Ubuntu faces, so a dialog whose text they cover never
//!   waits for anything (and renders the same on every machine: the goldens).
//! - A system font scan (fontdb, no fontconfig: the platform's font directories) runs on a
//!   background thread (`xdialog-fonts`). It is merged into the database when it is ready, or
//!   waited for (bounded) when a dialog opens with characters the bundled family lacks;
//!   cosmic-text then picks fallback faces from it. Merging bumps [`Fonts::generation`] and wakes
//!   the runtime so open dialogs lay out again.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use cosmic_text::{fontdb, FontSystem};
use skrifa::{FontRef, MetadataProvider, Tag};
use vello_cpu::peniko::FontData;

use crate::backends::gui::background::Background;
use crate::backends::gui::runtime::wake_all;

/// Bundled faces (the Ubuntu theme's, and every theme's family on this backend).
pub(crate) const UBUNTU_REGULAR: &[u8] = include_bytes!("fonts/Ubuntu-Regular.ttf");
pub(crate) const UBUNTU_BOLD: &[u8] = include_bytes!("fonts/Ubuntu-Bold.ttf");

/// The bundled family.
pub(crate) const BUNDLED_FAMILY: &str = "Ubuntu";

/// A face ready for drawing: the font data (shared with cosmic-text) and its normalized variation
/// coordinates (F2Dot14 bits: the `wght` cosmic-text shaped with; empty for static fonts).
#[derive(Clone, Debug)]
pub(crate) struct Face {
    pub data: FontData,
    pub coords: Arc<[i16]>,
    /// A colour font (COLR, CBDT or sbix: emoji): its glyphs are drawn in their own colours.
    pub color: bool,
}

/// Vertical metrics of a family, as ratios of the font size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct VMetrics {
    /// Baseline below the line top.
    pub ascent: f32,
    /// Below the baseline (positive).
    pub descent: f32,
    /// Height of flat capitals.
    pub cap_height: f32,
    /// Natural line pitch: ascent + descent + line gap (as DirectWrite and CoreText space lines).
    pub line_height: f32,
}

/// The process-wide font state (see the module docs).
pub(crate) struct Fonts {
    pub system: FontSystem,
    /// Bumped whenever faces are added: text laid out before is stale.
    pub generation: u64,
    scan_merged: bool,
    faces: HashMap<(fontdb::ID, u16), Option<Face>>,
    metrics: HashMap<(String, u16), VMetrics>,
}

/// The system font scan (an empty database if it failed).
static SCAN: Background<Arc<fontdb::Database>> = Background::new();

/// Lock the process-wide font state (created on first use).
pub(crate) fn lock() -> MutexGuard<'static, Fonts> {
    static FONTS: OnceLock<Mutex<Fonts>> = OnceLock::new();
    FONTS.get_or_init(|| Mutex::new(Fonts::new())).lock().unwrap_or_else(|e| e.into_inner())
}

/// Start the background system font scan (idempotent). The runtime is woken when it finishes.
pub(crate) fn start_scan() {
    SCAN.start("xdialog-fonts", || {
            let t0 = std::time::Instant::now();
            let db = scan();
            debug!("xdialog: font scan found {} faces in {:?}", db.len(), t0.elapsed());
            SCAN.set(Arc::new(db));
            wake_all();
        });
}

/// Every system font directory fontdb knows (no fontconfig), plus `$XDG_DATA_DIRS/fonts`.
fn scan() -> fontdb::Database {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    for dir in std::env::var("XDG_DATA_DIRS").unwrap_or_default().split(':') {
        // `/usr/share` and `/usr/local/share` are fontdb's own.
        if !dir.is_empty() && !matches!(dir.trim_end_matches('/'), "/usr/share" | "/usr/local/share") {
            db.load_fonts_dir(PathBuf::from(dir).join("fonts"));
        }
    }
    db
}

impl Fonts {
    fn new() -> Fonts {
        let mut db = fontdb::Database::new();
        for bytes in [UBUNTU_REGULAR, UBUNTU_BOLD] {
            db.load_font_source(fontdb::Source::Binary(Arc::new(bytes)));
        }
        let locale = sys_locale::get_locale().unwrap_or_else(|| "en-US".into());
        Fonts { system: FontSystem::new_with_locale_and_db(locale, db),
                generation: 0,
                scan_merged: false,
                faces: HashMap::new(),
                metrics: HashMap::new() }
    }

    /// Whether the database has a face of `family`.
    pub(crate) fn has_family(&self, family: &str) -> bool {
        self.system.db().faces().any(|f| f.families.iter().any(|(n, _)| n == family))
    }

    /// The drawable face for a glyph cosmic-text shaped with `id` at `weight` (cached).
    pub(crate) fn face(&mut self, id: fontdb::ID, weight: u16) -> Option<Face> {
        if let Some(face) = self.faces.get(&(id, weight)) {
            return face.clone();
        }
        let face = self.system.get_font(id, fontdb::Weight(weight)).and_then(|font| drawable_face(font.as_peniko(), weight));
        self.faces.insert((id, weight), face.clone());
        face
    }

    /// Vertical metrics of the face of `family` matching `weight` (cached); Ubuntu-like defaults
    /// when unknown.
    pub(crate) fn vmetrics(&mut self, family: &str, weight: u16) -> VMetrics {
        if let Some(m) = self.metrics.get(&(family.to_owned(), weight)) {
            return *m;
        }
        let db = self.system.db();
        let query = fontdb::Query { families: &[fontdb::Family::Name(family)], weight: fontdb::Weight(weight), ..Default::default() };
        let m = db.query(&query)
                  .and_then(|id| {
                      db.with_face_data(id, |data, index| {
                            let f = FontRef::from_index(data, index).ok()?;
                            let m = f.metrics(skrifa::instance::Size::unscaled(), skrifa::instance::LocationRef::default());
                            let upem = m.units_per_em as f32;
                            Some(VMetrics { ascent: m.ascent / upem,
                                           descent: -m.descent / upem,
                                           cap_height: m.cap_height.unwrap_or(0.7 * upem) / upem,
                                           line_height: (m.ascent - m.descent + m.leading) / upem })
                        })
                  })
                  .flatten()
                  .filter(|m| m.ascent.is_finite() && m.line_height.is_finite() && m.ascent > 0.3 && m.line_height > 0.5)
                  .unwrap_or(VMetrics { ascent: 0.93, descent: 0.19, cap_height: 0.7, line_height: 1.2 });
        self.metrics.insert((family.to_owned(), weight), m);
        m
    }

    /// Merge the system scan if it is ready (non-blocking).
    fn merge_ready_scan(&mut self) {
        if !self.scan_merged && SCAN.done() {
            if let Some(db) = SCAN.get(Duration::ZERO) {
                self.merge(&db);
            }
        }
    }

    /// Add the scanned faces (except files already loaded and the bundled family, which must stay
    /// the one the goldens were made with).
    fn merge(&mut self, scanned: &fontdb::Database) {
        let path = |f: &fontdb::FaceInfo| match &f.source {
            fontdb::Source::File(p) | fontdb::Source::SharedFile(p, _) => Some(p.clone()),
            fontdb::Source::Binary(_) => None,
        };
        let loaded: HashSet<(PathBuf, u32)> = self.system.db().faces().filter_map(|f| Some((path(f)?, f.index))).collect();
        let db = self.system.db_mut();
        for f in scanned.faces() {
            let known = path(f).is_some_and(|p| loaded.contains(&(p, f.index)));
            if !known && !f.families.iter().any(|(n, _)| n == BUNDLED_FAMILY) {
                db.push_face_info(f.clone());
            }
        }
        self.scan_merged = true;
        self.metrics.clear();
        self.generation += 1;
    }

    /// Whether the faces of `family` map every visible character of `texts`.
    fn family_covers(&self, family: &str, texts: &[&str]) -> bool {
        let db = self.system.db();
        let mut missing: HashSet<char> = texts.iter().flat_map(|t| t.chars()).filter(|c| !is_ignorable(*c)).collect();
        for f in db.faces().filter(|f| f.families.iter().any(|(n, _)| n == family)) {
            db.with_face_data(f.id, |data, index| {
                  if let Ok(font) = FontRef::from_index(data, index) {
                      let charmap = font.charmap();
                      missing.retain(|c| charmap.map(*c).is_none());
                  }
              });
        }
        missing.is_empty()
    }
}

/// `data` ready for drawing at `weight`: its `wght` coordinates and whether it is a colour font.
fn drawable_face(data: FontData, weight: u16) -> Option<Face> {
    let font = FontRef::from_index(data.data.data(), data.index).ok()?;
    let location = font.axes().location([(Tag::new(b"wght"), weight as f32)]);
    let color = [b"COLR", b"CBDT", b"sbix"].iter().any(|t| font.table_data(Tag::new(t)).is_some());
    Some(Face { coords: location.coords().iter().map(|c| c.to_bits()).collect(), data, color })
}

/// Make sure fallback faces are available for `texts` in `family`: merge the system scan when it
/// is ready, and when the family lacks a character, wait up to `wait` for the scan. Returns
/// whether nothing is left to wait for (`false`: the scan is still running; the runtime is woken
/// when it ends).
pub(crate) fn prepare_fonts(family: &str, texts: &[&str], wait: Duration) -> bool {
    start_scan();
    {
        let mut f = lock();
        f.merge_ready_scan();
        if f.scan_merged || f.family_covers(family, texts) {
            return true;
        }
    }
    let scanned = SCAN.get(wait);
    let mut f = lock();
    match scanned {
        Some(db) if !f.scan_merged => f.merge(&db),
        Some(_) => {}
        None => return false,
    }
    true
}

/// Characters the coverage check ignores: controls, whitespace and Unicode
/// `Default_Ignorable_Code_Point`s (variation selectors, ZWJ/ZWNJ, bidi marks, soft hyphen, ...).
pub(crate) fn is_ignorable(c: char) -> bool {
    c.is_control()
    || c.is_whitespace()
    || matches!(c as u32,
                0x00AD | 0x034F | 0x061C | 0x115F..=0x1160 | 0x17B4..=0x17B5 | 0x180B..=0x180F | 0x200B..=0x200F
                | 0x202A..=0x202E | 0x2060..=0x206F | 0x3164 | 0xFE00..=0xFE0F | 0xFEFF | 0xFFA0 | 0xFFF0..=0xFFF8
                | 0x1BCA0..=0x1BCA3 | 0x1D173..=0x1D17A | 0xE0000..=0xE0FFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignorable_chars() {
        for c in ['\n', ' ', '\u{200D}', '\u{FE0F}', '\u{200F}', '\u{00AD}', '\u{2066}', '\u{E0001}'] {
            assert!(is_ignorable(c), "{c:?}");
        }
        for c in ['a', 'é', '中', 'ש'] {
            assert!(!is_ignorable(c), "{c:?}");
        }
    }

    #[test]
    fn bundled_family_covers_latin_only() {
        let f = lock();
        assert!(f.has_family("Ubuntu"));
        assert!(f.family_covers("Ubuntu", &["Hello, world!\n\u{200D}"]));
        assert!(!f.family_covers("Ubuntu", &["中"]));
    }

    #[test]
    fn bundled_metrics() {
        let m = lock().vmetrics("Ubuntu", 400);
        // Ubuntu: ascent 0.932 em, natural line height (ascent + descent + gap) about 1.149 em.
        assert!((m.ascent - 0.932).abs() < 0.01, "{m:?}");
        assert!((m.line_height - 1.149).abs() < 0.01, "{m:?}");
    }

    #[test]
    fn scan_fallback_covers_cjk() {
        // Latin text never waits; CJK waits for the scan, after which the database has more faces.
        assert!(prepare_fonts("Ubuntu", &["Hello"], Duration::ZERO));
        assert!(prepare_fonts("Ubuntu", &["你好"], Duration::from_secs(30)));
        assert!(lock().scan_merged);
    }
}
