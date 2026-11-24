use crate::gfx::text::{GlyphAtlas, ShapedTextOutput};
use crate::gfx::viewport::{Canvas2DCmd, SceneSnapshot, TextureFilter};
use crate::ui::draw::{DrawCmd, DrawList};
use crate::ui::types::{Rect, RectShape, TextureId};

/// Instanced GPU primitive payload (`144` bytes, 9 `vec4` attributes).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SdfInstance {
    /// Physical pixel bounds `[x, y, w, h]` (or unused for `sdSegment` / `sdTri`).
    pub rect: [f32; 4],
    /// Normalized `[u0, v0, u1, v1]` for glyph quads, `[p0.x, p0.y, p1.x, p1.y]` for segments, or `[p2.x, p2.y, 0, 0]` for triangles.
    pub uv_rect: [f32; 4],
    /// Normalized RGBA fill/tint color.
    pub bg_color: [f32; 4],
    /// Normalized RGBA border or shadow color.
    pub border_color: [f32; 4],
    /// `[corner_radius_px, border_width_px, shadow_blur_px, shape_kind]`.
    pub radii_and_border: [f32; 4],
    /// `[cursor_x_px, cursor_y_px, glow_intensity, ripple_progress]`.
    pub glow_and_clip: [f32; 4],
    /// Secondary RGBA gradient stop color.
    pub grad_color: [f32; 4],
    /// `[scale, rot_rad, grad_angle_rad, grad_flags]`.
    pub xform_and_grad: [f32; 4],
    /// Independent physical-pixel corner radii `[tl, tr, br, bl]` for `Box`, `Squircle`, `Bevel`, `Shadow`, `Glass`, and `Image`.
    pub corner_radii: [f32; 4],
}

impl Default for SdfInstance {
    fn default() -> Self {
        Self {
            rect: [0.0; 4],
            uv_rect: [0.0; 4],
            bg_color: [0.0; 4],
            border_color: [0.0; 4],
            radii_and_border: [0.0; 4],
            glow_and_clip: [0.0; 4],
            grad_color: [0.0; 4],
            xform_and_grad: [1.0, 0.0, 0.0, 0.0],
            corner_radii: [0.0; 4],
        }
    }
}

/// Custom 2D shader or instanced draw pass tied to a specific `ClipDrawRun` index.
#[derive(Clone, Debug)]
pub enum Custom2DPassKind {
    Shader {
        key: u64,
        vert_glsl: Option<String>,
        frag_glsl: String,
        uniforms: Vec<f32>,
    },
    Instanced {
        key: u64,
        vertices: Vec<[f32; 4]>,
        instances: Vec<f32>,
        stride_floats: usize,
        vert_glsl: String,
        frag_glsl: String,
        uniforms: Vec<f32>,
    },
}

/// Resolved custom 2D GPU pass bound to a viewport rect and run index.
#[derive(Clone, Debug)]
pub struct Custom2DPass {
    pub rect: Rect,
    pub scissor: Option<[i32; 4]>,
    pub run_index: usize,
    pub kind: Custom2DPassKind,
}

/// Contiguous slice of `SdfInstance` items sharing a single physical scissor rect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipDrawRun {
    pub scissor: Option<[i32; 4]>,
    pub start: usize,
    pub count: usize,
    pub custom_tex: Option<(TextureId, TextureFilter)>,
}

/// Resolved 3D viewport render pass tied to a specific `ClipDrawRun` index.
#[derive(Clone, Debug)]
pub struct Viewport3DPass {
    pub rect: Rect,
    pub scene: SceneSnapshot,
    pub run_index: usize,
}

/// Built GPU batch for a frame: base and glass-split SDF instances + clip segments.
#[derive(Clone, Debug, Default)]
pub struct SdfBatch {
    pub instances: Vec<SdfInstance>,
    pub runs: Vec<ClipDrawRun>,
    pub viewport_3d: Vec<Viewport3DPass>,
    pub custom_2d: Vec<Custom2DPass>,
    pub has_glass: bool,
    pub first_glass_run: Option<usize>,
}

impl SdfBatch {
    /// Clears vectors while retaining heap capacity across frames.
    pub fn clear(&mut self) {
        self.instances.clear();
        self.runs.clear();
        self.viewport_3d.clear();
        self.custom_2d.clear();
        self.has_glass = false;
        self.first_glass_run = None;
    }

    /// Serializes instances into `out` as tightly packed little-endian `f32` bytes without `unsafe`.
    pub fn write_bytes_into(&self, out: &mut Vec<u8>) {
        out.clear();
        let byte_len = self
            .instances
            .len()
            .saturating_mul(core::mem::size_of::<SdfInstance>());
        out.reserve(byte_len);
        for inst in &self.instances {
            for arr in [
                inst.rect,
                inst.uv_rect,
                inst.bg_color,
                inst.border_color,
                inst.radii_and_border,
                inst.glow_and_clip,
                inst.grad_color,
                inst.xform_and_grad,
                inst.corner_radii,
            ] {
                for f in arr {
                    out.extend_from_slice(&f.to_le_bytes());
                }
            }
        }
    }

    /// Compiles a `DrawList` and shapes text via `GlyphAtlas` into physical-pixel SDF instances.
    pub fn build(
        &mut self,
        draw_list: &DrawList,
        atlas: &mut GlyphAtlas,
        scale_factor: f32,
        cursor_logical: Option<[f32; 2]>,
        physical_w: u32,
        physical_h: u32,
    ) {
        self.clear();
        let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        let cursor_px =
            cursor_logical.map_or([-10000.0, -10000.0], |[x, y]| [x * scale, y * scale]);
        let viewport_logical = Rect::new(
            0.0,
            0.0,
            physical_w as f32 / scale,
            physical_h as f32 / scale,
        );
        let cull_enabled = physical_w > 0 && physical_h > 0;

        let mut clip_stack: Vec<Rect> = Vec::new();
        let mut active_scissor: Option<[i32; 4]> = None;

        // Collect all shaped text outputs first so `atlas` can grow before UV normalization.
        // Track clip rects during pre-shaping so off-screen text inside scroll containers
        // skips cosmic-text shaping completely.
        enum PendingText {
            Culled,
            Plain(ShapedTextOutput),
            Rich(ShapedTextOutput),
        }
        let mut pre_clip_stack: Vec<Rect> = Vec::new();
        let mut shaped_texts: Vec<PendingText> = Vec::new();
        for pass in 0..2 {
            pre_clip_stack.clear();
            shaped_texts.clear();
            if pass == 0 {
                atlas.begin_batch();
            }
            for cmd in draw_list.all_commands() {
                match cmd {
                    DrawCmd::PushClip(rect) => {
                        let next = if let Some(prev) = pre_clip_stack.last() {
                            prev.intersect(*rect)
                        } else {
                            *rect
                        };
                        pre_clip_stack.push(next);
                    }
                    DrawCmd::PopClip => {
                        pre_clip_stack.pop();
                    }
                    DrawCmd::Text {
                        rect,
                        text,
                        color,
                        size,
                        weight,
                        mono,
                        font_family,
                        wrap,
                        ellipsis,
                        align,
                        tracking,
                        line_height,
                        transform,
                        ..
                    } => {
                        if cull_enabled && rect.w > 0.0 && rect.h > 0.0 {
                            let visible = active_visible_clip(&pre_clip_stack, viewport_logical);
                            let bounds = conservative_transformed_bounds(*rect, *transform, 4.0);
                            if !rects_overlap(bounds, visible) {
                                shaped_texts.push(PendingText::Culled);
                                continue;
                            }
                        }
                        let shaped = atlas.shape_plain_formatted_to_quads(
                            text,
                            *rect,
                            scale,
                            *color,
                            *size,
                            *weight,
                            *mono,
                            font_family.as_deref(),
                            *wrap,
                            *ellipsis,
                            *align,
                            *tracking,
                            *line_height,
                        );
                        shaped_texts.push(PendingText::Plain(shaped));
                    }
                    DrawCmd::RichText {
                        rect,
                        spans,
                        default_color,
                        code_bg,
                        mention_bg,
                        link_color,
                        size,
                        font_family,
                        wrap,
                        ellipsis,
                        align,
                        tracking,
                        line_height,
                        transform,
                        ..
                    } => {
                        if cull_enabled && rect.w > 0.0 && rect.h > 0.0 {
                            let visible = active_visible_clip(&pre_clip_stack, viewport_logical);
                            let bounds = conservative_transformed_bounds(*rect, *transform, 4.0);
                            if !rects_overlap(bounds, visible) {
                                shaped_texts.push(PendingText::Culled);
                                continue;
                            }
                        }
                        let shaped = atlas.shape_rich_formatted_to_quads(
                            spans,
                            *rect,
                            scale,
                            *default_color,
                            *code_bg,
                            *mention_bg,
                            *link_color,
                            *size,
                            font_family.as_deref(),
                            *wrap,
                            *ellipsis,
                            *align,
                            *tracking,
                            *line_height,
                        );
                        shaped_texts.push(PendingText::Rich(shaped));
                    }
                    _ => {}
                }
            }
            if pass == 0 && atlas.end_batch() {
                // Atlas reached capacity mid-frame; clear stale glyphs and repack only this frame's active glyphs.
                atlas.reset_atlas();
                continue;
            }
            break;
        }

        let mut text_cursor = 0usize;
        let mut cur_run_start = 0usize;

        let flush_run = |runs: &mut Vec<ClipDrawRun>,
                         start: &mut usize,
                         total: usize,
                         scissor: Option<[i32; 4]>| {
            if total > *start {
                runs.push(ClipDrawRun {
                    scissor,
                    start: *start,
                    count: total - *start,
                    custom_tex: None,
                });
                *start = total;
            }
        };

        for cmd in draw_list.all_commands() {
            match cmd {
                DrawCmd::PushClip(rect) => {
                    flush_run(
                        &mut self.runs,
                        &mut cur_run_start,
                        self.instances.len(),
                        active_scissor,
                    );
                    let next = if let Some(prev) = clip_stack.last() {
                        prev.intersect(*rect)
                    } else {
                        *rect
                    };
                    clip_stack.push(next);
                    active_scissor = Some(rect_to_scissor(next, scale, physical_w, physical_h));
                }
                DrawCmd::PopClip => {
                    flush_run(
                        &mut self.runs,
                        &mut cur_run_start,
                        self.instances.len(),
                        active_scissor,
                    );
                    clip_stack.pop();
                    active_scissor = clip_stack
                        .last()
                        .map(|r| rect_to_scissor(*r, scale, physical_w, physical_h));
                }
                DrawCmd::Rect {
                    rect,
                    bg,
                    bg_grad,
                    border_width,
                    border_color,
                    bd_grad,
                    radius,
                    corner_radii,
                    shadow_blur,
                    shadow_color,
                    shape,
                    glass,
                    cursor_glow,
                    ripple,
                    shimmer_phase,
                    transform,
                } => {
                    if cull_enabled {
                        let pad = shadow_blur.max(0.0) * 2.0 + border_width.max(0.0) + 4.0;
                        let visible = active_visible_clip(&clip_stack, viewport_logical);
                        let bounds = conservative_transformed_bounds(*rect, *transform, pad);
                        if !rects_overlap(bounds, visible) {
                            continue;
                        }
                    }
                    let [x_scale, rot_rad, tx, ty] = *transform;
                    let eff_scale = if x_scale.is_finite() && x_scale > 0.0 {
                        x_scale
                    } else {
                        1.0
                    };
                    let px_rect = [
                        ((rect.x + tx) * scale).round(),
                        ((rect.y + ty) * scale).round(),
                        (rect.w * scale).round(),
                        (rect.h * scale).round(),
                    ];
                    let rad_px = (*radius * scale).max(0.0);
                    let corners_px = [
                        (corner_radii[0] * scale).max(0.0),
                        (corner_radii[1] * scale).max(0.0),
                        (corner_radii[2] * scale).max(0.0),
                        (corner_radii[3] * scale).max(0.0),
                    ];
                    let bw_px = (*border_width * scale).max(0.0);
                    let blur_px = (*shadow_blur * scale).max(0.0);
                    let shape_code = match shape {
                        RectShape::Box => 0.0,
                        RectShape::Circle => 1.0,
                        RectShape::Squircle => 2.0,
                        RectShape::Bevel => 10.0,
                        RectShape::Hexagon => 11.0,
                        RectShape::HexagonPointy => 12.0,
                    };

                    if blur_px > 0.0 && shadow_color.a > 0 {
                        self.instances.push(SdfInstance {
                            rect: px_rect,
                            bg_color: shadow_color.to_normalized_f32(),
                            radii_and_border: [rad_px, 0.0, blur_px, 6.0],
                            glow_and_clip: [0.0, 0.0, shape_code, 0.0],
                            xform_and_grad: [eff_scale, rot_rad, 0.0, 0.0],
                            corner_radii: corners_px,
                            ..SdfInstance::default()
                        });
                    }

                    if *glass {
                        // Start a dedicated run at the first glass surface so FBO blur captures prior layers.
                        if !self.has_glass {
                            flush_run(
                                &mut self.runs,
                                &mut cur_run_start,
                                self.instances.len(),
                                active_scissor,
                            );
                            self.has_glass = true;
                            self.first_glass_run = Some(self.runs.len());
                        }
                    }

                    let kind = if *glass { 5.0 } else { shape_code };
                    let glow_intensity = if *cursor_glow { 1.0 } else { 0.0 };
                    let (fx_x, fx_y, fx_param) = if let Some(([rx, ry], prog)) = ripple {
                        (rx * scale, ry * scale, prog.clamp(0.001, 1.0))
                    } else if let Some(phase) = shimmer_phase {
                        (cursor_px[0], cursor_px[1], -(1.0 + phase))
                    } else {
                        (cursor_px[0], cursor_px[1], 0.0)
                    };

                    let (bg_col, bd_col, grad_col, grad_angle, grad_flags) =
                        if let Some(g) = bg_grad {
                            let flag = if g.radial { 2.0 } else { 1.0 };
                            (
                                g.c0.to_normalized_f32(),
                                border_color.to_normalized_f32(),
                                g.c1.to_normalized_f32(),
                                g.angle_rad,
                                flag,
                            )
                        } else if let Some(g) = bd_grad {
                            (
                                bg.to_normalized_f32(),
                                g.c0.to_normalized_f32(),
                                g.c1.to_normalized_f32(),
                                g.angle_rad,
                                4.0,
                            )
                        } else {
                            (
                                bg.to_normalized_f32(),
                                border_color.to_normalized_f32(),
                                [0.0; 4],
                                0.0,
                                0.0,
                            )
                        };

                    self.instances.push(SdfInstance {
                        rect: px_rect,
                        uv_rect: [0.0, 0.0, 1.0, 1.0],
                        bg_color: bg_col,
                        border_color: bd_col,
                        radii_and_border: [rad_px, bw_px, 0.0, kind],
                        glow_and_clip: [fx_x, fx_y, glow_intensity, fx_param],
                        grad_color: grad_col,
                        xform_and_grad: [eff_scale, rot_rad, grad_angle, grad_flags],
                        corner_radii: corners_px,
                    });
                }
                DrawCmd::Sparkline {
                    rect,
                    samples,
                    color,
                    fill_color,
                    stroke_width,
                } => {
                    if cull_enabled {
                        let pad = stroke_width.max(0.0) + 2.0;
                        let visible = active_visible_clip(&clip_stack, viewport_logical);
                        let bounds = Rect::new(
                            rect.x - pad,
                            rect.y - pad,
                            rect.w + pad * 2.0,
                            rect.h + pad * 2.0,
                        );
                        if !rects_overlap(bounds, visible) {
                            continue;
                        }
                    }
                    if samples.len() >= 2 && rect.w > 0.0 && rect.h > 0.0 {
                        let mut min_v = f32::INFINITY;
                        let mut max_v = f32::NEG_INFINITY;
                        for &s in samples {
                            if s.is_finite() {
                                min_v = min_v.min(s);
                                max_v = max_v.max(s);
                            }
                        }
                        if min_v.is_finite() && max_v.is_finite() {
                            let span = (max_v - min_v).max(1e-5);
                            let n = (samples.len() - 1) as f32;
                            if let Some(fc) = fill_color {
                                let fill_rgba = fc.to_normalized_f32();
                                let x0_px = rect.x * scale;
                                let w_px = (rect.w * scale).ceil().max(1.0);
                                let bot_px = (rect.y + rect.h) * scale;
                                let step_px = (w_px / 256.0).max(1.0);
                                let mut px = 0.0_f32;
                                while px < w_px {
                                    let u = (px / w_px).clamp(0.0, 1.0);
                                    let pos = u * n;
                                    let idx = (pos.floor() as usize).min(samples.len() - 2);
                                    let frac = pos - idx as f32;
                                    let v = samples[idx] + (samples[idx + 1] - samples[idx]) * frac;
                                    let t = ((v - min_v) / span).clamp(0.0, 1.0);
                                    let top_px = (rect.y + (1.0 - t) * rect.h) * scale;
                                    let slice_w = step_px.min(w_px - px);
                                    let slice_h = (bot_px - top_px).max(0.0);
                                    if slice_h > 0.25 {
                                        self.instances.push(SdfInstance {
                                            rect: [x0_px + px, top_px, slice_w, slice_h],
                                            uv_rect: [0.0, 0.0, 1.0, 1.0],
                                            bg_color: fill_rgba,
                                            ..SdfInstance::default()
                                        });
                                    }
                                    px += step_px;
                                }
                            }
                            let half_thick = (*stroke_width * scale * 0.5).max(0.75);
                            let col = color.to_normalized_f32();
                            for i in 0..(samples.len() - 1) {
                                let t0 = ((samples[i] - min_v) / span).clamp(0.0, 1.0);
                                let t1 = ((samples[i + 1] - min_v) / span).clamp(0.0, 1.0);
                                let x0 = (rect.x + (i as f32 / n) * rect.w) * scale;
                                let y0 = (rect.y + (1.0 - t0) * rect.h) * scale;
                                let x1 = (rect.x + ((i + 1) as f32 / n) * rect.w) * scale;
                                let y1 = (rect.y + (1.0 - t1) * rect.h) * scale;
                                let bx = x0.min(x1) - half_thick;
                                let by = y0.min(y1) - half_thick;
                                let bw = (x0.max(x1) - x0.min(x1)) + half_thick * 2.0;
                                let bh = (y0.max(y1) - y0.min(y1)) + half_thick * 2.0;
                                self.instances.push(SdfInstance {
                                    rect: [bx, by, bw, bh],
                                    uv_rect: [x0, y0, x1, y1],
                                    bg_color: col,
                                    radii_and_border: [half_thick, 0.0, 0.0, 4.0],
                                    ..SdfInstance::default()
                                });
                            }
                        }
                    }
                }
                DrawCmd::Text {
                    rect, transform, ..
                }
                | DrawCmd::RichText {
                    rect, transform, ..
                } => {
                    let shaped = match &shaped_texts[text_cursor] {
                        PendingText::Culled => {
                            text_cursor += 1;
                            continue;
                        }
                        PendingText::Plain(s) | PendingText::Rich(s) => s,
                    };
                    text_cursor += 1;

                    let [x_scale, rot_rad, tx, ty] = *transform;
                    let eff_scale = if x_scale.is_finite() && x_scale > 0.0 {
                        x_scale
                    } else {
                        1.0
                    };
                    let has_xform = (eff_scale - 1.0).abs() > 0.001
                        || rot_rad.abs() > 0.0001
                        || tx.abs() > 0.001
                        || ty.abs() > 0.001;
                    let pivot_px = [
                        (rect.x + rect.w * 0.5) * scale,
                        (rect.y + rect.h * 0.5) * scale,
                    ];
                    let tx_px = tx * scale;
                    let ty_px = ty * scale;
                    let xform_rect = |r: [f32; 4]| -> [f32; 4] {
                        if !has_xform {
                            return r;
                        }
                        let cx = r[0] + r[2] * 0.5 - pivot_px[0];
                        let cy = r[1] + r[3] * 0.5 - pivot_px[1];
                        let cs = rot_rad.cos();
                        let sn = rot_rad.sin();
                        let rx = (cx * cs - cy * sn) * eff_scale + pivot_px[0] + tx_px;
                        let ry = (cx * sn + cy * cs) * eff_scale + pivot_px[1] + ty_px;
                        [rx - r[2] * 0.5, ry - r[3] * 0.5, r[2], r[3]]
                    };

                    for bg in &shaped.bg_rects {
                        self.instances.push(SdfInstance {
                            rect: xform_rect(bg.phys_rect),
                            bg_color: bg.color.to_normalized_f32(),
                            radii_and_border: [bg.radius, 0.0, 0.0, 0.0],
                            xform_and_grad: [eff_scale, rot_rad, 0.0, 0.0],
                            ..SdfInstance::default()
                        });
                    }
                    for g in &shaped.glyphs {
                        let uv = atlas.normalized_uv(g.atlas_px);
                        self.instances.push(SdfInstance {
                            rect: xform_rect(g.phys_rect),
                            uv_rect: uv,
                            bg_color: g.color.to_normalized_f32(),
                            radii_and_border: [0.0, 0.0, 0.0, 3.0],
                            xform_and_grad: [eff_scale, rot_rad, 0.0, 0.0],
                            ..SdfInstance::default()
                        });
                    }
                    for fg in &shaped.fg_rects {
                        self.instances.push(SdfInstance {
                            rect: xform_rect(fg.phys_rect),
                            bg_color: fg.color.to_normalized_f32(),
                            radii_and_border: [fg.radius, 0.0, 0.0, 0.0],
                            xform_and_grad: [eff_scale, rot_rad, 0.0, 0.0],
                            ..SdfInstance::default()
                        });
                    }
                }
                DrawCmd::Image {
                    rect,
                    tex,
                    filter,
                    uv,
                    tint,
                    border_width,
                    border_color,
                    radius,
                    corner_radii,
                    shape,
                    transform,
                } => {
                    if cull_enabled {
                        let pad = border_width.max(0.0) + 1.0;
                        let visible = active_visible_clip(&clip_stack, viewport_logical);
                        let bounds = conservative_transformed_bounds(*rect, *transform, pad);
                        if !rects_overlap(bounds, visible) {
                            continue;
                        }
                    }
                    flush_run(
                        &mut self.runs,
                        &mut cur_run_start,
                        self.instances.len(),
                        active_scissor,
                    );
                    let [x_scale, rot_rad, tx, ty] = *transform;
                    let eff_scale = if x_scale.is_finite() && x_scale > 0.0 {
                        x_scale
                    } else {
                        1.0
                    };
                    let shape_code = match shape {
                        RectShape::Box => 0.0,
                        RectShape::Circle => 1.0,
                        RectShape::Squircle => 2.0,
                        RectShape::Bevel => 10.0,
                        RectShape::Hexagon => 11.0,
                        RectShape::HexagonPointy => 12.0,
                    };
                    self.instances.push(SdfInstance {
                        rect: [
                            ((rect.x + tx) * scale).round(),
                            ((rect.y + ty) * scale).round(),
                            (rect.w * scale).round(),
                            (rect.h * scale).round(),
                        ],
                        uv_rect: *uv,
                        bg_color: tint.to_normalized_f32(),
                        border_color: border_color.to_normalized_f32(),
                        radii_and_border: [
                            (*radius * scale).max(0.0),
                            (*border_width * scale).max(0.0),
                            0.0,
                            8.0,
                        ],
                        glow_and_clip: [0.0, 0.0, shape_code, 0.0],
                        xform_and_grad: [eff_scale, rot_rad, 0.0, 0.0],
                        corner_radii: [
                            (corner_radii[0] * scale).max(0.0),
                            (corner_radii[1] * scale).max(0.0),
                            (corner_radii[2] * scale).max(0.0),
                            (corner_radii[3] * scale).max(0.0),
                        ],
                        ..SdfInstance::default()
                    });
                    self.runs.push(ClipDrawRun {
                        scissor: active_scissor,
                        start: cur_run_start,
                        count: 1,
                        custom_tex: Some((*tex, *filter)),
                    });
                    cur_run_start = self.instances.len();
                }
                DrawCmd::Viewport {
                    rect,
                    canvas,
                    scene,
                } => {
                    let vp_clip = if let Some(prev) = clip_stack.last() {
                        prev.intersect(*rect)
                    } else {
                        *rect
                    };
                    if cull_enabled && !rects_overlap(vp_clip, viewport_logical) {
                        continue;
                    }
                    flush_run(
                        &mut self.runs,
                        &mut cur_run_start,
                        self.instances.len(),
                        active_scissor,
                    );
                    let vp_scissor = Some(rect_to_scissor(vp_clip, scale, physical_w, physical_h));
                    let vp_run_index = self.runs.len();
                    let px_vp = Rect::new(
                        (rect.x * scale).round(),
                        (rect.y * scale).round(),
                        (rect.w * scale).round(),
                        (rect.h * scale).round(),
                    );

                    if !scene.draws.is_empty() && rect.w > 0.0 && rect.h > 0.0 {
                        self.viewport_3d.push(Viewport3DPass {
                            rect: px_vp,
                            scene: scene.clone(),
                            run_index: vp_run_index,
                        });
                    }

                    for ccmd in canvas {
                        match ccmd {
                            Canvas2DCmd::Rect {
                                rect: r,
                                color,
                                radius,
                            } => {
                                self.instances.push(SdfInstance {
                                    rect: [
                                        ((rect.x + r.x) * scale).round(),
                                        ((rect.y + r.y) * scale).round(),
                                        (r.w * scale).round(),
                                        (r.h * scale).round(),
                                    ],
                                    uv_rect: [0.0, 0.0, 1.0, 1.0],
                                    bg_color: color.to_normalized_f32(),
                                    radii_and_border: [(*radius * scale).max(0.0), 0.0, 0.0, 0.0],
                                    ..SdfInstance::default()
                                });
                            }
                            Canvas2DCmd::Segment {
                                p0,
                                p1,
                                thickness,
                                color,
                            } => {
                                let half_thick = (*thickness * scale * 0.5).max(0.75);
                                let x0 = (rect.x + p0[0]) * scale;
                                let y0 = (rect.y + p0[1]) * scale;
                                let x1 = (rect.x + p1[0]) * scale;
                                let y1 = (rect.y + p1[1]) * scale;
                                let bx = x0.min(x1) - half_thick;
                                let by = y0.min(y1) - half_thick;
                                let bw = (x0.max(x1) - x0.min(x1)) + half_thick * 2.0;
                                let bh = (y0.max(y1) - y0.min(y1)) + half_thick * 2.0;
                                self.instances.push(SdfInstance {
                                    rect: [bx, by, bw, bh],
                                    uv_rect: [x0, y0, x1, y1],
                                    bg_color: color.to_normalized_f32(),
                                    radii_and_border: [half_thick, 0.0, 0.0, 4.0],
                                    ..SdfInstance::default()
                                });
                            }
                            Canvas2DCmd::Tri { p0, p1, p2, color } => {
                                let x0 = (rect.x + p0[0]) * scale;
                                let y0 = (rect.y + p0[1]) * scale;
                                let x1 = (rect.x + p1[0]) * scale;
                                let y1 = (rect.y + p1[1]) * scale;
                                let x2 = (rect.x + p2[0]) * scale;
                                let y2 = (rect.y + p2[1]) * scale;
                                self.instances.push(SdfInstance {
                                    rect: [x0, y0, x1, y1],
                                    uv_rect: [x2, y2, 0.0, 0.0],
                                    bg_color: color.to_normalized_f32(),
                                    radii_and_border: [0.0, 0.0, 0.0, 9.0],
                                    ..SdfInstance::default()
                                });
                            }
                            Canvas2DCmd::CustomShader {
                                key,
                                vert_glsl,
                                frag_glsl,
                                uniforms,
                                rect: _,
                            } => {
                                flush_run(
                                    &mut self.runs,
                                    &mut cur_run_start,
                                    self.instances.len(),
                                    vp_scissor,
                                );
                                self.custom_2d.push(Custom2DPass {
                                    rect: px_vp,
                                    scissor: vp_scissor,
                                    run_index: self.runs.len(),
                                    kind: Custom2DPassKind::Shader {
                                        key: *key,
                                        vert_glsl: vert_glsl.clone(),
                                        frag_glsl: frag_glsl.clone(),
                                        uniforms: uniforms.clone(),
                                    },
                                });
                            }
                            Canvas2DCmd::DrawInstanced {
                                key,
                                vertices,
                                instances,
                                stride_floats,
                                vert_glsl,
                                frag_glsl,
                                uniforms,
                                rect: _,
                            } => {
                                flush_run(
                                    &mut self.runs,
                                    &mut cur_run_start,
                                    self.instances.len(),
                                    vp_scissor,
                                );
                                self.custom_2d.push(Custom2DPass {
                                    rect: px_vp,
                                    scissor: vp_scissor,
                                    run_index: self.runs.len(),
                                    kind: Custom2DPassKind::Instanced {
                                        key: *key,
                                        vertices: vertices.clone(),
                                        instances: instances.clone(),
                                        stride_floats: *stride_floats,
                                        vert_glsl: vert_glsl.clone(),
                                        frag_glsl: frag_glsl.clone(),
                                        uniforms: uniforms.clone(),
                                    },
                                });
                            }
                            Canvas2DCmd::Tex {
                                tex,
                                rect: r,
                                filter,
                            } => {
                                flush_run(
                                    &mut self.runs,
                                    &mut cur_run_start,
                                    self.instances.len(),
                                    vp_scissor,
                                );
                                self.instances.push(SdfInstance {
                                    rect: [
                                        ((rect.x + r.x) * scale).round(),
                                        ((rect.y + r.y) * scale).round(),
                                        (r.w * scale).round(),
                                        (r.h * scale).round(),
                                    ],
                                    uv_rect: [0.0, 0.0, 1.0, 1.0],
                                    bg_color: [0.22, 0.25, 0.32, 1.0],
                                    border_color: [0.45, 0.50, 0.60, 0.8],
                                    radii_and_border: [4.0 * scale, 1.0 * scale, 0.0, 7.0],
                                    ..SdfInstance::default()
                                });
                                self.runs.push(ClipDrawRun {
                                    scissor: vp_scissor,
                                    start: cur_run_start,
                                    count: 1,
                                    custom_tex: Some((*tex, *filter)),
                                });
                                cur_run_start = self.instances.len();
                            }
                        }
                    }

                    // Always push a dedicated run slot so `vp_run_index` aligns even when `canvas` is empty.
                    self.runs.push(ClipDrawRun {
                        scissor: vp_scissor,
                        start: cur_run_start,
                        count: self.instances.len() - cur_run_start,
                        custom_tex: None,
                    });
                    cur_run_start = self.instances.len();
                }
            }
        }

        flush_run(
            &mut self.runs,
            &mut cur_run_start,
            self.instances.len(),
            active_scissor,
        );
    }
}

fn active_visible_clip(clip_stack: &[Rect], viewport: Rect) -> Rect {
    clip_stack
        .last()
        .copied()
        .map_or(viewport, |c| c.intersect(viewport))
}

fn rects_overlap(a: Rect, b: Rect) -> bool {
    a.w > 0.0
        && a.h > 0.0
        && b.w > 0.0
        && b.h > 0.0
        && a.x < b.x + b.w
        && a.x + a.w > b.x
        && a.y < b.y + b.h
        && a.y + a.h > b.y
}

fn conservative_transformed_bounds(rect: Rect, transform: [f32; 4], pad: f32) -> Rect {
    let [x_scale, rot_rad, tx, ty] = transform;
    let eff_scale = if x_scale.is_finite() && x_scale > 0.0 {
        x_scale
    } else {
        1.0
    };
    let tx = if tx.is_finite() { tx } else { 0.0 };
    let ty = if ty.is_finite() { ty } else { 0.0 };
    if (eff_scale - 1.0).abs() <= 0.001 && (!rot_rad.is_finite() || rot_rad.abs() <= 0.0001) {
        Rect::new(
            rect.x + tx - pad,
            rect.y + ty - pad,
            rect.w + pad * 2.0,
            rect.h + pad * 2.0,
        )
    } else {
        let cx = rect.x + tx + rect.w * 0.5;
        let cy = rect.y + ty + rect.h * 0.5;
        let half_diag = rect.w.hypot(rect.h) * 0.5 * eff_scale + pad;
        Rect::new(
            cx - half_diag,
            cy - half_diag,
            half_diag * 2.0,
            half_diag * 2.0,
        )
    }
}

fn rect_to_scissor(rect: Rect, scale: f32, physical_w: u32, physical_h: u32) -> [i32; 4] {
    let x0 = (rect.x * scale).round().max(0.0) as i32;
    let y0_top = (rect.y * scale).round().max(0.0) as i32;
    let w = (rect.w * scale).round().max(0.0) as i32;
    let h = (rect.h * scale).round().max(0.0) as i32;
    let ph = i32::try_from(physical_h).unwrap_or(i32::MAX);
    let pw = i32::try_from(physical_w).unwrap_or(i32::MAX);
    let x = x0.min(pw);
    let y_gl = (ph.saturating_sub(y0_top.saturating_add(h))).max(0);
    [x, y_gl, w.min(pw.saturating_sub(x)), h.min(ph)]
}

/// Unified GLSL vertex shader body (compatible with `#version 330 core` and `#version 300 es`).
pub const SDF_VERT_BODY: &str = r#"
layout(location = 0) in vec4 a_rect;
layout(location = 1) in vec4 a_uv;
layout(location = 2) in vec4 a_bg;
layout(location = 3) in vec4 a_bd;
layout(location = 4) in vec4 a_params;
layout(location = 5) in vec4 a_glow;
layout(location = 6) in vec4 a_grad_color;
layout(location = 7) in vec4 a_xform;
layout(location = 8) in vec4 a_corners;

uniform vec2 u_viewport;

out vec2 v_pos;
out vec2 v_uv;
out vec4 v_rect;
out vec4 v_uv_raw;
out vec4 v_bg;
out vec4 v_bd;
out vec4 v_params;
out vec4 v_glow;
out vec4 v_grad_color;
out vec4 v_xform;
out vec4 v_corners;

void main() {
    int vid = gl_VertexID;
    float vx = (vid == 1 || vid == 2 || vid == 4) ? 1.0 : 0.0;
    float vy = (vid == 2 || vid == 4 || vid == 5) ? 1.0 : 0.0;
    vec2 corner = vec2(vx, vy);

    int kind = int(a_params.w + 0.5);
    float pad = (kind == 3 || kind == 7) ? 0.0 : ((kind == 6) ? (a_params.z * 1.5 + 4.0) : (a_params.z + 2.0));

    vec2 base_xy = a_rect.xy;
    vec2 base_wh = a_rect.zw;
    if (kind == 4) {
        vec2 mn = min(a_uv.xy, a_uv.zw) - vec2(a_params.x);
        vec2 mx = max(a_uv.xy, a_uv.zw) + vec2(a_params.x);
        base_xy = mn;
        base_wh = mx - mn;
    } else if (kind == 9) {
        vec2 mn = min(min(a_rect.xy, a_rect.zw), a_uv.xy) - vec2(2.0);
        vec2 mx = max(max(a_rect.xy, a_rect.zw), a_uv.xy) + vec2(2.0);
        base_xy = mn;
        base_wh = mx - mn;
    }

    vec2 exp_xy = base_xy - vec2(pad);
    vec2 exp_wh = base_wh + vec2(pad * 2.0);
    float sc = max(a_xform.x, 0.001);
    float rot = a_xform.y;
    if (kind != 3 && kind != 4 && kind != 9 && (abs(sc - 1.0) > 0.001 || abs(rot) > 0.0001)) {
        vec2 center = base_xy + base_wh * 0.5;
        float rad = length(base_wh * 0.5) * sc + pad;
        exp_xy = center - vec2(rad);
        exp_wh = vec2(rad * 2.0);
    }
    vec2 px = exp_xy + corner * exp_wh;
    if (kind == 3 && (abs(sc - 1.0) > 0.001 || abs(rot) > 0.0001)) {
        vec2 center = base_xy + base_wh * 0.5;
        vec2 local = (corner - vec2(0.5)) * base_wh * sc;
        float cs = cos(rot);
        float sn = sin(rot);
        px = center + vec2(local.x * cs - local.y * sn, local.x * sn + local.y * cs);
    }

    vec2 ndc = vec2(
        (px.x / max(u_viewport.x, 1.0)) * 2.0 - 1.0,
        1.0 - (px.y / max(u_viewport.y, 1.0)) * 2.0
    );
    gl_Position = vec4(ndc, 0.0, 1.0);

    v_pos = px;
    vec2 uv_corner = (kind == 8) ? clamp((px - base_xy) / max(base_wh, vec2(1.0)), 0.0, 1.0) : corner;
    v_uv = mix(a_uv.xy, a_uv.zw, uv_corner);
    v_rect = (kind == 9) ? a_rect : vec4(base_xy, base_wh);
    v_uv_raw = a_uv;
    v_bg = a_bg;
    v_bd = a_bd;
    v_params = a_params;
    v_glow = a_glow;
    v_grad_color = a_grad_color;
    v_xform = a_xform;
    v_corners = a_corners;
}
"#;

/// Unified GLSL fragment shader body (compatible with `#version 330 core` and `#version 300 es`).
pub const SDF_FRAG_BODY: &str = r#"
uniform sampler2D u_atlas;
uniform sampler2D u_backdrop;
uniform sampler2D u_user_tex;
uniform int u_has_user_tex;
uniform vec2 u_viewport;

in vec2 v_pos;
in vec2 v_uv;
in vec4 v_rect;
in vec4 v_uv_raw;
in vec4 v_bg;
in vec4 v_bd;
in vec4 v_params;
in vec4 v_glow;
in vec4 v_grad_color;
in vec4 v_xform;
in vec4 v_corners;

out vec4 frag_color;

float sd_rounded_box4(vec2 p, vec2 half_sz, vec4 r4) {
    r4.xy = (p.x > 0.0) ? r4.yz : r4.xw;
    float r = (p.y > 0.0) ? r4.y : r4.x;
    vec2 q = abs(p) - half_sz + vec2(r);
    return min(max(q.x, q.y), 0.0) + length(max(q, 0.0)) - r;
}

float sd_bevel_box4(vec2 p, vec2 half_sz, vec4 b4, float fillet) {
    b4.xy = (p.x > 0.0) ? b4.yz : b4.xw;
    float b = (p.y > 0.0) ? b4.y : b4.x;
    float min_h = min(half_sz.x, half_sz.y);
    float f = clamp(fillet, 0.0, min_h);
    vec2 pa = abs(p);
    vec2 hs = max(half_sz - vec2(f), vec2(0.0));
    float b_eff = clamp(b - f * 0.58578644, 0.0, min(hs.x, hs.y));
    vec2 d_v = pa - hs;
    float d_box = length(max(d_v, 0.0)) + min(max(d_v.x, d_v.y), 0.0);
    float d_chamfer = (pa.x + pa.y - (hs.x + hs.y - b_eff)) * 0.70710678;
    return max(d_box, d_chamfer) - f;
}

// Regular flat-top hexagon centered at origin with inradius (apothem) `r`.
float sd_hexagon(vec2 p, float r) {
    const vec3 k = vec3(-0.866025404, 0.5, 0.577350269);
    p = abs(p);
    p -= 2.0 * min(dot(k.xy, p), 0.0) * k.xy;
    p -= vec2(clamp(p.x, -k.z * r, k.z * r), r);
    return length(p) * sign(p.y);
}

float erf_approx(float x) {
    float s = sign(x);
    float a = abs(x);
    float t = 1.0 + (0.278393 + (0.230389 + 0.078108 * (a * a)) * a) * a;
    float t2 = t * t;
    return s - s / (t2 * t2);
}

float sd_segment(vec2 p, vec2 a, vec2 b) {
    vec2 pa = p - a;
    vec2 ba = b - a;
    float h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    return length(pa - ba * h);
}

float sd_triangle(vec2 p, vec2 p0, vec2 p1, vec2 p2) {
    vec2 e0 = p1 - p0, e1 = p2 - p1, e2 = p0 - p2;
    vec2 v0 = p - p0, v1 = p - p1, v2 = p - p2;
    vec2 pq0 = v0 - e0 * clamp(dot(v0, e0) / max(dot(e0, e0), 1e-6), 0.0, 1.0);
    vec2 pq1 = v1 - e1 * clamp(dot(v1, e1) / max(dot(e1, e1), 1e-6), 0.0, 1.0);
    vec2 pq2 = v2 - e2 * clamp(dot(v2, e2) / max(dot(e2, e2), 1e-6), 0.0, 1.0);
    float s = sign(e0.x * e2.y - e0.y * e2.x);
    vec2 d = min(min(
        vec2(dot(pq0, pq0), s * (v0.x * e0.y - v0.y * e0.x)),
        vec2(dot(pq1, pq1), s * (v1.x * e1.y - v1.y * e1.x))),
        vec2(dot(pq2, pq2), s * (v2.x * e2.y - v2.y * e2.x))
    );
    return -sqrt(d.x) * sign(d.y);
}

float grad_factor(vec2 p, vec2 half_sz, float angle_rad, int mode) {
    if (mode == 2) {
        return clamp(length(p / max(half_sz, vec2(1.0))), 0.0, 1.0);
    }
    vec2 dir = vec2(cos(angle_rad), sin(angle_rad));
    vec2 norm_p = p / max(half_sz * 2.0, vec2(1.0));
    return clamp(dot(norm_p, dir) + 0.5, 0.0, 1.0);
}

void main() {
    int kind = int(v_params.w + 0.5);

    if (kind == 3) {
        vec4 texel = texture(u_atlas, v_uv);
        frag_color = texel * v_bg;
        if (frag_color.a <= 0.001) discard;
        return;
    }

    if (kind == 7 && u_has_user_tex != 0) {
        vec4 texel = texture(u_user_tex, v_uv);
        if (texel.a <= 0.001) discard;
        frag_color = texel;
        return;
    }

    if (kind == 4) {
        float d = sd_segment(v_pos, v_uv_raw.xy, v_uv_raw.zw) - v_params.x;
        float a = 1.0 - smoothstep(-0.75, 0.75, d);
        frag_color = vec4(v_bg.rgb, v_bg.a * a);
        if (frag_color.a <= 0.001) discard;
        return;
    }

    if (kind == 9) {
        float d = sd_triangle(v_pos, v_rect.xy, v_rect.zw, v_uv_raw.xy);
        float a = 1.0 - smoothstep(-0.75, 0.75, d);
        frag_color = vec4(v_bg.rgb, v_bg.a * a);
        if (frag_color.a <= 0.001) discard;
        return;
    }

    vec2 half_sz = v_rect.zw * 0.5;
    vec2 center = v_rect.xy + half_sz;
    float sc = max(v_xform.x, 0.001);
    float rot = v_xform.y;
    vec2 dp = (v_pos - center) / sc;
    float cs = cos(rot);
    float sn = sin(rot);
    vec2 p = vec2(dp.x * cs + dp.y * sn, -dp.x * sn + dp.y * cs);

    float min_half = min(half_sz.x, half_sz.y);
    vec4 r4 = (dot(v_corners, vec4(1.0)) > 0.0) ? v_corners : vec4(v_params.x);
    r4 = clamp(r4, vec4(0.0), vec4(min_half));
    int sub_shape = (kind == 8 || kind == 6) ? int(v_glow.z + 0.5) : kind;
    if (sub_shape == 1) {
        r4 = vec4(min_half);
    } else if (sub_shape == 2) {
        r4 = vec4(min_half * 0.44);
    }

    float d;
    if (sub_shape == 10) {
        float fillet = (dot(v_corners, vec4(1.0)) > 0.0) ? clamp(v_params.x, 0.0, min_half) : 0.0;
        d = sd_bevel_box4(p, half_sz, r4, fillet) * sc;
    } else if (sub_shape == 11) {
        float hex_r = min(half_sz.y, half_sz.x * 0.866025404);
        float vr = clamp(v_params.x, 0.0, max(hex_r - 0.5, 0.0));
        d = (sd_hexagon(p, max(hex_r - vr, 0.0)) - vr) * sc;
    } else if (sub_shape == 12) {
        float hex_r = min(half_sz.x, half_sz.y * 0.866025404);
        float vr = clamp(v_params.x, 0.0, max(hex_r - 0.5, 0.0));
        d = (sd_hexagon(p.yx, max(hex_r - vr, 0.0)) - vr) * sc;
    } else {
        d = sd_rounded_box4(p, half_sz, r4) * sc;
    }

    if (kind == 6) {
        float blur = max(v_params.z, 1.0);
        float gauss = 0.5 * (1.0 - erf_approx(d / (blur * 0.5)));
        float cutoff = 1.0 - smoothstep(blur * 1.25, blur * 1.5, d);
        float sa = gauss * cutoff;
        frag_color = vec4(v_bg.rgb, v_bg.a * sa);
        if (frag_color.a <= 0.001) discard;
        return;
    }

    float fill_mask = 1.0 - smoothstep(-0.75, 0.75, d);
    int gflag = int(v_xform.w + 0.5);
    vec4 fill = v_bg;
    if (gflag == 1 || gflag == 2) {
        fill = mix(v_bg, v_grad_color, grad_factor(p, half_sz, v_xform.z, gflag));
    }

    if (kind == 5) {
        vec2 screen_uv = vec2(v_pos.x / max(u_viewport.x, 1.0), 1.0 - v_pos.y / max(u_viewport.y, 1.0));
        vec4 blurred = texture(u_backdrop, clamp(screen_uv, 0.0, 1.0));
        float tint_alpha = clamp(fill.a, 0.15, 0.85);
        fill = vec4(mix(blurred.rgb, fill.rgb, tint_alpha), mix(blurred.a, 1.0, tint_alpha * 0.65));
    } else if (kind == 8) {
        if (u_has_user_tex != 0) {
            fill = texture(u_user_tex, v_uv) * v_bg;
        } else {
            fill = vec4(0.22, 0.25, 0.32, v_bg.a);
        }
    }

    vec4 out_col = vec4(fill.rgb, fill.a * fill_mask);

    if (kind != 8 && v_glow.w > 0.0 && v_glow.w < 1.0) {
        float max_dim = max(v_rect.z, v_rect.w);
        float rip_r = v_glow.w * max_dim;
        float dist = length(v_pos - v_glow.xy);
        float rip_mask = (1.0 - smoothstep(rip_r - 8.0, rip_r, dist)) * (1.0 - v_glow.w) * 0.35;
        out_col.rgb = mix(out_col.rgb, vec3(1.0), rip_mask * fill_mask);
    } else if (kind != 8 && v_glow.w < 0.0) {
        float phase = fract(-(v_glow.w + 1.0));
        float u = (v_pos.x - v_rect.x) / max(v_rect.z, 1.0) + (v_pos.y - v_rect.y) / max(v_rect.w, 1.0) * 0.25;
        float band = exp(-pow((u - (phase * 1.6 - 0.3)) * 5.0, 2.0)) * 0.25;
        out_col.rgb = mix(out_col.rgb, vec3(1.0), band * fill_mask);
    }

    float bw = v_params.y;
    if (bw > 0.0) {
        float outer_a = 1.0 - smoothstep(-0.75, 0.75, d);
        float inner_a = smoothstep(-bw - 0.75, -bw + 0.75, d);
        float border_mask = clamp(outer_a * inner_a, 0.0, 1.0);

        vec4 bd = v_bd;
        if (gflag == 4) {
            bd = mix(v_bd, v_grad_color, grad_factor(p, half_sz, v_xform.z, 1));
        }
        if (kind != 8 && v_glow.z > 0.0) {
            float dist = length(v_pos - v_glow.xy);
            float g = exp(-dist * 0.015) * v_glow.z;
            bd.rgb = mix(bd.rgb, vec3(1.0), clamp(g * 0.65, 0.0, 0.75));
        }
        out_col = mix(out_col, vec4(bd.rgb, bd.a), border_mask * bd.a);
    }

    if (out_col.a <= 0.001) discard;
    frag_color = out_col;
}
"#;

/// Fullscreen quad vertex shader body for Dual-Kawase blur and FBO blit passes.
pub const FULLSCREEN_VERT_BODY: &str = r#"
out vec2 v_uv;
void main() {
    int vid = gl_VertexID;
    vec2 pos = vec2((vid == 1) ? 3.0 : -1.0, (vid == 2) ? 3.0 : -1.0);
    v_uv = pos * 0.5 + 0.5;
    gl_Position = vec4(pos, 0.0, 1.0);
}
"#;

/// Dual-Kawase downsample fragment shader body.
pub const KAWASE_DOWN_FRAG_BODY: &str = r#"
uniform sampler2D u_tex;
uniform vec2 u_half_pixel;
in vec2 v_uv;
out vec4 frag_color;

void main() {
    vec4 sum = texture(u_tex, v_uv) * 4.0;
    sum += texture(u_tex, v_uv - u_half_pixel);
    sum += texture(u_tex, v_uv + u_half_pixel);
    sum += texture(u_tex, v_uv + vec2(u_half_pixel.x, -u_half_pixel.y));
    sum += texture(u_tex, v_uv - vec2(u_half_pixel.x, -u_half_pixel.y));
    frag_color = sum / 8.0;
}
"#;

/// Dual-Kawase upsample fragment shader body.
pub const KAWASE_UP_FRAG_BODY: &str = r#"
uniform sampler2D u_tex;
uniform vec2 u_half_pixel;
in vec2 v_uv;
out vec4 frag_color;

void main() {
    vec4 sum = texture(u_tex, v_uv + vec2(-u_half_pixel.x * 2.0, 0.0));
    sum += texture(u_tex, v_uv + vec2(-u_half_pixel.x, u_half_pixel.y)) * 2.0;
    sum += texture(u_tex, v_uv + vec2(0.0, u_half_pixel.y * 2.0));
    sum += texture(u_tex, v_uv + vec2(u_half_pixel.x, u_half_pixel.y)) * 2.0;
    sum += texture(u_tex, v_uv + vec2(u_half_pixel.x * 2.0, 0.0));
    sum += texture(u_tex, v_uv + vec2(u_half_pixel.x, -u_half_pixel.y)) * 2.0;
    sum += texture(u_tex, v_uv + vec2(0.0, -u_half_pixel.y * 2.0));
    sum += texture(u_tex, v_uv + vec2(-u_half_pixel.x, -u_half_pixel.y)) * 2.0;
    frag_color = sum / 12.0;
}
"#;

/// Fullscreen blit fragment shader body.
pub const BLIT_FRAG_BODY: &str = r#"
uniform sampler2D u_tex;
in vec2 v_uv;
out vec4 frag_color;
void main() {
    frag_color = texture(u_tex, v_uv);
}
"#;

/// Forward 3D mesh vertex shader body (OpenGL 3.3 Core and ES 3.0 compatible).
pub const MESH3D_VERT_BODY: &str = r#"
layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec3 a_normal;
layout(location = 2) in vec2 a_uv;

uniform mat4 u_mvp;
uniform mat4 u_model;
uniform mat4 u_view;

out vec3 v_world_pos;
out vec3 v_normal;
out vec3 v_view_normal;
out vec4 v_clip_pos;
out vec2 v_uv;

void main() {
    vec4 wp = u_model * vec4(a_pos, 1.0);
    v_world_pos = wp.xyz;
    vec3 wn = normalize(mat3(u_model) * a_normal);
    v_normal = wn;
    v_view_normal = normalize(mat3(u_view) * wn);
    vec4 cp = u_mvp * vec4(a_pos, 1.0);
    v_clip_pos = cp;
    v_uv = a_uv;
    gl_Position = cp;
}
"#;

/// Forward 3D hemisphere + key + rim lit and optical refractive glass fragment shader body.
pub const MESH3D_FRAG_BODY: &str = r#"
in vec3 v_world_pos;
in vec3 v_normal;
in vec3 v_view_normal;
in vec4 v_clip_pos;
in vec2 v_uv;

uniform vec4 u_base_color;
uniform float u_roughness;
uniform sampler2D u_albedo_tex;
uniform int u_has_albedo_tex;
uniform vec3 u_camera_pos;
uniform vec4 u_glass_params; // (transmission, ior, dispersion, thickness)
uniform sampler2D u_backdrop_tex;
uniform int u_has_backdrop;

out vec4 frag_color;

vec3 sample_backdrop_channel(vec2 uv, float blur_r) {
    if (blur_r <= 0.0005) {
        return texture(u_backdrop_tex, clamp(uv, 0.001, 0.999)).rgb;
    }
    vec3 acc = texture(u_backdrop_tex, clamp(uv, 0.001, 0.999)).rgb * 0.28;
    acc += texture(u_backdrop_tex, clamp(uv + vec2( blur_r,  blur_r), 0.001, 0.999)).rgb * 0.18;
    acc += texture(u_backdrop_tex, clamp(uv + vec2(-blur_r,  blur_r), 0.001, 0.999)).rgb * 0.18;
    acc += texture(u_backdrop_tex, clamp(uv + vec2( blur_r, -blur_r), 0.001, 0.999)).rgb * 0.18;
    acc += texture(u_backdrop_tex, clamp(uv + vec2(-blur_r, -blur_r), 0.001, 0.999)).rgb * 0.18;
    return acc;
}

void main() {
    vec3 n = normalize(v_normal);
    if (!gl_FrontFacing) {
        n = -n;
    }
    vec3 view_dir = normalize(u_camera_pos - v_world_pos);
    vec4 base = u_base_color;
    if (u_has_albedo_tex != 0) {
        base *= texture(u_albedo_tex, v_uv);
    }

    float transmission = u_glass_params.x;
    if (transmission > 0.001) {
        float ior = max(u_glass_params.y, 1.0);
        float dispersion = max(u_glass_params.z, 0.0);
        float thickness = max(u_glass_params.w, 0.0);
        float rough = clamp(u_roughness, 0.0, 1.0);

        vec3 vn = normalize(v_view_normal);
        if (!gl_FrontFacing) {
            vn = -vn;
        }
        vec2 screen_uv = clamp((v_clip_pos.xy / max(v_clip_pos.w, 1e-4)) * 0.5 + 0.5, 0.001, 0.999);
        float eta = 1.0 / ior;
        vec3 refr_v = refract(vec3(0.0, 0.0, -1.0), vn, eta);
        vec2 refr_off = (length(refr_v) > 0.001 ? refr_v.xy : -vn.xy) * (1.0 - eta) * thickness * 0.22;

        float blur_r = rough * 0.016;
        vec3 bg_rgb;
        if (u_has_backdrop != 0) {
            vec2 uv_r = screen_uv + refr_off * (1.0 + dispersion * 1.6);
            vec2 uv_g = screen_uv + refr_off;
            vec2 uv_b = screen_uv + refr_off * (1.0 - dispersion * 1.6);
            float r = sample_backdrop_channel(uv_r, blur_r).r;
            float g = sample_backdrop_channel(uv_g, blur_r).g;
            float b = sample_backdrop_channel(uv_b, blur_r).b;
            bg_rgb = vec3(r, g, b);
        } else {
            float hemi_bg = n.y * 0.5 + 0.5;
            bg_rgb = mix(vec3(0.08, 0.12, 0.20), vec3(0.32, 0.44, 0.62), hemi_bg);
        }

        // Beer-Lambert-style tinting through glass thickness
        vec3 tint = mix(vec3(1.0), base.rgb, clamp(0.20 + thickness * 0.22, 0.0, 0.85));
        vec3 refracted = bg_rgb * tint;

        // Schlick Fresnel + crisp dual-lobe studio reflections
        float f0 = pow((ior - 1.0) / (ior + 1.0), 2.0);
        float ndotv = clamp(dot(n, view_dir), 0.0, 1.0);
        float fresnel = f0 + (1.0 - f0) * pow(1.0 - ndotv, 5.0);

        vec3 key_dir = normalize(vec3(0.55, 0.85, 0.65));
        vec3 fill_dir = normalize(vec3(-0.65, 0.35, 0.55));
        float spec_pow = mix(180.0, 14.0, rough);
        float spec1 = pow(max(dot(n, normalize(key_dir + view_dir)), 0.0), spec_pow) * mix(1.15, 0.30, rough);
        float spec2 = pow(max(dot(n, normalize(fill_dir + view_dir)), 0.0), spec_pow * 0.5) * mix(0.45, 0.15, rough);
        float hemi = n.y * 0.5 + 0.5;
        vec3 env_refl = mix(vec3(0.18, 0.26, 0.40), vec3(0.84, 0.93, 1.0), hemi) * (0.45 + 0.55 * fresnel);
        vec3 rim_glow = mix(vec3(0.70, 0.88, 1.0), base.rgb, 0.35) * pow(1.0 - ndotv, 3.0) * 0.55;

        vec3 col = mix(refracted, env_refl, clamp(fresnel * 0.78 + 0.04, 0.0, 0.92)) + vec3(spec1 + spec2) + rim_glow;
        float out_a = clamp(mix(base.a, 1.0, transmission), 0.0, 1.0);
        frag_color = vec4(col, out_a);
        return;
    }

    vec3 sky_dir = vec3(0.0, 1.0, 0.0);
    float hemi = dot(n, sky_dir) * 0.5 + 0.5;
    vec3 ambient = mix(vec3(0.22, 0.23, 0.29), vec3(0.55, 0.58, 0.68), hemi);

    vec3 key_dir = normalize(vec3(0.55, 0.85, 0.65));
    float diff = max(dot(n, key_dir), 0.0);

    vec3 rim_dir = normalize(vec3(-0.65, 0.25, -0.55));
    float rim = max(dot(n, rim_dir), 0.0) * 0.35;

    float spec_pow = mix(36.0, 6.0, clamp(u_roughness, 0.0, 1.0));
    vec3 half_v = normalize(key_dir + view_dir);
    float spec = pow(max(dot(n, half_v), 0.0), spec_pow) * 0.28;

    vec3 col = base.rgb * (ambient + vec3(0.68 * diff + rim)) + vec3(spec);
    frag_color = vec4(col, base.a);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::text::GlyphAtlas;
    use crate::gfx::viewport::{Canvas2D, SceneSnapshot};
    use crate::ui::draw::DrawList;
    use crate::ui::types::{Camera3D, Color, Gradient, Rect, RectShape};

    #[test]
    fn phase7_sdf_batch_encodes_gradients_transforms_tris_and_custom_passes() {
        let red = Color::from_hex(0xEF4444);
        let blue = Color::from_hex(0x3B82F6);
        let green = Color::from_hex(0x22C55E);
        let mut draw_list = DrawList::default();
        draw_list.base_cmds.push(DrawCmd::Rect {
            rect: Rect::new(10.0, 20.0, 100.0, 50.0),
            bg: red,
            bg_grad: Some(Gradient::linear(red, blue, 90.0)),
            border_width: 2.0,
            border_color: Color::WHITE,
            bd_grad: None,
            radius: 8.0,
            corner_radii: [8.0; 4],
            shadow_blur: 0.0,
            shadow_color: Color::TRANSPARENT,
            shape: RectShape::Box,
            glass: false,
            cursor_glow: false,
            ripple: None,
            shimmer_phase: None,
            transform: [1.5, 0.25, 12.0, -4.0],
        });

        let mut canvas = Canvas2D::new([0.0, 0.0]);
        canvas.fill_poly(&[[0.0, 0.0], [20.0, 0.0], [10.0, 20.0]], green);
        canvas.shader(7, "void main() { frag_color = vec4(1.0); }", &[0.25]);
        draw_list.base_cmds.push(DrawCmd::Viewport {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            canvas: canvas.cmds,
            scene: SceneSnapshot {
                camera: Camera3D::default(),
                draws: Vec::new(),
                msaa_samples: 4,
            },
        });

        let mut atlas = GlyphAtlas::new_fallback_only();
        let mut batch = SdfBatch::default();
        batch.build(&draw_list, &mut atlas, 1.0, None, 800, 600);

        assert_eq!(core::mem::size_of::<SdfInstance>(), 144);
        assert!(batch.instances.iter().any(|inst| {
            (inst.rect[0] - 22.0).abs() < 0.5
                && (inst.rect[1] - 16.0).abs() < 0.5
                && (inst.xform_and_grad[0] - 1.5).abs() < 1e-4
                && (inst.xform_and_grad[1] - 0.25).abs() < 1e-4
                && (inst.xform_and_grad[3] - 1.0).abs() < 1e-4
        }));
        assert!(batch
            .instances
            .iter()
            .any(|inst| (inst.radii_and_border[3] - 9.0).abs() < 1e-4));
        assert_eq!(batch.custom_2d.len(), 1);
    }

    #[test]
    fn write_bytes_into_matches_instance_size_and_serializes_all_fields() {
        let mut batch = SdfBatch::default();
        batch.instances.push(SdfInstance {
            rect: [1.0, 2.0, 3.0, 4.0],
            uv_rect: [0.1, 0.2, 0.3, 0.4],
            bg_color: [0.5, 0.6, 0.7, 0.8],
            border_color: [0.9, 1.0, 0.1, 0.2],
            radii_and_border: [4.0, 1.0, 0.0, 1.0],
            glow_and_clip: [10.0, 20.0, 0.5, 0.25],
            grad_color: [0.2, 0.3, 0.4, 1.0],
            xform_and_grad: [1.0, 0.0, 0.0, 0.0],
            corner_radii: [5.0, 6.0, 7.0, 8.0],
        });
        batch.instances.push(SdfInstance {
            rect: [11.0, 12.0, 13.0, 14.0],
            corner_radii: [15.0, 16.0, 17.0, 18.0],
            ..SdfInstance::default()
        });

        let mut bytes = Vec::new();
        batch.write_bytes_into(&mut bytes);

        let stride = core::mem::size_of::<SdfInstance>();
        assert_eq!(stride, 144);
        assert_eq!(bytes.len(), batch.instances.len() * stride);

        let read_vec4 = |offset: usize| -> [f32; 4] {
            [
                f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()),
                f32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()),
                f32::from_le_bytes(bytes[offset + 8..offset + 12].try_into().unwrap()),
                f32::from_le_bytes(bytes[offset + 12..offset + 16].try_into().unwrap()),
            ]
        };

        assert_eq!(read_vec4(128), [5.0, 6.0, 7.0, 8.0]);
        assert_eq!(read_vec4(stride), [11.0, 12.0, 13.0, 14.0]);
        assert_eq!(read_vec4(stride + 128), [15.0, 16.0, 17.0, 18.0]);
    }

    #[test]
    fn clip_and_viewport_culling_skips_offscreen_commands() {
        let mut draw_list = DrawList::default();
        draw_list
            .base_cmds
            .push(DrawCmd::PushClip(Rect::new(0.0, 0.0, 400.0, 300.0)));
        // Visible rect + visible text
        draw_list.base_cmds.push(DrawCmd::Rect {
            rect: Rect::new(20.0, 20.0, 100.0, 40.0),
            bg: Color::rgb(30, 40, 50),
            bg_grad: None,
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
            bd_grad: None,
            radius: 0.0,
            corner_radii: [0.0; 4],
            shadow_blur: 0.0,
            shadow_color: Color::TRANSPARENT,
            shape: RectShape::Box,
            glass: false,
            cursor_glow: false,
            ripple: None,
            shimmer_phase: None,
            transform: [1.0, 0.0, 0.0, 0.0],
        });
        draw_list.base_cmds.push(DrawCmd::Text {
            rect: Rect::new(20.0, 70.0, 120.0, 24.0),
            text: String::from("Visible"),
            color: Color::WHITE,
            size: 14.0,
            weight: 400,
            mono: false,
            font_family: None,
            wrap: false,
            ellipsis: false,
            align: crate::ui::types::TextAlign::Left,
            tracking: 0.0,
            line_height: 1.35,
            transform: [1.0, 0.0, 0.0, 0.0],
        });
        // Off-screen scrolled rect + text + viewport below clip
        draw_list.base_cmds.push(DrawCmd::Rect {
            rect: Rect::new(20.0, 800.0, 100.0, 40.0),
            bg: Color::rgb(200, 50, 50),
            bg_grad: None,
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
            bd_grad: None,
            radius: 0.0,
            corner_radii: [0.0; 4],
            shadow_blur: 0.0,
            shadow_color: Color::TRANSPARENT,
            shape: RectShape::Box,
            glass: false,
            cursor_glow: false,
            ripple: None,
            shimmer_phase: None,
            transform: [1.0, 0.0, 0.0, 0.0],
        });
        draw_list.base_cmds.push(DrawCmd::Text {
            rect: Rect::new(20.0, 900.0, 120.0, 24.0),
            text: String::from("OffscreenTextShouldBeCulled"),
            color: Color::WHITE,
            size: 14.0,
            weight: 400,
            mono: false,
            font_family: None,
            wrap: false,
            ellipsis: false,
            align: crate::ui::types::TextAlign::Left,
            tracking: 0.0,
            line_height: 1.35,
            transform: [1.0, 0.0, 0.0, 0.0],
        });
        draw_list.base_cmds.push(DrawCmd::Viewport {
            rect: Rect::new(0.0, 1000.0, 400.0, 300.0),
            canvas: vec![Canvas2DCmd::CustomShader {
                key: 99,
                vert_glsl: None,
                frag_glsl: String::from("void main(){}"),
                uniforms: vec![],
                rect: Rect::new(0.0, 0.0, 400.0, 300.0),
            }],
            scene: SceneSnapshot {
                camera: crate::ui::types::Camera3D::default(),
                draws: Vec::new(),
                msaa_samples: 4,
            },
        });
        draw_list.base_cmds.push(DrawCmd::PopClip);

        let mut atlas = GlyphAtlas::new_fallback_only();
        let mut batch = SdfBatch::default();
        batch.build(&draw_list, &mut atlas, 1.0, None, 800, 600);

        // No off-screen rect at y=800, no off-screen custom shader, visible rect + glyphs present.
        assert!(batch.custom_2d.is_empty());
        assert!(!batch.instances.iter().any(|i| i.rect[1] >= 700.0));
        assert!(batch
            .instances
            .iter()
            .any(|i| (i.rect[1] - 20.0).abs() < 0.5));
        assert!(batch.instances.len() > 1);
    }

    #[test]
    fn hexagon_and_pointy_hexagon_encode_rect_shadow_and_image_shape_codes() {
        let mut draw_list = DrawList::default();
        draw_list.base_cmds.push(DrawCmd::Rect {
            rect: Rect::new(10.0, 10.0, 42.0, 42.0),
            bg: Color::from_hex(0x38BDF8),
            bg_grad: None,
            border_width: 2.0,
            border_color: Color::WHITE,
            bd_grad: None,
            radius: 0.0,
            corner_radii: [0.0; 4],
            shadow_blur: 10.0,
            shadow_color: Color::rgba(0, 0, 0, 125),
            shape: RectShape::Hexagon,
            glass: false,
            cursor_glow: false,
            ripple: None,
            shimmer_phase: None,
            transform: [1.0, 0.0, 0.0, 0.0],
        });
        draw_list.base_cmds.push(DrawCmd::Image {
            rect: Rect::new(60.0, 10.0, 42.0, 42.0),
            tex: TextureId(7),
            filter: TextureFilter::Linear,
            uv: [0.0, 0.0, 1.0, 1.0],
            tint: Color::WHITE,
            border_width: 2.0,
            border_color: Color::from_hex(0x38BDF8),
            radius: 0.0,
            corner_radii: [0.0; 4],
            shape: RectShape::HexagonPointy,
            transform: [1.0, 0.0, 0.0, 0.0],
        });

        let mut atlas = GlyphAtlas::new_fallback_only();
        let mut batch = SdfBatch::default();
        batch.build(&draw_list, &mut atlas, 1.0, None, 400, 200);

        // Shadow instance for flat-top hexagon: kind == 6.0, sub_shape == 11.0.
        assert!(batch.instances.iter().any(|i| {
            (i.radii_and_border[3] - 6.0).abs() < 1e-4 && (i.glow_and_clip[2] - 11.0).abs() < 1e-4
        }));
        // Rect instance for flat-top hexagon: kind == 11.0.
        assert!(batch
            .instances
            .iter()
            .any(|i| (i.radii_and_border[3] - 11.0).abs() < 1e-4));
        // Image instance for pointy-top hexagon: kind == 8.0, sub_shape == 12.0.
        assert!(batch.instances.iter().any(|i| {
            (i.radii_and_border[3] - 8.0).abs() < 1e-4 && (i.glow_and_clip[2] - 12.0).abs() < 1e-4
        }));
    }
}
