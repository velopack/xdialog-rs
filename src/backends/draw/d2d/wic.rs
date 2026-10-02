//! [`MemorySurface`]: renders into a WIC bitmap through Direct2D's software rasterizer
//! (deterministic), readable as RGBA8. Tests, benches and offscreen renders only.

use std::rc::Rc;
use std::sync::OnceLock;

use windows::core::Interface;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;

use super::canvas::{draw_frame, DeviceRes};
use super::window::present_retrying;
use super::{backend, target_properties, Text};
use crate::backends::draw::{list, DrawError, Frame};

/// Keep the process MTA alive (once, never released), so WIC works from any thread without
/// `CoInitializeEx`: short-lived test threads must not own COM apartments.
fn pin_mta() -> Result<(), DrawError> {
    static MTA: OnceLock<Result<(), String>> = OnceLock::new();
    // SAFETY: CoIncrementMTAUsage has no preconditions; the cookie is deliberately leaked.
    MTA.get_or_init(|| unsafe { CoIncrementMTAUsage().map(|_| ()).map_err(|e| e.to_string()) })
       .clone()
       .map_err(|e| DrawError::Backend(format!("d2d: CoIncrementMTAUsage: {e}")))
}

/// The thread's WIC factory.
fn wic() -> Result<IWICImagingFactory, DrawError> {
    thread_local! {
        static WIC: std::cell::OnceCell<IWICImagingFactory> = const { std::cell::OnceCell::new() };
    }
    pin_mta()?;
    WIC.with(|c| {
           if let Some(f) = c.get() {
               return Ok(f.clone());
           }
           // SAFETY: in-process COM activation; the MTA is pinned above.
           let f: IWICImagingFactory = unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
               .map_err(|e| DrawError::Backend(format!("d2d: WIC factory: {e}")))?;
           Ok(c.get_or_init(|| f).clone())
       })
}

/// A WIC bitmap and its render target.
struct Target {
    bitmap: IWICBitmap,
    dc: ID2D1DeviceContext,
    size: [u32; 2],
    dev: DeviceRes,
}

/// Renders into memory; the last frame is readable as opaque RGBA8.
pub(crate) struct MemorySurface {
    text: Rc<Text>,
    wic: IWICImagingFactory,
    target: Option<Target>,
    rgba: Vec<u8>,
    /// Size of the last frame (`[0, 0]`: none yet).
    size: [u32; 2],
}

impl Target {
    fn new(wic: &IWICImagingFactory, text: &Text, [w, h]: [u32; 2]) -> windows::core::Result<Target> {
        // SAFETY: plain WIC / Direct2D creation calls with locals.
        unsafe {
            let bitmap = wic.CreateBitmap(w, h, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnDemand)?;
            let rt = text.d2d.CreateWicBitmapRenderTarget(&bitmap, &target_properties(D2D1_ALPHA_MODE_PREMULTIPLIED, true))?;
            let dc = rt.cast::<ID2D1DeviceContext>()?;
            let dev = DeviceRes::new(&dc, text)?;
            Ok(Target { bitmap, dc, size: [w, h], dev })
        }
    }

    /// Copy the bitmap into `rgba` (BGRA to opaque RGBA).
    fn read_back(&self, rgba: &mut Vec<u8>) -> windows::core::Result<()> {
        let [w, h] = self.size;
        // SAFETY: the locked buffer is only read while `lock` lives, within its reported length.
        unsafe {
            let lock = self.bitmap.Lock(&WICRect { X: 0, Y: 0, Width: w as i32, Height: h as i32 }, WICBitmapLockRead.0 as u32)?;
            let stride = lock.GetStride()? as usize;
            let (mut len, mut ptr) = (0u32, std::ptr::null_mut());
            lock.GetDataPointer(&mut len, &mut ptr)?;
            let data = std::slice::from_raw_parts(ptr, len as usize);
            rgba.clear();
            rgba.reserve(w as usize * h as usize * 4);
            for y in 0..h as usize {
                let row = &data[y * stride..y * stride + w as usize * 4];
                rgba.extend(row.as_chunks::<4>().0.iter().flat_map(|p| [p[2], p[1], p[0], 255]));
            }
        }
        Ok(())
    }
}

impl crate::backends::draw::MemoryTarget for MemorySurface {
    fn new(text: &Rc<Text>) -> Result<Self, DrawError> {
        Ok(MemorySurface { text: text.clone(), wic: wic()?, target: None, rgba: Vec::new(), size: [0, 0] })
    }

    fn read_rgba(&self) -> Option<(u32, u32, &[u8])> {
        (!list::is_zero_size(self.size)).then(|| (self.size[0], self.size[1], &self.rgba[..]))
    }
}

impl crate::backends::draw::Surface for MemorySurface {
    fn present_nonzero(&mut self, frame: &Frame<'_>) -> Result<(), DrawError> {
        let MemorySurface { text, wic, target, rgba, size } = self;
        present_retrying(target, |target| {
            if target.as_ref().is_none_or(|t| t.size != frame.size_px) {
                *target = None;
                *target = Some(Target::new(wic, text, frame.size_px).map_err(backend("WIC target"))?);
            }
            let t = target.as_mut().expect("created above");
            draw_frame(&t.dc, &mut t.dev, text, frame)?;
            t.read_back(rgba).map_err(backend("WIC readback"))?;
            *size = frame.size_px;
            Ok(())
        })
    }
}
