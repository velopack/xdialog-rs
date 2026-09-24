//! Light/dark scheme and accent colour.
//!
//! - **Linux:** the XDG desktop portal (`org.freedesktop.appearance` `color-scheme` and
//!   `accent-color`), read on a background thread `xdialog-portal` that keeps one session-bus
//!   connection, subscribes to `SettingChanged` and updates a cache. The UI thread never talks to
//!   D-Bus: it reads the cache, waiting at most 150 ms for the very first read.
//! - **Windows:** `AppsUseLightTheme` and `Explorer\Accent\AccentPalette` from the registry, read
//!   inline on each dialog open (microseconds). No WinRT.
//! - Test builds (`debug_assertions` or `_test-hooks`): `XDIALOG_TEST_ACCENT=RRGGBB|none` overrides
//!   the accent (`AccentSource::Test`).

use egui::Color32;

use super::color::parse_hex;
use super::testhooks::test_env_enabled;
use crate::model::XDialogTheme;

/// Where an accent colour came from. Themes decide which sources they honour
/// (linux: `Portal` + `Test`; fluent: `WindowsRegistry` + `Test`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AccentSource {
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))] // the portal only exists on Linux
    Portal,
    #[cfg_attr(not(windows), allow(dead_code))] // the registry only exists on Windows
    WindowsRegistry,
    Test,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Accent {
    /// SystemAccentColor / portal accent.
    pub base: Color32,
    /// Windows AccentPalette order: `[Light3, Light2, Light1, Accent, Dark1, Dark2, Dark3]`.
    pub win_palette: Option<[Color32; 7]>,
    pub source: AccentSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) struct Appearance {
    pub dark: bool,
    pub accent: Option<Accent>,
}

impl Appearance {
    /// An injected appearance (offscreen harness / `__test`): `AccentSource::Test`. The base is
    /// `accent`, else the palette's `Accent` entry.
    pub(crate) fn test(dark: bool, accent: Option<[u8; 3]>, palette: Option<[[u8; 3]; 7]>) -> Appearance {
        let rgb = |c: [u8; 3]| Color32::from_rgb(c[0], c[1], c[2]);
        let win_palette = palette.map(|p| p.map(rgb));
        let base = accent.map(rgb).or(win_palette.map(|p| p[3]));
        Appearance { dark, accent: base.map(|base| Accent { base, win_palette, source: AccentSource::Test }) }
    }
}

/// Read the cached platform appearance (never blocks, except up to 150 ms for the very first
/// portal read on Linux), then apply the `XDialogTheme` override (forces `dark`, keeps the
/// accent), then the test env overrides (test builds only).
pub(crate) fn resolve_appearance(theme: &XDialogTheme) -> Appearance {
    let mut a = platform::read();
    match theme {
        XDialogTheme::SystemDefault => {}
        XDialogTheme::Light => a.dark = false,
        XDialogTheme::Dark => a.dark = true,
    }
    if test_env_enabled() {
        apply_test_env(&mut a, |k| std::env::var(k).ok());
    }
    a
}

/// Start watching for appearance changes (Linux portal thread; no-op elsewhere). `waker` is called
/// from the watcher thread whenever the cached appearance changes; the loop then re-resolves the
/// appearance of every open dialog. Idempotent per waker registration.
pub(crate) fn watch(waker: super::fonts::Waker) {
    platform::watch(waker);
}

/// Apply `XDIALOG_TEST_ACCENT` (read through `var`).
fn apply_test_env(a: &mut Appearance, var: impl Fn(&str) -> Option<String>) {
    let Some(accent) = var("XDIALOG_TEST_ACCENT") else { return };
    if accent.trim().eq_ignore_ascii_case("none") {
        a.accent = None;
    } else if let Some(base) = parse_hex(&accent) {
        a.accent = Some(Accent { base, win_palette: None, source: AccentSource::Test });
    }
}

/// Parse the 32-byte `AccentPalette` REG_BINARY (8 x RGBA; entries 0..7 are
/// `[L3, L2, L1, Accent, D1, D2, D3]`).
#[cfg_attr(not(windows), allow(dead_code))] // Windows reader + tests
pub(crate) fn parse_accent_palette(bytes: &[u8]) -> Option<[Color32; 7]> {
    if bytes.len() < 28 {
        return None;
    }
    let mut out = [Color32::BLACK; 7];
    for (i, c) in out.iter_mut().enumerate() {
        let p = &bytes[i * 4..i * 4 + 4];
        *c = Color32::from_rgb(p[0], p[1], p[2]);
    }
    Some(out)
}

#[cfg(windows)]
mod platform {
    use super::{parse_accent_palette, Accent, AccentSource, Appearance};
    use crate::backends::egui_core::platform_win;

    pub(super) fn read() -> Appearance {
        const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
        const ACCENT: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent";
        let dark = platform_win::read_hkcu_dword(PERSONALIZE, "AppsUseLightTheme") == Some(0);
        let accent = platform_win::read_hkcu_binary(ACCENT, "AccentPalette").and_then(|b| parse_accent_palette(&b))
                                                                              .map(|p| Accent { base: p[3],
                                                                                                win_palette: Some(p),
                                                                                                source: AccentSource::WindowsRegistry });
        Appearance { dark, accent }
    }

    pub(super) fn watch(_waker: crate::backends::egui_core::fonts::Waker) {
        // Windows: re-read on each dialog open, on ThemeChanged (the window is created without a
        // preferred theme when following the system, so winit reports it) and on focus (accent
        // changes have no event); nothing to watch.
    }
}

#[cfg(target_os = "linux")]
mod platform {
    //! Portal reader thread.

    use std::sync::{Condvar, Mutex, OnceLock};
    use std::time::{Duration, Instant};

    use egui::Color32;

    use super::{Accent, AccentSource, Appearance};
    use crate::backends::egui_core::fonts::Waker;

    #[derive(Default)]
    struct Cache {
        started: bool,
        /// `None` until the first read finished (unknown = light, no accent).
        value: Option<Appearance>,
        wakers: Vec<Waker>,
    }

    fn cache() -> &'static (Mutex<Cache>, Condvar) {
        static C: OnceLock<(Mutex<Cache>, Condvar)> = OnceLock::new();
        C.get_or_init(|| (Mutex::new(Cache::default()), Condvar::new()))
    }

    pub(super) fn read() -> Appearance {
        start();
        let (m, cv) = cache();
        let deadline = Instant::now() + Duration::from_millis(150);
        let mut c = m.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(v) = c.value {
                return v;
            }
            let now = Instant::now();
            if now >= deadline {
                return Appearance::default();
            }
            c = cv.wait_timeout(c, deadline - now).unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    pub(super) fn watch(waker: Waker) {
        start();
        cache().0.lock().unwrap_or_else(|e| e.into_inner()).wakers.push(waker);
    }

    fn start() {
        let (m, cv) = cache();
        {
            let mut c = m.lock().unwrap_or_else(|e| e.into_inner());
            if c.started {
                return;
            }
            c.started = true;
        }
        let spawned = std::thread::Builder::new().name("xdialog-portal".into()).spawn(|| {
            if let Err(e) = run() {
                debug!("xdialog: appearance portal unavailable: {e}");
            }
            // Whatever happened, unblock readers waiting for the first value.
            let (m, cv) = cache();
            let mut c = m.lock().unwrap_or_else(|e| e.into_inner());
            if c.value.is_none() {
                c.value = Some(Appearance::default());
            }
            cv.notify_all();
        });
        if spawned.is_err() {
            m.lock().unwrap_or_else(|e| e.into_inner()).value = Some(Appearance::default());
            cv.notify_all();
        }
    }

    fn publish(a: Appearance) {
        let (m, cv) = cache();
        let wakers = {
            let mut c = m.lock().unwrap_or_else(|e| e.into_inner());
            let changed = c.value != Some(a);
            c.value = Some(a);
            if changed {
                c.wakers.clone()
            } else {
                Vec::new()
            }
        };
        cv.notify_all();
        for w in wakers {
            w();
        }
    }

    fn run() -> zbus::Result<()> {
        use zbus::blocking::{connection, Proxy};

        let conn = connection::Builder::session()?.method_timeout(Duration::from_millis(500)).build()?;
        let proxy = Proxy::new(&conn,
                               "org.freedesktop.portal.Desktop",
                               "/org/freedesktop/portal/desktop",
                               "org.freedesktop.portal.Settings")?;
        let mut current = Appearance { dark: read_color_scheme(&proxy), accent: read_accent(&proxy) };
        publish(current);

        // Live updates (xdialog's UI thread may live for the whole process in linux-direct mode).
        let signals = proxy.receive_signal("SettingChanged")?;
        for msg in signals {
            let Ok((ns, key, value)) = msg.body().deserialize::<(String, String, zbus::zvariant::OwnedValue)>() else {
                continue;
            };
            if ns != "org.freedesktop.appearance" {
                continue;
            }
            match key.as_str() {
                "color-scheme" => current.dark = color_scheme_from(&value),
                "accent-color" => current.accent = accent_from(&value),
                _ => continue,
            }
            publish(current);
        }
        Ok(())
    }

    fn read_color_scheme(proxy: &zbus::blocking::Proxy) -> bool {
        read_setting(proxy, "color-scheme").is_some_and(|v| color_scheme_from(&v))
    }

    fn read_accent(proxy: &zbus::blocking::Proxy) -> Option<Accent> {
        let value = read_setting(proxy, "accent-color")?;
        accent_from(&value)
    }

    /// Spec: 0 = no preference, 1 = prefer dark, 2 = prefer light.
    fn color_scheme_from(value: &zbus::zvariant::Value<'_>) -> bool {
        matches!(deep_unwrap(value), zbus::zvariant::Value::U32(1))
    }

    /// Spec: a struct of three doubles (r, g, b) in 0..=1, or out of range for "no preference".
    fn accent_from(value: &zbus::zvariant::Value<'_>) -> Option<Accent> {
        let zbus::zvariant::Value::Structure(s) = deep_unwrap(value) else {
            return None;
        };
        let f = s.fields();
        if f.len() != 3 {
            return None;
        }
        let comp = |v: &zbus::zvariant::Value<'_>| match v {
            zbus::zvariant::Value::F64(x) => Some(*x),
            _ => None,
        };
        let (r, g, b) = (comp(&f[0])?, comp(&f[1])?, comp(&f[2])?);
        if [r, g, b].iter().any(|c| !(0.0..=1.0).contains(c)) {
            return None;
        }
        let u = |c: f64| (c * 255.0).round() as u8;
        Some(Accent { base: Color32::from_rgb(u(r), u(g), u(b)), win_palette: None, source: AccentSource::Portal })
    }

    /// `ReadOne` (Settings v2) first, then the legacy double-wrapped `Read`.
    fn read_setting(proxy: &zbus::blocking::Proxy, key: &str) -> Option<zbus::zvariant::OwnedValue> {
        const NS: &str = "org.freedesktop.appearance";
        if let Ok(v) = proxy.call::<_, _, zbus::zvariant::OwnedValue>("ReadOne", &(NS, key)) {
            return Some(v);
        }
        proxy.call::<_, _, zbus::zvariant::OwnedValue>("Read", &(NS, key)).ok()
    }

    fn deep_unwrap<'a, 'v>(value: &'a zbus::zvariant::Value<'v>) -> &'a zbus::zvariant::Value<'v> {
        match value {
            zbus::zvariant::Value::Value(inner) => deep_unwrap(inner),
            other => other,
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use super::Appearance;

    pub(super) fn read() -> Appearance {
        Appearance::default()
    }

    pub(super) fn watch(_waker: crate::backends::egui_core::fonts::Waker) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::egui_core::color::rgb;

    #[test]
    fn accent_palette_bytes() {
        let mut bytes = Vec::new();
        for i in 0..8u8 {
            bytes.extend([i, i + 10, i + 20, 0xFF]);
        }
        let p = parse_accent_palette(&bytes).unwrap();
        assert_eq!(p[0], Color32::from_rgb(0, 10, 20));
        assert_eq!(p[3], Color32::from_rgb(3, 13, 23));
        assert_eq!(p[6], Color32::from_rgb(6, 16, 26));
        assert!(parse_accent_palette(&bytes[..20]).is_none());
    }

    #[test]
    fn theme_override_keeps_accent() {
        let dark = resolve_appearance(&XDialogTheme::Dark);
        let light = resolve_appearance(&XDialogTheme::Light);
        assert!(dark.dark);
        assert!(!light.dark);
        assert_eq!(dark.accent, light.accent);
    }

    #[test]
    fn test_env_parsing() {
        let mut a = Appearance::default();
        apply_test_env(&mut a, |k| (k == "XDIALOG_TEST_ACCENT").then(|| "#A94DC1".to_string()));
        assert_eq!(a.accent.map(|x| (x.base, x.source)), Some((rgb(0xA94DC1), AccentSource::Test)));

        let mut a = Appearance { dark: true, accent: Some(Accent { base: rgb(1), win_palette: None, source: AccentSource::Portal }) };
        apply_test_env(&mut a, |k| (k == "XDIALOG_TEST_ACCENT").then(|| "none".to_string()));
        assert_eq!(a, Appearance { dark: true, accent: None });

        // Malformed values are ignored.
        let mut a = Appearance::default();
        apply_test_env(&mut a, |k| (k == "XDIALOG_TEST_ACCENT").then(|| "zz".to_string()));
        assert_eq!(a, Appearance::default());
    }

    #[test]
    fn test_env_is_gated() {
        // Unit tests are debug builds: gated on.
        assert_eq!(test_env_enabled(), cfg!(any(debug_assertions, xd_test_hooks)));
    }

    #[test]
    fn injected_appearance() {
        let a = Appearance::test(true, None, Some([[1, 1, 1], [2, 2, 2], [3, 3, 3], [4, 4, 4], [5, 5, 5], [6, 6, 6], [7, 7, 7]]));
        assert!(a.dark);
        assert_eq!(a.accent.unwrap().base, Color32::from_rgb(4, 4, 4));
        assert_eq!(Appearance::test(false, None, None).accent, None);
    }
}
