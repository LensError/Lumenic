use std::fs;
use std::path::PathBuf;

use crate::gfx::image::encode_png;
use crate::gfx::sdf::{SdfBatch, SdfInstance};
use crate::gfx::text::GlyphAtlas;
use crate::gfx::viewport::{camera_view_proj, mat4_mul, mat4_mul_vec4, SceneSnapshot};
use crate::os::App;
use crate::ui::draw::{Role, SemanticNode};
use crate::ui::types::{Color, Error, Rect};
use crate::ui::UiContext;

/// Configuration for headless `--inspect` PNG + `layout.json` snapshot generation.
///
/// Uses a pure-CPU software rasterizer for deterministic offscreen output, so custom GPU shader
/// passes (`Canvas2D::shader`, `draw_mesh_shader`, backdrop refraction) are skipped and must be
/// verified in a real OpenGL / WebGL 2 window.
#[derive(Clone, Debug)]
pub struct InspectOptions {
    /// Logical viewport width in points.
    pub width: u32,
    /// Logical viewport height in points.
    pub height: u32,
    /// DPI scale factor applied when rasterizing `out_png`.
    pub scale_factor: f32,
    /// Destination path for the offscreen rasterized PNG screenshot.
    pub out_png: PathBuf,
    /// Destination path for the JSON semantic layout tree dump.
    pub out_json: PathBuf,
}

impl Default for InspectOptions {
    fn default() -> Self {
        Self {
            width: 1024,
            height: 700,
            scale_factor: 1.0,
            out_png: PathBuf::from("inspect.png"),
            out_json: PathBuf::from("layout.json"),
        }
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn sd_rounded_box(px: f32, py: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let qx = px.abs() - hx + r;
    let qy = py.abs() - hy + r;
    let mx = qx.max(0.0);
    let my = qy.max(0.0);
    qx.max(qy).min(0.0) + (mx * mx + my * my).sqrt() - r
}

fn sd_hexagon(px: f32, py: f32, r: f32) -> f32 {
    const KX: f32 = -0.866_025_4;
    const KY: f32 = 0.5;
    const KZ: f32 = 0.577_350_26;
    let mut x = px.abs();
    let mut y = py.abs();
    let dot_k = (KX * x + KY * y).min(0.0);
    x -= 2.0 * dot_k * KX;
    y -= 2.0 * dot_k * KY;
    x -= x.clamp(-KZ * r, KZ * r);
    y -= r;
    (x * x + y * y).sqrt() * if y < 0.0 { -1.0 } else { 1.0 }
}

fn erf_approx(x: f32) -> f32 {
    let s = if x < 0.0 {
        -1.0
    } else if x > 0.0 {
        1.0
    } else {
        0.0
    };
    let a = x.abs();
    let t = 1.0 + (0.278_393 + (0.230_389 + 0.078_108 * (a * a)) * a) * a;
    let t2 = t * t;
    s - s / (t2 * t2)
}

fn sd_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let pax = px - ax;
    let pay = py - ay;
    let bax = bx - ax;
    let bay = by - ay;
    let denom = (bax * bax + bay * bay).max(1e-6);
    let h = ((pax * bax + pay * bay) / denom).clamp(0.0, 1.0);
    let dx = pax - bax * h;
    let dy = pay - bay * h;
    (dx * dx + dy * dy).sqrt()
}

fn sd_triangle(px: f32, py: f32, p0: [f32; 2], p1: [f32; 2], p2: [f32; 2]) -> f32 {
    let e0 = [p1[0] - p0[0], p1[1] - p0[1]];
    let e1 = [p2[0] - p1[0], p2[1] - p1[1]];
    let e2 = [p0[0] - p2[0], p0[1] - p2[1]];
    let v0 = [px - p0[0], py - p0[1]];
    let v1 = [px - p1[0], py - p1[1]];
    let v2 = [px - p2[0], py - p2[1]];

    let clamp01 = |v: f32| v.clamp(0.0, 1.0);
    let dot2 = |a: [f32; 2], b: [f32; 2]| a[0] * b[0] + a[1] * b[1];

    let t0 = clamp01(dot2(v0, e0) / dot2(e0, e0).max(1e-6));
    let pq0 = [v0[0] - e0[0] * t0, v0[1] - e0[1] * t0];
    let t1 = clamp01(dot2(v1, e1) / dot2(e1, e1).max(1e-6));
    let pq1 = [v1[0] - e1[0] * t1, v1[1] - e1[1] * t1];
    let t2 = clamp01(dot2(v2, e2) / dot2(e2, e2).max(1e-6));
    let pq2 = [v2[0] - e2[0] * t2, v2[1] - e2[1] * t2];

    let s = (e0[0] * e2[1] - e0[1] * e2[0]).signum();
    let d0 = (dot2(pq0, pq0), s * (v0[0] * e0[1] - v0[1] * e0[0]));
    let d1 = (dot2(pq1, pq1), s * (v1[0] * e1[1] - v1[1] * e1[0]));
    let d2 = (dot2(pq2, pq2), s * (v2[0] * e2[1] - v2[1] * e2[0]));
    let min_x = d0.0.min(d1.0).min(d2.0);
    let min_y = d0.1.min(d1.1).min(d2.1);
    -min_x.sqrt() * min_y.signum()
}

fn grad_factor(px: f32, py: f32, hx: f32, hy: f32, angle_rad: f32, mode: i32) -> f32 {
    if mode == 2 {
        let nx = px / hx.max(1.0);
        let ny = py / hy.max(1.0);
        return (nx * nx + ny * ny).sqrt().clamp(0.0, 1.0);
    }
    let dir_x = angle_rad.cos();
    let dir_y = angle_rad.sin();
    let norm_x = px / (hx * 2.0).max(1.0);
    let norm_y = py / (hy * 2.0).max(1.0);
    (norm_x * dir_x + norm_y * dir_y + 0.5).clamp(0.0, 1.0)
}

fn blend_pixel(buf: &mut [u8], idx: usize, sr: f32, sg: f32, sb: f32, sa: f32) {
    let a = sa.clamp(0.0, 1.0);
    if a <= 0.001 {
        return;
    }
    let inv = 1.0 - a;
    let dr = f32::from(buf[idx]) / 255.0;
    let dg = f32::from(buf[idx + 1]) / 255.0;
    let db = f32::from(buf[idx + 2]) / 255.0;
    let da = f32::from(buf[idx + 3]) / 255.0;

    buf[idx] = ((sr * a + dr * inv).clamp(0.0, 1.0) * 255.0).round() as u8;
    buf[idx + 1] = ((sg * a + dg * inv).clamp(0.0, 1.0) * 255.0).round() as u8;
    buf[idx + 2] = ((sb * a + db * inv).clamp(0.0, 1.0) * 255.0).round() as u8;
    buf[idx + 3] = ((a + da * inv).clamp(0.0, 1.0) * 255.0).round() as u8;
}

fn sample_atlas_bilinear(atlas_px: &[u8], atlas_sz: [u32; 2], u: f32, v: f32) -> [f32; 4] {
    let w = atlas_sz[0].max(1);
    let h = atlas_sz[1].max(1);
    let gx = (u * w as f32 - 0.5).clamp(0.0, (w - 1) as f32);
    let gy = (v * h as f32 - 0.5).clamp(0.0, (h - 1) as f32);
    let x0 = gx.floor() as u32;
    let y0 = gy.floor() as u32;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let fx = gx - x0 as f32;
    let fy = gy - y0 as f32;

    let fetch = |x: u32, y: u32| -> [f32; 4] {
        let idx = ((y as usize) * (w as usize) + (x as usize)) * 4;
        [
            f32::from(atlas_px[idx]) / 255.0,
            f32::from(atlas_px[idx + 1]) / 255.0,
            f32::from(atlas_px[idx + 2]) / 255.0,
            f32::from(atlas_px[idx + 3]) / 255.0,
        ]
    };

    let c00 = fetch(x0, y0);
    let c10 = fetch(x1, y0);
    let c01 = fetch(x0, y1);
    let c11 = fetch(x1, y1);
    let mut out = [0.0_f32; 4];
    for i in 0..4 {
        let top = c00[i] + (c10[i] - c00[i]) * fx;
        let bot = c01[i] + (c11[i] - c01[i]) * fx;
        out[i] = top + (bot - top) * fy;
    }
    out
}

fn rasterize_instance(
    buf: &mut [u8],
    fb_w: u32,
    fb_h: u32,
    clip: Option<[i32; 4]>,
    inst: &SdfInstance,
    atlas: &GlyphAtlas,
) {
    let kind = (inst.radii_and_border[3] + 0.5) as i32;
    let pad = if kind == 3 {
        0.0
    } else if kind == 6 {
        inst.radii_and_border[2] * 1.5 + 4.0
    } else {
        inst.radii_and_border[2] + 2.0
    };

    let (exp_x, exp_y, exp_w, exp_h) = if kind == 9 {
        let mn_x = inst.rect[0].min(inst.rect[2]).min(inst.uv_rect[0]) - 2.0;
        let mn_y = inst.rect[1].min(inst.rect[3]).min(inst.uv_rect[1]) - 2.0;
        let mx_x = inst.rect[0].max(inst.rect[2]).max(inst.uv_rect[0]) + 2.0;
        let mx_y = inst.rect[1].max(inst.rect[3]).max(inst.uv_rect[1]) + 2.0;
        (mn_x, mn_y, mx_x - mn_x, mx_y - mn_y)
    } else {
        let sc = inst.xform_and_grad[0].max(0.001);
        let rot = inst.xform_and_grad[1];
        if kind != 4 && ((sc - 1.0).abs() > 0.001 || rot.abs() > 0.0001) {
            let cx = inst.rect[0] + inst.rect[2] * 0.5;
            let cy = inst.rect[1] + inst.rect[3] * 0.5;
            let rad = ((inst.rect[2] * 0.5).hypot(inst.rect[3] * 0.5)) * sc + pad;
            (cx - rad, cy - rad, rad * 2.0, rad * 2.0)
        } else {
            (
                inst.rect[0] - pad,
                inst.rect[1] - pad,
                inst.rect[2] + pad * 2.0,
                inst.rect[3] + pad * 2.0,
            )
        }
    };

    let min_x = (exp_x.floor() as i32).max(0);
    let min_y = (exp_y.floor() as i32).max(0);
    let max_x = ((exp_x + exp_w).ceil() as i32).min(fb_w as i32);
    let max_y = ((exp_y + exp_h).ceil() as i32).min(fb_h as i32);

    let (cx0, cy0, cx1, cy1) = if let Some([sx, sy_gl, sw, sh]) = clip {
        // Convert GL bottom-left scissor Y back to top-left buffer coordinates.
        let top_y = (fb_h as i32) - (sy_gl + sh);
        (
            min_x.max(sx),
            min_y.max(top_y),
            max_x.min(sx + sw),
            max_y.min(top_y + sh),
        )
    } else {
        (min_x, min_y, max_x, max_y)
    };

    if cx0 >= cx1 || cy0 >= cy1 {
        return;
    }

    let atlas_sz = atlas.atlas_size();
    let atlas_px = atlas.pixels();

    for py in cy0..cy1 {
        for px in cx0..cx1 {
            let fx = px as f32 + 0.5;
            let fy = py as f32 + 0.5;
            let dst_idx = ((py as usize) * (fb_w as usize) + (px as usize)) * 4;

            if kind == 3 {
                let sc = inst.xform_and_grad[0].max(0.001);
                let rot = inst.xform_and_grad[1];
                let (u_frac, v_frac) = if (sc - 1.0).abs() > 0.001 || rot.abs() > 0.0001 {
                    let cx = inst.rect[0] + inst.rect[2] * 0.5;
                    let cy = inst.rect[1] + inst.rect[3] * 0.5;
                    let dp_x = (fx - cx) / sc;
                    let dp_y = (fy - cy) / sc;
                    let (sn, cs) = rot.sin_cos();
                    let lx = dp_x * cs + dp_y * sn + inst.rect[2] * 0.5;
                    let ly = -dp_x * sn + dp_y * cs + inst.rect[3] * 0.5;
                    (lx / inst.rect[2].max(1.0), ly / inst.rect[3].max(1.0))
                } else {
                    (
                        (fx - inst.rect[0]) / inst.rect[2].max(1.0),
                        (fy - inst.rect[1]) / inst.rect[3].max(1.0),
                    )
                };
                if !(0.0..=1.0).contains(&u_frac) || !(0.0..=1.0).contains(&v_frac) {
                    continue;
                }
                let u = inst.uv_rect[0] + (inst.uv_rect[2] - inst.uv_rect[0]) * u_frac;
                let v = inst.uv_rect[1] + (inst.uv_rect[3] - inst.uv_rect[1]) * v_frac;
                let [tr, tg, tb, ta] = sample_atlas_bilinear(atlas_px, atlas_sz, u, v);
                blend_pixel(
                    buf,
                    dst_idx,
                    tr * inst.bg_color[0],
                    tg * inst.bg_color[1],
                    tb * inst.bg_color[2],
                    ta * inst.bg_color[3],
                );
                continue;
            }

            if kind == 4 {
                let d = sd_segment(
                    fx,
                    fy,
                    inst.uv_rect[0],
                    inst.uv_rect[1],
                    inst.uv_rect[2],
                    inst.uv_rect[3],
                ) - inst.radii_and_border[0];
                let a = 1.0 - smoothstep(-0.75, 0.75, d);
                blend_pixel(
                    buf,
                    dst_idx,
                    inst.bg_color[0],
                    inst.bg_color[1],
                    inst.bg_color[2],
                    inst.bg_color[3] * a,
                );
                continue;
            }

            if kind == 9 {
                let d = sd_triangle(
                    fx,
                    fy,
                    [inst.rect[0], inst.rect[1]],
                    [inst.rect[2], inst.rect[3]],
                    [inst.uv_rect[0], inst.uv_rect[1]],
                );
                let a = 1.0 - smoothstep(-0.75, 0.75, d);
                blend_pixel(
                    buf,
                    dst_idx,
                    inst.bg_color[0],
                    inst.bg_color[1],
                    inst.bg_color[2],
                    inst.bg_color[3] * a,
                );
                continue;
            }

            let hx = inst.rect[2] * 0.5;
            let hy = inst.rect[3] * 0.5;
            let cx = inst.rect[0] + hx;
            let cy = inst.rect[1] + hy;
            let sc = inst.xform_and_grad[0].max(0.001);
            let rot = inst.xform_and_grad[1];
            let dp_x = (fx - cx) / sc;
            let dp_y = (fy - cy) / sc;
            let (sn, cs) = rot.sin_cos();
            let px_local = dp_x * cs + dp_y * sn;
            let py_local = -dp_x * sn + dp_y * cs;

            let min_half = hx.min(hy);
            let sub_shape = if kind == 8 || kind == 6 {
                (inst.glow_and_clip[2] + 0.5) as i32
            } else {
                kind
            };
            let has_corners = inst.corner_radii.iter().any(|&c| c > 0.0);
            let corner_r = if has_corners {
                let top = if px_local > 0.0 {
                    inst.corner_radii[1]
                } else {
                    inst.corner_radii[0]
                };
                let bot = if px_local > 0.0 {
                    inst.corner_radii[2]
                } else {
                    inst.corner_radii[3]
                };
                if py_local > 0.0 {
                    bot
                } else {
                    top
                }
            } else {
                inst.radii_and_border[0]
            };
            let r = match sub_shape {
                1 => min_half,
                2 => min_half * 0.44,
                _ => corner_r.clamp(0.0, min_half),
            };
            let d = if sub_shape == 10 {
                let pa_x = px_local.abs();
                let pa_y = py_local.abs();
                let d_box = sd_rounded_box(px_local, py_local, hx, hy, 0.0);
                let d_chamfer = (pa_x + pa_y - (hx + hy - r)) * core::f32::consts::FRAC_1_SQRT_2;
                d_box.max(d_chamfer) * sc
            } else if sub_shape == 11 {
                let hex_r = hy.min(hx * 0.866_025_4);
                sd_hexagon(px_local, py_local, hex_r) * sc
            } else if sub_shape == 12 {
                let hex_r = hx.min(hy * 0.866_025_4);
                sd_hexagon(py_local, px_local, hex_r) * sc
            } else {
                sd_rounded_box(px_local, py_local, hx, hy, r) * sc
            };

            if kind == 6 {
                let blur = inst.radii_and_border[2].max(1.0);
                let gauss = 0.5 * (1.0 - erf_approx(d / (blur * 0.5)));
                let cutoff = 1.0 - smoothstep(blur * 1.25, blur * 1.5, d);
                let sa = gauss * cutoff;
                blend_pixel(
                    buf,
                    dst_idx,
                    inst.bg_color[0],
                    inst.bg_color[1],
                    inst.bg_color[2],
                    inst.bg_color[3] * sa,
                );
                continue;
            }

            let fill_mask = 1.0 - smoothstep(-0.75, 0.75, d);
            let gflag = (inst.xform_and_grad[3] + 0.5) as i32;
            let mut fill = inst.bg_color;
            if gflag == 1 || gflag == 2 {
                let t = grad_factor(px_local, py_local, hx, hy, inst.xform_and_grad[2], gflag);
                for (f, g) in fill.iter_mut().zip(inst.grad_color) {
                    *f += (g - *f) * t;
                }
            }
            let alpha_mul = if kind == 5 {
                fill[3].clamp(0.25, 0.85)
            } else {
                fill[3]
            };
            blend_pixel(
                buf,
                dst_idx,
                fill[0],
                fill[1],
                fill[2],
                alpha_mul * fill_mask,
            );

            let bw = inst.radii_and_border[1];
            if bw > 0.0 {
                let outer_a = 1.0 - smoothstep(-0.75, 0.75, d);
                let inner_a = smoothstep(-bw - 0.75, -bw + 0.75, d);
                let b_mask = (outer_a * inner_a).clamp(0.0, 1.0);
                let mut bd = inst.border_color;
                if gflag == 4 {
                    let t = grad_factor(px_local, py_local, hx, hy, inst.xform_and_grad[2], 1);
                    for (b, g) in bd.iter_mut().zip(inst.grad_color) {
                        *b += (g - *b) * t;
                    }
                }
                blend_pixel(buf, dst_idx, bd[0], bd[1], bd[2], bd[3] * b_mask);
            }
        }
    }
}

fn project_3d_point(mvp: [[f32; 4]; 4], vp_rect: Rect, p: [f32; 3]) -> Option<[f32; 3]> {
    let clip = mat4_mul_vec4(mvp, [p[0], p[1], p[2], 1.0]);
    if clip[3] <= 1e-4 {
        return None;
    }
    let ndc_x = clip[0] / clip[3];
    let ndc_y = clip[1] / clip[3];
    let ndc_z = clip[2] / clip[3];
    let sx = vp_rect.x + (ndc_x * 0.5 + 0.5) * vp_rect.w;
    let sy = vp_rect.y + (1.0 - (ndc_y * 0.5 + 0.5)) * vp_rect.h;
    if sx.is_finite() && sy.is_finite() && ndc_z.is_finite() {
        Some([sx, sy, ndc_z])
    } else {
        None
    }
}

fn vec3_norm(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 1e-6 {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        [0.0, 1.0, 0.0]
    }
}

fn vec3_dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn transform_normal(model: [[f32; 4]; 4], n: [f32; 3]) -> [f32; 3] {
    vec3_norm([
        model[0][0] * n[0] + model[1][0] * n[1] + model[2][0] * n[2],
        model[0][1] * n[0] + model[1][1] * n[1] + model[2][1] * n[2],
        model[0][2] * n[0] + model[1][2] * n[1] + model[2][2] * n[2],
    ])
}

fn shade_mesh3d_fragment(n: [f32; 3], base: [f32; 4], roughness: f32) -> [f32; 4] {
    let n = vec3_norm(n);
    let hemi = n[1] * 0.5 + 0.5;
    let amb = [
        0.22 + (0.55 - 0.22) * hemi,
        0.23 + (0.58 - 0.23) * hemi,
        0.29 + (0.68 - 0.29) * hemi,
    ];
    let key_dir = vec3_norm([0.55, 0.85, 0.65]);
    let diff = vec3_dot(n, key_dir).max(0.0);
    let rim_dir = vec3_norm([-0.65, 0.25, -0.55]);
    let rim = vec3_dot(n, rim_dir).max(0.0) * 0.35;
    let spec_pow = 36.0 + (6.0 - 36.0) * roughness.clamp(0.0, 1.0);
    let half_v = vec3_norm([key_dir[0], key_dir[1], key_dir[2] + 1.0]);
    let spec = vec3_dot(n, half_v).max(0.0).powf(spec_pow) * 0.28;
    let light = 0.68 * diff + rim;
    [
        (base[0] * (amb[0] + light) + spec).clamp(0.0, 1.0),
        (base[1] * (amb[1] + light) + spec).clamp(0.0, 1.0),
        (base[2] * (amb[2] + light) + spec).clamp(0.0, 1.0),
        base[3],
    ]
}

fn rasterize_scene3d_solid(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    scissor: Option<[i32; 4]>,
    vp_rect: Rect,
    scene: &SceneSnapshot,
) {
    if vp_rect.w <= 0.0 || vp_rect.h <= 0.0 || scene.draws.is_empty() {
        return;
    }
    let mut min_x = (vp_rect.x.floor() as i32).max(0);
    let mut min_y = (vp_rect.y.floor() as i32).max(0);
    let mut max_x = ((vp_rect.x + vp_rect.w).ceil() as i32).min(width as i32);
    let mut max_y = ((vp_rect.y + vp_rect.h).ceil() as i32).min(height as i32);
    if let Some([sx, sy, sw, sh]) = scissor {
        let s_top = (height as i32).saturating_sub(sy + sh);
        let s_bot = (height as i32).saturating_sub(sy);
        min_x = min_x.max(sx);
        min_y = min_y.max(s_top);
        max_x = max_x.min(sx + sw);
        max_y = max_y.min(s_bot);
    }
    if min_x >= max_x || min_y >= max_y {
        return;
    }
    let buf_w = (max_x - min_x) as usize;
    let buf_h = (max_y - min_y) as usize;
    let use_msaa = scene.msaa_samples > 1;
    let sample_count = if use_msaa { 4usize } else { 1usize };
    let mut zbuf = vec![f32::INFINITY; buf_w.saturating_mul(buf_h).saturating_mul(sample_count)];
    let mut cbuf = if use_msaa {
        vec![[0.0_f32; 4]; buf_w.saturating_mul(buf_h).saturating_mul(4)]
    } else {
        Vec::new()
    };
    const MSAA_OFFSETS: [[f32; 2]; 4] = [
        [0.375, 0.125],
        [0.875, 0.375],
        [0.125, 0.625],
        [0.625, 0.875],
    ];
    const NO_MSAA_OFFSETS: [[f32; 2]; 1] = [[0.5, 0.5]];
    let offsets: &[[f32; 2]] = if use_msaa {
        &MSAA_OFFSETS
    } else {
        &NO_MSAA_OFFSETS
    };

    let aspect = (vp_rect.w / vp_rect.h.max(1.0)).max(0.1);
    let vp_mat = camera_view_proj(&scene.camera, aspect);

    let mut has_translucent = false;
    for opaque_pass in [true, false] {
        if !opaque_pass && !has_translucent {
            break;
        }
        for draw in &scene.draws {
            let is_glass = draw.glass.is_some();
            let is_opaque = !is_glass && draw.material.base_color.a == 255;
            if opaque_pass && !is_opaque {
                if is_glass || draw.material.base_color.a > 0 {
                    has_translucent = true;
                }
                continue;
            }
            if !opaque_pass && (is_opaque || (!is_glass && draw.material.base_color.a == 0)) {
                continue;
            }

            let mvp = mat4_mul(vp_mat, draw.model_mat);
            let col = draw.material.base_color.to_normalized_f32();
            let rough = draw.material.roughness;
            let glass_opt = draw.glass;

            for tri in draw.mesh.indices.as_chunks::<3>().0 {
                let (Some(v0), Some(v1), Some(v2)) = (
                    draw.mesh.vertices.get(tri[0] as usize),
                    draw.mesh.vertices.get(tri[1] as usize),
                    draw.mesh.vertices.get(tri[2] as usize),
                ) else {
                    continue;
                };
                let (Some(p0), Some(p1), Some(p2)) = (
                    project_3d_point(mvp, vp_rect, v0.pos),
                    project_3d_point(mvp, vp_rect, v1.pos),
                    project_3d_point(mvp, vp_rect, v2.pos),
                ) else {
                    continue;
                };
                // Screen space has Y-down, so CCW triangles in NDC have negative signed area.
                let signed_area =
                    (p1[0] - p0[0]) * (p2[1] - p0[1]) - (p1[1] - p0[1]) * (p2[0] - p0[0]);
                if signed_area >= -1e-4 {
                    continue;
                }
                let inv_area = 1.0 / signed_area;

                let n0 = transform_normal(draw.model_mat, v0.normal);
                let n1 = transform_normal(draw.model_mat, v1.normal);
                let n2 = transform_normal(draw.model_mat, v2.normal);

                let tri_min_x = (p0[0].min(p1[0]).min(p2[0]).floor() as i32).max(min_x);
                let tri_max_x = (p0[0].max(p1[0]).max(p2[0]).ceil() as i32).min(max_x - 1);
                let tri_min_y = (p0[1].min(p1[1]).min(p2[1]).floor() as i32).max(min_y);
                let tri_max_y = (p0[1].max(p1[1]).max(p2[1]).ceil() as i32).min(max_y - 1);

                for py in tri_min_y..=tri_max_y {
                    for px in tri_min_x..=tri_max_x {
                        let pix_idx = (py - min_y) as usize * buf_w + (px - min_x) as usize;
                        let mut cached_shaded: Option<[f32; 4]> = None;
                        for (s_i, &[ox, oy]) in offsets.iter().enumerate() {
                            let cx = px as f32 + ox;
                            let cy = py as f32 + oy;
                            let w0 = ((p2[0] - p1[0]) * (cy - p1[1])
                                - (p2[1] - p1[1]) * (cx - p1[0]))
                                * inv_area;
                            let w1 = ((p0[0] - p2[0]) * (cy - p2[1])
                                - (p0[1] - p2[1]) * (cx - p2[0]))
                                * inv_area;
                            let w2 = 1.0 - w0 - w1;
                            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                                continue;
                            }
                            let z = w0 * p0[2] + w1 * p1[2] + w2 * p2[2];
                            if z < -1.0 || z > 1.0 {
                                continue;
                            }
                            let z_idx = pix_idx * sample_count + s_i;
                            if z >= zbuf[z_idx] {
                                continue;
                            }
                            if is_opaque {
                                zbuf[z_idx] = z;
                            }
                            let shaded = *cached_shaded.get_or_insert_with(|| {
                                let n = [
                                    w0 * n0[0] + w1 * n1[0] + w2 * n2[0],
                                    w0 * n0[1] + w1 * n1[1] + w2 * n2[1],
                                    w0 * n0[2] + w1 * n1[2] + w2 * n2[2],
                                ];
                                if let Some(g) = glass_opt {
                                    let nl =
                                        (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
                                    let nn = [n[0] / nl, n[1] / nl, n[2] / nl];
                                    let ndotv = nn[2].abs().clamp(0.0, 1.0);
                                    let f0 = ((g.ior - 1.0) / (g.ior + 1.0)).powi(2);
                                    let fresnel = f0 + (1.0 - f0) * (1.0 - ndotv).powi(5);
                                    let base = shade_mesh3d_fragment(nn, col, rough);
                                    let rim = (1.0 - ndotv).powi(3) * 0.45;
                                    let out_a = ((1.0 - g.transmission * 0.55) + fresnel * 0.65)
                                        .clamp(0.18, 0.96);
                                    [
                                        (base[0] * 0.85 + rim).clamp(0.0, 1.0),
                                        (base[1] * 0.92 + rim * 1.1).clamp(0.0, 1.0),
                                        (base[2] + rim * 1.25).clamp(0.0, 1.0),
                                        out_a,
                                    ]
                                } else {
                                    shade_mesh3d_fragment(n, col, rough)
                                }
                            });
                            if use_msaa {
                                let dst = &mut cbuf[z_idx];
                                let sa = shaded[3];
                                let inv_a = 1.0 - sa;
                                dst[0] = shaded[0] * sa + dst[0] * inv_a;
                                dst[1] = shaded[1] * sa + dst[1] * inv_a;
                                dst[2] = shaded[2] * sa + dst[2] * inv_a;
                                dst[3] = sa + dst[3] * inv_a;
                            } else {
                                let dst_idx = (py as usize * width as usize + px as usize) * 4;
                                blend_pixel(
                                    rgba, dst_idx, shaded[0], shaded[1], shaded[2], shaded[3],
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    if use_msaa {
        for py in min_y..max_y {
            for px in min_x..max_x {
                let base = ((py - min_y) as usize * buf_w + (px - min_x) as usize) * 4;
                let mut sum = [0.0_f32; 4];
                for s in 0..4 {
                    let c = cbuf[base + s];
                    sum[0] += c[0];
                    sum[1] += c[1];
                    sum[2] += c[2];
                    sum[3] += c[3];
                }
                let a = (sum[3] * 0.25).clamp(0.0, 1.0);
                if a > 0.0 {
                    let dst_idx = (py as usize * width as usize + px as usize) * 4;
                    blend_pixel(
                        rgba,
                        dst_idx,
                        (sum[0] * 0.25) / a,
                        (sum[1] * 0.25) / a,
                        (sum[2] * 0.25) / a,
                        a,
                    );
                }
            }
        }
    }
}

/// Deterministically rasterizes an `SdfBatch` on CPU into an RGBA8 pixel buffer.
pub fn rasterize_sdf_batch_cpu(
    width: u32,
    height: u32,
    clear_color: Color,
    batch: &SdfBatch,
    atlas: &GlyphAtlas,
) -> Result<Vec<u8>, Error> {
    let pixel_count = usize::try_from(width)
        .ok()
        .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
        .ok_or_else(|| Error::InvalidInput(String::from("inspect dimension overflow")))?;
    let byte_len = pixel_count
        .checked_mul(4)
        .ok_or_else(|| Error::InvalidInput(String::from("inspect buffer overflow")))?;

    let mut rgba = vec![0u8; byte_len];
    for px in rgba.as_chunks_mut::<4>().0 {
        px.copy_from_slice(&[clear_color.r, clear_color.g, clear_color.b, clear_color.a]);
    }

    for (run_idx, run) in batch.runs.iter().enumerate() {
        for vp in &batch.viewport_3d {
            if vp.run_index == run_idx {
                rasterize_scene3d_solid(&mut rgba, width, height, run.scissor, vp.rect, &vp.scene);
            }
        }
        let end = run
            .start
            .saturating_add(run.count)
            .min(batch.instances.len());
        for inst in &batch.instances[run.start..end] {
            rasterize_instance(&mut rgba, width, height, run.scissor, inst, atlas);
        }
    }

    Ok(rgba)
}

fn push_json_escaped(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                use core::fmt::Write;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Window => "Window",
        Role::Container => "Container",
        Role::Button => "Button",
        Role::Toggle => "Toggle",
        Role::Slider => "Slider",
        Role::TextInput => "TextInput",
        Role::Dropdown => "Dropdown",
        Role::Modal => "Modal",
        Role::SplitDivider => "SplitDivider",
        Role::Divider => "Divider",
        Role::Label => "Label",
        Role::Image => "Image",
        Role::Viewport => "Viewport",
    }
}

/// Serializes viewport metadata and semantic layout nodes into compact JSON without `serde`.
pub fn serialize_layout_json(
    width: u32,
    height: u32,
    scale_factor: f32,
    semantics: &[SemanticNode],
) -> String {
    use core::fmt::Write;
    let mut out = String::with_capacity(256 + semantics.len() * 128);
    let _ = write!(
        out,
        "{{\"width\":{width},\"height\":{height},\"scale_factor\":{scale_factor:.4},\"nodes\":["
    );
    for (i, n) in semantics.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(out, "{{\"id\":\"0x{:016x}\",\"role\":", n.id.0);
        push_json_escaped(&mut out, role_name(n.role));
        out.push_str(",\"label\":");
        push_json_escaped(&mut out, &n.label);
        let _ = write!(
            out,
            ",\"bounds\":{{\"x\":{:.2},\"y\":{:.2},\"w\":{:.2},\"h\":{:.2}}},\"focused\":{},\"enabled\":{},\"focusable\":{},\"is_overlay\":{}}}",
            n.bounds.x,
            n.bounds.y,
            n.bounds.w,
            n.bounds.h,
            n.focused,
            n.enabled,
            n.focusable,
            n.is_overlay
        );
    }
    out.push_str("]}\n");
    out
}

/// Runs one headless UI frame, rasterizes PNG + `layout.json`, and writes them to disk.
pub fn inspect_to_files<F>(options: &InspectOptions, mut update: F) -> Result<(), Error>
where
    F: FnMut(&mut UiContext, &mut App),
{
    if options.width == 0 || options.height == 0 {
        return Err(Error::InvalidInput(String::from(
            "inspect dimensions must be non-zero",
        )));
    }
    let scale = if options.scale_factor.is_finite() && options.scale_factor > 0.0 {
        options.scale_factor
    } else {
        1.0
    };
    let logical_w = options.width as f32 / scale;
    let logical_h = options.height as f32 / scale;

    let mut ui = UiContext::new();
    let mut app = App::default();
    let mut atlas = GlyphAtlas::new_with_system_fonts();
    let mut batch = SdfBatch::default();

    ui.begin_frame(logical_w, logical_h, scale);
    update(&mut ui, &mut app);
    ui.end_frame_with(&mut atlas);

    // Run a second pass so `prev_rects`-backed widgets (`Canvas2D::size()`, `vlist_dynamic`)
    // and newly created `App` texture dimensions resolve in single-shot headless snapshots.
    ui.set_texture_sizes(app.texture_sizes());
    ui.begin_frame(logical_w, logical_h, scale);
    update(&mut ui, &mut app);
    ui.end_frame_with(&mut atlas);

    batch.build(
        ui.draw_list(),
        &mut atlas,
        scale,
        ui.pointer_pos(),
        options.width,
        options.height,
    );

    let rgba = rasterize_sdf_batch_cpu(
        options.width,
        options.height,
        ui.theme().background,
        &batch,
        &atlas,
    )?;
    let png_bytes = encode_png(options.width, options.height, &rgba)?;
    let json = serialize_layout_json(options.width, options.height, scale, ui.semantics());

    fs::write(&options.out_png, &png_bytes).map_err(|e| {
        Error::Io(format!(
            "failed to write {}: {e}",
            options.out_png.display()
        ))
    })?;
    fs::write(&options.out_json, json.as_bytes()).map_err(|e| {
        Error::Io(format!(
            "failed to write {}: {e}",
            options.out_json.display()
        ))
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{inspect_to_files, serialize_layout_json, InspectOptions};
    use crate::gfx::image::decode_png;
    use crate::ui::types::{Color, Span};

    #[test]
    fn inspect_produces_valid_png_and_layout_json() {
        let dir = std::env::temp_dir().join("lumenic_inspect_test");
        let _ = std::fs::create_dir_all(&dir);
        let opts = InspectOptions {
            width: 320,
            height: 200,
            scale_factor: 1.25,
            out_png: dir.join("test_inspect.png"),
            out_json: dir.join("test_layout.json"),
        };

        inspect_to_files(&opts, |ui, _app| {
            ui.col(|ui| {
                ui.txt("Inspect Test").sz(16.0);
                ui.rich(&[
                    Span::text("Hi "),
                    Span::mention("@user"),
                    Span::text(" "),
                    Span::code("ok"),
                ]);
                let _ = ui.btn("Action").r(8.0);
            })
            .p(12.0)
            .gap(8.0)
            .bg(Color::from_hex(0x282834))
            .r(10.0);
        })
        .expect("inspect succeeds");

        let png_bytes = std::fs::read(&opts.out_png).expect("png exists");
        let img = decode_png(&png_bytes).expect("valid png");
        assert_eq!(img.width, 320);
        assert_eq!(img.height, 200);
        assert!(img
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .any(|px| px[0] != 0 || px[1] != 0));

        let json = std::fs::read_to_string(&opts.out_json).expect("json exists");
        assert!(json.contains("\"width\":320"));
        assert!(json.contains("\"scale_factor\":1.2500"));
        assert!(json.contains("\"Inspect Test\""));
        assert!(json.contains("\"Action\""));

        let escaped = serialize_layout_json(100, 100, 1.0, &[]);
        assert!(escaped.contains("\"nodes\":[]"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn inspect_rasterizes_solid_and_translucent_scene3d() {
        use crate::gfx::viewport::{Material, MeshData, Transform3D, Vertex3D};
        let dir = std::env::temp_dir().join("lumenic_inspect_3d_test");
        let _ = std::fs::create_dir_all(&dir);
        let opts = InspectOptions {
            width: 160,
            height: 120,
            scale_factor: 1.0,
            out_png: dir.join("test_3d.png"),
            out_json: dir.join("test_3d.json"),
        };

        inspect_to_files(&opts, |ui, _app| {
            ui.viewport("vp3d", |_c2d, s3d| {
                s3d.set_camera(crate::ui::types::Camera3D {
                    position: [0.0, 0.0, 3.0],
                    target: [0.0, 0.0, 0.0],
                    up: [0.0, 1.0, 0.0],
                    fov_deg: 60.0,
                    near: 0.1,
                    far: 50.0,
                });
                let tri = s3d.register_mesh(MeshData {
                    vertices: vec![
                        Vertex3D {
                            pos: [-1.0, -1.0, 0.0],
                            normal: [0.0, 0.0, 1.0],
                            uv: [0.0, 0.0],
                        },
                        Vertex3D {
                            pos: [1.0, -1.0, 0.0],
                            normal: [0.0, 0.0, 1.0],
                            uv: [1.0, 0.0],
                        },
                        Vertex3D {
                            pos: [0.0, 1.0, 0.0],
                            normal: [0.0, 0.0, 1.0],
                            uv: [0.5, 1.0],
                        },
                    ],
                    indices: vec![0, 1, 2],
                });
                let n = s3d.create_node(None, Transform3D::default());
                s3d.draw_mesh(
                    tri,
                    n,
                    Material {
                        base_color: Color::rgba(220, 60, 60, 255),
                        ..Material::default()
                    },
                );
            })
            .w(160.0)
            .h(120.0);
        })
        .expect("inspect 3d succeeds");

        let png_bytes = std::fs::read(&opts.out_png).expect("png exists");
        let img = decode_png(&png_bytes).expect("valid png");
        let center_idx = (60 * 160 + 80) * 4;
        // Center pixel must be shaded reddish (solid fill, not background).
        assert!(img.data[center_idx] > img.data[center_idx + 2] + 30);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scene3d_msaa_smooths_edges_and_supports_override() {
        use crate::gfx::viewport::{Material, MeshData, Scene3D, Transform3D, Vertex3D};

        let mut s = Scene3D::new();
        assert_eq!(s.msaa(), 4);
        s.set_msaa(0);
        assert_eq!(s.msaa(), 1);
        s.set_msaa(8);
        assert_eq!(s.msaa(), 8);

        let dir = std::env::temp_dir().join("lumenic_inspect_msaa_test");
        let _ = std::fs::create_dir_all(&dir);
        let opts = InspectOptions {
            width: 100,
            height: 100,
            scale_factor: 1.0,
            out_png: dir.join("msaa.png"),
            out_json: dir.join("msaa.json"),
        };
        inspect_to_files(&opts, |ui, _app| {
            ui.viewport("msaa_vp", |_c2d, s3d| {
                assert_eq!(s3d.msaa(), 4);
                let tri = MeshData {
                    vertices: vec![
                        Vertex3D {
                            pos: [-0.8, -0.7, 0.0],
                            normal: [0.0, 0.0, 1.0],
                            uv: [0.0, 0.0],
                        },
                        Vertex3D {
                            pos: [0.8, -0.3, 0.0],
                            normal: [0.0, 0.0, 1.0],
                            uv: [1.0, 0.0],
                        },
                        Vertex3D {
                            pos: [-0.2, 0.8, 0.0],
                            normal: [0.0, 0.0, 1.0],
                            uv: [0.0, 1.0],
                        },
                    ],
                    indices: vec![0, 1, 2],
                };
                let mesh = s3d.register_mesh(tri);
                let node = s3d.create_node(None, Transform3D::default());
                s3d.draw_mesh(
                    mesh,
                    node,
                    Material {
                        base_color: Color::WHITE,
                        ..Material::default()
                    },
                );
            })
            .w(100.0)
            .h(100.0);
        })
        .expect("inspect succeeds");

        let img = decode_png(&std::fs::read(&opts.out_png).unwrap()).unwrap();
        let bg = img.data[0];
        let mut has_intermediate_edge = false;
        for px in img.data.as_chunks::<4>().0 {
            if px[0] > bg + 10 && px[0] < 180 {
                has_intermediate_edge = true;
                break;
            }
        }
        assert!(has_intermediate_edge, "expected anti-aliased edge pixels");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn inspect_rasterizes_triangles_and_gradients() {
        let dir = std::env::temp_dir().join("lumenic_inspect_tri_grad_test");
        let _ = std::fs::create_dir_all(&dir);
        let opts = InspectOptions {
            width: 120,
            height: 80,
            scale_factor: 1.0,
            out_png: dir.join("tri_grad.png"),
            out_json: dir.join("tri_grad.json"),
        };

        inspect_to_files(&opts, |ui, _app| {
            ui.col(|ui| {
                ui.txt("Grad").w(100.0).h(30.0).bg_grad(
                    Color::from_hex(0xEF4444),
                    Color::from_hex(0x3B82F6),
                    0.0,
                );
                ui.viewport("tri_vp", |c2d, _s3d| {
                    c2d.fill_poly(
                        &[[10.0, 5.0], [90.0, 5.0], [50.0, 35.0]],
                        Color::from_hex(0x22C55E),
                    );
                })
                .w(100.0)
                .h(40.0);
            });
        })
        .expect("inspect tri+grad succeeds");

        let png_bytes = std::fs::read(&opts.out_png).expect("png exists");
        let img = decode_png(&png_bytes).expect("valid png");
        let left_px = (15 * 120 + 10) * 4;
        let right_px = (15 * 120 + 90) * 4;
        assert!(img.data[left_px] > img.data[right_px] + 40);
        let tri_px = (45 * 120 + 50) * 4;
        assert!(img.data[tri_px + 1] > img.data[tri_px] + 40);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bevel_and_four_corner_radii_rasterize_cleanly() {
        let dir = std::env::temp_dir().join("lumenic_inspect_bevel_corners_test");
        let _ = std::fs::create_dir_all(&dir);
        let opts = InspectOptions {
            width: 140,
            height: 60,
            scale_factor: 1.0,
            out_png: dir.join("bevel.png"),
            out_json: dir.join("bevel.json"),
        };

        inspect_to_files(&opts, |ui, _app| {
            ui.row(|ui| {
                ui.btn("Bevel")
                    .w(50.0)
                    .h(40.0)
                    .bg(Color::from_hex(0xEF4444))
                    .bevel(12.0);
                ui.txt("Mix")
                    .w(50.0)
                    .h(40.0)
                    .bg(Color::from_hex(0x3B82F6))
                    .r_corners(14.0, 0.0, 14.0, 0.0);
            })
            .p(8.0)
            .gap(10.0);
        })
        .expect("inspect bevel+corners succeeds");

        let png_bytes = std::fs::read(&opts.out_png).expect("png exists");
        let img = decode_png(&png_bytes).expect("valid png");
        // Beveled button outer top-left corner at (8, 8) should be cut (not bright red).
        let bevel_tl = (8 * 140 + 8) * 4;
        assert!(img.data[bevel_tl] < 80);
        // Beveled button center at (33, 28) should be red.
        let bevel_mid = (28 * 140 + 33) * 4;
        assert!(img.data[bevel_mid] > 180);
        // Mixed-corner box sharp top-right at (68 + 49, 8) should be blue; rounded top-left at (68, 8) should be cut.
        let mix_tr = (8 * 140 + 116) * 4;
        let mix_tl = (8 * 140 + 68) * 4;
        assert!(img.data[mix_tr + 2] > 180);
        assert!(img.data[mix_tl + 2] < 80);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hexagon_and_pointy_hexagon_rasterize_cleanly() {
        let dir = std::env::temp_dir().join("lumenic_inspect_hexagon_test");
        let _ = std::fs::create_dir_all(&dir);
        let opts = InspectOptions {
            width: 160,
            height: 70,
            scale_factor: 1.0,
            out_png: dir.join("hex.png"),
            out_json: dir.join("hex.json"),
        };

        inspect_to_files(&opts, |ui, _app| {
            ui.row(|ui| {
                ui.col(|_| {})
                    .w(48.0)
                    .h(48.0)
                    .bg(Color::from_hex(0xEF4444))
                    .bd(2.0, Color::WHITE)
                    .hexagon();
                ui.col(|_| {})
                    .w(48.0)
                    .h(48.0)
                    .bg(Color::from_hex(0x3B82F6))
                    .bd(2.0, Color::WHITE)
                    .hexagon_pointy();
            })
            .p(10.0)
            .gap(16.0);
        })
        .expect("inspect hexagon succeeds");

        let png_bytes = std::fs::read(&opts.out_png).expect("png exists");
        let img = decode_png(&png_bytes).expect("valid png");

        // Flat-top hexagon occupies [10..58, 10..58], center at (34, 34).
        let flat_center = (34 * 160 + 34) * 4;
        let flat_tl_corner = (10 * 160 + 10) * 4;
        // Top-most pixel (34, 11) is outside flat-top hexagon (inradius = 24 * 0.866 = 20.78 -> top edge at y = 13.2).
        let flat_top_outside = (11 * 160 + 34) * 4;
        assert!(img.data[flat_center] > 180);
        assert!(img.data[flat_tl_corner] < 80);
        assert!(img.data[flat_top_outside] < 80);

        // Pointy-top hexagon occupies [74..122, 10..58], center at (98, 34).
        let pointy_center = (34 * 160 + 98) * 4;
        let pointy_tl_corner = (10 * 160 + 74) * 4;
        // Left-most pixel (75, 34) is outside pointy-top hexagon (inradius = 20.78 -> left edge at x = 77.2).
        let pointy_left_outside = (34 * 160 + 75) * 4;
        assert!(img.data[pointy_center + 2] > 180);
        assert!(img.data[pointy_tl_corner + 2] < 80);
        assert!(img.data[pointy_left_outside + 2] < 80);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shadow_falloff_reaches_zero_smoothly_without_hard_edge_seam() {
        let dir = std::env::temp_dir().join("lumenic_inspect_shadow_falloff_test");
        let _ = std::fs::create_dir_all(&dir);
        let opts = InspectOptions {
            width: 200,
            height: 120,
            scale_factor: 1.0,
            out_png: dir.join("shadow.png"),
            out_json: dir.join("shadow.json"),
        };

        inspect_to_files(&opts, |ui, _app| {
            let mut theme = *ui.theme();
            theme.background = Color::WHITE;
            ui.set_theme(theme);
            ui.col(|ui| {
                ui.col(|_| {})
                    .w(100.0)
                    .h(40.0)
                    .bg(Color::rgb(20, 24, 34))
                    .bevel(10.0)
                    .shadow(14.0, Color::rgba(0, 0, 0, 180));
            })
            .p(40.0);
        })
        .expect("inspect shadow succeeds");

        let png_bytes = std::fs::read(&opts.out_png).expect("png exists");
        let img = decode_png(&png_bytes).expect("valid png");

        // Card is at x=[40..140], y=[40..80]; sample horizontal midline y=60 moving outward from left/right edges.
        let sample_r = |x: usize| img.data[(60 * 200 + x) * 4];
        // Right outside left edge (x=39) should be darkened by shadow.
        assert!(sample_r(39) < 220);
        // Moving outward on the left (39 -> 15), brightness must increase monotonically and reach 255 before quad edge.
        let mut prev_l = sample_r(39);
        for x in (15..39).rev() {
            let cur = sample_r(x);
            assert!(
                cur >= prev_l,
                "left shadow must fade monotonically at x={x}: {cur} < {prev_l}"
            );
            assert!(
                cur.saturating_sub(prev_l) <= 45,
                "left shadow must not have a hard step seam at x={x}: {prev_l} -> {cur}"
            );
            prev_l = cur;
        }
        assert_eq!(sample_r(17), 255);

        // Moving outward on the right (140 -> 165), brightness must increase monotonically and reach 255 before quad edge.
        let mut prev_r = sample_r(140);
        for x in 141..=165 {
            let cur = sample_r(x);
            assert!(
                cur >= prev_r,
                "right shadow must fade monotonically at x={x}: {cur} < {prev_r}"
            );
            assert!(
                cur.saturating_sub(prev_r) <= 45,
                "right shadow must not have a hard step seam at x={x}: {prev_r} -> {cur}"
            );
            prev_r = cur;
        }
        assert_eq!(sample_r(163), 255);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
