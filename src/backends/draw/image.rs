//! Bitmaps (the custom icon).

use std::rc::Rc;

use super::color::mul_frac_round;

/// Immutable premultiplied RGBA8 bitmap (custom icon). Cheap to clone. `id` is process-unique
/// (global AtomicU64) and is what backends key their uploaded copies on.
#[derive(Clone)]
pub(crate) struct Image {
    id: u64,
    size: [u32; 2],
    rgba: Rc<[u8]>,
}

impl Image {
    /// From `icon::IconImage` (straight alpha); premultiplies once. `rgba` must hold
    /// `size[0] * size[1]` tightly packed pixels.
    pub(crate) fn from_straight_rgba(size: [u32; 2], rgba: &[u8]) -> Image {
        let len = size[0] as usize * size[1] as usize * 4;
        assert_eq!(rgba.len(), len, "image data does not match its size");
        let rgba: Rc<[u8]> = rgba.as_chunks::<4>().0.iter()
                                 .flat_map(|p| {
                                     let a = p[3];
                                     [mul_frac_round(p[0], a), mul_frac_round(p[1], a), mul_frac_round(p[2], a), a]
                                 })
                                 .collect();
        Image { id: super::next_id(), size, rgba }
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    /// Physical px.
    pub(crate) fn size(&self) -> [u32; 2] {
        self.size
    }

    /// Premultiplied RGBA8, tightly packed, row-major.
    pub(crate) fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Image").field("id", &self.id).field("size", &self.size).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiplies_and_ids_are_unique() {
        let a = Image::from_straight_rgba([2, 1], &[255, 0, 0, 255, 200, 100, 50, 128]);
        assert_eq!(a.size(), [2, 1]);
        assert_eq!(a.rgba(), &[255, 0, 0, 255, 100, 50, 25, 128]);
        let b = Image::from_straight_rgba([2, 1], &[255, 0, 0, 255, 200, 100, 50, 128]);
        assert_ne!(a.id(), b.id());
        assert_eq!(a.clone().id(), a.id());
    }
}
