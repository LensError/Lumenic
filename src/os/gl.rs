use core::mem::size_of;
use std::collections::HashMap;
use std::sync::{Arc, Weak};

use glow::HasContext;

use crate::gfx::sdf::{
    Custom2DPass, Custom2DPassKind, SdfBatch, SdfInstance, Viewport3DPass, BLIT_FRAG_BODY,
    FULLSCREEN_VERT_BODY, KAWASE_DOWN_FRAG_BODY, KAWASE_UP_FRAG_BODY, MESH3D_FRAG_BODY,
    MESH3D_VERT_BODY, SDF_FRAG_BODY, SDF_VERT_BODY,
};
use crate::gfx::text::AtlasUpload;
use crate::gfx::viewport::{camera_view, camera_view_proj, mat4_mul, MeshData, TextureFilter};
use crate::os::TextureOp;
use crate::ui::types::{Color, Error, TextureId};

fn flatten_mat4(m: [[f32; 4]; 4]) -> [f32; 16] {
    [
        m[0][0], m[0][1], m[0][2], m[0][3], m[1][0], m[1][1], m[1][2], m[1][3], m[2][0], m[2][1],
        m[2][2], m[2][3], m[3][0], m[3][1], m[3][2], m[3][3],
    ]
}

/// Computes Dual-Kawase pyramid dimensions `[half, quarter, eighth]`, returning `None` on `0x0`.
pub fn compute_kawase_pyramid_sizes(width: u32, height: u32) -> Option<[(u32, u32); 3]> {
    if width == 0 || height == 0 {
        return None;
    }
    let h_w = (width / 2).max(1);
    let h_h = (height / 2).max(1);
    let q_w = (h_w / 2).max(1);
    let q_h = (h_h / 2).max(1);
    let e_w = (q_w / 2).max(1);
    let e_h = (q_h / 2).max(1);
    Some([(h_w, h_h), (q_w, q_h), (e_w, e_h)])
}

/// Opaque internal handle to either the default framebuffer (`None`) or an offscreen FBO.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RenderTargetHandle(pub(crate) Option<glow::Framebuffer>);

impl RenderTargetHandle {
    pub(crate) const DEFAULT: Self = Self(None);
}

/// GPU color framebuffer + RGBA8 texture owned by an offscreen surface.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct OffscreenTarget {
    pub(crate) fbo: glow::Framebuffer,
    pub(crate) tex: glow::Texture,
    pub(crate) w: u32,
    pub(crate) h: u32,
}

#[cfg(not(target_arch = "wasm32"))]
impl OffscreenTarget {
    pub(crate) fn handle(&self) -> RenderTargetHandle {
        RenderTargetHandle(Some(self.fbo))
    }

    pub(crate) fn raw_fbo_id(&self) -> u32 {
        self.fbo.0.get()
    }

    pub(crate) fn raw_texture_id(&self) -> u32 {
        self.tex.0.get()
    }
}

struct FboTarget {
    fbo: glow::Framebuffer,
    tex: glow::Texture,
    w: u32,
    h: u32,
}

struct Fbo3DTarget {
    fbo: glow::Framebuffer,
    tex: glow::Texture,
    backdrop_tex: glow::Texture,
    depth_rbo: glow::Renderbuffer,
    msaa_fbo: Option<glow::Framebuffer>,
    msaa_color_rbo: Option<glow::Renderbuffer>,
    msaa_depth_rbo: Option<glow::Renderbuffer>,
    requested_samples: u8,
    w: u32,
    h: u32,
}

struct SdfUniforms {
    viewport: Option<glow::UniformLocation>,
    atlas: Option<glow::UniformLocation>,
    backdrop: Option<glow::UniformLocation>,
    user_tex: Option<glow::UniformLocation>,
    has_user_tex: Option<glow::UniformLocation>,
}

struct KawaseUniforms {
    tex: Option<glow::UniformLocation>,
    half_pixel: Option<glow::UniformLocation>,
}

struct BlitUniforms {
    tex: Option<glow::UniformLocation>,
}

struct Mesh3DUniforms {
    mvp: Option<glow::UniformLocation>,
    model: Option<glow::UniformLocation>,
    view: Option<glow::UniformLocation>,
    base_color: Option<glow::UniformLocation>,
    roughness: Option<glow::UniformLocation>,
    albedo_tex: Option<glow::UniformLocation>,
    has_albedo_tex: Option<glow::UniformLocation>,
    camera_pos: Option<glow::UniformLocation>,
    glass_params: Option<glow::UniformLocation>,
    backdrop_tex: Option<glow::UniformLocation>,
    has_backdrop: Option<glow::UniformLocation>,
}

struct Custom3DProgramEntry {
    src_hash: u64,
    program: glow::Program,
    u_mvp: Option<glow::UniformLocation>,
    u_model: Option<glow::UniformLocation>,
    u_view: Option<glow::UniformLocation>,
    u_camera_pos: Option<glow::UniformLocation>,
    u_resolution: Option<glow::UniformLocation>,
    u_time: Option<glow::UniformLocation>,
    u_backdrop_tex: Option<glow::UniformLocation>,
    u_has_backdrop: Option<glow::UniformLocation>,
    u_uniforms: Option<glow::UniformLocation>,
}

struct CustomProgramEntry {
    src_hash: u64,
    program: glow::Program,
    u_rect: Option<glow::UniformLocation>,
    u_viewport: Option<glow::UniformLocation>,
    u_resolution: Option<glow::UniformLocation>,
    u_time: Option<glow::UniformLocation>,
    u_pointer: Option<glow::UniformLocation>,
    u_scroll: Option<glow::UniformLocation>,
    u_uniforms: Option<glow::UniformLocation>,
}

struct CachedMesh3D {
    owner: Weak<MeshData>,
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    ebo: glow::Buffer,
    index_count: i32,
    byte_size: usize,
}

struct GlPipeline {
    dummy_vao: glow::VertexArray,
    instance_vbo: glow::Buffer,
    instance_vbo_cap: usize,
    atlas_tex: glow::Texture,
    atlas_size: [u32; 2],
    sdf_program: glow::Program,
    sdf_uniforms: SdfUniforms,
    kawase_down_program: glow::Program,
    kawase_down_uniforms: KawaseUniforms,
    kawase_up_program: glow::Program,
    kawase_up_uniforms: KawaseUniforms,
    blit_program: glow::Program,
    blit_uniforms: BlitUniforms,
    mesh3d_program: glow::Program,
    mesh3d_uniforms: Mesh3DUniforms,
    scene_target: Option<FboTarget>,
    down_targets: Vec<FboTarget>,
    up_targets: Vec<FboTarget>,
    fbo_size: [u32; 2],
    quad_vao: glow::VertexArray,
    #[allow(dead_code)]
    quad_vbo: glow::Buffer,
    custom_vao: glow::VertexArray,
    custom_vbo: glow::Buffer,
    custom_inst_vbo: glow::Buffer,
    #[cfg(not(target_arch = "wasm32"))]
    timer_query: Option<glow::Query>,
    #[cfg(not(target_arch = "wasm32"))]
    timer_pending: bool,
}

fn hash_shader_pair(vert: &str, frag: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in vert
        .as_bytes()
        .iter()
        .chain([0xFF].iter())
        .chain(frag.as_bytes())
    {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

const CUSTOM_QUAD_VERT_BODY: &str = r#"
layout(location = 0) in vec2 a_pos;
uniform vec4 u_rect;
uniform vec2 u_viewport;
out vec2 v_uv;
void main() {
    v_uv = a_pos;
    vec2 px = u_rect.xy + a_pos * u_rect.zw;
    vec2 ndc = vec2(
        (px.x / max(u_viewport.x, 1.0)) * 2.0 - 1.0,
        1.0 - (px.y / max(u_viewport.y, 1.0)) * 2.0
    );
    gl_Position = vec4(ndc, 0.0, 1.0);
}
"#;

/// Safe OpenGL / WebGL 2 backend keeping `glow` types strictly inside `os/`.
pub struct GlBackend {
    ctx: glow::Context,
    is_embedded: bool,
    pipeline: Option<GlPipeline>,
    viewport3d_target: Option<Fbo3DTarget>,
    user_textures: HashMap<TextureId, (glow::Texture, u32, u32)>,
    custom_programs: HashMap<u64, CustomProgramEntry>,
    custom_3d_programs: HashMap<u64, Custom3DProgramEntry>,
    mesh3d_cache: HashMap<usize, CachedMesh3D>,
    v3d_scratch: Vec<u8>,
    i3d_scratch: Vec<u8>,
    custom_v_scratch: Vec<u8>,
    custom_i_scratch: Vec<u8>,
    pub(crate) frame_time_s: f32,
    pub(crate) frame_pointer: [f32; 2],
    pub(crate) frame_scroll: [f32; 2],
    last_gpu_time_ms: Option<f32>,
    last_sdf_bytes: usize,
    last_atlas_bytes: usize,
}

impl GlBackend {
    pub(crate) fn new(ctx: glow::Context) -> Self {
        let is_embedded = ctx.version().is_embedded;
        Self {
            ctx,
            is_embedded,
            pipeline: None,
            viewport3d_target: None,
            user_textures: HashMap::new(),
            custom_programs: HashMap::new(),
            custom_3d_programs: HashMap::new(),
            mesh3d_cache: HashMap::new(),
            v3d_scratch: Vec::with_capacity(4096),
            i3d_scratch: Vec::with_capacity(2048),
            custom_v_scratch: Vec::with_capacity(1024),
            custom_i_scratch: Vec::with_capacity(4096),
            frame_time_s: 0.0,
            frame_pointer: [0.0, 0.0],
            frame_scroll: [0.0, 0.0],
            last_gpu_time_ms: None,
            last_sdf_bytes: 0,
            last_atlas_bytes: 0,
        }
    }

    pub(crate) fn telemetry_snapshot(&self) -> (Option<f32>, usize) {
        let mut bytes = self.last_sdf_bytes + self.last_atlas_bytes;
        for &(_, w, h) in self.user_textures.values() {
            bytes = bytes.saturating_add((w as usize).saturating_mul(h as usize).saturating_mul(4));
        }
        if let Some(p) = &self.pipeline {
            if let Some(st) = &p.scene_target {
                bytes = bytes.saturating_add(
                    (st.w as usize)
                        .saturating_mul(st.h as usize)
                        .saturating_mul(4),
                );
            }
            for t in p.down_targets.iter().chain(p.up_targets.iter()) {
                bytes = bytes.saturating_add(
                    (t.w as usize)
                        .saturating_mul(t.h as usize)
                        .saturating_mul(4),
                );
            }
        }
        if let Some(t3) = &self.viewport3d_target {
            bytes = bytes.saturating_add(
                (t3.w as usize)
                    .saturating_mul(t3.h as usize)
                    .saturating_mul(8),
            );
        }
        for m in self.mesh3d_cache.values() {
            bytes = bytes.saturating_add(m.byte_size);
        }
        (self.last_gpu_time_ms, bytes)
    }

    pub(crate) fn sync_textures(&mut self, ops: Vec<TextureOp>) {
        for op in ops {
            match op {
                TextureOp::Upload {
                    id,
                    width,
                    height,
                    rgba,
                } => {
                    let (Ok(wi), Ok(hi)) = (i32::try_from(width), i32::try_from(height)) else {
                        continue;
                    };
                    if let Some((old, _, _)) = self.user_textures.remove(&id) {
                        // SAFETY: `old` was created on `self.ctx`.
                        unsafe {
                            self.ctx.delete_texture(old);
                        }
                    }
                    // SAFETY: `self.ctx` is current and `rgba` matches `wi x hi` RGBA8 bytes.
                    unsafe {
                        let Ok(tex) = self.ctx.create_texture() else {
                            continue;
                        };
                        self.ctx.bind_texture(glow::TEXTURE_2D, Some(tex));
                        self.ctx.tex_parameter_i32(
                            glow::TEXTURE_2D,
                            glow::TEXTURE_MIN_FILTER,
                            glow::LINEAR as i32,
                        );
                        self.ctx.tex_parameter_i32(
                            glow::TEXTURE_2D,
                            glow::TEXTURE_MAG_FILTER,
                            glow::LINEAR as i32,
                        );
                        self.ctx.tex_parameter_i32(
                            glow::TEXTURE_2D,
                            glow::TEXTURE_WRAP_S,
                            glow::CLAMP_TO_EDGE as i32,
                        );
                        self.ctx.tex_parameter_i32(
                            glow::TEXTURE_2D,
                            glow::TEXTURE_WRAP_T,
                            glow::CLAMP_TO_EDGE as i32,
                        );
                        self.ctx.tex_image_2d(
                            glow::TEXTURE_2D,
                            0,
                            glow::RGBA8 as i32,
                            wi,
                            hi,
                            0,
                            glow::RGBA,
                            glow::UNSIGNED_BYTE,
                            glow::PixelUnpackData::Slice(Some(&rgba)),
                        );
                        self.ctx.bind_texture(glow::TEXTURE_2D, None);
                        self.user_textures.insert(id, (tex, width, height));
                    }
                }
                TextureOp::UpdateSub {
                    id,
                    width,
                    height,
                    rgba,
                } => {
                    let (Ok(wi), Ok(hi)) = (i32::try_from(width), i32::try_from(height)) else {
                        continue;
                    };
                    if let Some(&(tex, old_w, old_h)) = self.user_textures.get(&id) {
                        // SAFETY: `tex` belongs to `self.ctx` and `rgba` matches `wi x hi` RGBA8.
                        unsafe {
                            self.ctx.bind_texture(glow::TEXTURE_2D, Some(tex));
                            if old_w == width && old_h == height {
                                self.ctx.tex_sub_image_2d(
                                    glow::TEXTURE_2D,
                                    0,
                                    0,
                                    0,
                                    wi,
                                    hi,
                                    glow::RGBA,
                                    glow::UNSIGNED_BYTE,
                                    glow::PixelUnpackData::Slice(Some(&rgba)),
                                );
                            } else {
                                self.ctx.tex_image_2d(
                                    glow::TEXTURE_2D,
                                    0,
                                    glow::RGBA8 as i32,
                                    wi,
                                    hi,
                                    0,
                                    glow::RGBA,
                                    glow::UNSIGNED_BYTE,
                                    glow::PixelUnpackData::Slice(Some(&rgba)),
                                );
                            }
                            self.ctx.bind_texture(glow::TEXTURE_2D, None);
                        }
                        self.user_textures.insert(id, (tex, width, height));
                    }
                }
                TextureOp::Destroy(id) => {
                    if let Some((tex, _, _)) = self.user_textures.remove(&id) {
                        // SAFETY: `tex` was created on `self.ctx`.
                        unsafe {
                            self.ctx.delete_texture(tex);
                        }
                    }
                }
            }
        }
    }

    fn shader_header(&self) -> &'static str {
        if self.is_embedded {
            "#version 300 es\nprecision highp float;\nprecision highp int;\n"
        } else {
            "#version 330 core\n"
        }
    }

    fn compile_program(&self, vert_body: &str, frag_body: &str) -> Result<glow::Program, String> {
        let header = self.shader_header();
        let vert_src = format!("{header}{vert_body}");
        let frag_src = format!("{header}{frag_body}");

        // SAFETY: `self.ctx` is bound on the active thread.
        unsafe {
            let program = self.ctx.create_program()?;
            let vs = self.ctx.create_shader(glow::VERTEX_SHADER)?;
            self.ctx.shader_source(vs, &vert_src);
            self.ctx.compile_shader(vs);
            if !self.ctx.get_shader_compile_status(vs) {
                let log = self.ctx.get_shader_info_log(vs);
                self.ctx.delete_shader(vs);
                self.ctx.delete_program(program);
                return Err(format!("vertex shader error: {log}"));
            }

            let fs = self.ctx.create_shader(glow::FRAGMENT_SHADER)?;
            self.ctx.shader_source(fs, &frag_src);
            self.ctx.compile_shader(fs);
            if !self.ctx.get_shader_compile_status(fs) {
                let log = self.ctx.get_shader_info_log(fs);
                self.ctx.delete_shader(vs);
                self.ctx.delete_shader(fs);
                self.ctx.delete_program(program);
                return Err(format!("fragment shader error: {log}"));
            }

            self.ctx.attach_shader(program, vs);
            self.ctx.attach_shader(program, fs);
            self.ctx.link_program(program);
            self.ctx.detach_shader(program, vs);
            self.ctx.detach_shader(program, fs);
            self.ctx.delete_shader(vs);
            self.ctx.delete_shader(fs);

            if !self.ctx.get_program_link_status(program) {
                let log = self.ctx.get_program_info_log(program);
                self.ctx.delete_program(program);
                return Err(format!("program link error: {log}"));
            }
            Ok(program)
        }
    }

    fn create_fbo_target(&self, w: u32, h: u32) -> Result<FboTarget, String> {
        let wi = i32::try_from(w).map_err(|_| String::from("FBO width overflow"))?;
        let hi = i32::try_from(h).map_err(|_| String::from("FBO height overflow"))?;

        // SAFETY: `self.ctx` is current and `w`, `h` are verified non-zero i32 dimensions.
        unsafe {
            let tex = self.ctx.create_texture()?;
            self.ctx.bind_texture(glow::TEXTURE_2D, Some(tex));
            self.ctx.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            self.ctx.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            self.ctx.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            self.ctx.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            self.ctx.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                wi,
                hi,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            self.ctx.bind_texture(glow::TEXTURE_2D, None);

            let fbo = match self.ctx.create_framebuffer() {
                Ok(f) => f,
                Err(e) => {
                    self.ctx.delete_texture(tex);
                    return Err(e);
                }
            };
            self.ctx.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            self.ctx.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(tex),
                0,
            );
            self.ctx.bind_framebuffer(glow::FRAMEBUFFER, None);
            Ok(FboTarget { fbo, tex, w, h })
        }
    }

    fn destroy_fbo_target(&self, target: FboTarget) {
        // SAFETY: `target` handles were created by `self.ctx`.
        unsafe {
            self.ctx.delete_framebuffer(target.fbo);
            self.ctx.delete_texture(target.tex);
        }
    }

    fn destroy_3d_target(&self, target: Fbo3DTarget) {
        // SAFETY: `target` handles were created by `self.ctx`.
        unsafe {
            if let Some(mfbo) = target.msaa_fbo {
                self.ctx.delete_framebuffer(mfbo);
            }
            if let Some(mrbo) = target.msaa_color_rbo {
                self.ctx.delete_renderbuffer(mrbo);
            }
            if let Some(mdrbo) = target.msaa_depth_rbo {
                self.ctx.delete_renderbuffer(mdrbo);
            }
            self.ctx.delete_framebuffer(target.fbo);
            self.ctx.delete_texture(target.tex);
            self.ctx.delete_texture(target.backdrop_tex);
            self.ctx.delete_renderbuffer(target.depth_rbo);
        }
    }

    fn create_3d_target(
        &self,
        w: u32,
        h: u32,
        requested_samples: u8,
    ) -> Result<Fbo3DTarget, String> {
        let wi = i32::try_from(w).map_err(|_| String::from("3D FBO width overflow"))?;
        let hi = i32::try_from(h).map_err(|_| String::from("3D FBO height overflow"))?;

        // SAFETY: `self.ctx` is current and `w`, `h` are verified non-zero i32 dimensions.
        unsafe {
            let tex = self.ctx.create_texture()?;
            self.ctx.bind_texture(glow::TEXTURE_2D, Some(tex));
            self.ctx.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            self.ctx.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            self.ctx.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            self.ctx.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            self.ctx.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                wi,
                hi,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            self.ctx.bind_texture(glow::TEXTURE_2D, None);

            let backdrop_tex = self.ctx.create_texture()?;
            self.ctx.bind_texture(glow::TEXTURE_2D, Some(backdrop_tex));
            self.ctx.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            self.ctx.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            self.ctx.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            self.ctx.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            self.ctx.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                wi,
                hi,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            self.ctx.bind_texture(glow::TEXTURE_2D, None);

            let depth_rbo = self.ctx.create_renderbuffer()?;
            self.ctx
                .bind_renderbuffer(glow::RENDERBUFFER, Some(depth_rbo));
            self.ctx
                .renderbuffer_storage(glow::RENDERBUFFER, glow::DEPTH_COMPONENT24, wi, hi);
            self.ctx.bind_renderbuffer(glow::RENDERBUFFER, None);

            let fbo = self.ctx.create_framebuffer()?;
            self.ctx.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            self.ctx.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(tex),
                0,
            );
            self.ctx.framebuffer_renderbuffer(
                glow::FRAMEBUFFER,
                glow::DEPTH_ATTACHMENT,
                glow::RENDERBUFFER,
                Some(depth_rbo),
            );
            self.ctx.bind_framebuffer(glow::FRAMEBUFFER, None);

            let (mut msaa_fbo, mut msaa_color_rbo, mut msaa_depth_rbo) = (None, None, None);
            if requested_samples > 1 {
                let max_samples = self.ctx.get_parameter_i32(glow::MAX_SAMPLES).max(1) as u8;
                let samples = requested_samples.min(max_samples);
                if samples > 1 {
                    if let (Ok(crbo), Ok(drbo), Ok(mfbo)) = (
                        self.ctx.create_renderbuffer(),
                        self.ctx.create_renderbuffer(),
                        self.ctx.create_framebuffer(),
                    ) {
                        self.ctx.bind_renderbuffer(glow::RENDERBUFFER, Some(crbo));
                        self.ctx.renderbuffer_storage_multisample(
                            glow::RENDERBUFFER,
                            i32::from(samples),
                            glow::RGBA8,
                            wi,
                            hi,
                        );
                        self.ctx.bind_renderbuffer(glow::RENDERBUFFER, Some(drbo));
                        self.ctx.renderbuffer_storage_multisample(
                            glow::RENDERBUFFER,
                            i32::from(samples),
                            glow::DEPTH_COMPONENT24,
                            wi,
                            hi,
                        );
                        self.ctx.bind_renderbuffer(glow::RENDERBUFFER, None);

                        self.ctx.bind_framebuffer(glow::FRAMEBUFFER, Some(mfbo));
                        self.ctx.framebuffer_renderbuffer(
                            glow::FRAMEBUFFER,
                            glow::COLOR_ATTACHMENT0,
                            glow::RENDERBUFFER,
                            Some(crbo),
                        );
                        self.ctx.framebuffer_renderbuffer(
                            glow::FRAMEBUFFER,
                            glow::DEPTH_ATTACHMENT,
                            glow::RENDERBUFFER,
                            Some(drbo),
                        );
                        let complete = self.ctx.check_framebuffer_status(glow::FRAMEBUFFER)
                            == glow::FRAMEBUFFER_COMPLETE;
                        self.ctx.bind_framebuffer(glow::FRAMEBUFFER, None);
                        if complete {
                            msaa_fbo = Some(mfbo);
                            msaa_color_rbo = Some(crbo);
                            msaa_depth_rbo = Some(drbo);
                        } else {
                            self.ctx.delete_framebuffer(mfbo);
                            self.ctx.delete_renderbuffer(crbo);
                            self.ctx.delete_renderbuffer(drbo);
                        }
                    }
                }
            }

            Ok(Fbo3DTarget {
                fbo,
                tex,
                backdrop_tex,
                depth_rbo,
                msaa_fbo,
                msaa_color_rbo,
                msaa_depth_rbo,
                requested_samples,
                w,
                h,
            })
        }
    }

    fn ensure_3d_fbo(&mut self, w: u32, h: u32, requested_samples: u8) {
        if w == 0 || h == 0 {
            return;
        }
        if let Some(t) = &self.viewport3d_target {
            if t.w == w && t.h == h && t.requested_samples == requested_samples {
                return;
            }
        }
        if let Some(old) = self.viewport3d_target.take() {
            self.destroy_3d_target(old);
        }
        if let Ok(target) = self.create_3d_target(w, h, requested_samples) {
            self.viewport3d_target = Some(target);
        }
    }

    /// Returns the underlying GL framebuffer handle backing `Scene3D` viewports on native builds.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn viewport_fbo_id(&mut self) -> Option<u32> {
        if self.ensure_pipeline().is_err() {
            return None;
        }
        if self.viewport3d_target.is_none() {
            self.ensure_3d_fbo(64, 64, 4);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.viewport3d_target.as_ref().map(|t| t.fbo.0.get())
        }
        #[cfg(target_arch = "wasm32")]
        {
            None
        }
    }

    fn ensure_pipeline(&mut self) -> Result<(), String> {
        if self.pipeline.is_some() {
            return Ok(());
        }

        let sdf_program = self.compile_program(SDF_VERT_BODY, SDF_FRAG_BODY)?;
        let kawase_down_program =
            self.compile_program(FULLSCREEN_VERT_BODY, KAWASE_DOWN_FRAG_BODY)?;
        let kawase_up_program = self.compile_program(FULLSCREEN_VERT_BODY, KAWASE_UP_FRAG_BODY)?;
        let blit_program = self.compile_program(FULLSCREEN_VERT_BODY, BLIT_FRAG_BODY)?;
        let mesh3d_program = self.compile_program(MESH3D_VERT_BODY, MESH3D_FRAG_BODY)?;

        let stride = i32::try_from(size_of::<SdfInstance>())
            .map_err(|_| String::from("instance stride overflow"))?;

        // SAFETY: `self.ctx` is bound on the active thread; attribute strides/offsets match `SdfInstance` and static unit quad.
        let (
            dummy_vao,
            instance_vbo,
            atlas_tex,
            quad_vao,
            quad_vbo,
            custom_vao,
            custom_vbo,
            custom_inst_vbo,
            sdf_uniforms,
            kawase_down_uniforms,
            kawase_up_uniforms,
            blit_uniforms,
            mesh3d_uniforms,
            timer_query,
        ) = unsafe {
            let vao = self.ctx.create_vertex_array()?;
            let vbo = self.ctx.create_buffer()?;
            let tex = self.ctx.create_texture()?;

            self.ctx.bind_vertex_array(Some(vao));
            self.ctx.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));

            for loc in 0..9u32 {
                let offset = (loc as i32) * 16;
                self.ctx.enable_vertex_attrib_array(loc);
                self.ctx
                    .vertex_attrib_pointer_f32(loc, 4, glow::FLOAT, false, stride, offset);
                self.ctx.vertex_attrib_divisor(loc, 1);
            }

            self.ctx.bind_vertex_array(None);
            self.ctx.bind_buffer(glow::ARRAY_BUFFER, None);

            self.ctx.bind_texture(glow::TEXTURE_2D, Some(tex));
            self.ctx.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            self.ctx.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            self.ctx.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            self.ctx.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            self.ctx.bind_texture(glow::TEXTURE_2D, None);

            // Static unit quad for `Custom2DPassKind::Shader` uploaded once.
            const UNIT_QUAD: [f32; 12] =
                [0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0];
            let mut quad_bytes = [0u8; 48];
            for (i, f) in UNIT_QUAD.iter().enumerate() {
                quad_bytes[i * 4..i * 4 + 4].copy_from_slice(&f.to_le_bytes());
            }
            let q_vao = self.ctx.create_vertex_array()?;
            let q_vbo = self.ctx.create_buffer()?;
            self.ctx.bind_vertex_array(Some(q_vao));
            self.ctx.bind_buffer(glow::ARRAY_BUFFER, Some(q_vbo));
            self.ctx
                .buffer_data_u8_slice(glow::ARRAY_BUFFER, &quad_bytes, glow::STATIC_DRAW);
            self.ctx.enable_vertex_attrib_array(0);
            self.ctx
                .vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
            self.ctx.vertex_attrib_divisor(0, 0);
            self.ctx.bind_vertex_array(None);
            self.ctx.bind_buffer(glow::ARRAY_BUFFER, None);

            let c_vao = self.ctx.create_vertex_array()?;
            let c_vbo = self.ctx.create_buffer()?;
            let c_ivbo = self.ctx.create_buffer()?;

            let sdf_u = SdfUniforms {
                viewport: self.ctx.get_uniform_location(sdf_program, "u_viewport"),
                atlas: self.ctx.get_uniform_location(sdf_program, "u_atlas"),
                backdrop: self.ctx.get_uniform_location(sdf_program, "u_backdrop"),
                user_tex: self.ctx.get_uniform_location(sdf_program, "u_user_tex"),
                has_user_tex: self.ctx.get_uniform_location(sdf_program, "u_has_user_tex"),
            };
            let kd_u = KawaseUniforms {
                tex: self.ctx.get_uniform_location(kawase_down_program, "u_tex"),
                half_pixel: self
                    .ctx
                    .get_uniform_location(kawase_down_program, "u_half_pixel"),
            };
            let ku_u = KawaseUniforms {
                tex: self.ctx.get_uniform_location(kawase_up_program, "u_tex"),
                half_pixel: self
                    .ctx
                    .get_uniform_location(kawase_up_program, "u_half_pixel"),
            };
            let blit_u = BlitUniforms {
                tex: self.ctx.get_uniform_location(blit_program, "u_tex"),
            };
            let m3d_u = Mesh3DUniforms {
                mvp: self.ctx.get_uniform_location(mesh3d_program, "u_mvp"),
                model: self.ctx.get_uniform_location(mesh3d_program, "u_model"),
                view: self.ctx.get_uniform_location(mesh3d_program, "u_view"),
                base_color: self
                    .ctx
                    .get_uniform_location(mesh3d_program, "u_base_color"),
                roughness: self.ctx.get_uniform_location(mesh3d_program, "u_roughness"),
                albedo_tex: self
                    .ctx
                    .get_uniform_location(mesh3d_program, "u_albedo_tex"),
                has_albedo_tex: self
                    .ctx
                    .get_uniform_location(mesh3d_program, "u_has_albedo_tex"),
                camera_pos: self
                    .ctx
                    .get_uniform_location(mesh3d_program, "u_camera_pos"),
                glass_params: self
                    .ctx
                    .get_uniform_location(mesh3d_program, "u_glass_params"),
                backdrop_tex: self
                    .ctx
                    .get_uniform_location(mesh3d_program, "u_backdrop_tex"),
                has_backdrop: self
                    .ctx
                    .get_uniform_location(mesh3d_program, "u_has_backdrop"),
            };

            #[cfg(not(target_arch = "wasm32"))]
            let tq = if !self.is_embedded {
                self.ctx.create_query().ok()
            } else {
                None
            };
            #[cfg(target_arch = "wasm32")]
            let tq: Option<()> = None;

            (
                vao, vbo, tex, q_vao, q_vbo, c_vao, c_vbo, c_ivbo, sdf_u, kd_u, ku_u, blit_u,
                m3d_u, tq,
            )
        };
        #[cfg(target_arch = "wasm32")]
        let _ = timer_query;

        self.pipeline = Some(GlPipeline {
            dummy_vao,
            instance_vbo,
            instance_vbo_cap: 0,
            atlas_tex,
            atlas_size: [0, 0],
            sdf_program,
            sdf_uniforms,
            kawase_down_program,
            kawase_down_uniforms,
            kawase_up_program,
            kawase_up_uniforms,
            blit_program,
            blit_uniforms,
            mesh3d_program,
            mesh3d_uniforms,
            scene_target: None,
            down_targets: Vec::new(),
            up_targets: Vec::new(),
            fbo_size: [0, 0],
            quad_vao,
            quad_vbo,
            custom_vao,
            custom_vbo,
            custom_inst_vbo,
            #[cfg(not(target_arch = "wasm32"))]
            timer_query,
            #[cfg(not(target_arch = "wasm32"))]
            timer_pending: false,
        });
        Ok(())
    }

    fn ensure_glass_fbos(&mut self, width: u32, height: u32) {
        let Some(pyramid) = compute_kawase_pyramid_sizes(width, height) else {
            // Minimized (0x0): skip FBO allocation or resizing.
            return;
        };
        if let Some(p) = &self.pipeline {
            if p.fbo_size == [width, height] && p.scene_target.is_some() {
                return;
            }
        }

        if let Some(p) = self.pipeline.as_mut() {
            if let Some(old) = p.scene_target.take() {
                // SAFETY: handles belong to `self.ctx`.
                unsafe {
                    self.ctx.delete_framebuffer(old.fbo);
                    self.ctx.delete_texture(old.tex);
                }
            }
            for old in p.down_targets.drain(..) {
                unsafe {
                    self.ctx.delete_framebuffer(old.fbo);
                    self.ctx.delete_texture(old.tex);
                }
            }
            for old in p.up_targets.drain(..) {
                unsafe {
                    self.ctx.delete_framebuffer(old.fbo);
                    self.ctx.delete_texture(old.tex);
                }
            }
        }

        let Ok(scene) = self.create_fbo_target(width, height) else {
            return;
        };
        let mut downs = Vec::with_capacity(3);
        for (pw, ph) in pyramid {
            if let Ok(t) = self.create_fbo_target(pw, ph) {
                downs.push(t);
            } else {
                self.destroy_fbo_target(scene);
                for d in downs {
                    self.destroy_fbo_target(d);
                }
                return;
            }
        }
        let mut ups = Vec::with_capacity(2);
        for (pw, ph) in [pyramid[1], pyramid[0]] {
            if let Ok(t) = self.create_fbo_target(pw, ph) {
                ups.push(t);
            } else {
                self.destroy_fbo_target(scene);
                for d in downs {
                    self.destroy_fbo_target(d);
                }
                for u in ups {
                    self.destroy_fbo_target(u);
                }
                return;
            }
        }

        if let Some(p) = self.pipeline.as_mut() {
            p.scene_target = Some(scene);
            p.down_targets = downs;
            p.up_targets = ups;
            p.fbo_size = [width, height];
        }
    }

    /// Uploads the RGBA8 glyph atlas texture to the GPU (full upload).
    #[allow(dead_code)]
    pub fn upload_atlas(&mut self, width: u32, height: u32, rgba: &[u8]) {
        self.upload_atlas_region(AtlasUpload::Full {
            width,
            height,
            pixels: rgba,
        });
    }

    /// Uploads either the full RGBA8 glyph atlas or only the dirty row band via `tex_sub_image_2d`.
    pub fn upload_atlas_region(&mut self, upload: AtlasUpload<'_>) {
        if self.ensure_pipeline().is_err() {
            return;
        }
        let Some(p) = self.pipeline.as_mut() else {
            return;
        };
        match upload {
            AtlasUpload::Full {
                width,
                height,
                pixels,
            } => {
                if width == 0 || height == 0 {
                    return;
                }
                let (Ok(wi), Ok(hi)) = (i32::try_from(width), i32::try_from(height)) else {
                    return;
                };
                self.last_atlas_bytes = pixels.len();
                p.atlas_size = [width, height];
                // SAFETY: `atlas_tex` is bound and `pixels` length matches uploaded `wi x hi` RGBA8 data.
                unsafe {
                    self.ctx.bind_texture(glow::TEXTURE_2D, Some(p.atlas_tex));
                    self.ctx.tex_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        glow::RGBA8 as i32,
                        wi,
                        hi,
                        0,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelUnpackData::Slice(Some(pixels)),
                    );
                    self.ctx.bind_texture(glow::TEXTURE_2D, None);
                }
            }
            AtlasUpload::SubRows {
                atlas_width,
                atlas_height,
                y,
                height,
                pixels,
            } => {
                if atlas_width == 0 || height == 0 || p.atlas_size != [atlas_width, atlas_height] {
                    return;
                }
                let (Ok(wi), Ok(yi), Ok(hi)) = (
                    i32::try_from(atlas_width),
                    i32::try_from(y),
                    i32::try_from(height),
                ) else {
                    return;
                };
                // SAFETY: `atlas_tex` already has storage `[atlas_width, atlas_height]` and `pixels` covers `wi x hi` rows at `yi`.
                unsafe {
                    self.ctx.bind_texture(glow::TEXTURE_2D, Some(p.atlas_tex));
                    self.ctx.tex_sub_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        0,
                        yi,
                        wi,
                        hi,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelUnpackData::Slice(Some(pixels)),
                    );
                    self.ctx.bind_texture(glow::TEXTURE_2D, None);
                }
            }
        }
    }

    /// Sets viewport and clears the default color buffer.
    #[allow(dead_code)]
    pub fn clear(&self, width: u32, height: u32, color: Color) {
        self.clear_target(None, width, height, color);
    }

    /// Binds `target_fbo` (`None` = default framebuffer), sets viewport, and clears the color buffer.
    pub(crate) fn clear_target(
        &self,
        target_fbo: Option<glow::Framebuffer>,
        width: u32,
        height: u32,
        color: Color,
    ) {
        if width == 0 || height == 0 {
            return;
        }
        let rgba = color.to_normalized_f32();
        let (Ok(w), Ok(h)) = (i32::try_from(width), i32::try_from(height)) else {
            return;
        };
        // SAFETY: `self.ctx` is bound to a valid active GL/WebGL 2 context.
        unsafe {
            self.ctx.bind_framebuffer(glow::FRAMEBUFFER, target_fbo);
            self.ctx.disable(glow::SCISSOR_TEST);
            self.ctx.viewport(0, 0, w, h);
            self.ctx.clear_color(rgba[0], rgba[1], rgba[2], rgba[3]);
            self.ctx.clear(glow::COLOR_BUFFER_BIT);
            if target_fbo.is_some() {
                self.ctx.bind_framebuffer(glow::FRAMEBUFFER, None);
            }
        }
    }

    /// Draws the instanced SDF batch into an opaque [`RenderTargetHandle`] (used by `gfx::Renderer`).
    pub(crate) fn render_sdf_batch_handle(
        &mut self,
        target: RenderTargetHandle,
        width: u32,
        height: u32,
        clear_color: Color,
        batch: &SdfBatch,
        instance_bytes: &[u8],
    ) {
        self.render_sdf_batch(target.0, width, height, clear_color, batch, instance_bytes);
    }

    /// Draws the instanced SDF batch into `target_fbo` (`None` = default framebuffer),
    /// running Dual-Kawase backdrop blur when `glass` is active.
    pub(crate) fn render_sdf_batch(
        &mut self,
        target_fbo: Option<glow::Framebuffer>,
        width: u32,
        height: u32,
        clear_color: Color,
        batch: &SdfBatch,
        instance_bytes: &[u8],
    ) {
        if width == 0 || height == 0 {
            return;
        }
        let (Ok(wi), Ok(hi)) = (i32::try_from(width), i32::try_from(height)) else {
            return;
        };
        if self.ensure_pipeline().is_err() {
            self.clear_target(target_fbo, width, height, clear_color);
            return;
        }
        if batch.has_glass {
            self.ensure_glass_fbos(width, height);
        }

        if (instance_bytes.is_empty() && batch.viewport_3d.is_empty() && batch.custom_2d.is_empty())
            || batch.runs.is_empty()
        {
            self.clear_target(target_fbo, width, height, clear_color);
            return;
        }
        self.last_sdf_bytes = instance_bytes.len();

        #[cfg(not(target_arch = "wasm32"))]
        let active_timer = if let Some(p) = self.pipeline.as_mut() {
            if let Some(q) = p.timer_query {
                if p.timer_pending {
                    // SAFETY: `q` is a valid query handle on the active GL context.
                    let avail = unsafe {
                        self.ctx
                            .get_query_parameter_u32(q, glow::QUERY_RESULT_AVAILABLE)
                    };
                    if avail != 0 {
                        // SAFETY: result is available.
                        let ns = unsafe { self.ctx.get_query_parameter_u32(q, glow::QUERY_RESULT) };
                        self.last_gpu_time_ms = Some((f64::from(ns) / 1_000_000.0) as f32);
                        p.timer_pending = false;
                    }
                }
                if !p.timer_pending {
                    // SAFETY: `q` is idle and ready for a new `TIME_ELAPSED` measurement.
                    unsafe {
                        self.ctx.begin_query(glow::TIME_ELAPSED, q);
                    }
                    p.timer_pending = true;
                    Some(q)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let (
            use_glass_pipeline,
            scene_fbo,
            scene_tex,
            down_info,
            up_info,
            kawase_down_program,
            kawase_up_program,
            blit_program,
            dummy_vao,
            blurred_tex,
        ) = {
            let Some(p) = self.pipeline.as_mut() else {
                return;
            };
            // SAFETY: `p.instance_vbo` is valid and bound to `ARRAY_BUFFER`.
            unsafe {
                self.ctx
                    .bind_buffer(glow::ARRAY_BUFFER, Some(p.instance_vbo));
                if !instance_bytes.is_empty() && instance_bytes.len() <= p.instance_vbo_cap {
                    self.ctx
                        .buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, instance_bytes);
                } else {
                    self.ctx.buffer_data_u8_slice(
                        glow::ARRAY_BUFFER,
                        instance_bytes,
                        glow::STREAM_DRAW,
                    );
                    p.instance_vbo_cap = instance_bytes.len();
                }
                self.ctx.bind_buffer(glow::ARRAY_BUFFER, None);
            }

            let use_glass = batch.has_glass
                && p.scene_target.is_some()
                && p.down_targets.len() == 3
                && p.up_targets.len() == 2;
            let downs = if use_glass {
                Some([
                    (
                        p.down_targets[0].fbo,
                        p.down_targets[0].w,
                        p.down_targets[0].h,
                        p.down_targets[0].tex,
                    ),
                    (
                        p.down_targets[1].fbo,
                        p.down_targets[1].w,
                        p.down_targets[1].h,
                        p.down_targets[1].tex,
                    ),
                    (
                        p.down_targets[2].fbo,
                        p.down_targets[2].w,
                        p.down_targets[2].h,
                        p.down_targets[2].tex,
                    ),
                ])
            } else {
                None
            };
            let ups = if use_glass {
                Some([
                    (
                        p.up_targets[0].fbo,
                        p.up_targets[0].w,
                        p.up_targets[0].h,
                        p.up_targets[0].tex,
                    ),
                    (
                        p.up_targets[1].fbo,
                        p.up_targets[1].w,
                        p.up_targets[1].h,
                        p.up_targets[1].tex,
                    ),
                ])
            } else {
                None
            };
            let blur_tex = ups.map(|u| u[1].3);
            (
                use_glass,
                p.scene_target.as_ref().map(|t| t.fbo),
                p.scene_target.as_ref().map(|t| t.tex),
                downs,
                ups,
                p.kawase_down_program,
                p.kawase_up_program,
                p.blit_program,
                p.dummy_vao,
                blur_tex,
            )
        };

        let rgba = clear_color.to_normalized_f32();

        if use_glass_pipeline {
            let split_idx = batch.first_glass_run.unwrap_or(0);
            let scene_fbo = scene_fbo.expect("checked");
            let scene_tex = scene_tex.expect("checked");
            let down_info = down_info.expect("checked");
            let up_info = up_info.expect("checked");

            // Pass 1: Draw pre-glass commands into `scene_target`.
            // SAFETY: all FBO, VAO, shader, and texture handles belong to `self.ctx`.
            unsafe {
                self.ctx
                    .bind_framebuffer(glow::FRAMEBUFFER, Some(scene_fbo));
                self.ctx.disable(glow::SCISSOR_TEST);
                self.ctx.viewport(0, 0, wi, hi);
                self.ctx.clear_color(rgba[0], rgba[1], rgba[2], rgba[3]);
                self.ctx.clear(glow::COLOR_BUFFER_BIT);
                self.draw_runs(wi, hi, batch, 0..split_idx, None, Some(scene_fbo));

                // Pass 2: Dual-Kawase downsample + upsample pyramid.
                self.ctx.disable(glow::SCISSOR_TEST);
                self.ctx.disable(glow::BLEND);
                self.ctx.bind_vertex_array(Some(dummy_vao));

                let mut prev_tex = scene_tex;
                let mut prev_w = width;
                let mut prev_h = height;

                self.ctx.use_program(Some(kawase_down_program));
                if let Some(p) = &self.pipeline {
                    self.ctx
                        .uniform_1_i32(p.kawase_down_uniforms.tex.as_ref(), 0);
                }
                self.ctx.active_texture(glow::TEXTURE0);

                for (dst_fbo, dst_w, dst_h, dst_tex) in down_info {
                    self.ctx.bind_framebuffer(glow::FRAMEBUFFER, Some(dst_fbo));
                    self.ctx.viewport(0, 0, dst_w as i32, dst_h as i32);
                    self.ctx.bind_texture(glow::TEXTURE_2D, Some(prev_tex));
                    if let Some(p) = &self.pipeline {
                        self.ctx.uniform_2_f32(
                            p.kawase_down_uniforms.half_pixel.as_ref(),
                            0.5 / prev_w.max(1) as f32,
                            0.5 / prev_h.max(1) as f32,
                        );
                    }
                    self.ctx.draw_arrays(glow::TRIANGLES, 0, 3);
                    prev_tex = dst_tex;
                    prev_w = dst_w;
                    prev_h = dst_h;
                }

                self.ctx.use_program(Some(kawase_up_program));
                if let Some(p) = &self.pipeline {
                    self.ctx.uniform_1_i32(p.kawase_up_uniforms.tex.as_ref(), 0);
                }

                for (dst_fbo, dst_w, dst_h, dst_tex) in up_info {
                    self.ctx.bind_framebuffer(glow::FRAMEBUFFER, Some(dst_fbo));
                    self.ctx.viewport(0, 0, dst_w as i32, dst_h as i32);
                    self.ctx.bind_texture(glow::TEXTURE_2D, Some(prev_tex));
                    if let Some(p) = &self.pipeline {
                        self.ctx.uniform_2_f32(
                            p.kawase_up_uniforms.half_pixel.as_ref(),
                            0.5 / dst_w.max(1) as f32,
                            0.5 / dst_h.max(1) as f32,
                        );
                    }
                    self.ctx.draw_arrays(glow::TRIANGLES, 0, 3);
                    prev_tex = dst_tex;
                }

                // Pass 3: Blit `scene` to `target_fbo`, then draw glass + remaining runs.
                self.ctx.bind_framebuffer(glow::FRAMEBUFFER, target_fbo);
                self.ctx.disable(glow::BLEND);
                self.ctx.viewport(0, 0, wi, hi);
                self.ctx.use_program(Some(blit_program));
                if let Some(p) = &self.pipeline {
                    self.ctx.uniform_1_i32(p.blit_uniforms.tex.as_ref(), 0);
                }
                self.ctx.bind_texture(glow::TEXTURE_2D, Some(scene_tex));
                self.ctx.draw_arrays(glow::TRIANGLES, 0, 3);

                self.draw_runs(
                    wi,
                    hi,
                    batch,
                    split_idx..batch.runs.len(),
                    blurred_tex,
                    target_fbo,
                );
            }
        } else {
            // SAFETY: `target_fbo` (or default framebuffer) is active and pipeline handles belong to `self.ctx`.
            unsafe {
                self.ctx.bind_framebuffer(glow::FRAMEBUFFER, target_fbo);
                self.ctx.disable(glow::SCISSOR_TEST);
                self.ctx.viewport(0, 0, wi, hi);
                self.ctx.clear_color(rgba[0], rgba[1], rgba[2], rgba[3]);
                self.ctx.clear(glow::COLOR_BUFFER_BIT);
                self.draw_runs(wi, hi, batch, 0..batch.runs.len(), None, target_fbo);
            }
        }

        if target_fbo.is_some() {
            // SAFETY: unbinding to default framebuffer leaves GL state clean for external callers.
            unsafe {
                self.ctx.bind_framebuffer(glow::FRAMEBUFFER, None);
            }
        }

        #[cfg(not(target_arch = "wasm32"))]
        if active_timer.is_some() {
            // SAFETY: matches the `begin_query(TIME_ELAPSED, ...)` started at the top of `render_sdf_batch`.
            unsafe {
                self.ctx.end_query(glow::TIME_ELAPSED);
            }
        }
    }

    /// # Safety
    /// Active GL context must be current and all pipeline handles must belong to `self.ctx`.
    unsafe fn bind_sdf_state(&self, wi: i32, hi: i32, backdrop_tex: Option<glow::Texture>) {
        let Some(p) = &self.pipeline else {
            return;
        };
        self.ctx.enable(glow::BLEND);
        self.ctx.blend_func_separate(
            glow::SRC_ALPHA,
            glow::ONE_MINUS_SRC_ALPHA,
            glow::ONE,
            glow::ONE_MINUS_SRC_ALPHA,
        );
        self.ctx.use_program(Some(p.sdf_program));
        self.ctx.bind_vertex_array(Some(p.dummy_vao));
        self.ctx
            .bind_buffer(glow::ARRAY_BUFFER, Some(p.instance_vbo));

        self.ctx
            .uniform_2_f32(p.sdf_uniforms.viewport.as_ref(), wi as f32, hi as f32);
        self.ctx.uniform_1_i32(p.sdf_uniforms.atlas.as_ref(), 0);
        self.ctx.uniform_1_i32(p.sdf_uniforms.backdrop.as_ref(), 1);
        self.ctx.uniform_1_i32(p.sdf_uniforms.user_tex.as_ref(), 2);
        self.ctx
            .uniform_1_i32(p.sdf_uniforms.has_user_tex.as_ref(), 0);

        self.ctx.active_texture(glow::TEXTURE0);
        self.ctx.bind_texture(glow::TEXTURE_2D, Some(p.atlas_tex));

        self.ctx.active_texture(glow::TEXTURE1);
        self.ctx
            .bind_texture(glow::TEXTURE_2D, backdrop_tex.or(Some(p.atlas_tex)));

        self.ctx.active_texture(glow::TEXTURE2);
        self.ctx.bind_texture(glow::TEXTURE_2D, Some(p.atlas_tex));
    }

    /// # Safety
    /// Active GL context must be current; returns `(vao, index_count)` for an immutable `Arc<MeshData>`.
    unsafe fn ensure_cached_mesh3d(
        &mut self,
        mesh: &Arc<MeshData>,
    ) -> Option<(glow::VertexArray, i32)> {
        let index_count = i32::try_from(mesh.indices.len()).ok()?;
        let key = Arc::as_ptr(mesh) as usize;
        if let Some(entry) = self.mesh3d_cache.get(&key) {
            if entry.owner.strong_count() > 0 {
                return Some((entry.vao, entry.index_count));
            }
        }
        if let Some(stale) = self.mesh3d_cache.remove(&key) {
            self.ctx.delete_vertex_array(stale.vao);
            self.ctx.delete_buffer(stale.vbo);
            self.ctx.delete_buffer(stale.ebo);
        }
        if self.mesh3d_cache.len() >= 64 {
            let dead: Vec<usize> = self
                .mesh3d_cache
                .iter()
                .filter(|(_, e)| e.owner.strong_count() == 0)
                .map(|(&k, _)| k)
                .collect();
            for dk in dead {
                if let Some(stale) = self.mesh3d_cache.remove(&dk) {
                    self.ctx.delete_vertex_array(stale.vao);
                    self.ctx.delete_buffer(stale.vbo);
                    self.ctx.delete_buffer(stale.ebo);
                }
            }
        }

        self.v3d_scratch.clear();
        self.v3d_scratch
            .reserve(mesh.vertices.len().saturating_mul(32));
        for v in &mesh.vertices {
            for c in v.pos {
                self.v3d_scratch.extend_from_slice(&c.to_le_bytes());
            }
            for c in v.normal {
                self.v3d_scratch.extend_from_slice(&c.to_le_bytes());
            }
            for c in v.uv {
                self.v3d_scratch.extend_from_slice(&c.to_le_bytes());
            }
        }
        self.i3d_scratch.clear();
        self.i3d_scratch
            .reserve(mesh.indices.len().saturating_mul(4));
        for &idx in &mesh.indices {
            self.i3d_scratch.extend_from_slice(&idx.to_le_bytes());
        }

        let vao = self.ctx.create_vertex_array().ok()?;
        let vbo = match self.ctx.create_buffer() {
            Ok(b) => b,
            Err(_) => {
                self.ctx.delete_vertex_array(vao);
                return None;
            }
        };
        let ebo = match self.ctx.create_buffer() {
            Ok(b) => b,
            Err(_) => {
                self.ctx.delete_buffer(vbo);
                self.ctx.delete_vertex_array(vao);
                return None;
            }
        };

        self.ctx.bind_vertex_array(Some(vao));
        self.ctx.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        self.ctx
            .buffer_data_u8_slice(glow::ARRAY_BUFFER, &self.v3d_scratch, glow::STATIC_DRAW);
        self.ctx.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ebo));
        self.ctx.buffer_data_u8_slice(
            glow::ELEMENT_ARRAY_BUFFER,
            &self.i3d_scratch,
            glow::STATIC_DRAW,
        );
        let v3d_stride = 32_i32;
        self.ctx.enable_vertex_attrib_array(0);
        self.ctx
            .vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, v3d_stride, 0);
        self.ctx.enable_vertex_attrib_array(1);
        self.ctx
            .vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, v3d_stride, 12);
        self.ctx.enable_vertex_attrib_array(2);
        self.ctx
            .vertex_attrib_pointer_f32(2, 2, glow::FLOAT, false, v3d_stride, 24);
        self.ctx.bind_vertex_array(None);
        self.ctx.bind_buffer(glow::ARRAY_BUFFER, None);
        self.ctx.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, None);

        let byte_size = self
            .v3d_scratch
            .len()
            .saturating_add(self.i3d_scratch.len());
        self.mesh3d_cache.insert(
            key,
            CachedMesh3D {
                owner: Arc::downgrade(mesh),
                vao,
                vbo,
                ebo,
                index_count,
                byte_size,
            },
        );
        Some((vao, index_count))
    }

    /// # Safety
    /// Active GL context must be current and `vp` must contain valid finite transform matrices.
    unsafe fn render_viewport_3d_pass(
        &mut self,
        wi: i32,
        hi: i32,
        scissor: Option<[i32; 4]>,
        vp: &Viewport3DPass,
        target_fbo: Option<glow::Framebuffer>,
    ) {
        if vp.scene.draws.is_empty() || vp.rect.w < 1.0 || vp.rect.h < 1.0 {
            return;
        }
        let vpw = (vp.rect.w.round() as u32).max(1);
        let vph = (vp.rect.h.round() as u32).max(1);
        self.ensure_3d_fbo(vpw, vph, vp.scene.msaa_samples);

        let (
            Some(t3d_fbo),
            msaa_fbo,
            Some(t3d_tex),
            Some(t3d_backdrop_tex),
            Some(mesh3d_program),
            Some(blit_program),
            Some(dummy_vao),
        ) = (
            self.viewport3d_target.as_ref().map(|t| t.fbo),
            self.viewport3d_target.as_ref().and_then(|t| t.msaa_fbo),
            self.viewport3d_target.as_ref().map(|t| t.tex),
            self.viewport3d_target.as_ref().map(|t| t.backdrop_tex),
            self.pipeline.as_ref().map(|p| p.mesh3d_program),
            self.pipeline.as_ref().map(|p| p.blit_program),
            self.pipeline.as_ref().map(|p| p.dummy_vao),
        )
        else {
            return;
        };

        let vx = vp.rect.x.round() as i32;
        let vy_gl = hi.saturating_sub(vp.rect.y.round() as i32 + vph as i32);

        let needs_backdrop = vp
            .scene
            .draws
            .iter()
            .any(|d| d.glass.is_some() || d.custom_shader.is_some());
        let mut has_backdrop = false;
        if needs_backdrop {
            let src_x = vx.clamp(0, wi);
            let src_y = vy_gl.clamp(0, hi);
            let dst_x = (src_x - vx).max(0);
            let dst_y = (src_y - vy_gl).max(0);
            let copy_w = (vpw as i32 - dst_x).min(wi - src_x);
            let copy_h = (vph as i32 - dst_y).min(hi - src_y);
            if copy_w > 0 && copy_h > 0 {
                self.ctx
                    .bind_framebuffer(glow::READ_FRAMEBUFFER, target_fbo);
                self.ctx.active_texture(glow::TEXTURE1);
                self.ctx
                    .bind_texture(glow::TEXTURE_2D, Some(t3d_backdrop_tex));
                self.ctx.copy_tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    dst_x,
                    dst_y,
                    src_x,
                    src_y,
                    copy_w,
                    copy_h,
                );
                has_backdrop = true;
            }
        }

        let aspect = (vpw as f32 / vph.max(1) as f32).max(0.1);
        let view_mat = flatten_mat4(camera_view(&vp.scene.camera));
        let vp_mat = camera_view_proj(&vp.scene.camera, aspect);
        let cam_pos = vp.scene.camera.position;

        self.ctx
            .bind_framebuffer(glow::FRAMEBUFFER, Some(msaa_fbo.unwrap_or(t3d_fbo)));
        self.ctx.disable(glow::SCISSOR_TEST);
        self.ctx.viewport(0, 0, vpw as i32, vph as i32);
        self.ctx.enable(glow::DEPTH_TEST);
        self.ctx.depth_func(glow::LESS);
        self.ctx.depth_mask(true);
        self.ctx.enable(glow::CULL_FACE);
        self.ctx.cull_face(glow::BACK);
        self.ctx.front_face(glow::CCW);
        self.ctx.disable(glow::BLEND);
        self.ctx.clear_color(0.0, 0.0, 0.0, 0.0);
        self.ctx
            .clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);

        self.ctx.use_program(Some(mesh3d_program));
        if has_backdrop {
            self.ctx.active_texture(glow::TEXTURE1);
            self.ctx
                .bind_texture(glow::TEXTURE_2D, Some(t3d_backdrop_tex));
        }
        if let Some(p) = &self.pipeline {
            self.ctx
                .uniform_1_i32(p.mesh3d_uniforms.albedo_tex.as_ref(), 0);
            self.ctx
                .uniform_1_i32(p.mesh3d_uniforms.backdrop_tex.as_ref(), 1);
            self.ctx.uniform_1_i32(
                p.mesh3d_uniforms.has_backdrop.as_ref(),
                i32::from(has_backdrop),
            );
            self.ctx
                .uniform_matrix_4_f32_slice(p.mesh3d_uniforms.view.as_ref(), false, &view_mat);
            self.ctx.uniform_3_f32(
                p.mesh3d_uniforms.camera_pos.as_ref(),
                cam_pos[0],
                cam_pos[1],
                cam_pos[2],
            );
        }

        let mut has_translucent_or_glass = false;
        let mut has_custom_shader = false;
        for opaque_pass in [true, false] {
            if !opaque_pass {
                if !has_translucent_or_glass {
                    break;
                }
                self.ctx.enable(glow::BLEND);
                self.ctx.blend_func_separate(
                    glow::SRC_ALPHA,
                    glow::ONE_MINUS_SRC_ALPHA,
                    glow::ONE,
                    glow::ONE_MINUS_SRC_ALPHA,
                );
            }
            for draw in &vp.scene.draws {
                if draw.custom_shader.is_some() {
                    has_custom_shader = true;
                    continue;
                }
                let is_glass = draw.glass.is_some();
                let is_opaque = !is_glass && draw.material.base_color.a == 255;
                if opaque_pass && !is_opaque {
                    if is_glass || draw.material.base_color.a > 0 {
                        has_translucent_or_glass = true;
                    }
                    continue;
                }
                if !opaque_pass && (is_opaque || (!is_glass && draw.material.base_color.a == 0)) {
                    continue;
                }
                if draw.mesh.vertices.is_empty() || draw.mesh.indices.is_empty() {
                    continue;
                }
                let Some((vao, cnt)) = self.ensure_cached_mesh3d(&draw.mesh) else {
                    continue;
                };
                if !opaque_pass {
                    // Glass writes depth so overlapping lobes/letters occlude cleanly; flat alpha keeps depth read-only.
                    self.ctx.depth_mask(is_glass);
                }
                self.ctx.bind_vertex_array(Some(vao));

                let mvp = flatten_mat4(mat4_mul(vp_mat, draw.model_mat));
                let model = flatten_mat4(draw.model_mat);
                let col = draw.material.base_color.to_normalized_f32();
                let gl_tex = draw
                    .material
                    .texture
                    .and_then(|id| self.user_textures.get(&id).map(|&(t, _, _)| t));
                if let Some(p) = &self.pipeline {
                    self.ctx.uniform_matrix_4_f32_slice(
                        p.mesh3d_uniforms.mvp.as_ref(),
                        false,
                        &mvp,
                    );
                    self.ctx.uniform_matrix_4_f32_slice(
                        p.mesh3d_uniforms.model.as_ref(),
                        false,
                        &model,
                    );
                    self.ctx.uniform_4_f32(
                        p.mesh3d_uniforms.base_color.as_ref(),
                        col[0],
                        col[1],
                        col[2],
                        col[3],
                    );
                    self.ctx.uniform_1_f32(
                        p.mesh3d_uniforms.roughness.as_ref(),
                        draw.material.roughness,
                    );
                    if let Some(g) = draw.glass {
                        self.ctx.uniform_4_f32(
                            p.mesh3d_uniforms.glass_params.as_ref(),
                            g.transmission,
                            g.ior,
                            g.dispersion,
                            g.thickness,
                        );
                    } else {
                        self.ctx.uniform_4_f32(
                            p.mesh3d_uniforms.glass_params.as_ref(),
                            0.0,
                            1.5,
                            0.0,
                            0.0,
                        );
                    }
                    if let Some(tex) = gl_tex {
                        self.ctx.active_texture(glow::TEXTURE0);
                        self.ctx.bind_texture(glow::TEXTURE_2D, Some(tex));
                        self.ctx
                            .uniform_1_i32(p.mesh3d_uniforms.has_albedo_tex.as_ref(), 1);
                    } else {
                        self.ctx
                            .uniform_1_i32(p.mesh3d_uniforms.has_albedo_tex.as_ref(), 0);
                    }
                }

                self.ctx
                    .draw_elements(glow::TRIANGLES, cnt, glow::UNSIGNED_INT, 0);
            }
        }

        if has_custom_shader {
            self.ctx.enable(glow::BLEND);
            self.ctx.blend_func_separate(
                glow::SRC_ALPHA,
                glow::ONE_MINUS_SRC_ALPHA,
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
            );
            self.ctx.depth_mask(true);
            for draw in &vp.scene.draws {
                let Some(ref cs) = draw.custom_shader else {
                    continue;
                };
                if draw.mesh.vertices.is_empty() || draw.mesh.indices.is_empty() {
                    continue;
                }
                let src_hash = hash_shader_pair(MESH3D_VERT_BODY, &cs.frag_glsl);
                let needs_compile = self
                    .custom_3d_programs
                    .get(&cs.key)
                    .is_none_or(|e| e.src_hash != src_hash);
                if needs_compile {
                    if let Some(old) = self.custom_3d_programs.remove(&cs.key) {
                        self.ctx.delete_program(old.program);
                    }
                    let Ok(np) = self.compile_program(MESH3D_VERT_BODY, &cs.frag_glsl) else {
                        continue;
                    };
                    let entry = Custom3DProgramEntry {
                        src_hash,
                        program: np,
                        u_mvp: self.ctx.get_uniform_location(np, "u_mvp"),
                        u_model: self.ctx.get_uniform_location(np, "u_model"),
                        u_view: self.ctx.get_uniform_location(np, "u_view"),
                        u_camera_pos: self.ctx.get_uniform_location(np, "u_camera_pos"),
                        u_resolution: self.ctx.get_uniform_location(np, "u_resolution"),
                        u_time: self.ctx.get_uniform_location(np, "u_time"),
                        u_backdrop_tex: self.ctx.get_uniform_location(np, "u_backdrop_tex"),
                        u_has_backdrop: self.ctx.get_uniform_location(np, "u_has_backdrop"),
                        u_uniforms: self.ctx.get_uniform_location(np, "u_uniforms"),
                    };
                    self.custom_3d_programs.insert(cs.key, entry);
                }
                let Some((vao, cnt)) = self.ensure_cached_mesh3d(&draw.mesh) else {
                    continue;
                };
                let Some(entry) = self.custom_3d_programs.get(&cs.key) else {
                    continue;
                };
                self.ctx.use_program(Some(entry.program));
                self.ctx.bind_vertex_array(Some(vao));
                if has_backdrop {
                    self.ctx.active_texture(glow::TEXTURE1);
                    self.ctx
                        .bind_texture(glow::TEXTURE_2D, Some(t3d_backdrop_tex));
                }
                let mvp = flatten_mat4(mat4_mul(vp_mat, draw.model_mat));
                let model = flatten_mat4(draw.model_mat);
                self.ctx
                    .uniform_matrix_4_f32_slice(entry.u_mvp.as_ref(), false, &mvp);
                self.ctx
                    .uniform_matrix_4_f32_slice(entry.u_model.as_ref(), false, &model);
                self.ctx
                    .uniform_matrix_4_f32_slice(entry.u_view.as_ref(), false, &view_mat);
                self.ctx.uniform_3_f32(
                    entry.u_camera_pos.as_ref(),
                    cam_pos[0],
                    cam_pos[1],
                    cam_pos[2],
                );
                self.ctx
                    .uniform_2_f32(entry.u_resolution.as_ref(), vpw as f32, vph as f32);
                self.ctx
                    .uniform_1_f32(entry.u_time.as_ref(), self.frame_time_s);
                self.ctx.uniform_1_i32(entry.u_backdrop_tex.as_ref(), 1);
                self.ctx
                    .uniform_1_i32(entry.u_has_backdrop.as_ref(), i32::from(has_backdrop));
                if !cs.uniforms.is_empty() {
                    self.ctx
                        .uniform_1_f32_slice(entry.u_uniforms.as_ref(), &cs.uniforms);
                }
                self.ctx
                    .draw_elements(glow::TRIANGLES, cnt, glow::UNSIGNED_INT, 0);
            }
        }
        self.ctx.depth_mask(true);
        self.ctx.disable(glow::BLEND);

        self.ctx.disable(glow::CULL_FACE);
        self.ctx.disable(glow::DEPTH_TEST);
        if let Some(mfbo) = msaa_fbo {
            self.ctx
                .bind_framebuffer(glow::READ_FRAMEBUFFER, Some(mfbo));
            self.ctx
                .bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(t3d_fbo));
            self.ctx.blit_framebuffer(
                0,
                0,
                vpw as i32,
                vph as i32,
                0,
                0,
                vpw as i32,
                vph as i32,
                glow::COLOR_BUFFER_BIT,
                glow::NEAREST,
            );
        }
        self.ctx.bind_framebuffer(glow::FRAMEBUFFER, target_fbo);
        self.ctx.viewport(vx, vy_gl, vpw as i32, vph as i32);
        if let Some([sx, sy, sw, sh]) = scissor {
            self.ctx.enable(glow::SCISSOR_TEST);
            self.ctx.scissor(sx, sy, sw, sh);
        } else {
            self.ctx.disable(glow::SCISSOR_TEST);
        }
        self.ctx.enable(glow::BLEND);
        // `t3d_tex` is cleared to (0,0,0,0) and blended/MSAA-resolved into premultiplied RGBA;
        // composite with ONE / ONE_MINUS_SRC_ALPHA to avoid double-premultiplying silhouette edges.
        self.ctx.blend_func_separate(
            glow::ONE,
            glow::ONE_MINUS_SRC_ALPHA,
            glow::ONE,
            glow::ONE_MINUS_SRC_ALPHA,
        );
        self.ctx.use_program(Some(blit_program));
        self.ctx.bind_vertex_array(Some(dummy_vao));
        self.ctx.active_texture(glow::TEXTURE0);
        self.ctx.bind_texture(glow::TEXTURE_2D, Some(t3d_tex));
        if let Some(p) = &self.pipeline {
            self.ctx.uniform_1_i32(p.blit_uniforms.tex.as_ref(), 0);
        }
        self.ctx.draw_arrays(glow::TRIANGLES, 0, 3);
        self.ctx.viewport(0, 0, wi, hi);
    }

    /// # Safety
    /// Active GL context must be current and `pass` GLSL sources / vertex payloads must be valid.
    unsafe fn render_custom_2d_pass(&mut self, wi: i32, hi: i32, pass: &Custom2DPass) {
        if pass.rect.w < 1.0 || pass.rect.h < 1.0 {
            return;
        }
        let (vert_src, frag_src, key) = match &pass.kind {
            Custom2DPassKind::Shader {
                key,
                vert_glsl,
                frag_glsl,
                ..
            } => (
                vert_glsl.as_deref().unwrap_or(CUSTOM_QUAD_VERT_BODY),
                frag_glsl.as_str(),
                *key,
            ),
            Custom2DPassKind::Instanced {
                key,
                vert_glsl,
                frag_glsl,
                ..
            } => (vert_glsl.as_str(), frag_glsl.as_str(), *key),
        };
        let src_hash = hash_shader_pair(vert_src, frag_src);
        let needs_compile = self
            .custom_programs
            .get(&key)
            .is_none_or(|e| e.src_hash != src_hash);
        if needs_compile {
            if let Some(old) = self.custom_programs.remove(&key) {
                self.ctx.delete_program(old.program);
            }
            let Ok(np) = self.compile_program(vert_src, frag_src) else {
                return;
            };
            let entry = CustomProgramEntry {
                src_hash,
                program: np,
                u_rect: self.ctx.get_uniform_location(np, "u_rect"),
                u_viewport: self.ctx.get_uniform_location(np, "u_viewport"),
                u_resolution: self.ctx.get_uniform_location(np, "u_resolution"),
                u_time: self.ctx.get_uniform_location(np, "u_time"),
                u_pointer: self.ctx.get_uniform_location(np, "u_pointer"),
                u_scroll: self.ctx.get_uniform_location(np, "u_scroll"),
                u_uniforms: self.ctx.get_uniform_location(np, "u_uniforms"),
            };
            self.custom_programs.insert(key, entry);
        }

        let Some((quad_vao, c_vao, c_vbo, c_ivbo)) = self
            .pipeline
            .as_ref()
            .map(|p| (p.quad_vao, p.custom_vao, p.custom_vbo, p.custom_inst_vbo))
        else {
            return;
        };
        let Some(entry) = self.custom_programs.get(&key) else {
            return;
        };

        if let Some([sx, sy, sw, sh]) = pass.scissor {
            self.ctx.enable(glow::SCISSOR_TEST);
            self.ctx.scissor(sx, sy, sw, sh);
        } else {
            self.ctx.disable(glow::SCISSOR_TEST);
        }
        self.ctx.enable(glow::BLEND);
        self.ctx.blend_func_separate(
            glow::SRC_ALPHA,
            glow::ONE_MINUS_SRC_ALPHA,
            glow::ONE,
            glow::ONE_MINUS_SRC_ALPHA,
        );
        self.ctx.use_program(Some(entry.program));

        let bind_common_uniforms = |uniforms: &[f32]| {
            self.ctx.uniform_4_f32(
                entry.u_rect.as_ref(),
                pass.rect.x,
                pass.rect.y,
                pass.rect.w,
                pass.rect.h,
            );
            self.ctx
                .uniform_2_f32(entry.u_viewport.as_ref(), wi as f32, hi as f32);
            self.ctx
                .uniform_2_f32(entry.u_resolution.as_ref(), pass.rect.w, pass.rect.h);
            self.ctx
                .uniform_1_f32(entry.u_time.as_ref(), self.frame_time_s);
            self.ctx.uniform_2_f32(
                entry.u_pointer.as_ref(),
                self.frame_pointer[0],
                self.frame_pointer[1],
            );
            self.ctx.uniform_2_f32(
                entry.u_scroll.as_ref(),
                self.frame_scroll[0],
                self.frame_scroll[1],
            );
            if !uniforms.is_empty() {
                self.ctx
                    .uniform_1_f32_slice(entry.u_uniforms.as_ref(), uniforms);
            }
        };

        match &pass.kind {
            Custom2DPassKind::Shader { uniforms, .. } => {
                bind_common_uniforms(uniforms);
                self.ctx.bind_vertex_array(Some(quad_vao));
                self.ctx.draw_arrays(glow::TRIANGLES, 0, 6);
            }
            Custom2DPassKind::Instanced {
                vertices,
                instances,
                stride_floats,
                uniforms,
                ..
            } => {
                if vertices.is_empty() || instances.is_empty() || *stride_floats == 0 {
                    return;
                }
                let inst_count = instances.len() / *stride_floats;
                if inst_count == 0 {
                    return;
                }
                bind_common_uniforms(uniforms);

                self.custom_v_scratch.clear();
                self.custom_v_scratch
                    .reserve(vertices.len().saturating_mul(16));
                for v in vertices {
                    for f in v {
                        self.custom_v_scratch.extend_from_slice(&f.to_le_bytes());
                    }
                }
                self.custom_i_scratch.clear();
                self.custom_i_scratch
                    .reserve(instances.len().saturating_mul(4));
                for &f in instances {
                    self.custom_i_scratch.extend_from_slice(&f.to_le_bytes());
                }

                self.ctx.bind_vertex_array(Some(c_vao));
                self.ctx.bind_buffer(glow::ARRAY_BUFFER, Some(c_vbo));
                self.ctx.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    &self.custom_v_scratch,
                    glow::STREAM_DRAW,
                );
                self.ctx.enable_vertex_attrib_array(0);
                self.ctx
                    .vertex_attrib_pointer_f32(0, 4, glow::FLOAT, false, 16, 0);
                self.ctx.vertex_attrib_divisor(0, 0);

                self.ctx.bind_buffer(glow::ARRAY_BUFFER, Some(c_ivbo));
                self.ctx.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    &self.custom_i_scratch,
                    glow::STREAM_DRAW,
                );
                let byte_stride = (*stride_floats * 4) as i32;
                let vec4_slots = (*stride_floats).div_ceil(4).min(6);
                for s in 0..vec4_slots {
                    let loc = 1 + s as u32;
                    let elems = ((*stride_floats - s * 4).min(4)) as i32;
                    self.ctx.enable_vertex_attrib_array(loc);
                    self.ctx.vertex_attrib_pointer_f32(
                        loc,
                        elems,
                        glow::FLOAT,
                        false,
                        byte_stride,
                        (s * 16) as i32,
                    );
                    self.ctx.vertex_attrib_divisor(loc, 1);
                }

                if let (Ok(vc), Ok(ic)) = (i32::try_from(vertices.len()), i32::try_from(inst_count))
                {
                    self.ctx.draw_arrays_instanced(glow::TRIANGLES, 0, vc, ic);
                }
                for s in 0..vec4_slots {
                    self.ctx.disable_vertex_attrib_array(1 + s as u32);
                }
            }
        }
    }

    /// # Safety
    /// Active GL context must be current and all pipeline handles must belong to `self.ctx`.
    unsafe fn draw_runs(
        &mut self,
        wi: i32,
        hi: i32,
        batch: &SdfBatch,
        run_range: core::ops::Range<usize>,
        backdrop_tex: Option<glow::Texture>,
        target_fbo: Option<glow::Framebuffer>,
    ) {
        if run_range.is_empty()
            || (batch.instances.is_empty()
                && batch.viewport_3d.is_empty()
                && batch.custom_2d.is_empty())
        {
            return;
        }

        self.bind_sdf_state(wi, hi, backdrop_tex);

        let stride = size_of::<SdfInstance>() as i32;
        for run_idx in run_range {
            let run = batch.runs[run_idx];
            for c2d in batch.custom_2d.iter().filter(|c| c.run_index == run_idx) {
                self.render_custom_2d_pass(wi, hi, c2d);
                self.bind_sdf_state(wi, hi, backdrop_tex);
            }
            if let Some(vp) = batch.viewport_3d.iter().find(|v| v.run_index == run_idx) {
                self.render_viewport_3d_pass(wi, hi, run.scissor, vp, target_fbo);
                self.bind_sdf_state(wi, hi, backdrop_tex);
            }
            if run.count == 0 {
                continue;
            }
            if let Some([sx, sy, sw, sh]) = run.scissor {
                self.ctx.enable(glow::SCISSOR_TEST);
                self.ctx.scissor(sx, sy, sw, sh);
            } else {
                self.ctx.disable(glow::SCISSOR_TEST);
            }
            let bound_custom = if let Some((tex_id, filter)) = run.custom_tex {
                if let Some(&(gl_tex, _, _)) = self.user_textures.get(&tex_id) {
                    let fmode = match filter {
                        TextureFilter::Nearest => glow::NEAREST as i32,
                        TextureFilter::Linear => glow::LINEAR as i32,
                    };
                    self.ctx.active_texture(glow::TEXTURE2);
                    self.ctx.bind_texture(glow::TEXTURE_2D, Some(gl_tex));
                    self.ctx
                        .tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, fmode);
                    self.ctx
                        .tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, fmode);
                    if let Some(p) = &self.pipeline {
                        self.ctx
                            .uniform_1_i32(p.sdf_uniforms.has_user_tex.as_ref(), 1);
                    }
                    true
                } else {
                    if let Some(p) = &self.pipeline {
                        self.ctx
                            .uniform_1_i32(p.sdf_uniforms.has_user_tex.as_ref(), 0);
                    }
                    false
                }
            } else {
                false
            };
            let base_offset = (run.start as i32) * stride;
            for loc in 0..9u32 {
                self.ctx.vertex_attrib_pointer_f32(
                    loc,
                    4,
                    glow::FLOAT,
                    false,
                    stride,
                    base_offset + (loc as i32) * 16,
                );
            }
            self.ctx
                .draw_arrays_instanced(glow::TRIANGLES, 0, 6, run.count as i32);
            if bound_custom {
                if let Some(p) = &self.pipeline {
                    self.ctx
                        .uniform_1_i32(p.sdf_uniforms.has_user_tex.as_ref(), 0);
                }
            }
        }

        self.ctx.disable(glow::SCISSOR_TEST);
        self.ctx.bind_vertex_array(None);
        self.ctx.bind_buffer(glow::ARRAY_BUFFER, None);
    }

    /// Reads the active framebuffer back as top-down RGBA8 pixels.
    #[allow(dead_code)]
    pub fn read_pixels_rgba(&self, width: u32, height: u32) -> Result<Vec<u8>, Error> {
        if width == 0 || height == 0 {
            return Err(Error::InvalidInput(String::from(
                "read_pixels dimensions must be non-zero",
            )));
        }
        let wi = i32::try_from(width)
            .map_err(|_| Error::InvalidInput(String::from("width exceeds i32")))?;
        let hi = i32::try_from(height)
            .map_err(|_| Error::InvalidInput(String::from("height exceeds i32")))?;
        let row_bytes = usize::try_from(width)
            .ok()
            .and_then(|w| w.checked_mul(4))
            .ok_or_else(|| Error::InvalidInput(String::from("row stride overflow")))?;
        let total_bytes = row_bytes
            .checked_mul(height as usize)
            .ok_or_else(|| Error::InvalidInput(String::from("framebuffer size overflow")))?;

        let mut bottom_up = vec![0u8; total_bytes];
        // SAFETY: `self.ctx` is bound and `bottom_up` has `width * height * 4` writable bytes.
        unsafe {
            self.ctx.read_pixels(
                0,
                0,
                wi,
                hi,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut bottom_up)),
            );
        }

        let mut top_down = vec![0u8; total_bytes];
        for row in 0..(height as usize) {
            let src_row = (height as usize) - 1 - row;
            let src_off = src_row * row_bytes;
            let dst_off = row * row_bytes;
            top_down[dst_off..dst_off + row_bytes]
                .copy_from_slice(&bottom_up[src_off..src_off + row_bytes]);
        }
        Ok(top_down)
    }

    /// Creates a dedicated RGBA8 color FBO + texture target for offscreen rendering.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn create_offscreen_target(
        &self,
        width: u32,
        height: u32,
    ) -> Result<OffscreenTarget, Error> {
        if width == 0 || height == 0 {
            return Err(Error::InvalidInput(String::from(
                "offscreen target dimensions must be non-zero",
            )));
        }
        // SAFETY: `self.ctx` is current on this thread.
        let max_tex = unsafe { self.ctx.get_parameter_i32(glow::MAX_TEXTURE_SIZE) };
        if max_tex > 0 && (width > max_tex as u32 || height > max_tex as u32) {
            return Err(Error::InvalidInput(format!(
                "offscreen target {width}x{height} exceeds GL_MAX_TEXTURE_SIZE ({max_tex})"
            )));
        }
        let target = self
            .create_fbo_target(width, height)
            .map_err(|e| Error::Graphics(format!("offscreen FBO allocation failed: {e}")))?;
        // SAFETY: `self.ctx` is current and `target.fbo` was just created by `self.ctx`.
        let status = unsafe {
            self.ctx
                .bind_framebuffer(glow::FRAMEBUFFER, Some(target.fbo));
            let s = self.ctx.check_framebuffer_status(glow::FRAMEBUFFER);
            self.ctx.bind_framebuffer(glow::FRAMEBUFFER, None);
            s
        };
        if status != glow::FRAMEBUFFER_COMPLETE {
            self.destroy_fbo_target(target);
            return Err(Error::Graphics(format!(
                "offscreen FBO incomplete (status 0x{status:x})"
            )));
        }
        Ok(OffscreenTarget {
            fbo: target.fbo,
            tex: target.tex,
            w: target.w,
            h: target.h,
        })
    }

    /// Deletes an offscreen FBO + texture target while the owning GL context is current.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn destroy_offscreen_target(&self, target: OffscreenTarget) {
        self.destroy_fbo_target(FboTarget {
            fbo: target.fbo,
            tex: target.tex,
            w: target.w,
            h: target.h,
        });
    }

    /// Reads an offscreen FBO directly into caller-owned `dst` with zero heap allocations.
    ///
    /// When `bottom_up` is `true`, pixels stay in native OpenGL bottom-up row order (row 0 = bottom).
    /// When `bottom_up` is `false`, rows are flipped in-place into top-down order.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn read_target_pixels_rgba_into(
        &self,
        target: &OffscreenTarget,
        dst: &mut [u8],
        bottom_up: bool,
    ) -> Result<(), Error> {
        let width = target.w;
        let height = target.h;
        if width == 0 || height == 0 {
            return Err(Error::InvalidInput(String::from(
                "read_pixels dimensions must be non-zero",
            )));
        }
        let wi = i32::try_from(width)
            .map_err(|_| Error::InvalidInput(String::from("width exceeds i32")))?;
        let hi = i32::try_from(height)
            .map_err(|_| Error::InvalidInput(String::from("height exceeds i32")))?;
        let row_bytes = usize::try_from(width)
            .ok()
            .and_then(|w| w.checked_mul(4))
            .ok_or_else(|| Error::InvalidInput(String::from("row stride overflow")))?;
        let total_bytes = usize::try_from(height)
            .ok()
            .and_then(|h| row_bytes.checked_mul(h))
            .ok_or_else(|| Error::InvalidInput(String::from("framebuffer size overflow")))?;
        if dst.len() < total_bytes {
            return Err(Error::InvalidInput(format!(
                "destination buffer too small: need {total_bytes} bytes for {width}x{height} RGBA8, got {}",
                dst.len()
            )));
        }

        let buf = &mut dst[..total_bytes];
        // SAFETY: `self.ctx` is current, `target.fbo` belongs to `self.ctx`, `PIXEL_PACK_BUFFER`
        // and all pack store strides/offsets are reset before `read_pixels`, and `buf` has exact
        // `width * height * 4` writable bytes.
        unsafe {
            self.ctx
                .bind_framebuffer(glow::FRAMEBUFFER, Some(target.fbo));
            self.ctx.read_buffer(glow::COLOR_ATTACHMENT0);
            self.ctx.bind_buffer(glow::PIXEL_PACK_BUFFER, None);
            self.ctx.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            self.ctx.pixel_store_i32(glow::PACK_ROW_LENGTH, 0);
            self.ctx.pixel_store_i32(glow::PACK_SKIP_ROWS, 0);
            self.ctx.pixel_store_i32(glow::PACK_SKIP_PIXELS, 0);
            self.ctx.read_pixels(
                0,
                0,
                wi,
                hi,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(buf)),
            );
            self.ctx.bind_framebuffer(glow::FRAMEBUFFER, None);
        }

        if !bottom_up {
            let h = height as usize;
            for row in 0..(h / 2) {
                let top_off = row * row_bytes;
                let bot_off = (h - 1 - row) * row_bytes;
                let (head, tail) = buf.split_at_mut(bot_off);
                head[top_off..top_off + row_bytes].swap_with_slice(&mut tail[..row_bytes]);
            }
        }
        Ok(())
    }

    /// Flushes queued OpenGL commands on the active context.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn flush(&self) {
        // SAFETY: active GL context is current on this thread.
        unsafe {
            self.ctx.flush();
        }
    }

    /// Blocks until all queued OpenGL commands on the active context complete.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn finish(&self) {
        // SAFETY: active GL context is current on this thread.
        unsafe {
            self.ctx.finish();
        }
    }

    /// Deterministically deletes all GL programs, buffers, VAOs, textures, FBOs, and queries.
    ///
    /// Must be called while the owning GL context is still current (before destroying the context).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn destroy_gl_resources(&mut self) {
        // SAFETY: unbinding active GL slots before deleting owned objects is valid while context is current.
        unsafe {
            self.ctx.bind_framebuffer(glow::FRAMEBUFFER, None);
            self.ctx.bind_vertex_array(None);
            self.ctx.bind_buffer(glow::ARRAY_BUFFER, None);
            self.ctx.bind_texture(glow::TEXTURE_2D, None);
            self.ctx.use_program(None);
        }
        if let Some(p) = self.pipeline.take() {
            if let Some(scene) = p.scene_target {
                self.destroy_fbo_target(scene);
            }
            for t in p.down_targets {
                self.destroy_fbo_target(t);
            }
            for t in p.up_targets {
                self.destroy_fbo_target(t);
            }
            // SAFETY: all handles in `p` were created by `self.ctx` and the context is current.
            unsafe {
                self.ctx.delete_vertex_array(p.dummy_vao);
                self.ctx.delete_buffer(p.instance_vbo);
                self.ctx.delete_texture(p.atlas_tex);
                self.ctx.delete_program(p.sdf_program);
                self.ctx.delete_program(p.kawase_down_program);
                self.ctx.delete_program(p.kawase_up_program);
                self.ctx.delete_program(p.blit_program);
                self.ctx.delete_program(p.mesh3d_program);
                self.ctx.delete_vertex_array(p.quad_vao);
                self.ctx.delete_buffer(p.quad_vbo);
                self.ctx.delete_vertex_array(p.custom_vao);
                self.ctx.delete_buffer(p.custom_vbo);
                self.ctx.delete_buffer(p.custom_inst_vbo);
                if let Some(q) = p.timer_query {
                    self.ctx.delete_query(q);
                }
            }
        }
        if let Some(vp3d) = self.viewport3d_target.take() {
            self.destroy_3d_target(vp3d);
        }
        for (_, (tex, _, _)) in self.user_textures.drain() {
            // SAFETY: texture belongs to `self.ctx` and the context is current.
            unsafe {
                self.ctx.delete_texture(tex);
            }
        }
        for (_, entry) in self.custom_programs.drain() {
            // SAFETY: program belongs to `self.ctx` and the context is current.
            unsafe {
                self.ctx.delete_program(entry.program);
            }
        }
        for (_, entry) in self.custom_3d_programs.drain() {
            // SAFETY: program belongs to `self.ctx` and the context is current.
            unsafe {
                self.ctx.delete_program(entry.program);
            }
        }
        for (_, cached) in self.mesh3d_cache.drain() {
            // SAFETY: handles belong to `self.ctx` and the context is current.
            unsafe {
                self.ctx.delete_vertex_array(cached.vao);
                self.ctx.delete_buffer(cached.vbo);
                self.ctx.delete_buffer(cached.ebo);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::compute_kawase_pyramid_sizes;

    #[test]
    fn minimized_0x0_fbo_guard_returns_none() {
        assert_eq!(compute_kawase_pyramid_sizes(0, 0), None);
        assert_eq!(compute_kawase_pyramid_sizes(0, 720), None);
        assert_eq!(compute_kawase_pyramid_sizes(1280, 0), None);
        assert_eq!(
            compute_kawase_pyramid_sizes(1280, 720),
            Some([(640, 360), (320, 180), (160, 90)])
        );
        assert_eq!(
            compute_kawase_pyramid_sizes(3, 3),
            Some([(1, 1), (1, 1), (1, 1)])
        );
    }
}
