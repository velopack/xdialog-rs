//! Fonts: the theme's two faces bound to egui families, per-char fallback discovery,
//! `&'static` font bytes.
//!
//! - A theme provides a regular and a bold face ([`ThemeFonts`]); [`font_definitions`] binds
//!   `Proportional` and `Monospace` to the regular face and [`bold_family`] to the bold one, and
//!   appends every fallback face found so far (bold chains get the fallback family's bold face).
//! - Font files are read **once per process** and leaked into `&'static [u8]` (epaint's `FontData`
//!   is `Cow<'static, [u8]>`, so an owned buffer would be copied into every dialog's context).
//! - Every face read from disk is validated with skrifa (parses, has outlines) before it can reach
//!   egui, because epaint panics on a font it cannot parse.
//! - Coverage is checked outside egui with skrifa charmaps, for every visible character of a
//!   dialog's strings, against the theme's primary faces and the fallbacks found so far in this
//!   process. Misses are resolved from a Windows known-path table, or on Linux from a `fontdb`
//!   system scan that runs on a background thread (`xdialog-fontdb`).
//! - Fallbacks found once are kept for the life of the process and seeded into every new dialog.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
#[cfg(target_os = "linux")]
use std::time::Duration;

use egui::epaint::text::VariationCoords;
use skrifa::{FontRef, MetadataProvider};

/// A font face: file bytes (leaked once per process, or `include_bytes!`) + face index (TTC).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FaceRef {
    pub bytes: &'static [u8],
    pub index: u32,
}

impl FaceRef {
    pub(crate) const fn new(bytes: &'static [u8], index: u32) -> Self {
        FaceRef { bytes, index }
    }

    /// egui font data for this face.
    pub(crate) fn font_data(&self) -> egui::FontData {
        let mut data = egui::FontData::from_static(self.bytes);
        data.index = self.index;
        data
    }

    fn font_ref(&self) -> Option<FontRef<'static>> {
        FontRef::from_index(self.bytes, self.index).ok()
    }

    /// Whether the face maps `c` to a glyph.
    pub(crate) fn covers(&self, c: char) -> bool {
        self.font_ref().is_some_and(|f| f.charmap().map(c).is_some())
    }
}

/// A theme face: a [`FaceRef`] plus the variation coordinates it is rendered at (variable fonts).
#[derive(Clone, Debug)]
pub(crate) struct ThemeFace {
    pub face: FaceRef,
    pub coords: VariationCoords,
}

impl From<FaceRef> for ThemeFace {
    fn from(face: FaceRef) -> Self {
        ThemeFace { face, coords: VariationCoords::default() }
    }
}

impl ThemeFace {
    fn font_data(&self) -> egui::FontData {
        let mut data = self.face.font_data();
        data.tweak.coords = self.coords.clone();
        data
    }
}

/// The two faces a theme renders with (`Theme::fonts`).
#[derive(Clone, Debug)]
pub(crate) struct ThemeFonts {
    pub regular: ThemeFace,
    pub bold: ThemeFace,
}

impl ThemeFonts {
    /// Faces without variation coordinates.
    pub(crate) fn new(regular: FaceRef, bold: FaceRef) -> Self {
        ThemeFonts { regular: regular.into(), bold: bold.into() }
    }
}

const REGULAR: &str = "xdialog.regular";
const BOLD: &str = "xdialog.bold";

/// The egui family of bold text (titles).
pub(crate) fn bold_family() -> egui::FontFamily {
    egui::FontFamily::Name(BOLD.into())
}

/// The theme's faces bound to `Proportional`, `Monospace` and [`bold_family`], plus every
/// fallback face appended to each chain (bold chains: the fallback's bold face first, its regular
/// face for glyphs the bold lacks).
pub(crate) fn font_definitions(fonts: &ThemeFonts, fallbacks: &[Fallback]) -> egui::FontDefinitions {
    let mut defs = egui::FontDefinitions::empty();
    defs.font_data.insert(REGULAR.into(), Arc::new(fonts.regular.font_data()));
    defs.font_data.insert(BOLD.into(), Arc::new(fonts.bold.font_data()));
    defs.families.insert(egui::FontFamily::Proportional, vec![REGULAR.into()]);
    defs.families.insert(egui::FontFamily::Monospace, vec![REGULAR.into()]);
    defs.families.insert(bold_family(), vec![BOLD.into()]);
    for fb in fallbacks {
        defs.font_data.insert(fb.name.clone(), Arc::new(fb.face.font_data()));
        if let Some(bold) = fb.bold {
            defs.font_data.insert(fb.bold_name(), Arc::new(bold.font_data()));
        }
        for (family, chain) in &mut defs.families {
            if fb.bold.is_some() && *family == bold_family() {
                chain.push(fb.bold_name());
            }
            chain.push(fb.name.clone());
        }
    }
    defs
}

/// Fonts bundled with the crate (the Ubuntu theme's faces and the Fluent theme's last resort).
pub(crate) mod bundled {
    use super::FaceRef;

    pub(crate) static UBUNTU_REGULAR: &[u8] = include_bytes!("fonts/Ubuntu-Regular.ttf");
    pub(crate) static UBUNTU_BOLD: &[u8] = include_bytes!("fonts/Ubuntu-Bold.ttf");

    pub(crate) const fn ubuntu_regular() -> FaceRef {
        FaceRef::new(UBUNTU_REGULAR, 0)
    }
    pub(crate) const fn ubuntu_bold() -> FaceRef {
        FaceRef::new(UBUNTU_BOLD, 0)
    }
}

/// A fallback face registered process-wide. `name` is the egui font-data key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Fallback {
    pub name: String,
    pub face: FaceRef,
    /// The same family's bold face (weight nearest 700, at least 600) for [`bold_family`]; `None`
    /// when the family has none.
    pub bold: Option<FaceRef>,
}

impl Fallback {
    /// egui font-data key of [`Fallback::bold`].
    pub(crate) fn bold_name(&self) -> String {
        format!("{}:bold", self.name)
    }
}

/// A callback that wakes an event loop (the loop then re-checks fonts and appearance).
pub(crate) type Waker = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct Inner {
    files: HashMap<PathBuf, Option<&'static [u8]>>,
    fallbacks: Vec<Fallback>,
    /// Characters no available face covers (never searched again).
    uncoverable: HashSet<char>,
}

/// Process-wide font registry.
pub(crate) struct FontRegistry {
    inner: Mutex<Inner>,
    /// Bumped whenever something that may resolve earlier misses happens (fontdb scan finished).
    generation: AtomicU64,
    wakers: Mutex<Vec<Waker>>,
    #[cfg(target_os = "linux")]
    scan: linux::Scan,
}

impl FontRegistry {
    /// The process-wide registry.
    pub(crate) fn global() -> &'static FontRegistry {
        static REG: OnceLock<FontRegistry> = OnceLock::new();
        REG.get_or_init(|| FontRegistry { inner: Mutex::new(Inner::default()),
                                          generation: AtomicU64::new(0),
                                          wakers: Mutex::new(Vec::new()),
                                          #[cfg(target_os = "linux")]
                                          scan: linux::Scan::default() })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Load a font file from disk once per process and return its bytes as `&'static` (leaked).
    /// `None` if the file is missing, unreadable or not a valid font (no face parses or has
    /// outlines). Results, including failures, are cached.
    pub(crate) fn load_file(&self, path: &Path) -> Option<&'static [u8]> {
        if let Some(cached) = self.lock().files.get(path) {
            return *cached;
        }
        // Read outside the lock (large files); a racing reader just wastes one read.
        let loaded = std::fs::read(path).ok().filter(|b| file_is_valid(b)).map(|b| &*Box::leak(b.into_boxed_slice()));
        let mut inner = self.lock();
        *inner.files.entry(path.to_path_buf()).or_insert(loaded)
    }

    /// Load face `index` of a font file (see [`FontRegistry::load_file`]); `None` unless that face
    /// validates.
    pub(crate) fn load_face(&self, path: &Path, index: u32) -> Option<FaceRef> {
        let face = FaceRef::new(self.load_file(path)?, index);
        validate_face(&face).then_some(face)
    }

    /// A font from the Windows fonts directory (`%WINDIR%\Fonts\<file>`), validated. Always `None`
    /// on other platforms.
    pub(crate) fn windows_font(&self, file: &str, index: u32) -> Option<FaceRef> {
        let dir = windows_fonts_dir()?;
        self.load_face(&dir.join(file), index)
    }

    /// Fallback faces found so far in this process, in discovery order.
    pub(crate) fn fallbacks(&self) -> Vec<Fallback> {
        self.lock().fallbacks.clone()
    }

    /// Changes when a background discovery step finished (see [`FontRegistry::ensure_coverage`]).
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Register a waker called (from any thread) when [`FontRegistry::generation`] changes.
    pub(crate) fn add_waker(&self, waker: Waker) {
        self.wakers.lock().unwrap_or_else(|e| e.into_inner()).push(waker);
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))] // only the Linux fontdb scan is asynchronous
    fn bump_generation(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        let wakers = self.wakers.lock().unwrap_or_else(|e| e.into_inner()).clone();
        for w in wakers {
            w();
        }
    }

    /// Start background discovery (the Linux `fontdb` scan). Cheap and idempotent; call when the
    /// first dialog request arrives. No-op on Windows.
    pub(crate) fn start_background_scan(&'static self) {
        #[cfg(target_os = "linux")]
        self.scan.start(move || self.bump_generation());
    }

    /// Make sure every visible character of `texts` is covered by the theme's faces or a
    /// registered fallback, discovering and registering new fallbacks as needed. `wait`: on a
    /// miss, wait up to 300 ms for the Linux system-font scan (never on later retries). Returns
    /// whether every character is covered or known to be uncoverable (`false` while a Linux scan
    /// that could still resolve a miss is running: retry when [`FontRegistry::generation`]
    /// changes).
    pub(crate) fn ensure_coverage(&self, fonts: &ThemeFonts, texts: &[&str], wait: bool) -> bool {
        let mut faces: Vec<FontRef<'static>> = [fonts.regular.face, fonts.bold.face].iter().filter_map(FaceRef::font_ref).collect();
        let (fallbacks, uncoverable) = {
            let inner = self.lock();
            (inner.fallbacks.clone(), inner.uncoverable.clone())
        };
        faces.extend(fallbacks.iter().filter_map(|f| f.face.font_ref()));

        let mut seen = HashSet::new();
        let mut misses: Vec<char> = Vec::new();
        for c in texts.iter().flat_map(|t| t.chars()) {
            if is_ignorable(c) || !seen.insert(c) || uncoverable.contains(&c) {
                continue;
            }
            if !faces.iter().any(|f| f.charmap().map(c).is_some()) {
                misses.push(c);
            }
        }
        if misses.is_empty() {
            return true;
        }

        let mut complete = true;
        let mut ctx = self.discovery(wait);
        for c in misses {
            if faces.iter().any(|f| f.charmap().map(c).is_some()) {
                continue; // covered by a face found for an earlier miss
            }
            match self.discover(&mut ctx, c) {
                Discovered::Face(fb) => {
                    if let Some(f) = fb.face.font_ref() {
                        faces.push(f);
                    }
                    let mut inner = self.lock();
                    if !inner.fallbacks.iter().any(|x| x.name == fb.name) {
                        inner.fallbacks.push(fb);
                    }
                }
                Discovered::Never => {
                    self.lock().uncoverable.insert(c);
                }
                Discovered::NotYet => complete = false,
            }
        }
        complete
    }
}

enum Discovered {
    Face(Fallback),
    /// No face on this system covers the character.
    Never,
    /// Discovery isn't finished (Linux scan still running).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))] // only the Linux fontdb scan is asynchronous
    NotYet,
}

/// Per-`ensure_coverage` discovery state.
struct DiscoveryCtx {
    #[cfg(target_os = "linux")]
    db: Option<Arc<fontdb::Database>>,
    #[cfg(target_os = "linux")]
    budget: u64,
    #[cfg(windows)]
    locale: String,
}

impl FontRegistry {
    fn discovery(&self, wait: bool) -> DiscoveryCtx {
        let _ = wait;
        DiscoveryCtx { #[cfg(target_os = "linux")]
                       db: self.scan.get(if wait { Duration::from_millis(300) } else { Duration::ZERO }),
                       #[cfg(target_os = "linux")]
                       budget: 64 << 20,
                       #[cfg(windows)]
                       locale: super::platform_win::user_locale().unwrap_or_default() }
    }

    fn discover(&self, ctx: &mut DiscoveryCtx, c: char) -> Discovered {
        #[cfg(windows)]
        {
            let Some(dir) = windows_fonts_dir() else { return Discovered::Never };
            for (file, index) in windows_candidates(c, &ctx.locale) {
                let path = dir.join(file);
                if let Some(face) = self.load_face(&path, index) {
                    if face.covers(c) {
                        let bold = windows_bold_counterpart(file, index).and_then(|(f, i)| self.load_face(&dir.join(f), i));
                        return Discovered::Face(Fallback { name: format!("fallback:{file}#{index}"), face, bold });
                    }
                }
            }
            Discovered::Never
        }
        #[cfg(target_os = "linux")]
        {
            let Some(db) = ctx.db.clone() else {
                return if self.scan.finished() { Discovered::Never } else { Discovered::NotYet };
            };
            match linux::find_face(self, &db, c, &mut ctx.budget) {
                Some((path, index, face, bold)) => {
                    Discovered::Face(Fallback { name: format!("fallback:{}#{index}", path.display()), face, bold })
                }
                None => Discovered::Never,
            }
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = (ctx, c);
            Discovered::Never
        }
    }
}

/// A file is valid when at least one face in it validates.
fn file_is_valid(bytes: &[u8]) -> bool {
    match skrifa::raw::FileRef::new(bytes) {
        Ok(file) => file.fonts().any(|f| f.is_ok_and(|f| has_outlines(&f))),
        Err(_) => false,
    }
}

/// The face parses, has a character map and has vector outlines (glyf / CFF / CFF2). Bitmap-only
/// faces (CBDT colour emoji) are rejected: epaint 0.36 cannot draw them.
pub(crate) fn validate_face(face: &FaceRef) -> bool {
    face.font_ref().is_some_and(|f| has_outlines(&f))
}

fn has_outlines(f: &FontRef<'_>) -> bool {
    f.outline_glyphs().format().is_some() && f.charmap().has_map()
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

/// `%WINDIR%\Fonts` (Windows only).
fn windows_fonts_dir() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let windir = std::env::var_os("WINDIR").or_else(|| std::env::var_os("SystemRoot")).unwrap_or_else(|| "C:\\Windows".into());
    Some(PathBuf::from(windir).join("Fonts"))
}

/// Windows known-path fallback table, most specific first.
#[cfg(any(windows, test))]
fn windows_candidates(c: char, locale: &str) -> Vec<(&'static str, u32)> {
    let u = c as u32;
    let cjk = |hangul: bool| -> Vec<(&'static str, u32)> {
        let sc = ("msyh.ttc", 0);
        let tc = ("msjh.ttc", 0);
        // Yu Gothic Regular first: a weight-400 "Yu Gothic" lookup (as the system font lookup does)
        // resolves to Regular, not Medium; Medium is the fallback for systems without it.
        let jp = [("YuGothR.ttc", 0), ("YuGothM.ttc", 0), ("meiryo.ttc", 0)];
        let kr = ("malgun.ttf", 0);
        let loc = locale.to_ascii_lowercase();
        let mut v: Vec<(&'static str, u32)> = Vec::new();
        if hangul || loc.starts_with("ko") {
            v.push(kr);
        }
        if loc.starts_with("ja") {
            v.extend(jp);
        }
        if loc.starts_with("zh-tw") || loc.starts_with("zh-hk") || loc.starts_with("zh-mo") || loc.contains("hant") {
            v.push(tc);
        }
        v.push(sc);
        v.extend(jp);
        v.push(kr);
        v.push(tc);
        let mut seen = HashSet::new();
        v.retain(|x| seen.insert(*x));
        v
    };
    let segoe = ("segoeui.ttf", 0);
    let sym = ("seguisym.ttf", 0);
    let emoji = ("seguiemj.ttf", 0);
    let mut v: Vec<(&'static str, u32)> = match u {
        // Latin extended, IPA, Greek, Cyrillic, Armenian, Hebrew, Arabic, Georgian, Vietnamese
        0x0080..=0x058F | 0x10A0..=0x10FF | 0x1C80..=0x1CBF | 0x1D00..=0x1FFF | 0x2C60..=0x2C7F | 0x2DE0..=0x2DFF
        | 0xA640..=0xA69F | 0xA720..=0xA7FF => vec![segoe],
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF => vec![segoe, ("tahoma.ttf", 0)],
        0x0900..=0x0DFF => vec![("Nirmala.ttc", 0), ("Nirmala.ttf", 0), ("NirmalaUI.ttf", 0)],
        0x0E00..=0x0EFF => vec![("leelawui.ttf", 0), ("LeelawUI.ttf", 0)],
        0x1200..=0x139F | 0x2D80..=0x2DDF => vec![("ebrima.ttf", 0)],
        0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7FF => cjk(true),
        0x2E80..=0x2FFF | 0x3000..=0x312F | 0x3190..=0x9FFF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF
        | 0x20000..=0x3FFFF => cjk(false),
        // Emoji and pictographs.
        0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2300..=0x23FF | 0x2B00..=0x2BFF => vec![emoji, sym],
        // Arrows, maths, box drawing, geometric shapes, misc symbols.
        0x2000..=0x22FF | 0x2400..=0x25FF | 0x2900..=0x2AFF => vec![segoe, sym, emoji],
        _ => vec![],
    };
    // Generic last resorts.
    for extra in [segoe, sym, emoji, ("msyh.ttc", 0), ("malgun.ttf", 0), ("Nirmala.ttc", 0), ("ebrima.ttf", 0), ("arialuni.ttf", 0)] {
        if !v.contains(&extra) {
            v.push(extra);
        }
    }
    v
}

/// The bold face of the family of a [`windows_candidates`] entry (same TTC index unless noted).
#[cfg(any(windows, test))]
fn windows_bold_counterpart(file: &str, index: u32) -> Option<(&'static str, u32)> {
    Some(match file.to_ascii_lowercase().as_str() {
        "msyh.ttc" => ("msyhbd.ttc", index),
        "msjh.ttc" => ("msjhbd.ttc", index),
        // YuGothR/YuGothM #0 "Yu Gothic Regular/Medium" / #1 "Yu Gothic UI" -> YuGothB #0 "Yu Gothic Bold" / #1 "Yu Gothic UI Bold".
        "yugothm.ttc" | "yugothr.ttc" => ("YuGothB.ttc", index),
        "meiryo.ttc" => ("meiryob.ttc", index),
        "malgun.ttf" => ("malgunbd.ttf", 0),
        "segoeui.ttf" => ("segoeuib.ttf", 0),
        "tahoma.ttf" => ("tahomabd.ttf", 0),
        "ebrima.ttf" => ("ebrimabd.ttf", 0),
        "leelawui.ttf" => ("LeelaUIb.ttf", 0),
        // Nirmala.ttc: #0 Nirmala UI, #1 Nirmala UI Bold (Windows 10+); older systems ship separate files.
        "nirmala.ttc" if index == 0 => ("Nirmala.ttc", 1),
        "nirmala.ttf" | "nirmalaui.ttf" => ("NirmalaB.ttf", 0),
        _ => return None,
    })
}

#[cfg(target_os = "linux")]
mod linux {
    //! fontdb system scan on a background thread + face lookup.

    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    use skrifa::{FontRef, MetadataProvider};

    use super::{FaceRef, FontRegistry};

    #[derive(Default)]
    enum State {
        #[default]
        NotStarted,
        Running,
        Done(Arc<fontdb::Database>),
    }

    #[derive(Default)]
    pub(super) struct Scan {
        state: Mutex<State>,
        cv: Condvar,
    }

    impl Scan {
        pub(super) fn start(&'static self, on_done: impl FnOnce() + Send + 'static) {
            {
                let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
                if !matches!(*st, State::NotStarted) {
                    return;
                }
                *st = State::Running;
            }
            let spawned = std::thread::Builder::new().name("xdialog-fontdb".into()).spawn(move || {
                let t0 = Instant::now();
                // A panic inside the scan must still leave `Running`, or every coverage check
                // would wait for it for the rest of the process.
                let scanned = std::panic::catch_unwind(|| {
                    let mut db = fontdb::Database::new();
                    db.load_system_fonts();
                    db
                });
                let db = scanned.unwrap_or_else(|_| {
                                    warn!("xdialog: the system font scan panicked; continuing without system fonts");
                                    fontdb::Database::new()
                                });
                debug!("xdialog: fontdb scanned {} faces in {:?}", db.len(), t0.elapsed());
                *self.state.lock().unwrap_or_else(|e| e.into_inner()) = State::Done(Arc::new(db));
                self.cv.notify_all();
                on_done();
            });
            if let Err(e) = spawned {
                warn!("xdialog: could not start the font scan thread: {e}");
                *self.state.lock().unwrap_or_else(|e| e.into_inner()) = State::Done(Arc::new(fontdb::Database::new()));
                self.cv.notify_all();
            }
        }

        pub(super) fn finished(&self) -> bool {
            matches!(*self.state.lock().unwrap_or_else(|e| e.into_inner()), State::Done(_))
        }

        /// The scanned database, waiting up to `wait` for a running scan.
        pub(super) fn get(&self, wait: Duration) -> Option<Arc<fontdb::Database>> {
            let deadline = Instant::now() + wait;
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                match &*st {
                    State::Done(db) => return Some(db.clone()),
                    State::NotStarted => return None,
                    State::Running => {
                        let now = Instant::now();
                        if now >= deadline {
                            return None;
                        }
                        st = self.cv.wait_timeout(st, deadline - now).unwrap_or_else(|e| e.into_inner()).0;
                    }
                }
            }
        }
    }

    /// Preferred fallback families, CJK ordered by `$LANG`.
    fn preferred_families() -> Vec<&'static str> {
        let lang = std::env::var("LC_ALL").or_else(|_| std::env::var("LC_CTYPE")).or_else(|_| std::env::var("LANG")).unwrap_or_default().to_ascii_lowercase();
        let mut cjk = vec!["Noto Sans CJK SC", "Noto Sans CJK JP", "Noto Sans CJK KR", "Noto Sans CJK TC"];
        let first = if lang.starts_with("ja") {
            Some("Noto Sans CJK JP")
        } else if lang.starts_with("ko") {
            Some("Noto Sans CJK KR")
        } else if lang.starts_with("zh_tw") || lang.starts_with("zh_hk") {
            Some("Noto Sans CJK TC")
        } else {
            None
        };
        if let Some(f) = first {
            cjk.retain(|x| *x != f);
            cjk.insert(0, f);
        }
        let mut v = cjk;
        v.extend(["Noto Sans Arabic",
                  "Noto Sans Hebrew",
                  "Noto Sans Devanagari",
                  "Noto Sans Bengali",
                  "Noto Sans Tamil",
                  "Noto Sans Thai",
                  "Noto Sans Ethiopic",
                  "Noto Sans Armenian",
                  "Noto Sans Georgian",
                  "Noto Sans",
                  "DejaVu Sans",
                  "Noto Sans Symbols",
                  "Noto Sans Symbols2",
                  "Noto Emoji",
                  "Symbola"]);
        v
    }

    fn rank(f: &fontdb::FaceInfo) -> (u32, u16) {
        let style = if f.style == fontdb::Style::Normal { 0 } else { 1 };
        (style, f.weight.0.abs_diff(400))
    }

    /// The bold face (weight nearest 700, at least 600, upright preferred) of `of`'s family.
    fn bold_sibling(reg: &FontRegistry, db: &fontdb::Database, of: &fontdb::FaceInfo) -> Option<FaceRef> {
        let family = &of.families.first()?.0;
        let mut faces: Vec<&fontdb::FaceInfo> =
            db.faces().filter(|f| f.weight.0 >= 600 && f.families.iter().any(|(n, _)| n == family)).collect();
        faces.sort_by_key(|f| (f.style != fontdb::Style::Normal, f.weight.0.abs_diff(700), std::cmp::Reverse(f.weight.0)));
        faces.into_iter().find_map(|f| match &f.source {
                             fontdb::Source::File(path) | fontdb::Source::SharedFile(path, _) => reg.load_face(path, f.index),
                             _ => None,
                         })
    }

    /// Find a validated face covering `c` (plus its family's bold face): preferred families first,
    /// then any face (reading at most `budget` bytes of candidate files).
    pub(super) fn find_face(reg: &FontRegistry, db: &fontdb::Database, c: char, budget: &mut u64) -> Option<(PathBuf, u32, FaceRef, Option<FaceRef>)> {
        for family in preferred_families() {
            let mut faces: Vec<&fontdb::FaceInfo> = db.faces().filter(|f| f.families.iter().any(|(n, _)| n == family)).collect();
            faces.sort_by_key(|f| rank(f));
            for f in faces {
                if let fontdb::Source::File(path) | fontdb::Source::SharedFile(path, _) = &f.source {
                    if let Some(face) = reg.load_face(path, f.index) {
                        if face.covers(c) {
                            return Some((path.clone(), f.index, face, bold_sibling(reg, db, f)));
                        }
                    }
                }
            }
        }
        // Any face: regular upright faces first.
        let mut faces: Vec<&fontdb::FaceInfo> = db.faces().collect();
        faces.sort_by_key(|f| rank(f));
        // Candidate file bytes read during this lookup (not leaked; only the winner is loaded
        // through the registry).
        let mut read: HashMap<&Path, Option<Vec<u8>>> = HashMap::new();
        for f in faces {
            let path = match &f.source {
                fontdb::Source::File(p) | fontdb::Source::SharedFile(p, _) => p,
                _ => continue,
            };
            let covers = match &f.source {
                fontdb::Source::SharedFile(_, data) => covers_bytes((**data).as_ref(), f.index, c),
                _ => {
                    let bytes = read.entry(path.as_path()).or_insert_with(|| {
                                                              let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX);
                                                              if len > *budget {
                                                                  return None;
                                                              }
                                                              *budget -= len;
                                                              std::fs::read(path).ok()
                                                          });
                    bytes.as_deref().is_some_and(|b| covers_bytes(b, f.index, c))
                }
            };
            if covers {
                if let Some(face) = reg.load_face(path, f.index) {
                    return Some((path.clone(), f.index, face, bold_sibling(reg, db, f)));
                }
            }
        }
        None
    }

    fn covers_bytes(bytes: &[u8], index: u32, c: char) -> bool {
        FontRef::from_index(bytes, index).is_ok_and(|f| f.charmap().map(c).is_some() && f.outline_glyphs().format().is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ubuntu() -> ThemeFonts {
        ThemeFonts::new(bundled::ubuntu_regular(), bundled::ubuntu_bold())
    }

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
    fn bundled_faces_validate_and_cover_latin() {
        let r = bundled::ubuntu_regular();
        assert!(validate_face(&r));
        assert!(r.covers('A'));
        assert!(!r.covers('中'));
        let reg = FontRegistry::global();
        // Latin-only text needs nothing.
        assert!(reg.ensure_coverage(&ubuntu(), &["Hello, world!\n\u{200D}"], false));
    }

    #[test]
    fn truncated_font_is_rejected() {
        let dir = std::env::temp_dir().join(format!("xdialog-font-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("truncated.ttf");
        std::fs::write(&path, &bundled::UBUNTU_REGULAR[..2000]).unwrap();
        let reg = FontRegistry::global();
        assert!(reg.load_file(&path).is_none());
        assert!(reg.load_face(&path, 0).is_none());
        let junk = dir.join("junk.ttf");
        std::fs::write(&junk, b"not a font at all").unwrap();
        assert!(reg.load_file(&junk).is_none());
        let good = dir.join("good.ttf");
        std::fs::write(&good, bundled::UBUNTU_REGULAR).unwrap();
        assert!(reg.load_face(&good, 0).is_some());
        assert!(reg.load_face(&good, 1).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn windows_table_orders_cjk_by_locale() {
        assert_eq!(windows_candidates('中', "ja-JP")[0], ("YuGothR.ttc", 0));
        assert_eq!(windows_candidates('中', "zh-TW")[0], ("msjh.ttc", 0));
        assert_eq!(windows_candidates('中', "en-US")[0], ("msyh.ttc", 0));
        assert_eq!(windows_candidates('한', "en-US")[0], ("malgun.ttf", 0));
        assert_eq!(windows_candidates('ש', "en-US")[0], ("segoeui.ttf", 0));
    }

    #[cfg(windows)]
    #[test]
    fn windows_resolves_cjk_fallback() {
        let reg = FontRegistry::global();
        if reg.windows_font("msyh.ttc", 0).is_none() && reg.windows_font("YuGothM.ttc", 0).is_none() {
            return; // no CJK fonts installed
        }
        assert!(reg.ensure_coverage(&ubuntu(), &["你好"], false));
        let fb = reg.fallbacks().into_iter().find(|f| f.face.covers('你')).unwrap();
        // The bold chain gets the family's bold face (msyhbd / YuGothB / ...).
        let bold = fb.bold.expect("CJK fallback has a bold face");
        assert!(bold.covers('你'));
        assert_ne!(bold, fb.face);
    }

    #[test]
    fn windows_bold_table_covers_candidates() {
        for (file, index) in windows_candidates('中', "en-US").into_iter().chain(windows_candidates('ש', "en-US")) {
            if ["seguisym.ttf", "seguiemj.ttf", "arialuni.ttf"].contains(&file) {
                continue; // single-weight families
            }
            assert!(windows_bold_counterpart(file, index).is_some(), "{file}#{index}");
        }
        assert_eq!(windows_bold_counterpart("msyh.ttc", 1), Some(("msyhbd.ttc", 1)));
        assert_eq!(windows_bold_counterpart("Nirmala.ttc", 0), Some(("Nirmala.ttc", 1)));
    }
}
