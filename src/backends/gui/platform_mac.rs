//! macOS: the system appearance (dark mode and accent colour, read from AppKit on the main
//! thread), the behind-window material of translucent dialogs (an `NSVisualEffectView`, or Liquid
//! Glass's `NSGlassEffectView` on macOS 26+, under the window's content) and the system's alert
//! icons.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use objc2::{AllocAnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSAutoresizingMaskOptions, NSBitmapImageFileType,
    NSBitmapImageRep, NSColor, NSColorSpace, NSDeviceRGBColorSpace, NSGlassEffectView, NSGlassEffectViewStyle, NSGraphicsContext, NSImage,
    NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindowOrderingMode,
};
use objc2_foundation::{NSArray, NSBundle, NSDictionary, NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::kCACornerCurveContinuous;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

use super::appearance::{Accent, Appearance};
use super::theme::{MaterialKind, WindowMaterial};
use crate::backends::draw::Color;
use crate::icon::IconFile;
use crate::model::{XDialogIcon, XDialogIconSource};

/// The vibrancy material behind a translucent dialog (what alerts use up to Sequoia: light or
/// dark by the window's appearance).
const MATERIAL: NSVisualEffectMaterial = NSVisualEffectMaterial::Popover;

/// Liquid Glass is available (macOS 26+). Checks for the `NSGlassEffectView` class, not the OS
/// version: binaries linked against a pre-26 SDK see version 16.0.
pub(crate) fn has_liquid_glass() -> bool {
    static GLASS: OnceLock<bool> = OnceLock::new();
    *GLASS.get_or_init(|| objc2::runtime::AnyClass::get(c"NSGlassEffectView").is_some())
}

/// The app's effective appearance (dark mode) and `controlAccentColor` as it resolves in it. Off
/// the main thread (never the case for the event loop thread): the default appearance.
pub(crate) fn read_appearance() -> Appearance {
    let Some(mtm) = MainThreadMarker::new() else { return Appearance::default() };
    let appearance = NSApplication::sharedApplication(mtm).effectiveAppearance();
    // SAFETY: constant strings provided by AppKit.
    let (aqua, dark_aqua) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
    let names = NSArray::from_slice(&[aqua, dark_aqua]);
    let dark = appearance.bestMatchFromAppearancesWithNames(&names).is_some_and(|n| &*n == dark_aqua);
    Appearance { dark, accent: accent_in(&appearance).map(|base| Accent { base, win_palette: None }) }
}

/// `controlAccentColor` resolved in `appearance` (a dynamic colour: dark mode has its own shade).
fn accent_in(appearance: &NSAppearance) -> Option<Color> {
    // `performAsCurrentDrawingAppearance:` needs blocks; the (deprecated) current appearance is
    // the same thing for a synchronous read.
    #[allow(deprecated)]
    let previous = NSAppearance::currentAppearance();
    // SAFETY: setting and restoring the current appearance on the main thread.
    #[allow(deprecated)]
    unsafe {
        NSAppearance::setCurrentAppearance(Some(appearance))
    };
    let rgb = NSColor::controlAccentColor().colorUsingColorSpace(&NSColorSpace::sRGBColorSpace());
    #[allow(deprecated)]
    unsafe {
        NSAppearance::setCurrentAppearance(previous.as_deref())
    };
    let c = rgb?;
    let u = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some(Color::from_rgb(u(c.redComponent()), u(c.greenComponent()), u(c.blueComponent())))
}

/// Put the alert material `material` behind `window`'s content: an `NSVisualEffectView`
/// (vibrancy) or an `NSGlassEffectView` (Liquid Glass; vibrancy where it doesn't exist) in the
/// window's frame view, under the content view, clipped to `material.corner_radius`. The window
/// must be transparent (`with_transparent(true)`) and its frames translucent. `false`: not
/// possible (no AppKit view or window, not on the main thread).
pub(crate) fn add_material(window: &Window, material: WindowMaterial) -> bool {
    let Some(mtm) = MainThreadMarker::new() else { return false };
    let Ok(handle) = window.window_handle() else { return false };
    let RawWindowHandle::AppKit(h) = handle.as_raw() else { return false };
    // SAFETY: winit's AppKit handle holds the window's `NSView`, valid while the window lives.
    let view: &NSView = unsafe { h.ns_view.cast().as_ref() };
    let Some(content) = view.window().and_then(|w| w.contentView()) else { return false };
    // SAFETY: reading the view hierarchy on the main thread.
    let Some(frame_view) = (unsafe { content.superview() }) else { return false };
    let mask = NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable;
    let radius = material.corner_radius;
    if material.kind == MaterialKind::Glass && has_liquid_glass() {
        // The glass draws its own rounded shape (continuous corners) and rim, plus a faint shadow
        // around it that a titled window's frame would show in the corners (alerts have none):
        // it sits in a container clipped to the same continuous corner.
        let clip = NSView::initWithFrame(NSView::alloc(mtm), frame_view.bounds());
        clip.setAutoresizingMask(mask);
        clip.setWantsLayer(true);
        if let Some(layer) = clip.layer() {
            layer.setCornerRadius(radius);
            // SAFETY: a constant string provided by Core Animation.
            layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
            layer.setMasksToBounds(true);
        }
        let glass = NSGlassEffectView::initWithFrame(NSGlassEffectView::alloc(mtm), clip.bounds());
        glass.setStyle(NSGlassEffectViewStyle::Regular);
        glass.setCornerRadius(radius);
        glass.setAutoresizingMask(mask);
        clip.addSubview(&glass);
        frame_view.addSubview_positioned_relativeTo(&clip, NSWindowOrderingMode::Below, Some(&content));
        return true;
    }
    let effect = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), frame_view.bounds());
    effect.setMaterial(MATERIAL);
    effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    effect.setState(NSVisualEffectState::Active);
    effect.setAutoresizingMask(mask);
    effect.setWantsLayer(true);
    if let Some(layer) = effect.layer() {
        layer.setCornerRadius(radius);
        layer.setMasksToBounds(true);
    }
    frame_view.addSubview_positioned_relativeTo(&effect, NSWindowOrderingMode::Below, Some(&content));
    true
}

/// Recompute `window`'s shadow from its current contents (a transparent window's shadow follows
/// its opaque pixels: call it once the first translucent frame is on screen).
pub(crate) fn invalidate_shadow(window: &Window) {
    let Ok(handle) = window.window_handle() else { return };
    let RawWindowHandle::AppKit(h) = handle.as_raw() else { return };
    // SAFETY: as in `add_material`.
    let view: &NSView = unsafe { h.ns_view.cast().as_ref() };
    if let Some(w) = view.window() {
        w.invalidateShadow();
    }
}

// ------------------------------------------------------------------------------------------------
// Alert icons
// ------------------------------------------------------------------------------------------------

/// The system's alert icons (CoreTypes).
const CORE_TYPES: &str = "/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources";

/// The image an alert shows for a severity icon: the app's icon (when running from an app bundle
/// that has an `.icns` icon) or the system's note icon for `Information`, the caution triangle
/// for `Warning`, the stop icon for `Error`. Loaded once per process; `None` when the file is
/// missing or unreadable (the theme then draws its own). macOS 26 has no `AlertCautionIcon.icns`:
/// the caution triangle is then AppKit's `NSCaution` image (what its alerts show).
pub(crate) fn alert_icon(icon: &XDialogIcon) -> Option<Arc<IconFile>> {
    static NOTE: OnceLock<Option<Arc<IconFile>>> = OnceLock::new();
    static CAUTION: OnceLock<Option<Arc<IconFile>>> = OnceLock::new();
    static STOP: OnceLock<Option<Arc<IconFile>>> = OnceLock::new();
    let core = |name: &str| load(&Path::new(CORE_TYPES).join(name));
    match icon {
        XDialogIcon::Information => NOTE.get_or_init(|| app_icon().and_then(|p| load(&p)).or_else(|| core("AlertNoteIcon.icns"))).clone(),
        XDialogIcon::Warning => CAUTION.get_or_init(|| core("AlertCautionIcon.icns").or_else(|| named_image("NSCaution"))).clone(),
        XDialogIcon::Error => STOP.get_or_init(|| core("AlertStopIcon.icns")).clone(),
        XDialogIcon::None | XDialogIcon::Custom => None,
    }
}

fn load(path: &Path) -> Option<Arc<IconFile>> {
    match IconFile::load(&XDialogIconSource::File(path.to_path_buf())) {
        Ok(f) => Some(Arc::new(f)),
        Err(e) => {
            debug!("xdialog: no alert icon {}: {e}", path.display());
            None
        }
    }
}

/// AppKit's image `name` (`+[NSImage imageNamed:]`) rendered at 256 px square (a 64 pt icon at
/// 4x) as a PNG icon file.
fn named_image(name: &str) -> Option<Arc<IconFile>> {
    const PX: isize = 256;
    let image = NSImage::imageNamed(&NSString::from_str(name))?;
    // SAFETY: a new 8-bit RGBA bitmap that allocates its own buffer (no planes passed);
    // `NSDeviceRGBColorSpace` is a constant string provided by AppKit.
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            PX,
            PX,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )
    }?;
    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    image.drawInRect(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(PX as f64, PX as f64)));
    NSGraphicsContext::restoreGraphicsState_class();
    // SAFETY: an empty properties dictionary.
    let png = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }?;
    match IconFile::load(&XDialogIconSource::Bytes(png.to_vec().into())) {
        Ok(f) => Some(Arc::new(f)),
        Err(e) => {
            debug!("xdialog: no alert icon {name}: {e}");
            None
        }
    }
}

/// The `.icns` of the main bundle's `CFBundleIconFile`, when running from an `.app`.
fn app_icon() -> Option<PathBuf> {
    let bundle = NSBundle::mainBundle();
    if !bundle.bundlePath().to_string().ends_with(".app") {
        return None;
    }
    let name = bundle.objectForInfoDictionaryKey(&NSString::from_str("CFBundleIconFile"))?.downcast::<NSString>().ok()?.to_string();
    let name = name.strip_suffix(".icns").unwrap_or(&name);
    let path = bundle.pathForResource_ofType(Some(&NSString::from_str(name)), Some(&NSString::from_str("icns")))?;
    Some(PathBuf::from(path.to_string()))
}
