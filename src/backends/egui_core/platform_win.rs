//! Windows specifics for the egui backends: thread DPI awareness, the dark title bar, registry
//! reads, locale, OS version.
//!
//! Never mutates process-wide state: DPI awareness is set per thread and restored by a guard.
//! No WinRT (see the MTA factory-cache crash note), so the accent comes from the registry.

use windows::core::HSTRING;
use windows::Win32::Foundation::{ERROR_SUCCESS, HWND};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, REG_ROUTINE_FLAGS, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD};
use windows::Win32::UI::HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};

/// Sets the calling thread's DPI awareness to per-monitor v2 and restores the previous context
/// when dropped (builder mode runs on the user's main thread).
pub(crate) struct ThreadDpiGuard {
    prev: DPI_AWARENESS_CONTEXT,
}

impl ThreadDpiGuard {
    pub(crate) fn per_monitor_v2() -> Self {
        // SAFETY: plain Win32 call on the current thread; a null return (unsupported or invalid)
        // is handled by not restoring.
        let prev = unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        ThreadDpiGuard { prev }
    }
}

impl Drop for ThreadDpiGuard {
    fn drop(&mut self) {
        if !self.prev.0.is_null() {
            // SAFETY: restores the context returned by the matching Set call on this thread.
            unsafe {
                SetThreadDpiAwarenessContext(self.prev);
            }
        }
    }
}

/// Dark or light title bar (DWMWA_USE_IMMERSIVE_DARK_MODE). Failures (older Windows) are ignored.
pub(crate) fn set_dark_titlebar(window: &winit::window::Window, dark: bool) {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(RawWindowHandle::Win32(h)) = window.window_handle().map(|h| h.as_raw()) else { return };
    let dark: i32 = dark as i32;
    // SAFETY: the attribute pointer references a local of the documented size; `h.hwnd` is the
    // live window borrowed from the caller.
    unsafe {
        let _ = DwmSetWindowAttribute(HWND(h.hwnd.get() as *mut core::ffi::c_void), DWMWA_USE_IMMERSIVE_DARK_MODE, (&dark as *const i32).cast(), 4);
    }
}

/// A registry value of at most 64 bytes (the values read here: a DWORD, the 32-byte accent palette).
fn reg_get(subkey: &str, value: &str, flags: REG_ROUTINE_FLAGS) -> Option<Vec<u8>> {
    let (k, v) = (HSTRING::from(subkey), HSTRING::from(value));
    let mut buf = [0u8; 64];
    let mut size = buf.len() as u32;
    // SAFETY: `buf` has `size` writable bytes, which is what we report; the HSTRINGs are
    // NUL-terminated and outlive the call.
    let err = unsafe { RegGetValueW(HKEY_CURRENT_USER, &k, &v, flags, None, Some(buf.as_mut_ptr().cast()), Some(&mut size)) };
    (err == ERROR_SUCCESS).then(|| buf[..size as usize].to_vec())
}

/// A DWORD under HKCU.
pub(crate) fn read_hkcu_dword(subkey: &str, value: &str) -> Option<u32> {
    let b = reg_get(subkey, value, RRF_RT_REG_DWORD)?;
    Some(u32::from_le_bytes(b.get(..4)?.try_into().ok()?))
}

/// A REG_BINARY under HKCU.
pub(crate) fn read_hkcu_binary(subkey: &str, value: &str) -> Option<Vec<u8>> {
    reg_get(subkey, value, RRF_RT_REG_BINARY)
}

/// The user's default locale name, e.g. `en-US`, `zh-TW`.
pub(crate) fn user_locale() -> Option<String> {
    let mut buf = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH
    // SAFETY: writes at most buf.len() UTF-16 units.
    let n = unsafe { windows::Win32::Globalization::GetUserDefaultLocaleName(&mut buf) };
    if n <= 1 {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..(n as usize - 1)]))
}

/// Windows 10 or later, from `RtlGetVersion` (not subject to the manifest's version lie). Cached.
pub(crate) fn windows_10_or_later() -> bool {
    static WIN10: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *WIN10.get_or_init(|| {
              let mut info = windows::Win32::System::SystemInformation::OSVERSIONINFOW {
                  dwOSVersionInfoSize: size_of::<windows::Win32::System::SystemInformation::OSVERSIONINFOW>() as u32,
                  ..Default::default()
              };
              // SAFETY: `info` is a writable OSVERSIONINFOW with its size field set, as required.
              // On failure the struct stays zeroed (reads as < 10: the TaskDialog side).
              let _ = unsafe { windows::Wdk::System::SystemServices::RtlGetVersion(&mut info) };
              info.dwMajorVersion >= 10
          })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_and_locale_reads_dont_fail_hard() {
        // Values may be missing on CI; the calls must just not panic.
        let _ = read_hkcu_dword(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "AppsUseLightTheme");
        let _ = read_hkcu_binary(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent", "AccentPalette");
        assert!(read_hkcu_dword(r"Software\xdialog-does-not-exist", "Nope").is_none());
        let loc = user_locale();
        assert!(loc.as_deref().is_none_or(|l| !l.is_empty() && !l.contains('\0')));
    }

    #[test]
    fn windows_version_is_read() {
        // CI and dev machines run Windows 10+ (Rust's tier-1 MSVC std requires it).
        assert!(windows_10_or_later());
    }

    #[test]
    fn dpi_guard_restores() {
        use windows::Win32::UI::HiDpi::{AreDpiAwarenessContextsEqual, GetThreadDpiAwarenessContext};
        // SAFETY: thread-local DPI context queries.
        unsafe {
            let before = GetThreadDpiAwarenessContext();
            {
                let _g = ThreadDpiGuard::per_monitor_v2();
                assert!(AreDpiAwarenessContextsEqual(GetThreadDpiAwarenessContext(), DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).as_bool());
            }
            assert!(AreDpiAwarenessContextsEqual(GetThreadDpiAwarenessContext(), before).as_bool());
        }
    }
}
