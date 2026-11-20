use std::collections::HashMap;
use std::sync::Arc;

#[cfg(feature = "gltf")]
use crate::ui::types::Error;
use crate::ui::types::{Camera3D, Color, MeshId, NodeId, Rect, TextureId};

/// Sampling filter mode for 2D viewport textures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextureFilter {
    /// Crisp pixel-art nearest-neighbor sampling (`GL_NEAREST`).
    Nearest,
    /// Smooth bilinear texture filtering (`GL_LINEAR`).
    #[default]
    Linear,
}

/// Batched 2D primitive emitted by `Canvas2D`.
#[derive(Clone, Debug, PartialEq)]
pub enum Canvas2DCmd {
    Tex {
        tex: TextureId,
        rect: Rect,
        filter: TextureFilter,
    },
    Rect {
        rect: Rect,
        color: Color,
        radius: f32,
    },
    Segment {
        p0: [f32; 2],
        p1: [f32; 2],
        thickness: f32,
        color: Color,
    },
    Tri {
        p0: [f32; 2],
        p1: [f32; 2],
        p2: [f32; 2],
        color: Color,
    },
    CustomShader {
        key: u64,
        vert_glsl: Option<String>,
        frag_glsl: String,
        uniforms: Vec<f32>,
        rect: Rect,
    },
    DrawInstanced {
        key: u64,
        vertices: Vec<[f32; 4]>,
        instances: Vec<f32>,
        stride_floats: usize,
        vert_glsl: String,
        frag_glsl: String,
        uniforms: Vec<f32>,
        rect: Rect,
    },
}

/// Immediate-mode batched 2D drawing surface inside a `viewport` widget.
#[derive(Clone, Debug, Default)]
pub struct Canvas2D {
    pub(crate) origin: [f32; 2],
    pub(crate) size: [f32; 2],
    pub(crate) cmds: Vec<Canvas2DCmd>,
}

impl Canvas2D {
    /// Creates a new `Canvas2D` rooted at viewport top-left `origin` in logical points.
    pub fn new(origin: [f32; 2]) -> Self {
        Self {
            origin,
            size: [0.0, 0.0],
            cmds: Vec::new(),
        }
    }

    /// Returns the viewport's latest laid-out `[width, height]` in logical points.
    pub fn size(&self) -> [f32; 2] {
        self.size
    }

    fn offset(&self, p: [f32; 2]) -> [f32; 2] {
        [self.origin[0] + p[0], self.origin[1] + p[1]]
    }

    /// Draws a registered GPU texture into `rect` with `Nearest` or `Linear` filtering.
    pub fn tex(&mut self, tex: TextureId, rect: Rect, filter: TextureFilter) {
        self.cmds.push(Canvas2DCmd::Tex {
            tex,
            rect: Rect::new(
                self.origin[0] + rect.x,
                self.origin[1] + rect.y,
                rect.w,
                rect.h,
            ),
            filter,
        });
    }

    /// Draws a solid rounded rectangle inside the viewport.
    pub fn rect(&mut self, rect: Rect, color: Color, radius: f32) {
        self.cmds.push(Canvas2DCmd::Rect {
            rect: Rect::new(
                self.origin[0] + rect.x,
                self.origin[1] + rect.y,
                rect.w,
                rect.h,
            ),
            color,
            radius,
        });
    }

    /// Draws an anti-aliased `sdSegment` line from `p0` to `p1`.
    pub fn line(&mut self, p0: [f32; 2], p1: [f32; 2], thickness: f32, color: Color) {
        self.cmds.push(Canvas2DCmd::Segment {
            p0: self.offset(p0),
            p1: self.offset(p1),
            thickness: thickness.max(0.5),
            color,
        });
    }

    /// Draws a cubic Bezier curve tessellated into smooth `sdSegment` segments.
    pub fn bezier(
        &mut self,
        p0: [f32; 2],
        c0: [f32; 2],
        c1: [f32; 2],
        p1: [f32; 2],
        thickness: f32,
        color: Color,
    ) {
        const STEPS: usize = 16;
        let mut prev = p0;
        for step in 1..=STEPS {
            let t = step as f32 / STEPS as f32;
            let u = 1.0 - t;
            let u2 = u * u;
            let t2 = t * t;
            let w0 = u2 * u;
            let w1 = 3.0 * u2 * t;
            let w2 = 3.0 * u * t2;
            let w3 = t2 * t;
            let pt = [
                w0 * p0[0] + w1 * c0[0] + w2 * c1[0] + w3 * p1[0],
                w0 * p0[1] + w1 * c0[1] + w2 * c1[1] + w3 * p1[1],
            ];
            self.line(prev, pt, thickness, color);
            prev = pt;
        }
    }

    /// Draws a polyline (optionally closed) from `points`.
    pub fn poly(&mut self, points: &[[f32; 2]], closed: bool, thickness: f32, color: Color) {
        if points.len() < 2 {
            return;
        }
        for pair in points.windows(2) {
            self.line(pair[0], pair[1], thickness, color);
        }
        if closed {
            if let (Some(&first), Some(&last)) = (points.first(), points.last()) {
                self.line(last, first, thickness, color);
            }
        }
    }

    /// Draws a filled anti-aliased SDF circle at `center` with `radius`.
    pub fn circle(&mut self, center: [f32; 2], radius: f32, color: Color) {
        let r = radius.max(0.5);
        self.rect(
            Rect::new(center[0] - r, center[1] - r, r * 2.0, r * 2.0),
            color,
            r,
        );
    }

    /// Draws a stroked circle outline centered at `center`.
    pub fn circle_stroke(&mut self, center: [f32; 2], radius: f32, thickness: f32, color: Color) {
        self.arc(
            center,
            radius,
            0.0,
            core::f32::consts::TAU,
            thickness,
            color,
        );
    }

    /// Draws a stroked circular arc from `start_rad` to `end_rad` around `center`.
    pub fn arc(
        &mut self,
        center: [f32; 2],
        radius: f32,
        start_rad: f32,
        end_rad: f32,
        thickness: f32,
        color: Color,
    ) {
        let r = radius.max(0.5);
        let span = end_rad - start_rad;
        if span.abs() < 1e-4 {
            return;
        }
        let steps = ((span.abs() * r / 3.0).ceil() as usize).clamp(8, 72);
        let mut prev = [
            center[0] + r * start_rad.cos(),
            center[1] + r * start_rad.sin(),
        ];
        for i in 1..=steps {
            let a = start_rad + span * (i as f32 / steps as f32);
            let next = [center[0] + r * a.cos(), center[1] + r * a.sin()];
            self.line(prev, next, thickness, color);
            prev = next;
        }
    }

    /// Draws a filled polygon (convex or simple concave) tessellated via ear clipping.
    pub fn fill_poly(&mut self, points: &[[f32; 2]], color: Color) {
        if points.len() < 3 {
            return;
        }
        let mut verts: Vec<[f32; 2]> = points.iter().map(|&p| self.offset(p)).collect();
        dedup_ring(&mut verts);
        if verts.len() < 3 {
            return;
        }
        let area = signed_poly_area(&verts);
        if area.abs() < 1e-4 {
            return;
        }
        if area < 0.0 {
            verts.reverse();
        }
        self.ear_clip_triangulate(&verts, color);
    }

    /// Draws multiple closed rings using the SVG even-odd fill rule (supporting nested hole cutouts).
    pub fn fill_poly_even_odd(&mut self, rings: &[Vec<[f32; 2]>], color: Color) {
        let mut cleaned: Vec<(Vec<[f32; 2]>, f32)> = Vec::new();
        for ring in rings {
            if ring.len() < 3 {
                continue;
            }
            let mut verts: Vec<[f32; 2]> = ring.iter().map(|&p| self.offset(p)).collect();
            dedup_ring(&mut verts);
            if verts.len() < 3 {
                continue;
            }
            let area = signed_poly_area(&verts);
            if area.abs() < 1e-4 {
                continue;
            }
            cleaned.push((verts, area.abs()));
        }
        if cleaned.is_empty() {
            return;
        }
        if cleaned.len() == 1 {
            let (mut verts, _) = cleaned.into_iter().next().unwrap();
            if signed_poly_area(&verts) < 0.0 {
                verts.reverse();
            }
            self.ear_clip_triangulate(&verts, color);
            return;
        }

        let probes: Vec<[f32; 2]> = cleaned.iter().map(|(r, _)| interior_point(r)).collect();
        let mut depths = vec![0usize; cleaned.len()];
        for i in 0..cleaned.len() {
            let pt = probes[i];
            let mut d = 0usize;
            for j in 0..cleaned.len() {
                if i != j && cleaned[j].1 > cleaned[i].1 && point_in_poly(pt, &cleaned[j].0) {
                    d += 1;
                }
            }
            depths[i] = d;
        }

        let mut holes_for: Vec<Vec<usize>> = vec![Vec::new(); cleaned.len()];
        for i in 0..cleaned.len() {
            if !depths[i].is_multiple_of(2) {
                let pt = probes[i];
                let mut best_parent: Option<(usize, usize, f32)> = None;
                for j in 0..cleaned.len() {
                    if i != j
                        && depths[j].is_multiple_of(2)
                        && cleaned[j].1 > cleaned[i].1
                        && point_in_poly(pt, &cleaned[j].0)
                    {
                        let cand = (j, depths[j], cleaned[j].1);
                        let better = match best_parent {
                            None => true,
                            Some((_, bd, ba)) => cand.1 > bd || (cand.1 == bd && cand.2 < ba),
                        };
                        if better {
                            best_parent = Some(cand);
                        }
                    }
                }
                if let Some((parent, _, _)) = best_parent {
                    holes_for[parent].push(i);
                }
            }
        }

        for i in 0..cleaned.len() {
            if !depths[i].is_multiple_of(2) {
                continue;
            }
            let mut outer = cleaned[i].0.clone();
            if signed_poly_area(&outer) < 0.0 {
                outer.reverse();
            }
            let mut holes: Vec<Vec<[f32; 2]>> = holes_for[i]
                .iter()
                .map(|&hi| {
                    let mut h = cleaned[hi].0.clone();
                    if signed_poly_area(&h) > 0.0 {
                        h.reverse();
                    }
                    h
                })
                .collect();
            if !holes.is_empty() {
                outer = bridge_holes_into_outer(outer, &mut holes);
            }
            self.ear_clip_triangulate(&outer, color);
        }
    }

    fn ear_clip_triangulate(&mut self, verts: &[[f32; 2]], color: Color) {
        if verts.len() < 3 {
            return;
        }
        let mut idx_list: Vec<usize> = (0..verts.len()).collect();
        let mut guard = verts.len().saturating_mul(verts.len());
        while idx_list.len() > 3 && guard > 0 {
            guard -= 1;
            let n = idx_list.len();
            let mut clipped = false;
            for i in 0..n {
                let prev = idx_list[(i + n - 1) % n];
                let curr = idx_list[i];
                let next = idx_list[(i + 1) % n];
                let (a, b, c) = (verts[prev], verts[curr], verts[next]);
                if cross2d(a, b, c) <= 1e-5 {
                    continue;
                }
                let contains_other = idx_list.iter().any(|&k| {
                    k != prev
                        && k != curr
                        && k != next
                        && !pts_close(verts[k], a)
                        && !pts_close(verts[k], b)
                        && !pts_close(verts[k], c)
                        && point_in_tri(verts[k], a, b, c)
                });
                if !contains_other {
                    self.cmds.push(Canvas2DCmd::Tri {
                        p0: a,
                        p1: b,
                        p2: c,
                        color,
                    });
                    idx_list.remove(i);
                    clipped = true;
                    break;
                }
            }
            if !clipped {
                break;
            }
        }
        if idx_list.len() == 3 {
            let (a, b, c) = (verts[idx_list[0]], verts[idx_list[1]], verts[idx_list[2]]);
            if cross2d(a, b, c).abs() > 1e-5 {
                self.cmds.push(Canvas2DCmd::Tri {
                    p0: a,
                    p1: b,
                    p2: c,
                    color,
                });
            }
        }
    }

    /// Parses and renders an SVG path `d` string (`M/m/L/l/H/h/V/v/C/c/S/s/Q/q/T/t/A/a/Z/z`) scaled by `1.0`.
    pub fn svg_path(&mut self, d: &str, stroke_w: f32, fill: Option<Color>, stroke: Option<Color>) {
        self.svg_path_scaled(d, [0.0, 0.0], 1.0, stroke_w, fill, stroke);
    }

    /// Parses and renders an SVG path `d` string translated by `offset` and scaled by `scale`.
    pub fn svg_path_scaled(
        &mut self,
        d: &str,
        offset: [f32; 2],
        scale: f32,
        stroke_w: f32,
        fill: Option<Color>,
        stroke: Option<Color>,
    ) {
        let subpaths = parse_svg_path_subpaths(d);
        let s = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let mut fill_rings: Vec<Vec<[f32; 2]>> = Vec::new();
        let mut stroke_rings: Vec<(Vec<[f32; 2]>, bool)> = Vec::new();
        for (pts, closed) in subpaths {
            if pts.is_empty() {
                continue;
            }
            let mapped: Vec<[f32; 2]> = pts
                .into_iter()
                .map(|[x, y]| [offset[0] + x * s, offset[1] + y * s])
                .collect();
            if fill.is_some() && mapped.len() >= 3 {
                fill_rings.push(mapped.clone());
            }
            if stroke.is_some() {
                stroke_rings.push((mapped, closed));
            }
        }
        if let Some(fill_col) = fill {
            self.fill_poly_even_odd(&fill_rings, fill_col);
        }
        if let Some(stroke_col) = stroke {
            for (mapped, closed) in stroke_rings {
                self.poly(&mapped, closed, stroke_w, stroke_col);
            }
        }
    }

    /// Parses and renders an SVG icon path fitted into `size x size` pixels (assuming a `24x24` viewBox unless coordinates exceed `24.5`).
    pub fn svg_icon_path(&mut self, d: &str, size: f32, color: Color) {
        let subpaths = parse_svg_path_subpaths(d);
        let mut max_coord = 0.0_f32;
        for (pts, _) in &subpaths {
            for &[x, y] in pts {
                max_coord = max_coord.max(x.abs()).max(y.abs());
            }
        }
        let vb = if max_coord > 24.5 { max_coord } else { 24.0 };
        let scale = (size.max(1.0)) / vb;
        let stroke_w = (1.8 * scale).max(1.0);
        let mut closed_rings: Vec<Vec<[f32; 2]>> = Vec::new();
        for (pts, closed) in subpaths {
            if pts.is_empty() {
                continue;
            }
            let mapped: Vec<[f32; 2]> = pts
                .into_iter()
                .map(|[x, y]| [x * scale, y * scale])
                .collect();
            if closed && mapped.len() >= 3 {
                closed_rings.push(mapped);
            } else {
                self.poly(&mapped, closed, stroke_w, color);
            }
        }
        if !closed_rings.is_empty() {
            let before = self.cmds.len();
            self.fill_poly_even_odd(&closed_rings, color);
            if self.cmds.len() == before {
                for ring in &closed_rings {
                    self.poly(ring, true, stroke_w, color);
                }
            }
        }
    }

    /// Executes a custom GLSL fragment shader pass over the viewport rect.
    ///
    /// # GLSL Contract
    /// Lumenic automatically prepends `#version 330 core` (desktop OpenGL) or
    /// `#version 300 es\nprecision highp float;` (GLES 3.0 / WebGL 2). Do **not**
    /// include `#version` or `precision` lines in `frag_glsl`.
    ///
    /// - **Vertex input:** `in vec2 v_uv;` (`0.0..=1.0` across the viewport rect, top-left to bottom-right).
    /// - **Fragment output:** `out vec4 frag_color;` (do not use `gl_FragColor`).
    /// - **Built-in uniforms:**
    ///   - `uniform vec4 u_rect;` (`[x, y, w, h]` in physical pixels)
    ///   - `uniform vec2 u_viewport;` (window framebuffer `[w, h]` in physical pixels)
    ///   - `uniform vec2 u_resolution;` (viewport `[w, h]` in physical pixels)
    ///   - `uniform float u_time;` (elapsed seconds)
    ///   - `uniform vec2 u_pointer;` (pointer position in physical pixels)
    ///   - `uniform vec2 u_scroll;` (frame scroll delta)
    ///   - `uniform float u_uniforms[N];` (bound from `uniforms` slice)
    ///
    /// # Compositing & `--inspect`
    /// Custom shader passes execute before [`Scene3D`] passes in the same `ui.viewport`,
    /// and `Scene3D` clears its FBO to transparent `(0, 0, 0, 0)` before alpha-blitting
    /// over `Canvas2D`. Note that `--inspect` uses a CPU-only software rasterizer with no
    /// OpenGL context, so custom GLSL passes are skipped during `--inspect`.
    pub fn shader(&mut self, key: u64, frag_glsl: &str, uniforms: &[f32]) {
        self.cmds.push(Canvas2DCmd::CustomShader {
            key,
            vert_glsl: None,
            frag_glsl: String::from(frag_glsl),
            uniforms: uniforms.to_vec(),
            rect: Rect::new(self.origin[0], self.origin[1], self.size[0], self.size[1]),
        });
    }

    /// Executes a custom vertex + fragment GLSL shader pass over the viewport rect.
    ///
    /// Uses the same `#version` injection, `out vec4 frag_color;`, and built-in uniforms
    /// (`u_rect`, `u_viewport`, `u_resolution`, `u_time`, `u_pointer`, `u_scroll`, `u_uniforms`)
    /// as [`Canvas2D::shader`]. The built-in quad vertex stream provides
    /// `layout(location = 0) in vec2 a_pos;` (`0.0..=1.0`).
    /// Skipped in CPU-only `--inspect` mode.
    pub fn shader_vf(&mut self, key: u64, vert_glsl: &str, frag_glsl: &str, uniforms: &[f32]) {
        self.cmds.push(Canvas2DCmd::CustomShader {
            key,
            vert_glsl: Some(String::from(vert_glsl)),
            frag_glsl: String::from(frag_glsl),
            uniforms: uniforms.to_vec(),
            rect: Rect::new(self.origin[0], self.origin[1], self.size[0], self.size[1]),
        });
    }

    /// Issues a single instanced GPU draw call (`vertexAttribDivisor`) over custom vertices
    /// and per-instance float payloads.
    ///
    /// # GLSL Attribute & Uniform Contract
    /// Uses the same `#version` injection, `out vec4 frag_color;`, and built-in uniforms as
    /// [`Canvas2D::shader`]:
    /// - `layout(location = 0) in vec4 a_vertex;` (per-vertex `[f32; 4]` from `vertices`)
    /// - `layout(location = 1..=6) in vec4 ...;` (per-instance attributes with divisor `1`,
    ///   packed in 4-float chunks from `instances` up to `stride_floats`, max 6 slots / 24 floats)
    ///
    /// Skipped in CPU-only `--inspect` mode.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_instanced(
        &mut self,
        key: u64,
        vertices: &[[f32; 4]],
        instances: &[f32],
        stride_floats: usize,
        vert_glsl: &str,
        frag_glsl: &str,
        uniforms: &[f32],
    ) {
        if vertices.is_empty() || instances.is_empty() || stride_floats == 0 {
            return;
        }
        self.cmds.push(Canvas2DCmd::DrawInstanced {
            key,
            vertices: vertices.to_vec(),
            instances: instances.to_vec(),
            stride_floats,
            vert_glsl: String::from(vert_glsl),
            frag_glsl: String::from(frag_glsl),
            uniforms: uniforms.to_vec(),
            rect: Rect::new(self.origin[0], self.origin[1], self.size[0], self.size[1]),
        });
    }

    /// Returns queued 2D commands for this viewport frame.
    pub fn commands(&self) -> &[Canvas2DCmd] {
        &self.cmds
    }
}

fn cross2d(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn signed_poly_area(pts: &[[f32; 2]]) -> f32 {
    let mut sum = 0.0_f32;
    for i in 0..pts.len() {
        let p0 = pts[i];
        let p1 = pts[(i + 1) % pts.len()];
        sum += p0[0] * p1[1] - p1[0] * p0[1];
    }
    sum * 0.5
}

fn point_in_tri(p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> bool {
    let c0 = cross2d(a, b, p);
    let c1 = cross2d(b, c, p);
    let c2 = cross2d(c, a, p);
    c0 >= -1e-5 && c1 >= -1e-5 && c2 >= -1e-5
}

fn pts_close(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4
}

fn dedup_ring(verts: &mut Vec<[f32; 2]>) {
    verts.dedup_by(|a, b| pts_close(*a, *b));
    while verts.len() > 2 && pts_close(verts[0], verts[verts.len() - 1]) {
        verts.pop();
    }
}

fn point_in_poly(p: [f32; 2], poly: &[[f32; 2]]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let a = poly[i];
        let b = poly[j];
        let cond = (a[1] > p[1]) != (b[1] > p[1]);
        if cond {
            let x_int = a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
            if p[0] < x_int {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

fn interior_point(ring: &[[f32; 2]]) -> [f32; 2] {
    let n = ring.len();
    if n < 3 {
        return ring.first().copied().unwrap_or([0.0, 0.0]);
    }
    let sign = if signed_poly_area(ring) >= 0.0 {
        1.0
    } else {
        -1.0
    };
    for i in 0..n {
        let a = ring[(i + n - 1) % n];
        let b = ring[i];
        let c = ring[(i + 1) % n];
        if cross2d(a, b, c) * sign <= 1e-5 {
            continue;
        }
        let (ta, tb, tc) = if sign > 0.0 { (a, b, c) } else { (c, b, a) };
        let has_inside = ring.iter().enumerate().any(|(k, &p)| {
            k != (i + n - 1) % n && k != i && k != (i + 1) % n && point_in_tri(p, ta, tb, tc)
        });
        if !has_inside {
            return [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0];
        }
    }
    ring[0]
}

// Splices CW holes into a CCW outer ring via Eberly's rightmost-vertex ray-cast bridge.
fn bridge_holes_into_outer(mut outer: Vec<[f32; 2]>, holes: &mut [Vec<[f32; 2]>]) -> Vec<[f32; 2]> {
    let mut hole_order: Vec<(usize, f32)> = holes
        .iter()
        .enumerate()
        .map(|(idx, h)| {
            let max_x = h.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max);
            (idx, max_x)
        })
        .collect();
    hole_order.sort_by(|a, b| b.1.total_cmp(&a.1));

    for (h_idx, _) in hole_order {
        let hole = &holes[h_idx];
        if hole.len() < 3 || outer.len() < 3 {
            continue;
        }
        let mut m_idx = 0usize;
        for (i, &p) in hole.iter().enumerate() {
            if p[0] > hole[m_idx][0] || (p[0] == hole[m_idx][0] && p[1] > hole[m_idx][1]) {
                m_idx = i;
            }
        }
        let m = hole[m_idx];

        let mut best_x = f32::INFINITY;
        let mut best_edge_p_idx: Option<usize> = None;
        let mut hit_pt = m;
        let on = outer.len();
        for i in 0..on {
            let a = outer[i];
            let b = outer[(i + 1) % on];
            let crosses_y = (a[1] <= m[1] && b[1] > m[1]) || (b[1] <= m[1] && a[1] > m[1]);
            if !crosses_y {
                continue;
            }
            let t = (m[1] - a[1]) / (b[1] - a[1]);
            let ix = a[0] + t * (b[0] - a[0]);
            if ix >= m[0] - 1e-5 && ix < best_x {
                best_x = ix;
                hit_pt = [ix, m[1]];
                best_edge_p_idx = Some(if a[0] >= b[0] { i } else { (i + 1) % on });
            }
        }

        let bridge_outer_idx = if let Some(p_idx) = best_edge_p_idx {
            let p = outer[p_idx];
            if (hit_pt[0] - p[0]).abs() < 1e-5 && (hit_pt[1] - p[1]).abs() < 1e-5 {
                p_idx
            } else {
                let mut best_r = p_idx;
                let mut best_cos = -2.0_f32;
                let mut best_dist = f32::INFINITY;
                let tri_ccw = cross2d(m, hit_pt, p) >= 0.0;
                for (ri, &r) in outer.iter().enumerate() {
                    if ri == p_idx {
                        continue;
                    }
                    let inside = if tri_ccw {
                        point_in_tri(r, m, hit_pt, p)
                    } else {
                        point_in_tri(r, m, p, hit_pt)
                    };
                    if !inside {
                        continue;
                    }
                    let dx = r[0] - m[0];
                    let dy = r[1] - m[1];
                    let dist = dx * dx + dy * dy;
                    if dist <= 1e-8 {
                        continue;
                    }
                    let cos_a = dx / dist.sqrt();
                    if cos_a > best_cos + 1e-5
                        || ((cos_a - best_cos).abs() <= 1e-5 && dist < best_dist)
                    {
                        best_cos = cos_a;
                        best_dist = dist;
                        best_r = ri;
                    }
                }
                best_r
            }
        } else {
            // Fallback: nearest outer vertex to m.
            let mut nearest = 0usize;
            let mut best_d = f32::INFINITY;
            for (ri, &r) in outer.iter().enumerate() {
                let d = (r[0] - m[0]).powi(2) + (r[1] - m[1]).powi(2);
                if d < best_d {
                    best_d = d;
                    nearest = ri;
                }
            }
            nearest
        };

        let mut merged = Vec::with_capacity(outer.len() + hole.len() + 2);
        merged.extend_from_slice(&outer[..=bridge_outer_idx]);
        for k in 0..hole.len() {
            merged.push(hole[(m_idx + k) % hole.len()]);
        }
        merged.push(hole[m_idx]);
        merged.push(outer[bridge_outer_idx]);
        merged.extend_from_slice(&outer[bridge_outer_idx + 1..]);
        outer = merged;
    }
    outer
}

#[derive(Clone, Debug)]
enum SvgTok {
    Cmd(char),
    Num(f32),
}

fn tokenize_svg_path(d: &str) -> Vec<SvgTok> {
    let bytes = d.as_bytes();
    let mut toks = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() || b == b',' {
            i += 1;
            continue;
        }
        if b.is_ascii_alphabetic() {
            toks.push(SvgTok::Cmd(b as char));
            i += 1;
            continue;
        }
        let start = i;
        if b == b'+' || b == b'-' {
            i += 1;
        }
        let mut seen_dot = false;
        let mut seen_exp = false;
        while i < bytes.len() {
            let c = bytes[i];
            if c.is_ascii_digit() {
                i += 1;
            } else if c == b'.' && !seen_dot && !seen_exp {
                seen_dot = true;
                i += 1;
            } else if (c == b'e' || c == b'E') && !seen_exp {
                seen_exp = true;
                i += 1;
                if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
                    i += 1;
                }
            } else {
                break;
            }
        }
        if i > start {
            if let Ok(v) = d[start..i].parse::<f32>() {
                if v.is_finite() {
                    toks.push(SvgTok::Num(v));
                }
            }
        } else {
            i += 1;
        }
    }
    toks
}

/// Parses an SVG `d` path string into `(polyline_points, closed)` subpaths.
pub(crate) fn parse_svg_path_subpaths(d: &str) -> Vec<(Vec<[f32; 2]>, bool)> {
    let toks = tokenize_svg_path(d);
    let mut subpaths: Vec<(Vec<[f32; 2]>, bool)> = Vec::new();
    let mut cur_pts: Vec<[f32; 2]> = Vec::new();
    let mut cur = [0.0_f32, 0.0_f32];
    let mut start = [0.0_f32, 0.0_f32];
    let mut last_cubic_ctrl: Option<[f32; 2]> = None;
    let mut last_quad_ctrl: Option<[f32; 2]> = None;

    let mut i = 0usize;
    let mut active_cmd = ' ';
    let take_num = |idx: &mut usize| -> Option<f32> {
        if let Some(SvgTok::Num(v)) = toks.get(*idx) {
            *idx += 1;
            Some(*v)
        } else {
            None
        }
    };

    while i < toks.len() {
        if let SvgTok::Cmd(c) = toks[i] {
            active_cmd = c;
            i += 1;
            if active_cmd == 'Z' || active_cmd == 'z' {
                if !cur_pts.is_empty() {
                    subpaths.push((core::mem::take(&mut cur_pts), true));
                }
                cur = start;
                last_cubic_ctrl = None;
                last_quad_ctrl = None;
                continue;
            }
        } else if active_cmd == ' ' {
            i += 1;
            continue;
        }

        let rel = active_cmd.is_ascii_lowercase();
        let up = active_cmd.to_ascii_uppercase();
        match up {
            'M' => {
                let (Some(x), Some(y)) = (take_num(&mut i), take_num(&mut i)) else {
                    break;
                };
                if !cur_pts.is_empty() {
                    subpaths.push((core::mem::take(&mut cur_pts), false));
                }
                cur = if rel {
                    [cur[0] + x, cur[1] + y]
                } else {
                    [x, y]
                };
                start = cur;
                cur_pts.push(cur);
                active_cmd = if rel { 'l' } else { 'L' };
                last_cubic_ctrl = None;
                last_quad_ctrl = None;
            }
            'L' => {
                let (Some(x), Some(y)) = (take_num(&mut i), take_num(&mut i)) else {
                    break;
                };
                cur = if rel {
                    [cur[0] + x, cur[1] + y]
                } else {
                    [x, y]
                };
                cur_pts.push(cur);
                last_cubic_ctrl = None;
                last_quad_ctrl = None;
            }
            'H' => {
                let Some(x) = take_num(&mut i) else {
                    break;
                };
                cur[0] = if rel { cur[0] + x } else { x };
                cur_pts.push(cur);
                last_cubic_ctrl = None;
                last_quad_ctrl = None;
            }
            'V' => {
                let Some(y) = take_num(&mut i) else {
                    break;
                };
                cur[1] = if rel { cur[1] + y } else { y };
                cur_pts.push(cur);
                last_cubic_ctrl = None;
                last_quad_ctrl = None;
            }
            'C' => {
                let (Some(x1), Some(y1), Some(x2), Some(y2), Some(x), Some(y)) = (
                    take_num(&mut i),
                    take_num(&mut i),
                    take_num(&mut i),
                    take_num(&mut i),
                    take_num(&mut i),
                    take_num(&mut i),
                ) else {
                    break;
                };
                let c1 = if rel {
                    [cur[0] + x1, cur[1] + y1]
                } else {
                    [x1, y1]
                };
                let c2 = if rel {
                    [cur[0] + x2, cur[1] + y2]
                } else {
                    [x2, y2]
                };
                let p1 = if rel {
                    [cur[0] + x, cur[1] + y]
                } else {
                    [x, y]
                };
                flatten_cubic(cur, c1, c2, p1, &mut cur_pts);
                cur = p1;
                last_cubic_ctrl = Some(c2);
                last_quad_ctrl = None;
            }
            'S' => {
                let (Some(x2), Some(y2), Some(x), Some(y)) = (
                    take_num(&mut i),
                    take_num(&mut i),
                    take_num(&mut i),
                    take_num(&mut i),
                ) else {
                    break;
                };
                let c1 = match last_cubic_ctrl {
                    Some(pc) => [2.0 * cur[0] - pc[0], 2.0 * cur[1] - pc[1]],
                    None => cur,
                };
                let c2 = if rel {
                    [cur[0] + x2, cur[1] + y2]
                } else {
                    [x2, y2]
                };
                let p1 = if rel {
                    [cur[0] + x, cur[1] + y]
                } else {
                    [x, y]
                };
                flatten_cubic(cur, c1, c2, p1, &mut cur_pts);
                cur = p1;
                last_cubic_ctrl = Some(c2);
                last_quad_ctrl = None;
            }
            'Q' => {
                let (Some(x1), Some(y1), Some(x), Some(y)) = (
                    take_num(&mut i),
                    take_num(&mut i),
                    take_num(&mut i),
                    take_num(&mut i),
                ) else {
                    break;
                };
                let c = if rel {
                    [cur[0] + x1, cur[1] + y1]
                } else {
                    [x1, y1]
                };
                let p1 = if rel {
                    [cur[0] + x, cur[1] + y]
                } else {
                    [x, y]
                };
                flatten_quad(cur, c, p1, &mut cur_pts);
                cur = p1;
                last_quad_ctrl = Some(c);
                last_cubic_ctrl = None;
            }
            'T' => {
                let (Some(x), Some(y)) = (take_num(&mut i), take_num(&mut i)) else {
                    break;
                };
                let c = match last_quad_ctrl {
                    Some(pc) => [2.0 * cur[0] - pc[0], 2.0 * cur[1] - pc[1]],
                    None => cur,
                };
                let p1 = if rel {
                    [cur[0] + x, cur[1] + y]
                } else {
                    [x, y]
                };
                flatten_quad(cur, c, p1, &mut cur_pts);
                cur = p1;
                last_quad_ctrl = Some(c);
                last_cubic_ctrl = None;
            }
            'A' => {
                let (Some(rx), Some(ry), Some(rot_deg), Some(large), Some(sweep), Some(x), Some(y)) = (
                    take_num(&mut i),
                    take_num(&mut i),
                    take_num(&mut i),
                    take_num(&mut i),
                    take_num(&mut i),
                    take_num(&mut i),
                    take_num(&mut i),
                ) else {
                    break;
                };
                let p1 = if rel {
                    [cur[0] + x, cur[1] + y]
                } else {
                    [x, y]
                };
                flatten_svg_arc(
                    cur,
                    rx,
                    ry,
                    rot_deg,
                    large != 0.0,
                    sweep != 0.0,
                    p1,
                    &mut cur_pts,
                );
                cur = p1;
                last_cubic_ctrl = None;
                last_quad_ctrl = None;
            }
            _ => break,
        }
    }
    if !cur_pts.is_empty() {
        subpaths.push((cur_pts, false));
    }
    subpaths
}

fn flatten_cubic(p0: [f32; 2], c1: [f32; 2], c2: [f32; 2], p1: [f32; 2], out: &mut Vec<[f32; 2]>) {
    const STEPS: usize = 12;
    for s in 1..=STEPS {
        let t = s as f32 / STEPS as f32;
        let u = 1.0 - t;
        let u2 = u * u;
        let t2 = t * t;
        out.push([
            u2 * u * p0[0] + 3.0 * u2 * t * c1[0] + 3.0 * u * t2 * c2[0] + t2 * t * p1[0],
            u2 * u * p0[1] + 3.0 * u2 * t * c1[1] + 3.0 * u * t2 * c2[1] + t2 * t * p1[1],
        ]);
    }
}

fn flatten_quad(p0: [f32; 2], c: [f32; 2], p1: [f32; 2], out: &mut Vec<[f32; 2]>) {
    const STEPS: usize = 10;
    for s in 1..=STEPS {
        let t = s as f32 / STEPS as f32;
        let u = 1.0 - t;
        out.push([
            u * u * p0[0] + 2.0 * u * t * c[0] + t * t * p1[0],
            u * u * p0[1] + 2.0 * u * t * c[1] + t * t * p1[1],
        ]);
    }
}

#[allow(clippy::too_many_arguments)]
fn flatten_svg_arc(
    p0: [f32; 2],
    rx_in: f32,
    ry_in: f32,
    x_rot_deg: f32,
    large_arc: bool,
    sweep: bool,
    p1: [f32; 2],
    out: &mut Vec<[f32; 2]>,
) {
    let dx2 = (p0[0] - p1[0]) * 0.5;
    let dy2 = (p0[1] - p1[1]) * 0.5;
    if (dx2 * dx2 + dy2 * dy2) < 1e-6 || rx_in.abs() < 1e-6 || ry_in.abs() < 1e-6 {
        out.push(p1);
        return;
    }
    let phi = x_rot_deg.to_radians();
    let (sin_p, cos_p) = phi.sin_cos();
    let x1p = cos_p * dx2 + sin_p * dy2;
    let y1p = -sin_p * dx2 + cos_p * dy2;
    let mut rx = rx_in.abs();
    let mut ry = ry_in.abs();
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let rx2 = rx * rx;
    let ry2 = ry * ry;
    let num = (rx2 * ry2 - rx2 * y1p * y1p - ry2 * x1p * x1p).max(0.0);
    let den = (rx2 * y1p * y1p + ry2 * x1p * x1p).max(1e-8);
    let sign = if large_arc == sweep { -1.0 } else { 1.0 };
    let factor = sign * (num / den).sqrt();
    let cxp = factor * (rx * y1p / ry);
    let cyp = factor * -(ry * x1p / rx);
    let cx = cos_p * cxp - sin_p * cyp + (p0[0] + p1[0]) * 0.5;
    let cy = sin_p * cxp + cos_p * cyp + (p0[1] + p1[1]) * 0.5;

    let v_angle = |ux: f32, uy: f32, vx: f32, vy: f32| -> f32 {
        let dot = ux * vx + uy * vy;
        let len = ((ux * ux + uy * uy) * (vx * vx + vy * vy)).sqrt().max(1e-8);
        let a = (dot / len).clamp(-1.0, 1.0).acos();
        if ux * vy - uy * vx < 0.0 {
            -a
        } else {
            a
        }
    };
    let ux = (x1p - cxp) / rx;
    let uy = (y1p - cyp) / ry;
    let theta1 = v_angle(1.0, 0.0, ux, uy);
    let mut dtheta = v_angle(ux, uy, (-x1p - cxp) / rx, (-y1p - cyp) / ry);
    if !sweep && dtheta > 0.0 {
        dtheta -= core::f32::consts::TAU;
    } else if sweep && dtheta < 0.0 {
        dtheta += core::f32::consts::TAU;
    }
    let steps = ((dtheta.abs() * rx.max(ry) / 3.0).ceil() as usize).clamp(8, 48);
    for s in 1..=steps {
        let a = theta1 + dtheta * (s as f32 / steps as f32);
        let (sa, ca) = a.sin_cos();
        out.push([
            cx + cos_p * (rx * ca) - sin_p * (ry * sa),
            cy + sin_p * (rx * ca) + cos_p * (ry * sa),
        ]);
    }
}

/// 3D vertex payload for `Scene3D` meshes.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex3D {
    /// Local object-space position `[x, y, z]`.
    pub pos: [f32; 3],
    /// Unit surface normal `[nx, ny, nz]`.
    pub normal: [f32; 3],
    /// Texture coordinates `[u, v]`.
    pub uv: [f32; 2],
}

/// Indexed triangle mesh data owned by `Scene3D`.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshData {
    /// Vertex buffer elements.
    pub vertices: Vec<Vertex3D>,
    /// Zero-based triangle index list (`len % 3 == 0`).
    pub indices: Vec<u32>,
}

impl MeshData {
    /// Generates a unit-normal indexed cube centered at the origin with side length `size`.
    pub fn cube(size: f32) -> Self {
        let h = size * 0.5;
        let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
            (
                [0.0, 0.0, 1.0],
                [[-h, -h, h], [h, -h, h], [h, h, h], [-h, h, h]],
            ),
            (
                [0.0, 0.0, -1.0],
                [[h, -h, -h], [-h, -h, -h], [-h, h, -h], [h, h, -h]],
            ),
            (
                [0.0, 1.0, 0.0],
                [[-h, h, h], [h, h, h], [h, h, -h], [-h, h, -h]],
            ),
            (
                [0.0, -1.0, 0.0],
                [[-h, -h, -h], [h, -h, -h], [h, -h, h], [-h, -h, h]],
            ),
            (
                [1.0, 0.0, 0.0],
                [[h, -h, h], [h, -h, -h], [h, h, -h], [h, h, h]],
            ),
            (
                [-1.0, 0.0, 0.0],
                [[-h, -h, -h], [-h, -h, h], [-h, h, h], [-h, h, -h]],
            ),
        ];
        let uvs = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let mut vertices = Vec::with_capacity(24);
        let mut indices = Vec::with_capacity(36);
        for (fi, (n, corners)) in faces.iter().enumerate() {
            let base = (fi as u32) * 4;
            for ci in 0..4 {
                vertices.push(Vertex3D {
                    pos: corners[ci],
                    normal: *n,
                    uv: uvs[ci],
                });
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        Self { vertices, indices }
    }
}

/// Forward PBR-lite material properties for `Scene3D`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    /// Albedo base tint multiplied by optional `texture`.
    pub base_color: Color,
    /// Metallicity factor (`0.0` dielectric to `1.0` metal).
    pub metallic: f32,
    /// Surface roughness factor (`0.0` glossy to `1.0` matte).
    pub roughness: f32,
    /// Optional albedo texture handle.
    pub texture: Option<TextureId>,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            base_color: Color::WHITE,
            metallic: 0.0,
            roughness: 0.5,
            texture: None,
        }
    }
}

/// Optical refractive glass material for [`Scene3D::draw_mesh_glass`].
///
/// Samples the underlying `Canvas2D` / UI backdrop behind the `Scene3D` viewport and applies
/// index-of-refraction (IOR) distortion, RGB chromatic dispersion, roughness-driven frosted blur,
/// Schlick Fresnel reflections, and crisp dual-lobe specular highlights.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassMaterial {
    /// Body tint / absorption color of the glass.
    pub tint: Color,
    /// Optical transmission (`0.0` opaque to `1.0` fully transmissive). Default `0.94`.
    pub transmission: f32,
    /// Index of refraction (`1.0` air, `1.33` water, `1.5` crown glass, `2.4` diamond). Default `1.52`.
    pub ior: f32,
    /// Chromatic dispersion separating R/G/B refracted UV offsets (`0.0..=0.25`). Default `0.04`.
    pub dispersion: f32,
    /// Virtual optical thickness scaling refraction offset (`0.0..=2.0`). Default `0.65`.
    pub thickness: f32,
    /// Surface roughness (`0.0` crystal clear to `1.0` frosted blur). Default `0.08`.
    pub roughness: f32,
    /// Optional tint/albedo texture.
    pub texture: Option<TextureId>,
}

impl Default for GlassMaterial {
    fn default() -> Self {
        Self {
            tint: Color::rgba(220, 238, 255, 255),
            transmission: 0.94,
            ior: 1.52,
            dispersion: 0.04,
            thickness: 0.65,
            roughness: 0.08,
            texture: None,
        }
    }
}

impl GlassMaterial {
    /// Sets the glass body tint color.
    pub fn tint(mut self, color: Color) -> Self {
        self.tint = color;
        self
    }

    /// Sets optical transmission (`0.0..=1.0`).
    pub fn transmission(mut self, transmission: f32) -> Self {
        self.transmission = transmission.clamp(0.0, 1.0);
        self
    }

    /// Sets the index of refraction (clamped to `1.0..=3.0`).
    pub fn ior(mut self, ior: f32) -> Self {
        self.ior = ior.clamp(1.0, 3.0);
        self
    }

    /// Sets RGB chromatic dispersion strength (`0.0..=0.25`).
    pub fn dispersion(mut self, dispersion: f32) -> Self {
        self.dispersion = dispersion.clamp(0.0, 0.25);
        self
    }

    /// Sets virtual optical thickness (`0.0..=2.0`).
    pub fn thickness(mut self, thickness: f32) -> Self {
        self.thickness = thickness.clamp(0.0, 2.0);
        self
    }

    /// Sets surface roughness / frostiness (`0.0..=1.0`).
    pub fn roughness(mut self, roughness: f32) -> Self {
        self.roughness = roughness.clamp(0.0, 1.0);
        self
    }

    /// Attaches an optional tint texture.
    pub fn texture(mut self, texture: Option<TextureId>) -> Self {
        self.texture = texture;
        self
    }
}

/// Custom 3D fragment shader payload queued via [`Scene3D::draw_mesh_shader`].
#[derive(Clone, Debug, PartialEq)]
pub struct CustomShader3D {
    pub key: u64,
    pub frag_glsl: String,
    pub uniforms: Vec<f32>,
}

/// Local TRS transform on a `Scene3D` graph node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform3D {
    /// Local `[x, y, z]` translation offset.
    pub translation: [f32; 3],
    /// Local orientation quaternion `[x, y, z, w]`.
    pub rotation_quat: [f32; 4],
    /// Local non-uniform `[sx, sy, sz]` scale factors.
    pub scale: [f32; 3],
}

impl Default for Transform3D {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform3D {
    /// Identity TRS transform (`[0, 0, 0]`, unit quaternion `[0, 0, 0, 1]`, scale `[1, 1, 1]`).
    pub const IDENTITY: Self = Self {
        translation: [0.0, 0.0, 0.0],
        rotation_quat: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0, 1.0, 1.0],
    };

    /// Creates a pure translation transform.
    pub const fn from_translation(x: f32, y: f32, z: f32) -> Self {
        Self {
            translation: [x, y, z],
            ..Self::IDENTITY
        }
    }

    /// Creates a pure quaternion rotation transform (`[x, y, z, w]`).
    pub const fn from_quat(q: [f32; 4]) -> Self {
        Self {
            rotation_quat: q,
            ..Self::IDENTITY
        }
    }

    /// Creates a pure non-uniform scale transform.
    pub const fn from_scale(sx: f32, sy: f32, sz: f32) -> Self {
        Self {
            scale: [sx, sy, sz],
            ..Self::IDENTITY
        }
    }

    /// Creates a full translation, quaternion rotation (`[x, y, z, w]`), and scale transform.
    pub const fn from_trs(pos: [f32; 3], rot_quat: [f32; 4], scale: [f32; 3]) -> Self {
        Self {
            translation: pos,
            rotation_quat: rot_quat,
            scale,
        }
    }

    /// Creates a transform from XYZ Euler angles in radians.
    pub fn from_euler_xyz(rx: f32, ry: f32, rz: f32) -> Self {
        let (sx, cx) = (rx * 0.5).sin_cos();
        let (sy, cy) = (ry * 0.5).sin_cos();
        let (sz, cz) = (rz * 0.5).sin_cos();
        // Intrinsic Z * Y * X quaternion composition.
        let qx = sx * cy * cz - cx * sy * sz;
        let qy = cx * sy * cz + sx * cy * sz;
        let qz = cx * cy * sz - sx * sy * cz;
        let qw = cx * cy * cz + sx * sy * sz;
        Self {
            translation: [0.0, 0.0, 0.0],
            rotation_quat: [qx, qy, qz, qw],
            scale: [1.0, 1.0, 1.0],
        }
    }

    /// Converts TRS into a column-major 4x4 affine matrix (`m[col][row]`).
    pub fn to_mat4(&self) -> [[f32; 4]; 4] {
        let [qx, qy, qz, qw] = self.rotation_quat;
        let len = (qx * qx + qy * qy + qz * qz + qw * qw).sqrt();
        let (x, y, z, w) = if len > 1e-6 {
            (qx / len, qy / len, qz / len, qw / len)
        } else {
            (0.0, 0.0, 0.0, 1.0)
        };

        let x2 = x + x;
        let y2 = y + y;
        let z2 = z + z;
        let xx = x * x2;
        let xy = x * y2;
        let xz = x * z2;
        let yy = y * y2;
        let yz = y * z2;
        let zz = z * z2;
        let wx = w * x2;
        let wy = w * y2;
        let wz = w * z2;

        let [sx, sy, sz] = self.scale;
        let [tx, ty, tz] = self.translation;

        [
            [(1.0 - (yy + zz)) * sx, (xy + wz) * sx, (xz - wy) * sx, 0.0],
            [(xy - wz) * sy, (1.0 - (xx + zz)) * sy, (yz + wx) * sy, 0.0],
            [(xz + wy) * sz, (yz - wx) * sz, (1.0 - (xx + yy)) * sz, 0.0],
            [tx, ty, tz, 1.0],
        ]
    }
}

/// Multiplies two column-major 4x4 matrices (`a * b`).
pub fn mat4_mul(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0.0_f32; 4]; 4];
    for c in 0..4 {
        for r in 0..4 {
            out[c][r] =
                a[0][r] * b[c][0] + a[1][r] * b[c][1] + a[2][r] * b[c][2] + a[3][r] * b[c][3];
        }
    }
    out
}

pub fn mat4_mul_vec4(m: [[f32; 4]; 4], v: [f32; 4]) -> [f32; 4] {
    [
        m[0][0] * v[0] + m[1][0] * v[1] + m[2][0] * v[2] + m[3][0] * v[3],
        m[0][1] * v[0] + m[1][1] * v[1] + m[2][1] * v[2] + m[3][1] * v[3],
        m[0][2] * v[0] + m[1][2] * v[1] + m[2][2] * v[2] + m[3][2] * v[3],
        m[0][3] * v[0] + m[1][3] * v[1] + m[2][3] * v[2] + m[3][3] * v[3],
    ]
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm3(v: [f32; 3]) -> [f32; 3] {
    let l = dot3(v, v).sqrt();
    if l > 1e-6 {
        [v[0] / l, v[1] / l, v[2] / l]
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// Computes the column-major view matrix for `Camera3D`.
pub fn camera_view(cam: &Camera3D) -> [[f32; 4]; 4] {
    let f = norm3(sub3(cam.target, cam.position));
    let s = norm3(cross3(f, norm3(cam.up)));
    let u = cross3(s, f);

    [
        [s[0], u[0], -f[0], 0.0],
        [s[1], u[1], -f[1], 0.0],
        [s[2], u[2], -f[2], 0.0],
        [
            -dot3(s, cam.position),
            -dot3(u, cam.position),
            dot3(f, cam.position),
            1.0,
        ],
    ]
}

/// Computes `proj * view` column-major 4x4 matrix for `Camera3D` at `aspect = w / h`.
pub fn camera_view_proj(cam: &Camera3D, aspect: f32) -> [[f32; 4]; 4] {
    let view = camera_view(cam);

    let fov_rad = cam.fov_deg.clamp(10.0, 140.0).to_radians();
    let tan_half = (fov_rad * 0.5).tan().max(1e-4);
    let a = aspect.max(0.01);
    let n = cam.near.max(0.01);
    let far = cam.far.max(n + 0.1);
    let range = far - n;

    let proj = [
        [1.0 / (a * tan_half), 0.0, 0.0, 0.0],
        [0.0, 1.0 / tan_half, 0.0, 0.0],
        [0.0, 0.0, -(far + n) / range, -1.0],
        [0.0, 0.0, -(2.0 * far * n) / range, 0.0],
    ];

    mat4_mul(proj, view)
}

#[derive(Clone, Debug)]
struct SceneNodeRecord {
    parent: Option<NodeId>,
    local: Transform3D,
    explicit_mat: Option<[[f32; 4]; 4]>,
    last_used_frame: u64,
    persistent: bool,
}

/// Queued 3D draw instance resolved to world space.
#[derive(Clone, Debug)]
pub struct SceneDrawCall {
    pub mesh: MeshId,
    pub model_mat: [[f32; 4]; 4],
    pub material: Material,
    pub glass: Option<GlassMaterial>,
    pub custom_shader: Option<CustomShader3D>,
}

/// Resolved 3D draw call carrying a shared `Arc<MeshData>` handle for zero-copy per-frame emission.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneDrawResolved {
    pub mesh_id: MeshId,
    pub mesh: Arc<MeshData>,
    pub model_mat: [[f32; 4]; 4],
    pub material: Material,
    pub glass: Option<GlassMaterial>,
    pub custom_shader: Option<CustomShader3D>,
}

/// Snapshot of a `Scene3D` frame stored inside `DrawCmd::Viewport`.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneSnapshot {
    pub camera: Camera3D,
    pub draws: Vec<SceneDrawResolved>,
    pub msaa_samples: u8,
}

/// Framework-owned result of importing a `.glb` asset without exposing `gltf` types.
#[cfg(feature = "gltf")]
#[derive(Clone, Debug, Default)]
pub struct LoadedGltf {
    pub meshes: Vec<MeshId>,
    pub nodes: Vec<NodeId>,
}

/// Forward depth-buffered 3D scene pass with hierarchical `NodeId` parent-child transforms.
#[derive(Clone, Debug)]
pub struct Scene3D {
    camera: Camera3D,
    next_mesh_id: u64,
    next_node_id: u64,
    frame_gen: u64,
    meshes: HashMap<MeshId, Arc<MeshData>>,
    nodes: HashMap<NodeId, SceneNodeRecord>,
    draws: Vec<SceneDrawCall>,
    raw_fbo_id: Option<u32>,
    msaa_samples: u8,
}

impl Default for Scene3D {
    fn default() -> Self {
        Self::new()
    }
}

impl Scene3D {
    /// Creates an empty `Scene3D` with a default perspective camera and 4x MSAA enabled.
    pub fn new() -> Self {
        Self {
            camera: Camera3D::default(),
            next_mesh_id: 1,
            next_node_id: 1,
            frame_gen: 0,
            meshes: HashMap::new(),
            nodes: HashMap::new(),
            draws: Vec::new(),
            raw_fbo_id: None,
            msaa_samples: 4,
        }
    }

    /// Sets the hardware MSAA sample count for the 3D viewport FBO (`0` or `1` disables MSAA; default `4`).
    pub fn set_msaa(&mut self, samples: u8) {
        self.msaa_samples = if samples <= 1 { 1 } else { samples.min(16) };
    }

    /// Returns the configured MSAA sample count for the 3D viewport FBO.
    pub fn msaa(&self) -> u8 {
        self.msaa_samples
    }

    /// Sets the active 3D perspective camera.
    pub fn set_camera(&mut self, camera: Camera3D) {
        self.camera = camera;
    }

    /// Returns the active camera configuration.
    pub fn camera(&self) -> Camera3D {
        self.camera
    }

    /// Returns the underlying GL framebuffer ID for this viewport when GPU-backed.
    pub fn raw_fbo_id(&self) -> Option<u32> {
        self.raw_fbo_id
    }

    pub(crate) fn set_raw_fbo_id(&mut self, id: Option<u32>) {
        self.raw_fbo_id = id;
    }

    /// Clears queued per-frame draw calls and prunes stale unreferenced one-shot nodes.
    pub fn begin_frame(&mut self) {
        self.draws.clear();
        self.frame_gen = self.frame_gen.wrapping_add(1);
        if self.nodes.len() > 256 {
            let fg = self.frame_gen;
            self.nodes
                .retain(|_, rec| rec.persistent || fg.wrapping_sub(rec.last_used_frame) <= 2);
        }
    }

    /// Registers a 3D mesh and returns an opaque `MeshId`.
    pub fn register_mesh(&mut self, mesh: MeshData) -> MeshId {
        let id = MeshId(self.next_mesh_id);
        self.next_mesh_id = self.next_mesh_id.saturating_add(1);
        self.meshes.insert(id, Arc::new(mesh));
        id
    }

    /// Creates a hierarchical scene node optionally parented under `parent`.
    pub fn create_node(&mut self, parent: Option<NodeId>, local: Transform3D) -> NodeId {
        let id = NodeId(self.next_node_id);
        self.next_node_id = self.next_node_id.saturating_add(1);
        self.nodes.insert(
            id,
            SceneNodeRecord {
                parent,
                local,
                explicit_mat: None,
                last_used_frame: self.frame_gen,
                persistent: false,
            },
        );
        id
    }

    /// Updates a node's local TRS transform.
    pub fn set_node_transform(&mut self, node: NodeId, local: Transform3D) {
        if let Some(rec) = self.nodes.get_mut(&node) {
            rec.local = local;
            rec.explicit_mat = None;
            rec.last_used_frame = self.frame_gen;
            rec.persistent = true;
        }
    }

    /// Computes the world-space transform matrix for `node` by multiplying its parent chain.
    pub fn world_matrix(&self, node: NodeId) -> [[f32; 4]; 4] {
        let mut chain = Vec::with_capacity(8);
        let mut cur = Some(node);
        let mut depth = 0usize;
        while let Some(nid) = cur {
            if depth >= 64 {
                break;
            }
            let Some(rec) = self.nodes.get(&nid) else {
                break;
            };
            let local_mat = rec.explicit_mat.unwrap_or_else(|| rec.local.to_mat4());
            chain.push(local_mat);
            cur = rec.parent;
            depth += 1;
        }

        let mut acc = Transform3D::IDENTITY.to_mat4();
        for mat in chain.into_iter().rev() {
            acc = mat4_mul(acc, mat);
        }
        acc
    }

    fn touch_node_chain(&mut self, node: NodeId) {
        let mut cur = Some(node);
        let mut depth = 0usize;
        let fg = self.frame_gen;
        while let Some(nid) = cur {
            if depth >= 64 {
                break;
            }
            let Some(rec) = self.nodes.get_mut(&nid) else {
                break;
            };
            rec.last_used_frame = fg;
            cur = rec.parent;
            depth += 1;
        }
    }

    /// Queues `mesh` to render at `node`'s world transform with `material`.
    pub fn draw_mesh(&mut self, mesh: MeshId, node: NodeId, material: Material) {
        self.touch_node_chain(node);
        let model_mat = self.world_matrix(node);
        self.draws.push(SceneDrawCall {
            mesh,
            model_mat,
            material,
            glass: None,
            custom_shader: None,
        });
    }

    /// Queues `mesh` to render at `node`'s world transform with optical refractive glass shading.
    ///
    /// Captures the underlying `Canvas2D` / UI backdrop behind this viewport and refracts it
    /// using `glass.ior`, `glass.dispersion`, `glass.thickness`, and `glass.roughness` with
    /// Schlick Fresnel reflections and MSAA edges.
    pub fn draw_mesh_glass(&mut self, mesh: MeshId, node: NodeId, glass: GlassMaterial) {
        self.touch_node_chain(node);
        let model_mat = self.world_matrix(node);
        self.draws.push(SceneDrawCall {
            mesh,
            model_mat,
            material: Material {
                base_color: glass.tint,
                metallic: 0.0,
                roughness: glass.roughness,
                texture: glass.texture,
            },
            glass: Some(glass),
            custom_shader: None,
        });
    }

    /// Queues `mesh` to render at `node`'s world transform with a custom GLSL fragment shader.
    ///
    /// ### GLSL Shader Contract
    /// Lumenic automatically prepends `#version 330 core` (desktop) or
    /// `#version 300 es\nprecision highp float;\nprecision highp int;\n` (WebGL 2 / GLES 3).
    /// Do **not** include a `#version` line in `frag_glsl`.
    ///
    /// | Direction | Declaration | Description |
    /// |---|---|---|
    /// | Input | `in vec3 v_world_pos;` | World-space vertex position |
    /// | Input | `in vec3 v_normal;` | Normalized world-space normal |
    /// | Input | `in vec3 v_view_normal;` | Normalized view-space normal |
    /// | Input | `in vec4 v_clip_pos;` | Clip-space position (`v_clip_pos.xy / v_clip_pos.w * 0.5 + 0.5` = viewport UV) |
    /// | Input | `in vec2 v_uv;` | Mesh texture coordinates |
    /// | Output | `out vec4 frag_color;` | Final RGBA color |
    /// | Uniform | `uniform sampler2D u_backdrop_tex;` | Underlying `Canvas2D`/UI backdrop captured behind this viewport |
    /// | Uniform | `uniform int u_has_backdrop;` | `1` when `u_backdrop_tex` is valid |
    /// | Uniform | `uniform vec3 u_camera_pos;` | World-space camera position |
    /// | Uniform | `uniform vec2 u_resolution;` | Viewport physical `[width, height]` |
    /// | Uniform | `uniform float u_time;` | Elapsed app time in seconds |
    /// | Uniform | `uniform float u_uniforms[16];` | Up to 16 caller-provided `f32` values from `uniforms` |
    pub fn draw_mesh_shader(
        &mut self,
        mesh: MeshId,
        node: NodeId,
        key: u64,
        frag_glsl: &str,
        uniforms: &[f32],
    ) {
        self.touch_node_chain(node);
        let model_mat = self.world_matrix(node);
        self.draws.push(SceneDrawCall {
            mesh,
            model_mat,
            material: Material::default(),
            glass: None,
            custom_shader: Some(CustomShader3D {
                key,
                frag_glsl: String::from(frag_glsl),
                uniforms: uniforms.iter().copied().take(16).collect(),
            }),
        });
    }

    /// Returns registered mesh data by `MeshId`.
    pub fn mesh(&self, id: MeshId) -> Option<&MeshData> {
        self.meshes.get(&id).map(Arc::as_ref)
    }

    /// Returns all queued 3D draw calls for the current frame.
    pub fn draws(&self) -> &[SceneDrawCall] {
        &self.draws
    }

    /// Captures an immutable snapshot of this frame's camera and resolved 3D draws.
    pub(crate) fn snapshot(&self) -> SceneSnapshot {
        let mut resolved = Vec::with_capacity(self.draws.len());
        for d in &self.draws {
            if let Some(mesh) = self.meshes.get(&d.mesh) {
                resolved.push(SceneDrawResolved {
                    mesh_id: d.mesh,
                    mesh: Arc::clone(mesh),
                    model_mat: d.model_mat,
                    material: d.material,
                    glass: d.glass,
                    custom_shader: d.custom_shader.clone(),
                });
            }
        }
        SceneSnapshot {
            camera: self.camera,
            draws: resolved,
            msaa_samples: self.msaa_samples,
        }
    }

    /// Parses a binary `.glb` asset into internal `MeshId` and `NodeId` entries.
    #[cfg(feature = "gltf")]
    pub fn load_glb_bytes(&mut self, bytes: &[u8]) -> Result<LoadedGltf, Error> {
        let doc = gltf::Gltf::from_slice(bytes)
            .map_err(|e| Error::InvalidInput(format!("glTF parse error: {e}")))?;
        let blob = doc.blob.as_deref();

        let mut loaded = LoadedGltf::default();
        for mesh in doc.meshes() {
            let mut vertices = Vec::new();
            let mut indices = Vec::new();
            for prim in mesh.primitives() {
                let reader = prim.reader(|_| blob);
                let Some(positions) = reader.read_positions() else {
                    continue;
                };
                let base_vertex = u32::try_from(vertices.len())
                    .map_err(|_| Error::InvalidInput(String::from("mesh vertex index overflow")))?;
                let mut normals: Vec<[f32; 3]> = reader
                    .read_normals()
                    .map(|it| it.collect())
                    .unwrap_or_default();
                let mut uvs: Vec<[f32; 2]> = reader
                    .read_tex_coords(0)
                    .map(|tc| tc.into_f32().collect())
                    .unwrap_or_default();

                let pos_vec: Vec<[f32; 3]> = positions.collect();
                if normals.len() != pos_vec.len() {
                    normals.resize(pos_vec.len(), [0.0, 1.0, 0.0]);
                }
                if uvs.len() != pos_vec.len() {
                    uvs.resize(pos_vec.len(), [0.0, 0.0]);
                }
                for i in 0..pos_vec.len() {
                    vertices.push(Vertex3D {
                        pos: pos_vec[i],
                        normal: normals[i],
                        uv: uvs[i],
                    });
                }

                if let Some(read_idx) = reader.read_indices() {
                    for idx in read_idx.into_u32() {
                        let shifted = base_vertex.checked_add(idx).ok_or_else(|| {
                            Error::InvalidInput(String::from("mesh index overflow"))
                        })?;
                        indices.push(shifted);
                    }
                } else {
                    for i in 0..u32::try_from(pos_vec.len()).unwrap_or(0) {
                        indices.push(base_vertex.saturating_add(i));
                    }
                }
            }
            if !vertices.is_empty() {
                let mesh_id = self.register_mesh(MeshData { vertices, indices });
                loaded.meshes.push(mesh_id);
            }
        }

        let mut node_map: HashMap<usize, NodeId> = HashMap::new();
        for node in doc.nodes() {
            let mat = node.transform().matrix();
            let id = self.create_node(None, Transform3D::IDENTITY);
            if let Some(rec) = self.nodes.get_mut(&id) {
                rec.explicit_mat = Some(mat);
                rec.persistent = true;
            }
            node_map.insert(node.index(), id);
            loaded.nodes.push(id);
        }
        for node in doc.nodes() {
            let parent_id = node_map.get(&node.index()).copied();
            for child in node.children() {
                if let Some(&cid) = node_map.get(&child.index()) {
                    if let Some(rec) = self.nodes.get_mut(&cid) {
                        rec.parent = parent_id;
                    }
                }
            }
        }

        Ok(loaded)
    }
}

#[cfg(test)]
mod tests {
    use super::{Canvas2D, Canvas2DCmd, Material, MeshData, Scene3D, TextureFilter, Transform3D};
    use crate::ui::types::{Color, Rect, TextureId};

    #[test]
    fn node_hierarchy_world_matrix_and_canvas2d_batching() {
        let mut scene = Scene3D::new();
        let mesh = scene.register_mesh(MeshData::cube(1.0));
        let root = scene.create_node(None, Transform3D::from_translation(10.0, 0.0, 0.0));
        let child = scene.create_node(Some(root), Transform3D::from_translation(0.0, 5.0, 0.0));
        let grand = scene.create_node(Some(child), Transform3D::from_translation(0.0, 0.0, -3.0));

        let m = scene.world_matrix(grand);
        assert!((m[3][0] - 10.0).abs() < 1e-4);
        assert!((m[3][1] - 5.0).abs() < 1e-4);
        assert!((m[3][2] - (-3.0)).abs() < 1e-4);

        let q_node = scene.create_node(
            None,
            Transform3D::from_trs([1.0, 2.0, 3.0], [0.0, 0.0, 0.0, 1.0], [2.0, 3.0, 4.0]),
        );
        let qm = scene.world_matrix(q_node);
        assert!((qm[0][0] - 2.0).abs() < 1e-4);
        assert!((qm[1][1] - 3.0).abs() < 1e-4);
        assert!((qm[2][2] - 4.0).abs() < 1e-4);
        assert!((qm[3][0] - 1.0).abs() < 1e-4);
        assert!((qm[3][1] - 2.0).abs() < 1e-4);
        assert!((qm[3][2] - 3.0).abs() < 1e-4);
        assert_eq!(
            Transform3D::from_quat([0.0, 0.0, 0.0, 1.0]),
            Transform3D::IDENTITY
        );
        assert_eq!(
            Transform3D::from_scale(1.0, 1.0, 1.0),
            Transform3D::IDENTITY
        );

        scene.draw_mesh(mesh, grand, Material::default());
        assert_eq!(scene.draws().len(), 1);

        let mut c2d = Canvas2D::new([10.0, 20.0]);
        c2d.tex(
            TextureId(1),
            Rect::new(0.0, 0.0, 32.0, 32.0),
            TextureFilter::Nearest,
        );
        c2d.bezier(
            [0.0, 0.0],
            [10.0, 20.0],
            [30.0, 20.0],
            [40.0, 0.0],
            2.0,
            Color::WHITE,
        );
        c2d.poly(
            &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]],
            true,
            1.5,
            Color::WHITE,
        );
        // 1 tex + 16 bezier segments + 3 closed poly segments = 20 commands.
        assert_eq!(c2d.commands().len(), 20);
    }

    #[cfg(feature = "gltf")]
    #[test]
    fn glb_binary_loads_mesh_and_nodes() {
        // Minimal valid GLB containing one triangle (3 vertices, 36 bytes binary buffer).
        let mut bin = Vec::new();
        for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        let json = concat!(
            r#"{"asset":{"version":"2.0"},"buffers":[{"byteLength":36}],"#,
            r#""bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36}],"#,
            r#""accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}],"#,
            r#""meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],"#,
            r#""nodes":[{"mesh":0,"translation":[2,3,4]}]}"#
        );
        let mut json_bytes = json.as_bytes().to_vec();
        while !json_bytes.len().is_multiple_of(4) {
            json_bytes.push(b' ');
        }
        let total_len = 12 + 8 + json_bytes.len() + 8 + bin.len();
        let mut glb = Vec::with_capacity(total_len);
        glb.extend_from_slice(&0x46546C67u32.to_le_bytes()); // "glTF"
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&(total_len as u32).to_le_bytes());
        glb.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x4E4F534Au32.to_le_bytes()); // "JSON"
        glb.extend_from_slice(&json_bytes);
        glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x004E4942u32.to_le_bytes()); // "BIN\0"
        glb.extend_from_slice(&bin);

        let mut scene = Scene3D::new();
        let loaded = scene.load_glb_bytes(&glb).expect("glb loads");
        assert_eq!(loaded.meshes.len(), 1);
        assert_eq!(loaded.nodes.len(), 1);
        let m = scene.world_matrix(loaded.nodes[0]);
        assert!((m[3][0] - 2.0).abs() < 1e-4);
        assert!((m[3][1] - 3.0).abs() < 1e-4);
        assert!((m[3][2] - 4.0).abs() < 1e-4);
    }

    #[test]
    fn phase7_vector_svg_shader_and_instanced_commands() {
        let mut c2d = Canvas2D::new([0.0, 0.0]);
        let red = Color::from_hex(0xEF4444);
        let blue = Color::from_hex(0x3B82F6);
        let green = Color::from_hex(0x22C55E);
        c2d.circle([50.0, 50.0], 20.0, red);
        c2d.circle_stroke([50.0, 50.0], 20.0, 2.0, blue);
        c2d.arc([50.0, 50.0], 15.0, 0.0, core::f32::consts::PI, 2.0, green);
        c2d.fill_poly(
            &[[0.0, 0.0], [20.0, 0.0], [20.0, 20.0], [0.0, 20.0]],
            Color::WHITE,
        );
        c2d.svg_path(
            "M 2 2 L 22 2 L 12 22 Z",
            1.5,
            Some(Color::from_hex(0xFACC15)),
            None,
        );
        c2d.shader(101, "void main() { frag_color = vec4(1.0); }", &[0.5, 1.0]);
        c2d.draw_instanced(
            202,
            &[
                [0.0, 0.0, 0.0, 0.0],
                [1.0, 0.0, 1.0, 0.0],
                [0.0, 1.0, 0.0, 1.0],
            ],
            &[10.0, 20.0, 30.0, 40.0],
            2,
            "void main() { gl_Position = vec4(0.0); }",
            "void main() { frag_color = vec4(1.0); }",
            &[1.0],
        );

        let tri_count = c2d
            .commands()
            .iter()
            .filter(|c| matches!(c, Canvas2DCmd::Tri { .. }))
            .count();
        assert!(
            tri_count >= 3,
            "expected polygon + SVG triangles, got {tri_count}"
        );
        assert!(c2d
            .commands()
            .iter()
            .any(|c| matches!(c, Canvas2DCmd::Rect { radius, .. } if *radius >= 19.5)));
        assert!(c2d
            .commands()
            .iter()
            .any(|c| matches!(c, Canvas2DCmd::CustomShader { key: 101, .. })));
        assert!(c2d.commands().iter().any(|c| matches!(
            c,
            Canvas2DCmd::DrawInstanced {
                key: 202,
                stride_floats: 2,
                ..
            }
        )));
    }

    #[test]
    fn scene3d_glass_material_and_custom_3d_shader_snapshot() {
        let mut scene = Scene3D::new();
        let mesh = scene.register_mesh(MeshData {
            vertices: vec![
                super::Vertex3D {
                    pos: [0.0, 0.0, 0.0],
                    normal: [0.0, 0.0, 1.0],
                    uv: [0.0, 0.0],
                },
                super::Vertex3D {
                    pos: [1.0, 0.0, 0.0],
                    normal: [0.0, 0.0, 1.0],
                    uv: [1.0, 0.0],
                },
                super::Vertex3D {
                    pos: [0.0, 1.0, 0.0],
                    normal: [0.0, 0.0, 1.0],
                    uv: [0.0, 1.0],
                },
            ],
            indices: vec![0, 1, 2],
        });
        let node = scene.create_node(None, Transform3D::IDENTITY);
        let glass = super::GlassMaterial::default()
            .tint(Color::rgb(200, 232, 255))
            .transmission(0.95)
            .ior(1.55)
            .dispersion(0.05)
            .thickness(0.8)
            .roughness(0.12);
        scene.draw_mesh_glass(mesh, node, glass);
        scene.draw_mesh_shader(
            mesh,
            node,
            777,
            "void main() { frag_color = vec4(1.0); }",
            &[1.0, 2.0],
        );

        let snap = scene.snapshot();
        assert_eq!(snap.draws.len(), 2);
        assert_eq!(snap.draws[0].glass, Some(glass));
        assert!(snap.draws[0].custom_shader.is_none());
        assert!(snap.draws[1].glass.is_none());
        let cs = snap.draws[1]
            .custom_shader
            .as_ref()
            .expect("custom shader present");
        assert_eq!(cs.key, 777);
        assert_eq!(cs.uniforms, vec![1.0, 2.0]);
    }

    #[test]
    fn svg_icon_even_odd_cutout_leaves_center_hollow() {
        let mut c = super::Canvas2D::new([0.0, 0.0]);
        c.svg_icon_path("M4 4h16v16H4V4zm1.5 1.5v13h13v-13h-13z", 24.0, Color::WHITE);
        let tris: Vec<([f32; 2], [f32; 2], [f32; 2])> = c
            .cmds
            .iter()
            .filter_map(|cmd| match cmd {
                super::Canvas2DCmd::Tri { p0, p1, p2, .. } => Some((*p0, *p1, *p2)),
                _ => None,
            })
            .collect();
        assert!(!tris.is_empty(), "expected triangulated border triangles");
        let tri_covers = |pt: [f32; 2], (a, b, d): ([f32; 2], [f32; 2], [f32; 2])| {
            let (ta, tb, tc) = if super::cross2d(a, b, d) >= 0.0 {
                (a, b, d)
            } else {
                (a, d, b)
            };
            super::point_in_tri(pt, ta, tb, tc)
        };
        assert!(
            tris.iter().any(|&t| tri_covers([4.75, 12.0], t)),
            "outer border frame must be filled"
        );
        assert!(
            !tris.iter().any(|&t| tri_covers([12.0, 12.0], t)),
            "inner cutout center (12, 12) must remain hollow"
        );
    }
}
