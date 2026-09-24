//! Windows specifics for the egui backends: thread DPI awareness, DWM
//! window attributes, registry reads, locale, virtual-screen metrics.
//!
//! Never mutates process-wide state: DPI awareness is set per thread and restored by a guard.
//! No WinRT (see the MTA factory-cache crash note), so the accent comes from the registry.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_SUCCESS, HWND};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, REG_ROUTINE_FLAGS, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD};
use windows::Win32::UI::HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

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

/// Apply dialog DWM attributes: dark title bar (DWMWA_USE_IMMERSIVE_DARK_MODE) and rounded corners
/// (DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND). Failures (older Windows) are ignored.
pub(crate) fn apply_dwm_attributes(hwnd: isize, dark: bool) {
    if hwnd == 0 {
        return;
    }
    let hwnd = HWND(hwnd as *mut core::ffi::c_void);
    let dark: i32 = dark as i32;
    let corner = DWMWCP_ROUND;
    // SAFETY: the attribute pointers reference locals of the documented sizes; `hwnd` is a live
    // window owned by the caller.
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, (&dark as *const i32).cast(), 4);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &corner as *const _ as *const core::ffi::c_void, 4);
    }
}

fn reg_get(subkey: &str, value: &str, flags: REG_ROUTINE_FLAGS) -> Option<Vec<u8>> {
    let (k, v) = (wide(subkey), wide(value));
    let mut size: u32 = 0;
    // SAFETY: size query with no data buffer; the strings are NUL-terminated and outlive the call.
    let err = unsafe { RegGetValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(v.as_ptr()), flags, None, None, Some(&mut size)) };
    if err != ERROR_SUCCESS || size == 0 || size > 4096 {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    // SAFETY: `buf` has `size` writable bytes, which is what we report.
    let err = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(v.as_ptr()), flags, None, Some(buf.as_mut_ptr().cast()), Some(&mut size))
    };
    if err != ERROR_SUCCESS {
        return None;
    }
    buf.truncate(size as usize);
    Some(buf)
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

/// Left edge of the virtual screen (all monitors), physical px.
pub(crate) fn virtual_screen_left() -> i32 {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_XVIRTUALSCREEN};
    // SAFETY: plain metrics query.
    unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) }
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
