//! Best-effort detection of the user's desktop appearance (light/dark + accent color).
//!
//! On Linux this reads the standardized XDG desktop portal `org.freedesktop.portal.Settings`
//! interface (`org.freedesktop.appearance` namespace), which GNOME, KDE Plasma and most other
//! desktops expose through `xdg-desktop-portal`. Any failure — no portal, no D-Bus session, an
//! unimplemented key, or "no preference" — falls back to the hard-coded Ubuntu light theme, so
//! this is purely additive.
//!
//! On non-Linux platforms detection is a no-op that always reports no preference.

use crate::model::XDialogTheme;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ColorScheme {
    /// No explicit preference; callers should use their default (light) theme.
    #[default]
    NoPreference,
    Light,
    Dark,
}

/// The user's resolved desktop appearance preferences.
#[derive(Debug, Clone, Copy, Default)]
pub struct DesktopAppearance {
    pub color_scheme: ColorScheme,
    /// Accent color as 8-bit RGB, if the desktop exposes one.
    pub accent_color: Option<(u8, u8, u8)>,
}

/// Resolve the appearance to use for a dialog given the requested [`XDialogTheme`].
///
/// The desktop's accent color is always detected and honored. `SystemDefault` also takes the
/// desktop's light/dark preference; `Light`/`Dark` force the scheme but keep the accent.
pub fn resolve_appearance(theme: XDialogTheme) -> DesktopAppearance {
    let mut appearance = detect_appearance();
    match theme {
        XDialogTheme::SystemDefault => {}
        XDialogTheme::Light => appearance.color_scheme = ColorScheme::Light,
        XDialogTheme::Dark => appearance.color_scheme = ColorScheme::Dark,
    }
    appearance
}

#[cfg(target_os = "linux")]
fn detect_appearance() -> DesktopAppearance {
    detect_via_portal().unwrap_or_default()
}

#[cfg(not(target_os = "linux"))]
fn detect_appearance() -> DesktopAppearance {
    DesktopAppearance::default()
}

#[cfg(target_os = "linux")]
fn detect_via_portal() -> Option<DesktopAppearance> {
    use zbus::blocking::{Connection, Proxy};

    let conn = Connection::session().ok()?;
    let proxy = Proxy::new(
        &conn,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Settings",
    )
    .ok()?;

    Some(DesktopAppearance {
        color_scheme: read_color_scheme(&proxy),
        accent_color: read_accent_color(&proxy),
    })
}

#[cfg(target_os = "linux")]
fn read_color_scheme(proxy: &zbus::blocking::Proxy) -> ColorScheme {
    use zbus::zvariant::Value;

    let Some(value) = read_setting(proxy, "color-scheme") else {
        return ColorScheme::NoPreference;
    };
    // Spec: 0 = no preference, 1 = prefer dark, 2 = prefer light.
    match deep_unwrap(&value) {
        Value::U32(1) => ColorScheme::Dark,
        Value::U32(2) => ColorScheme::Light,
        _ => ColorScheme::NoPreference,
    }
}

#[cfg(target_os = "linux")]
fn read_accent_color(proxy: &zbus::blocking::Proxy) -> Option<(u8, u8, u8)> {
    use zbus::zvariant::Value;

    let value = read_setting(proxy, "accent-color")?;
    // Spec: a struct of three doubles (r, g, b) in 0.0..=1.0, or all -1 for "no preference".
    let Value::Structure(s) = deep_unwrap(&value) else {
        return None;
    };
    let fields = s.fields();
    if fields.len() != 3 {
        return None;
    }
    let r = as_f64(&fields[0])?;
    let g = as_f64(&fields[1])?;
    let b = as_f64(&fields[2])?;
    if r < 0.0 || g < 0.0 || b < 0.0 {
        return None; // explicit "no preference"
    }
    let to_u8 = |c: f64| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some((to_u8(r), to_u8(g), to_u8(b)))
}

/// Read a key from the `org.freedesktop.appearance` namespace, trying the modern `ReadOne`
/// (portal Settings v2) first and falling back to the older, double-wrapped `Read`.
#[cfg(target_os = "linux")]
fn read_setting(proxy: &zbus::blocking::Proxy, key: &str) -> Option<zbus::zvariant::OwnedValue> {
    const NS: &str = "org.freedesktop.appearance";
    if let Ok(v) = proxy.call::<_, _, zbus::zvariant::OwnedValue>("ReadOne", &(NS, key)) {
        return Some(v);
    }
    proxy.call::<_, _, zbus::zvariant::OwnedValue>("Read", &(NS, key)).ok()
}

/// `Read` wraps the value in an extra variant layer versus `ReadOne`; unwrap any nesting so
/// both methods produce the same concrete value.
#[cfg(target_os = "linux")]
fn deep_unwrap<'a, 'v>(value: &'a zbus::zvariant::Value<'v>) -> &'a zbus::zvariant::Value<'v> {
    match value {
        zbus::zvariant::Value::Value(inner) => deep_unwrap(inner),
        other => other,
    }
}

#[cfg(target_os = "linux")]
fn as_f64(value: &zbus::zvariant::Value<'_>) -> Option<f64> {
    match value {
        zbus::zvariant::Value::F64(f) => Some(*f),
        _ => None,
    }
}
