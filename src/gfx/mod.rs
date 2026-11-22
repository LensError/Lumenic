#![forbid(unsafe_code)]

pub mod image;
pub mod inspect;
pub mod sdf;
pub mod text;
pub mod viewport;

use crate::gfx::sdf::SdfBatch;
use crate::gfx::text::GlyphAtlas;
use crate::os::{GlBackend, RenderTargetHandle};
use crate::ui::draw::DrawList;
use crate::ui::types::{Color, Theme};

/// Phase 2 GPU renderer holding the dynamic glyph atlas and instanced SDF batcher.
pub struct Renderer {
    pub glyph_atlas: GlyphAtlas,
    batch: SdfBatch,
    instance_bytes: Vec<u8>,
    last_render_ms: f32,
    last_sdf_instances: usize,
    last_gpu_time_ms: Option<f32>,
    last_gpu_buffer_bytes: usize,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    /// Creates a new renderer using system fonts (when available) plus the embedded fallback font.
    pub fn new() -> Self {
        Self {
            glyph_atlas: GlyphAtlas::new_with_system_fonts(),
            batch: SdfBatch::default(),
            instance_bytes: Vec::with_capacity(64 * 1024),
            last_render_ms: 0.0,
            last_sdf_instances: 0,
            last_gpu_time_ms: None,
            last_gpu_buffer_bytes: 0,
        }
    }

    /// Creates a fast renderer initialized solely with the embedded fallback font.
    #[cfg(target_arch = "wasm32")]
    pub fn new_fallback_only() -> Self {
        Self {
            glyph_atlas: GlyphAtlas::new_fallback_only(),
            batch: SdfBatch::default(),
            instance_bytes: Vec::with_capacity(64 * 1024),
            last_render_ms: 0.0,
            last_sdf_instances: 0,
            last_gpu_time_ms: None,
            last_gpu_buffer_bytes: 0,
        }
    }

    /// Returns `(render_time_ms, sdf_instances, atlas_size, atlas_occupancy)` from the last frame.
    pub fn perf_snapshot(&self) -> (f32, usize, [u32; 2], f32) {
        (
            self.last_render_ms,
            self.last_sdf_instances,
            self.glyph_atlas.atlas_size(),
            self.glyph_atlas.occupancy_ratio(),
        )
    }

    /// Returns `(gpu_time_ms, gpu_buffer_bytes)` from the last frame.
    pub fn gpu_telemetry(&self) -> (Option<f32>, usize) {
        (self.last_gpu_time_ms, self.last_gpu_buffer_bytes)
    }

    /// Compiles `draw_list` to instanced SDF primitives, syncs the glyph atlas, and draws to the default framebuffer.
    pub fn render_frame(
        &mut self,
        gl: &mut GlBackend,
        physical_size: [u32; 2],
        scale_factor: f32,
        cursor_logical: Option<[f32; 2]>,
        theme: &Theme,
        draw_list: &DrawList,
    ) {
        self.render_frame_to(
            gl,
            RenderTargetHandle::DEFAULT,
            physical_size,
            scale_factor,
            cursor_logical,
            theme.background,
            draw_list,
        );
    }

    /// Compiles `draw_list` to instanced SDF primitives, syncs the glyph atlas, and draws into `target` with `clear_color`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_frame_to(
        &mut self,
        gl: &mut GlBackend,
        target: RenderTargetHandle,
        physical_size: [u32; 2],
        scale_factor: f32,
        cursor_logical: Option<[f32; 2]>,
        clear_color: Color,
        draw_list: &DrawList,
    ) {
        let [physical_width, physical_height] = physical_size;
        if physical_width == 0 || physical_height == 0 {
            return;
        }

        #[cfg(not(target_arch = "wasm32"))]
        let t0 = std::time::Instant::now();

        self.batch.build(
            draw_list,
            &mut self.glyph_atlas,
            scale_factor,
            cursor_logical,
            physical_width,
            physical_height,
        );

        if let Some(upload) = self.glyph_atlas.take_dirty_upload_region() {
            gl.upload_atlas_region(upload);
        }

        self.batch.write_bytes_into(&mut self.instance_bytes);
        gl.render_sdf_batch_handle(
            target,
            physical_width,
            physical_height,
            clear_color,
            &self.batch,
            &self.instance_bytes,
        );
        let (gpu_ms, gpu_bytes) = gl.telemetry_snapshot();
        self.last_gpu_time_ms = gpu_ms;
        self.last_gpu_buffer_bytes = gpu_bytes;

        self.last_sdf_instances = self.batch.instances.len();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.last_render_ms = t0.elapsed().as_secs_f32() * 1000.0;
        }
    }
}
