//! [`WindowSurface`]: presents into the dialog's HWND through an `ID2D1HwndRenderTarget` (the
//! `DEFAULT` type: hardware when available, software otherwise, so RDP, VMs and driverless
//! machines work), recreated with all its device resources when the device is lost.

use std::rc::Rc;

use windows::core::Interface;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

use super::canvas::{draw_frame, DeviceRes};
use super::{backend, target_properties, Text};
use crate::backends::draw::{DrawError, Frame};

/// Several dialogs share a thread: never wait for vsync (the clock paces frames).
#[cfg(not(test))]
const PRESENT_OPTIONS: D2D1_PRESENT_OPTIONS = D2D1_PRESENT_OPTIONS_IMMEDIATELY;
/// Tests read the target back after `EndDraw` presented it, which is only defined when the
/// contents are retained.
#[cfg(test)]
const PRESENT_OPTIONS: D2D1_PRESENT_OPTIONS =
    D2D1_PRESENT_OPTIONS(D2D1_PRESENT_OPTIONS_IMMEDIATELY.0 | D2D1_PRESENT_OPTIONS_RETAIN_CONTENTS.0);

struct Target {
    rt: ID2D1HwndRenderTarget,
    dc: ID2D1DeviceContext,
    size: [u32; 2],
    dev: DeviceRes,
}

impl Target {
    fn new(hwnd: HWND, text: &Text, size_px: [u32; 2]) -> windows::core::Result<Target> {
        let size = D2D_SIZE_U { width: size_px[0].max(1), height: size_px[1].max(1) };
        let hprops = D2D1_HWND_RENDER_TARGET_PROPERTIES { hwnd, pixelSize: size, presentOptions: PRESENT_OPTIONS };
        // SAFETY: property structs are locals; `hwnd` is a live window (kept by the owner).
        let rt = unsafe { text.d2d.CreateHwndRenderTarget(&target_properties(D2D1_ALPHA_MODE_IGNORE, false), &hprops)? };
        let dc = rt.cast::<ID2D1DeviceContext>()?;
        let dev = DeviceRes::new(&dc, text)?;
        Ok(Target { rt, dc, size: [size.width, size.height], dev })
    }
}

/// Runs `attempt` (which creates the target when it is `None`, then draws); if that fails (device
/// lost, `D2DERR_RECREATE_TARGET`, or anything else), once more with a new target and new device
/// resources. The target is left `None` after a failure, so the next present recreates it.
pub(super) fn present_retrying<T>(target: &mut Option<T>,
                                  mut attempt: impl FnMut(&mut Option<T>) -> Result<(), DrawError>)
                                  -> Result<(), DrawError> {
    let Err(e) = attempt(target) else { return Ok(()) };
    debug!("xdialog: Direct2D present failed ({e}); recreating the render target");
    *target = None;
    attempt(target).inspect_err(|_| *target = None)
}

/// Direct2D on an HWND (the owner keeps the window alive).
struct HwndSurface {
    hwnd: HWND,
    text: Rc<Text>,
    /// `None` after a failed present (recreated by the next one).
    target: Option<Target>,
}

impl HwndSurface {
    fn present_nonzero(&mut self, frame: &Frame<'_>) -> Result<(), DrawError> {
        let HwndSurface { hwnd, text, target } = self;
        present_retrying(target, |target| {
            if target.is_none() {
                *target = Some(Target::new(*hwnd, text, frame.size_px).map_err(backend("render target"))?);
            }
            let t = target.as_mut().expect("created above");
            if t.size != frame.size_px {
                // SAFETY: plain call on a live render target, outside BeginDraw / EndDraw.
                unsafe { t.rt.Resize(&D2D_SIZE_U { width: frame.size_px[0], height: frame.size_px[1] }) }.map_err(backend("Resize"))?;
                t.size = frame.size_px;
            }
            draw_frame(&t.dc, &mut t.dev, text, frame)
        })
    }
}

pub(crate) struct WindowSurface {
    surface: HwndSurface,
    /// Keeps the HWND alive (dropped after the render target).
    _window: Rc<Window>,
}

impl crate::backends::draw::WindowTarget for WindowSurface {
    fn new(window: &Rc<Window>, text: &Rc<Text>) -> Result<Self, DrawError> {
        // Test builds: `XDIALOG_TEST_D2D_FAIL=1` fails here, as a machine without Direct2D would
        // (`Auto` then falls back to Win32 TaskDialogs).
        if crate::backends::gui::appearance::test_flag("XDIALOG_TEST_D2D_FAIL") {
            return Err(DrawError::Backend("d2d: XDIALOG_TEST_D2D_FAIL is set".into()));
        }
        let hwnd = match window.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(h)) => HWND(h.hwnd.get() as *mut core::ffi::c_void),
            _ => return Err(DrawError::Backend("d2d: no Win32 window handle".into())),
        };
        let s = window.inner_size();
        // Created up front: a machine without Direct2D fails here, before the window is shown.
        let target = Target::new(hwnd, text, [s.width, s.height]).map_err(backend("render target"))?;
        Ok(WindowSurface { surface: HwndSurface { hwnd, text: text.clone(), target: Some(target) }, _window: window.clone() })
    }
}

impl crate::backends::draw::Surface for WindowSurface {
    fn present_nonzero(&mut self, frame: &Frame<'_>) -> Result<(), DrawError> {
        self.surface.present_nonzero(frame)
    }
}

#[cfg(test)]
mod tests {
    //! The HWND path on a real (never activated, off-screen) window, read back from the target.

    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::*;

    use super::*;
    use crate::backends::draw::{Color, Rect, Shape, TextSystem};

    #[test]
    fn draws_resizes_and_recreates() {
        let text = Text::shared().unwrap();
        let bg = Color::from_rgb(0x20, 0x20, 0x20);
        let shapes = [Shape::Rect { rect: Rect::new(10.0, 10.0, 60.0, 40.0), radius: 4.0, color: Color::RED, snap: true }];
        for ppp in [1.0, 1.5] {
            // SAFETY: a plain popup window, destroyed below after the surface.
            let hwnd = unsafe {
                let hwnd = CreateWindowExW(WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                                           w!("STATIC"),
                                           w!("xdialog d2d test"),
                                           WS_POPUP,
                                           -10000,
                                           100,
                                           200,
                                           100,
                                           None,
                                           None,
                                           None,
                                           None).unwrap();
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                hwnd
            };
            let mut s = HwndSurface { hwnd, text: text.clone(), target: None };
            let k = |v: f64| (v * ppp) as u32;
            for (i, size) in [[100.0, 60.0], [120.0, 70.0], [120.0, 70.0]].into_iter().enumerate() {
                if i == 2 {
                    s.target = None; // as after a lost device
                }
                let size_px = [k(size[0]), k(size[1])];
                s.present_nonzero(&Frame { shapes: &shapes, size_px, ppp, clear: bg }).unwrap();
                let t = s.target.as_ref().unwrap();
                // SAFETY: reads the target through a CPU-readable copy, only while it is mapped.
                unsafe {
                    let src: ID2D1Bitmap1 = t.dc.GetTarget().unwrap().cast().unwrap();
                    let px_size = src.GetPixelSize();
                    assert_eq!([px_size.width, px_size.height], size_px);
                    let props = D2D1_BITMAP_PROPERTIES1 { pixelFormat: src.GetPixelFormat(),
                                                          dpiX: 96.0,
                                                          dpiY: 96.0,
                                                          bitmapOptions: D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                                                          ..Default::default() };
                    let copy = t.dc.CreateBitmap(px_size, None, 0, &props).unwrap();
                    copy.CopyFromBitmap(None, &src, None).unwrap();
                    let m = copy.Map(D2D1_MAP_OPTIONS_READ).unwrap();
                    let px = |x: u32, y: u32| {
                        let p = m.bits.add((y * m.pitch + x * 4) as usize);
                        [*p.add(2), *p.add(1), *p]
                    };
                    // Inside, the crisp (snapped) left edge, and the background.
                    assert_eq!(px(k(30.0), k(25.0)), [255, 0, 0], "ppp {ppp}");
                    assert_eq!(px(k(10.0), k(25.0)), [255, 0, 0], "ppp {ppp}");
                    assert_eq!(px(k(10.0) - 1, k(25.0)), [0x20, 0x20, 0x20], "ppp {ppp}");
                    assert_eq!(px(k(80.0), k(50.0)), [0x20, 0x20, 0x20], "ppp {ppp}");
                    copy.Unmap().unwrap();
                }
            }
            drop(s);
            // SAFETY: our own window.
            unsafe { DestroyWindow(hwnd).unwrap() };
        }
    }
}
