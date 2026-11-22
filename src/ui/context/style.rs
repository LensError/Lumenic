use super::*;

macro_rules! impl_style_methods {
    ($ty:ident) => {
        impl $ty<'_> {
            /// Sets uniform padding (`left`, `right`, `top`, `bottom`) in logical points.
            pub fn p(self, v: f32) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                s.pad_l = v;
                s.pad_r = v;
                s.pad_t = v;
                s.pad_b = v;
                self
            }

            /// Sets horizontal padding (`left` and `right`) in logical points.
            pub fn px(self, v: f32) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                s.pad_l = v;
                s.pad_r = v;
                self
            }

            /// Sets vertical padding (`top` and `bottom`) in logical points.
            pub fn py(self, v: f32) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                s.pad_t = v;
                s.pad_b = v;
                self
            }

            /// Sets top padding in logical points.
            pub fn pt(self, v: f32) -> Self {
                self.ui.nodes[self.node_idx].style.pad_t = v;
                self
            }

            /// Sets bottom padding in logical points.
            pub fn pb(self, v: f32) -> Self {
                self.ui.nodes[self.node_idx].style.pad_b = v;
                self
            }

            /// Sets left padding in logical points.
            pub fn pl(self, v: f32) -> Self {
                self.ui.nodes[self.node_idx].style.pad_l = v;
                self
            }

            /// Sets right padding in logical points.
            pub fn pr(self, v: f32) -> Self {
                self.ui.nodes[self.node_idx].style.pad_r = v;
                self
            }

            /// Sets spacing between child nodes along the container's main axis.
            pub fn gap(self, v: f32) -> Self {
                self.ui.nodes[self.node_idx].style.gap = v;
                self
            }

            /// Sets an explicit width in logical points.
            pub fn w(self, v: f32) -> Self {
                self.ui.nodes[self.node_idx].style.width = Some(v);
                self
            }

            /// Sets an explicit height in logical points.
            pub fn h(self, v: f32) -> Self {
                self.ui.nodes[self.node_idx].style.height = Some(v);
                self
            }

            /// Sets the flex grow weight for distributing remaining parent main-axis space.
            pub fn grow(self, factor: f32) -> Self {
                self.ui.nodes[self.node_idx].style.grow = factor.max(0.0);
                self
            }

            /// Sets the background fill color.
            pub fn bg(self, color: Color) -> Self {
                self.ui.nodes[self.node_idx].style.bg = Some(color);
                self
            }

            /// Sets the foreground text or sparkline stroke color.
            pub fn fg(self, color: Color) -> Self {
                self.ui.nodes[self.node_idx].style.fg = Some(color);
                for i in 0..self.ui.nodes[self.node_idx].children.len() {
                    let cid = self.ui.nodes[self.node_idx].children[i];
                    if self.ui.nodes[cid].role == Role::Label {
                        self.ui.nodes[cid].style.fg = Some(color);
                    }
                }
                self
            }

            /// Sets the border stroke width (in logical points) and color.
            pub fn bd(self, width: f32, color: Color) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                s.border_width = width;
                s.border_color = color;
                self
            }

            /// Sets the corner radius in logical points.
            pub fn r(self, radius: f32) -> Self {
                self.ui.nodes[self.node_idx].style.radius = radius;
                self
            }

            /// Sets the font size in logical points.
            pub fn sz(self, pt: f32) -> Self {
                self.ui.nodes[self.node_idx].style.font_size = Some(pt);
                for i in 0..self.ui.nodes[self.node_idx].children.len() {
                    let cid = self.ui.nodes[self.node_idx].children[i];
                    if self.ui.nodes[cid].role == Role::Label {
                        self.ui.nodes[cid].style.font_size = Some(pt);
                    }
                }
                self
            }

            /// Sets the CSS font weight (`100..=900`, e.g. `400` regular, `600` semibold, `700` bold).
            pub fn weight(self, weight: u16) -> Self {
                let w = weight.clamp(100, 900);
                self.ui.nodes[self.node_idx].style.font_weight = Some(w);
                for i in 0..self.ui.nodes[self.node_idx].children.len() {
                    let cid = self.ui.nodes[self.node_idx].children[i];
                    if self.ui.nodes[cid].role == Role::Label {
                        self.ui.nodes[cid].style.font_weight = Some(w);
                    }
                }
                self
            }

            /// Sets bold (`700`) font weight on this node and its label children.
            pub fn bold(self) -> Self {
                self.weight(700)
            }

            /// Sets semibold (`600`) font weight on this node and its label children.
            pub fn semibold(self) -> Self {
                self.weight(600)
            }

            /// Sets medium (`500`) font weight on this node and its label children.
            pub fn medium(self) -> Self {
                self.weight(500)
            }

            /// Uses a monospace font family on this node and its label children.
            pub fn mono(self) -> Self {
                self.ui.nodes[self.node_idx].style.mono = true;
                for i in 0..self.ui.nodes[self.node_idx].children.len() {
                    let cid = self.ui.nodes[self.node_idx].children[i];
                    if self.ui.nodes[cid].role == Role::Label {
                        self.ui.nodes[cid].style.mono = true;
                    }
                }
                self
            }

            /// Sets the area fill color for sparklines (or similar curve widgets).
            pub fn fill(self, color: Color) -> Self {
                self.ui.nodes[self.node_idx].style.fill_color = Some(color);
                self
            }

            /// Enables automatic translucent area fill derived from the foreground stroke color.
            pub fn filled(self) -> Self {
                self.ui.nodes[self.node_idx].style.fill_auto = true;
                self
            }

            /// Sets the stroke width in logical pixels for sparklines.
            pub fn stroke_width(self, width: f32) -> Self {
                self.ui.nodes[self.node_idx].style.stroke_width = width.max(0.5);
                self
            }

            /// Removes the border from this node.
            pub fn borderless(self) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                s.border_width = 0.0;
                s.border_color = Color::TRANSPARENT;
                self
            }

            /// Clears the background fill from this node.
            pub fn no_bg(self) -> Self {
                self.ui.nodes[self.node_idx].style.bg = None;
                self
            }

            /// Configures a `textarea` to submit on `Enter` (while `Shift+Enter` inserts a newline).
            pub fn submit_on_enter(self) -> Self {
                let id = self.ui.nodes[self.node_idx].id;
                self.ui.pending_submit_on_enter_ids.insert(id);
                self
            }

            /// Returns `true` if `Enter` submitted this `input` or `textarea` on this frame.
            pub fn submitted(&self) -> bool {
                let id = self.ui.nodes[self.node_idx].id;
                self.ui.submitted_ids.contains(&id)
            }

            /// Returns the last computed caret screen rectangle for this `input` or `textarea`.
            pub fn caret_rect(&self) -> Option<Rect> {
                let id = self.ui.nodes[self.node_idx].id;
                self.ui.prev_caret_rects.get(&id).copied()
            }

            /// Enables mouse drag text selection and `Ctrl+C` clipboard copy on `txt` / `rich` nodes.
            pub fn selectable(self) -> Self {
                let n = &mut self.ui.nodes[self.node_idx];
                n.style.selectable = true;
                n.focusable = true;
                n.hit_testable = true;
                self
            }

            /// Sets the background color when hovered.
            pub fn hover_bg(self, color: Color) -> Self {
                let n = &mut self.ui.nodes[self.node_idx];
                n.style.hover_bg = Some(color);
                n.hit_testable = true;
                self
            }

            /// Sets the background color when actively pressed.
            pub fn active_bg(self, color: Color) -> Self {
                let n = &mut self.ui.nodes[self.node_idx];
                n.style.active_bg = Some(color);
                n.hit_testable = true;
                self
            }

            /// Sets minimum width in logical pixels.
            pub fn min_w(self, w: f32) -> Self {
                self.ui.nodes[self.node_idx].style.min_w = Some(w.max(0.0));
                self
            }

            /// Sets maximum width in logical pixels.
            pub fn max_w(self, w: f32) -> Self {
                self.ui.nodes[self.node_idx].style.max_w = Some(w.max(0.0));
                self
            }

            /// Sets minimum height in logical pixels.
            pub fn min_h(self, h: f32) -> Self {
                self.ui.nodes[self.node_idx].style.min_h = Some(h.max(0.0));
                self
            }

            /// Sets maximum height in logical pixels.
            pub fn max_h(self, h: f32) -> Self {
                self.ui.nodes[self.node_idx].style.max_h = Some(h.max(0.0));
                self
            }

            /// Sets node opacity multiplier (`0.0..=1.0`), propagated to descendants.
            pub fn opacity(self, alpha: f32) -> Self {
                self.ui.nodes[self.node_idx].style.opacity = alpha.clamp(0.0, 1.0);
                self
            }

            /// Clips descendants to this container's bounds.
            pub fn clip(self) -> Self {
                self.ui.nodes[self.node_idx].style.clip = true;
                self
            }

            /// Hides the interactive scrollbar track/thumb on a `scroll` or `hscroll` container.
            pub fn no_scrollbar(self) -> Self {
                self.ui.nodes[self.node_idx].style.no_scrollbar = true;
                self
            }

            /// Sets width as a fraction (`0.0..=1.0`) of the parent container's inner width.
            pub fn w_frac(self, frac: f32) -> Self {
                self.ui.nodes[self.node_idx].style.width_frac = Some(frac.max(0.0));
                self
            }

            /// Sets height as a fraction (`0.0..=1.0`) of the parent container's inner height.
            pub fn h_frac(self, frac: f32) -> Self {
                self.ui.nodes[self.node_idx].style.height_frac = Some(frac.max(0.0));
                self
            }

            /// Enforces an explicit aspect ratio (`width / height`, e.g. `16.0 / 9.0`).
            pub fn aspect(self, ratio: f32) -> Self {
                if ratio.is_finite() && ratio > 0.0 {
                    self.ui.nodes[self.node_idx].style.aspect_ratio = Some(ratio);
                }
                self
            }

            /// Pins this child at `top_offset` inside its parent `scroll` container once scrolled past.
            pub fn sticky(self, top_offset: f32) -> Self {
                self.ui.nodes[self.node_idx].style.sticky_top = Some(top_offset);
                self
            }

            /// Sets horizontal text alignment (`Left`, `Center`, or `Right`) for `txt` / `rich`.
            pub fn align(self, align: TextAlign) -> Self {
                self.ui.nodes[self.node_idx].style.text_align = align;
                self
            }

            /// Sets a named font family registered via [`crate::register_font`], [`crate::register_font_bytes`], `WindowConfig::font`, or a system family name.
            pub fn font(self, family: impl Into<String>) -> Self {
                let fam = family.into();
                self.ui.nodes[self.node_idx].style.font_family = Some(fam.clone());
                for i in 0..self.ui.nodes[self.node_idx].children.len() {
                    let cid = self.ui.nodes[self.node_idx].children[i];
                    if self.ui.nodes[cid].role == Role::Label {
                        self.ui.nodes[cid].style.font_family = Some(fam.clone());
                    }
                }
                self
            }

            /// Alias for [`Self::font`]: sets a named font family on this node and its label children.
            pub fn font_family(self, family: impl Into<String>) -> Self {
                self.font(family)
            }

            /// Sets letter spacing (tracking) in `em` units (e.g. `-0.03` or `0.06`).
            pub fn tracking(self, em: f32) -> Self {
                if em.is_finite() {
                    self.ui.nodes[self.node_idx].style.tracking = em.clamp(-0.25, 1.0);
                }
                self
            }

            /// Sets line-height multiplier (default `1.35`).
            pub fn line_height(self, mult: f32) -> Self {
                if mult.is_finite() && mult > 0.0 {
                    self.ui.nodes[self.node_idx].style.line_height = mult.clamp(0.6, 4.0);
                }
                self
            }

            /// Overrides the highlight background color for `.selectable()` text selection.
            pub fn selection_bg(self, color: Color) -> Self {
                self.ui.nodes[self.node_idx].style.selection_bg = Some(color);
                self
            }

            /// Sets the OS pointer cursor icon when hovering or pressing this node.
            pub fn cursor(self, cursor: CursorIcon) -> Self {
                self.ui.nodes[self.node_idx].style.cursor = Some(cursor);
                self.ui.nodes[self.node_idx].hit_testable = true;
                self
            }

            /// Marks this node non-blocking so clicks and scrolls pass through to underlying layers.
            pub fn passthrough(self) -> Self {
                let n = &mut self.ui.nodes[self.node_idx];
                n.style.passthrough = true;
                n.hit_testable = false;
                n.focusable = false;
                self
            }

            /// Sets a linear background gradient from `c0` to `c1` at `angle_deg` degrees (`0.0` = left-to-right, `90.0` = top-to-bottom).
            pub fn bg_grad(self, c0: Color, c1: Color, angle_deg: f32) -> Self {
                self.ui.nodes[self.node_idx].style.bg_grad =
                    Some(Gradient::linear(c0, c1, angle_deg));
                self
            }

            /// Sets a radial background gradient from center color `c0` to outer color `c1`.
            pub fn bg_radial(self, c0: Color, c1: Color) -> Self {
                self.ui.nodes[self.node_idx].style.bg_grad = Some(Gradient::radial(c0, c1));
                self
            }

            /// Sets a linear gradient border of `width` pixels from `c0` to `c1` at `angle_deg` degrees (`0.0` = left-to-right, `90.0` = top-to-bottom).
            pub fn bd_grad(self, width: f32, c0: Color, c1: Color, angle_deg: f32) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                s.border_width = width.max(0.0);
                s.border_color = c0;
                s.bd_grad = Some(Gradient::linear(c0, c1, angle_deg));
                self
            }

            /// Sets uniform visual scale around this node's center (also applied to pointer hit-testing).
            pub fn scale(self, s: f32) -> Self {
                self.ui.nodes[self.node_idx].style.scale =
                    if s.is_finite() { s.max(0.01) } else { 1.0 };
                self
            }

            /// Sets visual rotation in radians around this node's center (also applied to hit-testing).
            pub fn rotate(self, rad: f32) -> Self {
                self.ui.nodes[self.node_idx].style.rotate_rad =
                    if rad.is_finite() { rad } else { 0.0 };
                self
            }

            /// Sets post-layout visual translation in logical pixels (also shifts hit-testing bounds).
            pub fn translate(self, x: f32, y: f32) -> Self {
                self.ui.nodes[self.node_idx].style.translate = [
                    if x.is_finite() { x } else { 0.0 },
                    if y.is_finite() { y } else { 0.0 },
                ];
                self
            }

            /// Sets per-corner border radii (`top-left`, `top-right`, `bottom-right`, `bottom-left`).
            pub fn r_corners(self, tl: f32, tr: f32, br: f32, bl: f32) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                s.corner_radii = Some([tl.max(0.0), tr.max(0.0), br.max(0.0), bl.max(0.0)]);
                s.radius = tl.max(tr).max(br).max(bl).max(0.0);
                self
            }

            /// Sets per-side border widths (`left`, `right`, `top`, `bottom`) with `color`.
            pub fn bd_sides(self, l: f32, r: f32, t: f32, b: f32, color: Color) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                s.border_sides = Some([l.max(0.0), r.max(0.0), t.max(0.0), b.max(0.0)]);
                s.border_sides_color = color;
                self
            }

            /// Sets a left border of `width` and `color`.
            pub fn bd_left(self, width: f32, color: Color) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                let mut sides = s.border_sides.unwrap_or([0.0; 4]);
                sides[0] = width.max(0.0);
                s.border_sides = Some(sides);
                s.border_sides_color = color;
                self
            }

            /// Sets a right border of `width` and `color`.
            pub fn bd_right(self, width: f32, color: Color) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                let mut sides = s.border_sides.unwrap_or([0.0; 4]);
                sides[1] = width.max(0.0);
                s.border_sides = Some(sides);
                s.border_sides_color = color;
                self
            }

            /// Sets a top border of `width` and `color`.
            pub fn bd_top(self, width: f32, color: Color) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                let mut sides = s.border_sides.unwrap_or([0.0; 4]);
                sides[2] = width.max(0.0);
                s.border_sides = Some(sides);
                s.border_sides_color = color;
                self
            }

            /// Sets a bottom border of `width` and `color`.
            pub fn bd_bottom(self, width: f32, color: Color) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                let mut sides = s.border_sides.unwrap_or([0.0; 4]);
                sides[3] = width.max(0.0);
                s.border_sides = Some(sides);
                s.border_sides_color = color;
                self
            }

            /// Removes card padding, border, and background fill from this node (and removes the header row on `slider`/`range_slider`).
            pub fn compact(self) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                s.pad_l = 0.0;
                s.pad_r = 0.0;
                s.pad_t = 0.0;
                s.pad_b = 0.0;
                s.border_width = 0.0;
                s.border_color = Color::TRANSPARENT;
                s.bg = None;
                let has_slider = self.ui.nodes[self.node_idx]
                    .children
                    .iter()
                    .any(|&cid| self.ui.nodes[cid].role == Role::Slider);
                if has_slider {
                    let mut retained = Vec::new();
                    for &cid in &self.ui.nodes[self.node_idx].children {
                        if self.ui.nodes[cid].role == Role::Slider {
                            retained.push(cid);
                        }
                    }
                    self.ui.nodes[self.node_idx].children = retained;
                }
                self
            }

            /// Sets uniform outer margin on all four sides in logical points.
            pub fn m(self, all: f32) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                s.margin_l = all;
                s.margin_r = all;
                s.margin_t = all;
                s.margin_b = all;
                self
            }

            /// Sets horizontal outer margins (`left`, `right`) in logical points.
            pub fn mx(self, horizontal: f32) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                s.margin_l = horizontal;
                s.margin_r = horizontal;
                self
            }

            /// Sets vertical outer margins (`top`, `bottom`) in logical points.
            pub fn my(self, vertical: f32) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                s.margin_t = vertical;
                s.margin_b = vertical;
                self
            }

            /// Sets top outer margin in logical points.
            pub fn mt(self, top: f32) -> Self {
                self.ui.nodes[self.node_idx].style.margin_t = top;
                self
            }

            /// Sets bottom outer margin in logical points.
            pub fn mb(self, bottom: f32) -> Self {
                self.ui.nodes[self.node_idx].style.margin_b = bottom;
                self
            }

            /// Sets left outer margin in logical points.
            pub fn ml(self, left: f32) -> Self {
                self.ui.nodes[self.node_idx].style.margin_l = left;
                self
            }

            /// Sets right outer margin in logical points.
            pub fn mr(self, right: f32) -> Self {
                self.ui.nodes[self.node_idx].style.margin_r = right;
                self
            }

            /// Enables an analytical SDF drop shadow with `blur` radius and `color`.
            pub fn shadow(self, blur: f32, color: Color) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                s.shadow_blur = blur.max(0.0);
                s.shadow_color = color;
                self
            }

            /// Renders this node's SDF background/border as a circle (`sdCircle`).
            pub fn circle(self) -> Self {
                self.ui.nodes[self.node_idx].style.shape = RectShape::Circle;
                self
            }

            /// Renders this node's SDF background/border as a smooth squircle.
            pub fn squircle(self) -> Self {
                self.ui.nodes[self.node_idx].style.shape = RectShape::Squircle;
                self
            }

            /// Renders this node's SDF background/border with 45-degree beveled (chamfered) corners of `size` logical points.
            pub fn bevel(self, size: f32) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                s.shape = RectShape::Bevel;
                let sz = size.max(0.0);
                s.radius = sz;
                s.corner_radii = Some([sz; 4]);
                self
            }

            /// Renders this node's SDF background/border with 45-degree beveled corners of `size` logical points and vertex fillet rounding `fillet_r`.
            pub fn bevel_rounded(self, size: f32, fillet_r: f32) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                s.shape = RectShape::Bevel;
                let sz = size.max(0.0);
                s.radius = sz;
                s.corner_radii = Some([sz; 4]);
                s.corner_fillet = fillet_r.max(0.0);
                self
            }

            /// Renders this node's SDF background/border with independent 45-degree beveled corners `[tl, tr, br, bl]`.
            pub fn bevel_corners(self, tl: f32, tr: f32, br: f32, bl: f32) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                s.shape = RectShape::Bevel;
                s.radius = tl.max(tr).max(br).max(bl).max(0.0);
                s.corner_radii = Some([tl.max(0.0), tr.max(0.0), br.max(0.0), bl.max(0.0)]);
                self
            }

            /// Renders this node's SDF background/border with independent 45-degree beveled corners `[tl, tr, br, bl]` and vertex fillet rounding `fillet_r`.
            pub fn bevel_corners_rounded(
                self,
                tl: f32,
                tr: f32,
                br: f32,
                bl: f32,
                fillet_r: f32,
            ) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                s.shape = RectShape::Bevel;
                s.radius = tl.max(tr).max(br).max(bl).max(0.0);
                s.corner_radii = Some([tl.max(0.0), tr.max(0.0), br.max(0.0), bl.max(0.0)]);
                s.corner_fillet = fillet_r.max(0.0);
                self
            }

            /// Sets the vertex fillet rounding radius in logical points on `.bevel(...)`, `.bevel_corners(...)`, `.hexagon()`, or `.hexagon_pointy()` shapes.
            pub fn fillet(self, fillet_r: f32) -> Self {
                let r = fillet_r.max(0.0);
                let s = &mut self.ui.nodes[self.node_idx].style;
                s.corner_fillet = r;
                if matches!(s.shape, RectShape::Hexagon | RectShape::HexagonPointy) {
                    s.radius = r;
                }
                self
            }

            /// Alias for [`Self::fillet`]: sets the vertex fillet rounding radius in logical points.
            pub fn corner_round(self, fillet_r: f32) -> Self {
                self.fillet(fillet_r)
            }

            /// Renders this node's SDF background, border, shadow, and image clip mask as a regular flat-top hexagon.
            ///
            /// Chain `.r(vertex_radius)` or `.fillet(vertex_radius)` to round the hexagon's 6 vertices.
            pub fn hexagon(self) -> Self {
                self.ui.nodes[self.node_idx].style.shape = RectShape::Hexagon;
                self
            }

            /// Renders this node's SDF background, border, shadow, and image clip mask as a regular pointy-top hexagon.
            ///
            /// Chain `.r(vertex_radius)` or `.fillet(vertex_radius)` to round the hexagon's 6 vertices.
            pub fn hexagon_pointy(self) -> Self {
                self.ui.nodes[self.node_idx].style.shape = RectShape::HexagonPointy;
                self
            }

            /// Overrides hover foreground text color (`:hover { color }`).
            pub fn hover_fg(self, color: Color) -> Self {
                self.ui.nodes[self.node_idx].style.hover_fg = Some(color);
                self
            }

            /// Overrides pressed/active foreground text color (`:active { color }`).
            pub fn active_fg(self, color: Color) -> Self {
                self.ui.nodes[self.node_idx].style.active_fg = Some(color);
                self
            }

            /// Overrides hover border color (`:hover { border-color }`).
            pub fn hover_bd(self, color: Color) -> Self {
                self.ui.nodes[self.node_idx].style.hover_bd = Some(color);
                self
            }

            /// Overrides focused border color (`:focus { border-color }`).
            pub fn focus_bd(self, color: Color) -> Self {
                self.ui.nodes[self.node_idx].style.focus_bd = Some(color);
                self
            }

            /// Overrides focus ring stroke width and color on this node (`width <= 0.0` disables it).
            pub fn focus_ring(self, width: f32, color: Color) -> Self {
                let w = width.max(0.0);
                let st = &mut self.ui.nodes[self.node_idx].style;
                st.focus_ring_width = Some(w);
                st.focus_ring_color = Some(color);
                st.no_focus_ring = w <= 0.0;
                self
            }

            /// Sets the outward focus ring offset in logical points (`outline-offset`).
            pub fn focus_ring_offset(self, offset: f32) -> Self {
                self.ui.nodes[self.node_idx].style.focus_ring_offset = Some(offset);
                self
            }

            /// Disables the focus ring on this node.
            pub fn no_focus_ring(self) -> Self {
                self.ui.nodes[self.node_idx].style.no_focus_ring = true;
                self
            }

            /// Overrides background color when disabled (`:disabled { background }`).
            pub fn disabled_bg(self, color: Color) -> Self {
                self.ui.nodes[self.node_idx].style.disabled_bg = Some(color);
                self
            }

            /// Overrides foreground text color when disabled (`:disabled { color }`).
            pub fn disabled_fg(self, color: Color) -> Self {
                self.ui.nodes[self.node_idx].style.disabled_fg = Some(color);
                self
            }

            /// Explicitly enables or disables pointer-down ripple feedback on this node.
            pub fn ripple(self, enabled: bool) -> Self {
                self.ui.nodes[self.node_idx].style.ripple = Some(enabled);
                self
            }

            /// Overrides the scrollbar track width/height in logical points on a `scroll` / `hscroll` container.
            pub fn scrollbar_w(self, width: f32) -> Self {
                self.ui.nodes[self.node_idx].style.scrollbar_w = Some(width.max(4.0));
                self
            }

            /// Overrides the scrollbar track and thumb colors on a `scroll` / `hscroll` container.
            pub fn scrollbar_colors(self, track: Color, thumb: Color) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                s.scrollbar_track = Some(track);
                s.scrollbar_thumb = Some(thumb);
                self
            }

            /// Enables Dual-Kawase frosted glass backdrop blur beneath this surface.
            pub fn glass(self) -> Self {
                self.ui.nodes[self.node_idx].style.glass = true;
                self
            }

            /// Enables multi-line word wrapping for text inside this node.
            pub fn wrap(self) -> Self {
                self.ui.nodes[self.node_idx].style.wrap = true;
                self
            }

            /// Truncates single-line overflowing text with `...`.
            pub fn ellipsis(self) -> Self {
                self.ui.nodes[self.node_idx].style.ellipsis = true;
                self
            }

            /// Animates layout bounding-box changes smoothly across frames using a FLIP spring keyed by `id`.
            pub fn anim_layout(self, id: WidgetId) -> Self {
                self.ui.nodes[self.node_idx].style.anim_layout_id = Some(id);
                self
            }

            /// Enables radial pointer-tracking highlight on the node's border/surface (`Reveal` effect).
            pub fn cursor_glow(self) -> Self {
                self.ui.nodes[self.node_idx].style.cursor_glow = true;
                self
            }

            /// Enables animated loading shimmer sweep across the surface.
            pub fn shimmer(self) -> Self {
                self.ui.nodes[self.node_idx].style.shimmer = true;
                self
            }

            /// Sets explicit `(x, y)` viewport coordinates for a root `overlay` or positioned child.
            pub fn pos(self, x: f32, y: f32) -> Self {
                self.ui.nodes[self.node_idx].style.pos = Some([x, y]);
                self
            }

            /// Positions this node at `(x, y)` logical points relative to its parent container's inner origin (or offsets its normal flow slot).
            pub fn offset(self, x: f32, y: f32) -> Self {
                self.ui.nodes[self.node_idx].style.offset = Some([x, y]);
                self
            }

            /// Alias for [`Self::offset`]: positions this node at `(x, y)` relative to its parent container's inner origin.
            pub fn rel_pos(self, x: f32, y: f32) -> Self {
                self.offset(x, y)
            }

            /// Centers child nodes along both main and cross axes of this container.
            pub fn center(self) -> Self {
                let s = &mut self.ui.nodes[self.node_idx].style;
                s.justify = Justify::Center;
                s.align = Some(Align::Center);
                self
            }

            /// Packs child nodes at the start of the container's main axis.
            pub fn justify_start(self) -> Self {
                self.ui.nodes[self.node_idx].style.justify = Justify::Start;
                self
            }

            /// Centers child nodes along the container's main axis when no child uses `.grow()`.
            pub fn justify_center(self) -> Self {
                self.ui.nodes[self.node_idx].style.justify = Justify::Center;
                self
            }

            /// Packs child nodes at the end of the container's main axis when no child uses `.grow()`.
            pub fn justify_end(self) -> Self {
                self.ui.nodes[self.node_idx].style.justify = Justify::End;
                self
            }

            /// Distributes remaining main-axis space evenly between child nodes.
            pub fn justify_between(self) -> Self {
                self.ui.nodes[self.node_idx].style.justify = Justify::SpaceBetween;
                self
            }

            /// Distributes remaining main-axis space evenly around child nodes.
            pub fn justify_around(self) -> Self {
                self.ui.nodes[self.node_idx].style.justify = Justify::SpaceAround;
                self
            }

            /// Aligns child nodes to the start of the container's cross axis.
            pub fn align_start(self) -> Self {
                self.ui.nodes[self.node_idx].style.align = Some(Align::Start);
                self
            }

            /// Centers child nodes along the container's cross axis.
            pub fn align_center(self) -> Self {
                self.ui.nodes[self.node_idx].style.align = Some(Align::Center);
                self
            }

            /// Aligns child nodes to the end of the container's cross axis.
            pub fn align_end(self) -> Self {
                self.ui.nodes[self.node_idx].style.align = Some(Align::End);
                self
            }

            /// Stretches child nodes across the container's cross axis.
            pub fn align_stretch(self) -> Self {
                self.ui.nodes[self.node_idx].style.align = Some(Align::Stretch);
                self
            }

            /// Overrides cross-axis alignment for this specific child node inside a `row` or `col`.
            pub fn align_self(self, align: Align) -> Self {
                self.ui.nodes[self.node_idx].style.align_self = Some(align);
                self
            }

            /// Clamps a root `overlay` container's position so its bounds stay inside the viewport.
            pub fn clamp_viewport(self) -> Self {
                self.ui.nodes[self.node_idx].style.clamp_viewport = true;
                self
            }

            /// Sets placeholder hint text displayed when a text input or textarea is empty.
            pub fn placeholder(self, text: &str) -> Self {
                self.ui.nodes[self.node_idx].style.placeholder = Some(String::from(text));
                self
            }

            /// Attaches a hover-delayed floating tooltip to this widget or container.
            pub fn tooltip(self, text: &str) -> Self {
                if !text.is_empty() {
                    let node = &mut self.ui.nodes[self.node_idx];
                    node.style.tooltip = Some(String::from(text));
                    node.hit_testable = true;
                }
                self
            }

            /// Sets the aspect-ratio scaling mode (`Fill`, `Contain`, `Cover`) on an `img` / `image` widget.
            pub fn fit(self, fit: ImageFit) -> Self {
                self.ui.nodes[self.node_idx].style.image_fit = fit;
                self
            }

            /// Sets the texture sampling filter (`Nearest` or `Linear`) on an `img` / `image` widget.
            pub fn filter(self, filter: TextureFilter) -> Self {
                self.ui.nodes[self.node_idx].style.image_filter = filter;
                self
            }

            /// Sets a multiplicative RGBA tint color on an `img` / `image` widget (defaults to `Color::WHITE`).
            pub fn tint(self, color: Color) -> Self {
                self.ui.nodes[self.node_idx].style.image_tint = color;
                self
            }

            /// Hints explicit source texture `[width, height]` pixels for `img` aspect-ratio calculation on frame 1.
            pub fn tex_size(self, width: u32, height: u32) -> Self {
                self.ui.nodes[self.node_idx].style.image_size = Some([width.max(1), height.max(1)]);
                self
            }
        }
    };
}

impl UiContext {
    pub(crate) fn set_subtree_enabled(&mut self, idx: usize, enabled: bool) {
        self.nodes[idx].enabled = enabled;
        if !enabled {
            self.nodes[idx].focusable = false;
            let muted = self.theme.text_muted;
            let surf_alt = self.theme.surface_alt;
            let border = self.theme.border;
            match self.nodes[idx].role {
                Role::Button => {
                    self.nodes[idx].style.bg = Some(surf_alt);
                    self.nodes[idx].style.fg = Some(muted);
                }
                Role::Label => {
                    if self.nodes[idx].style.bg.is_some() && self.nodes[idx].label.is_empty() {
                        self.nodes[idx].style.bg = Some(muted);
                    }
                    self.nodes[idx].style.fg = Some(muted);
                }
                Role::Toggle => {
                    if let Some(&track_idx) = self.nodes[idx].children.first() {
                        self.nodes[track_idx].style.bg = Some(surf_alt);
                        self.nodes[track_idx].style.border_color = border;
                        if let Some(&thumb_idx) = self.nodes[track_idx].children.get(1) {
                            self.nodes[thumb_idx].style.bg = Some(muted);
                        } else if let Some((ref mut cmds, _)) = self.nodes[track_idx].viewport_data
                        {
                            for cmd in cmds {
                                if let Canvas2DCmd::Segment {
                                    color: ref mut c, ..
                                } = cmd
                                {
                                    *c = muted;
                                }
                            }
                        }
                    }
                }
                Role::Slider => {
                    self.nodes[idx].style.bg = Some(surf_alt);
                    self.nodes[idx].style.border_color = border;
                    if let Some(&fill_idx) = self.nodes[idx].children.first() {
                        if self.nodes[fill_idx].style.bg.is_some() {
                            self.nodes[fill_idx].style.bg = Some(muted);
                        }
                    }
                }
                Role::TextInput => {
                    self.nodes[idx].style.fg = Some(muted);
                    self.nodes[idx].style.border_color = border;
                }
                _ => {}
            }
        }
        let children = core::mem::take(&mut self.nodes[idx].children);
        for &cid in &children {
            self.set_subtree_enabled(cid, enabled);
        }
        self.nodes[idx].children = children;
    }
}

/// Fluent shorthand style builder returned by layout containers and widgets.
pub struct StyleBuilder<'a> {
    pub(crate) ui: &'a mut UiContext,
    pub(crate) node_idx: usize,
    pub(crate) changed: bool,
}
impl_style_methods!(StyleBuilder);

impl core::ops::Deref for StyleBuilder<'_> {
    type Target = bool;

    fn deref(&self) -> &Self::Target {
        &self.changed
    }
}

impl PartialEq<bool> for StyleBuilder<'_> {
    fn eq(&self, other: &bool) -> bool {
        self.changed == *other
    }
}

impl StyleBuilder<'_> {
    /// Returns `true` if a stateful widget (`toggle`, `checkbox`, `slider`, `input`, `dropdown`, `keybind`) modified its value this frame.
    pub fn changed(&self) -> bool {
        self.changed
    }

    /// Returns the deterministic `WidgetId` of this node.
    pub fn id(&self) -> WidgetId {
        self.ui.nodes[self.node_idx].id
    }

    /// Returns the laid-out `Rect` of this node from the previous frame, if available.
    pub fn prev_rect(&self) -> Option<Rect> {
        let id = self.id();
        self.ui.prev_rects.get(&id).copied()
    }

    /// Enables or disables interaction and focus on this node and its subtree.
    pub fn enabled(mut self, enabled: bool) -> Self {
        let idx = self.node_idx;
        self.ui.set_subtree_enabled(idx, enabled);
        if !enabled {
            self.changed = false;
        }
        self
    }

    /// Customizes the indicator track/box dimensions (`width`, `height`) on a `toggle` or `checkbox`.
    pub fn switch_size(self, width: f32, height: f32) -> Self {
        let w = width.max(8.0);
        let h = height.max(8.0);
        if let Some(&track_idx) = self.ui.nodes[self.node_idx].children.first() {
            self.ui.nodes[track_idx].style.width = Some(w);
            self.ui.nodes[track_idx].style.height = Some(h);
            self.ui.nodes[track_idx].intrinsic_w = w;
            self.ui.nodes[track_idx].intrinsic_h = h;
            if self.ui.nodes[self.node_idx].role == Role::Toggle {
                if self.ui.nodes[track_idx].children.len() == 2 {
                    self.ui.nodes[track_idx].style.radius = h * 0.5;
                    let pad = (h * 0.125).clamp(2.0, 4.0);
                    self.ui.nodes[track_idx].style.pad_t = pad;
                    self.ui.nodes[track_idx].style.pad_b = pad;
                    let thumb_d = (h - pad * 2.0).max(4.0);
                    let spacer_idx = self.ui.nodes[track_idx].children[0];
                    let thumb_idx = self.ui.nodes[track_idx].children[1];
                    let prev_spacer_w = self.ui.nodes[spacer_idx].style.width.unwrap_or(3.0);
                    let t_norm = ((prev_spacer_w - 3.0) / 16.0).clamp(0.0, 1.0);
                    let travel = (w - thumb_d - pad * 2.0).max(0.0);
                    self.ui.nodes[spacer_idx].style.width = Some(pad + t_norm * travel);
                    self.ui.nodes[spacer_idx].style.height = Some(thumb_d);
                    self.ui.nodes[thumb_idx].style.width = Some(thumb_d);
                    self.ui.nodes[thumb_idx].style.height = Some(thumb_d);
                } else {
                    self.ui.nodes[track_idx].style.radius =
                        (w.min(h) * (4.0 / 18.0)).clamp(3.0, 8.0);
                    let default_mark = self.ui.theme.on_primary;
                    if let Some((ref mut cmds, _)) = self.ui.nodes[track_idx].viewport_data {
                        let mark_col = cmds
                            .iter()
                            .find_map(|cmd| match cmd {
                                Canvas2DCmd::Segment { color, .. } => Some(*color),
                                _ => None,
                            })
                            .unwrap_or(default_mark);
                        *cmds = crate::ui::widgets::checkbox_mark_cmds(w, h, mark_col);
                    }
                }
            }
        }
        self
    }

    /// Customizes the square indicator size on a `checkbox` (or `toggle`).
    pub fn box_size(self, size: f32) -> Self {
        self.switch_size(size, size)
    }

    /// Overrides the indicator track or checkbox box background color on a `toggle`, `checkbox`, or `slider`.
    pub fn track_bg(self, color: Color) -> Self {
        let track_idx = self.ui.nodes[self.node_idx]
            .children
            .iter()
            .copied()
            .find(|&cid| self.ui.nodes[cid].role == Role::Slider)
            .or_else(|| self.ui.nodes[self.node_idx].children.first().copied());
        if let Some(track_idx) = track_idx {
            self.ui.nodes[track_idx].style.bg = Some(color);
        }
        self
    }

    /// Overrides the thumb circle or checkmark color on a `toggle` or `checkbox`.
    pub fn thumb_bg(self, color: Color) -> Self {
        if let Some(&track_idx) = self.ui.nodes[self.node_idx].children.first() {
            if let Some(&thumb_idx) = self.ui.nodes[track_idx].children.get(1) {
                self.ui.nodes[thumb_idx].style.bg = Some(color);
            } else if let Some((ref mut cmds, _)) = self.ui.nodes[track_idx].viewport_data {
                for cmd in cmds {
                    if let Canvas2DCmd::Segment {
                        color: ref mut c, ..
                    } = cmd
                    {
                        *c = color;
                    }
                }
            }
        }
        self
    }

    /// Overrides the indicator track or checkbox box border on a `toggle`, `checkbox`, or `slider`.
    pub fn track_bd(self, width: f32, color: Color) -> Self {
        let track_idx = self.ui.nodes[self.node_idx]
            .children
            .iter()
            .copied()
            .find(|&cid| self.ui.nodes[cid].role == Role::Slider)
            .or_else(|| self.ui.nodes[self.node_idx].children.first().copied());
        if let Some(track_idx) = track_idx {
            self.ui.nodes[track_idx].style.border_width = width;
            self.ui.nodes[track_idx].style.border_color = color;
        }
        self
    }

    /// Customizes the track height on a `slider` or `range_slider`.
    pub fn track_h(self, height: f32) -> Self {
        let h = height.max(4.0);
        for i in 0..self.ui.nodes[self.node_idx].children.len() {
            let cid = self.ui.nodes[self.node_idx].children[i];
            if self.ui.nodes[cid].role == Role::Slider {
                self.ui.nodes[cid].style.height = Some(h + 8.0);
                self.ui.nodes[cid].style.radius = (h + 8.0) * 0.5;
                for j in 0..self.ui.nodes[cid].children.len() {
                    let scid = self.ui.nodes[cid].children[j];
                    if self.ui.nodes[scid].style.shape != RectShape::Circle {
                        self.ui.nodes[scid].style.height = Some(h);
                        self.ui.nodes[scid].style.radius = h * 0.5;
                    }
                }
            }
        }
        self
    }

    /// Customizes the draggable divider thickness in logical points on an `hsplit` or `vsplit`.
    pub fn divider_size(self, size: f32) -> Self {
        let sz = size.max(1.0);
        let is_row = self.ui.nodes[self.node_idx].kind == AxisKind::Row;
        for i in 0..self.ui.nodes[self.node_idx].children.len() {
            let cid = self.ui.nodes[self.node_idx].children[i];
            if self.ui.nodes[cid].role == Role::SplitDivider {
                if is_row {
                    self.ui.nodes[cid].style.width = Some(sz);
                    self.ui.nodes[cid].intrinsic_w = sz;
                } else {
                    self.ui.nodes[cid].style.height = Some(sz);
                    self.ui.nodes[cid].intrinsic_h = sz;
                }
            }
        }
        self
    }

    /// Customizes the thumb size on a `range_slider` or `toggle`.
    pub fn thumb_size(self, diameter: f32) -> Self {
        let d = diameter.max(6.0);
        if self.ui.nodes[self.node_idx].role == Role::Toggle {
            return self.switch_size(d * 2.1, d + 6.0);
        }
        for i in 0..self.ui.nodes[self.node_idx].children.len() {
            let cid = self.ui.nodes[self.node_idx].children[i];
            if self.ui.nodes[cid].role == Role::Slider {
                for j in 0..self.ui.nodes[cid].children.len() {
                    let scid = self.ui.nodes[cid].children[j];
                    if self.ui.nodes[scid].style.shape == RectShape::Circle {
                        self.ui.nodes[scid].style.width = Some(d);
                        self.ui.nodes[scid].style.height = Some(d);
                    }
                }
            }
        }
        self
    }

    /// Marks this node hit-testable/focusable and returns `true` if clicked or activated this frame.
    pub fn clicked(&mut self) -> bool {
        if !self.ui.nodes[self.node_idx].enabled {
            return false;
        }
        let id = self.ui.nodes[self.node_idx].id;
        let _ = self.ui.ids.register_interactive(id);
        self.ui.nodes[self.node_idx].hit_testable = true;
        self.ui.nodes[self.node_idx].focusable = true;
        !self.ui.prev_disabled_ids.contains(&id)
            && (self.ui.clicked_id == Some(id) || self.ui.activated_id == Some(id))
    }

    /// Marks this node hit-testable and returns `true` if double-clicked this frame.
    pub fn double_clicked(&mut self) -> bool {
        if !self.ui.nodes[self.node_idx].enabled {
            return false;
        }
        let id = self.ui.nodes[self.node_idx].id;
        let _ = self.ui.ids.register_interactive(id);
        self.ui.nodes[self.node_idx].hit_testable = true;
        !self.ui.prev_disabled_ids.contains(&id) && self.ui.double_clicked_id == Some(id)
    }

    /// Marks this node hit-testable and returns `true` if right-clicked this frame.
    pub fn right_clicked(&mut self) -> bool {
        if !self.ui.nodes[self.node_idx].enabled {
            return false;
        }
        let id = self.ui.nodes[self.node_idx].id;
        let _ = self.ui.ids.register_interactive(id);
        self.ui.nodes[self.node_idx].hit_testable = true;
        !self.ui.prev_disabled_ids.contains(&id) && self.ui.right_clicked_id == Some(id)
    }

    /// Marks this node hit-testable and returns `true` if the pointer is hovering over it.
    pub fn hovered(&mut self) -> bool {
        if !self.ui.nodes[self.node_idx].enabled {
            return false;
        }
        let id = self.ui.nodes[self.node_idx].id;
        self.ui.nodes[self.node_idx].hit_testable = true;
        !self.ui.prev_disabled_ids.contains(&id) && self.ui.hovered_id == Some(id)
    }

    /// Returns `true` if this node holds keyboard focus.
    pub fn focused(&self) -> bool {
        if !self.ui.nodes[self.node_idx].enabled {
            return false;
        }
        let id = self.ui.nodes[self.node_idx].id;
        !self.ui.prev_disabled_ids.contains(&id) && self.ui.focused_id == Some(id)
    }
}

/// Interactive response and style builder returned by `UiContext::btn`.
pub struct ButtonResponse<'a> {
    pub(super) ui: &'a mut UiContext,
    pub(super) node_idx: usize,
    pub(super) clicked: bool,
    pub(super) hovered: bool,
    pub(super) focused: bool,
}

impl ButtonResponse<'_> {
    /// Returns the deterministic `WidgetId` of this button.
    pub fn id(&self) -> WidgetId {
        self.ui.nodes[self.node_idx].id
    }

    /// Returns the laid-out `Rect` of this button from the previous frame, if available.
    pub fn prev_rect(&self) -> Option<Rect> {
        let id = self.id();
        self.ui.prev_rects.get(&id).copied()
    }

    /// Enables or disables this button, suppressing click/hover/focus and applying muted styling when `false`.
    pub fn enabled(mut self, enabled: bool) -> Self {
        let idx = self.node_idx;
        self.ui.set_subtree_enabled(idx, enabled);
        if !enabled {
            self.clicked = false;
            self.hovered = false;
            self.focused = false;
        }
        self
    }

    /// Returns `true` if the button was clicked or activated via keyboard in this frame.
    pub fn clicked(&self) -> bool {
        self.clicked && self.ui.nodes[self.node_idx].enabled
    }

    /// Returns `true` if the button was double-clicked in this frame.
    pub fn double_clicked(&self) -> bool {
        let id = self.id();
        self.ui.nodes[self.node_idx].enabled && self.ui.double_clicked_id == Some(id)
    }

    /// Returns `true` if the button was right-clicked in this frame.
    pub fn right_clicked(&self) -> bool {
        let id = self.id();
        self.ui.nodes[self.node_idx].enabled && self.ui.right_clicked_id == Some(id)
    }

    /// Returns `true` if the pointer is currently hovering over this button.
    pub fn hovered(&self) -> bool {
        self.hovered && self.ui.nodes[self.node_idx].enabled
    }

    /// Returns `true` if this button holds keyboard focus.
    pub fn focused(&self) -> bool {
        self.focused && self.ui.nodes[self.node_idx].enabled
    }
}
impl_style_methods!(ButtonResponse);

/// Interactive response and style builder returned by `UiContext::rich`.
pub struct RichResponse<'a> {
    pub(super) ui: &'a mut UiContext,
    pub(super) node_idx: usize,
    pub(super) clicked_link: Option<String>,
}

impl RichResponse<'_> {
    /// Returns the ID of the clicked or keyboard-activated link span, if any.
    pub fn clicked_link(&self) -> Option<&str> {
        self.clicked_link.as_deref()
    }

    /// Opens the clicked link via [`crate::App::open_url`] when its target uses `http://`, `https://`, or `mailto:`. Returns `true` if launched.
    pub fn open_clicked_url(&self, app: &mut crate::os::App) -> bool {
        if let Some(url) = self.clicked_link() {
            return app.open_url(url).is_ok();
        }
        false
    }
}
impl_style_methods!(RichResponse);
