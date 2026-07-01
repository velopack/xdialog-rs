//! Pixel-format conversion for the software-rendering backend.
//!
//! tiny-skia renders into a premultiplied **RGBA** byte buffer, but `softbuffer` presents a
//! packed **`0x00RRGGBB`** `u32` per pixel (the alpha/high byte is ignored by every platform
//! backend). The dialog converts its whole internal pixmap to that format on every present, so
//! this conversion needs to be fast.
//!
//! [`rgba_to_argb`] is annotated with [`multiversion`], which compiles the (auto-vectorizable)
//! loop once per SIMD feature level for the *current* target architecture — SSE/AVX/AVX2/… on
//! x86/x86-64, NEON on aarch64 — and dispatches to the best one at runtime (the choice is cached
//! after the first call). The body uses no architecture-specific intrinsics, so the exact same
//! source covers x86, x86-64 and Apple-Silicon/arm64.

use multiversion::multiversion;

/// Convert a premultiplied-RGBA byte buffer (tiny-skia layout) into packed `0x00RRGGBB` `u32`s
/// (softbuffer layout), writing one `u32` per 4 input bytes.
///
/// `dst.len()` must be at least `src.len() / 4`; extra `dst` entries are left untouched. The
/// dialog always sizes the two buffers to the same physical dimensions, so they match exactly.
///
/// Reads operate on `&[u8]` via `chunks_exact(4)` rather than casting to `&[u32]`, because a
/// tiny-skia `Pixmap`'s backing `Vec<u8>` is only 1-byte aligned; the multiversioned loop still
/// vectorizes the per-pixel channel shuffle.
#[multiversion(targets = "simd")]
pub fn rgba_to_argb(src: &[u8], dst: &mut [u32]) {
    for (px, out) in src.chunks_exact(4).zip(dst.iter_mut()) {
        // px = [R, G, B, A] → 0x00RRGGBB. Alpha is dropped (opaque dialogs; ignored by softbuffer).
        *out = (px[0] as u32) << 16 | (px[1] as u32) << 8 | px[2] as u32;
    }
}

/// Convert only the axis-aligned rectangle `(x, y, w, h)` of an RGBA buffer into the matching
/// region of the packed-`u32` destination, leaving every pixel outside the rect untouched.
///
/// Both buffers are row-major with `buf_width` pixels per row (the physical window width). Used
/// for partial-damage presents: when softbuffer hands back the previous frame's buffer, only the
/// pixels under the frame's damage region need reconverting, not the whole window.
///
/// The caller guarantees the rect lies within the buffers (`x + w <= buf_width` and the rows
/// exist), so indexing stays in bounds. The per-row inner loop vectorizes exactly like
/// [`rgba_to_argb`].
#[multiversion(targets = "simd")]
pub fn rgba_to_argb_rect(
    src: &[u8],
    dst: &mut [u32],
    buf_width: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
) {
    for row in y..y + h {
        let start = row * buf_width + x;
        let src_row = &src[start * 4..(start + w) * 4];
        let dst_row = &mut dst[start..start + w];
        for (px, out) in src_row.chunks_exact(4).zip(dst_row.iter_mut()) {
            *out = (px[0] as u32) << 16 | (px[1] as u32) << 8 | px[2] as u32;
        }
    }
}

/// Plain scalar reference implementation, identical in output to [`rgba_to_argb`]. Kept for the
/// `convert` benchmark (to measure the SIMD speedup) and as a behavioural oracle in tests.
pub fn rgba_to_argb_scalar(src: &[u8], dst: &mut [u32]) {
    for (px, out) in src.chunks_exact(4).zip(dst.iter_mut()) {
        *out = (px[0] as u32) << 16 | (px[1] as u32) << 8 | px[2] as u32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_scalar_and_packs_rgb() {
        // A few pixels with distinct channels, plus a non-multiple-of-stride tail in dst.
        let src: Vec<u8> = vec![
            0x12, 0x34, 0x56, 0xFF, // R=12 G=34 B=56
            0x00, 0x00, 0x00, 0x00, // black
            0xFF, 0xFF, 0xFF, 0x80, // white, half alpha (ignored)
            0xDE, 0xAD, 0xBE, 0x01,
        ];
        let mut a = vec![0u32; 4];
        let mut b = vec![0u32; 4];
        rgba_to_argb(&src, &mut a);
        rgba_to_argb_scalar(&src, &mut b);
        assert_eq!(a, b);
        assert_eq!(a[0], 0x0012_3456);
        assert_eq!(a[1], 0x0000_0000);
        assert_eq!(a[2], 0x00FF_FFFF);
        assert_eq!(a[3], 0x00DE_ADBE);
    }

    #[test]
    fn rect_converts_only_the_region() {
        // 4x3 buffer; convert the 2x2 rect at (1, 1) and check the rest is untouched.
        let w = 4usize;
        let h = 3usize;
        let mut src = vec![0u8; w * h * 4];
        for (i, px) in src.chunks_exact_mut(4).enumerate() {
            px.copy_from_slice(&[i as u8, (i + 1) as u8, (i + 2) as u8, 0xFF]);
        }
        let sentinel = 0xDEAD_BEEFu32;
        let mut dst = vec![sentinel; w * h];

        rgba_to_argb_rect(&src, &mut dst, w, 1, 1, 2, 2);

        for row in 0..h {
            for col in 0..w {
                let idx = row * w + col;
                let inside = (1..3).contains(&col) && (1..3).contains(&row);
                if inside {
                    let px = &src[idx * 4..idx * 4 + 4];
                    let expected = (px[0] as u32) << 16 | (px[1] as u32) << 8 | px[2] as u32;
                    assert_eq!(dst[idx], expected, "pixel ({col},{row}) should be converted");
                } else {
                    assert_eq!(dst[idx], sentinel, "pixel ({col},{row}) must be untouched");
                }
            }
        }
    }
}
