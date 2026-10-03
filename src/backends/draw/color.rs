//! Colours: a bit-for-bit port of `ecolor::Color32` 0.36.2 (the representation and maths the
//! themes' tokens were written against).

/// Premultiplied, sRGB-encoded (gamma space) RGBA8. All blending on every backend is gamma-space
/// source-over.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Color([u8; 4]);

/// Rounds, saturating (ecolor's `fast_round`).
const fn fast_round(r: f32) -> u8 {
    (r + 0.5) as u8
}

/// `val * frac / 255`, rounded, with no floating point (ecolor's `mul_frac_round`).
pub(crate) const fn mul_frac_round(val: u8, frac: u8) -> u8 {
    let p = (val as u16) * (frac as u16) + 128;
    ((p + (p >> 8)) >> 8) as u8
}

impl Color {
    pub const TRANSPARENT: Color = Color::from_rgba_premultiplied(0, 0, 0, 0);
    pub const BLACK: Color = Color::from_rgb(0, 0, 0);
    pub const WHITE: Color = Color::from_rgb(255, 255, 255);
    /// As `Color32::GRAY`.
    #[cfg(test)]
    pub const GRAY: Color = Color::from_rgb(160, 160, 160);
    #[cfg(test)]
    pub const RED: Color = Color::from_rgb(255, 0, 0);

    /// Opaque.
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Color([r, g, b, 255])
    }

    pub const fn from_rgba_premultiplied(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color([r, g, b, a])
    }

    /// From straight ("unmultiplied") alpha; premultiplies (ecolor's `_const` variant).
    pub const fn from_rgba_unmultiplied(r: u8, g: u8, b: u8, a: u8) -> Self {
        match a {
            0 => Color::TRANSPARENT,
            255 => Color::from_rgb(r, g, b),
            a => Color([mul_frac_round(r, a), mul_frac_round(g, a), mul_frac_round(b, a), a]),
        }
    }

    /// Red, multiplied by alpha.
    pub const fn r(self) -> u8 {
        self.0[0]
    }

    /// Green, multiplied by alpha.
    pub const fn g(self) -> u8 {
        self.0[1]
    }

    /// Blue, multiplied by alpha.
    pub const fn b(self) -> u8 {
        self.0[2]
    }

    pub const fn a(self) -> u8 {
        self.0[3]
    }

    /// Premultiplied RGBA.
    #[cfg(test)]
    pub const fn to_array(self) -> [u8; 4] {
        self.0
    }

    /// Straight-alpha RGBA (brush colours of CG and D2D).
    pub fn to_srgba_unmultiplied(self) -> [u8; 4] {
        let [r, g, b, a] = self.0;
        match a {
            0 | 255 => self.0,
            a => {
                let factor = 255.0 / a as f32;
                [fast_round(factor * r as f32), fast_round(factor * g as f32), fast_round(factor * b as f32), a]
            }
        }
    }

    /// Straight-alpha RGBA in 0..=1 (`D2D1_COLOR_F`, `CGContextSetRGBFillColor`).
    #[cfg_attr(draw_soft, allow(dead_code))]
    pub fn to_straight_f32(self) -> [f32; 4] {
        self.to_srgba_unmultiplied().map(|c| c as f32 / 255.0)
    }

    /// Multiply every channel by `factor / 255` (gamma space).
    pub fn gamma_multiply_u8(self, factor: u8) -> Color {
        let f = factor as u32;
        Color(self.0.map(|c| ((c as u32 * f + 127) / 255) as u8))
    }

    /// `on_top` composited over `self` (gamma space, source-over).
    pub fn blend(self, on_top: Color) -> Color {
        self.gamma_multiply_u8(255 - on_top.a()) + on_top
    }

    /// Interpolate towards `other` by `t` in gamma space.
    pub fn lerp_to_gamma(&self, other: Color, t: f32) -> Color {
        let lerp = |a: u8, b: u8| fast_round((1.0 - t) * a as f32 + t * b as f32);
        Color([lerp(self.0[0], other.0[0]), lerp(self.0[1], other.0[1]), lerp(self.0[2], other.0[2]), lerp(self.0[3], other.0[3])])
    }

    /// Perceived brightness in 0..=1.
    pub fn intensity(&self) -> f32 {
        (self.r() as f32 * 0.299 + self.g() as f32 * 0.587 + self.b() as f32 * 0.114) / 255.0
    }
}

impl core::ops::Add for Color {
    type Output = Color;

    fn add(self, o: Color) -> Color {
        Color([self.0[0].saturating_add(o.0[0]),
               self.0[1].saturating_add(o.0[1]),
               self.0[2].saturating_add(o.0[2]),
               self.0[3].saturating_add(o.0[3])])
    }
}

/// Opaque colour from `0xRRGGBB`.
pub(crate) const fn rgb(hex: u32) -> Color {
    Color::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Colour from `0xAARRGGBB` with straight alpha (the notation of WinUI resource tables); stored
/// premultiplied.
pub(crate) const fn argb(hex: u32) -> Color {
    Color::from_rgba_unmultiplied((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, (hex >> 24) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_and_premultiplication() {
        assert_eq!(rgb(0x102030).to_array(), [0x10, 0x20, 0x30, 255]);
        assert_eq!(Color::GRAY.to_array(), [160, 160, 160, 255]);
        assert_eq!(argb(0x00FFFFFF), Color::TRANSPARENT);
        assert_eq!(argb(0xFF102030), rgb(0x102030));
        // 255 * 0x14 / 255 = 20; 0x80 * 0x14 / 255 = 10.04 -> 10.
        assert_eq!(argb(0x14FFFFFF).to_array(), [20, 20, 20, 20]);
        assert_eq!(argb(0x14808080).to_array(), [10, 10, 10, 20]);
        assert_eq!(Color::from_rgba_unmultiplied(200, 100, 50, 128).to_array(), [100, 50, 25, 128]);
    }

    #[test]
    fn unmultiply_round_trips() {
        assert_eq!(Color::GRAY.to_srgba_unmultiplied(), [160, 160, 160, 255]);
        assert_eq!(Color::GRAY.to_straight_f32(), [160.0 / 255.0, 160.0 / 255.0, 160.0 / 255.0, 1.0]);
        assert_eq!(Color::TRANSPARENT.to_srgba_unmultiplied(), [0, 0, 0, 0]);
        assert_eq!(Color::from_rgba_unmultiplied(200, 100, 50, 128).to_srgba_unmultiplied(), [199, 100, 50, 128]);
    }

    #[test]
    fn blend_matches_color32() {
        // ecolor's own test: additive colour on top of opaque, and the reverse.
        let opaque = Color::from_rgb(40, 50, 60);
        let additive = Color::from_rgba_premultiplied(255, 127, 10, 0);
        assert_eq!(additive.blend(opaque), opaque);
        assert_eq!(opaque.blend(additive), Color::from_rgb(255, 177, 70));
        assert_eq!(Color::WHITE.blend(Color::from_rgba_unmultiplied(0, 0, 0, 128)), Color::from_rgb(127, 127, 127));
        assert_eq!(Color::WHITE.gamma_multiply_u8(128).to_array(), [128, 128, 128, 128]);
    }

    #[test]
    fn lerp_and_intensity() {
        assert_eq!(Color::BLACK.lerp_to_gamma(Color::WHITE, 0.0), Color::BLACK);
        assert_eq!(Color::BLACK.lerp_to_gamma(Color::WHITE, 1.0), Color::WHITE);
        assert_eq!(Color::BLACK.lerp_to_gamma(Color::WHITE, 0.5), Color::from_rgb(128, 128, 128));
        assert_eq!(Color::TRANSPARENT.lerp_to_gamma(Color::RED, 0.25).to_array(), [64, 0, 0, 64]);
        assert!((Color::WHITE.intensity() - 1.0).abs() < 1e-6);
        assert_eq!(Color::BLACK.intensity(), 0.0);
    }

    #[test]
    fn add_saturates() {
        assert_eq!(Color::from_rgb(200, 10, 0) + Color::from_rgba_premultiplied(100, 10, 0, 0), Color::from_rgb(255, 20, 0));
    }
}
