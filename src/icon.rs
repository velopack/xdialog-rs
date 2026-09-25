//! Icon images ([`XDialogOptions::icon_source`](crate::XDialogOptions::icon_source)) decoded to
//! RGBA: `.ico` and `.png` with the `ico` crate (every PNG and BMP frame of an `.ico`), `.icns`
//! with the `icns` crate (every PNG, ARGB and 24-bit + mask frame; JPEG 2000 frames are skipped).
//! [`IconFile::render`] picks the best frame for a pixel size (the title bar, the taskbar and the
//! dialog each ask for their own) and resamples it to exactly that size.

use icns::{Encoding, IconFamily, IconType, PixelFormat};

use crate::XDialogIconSource;

/// A square, straight-alpha (not premultiplied) RGBA image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IconImage {
    pub size: u32,
    /// `size * size * 4` bytes, rows top to bottom.
    pub rgba: Vec<u8>,
}

/// A parsed icon source; frames are decoded on demand.
pub(crate) enum IconFile {
    /// A PNG, decoded.
    Png(ico::IconImage),
    Ico(ico::IconDir),
    Icns(IconFamily),
}

impl std::fmt::Debug for IconFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IconFile::Png(img) => write!(f, "Png({}x{})", img.width(), img.height()),
            IconFile::Ico(dir) => f.debug_tuple("Ico").field(&dir.entries().iter().map(|e| (e.width(), e.bits_per_pixel())).collect::<Vec<_>>()).finish(),
            IconFile::Icns(family) => f.debug_tuple("Icns").field(&family.available_icons()).finish(),
        }
    }
}

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

impl IconFile {
    /// Read (a file) and parse `source` (the format is detected from the content, not the
    /// extension).
    pub(crate) fn load(source: &XDialogIconSource) -> Result<IconFile, String> {
        match source {
            XDialogIconSource::File(path) => {
                let bytes = std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
                IconFile::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))
            }
            XDialogIconSource::Bytes(bytes) => IconFile::parse(bytes),
        }
    }

    pub(crate) fn parse(bytes: &[u8]) -> Result<IconFile, String> {
        let file = if bytes.starts_with(PNG_SIGNATURE) {
            IconFile::Png(ico::IconImage::read_png(bytes).map_err(|e| format!("PNG: {e}"))?)
        } else if bytes.starts_with(b"icns") {
            IconFile::Icns(IconFamily::read(bytes).map_err(|e| format!("ICNS: {e}"))?)
        } else if bytes.starts_with(&[0, 0, 1, 0]) {
            IconFile::Ico(ico::IconDir::read(std::io::Cursor::new(bytes)).map_err(|e| format!("ICO: {e}"))?)
        } else {
            return Err("not an .ico, .png or .icns image".into());
        };
        let empty = match &file {
            IconFile::Png(_) => false,
            IconFile::Ico(dir) => dir.entries().is_empty(),
            IconFile::Icns(family) => icns_frames(family, 0).is_empty(),
        };
        if empty {
            return Err("the icon file has no supported images".into());
        }
        Ok(file)
    }

    /// The icon at `size` x `size` px, resampled to fit, centred: the smallest frame at least
    /// that big (else the largest). Frames that fail to decode are skipped; `None` if none
    /// decodes.
    pub(crate) fn render(&self, size: u32) -> Option<IconImage> {
        let size = size.max(1);
        let fit_ico = |img: ico::IconImage| fit(img.rgba_data(), img.width(), img.height(), size);
        match self {
            IconFile::Png(img) => Some(fit(img.rgba_data(), img.width(), img.height(), size)),
            IconFile::Ico(dir) => {
                let mut entries: Vec<&ico::IconDirEntry> = dir.entries().iter().collect();
                // Deepest colour first among frames of the same size.
                entries.sort_by_key(|e| (frame_order(e.width().max(e.height()), size), std::cmp::Reverse(e.bits_per_pixel())));
                entries.into_iter().find_map(|e| e.decode().map(fit_ico).map_err(|err| warn!("xdialog: skipping an ICO frame: {err}")).ok())
            }
            IconFile::Icns(family) => icns_frames(family, size).into_iter().find_map(|t| {
                                                                            let img = family.get_icon_with_type(t)
                                                                                            .map_err(|e| warn!("xdialog: skipping an ICNS frame ({t:?}): {e}"))
                                                                                            .ok()?
                                                                                            .convert_to(PixelFormat::RGBA);
                                                                            Some(fit(img.data(), img.width(), img.height(), size))
                                                                        }),
        }
    }
}

/// Sort key of a frame `side` px big for a `size` px icon: big enough first (smallest first),
/// then the too-small ones (largest first).
fn frame_order(side: u32, size: u32) -> (bool, i64) {
    (side < size, if side >= size { side as i64 } else { -(side as i64) })
}

/// The full-colour frames of `family` in the order to try for `size` px ([`frame_order`]); at
/// equal size PNG / ARGB frames before the 24-bit + mask ones.
fn icns_frames(family: &IconFamily, size: u32) -> Vec<IconType> {
    let mut types: Vec<IconType> = family.available_icons()
                                         .into_iter()
                                         .filter(|t| matches!(t.encoding(), Encoding::RLE24 | Encoding::JP2PNG | Encoding::ARGB))
                                         .collect();
    types.sort_by_key(|t| (frame_order(t.pixel_width(), size), t.encoding() == Encoding::RLE24));
    types
}

// ------------------------------------------------------------------------------------------------
// Resampling
// ------------------------------------------------------------------------------------------------

/// Scale a `w` x `h` image to fit `size` x `size` (aspect kept, centred, transparent padding).
/// Area averaging when shrinking, bilinear when growing; in premultiplied alpha.
fn fit(rgba: &[u8], w: u32, h: u32, size: u32) -> IconImage {
    let scale = size as f32 / w.max(h) as f32;
    let dw = ((w as f32 * scale).round() as u32).clamp(1, size);
    let dh = ((h as f32 * scale).round() as u32).clamp(1, size);
    let premul: Vec<[f32; 4]> = rgba.chunks_exact(4)
                                    .map(|p| {
                                        let a = p[3] as f32 / 255.0;
                                        [p[0] as f32 * a, p[1] as f32 * a, p[2] as f32 * a, p[3] as f32]
                                    })
                                    .collect();
    // Separable: rows first, then columns.
    let horizontal = resample_axis(&premul, w as usize, h as usize, dw as usize, true);
    let scaled = resample_axis(&horizontal, dw as usize, h as usize, dh as usize, false);

    let (ox, oy) = (((size - dw) / 2) as usize, ((size - dh) / 2) as usize);
    let mut out = vec![0u8; (4 * size * size) as usize];
    for y in 0..dh as usize {
        for x in 0..dw as usize {
            let [r, g, b, a] = scaled[y * dw as usize + x];
            if a <= 0.0 {
                continue;
            }
            let un = 255.0 / a;
            let px = [r * un, g * un, b * un, a].map(|c| c.round().clamp(0.0, 255.0) as u8);
            out[4 * ((y + oy) * size as usize + x + ox)..][..4].copy_from_slice(&px);
        }
    }
    IconImage { size, rgba: out }
}

/// Resample along x (`horizontal`) or y from `n` to `m` samples (the other axis unchanged).
fn resample_axis(src: &[[f32; 4]], w: usize, h: usize, m: usize, horizontal: bool) -> Vec<[f32; 4]> {
    let n = if horizontal { w } else { h };
    // (first source index, weights) per output sample.
    let taps: Vec<(usize, Vec<f32>)> = (0..m).map(|i| weights(n, m, i)).collect();
    let (ow, oh) = if horizontal { (m, h) } else { (w, m) };
    let mut out = vec![[0.0; 4]; ow * oh];
    for y in 0..oh {
        for x in 0..ow {
            let (first, ws) = &taps[if horizontal { x } else { y }];
            let mut acc = [0.0f32; 4];
            for (k, wt) in ws.iter().enumerate() {
                let s = if horizontal { src[y * w + first + k] } else { src[(first + k) * w + x] };
                for c in 0..4 {
                    acc[c] += s[c] * wt;
                }
            }
            out[y * ow + x] = acc;
        }
    }
    out
}

/// Normalised weights of output sample `i` (of `m`) over `n` source samples: the overlap of the
/// source pixels with the output pixel's footprint when shrinking, a tent when growing.
fn weights(n: usize, m: usize, i: usize) -> (usize, Vec<f32>) {
    let ratio = n as f32 / m as f32;
    let (lo, hi, tent) = if ratio >= 1.0 {
        (i as f32 * ratio, (i + 1) as f32 * ratio, false)
    } else {
        let c = (i as f32 + 0.5) * ratio - 0.5;
        (c - 1.0, c + 1.0, true)
    };
    let first = (lo.floor().max(0.0) as usize).min(n - 1);
    let last = ((hi.ceil() as usize).min(n)).max(first + 1);
    let mut ws: Vec<f32> = (first..last).map(|j| {
                                            if tent {
                                                let c = (i as f32 + 0.5) * ratio - 0.5;
                                                (1.0 - (j as f32 - c).abs()).max(0.0)
                                            } else {
                                                (hi.min(j as f32 + 1.0) - lo.max(j as f32)).max(0.0)
                                            }
                                        })
                                        .collect();
    let sum: f32 = ws.iter().sum();
    if sum > 0.0 {
        ws.iter_mut().for_each(|w| *w /= sum);
    } else {
        // Growing past the edge: the nearest pixel.
        ws.iter_mut().for_each(|w| *w = 0.0);
        let near = (((i as f32 + 0.5) * ratio) as usize).clamp(first, last - 1);
        ws[near - first] = 1.0;
    }
    (first, ws)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// `size` x `size` RGBA pixels of `color`, with a transparent top-left pixel.
    fn pixels(size: u32, color: [u8; 3]) -> Vec<u8> {
        let mut rgba: Vec<u8> = (0..size * size).flat_map(|_| [color[0], color[1], color[2], 255]).collect();
        rgba[3] = 0;
        rgba
    }

    fn ico_image(size: u32, color: [u8; 3]) -> ico::IconImage {
        ico::IconImage::from_rgba_data(size, size, pixels(size, color))
    }

    /// A red PNG (see [`pixels`]).
    pub(crate) fn png_bytes(size: u32) -> Vec<u8> {
        let mut out = Vec::new();
        ico_image(size, [255, 0, 0]).write_png(&mut out).unwrap();
        out
    }

    /// An ICO with a green `small` px BMP frame and a red `big` px PNG frame.
    pub(crate) fn ico_bytes(small: u32, big: u32) -> Vec<u8> {
        let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
        dir.add_entry(ico::IconDirEntry::encode_as_bmp(&ico_image(small, [0, 255, 0])).unwrap());
        dir.add_entry(ico::IconDirEntry::encode_as_png(&ico_image(big, [255, 0, 0])).unwrap());
        let mut out = Vec::new();
        dir.write(&mut out).unwrap();
        out
    }

    fn pixel(img: &IconImage, x: u32, y: u32) -> [u8; 4] {
        img.rgba[(4 * (y * img.size + x)) as usize..][..4].try_into().unwrap()
    }

    #[test]
    fn png_file() {
        let icon = IconFile::parse(&png_bytes(64)).unwrap();
        let img = icon.render(64).unwrap();
        assert_eq!((img.size, pixel(&img, 0, 0)[3], pixel(&img, 10, 10)), (64, 0, [255, 0, 0, 255]));
        // Shrunk by area averaging: the top-left output pixel covers 3 of 4 opaque source pixels.
        let img = icon.render(32).unwrap();
        assert_eq!(pixel(&img, 0, 0), [255, 0, 0, 191]);
        assert_eq!(pixel(&img, 31, 31), [255, 0, 0, 255]);
        // Grown.
        assert_eq!(pixel(&icon.render(100).unwrap(), 50, 50), [255, 0, 0, 255]);
    }

    #[test]
    fn ico_picks_the_nearest_frame() {
        let icon = IconFile::parse(&ico_bytes(16, 256)).unwrap();
        // 16 px: the BMP frame, top-left transparent.
        let img = icon.render(16).unwrap();
        assert_eq!((pixel(&img, 8, 8), pixel(&img, 0, 0)[3]), ([0, 255, 0, 255], 0));
        // Too small for the 16 px frame: shrunk from it.
        assert_eq!(pixel(&icon.render(12).unwrap(), 6, 6), [0, 255, 0, 255]);
        // Anything bigger: the PNG frame.
        assert_eq!(pixel(&icon.render(20).unwrap(), 10, 10), [255, 0, 0, 255]);
        assert_eq!(pixel(&icon.render(512).unwrap(), 10, 10), [255, 0, 0, 255]);
    }

    #[test]
    fn icns_picks_the_nearest_frame() {
        let mut family = IconFamily::new();
        let frame = |size: u32, color: [u8; 3]| icns::Image::from_data(PixelFormat::RGBA, size, size, pixels(size, color)).unwrap();
        // 16 px: 24-bit RLE + 8-bit mask; 32 px ARGB; 128 px PNG.
        family.add_icon_with_type(&frame(16, [0, 0, 200]), IconType::RGB24_16x16).unwrap();
        family.add_icon_with_type(&frame(32, [0, 9, 0]), IconType::RGBA32_32x32).unwrap();
        family.add_icon_with_type(&frame(128, [255, 0, 0]), IconType::RGBA32_128x128).unwrap();
        let mut bytes = Vec::new();
        family.write(&mut bytes).unwrap();

        let icon = IconFile::parse(&bytes).unwrap();
        let img = icon.render(16).unwrap();
        assert_eq!((pixel(&img, 8, 8), pixel(&img, 0, 0)[3]), ([0, 0, 200, 255], 0));
        assert_eq!(pixel(&icon.render(32).unwrap(), 8, 8), [0, 9, 0, 255]);
        assert_eq!(pixel(&icon.render(64).unwrap(), 8, 8), [255, 0, 0, 255]);
        assert_eq!(pixel(&icon.render(512).unwrap(), 8, 8), [255, 0, 0, 255]);
    }

    #[test]
    fn sources_and_garbage() {
        assert!(IconFile::parse(b"hello").is_err());
        assert!(IconFile::parse(&[0, 0, 1, 0, 0, 0]).is_err());
        assert!(IconFile::parse(b"icns\0\0\0\x08").is_err());
        assert!(IconFile::load(&XDialogIconSource::File("does/not/exist.ico".into())).is_err());
        let bytes = XDialogIconSource::Bytes(png_bytes(8).into());
        assert_eq!(IconFile::load(&bytes).unwrap().render(8).unwrap().size, 8);
        assert_eq!(format!("{bytes:?}"), format!("Bytes({} bytes)", png_bytes(8).len()));
    }
}
