//! CPU software rasterizer for egui meshes.
//!
//! Vendored from `egui_software_backend` 0.0.3 (<https://github.com/DGriffin91/egui_software_backend>),
//! Copyright (c) the egui_software_backend authors. Licensed under either of the MIT license
//! (`LICENSE-MIT`) or the Apache License, Version 2.0 (`LICENSE-APACHE`), both in this directory, at
//! your option.
//!
//! Changes made for xdialog:
//! - ported to egui 0.36 (`TexturesDelta::set` is a multimap; `run_ui`);
//! - no `no_std`/`std`, `rayon`, `log`, `raster_stats`, `winit` or `test_render` features: this is
//!   the `std` + single-threaded configuration only, and the winit glue (`winit.rs`) is dropped;
//! - the primitive cache (`with_caching(true)`, tiled canvas, `hash.rs`) is removed: xdialog always
//!   renders directly (the cache re-inits the whole canvas whenever any primitive changes, so it
//!   was slower for animated content; see the egui tech notes);
//! - the `constify` proc-macro is replaced by a hand-written const-generic dispatch
//!   (`raster/rect.rs`, `raster/tri.rs`), and `dispatch_simd_impl!` is a crate-local macro;
//! - `with_simd_impl` (tests) to cross-check the SIMD paths against the generic one.
//!
//! The renderer only *reads* the `TexturesDelta`; the caller (the presenter) clears it afterwards.

// Vendored code: keep upstream shape (a few builder knobs are only exercised by tests, and the
// code follows upstream's clippy configuration).
#![allow(dead_code, clippy::explicit_counter_loop)]

use std::borrow::Cow;

pub(crate) type HashMap<K, V> = std::collections::HashMap<K, V, ahash::RandomState>;
use egui::{Color32, Mesh, Vec2};

use crate::backends::egui_core::render::raster::{
    color::{AvailableImpl, swizzle_rgba_bgra},
    egui_texture::EguiTexture,
    render::{draw_egui_mesh, egui_orient2df},
};

pub(crate) mod color;
pub(crate) mod egui_texture;
pub(crate) mod math;
#[allow(clippy::module_inception)]
pub(crate) mod raster;
pub(crate) mod render;

/// Used to define the color swizzle order. Some backends require Rgba and others require Bgra. The renderer swizzles
/// textures as they are loaded so they can later be rasterized directly onto the frame buffer.
#[derive(Copy, Clone, Default)]
pub enum ColorFieldOrder {
    #[default]
    Rgba,
    Bgra,
}

/// Software render backend for egui.
pub struct EguiSoftwareRender {
    textures: HashMap<egui::TextureId, EguiTexture>,
    output_field_order: ColorFieldOrder,
    convert_tris_to_rects: bool,
    allow_raster_opt: bool,
    simd_impl: AvailableImpl,
}

impl EguiSoftwareRender {
    /// # Arguments
    /// * `output_field_order` - egui textures and vertex colors will be swizzled before rendering to match the desired
    ///   output buffer order.
    pub fn new(output_field_order: ColorFieldOrder) -> Self {
        EguiSoftwareRender {
            textures: Default::default(),
            output_field_order,
            convert_tris_to_rects: true,
            allow_raster_opt: true,
            simd_impl: Default::default(),
        }
    }

    /// If true: attempts to optimize by converting suitable triangle pairs into rectangles for faster rendering.
    ///   Things *should* look the same with this set to `true` while rendering faster.
    pub fn with_convert_tris_to_rects(mut self, set: bool) -> Self {
        self.convert_tris_to_rects = set;
        self
    }

    /// If false: Rasterize everything with triangles, always calculate vertex colors, uvs, use bilinear
    ///   everywhere, etc... Things *should* look the same with this set to `true` while rendering faster.
    pub fn with_allow_raster_opt(mut self, set: bool) -> Self {
        self.allow_raster_opt = set;
        self
    }

    /// Force a specific SIMD implementation (tests). Only implementations the CPU supports can be
    /// named, because [`AvailableImpl`] values only come from [`color::available_instrs`].
    pub(crate) fn with_simd_impl(mut self, simd_impl: AvailableImpl) -> Self {
        self.simd_impl = simd_impl;
        self
    }

    /// Renders the given paint jobs directly into `buffer_ref`, blending over its current contents.
    ///
    /// # Arguments
    /// * `paint_jobs` - List of `egui::ClippedPrimitive` from egui to be rendered.
    /// * `textures_delta` - The change in egui textures since last frame. Applied before drawing;
    ///   `free` entries are applied after. The caller must clear it afterwards.
    /// * `pixels_per_point` - The number of physical pixels for each logical point.
    pub fn render(
        &mut self,
        buffer_ref: &mut BufferMutRef,
        paint_jobs: &[egui::ClippedPrimitive],
        textures_delta: &egui::TexturesDelta,
        pixels_per_point: f32,
    ) {
        self.set_textures(textures_delta);
        self.render_direct(buffer_ref, paint_jobs, pixels_per_point);
        self.free_textures(textures_delta);
    }

    /// Apply a texture delta without drawing anything (e.g. while the window has a zero-sized
    /// client area), so later frames that only carry partial updates still find their textures.
    pub fn update_textures(&mut self, textures_delta: &egui::TexturesDelta) {
        self.set_textures(textures_delta);
        self.free_textures(textures_delta);
    }

    fn render_direct(
        &mut self,
        direct_draw_buffer: &mut BufferMutRef,
        paint_jobs: &[egui::ClippedPrimitive],
        pixels_per_point: f32,
    ) {
        for egui::ClippedPrimitive {
            clip_rect,
            primitive,
        } in paint_jobs.iter()
        {
            let input_mesh = match primitive {
                egui::epaint::Primitive::Mesh(input_mesh) => input_mesh,
                // xdialog never emits paint callbacks.
                egui::epaint::Primitive::Callback(_) => continue,
            };

            if input_mesh.vertices.is_empty() || input_mesh.indices.is_empty() {
                continue;
            }

            let clip_rect = egui::Rect {
                min: clip_rect.min * pixels_per_point,
                max: clip_rect.max * pixels_per_point,
            };

            let mut mesh_min = egui::Vec2::splat(f32::MAX);
            let mut mesh_max = egui::Vec2::splat(-f32::MAX);

            let px_mesh =
                self.prepare_px_mesh(pixels_per_point, input_mesh, &mut mesh_min, &mut mesh_max);

            let mesh_size = mesh_max - mesh_min;
            if mesh_size.x > 8192.0 || mesh_size.y > 8192.0 {
                // TODO it occasionally tries to make giant buffers in the first couple frames initially for some reason.
                continue;
            }

            let render_in_low_precision = mesh_size.x > 4096.0 || mesh_size.y > 4096.0;
            if render_in_low_precision {
                draw_egui_mesh::<2>(
                    self.simd_impl,
                    &self.textures,
                    direct_draw_buffer,
                    &clip_rect,
                    &px_mesh,
                    Vec2::ZERO,
                    self.allow_raster_opt,
                    self.convert_tris_to_rects,
                );
            } else {
                draw_egui_mesh::<8>(
                    self.simd_impl,
                    &self.textures,
                    direct_draw_buffer,
                    &clip_rect,
                    &px_mesh,
                    Vec2::ZERO,
                    self.allow_raster_opt,
                    self.convert_tris_to_rects,
                );
            }
        }
    }

    fn prepare_px_mesh(
        &self,
        pixels_per_point: f32,
        mesh: &egui::Mesh,
        mesh_min: &mut Vec2,
        mesh_max: &mut Vec2,
    ) -> Mesh {
        let mut px_mesh = mesh.clone();

        for v in px_mesh.vertices.iter_mut() {
            v.pos *= pixels_per_point;

            match self.output_field_order {
                ColorFieldOrder::Rgba => (), // egui uses rgba
                ColorFieldOrder::Bgra => {
                    let d = swizzle_rgba_bgra(v.color.to_array());
                    v.color = Color32::from_rgba_premultiplied(d[0], d[1], d[2], d[3]);
                }
            }

            *mesh_min = mesh_min.min(v.pos.to_vec2());
            *mesh_max = mesh_max.max(v.pos.to_vec2());
        }

        // Make all the tris face forward (ccw) to simplify rasterization.
        // TODO perf: could store the area so it's not recomputed later.
        for i in (0..px_mesh.indices.len()).step_by(3) {
            let i0 = px_mesh.indices[i] as usize;
            let i1 = px_mesh.indices[i + 1] as usize;
            let i2 = px_mesh.indices[i + 2] as usize;
            let v0 = px_mesh.vertices[i0];
            let v1 = px_mesh.vertices[i1];
            let v2 = px_mesh.vertices[i2];
            let area = egui_orient2df(&v0.pos, &v1.pos, &v2.pos);
            if area < 0.0 {
                px_mesh.indices.swap(i + 1, i + 2);
            }
        }
        px_mesh
    }

    fn set_textures(&mut self, textures_delta: &egui::TexturesDelta) {
        for (id, deltas) in &textures_delta.set {
            for delta in deltas {
                let pixels = match &delta.image {
                    egui::ImageData::Color(image) => {
                        assert_eq!(image.width() * image.height(), image.pixels.len());
                        Cow::Borrowed(&image.pixels)
                    }
                };
                let size = delta.image.size();
                if let Some(pos) = delta.pos {
                    if let Some(texture) = self.textures.get_mut(id) {
                        for y in 0..size[1] {
                            for x in 0..size[0] {
                                let src_pos = x + y * size[0];
                                let dest_pos = (x + pos[0]) + (y + pos[1]) * texture.width;
                                texture.data[dest_pos] = match self.output_field_order {
                                    ColorFieldOrder::Rgba => pixels[src_pos].to_array(),
                                    ColorFieldOrder::Bgra => {
                                        swizzle_rgba_bgra(pixels[src_pos].to_array())
                                    }
                                };
                            }
                        }
                    }
                } else {
                    let new_texture =
                        EguiTexture::new(self.output_field_order, delta.options, size, &pixels);

                    self.textures.insert(*id, new_texture);
                }
            }
        }
    }

    fn free_textures(&mut self, textures_delta: &egui::TexturesDelta) {
        for free in &textures_delta.free {
            self.textures.remove(free);
        }
    }
}

/// A mutable reference to a slice of image buffer data and corresponding image extents.
#[derive(Debug)]
pub struct BufferMutRef<'a> {
    pub data: &'a mut [[u8; 4]],
    pub width: usize,
    pub height: usize,
    pub width_extent: usize,
    pub height_extent: usize,
}

impl<'a> BufferMutRef<'a> {
    pub fn new(data: &'a mut [[u8; 4]], width: usize, height: usize) -> Self {
        assert!(width > 0);
        assert!(height > 0);
        assert_eq!(data.len(), width * height);
        BufferMutRef {
            data,
            width,
            height,
            width_extent: width - 1,
            height_extent: height - 1,
        }
    }

    #[inline(always)]
    pub fn get_range(&self, start: usize, end: usize, y: usize) -> std::ops::Range<usize> {
        let row_start = y * self.width;
        let start = row_start + start;
        let end = row_start + end;
        start..end
    }

    #[inline(always)]
    pub fn get_mut_span(&mut self, start: usize, end: usize, y: usize) -> &mut [[u8; 4]] {
        let range = self.get_range(start, end, y);
        &mut self.data[range]
    }

    #[inline(always)]
    pub fn get_mut_clamped(&mut self, x: usize, y: usize) -> &mut [u8; 4] {
        let x = x.min(self.width_extent);
        let y = y.min(self.height_extent);
        &mut self.data[x + y * self.width]
    }

    #[inline(always)]
    pub fn get_mut(&mut self, x: usize, y: usize) -> &mut [u8; 4] {
        &mut self.data[x + y * self.width]
    }
}

/// A reference to a slice of image buffer data and corresponding image extents.
#[derive(Debug)]
pub struct BufferRef<'a> {
    pub data: &'a [[u8; 4]],
    pub width: usize,
    pub height: usize,
    pub width_extent: usize,
    pub height_extent: usize,
}

impl BufferRef<'_> {
    #[inline(always)]
    pub fn get_ref_clamped(&self, x: usize, y: usize) -> &[u8; 4] {
        let x = x.min(self.width_extent);
        let y = y.min(self.height_extent);
        &self.data[x + y * self.width]
    }

    #[inline(always)]
    pub fn get_ref(&self, x: usize, y: usize) -> &[u8; 4] {
        &self.data[x + y * self.width]
    }
}
