use super::*;

impl UiContext {
    fn measure_leaf_content(
        node: &LayoutNode,
        default_font_size: f32,
        max_width: Option<f32>,
        measurer: &mut Option<&mut dyn TextMeasure>,
    ) -> [f32; 2] {
        let font_sz = node.style.font_size.unwrap_or(default_font_size);
        let weight = node.style.font_weight.unwrap_or(400);
        let wrap = node.style.wrap;
        let ellipsis = node.style.ellipsis;

        if node.sparkline_samples.is_some() {
            return [180.0, 40.0];
        }
        if node.image_tex.is_some() {
            if let Some([tw, th]) = node.style.image_size {
                return [(tw as f32).max(1.0), (th as f32).max(1.0)];
            }
            return [48.0, 48.0];
        }
        if node.viewport_data.is_some() {
            return [320.0, 200.0];
        }
        let min_input = if node.role == Role::TextInput {
            if wrap {
                [160.0_f32, 80.0_f32]
            } else {
                [120.0_f32, font_sz * 1.35]
            }
        } else {
            [0.0, 0.0]
        };

        if let Some(ref spans) = node.rich_spans {
            if let Some(m) = measurer.as_deref_mut() {
                return m.measure_rich_formatted(
                    spans,
                    font_sz,
                    node.style.font_family.as_deref(),
                    node.style.tracking,
                    node.style.line_height,
                    max_width,
                    wrap,
                    ellipsis,
                );
            }
            let total_chars: usize = spans.iter().map(|s| s.text.chars().count()).sum();
            let raw_w = total_chars as f32 * font_sz * (0.6 + node.style.tracking.max(-0.2));
            let lh = font_sz * node.style.line_height.max(0.5);
            if wrap {
                if let Some(mw) = max_width {
                    if mw > 0.0 && raw_w > mw {
                        let lines = (raw_w / mw).ceil().max(1.0);
                        return [mw, lines * lh];
                    }
                }
            }
            return [raw_w, lh];
        }

        if let Some(ref txt) = node.text {
            let [mw, mh] = if let Some(m) = measurer.as_deref_mut() {
                m.measure_plain_formatted(
                    txt,
                    font_sz,
                    weight,
                    node.style.mono,
                    node.style.font_family.as_deref(),
                    node.style.tracking,
                    node.style.line_height,
                    max_width,
                    wrap,
                    ellipsis,
                )
            } else {
                let chars = txt.chars().count() as f32;
                let raw_w = chars * font_sz * (0.6 + node.style.tracking.max(-0.2));
                let lh = font_sz * node.style.line_height.max(0.5);
                if wrap {
                    if let Some(mw) = max_width {
                        if mw > 0.0 && raw_w > mw {
                            let lines = (raw_w / mw).ceil().max(1.0);
                            [mw, lines * lh]
                        } else {
                            [raw_w, lh]
                        }
                    } else {
                        [raw_w, lh]
                    }
                } else {
                    [raw_w, lh]
                }
            };
            return [mw.max(min_input[0]), mh.max(min_input[1])];
        }

        min_input
    }

    pub(super) fn measure_node(
        &mut self,
        idx: usize,
        default_font_size: f32,
        avail_w: Option<f32>,
        measurer: &mut Option<&mut dyn TextMeasure>,
    ) {
        let kind = self.nodes[idx].kind;
        let pad_x = self.nodes[idx].style.pad_l + self.nodes[idx].style.pad_r;
        let pad_y = self.nodes[idx].style.pad_t + self.nodes[idx].style.pad_b;
        let gap = self.nodes[idx].style.gap;
        let explicit_w = self.nodes[idx].style.width.or_else(|| {
            self.nodes[idx]
                .style
                .width_frac
                .and_then(|f| avail_w.map(|aw| (aw * f).max(0.0)))
        });
        let explicit_h = self.nodes[idx].style.height.or_else(|| {
            self.nodes[idx]
                .style
                .height_frac
                .map(|f| (self.viewport_h * f).max(0.0))
        });
        let inner_avail_w = match (explicit_w.or(avail_w), self.nodes[idx].style.max_w) {
            (Some(w), Some(mw)) => Some((w.min(mw) - pad_x).max(1.0)),
            (Some(w), None) => Some((w - pad_x).max(1.0)),
            (None, Some(mw)) => Some((mw - pad_x).max(1.0)),
            (None, None) => None,
        };

        let children = core::mem::take(&mut self.nodes[idx].children);
        match kind {
            AxisKind::Leaf => {}
            AxisKind::Col | AxisKind::ZStack => {
                for &cid in &children {
                    let child_avail = self.nodes[cid].style.width.or(inner_avail_w);
                    self.measure_node(cid, default_font_size, child_avail, measurer);
                }
            }
            AxisKind::Grid => {
                let grid_avail = inner_avail_w.unwrap_or(self.viewport_w);
                let cols = if let Some(min_w) = self.nodes[idx].style.grid_min_col_w {
                    ((grid_avail + gap) / (min_w + gap)).floor().max(1.0) as usize
                } else {
                    self.nodes[idx].style.grid_cols.unwrap_or(1).max(1)
                };
                let col_w =
                    ((grid_avail - gap * (cols.saturating_sub(1)) as f32) / cols as f32).max(1.0);
                for &cid in &children {
                    let child_avail = self.nodes[cid].style.width.or(Some(col_w));
                    self.measure_node(cid, default_font_size, child_avail, measurer);
                }
            }
            AxisKind::Row => {
                let mut has_grow = false;
                for &cid in &children {
                    if self.nodes[cid].style.grow > 0.0 {
                        has_grow = true;
                    } else {
                        let child_avail = self.nodes[cid].style.width;
                        self.measure_node(cid, default_font_size, child_avail, measurer);
                    }
                }
                if has_grow {
                    let total_gap = if children.len() > 1 {
                        gap * (children.len() - 1) as f32
                    } else {
                        0.0
                    };
                    let mut fixed_sum = 0.0_f32;
                    let mut total_grow = 0.0_f32;
                    for &cid in &children {
                        let c = &self.nodes[cid];
                        if c.style.grow > 0.0 {
                            total_grow += c.style.grow;
                            fixed_sum +=
                                c.style.width.unwrap_or(0.0) + c.style.margin_l + c.style.margin_r;
                        } else {
                            fixed_sum += c.intrinsic_w + c.style.margin_l + c.style.margin_r;
                        }
                    }
                    let remaining = inner_avail_w.map(|iw| (iw - total_gap - fixed_sum).max(0.0));
                    for &cid in &children {
                        let grow = self.nodes[cid].style.grow;
                        if grow > 0.0 {
                            let exp_w = self.nodes[cid].style.width;
                            let child_avail = match (exp_w, remaining) {
                                (Some(w), None) => Some(w),
                                (w_opt, Some(rem)) if total_grow > 0.0 => {
                                    Some(w_opt.unwrap_or(0.0) + rem * (grow / total_grow))
                                }
                                (w_opt, _) => w_opt,
                            };
                            self.measure_node(cid, default_font_size, child_avail, measurer);
                        }
                    }
                }
            }
        }

        let (content_w, mut content_h) = match kind {
            AxisKind::Leaf => {
                let [w, h] = Self::measure_leaf_content(
                    &self.nodes[idx],
                    default_font_size,
                    inner_avail_w,
                    measurer,
                );
                self.nodes[idx].content_w = w;
                self.nodes[idx].content_h = h;
                (w, h)
            }
            AxisKind::Row => {
                if self.nodes[idx].style.flex_wrap {
                    let max_line_w = inner_avail_w.unwrap_or(f32::INFINITY);
                    let mut line_w = 0.0_f32;
                    let mut line_h = 0.0_f32;
                    let mut max_w = 0.0_f32;
                    let mut total_h = 0.0_f32;
                    let mut on_line = 0usize;
                    for &cid in &children {
                        let c = &self.nodes[cid];
                        let cw = c.intrinsic_w + c.style.margin_l + c.style.margin_r;
                        let ch = c.intrinsic_h + c.style.margin_t + c.style.margin_b;
                        let needed = if on_line > 0 { gap + cw } else { cw };
                        if on_line > 0 && line_w + needed > max_line_w {
                            max_w = max_w.max(line_w);
                            total_h += if total_h > 0.0 { gap + line_h } else { line_h };
                            line_w = cw;
                            line_h = ch;
                            on_line = 1;
                        } else {
                            line_w += needed;
                            line_h = line_h.max(ch);
                            on_line += 1;
                        }
                    }
                    if on_line > 0 {
                        max_w = max_w.max(line_w);
                        total_h += if total_h > 0.0 { gap + line_h } else { line_h };
                    }
                    (max_w, total_h)
                } else {
                    let mut sum_w = 0.0_f32;
                    let mut max_h = 0.0_f32;
                    for (i, &cid) in children.iter().enumerate() {
                        if i > 0 {
                            sum_w += gap;
                        }
                        let c = &self.nodes[cid];
                        sum_w += c.intrinsic_w + c.style.margin_l + c.style.margin_r;
                        max_h = max_h.max(c.intrinsic_h + c.style.margin_t + c.style.margin_b);
                    }
                    (sum_w, max_h)
                }
            }
            AxisKind::Col => {
                let mut max_w = 0.0_f32;
                let mut sum_h = 0.0_f32;
                for (i, &cid) in children.iter().enumerate() {
                    if i > 0 {
                        sum_h += gap;
                    }
                    let c = &self.nodes[cid];
                    max_w = max_w.max(c.intrinsic_w + c.style.margin_l + c.style.margin_r);
                    sum_h += c.intrinsic_h + c.style.margin_t + c.style.margin_b;
                }
                (max_w, sum_h)
            }
            AxisKind::Grid => {
                let grid_avail = inner_avail_w.unwrap_or(self.viewport_w);
                let cols = if let Some(min_w) = self.nodes[idx].style.grid_min_col_w {
                    ((grid_avail + gap) / (min_w + gap)).floor().max(1.0) as usize
                } else {
                    self.nodes[idx].style.grid_cols.unwrap_or(1).max(1)
                };
                let col_w =
                    ((grid_avail - gap * (cols.saturating_sub(1)) as f32) / cols as f32).max(1.0);
                let mut sum_h = 0.0_f32;
                for (row_idx, row_chunk) in children.chunks(cols).enumerate() {
                    if row_idx > 0 {
                        sum_h += gap;
                    }
                    let mut row_h = 0.0_f32;
                    for &cid in row_chunk {
                        let c = &self.nodes[cid];
                        let ch = if let (Some(r), None) = (c.style.aspect_ratio, c.style.height) {
                            if r > 0.0 {
                                col_w / r
                            } else {
                                c.intrinsic_h
                            }
                        } else {
                            c.intrinsic_h
                        };
                        row_h = row_h.max(ch);
                    }
                    sum_h += row_h;
                }
                let total_w = (cols as f32) * col_w + gap * (cols.saturating_sub(1)) as f32;
                (total_w, sum_h)
            }
            AxisKind::ZStack => {
                let mut max_w = 0.0_f32;
                let mut max_h = 0.0_f32;
                for &cid in &children {
                    let [ox, oy] = self.nodes[cid]
                        .style
                        .offset
                        .map_or([0.0, 0.0], |[x, y]| [x.max(0.0), y.max(0.0)]);
                    max_w = max_w.max(ox + self.nodes[cid].intrinsic_w);
                    max_h = max_h.max(oy + self.nodes[cid].intrinsic_h);
                }
                (max_w, max_h)
            }
        };

        if self.nodes[idx].is_scroll {
            if self.nodes[idx].role == Role::Modal {
                let top_inset = if self.has_csd_titlebar { 40.0 } else { 0.0 };
                let max_modal_h = (self.viewport_h - top_inset - 32.0 - pad_y).max(80.0);
                content_h = content_h.min(max_modal_h);
            } else if explicit_h.is_none()
                && self.nodes[idx].style.max_h.is_none()
                && self.nodes[idx].style.grow <= 0.0
            {
                content_h = content_h.min(self.viewport_h.max(180.0));
            }
        }

        let mut iw = explicit_w.unwrap_or(content_w + pad_x).max(0.0);
        let mut ih = explicit_h.unwrap_or(content_h + pad_y).max(0.0);
        if let Some(ratio) = self.nodes[idx].style.aspect_ratio {
            if ratio > 0.0 {
                if explicit_h.is_none() && (explicit_w.is_some() || avail_w.is_some()) {
                    let target_w = explicit_w.or(avail_w).unwrap_or(iw);
                    iw = target_w;
                    ih = target_w / ratio;
                } else if explicit_w.is_none() && explicit_h.is_some() {
                    iw = ih * ratio;
                }
            }
        }
        if let Some(min_w) = self.nodes[idx].style.min_w {
            iw = iw.max(min_w);
        }
        if let Some(max_w) = self.nodes[idx].style.max_w {
            iw = iw.min(max_w);
        }
        if let Some(min_h) = self.nodes[idx].style.min_h {
            ih = ih.max(min_h);
        }
        if let Some(max_h) = self.nodes[idx].style.max_h {
            ih = ih.min(max_h);
        }
        self.nodes[idx].intrinsic_w = iw;
        self.nodes[idx].intrinsic_h = ih;
        self.nodes[idx].children = children;
    }

    pub(super) fn layout_node(
        &mut self,
        idx: usize,
        mut rect: Rect,
        scale_factor: f32,
        measurer: &mut Option<&mut dyn TextMeasure>,
    ) {
        {
            let s = &self.nodes[idx].style;
            if let Some(ratio) = s.aspect_ratio {
                if ratio > 0.0 {
                    if s.height.is_none() && s.height_frac.is_none() && rect.w > 0.0 {
                        rect.h = rect.w / ratio;
                    } else if s.width.is_none() && s.width_frac.is_none() && rect.h > 0.0 {
                        rect.w = rect.h * ratio;
                    }
                }
            }
            if let Some(min_w) = s.min_w {
                rect.w = rect.w.max(min_w);
            }
            if let Some(max_w) = s.max_w {
                rect.w = rect.w.min(max_w);
            }
            if let Some(min_h) = s.min_h {
                rect.h = rect.h.max(min_h);
            }
            if let Some(max_h) = s.max_h {
                rect.h = rect.h.min(max_h);
            }
        }
        self.nodes[idx].rect = rect.snap(scale_factor);
        let kind = self.nodes[idx].kind;
        let is_scroll = self.nodes[idx].is_scroll;
        let scroll_id = self.nodes[idx].id;
        let scroll_offset_y = self.nodes[idx].scroll_offset_y;
        let children = core::mem::take(&mut self.nodes[idx].children);
        if children.is_empty() {
            self.nodes[idx].children = children;
            return;
        }

        let (pad_l, pad_r, pad_t, pad_b, gap, justify, align) = {
            let s = &self.nodes[idx].style;
            (
                s.pad_l, s.pad_r, s.pad_t, s.pad_b, s.gap, s.justify, s.align,
            )
        };
        let row_align = align.unwrap_or(Align::Center);
        let col_align = align.unwrap_or(Align::Start);
        let justify_center = justify == Justify::Center;
        let align_center = col_align == Align::Center;
        let distribute_main = |j: Justify, free: f32, count: usize, base_gap: f32| -> (f32, f32) {
            match j {
                Justify::Start => (0.0, base_gap),
                Justify::Center => (free * 0.5, base_gap),
                Justify::End => (free, base_gap),
                Justify::SpaceBetween => {
                    if count > 1 {
                        (0.0, base_gap + free / (count - 1) as f32)
                    } else {
                        (0.0, base_gap)
                    }
                }
                Justify::SpaceAround => {
                    if count > 0 {
                        let extra = free / count as f32;
                        (extra * 0.5, base_gap + extra)
                    } else {
                        (0.0, base_gap)
                    }
                }
            }
        };
        let inner_x = rect.x + pad_l;
        let inner_y = rect.y + pad_t;
        let inner_w = (rect.w - pad_l - pad_r).max(0.0);
        let inner_h = (rect.h - pad_t - pad_b).max(0.0);
        let total_gap = if children.len() > 1 {
            gap * (children.len() - 1) as f32
        } else {
            0.0
        };

        match kind {
            AxisKind::Leaf => {}
            AxisKind::Row => {
                if self.nodes[idx].style.flex_wrap {
                    let mut cursor_x = inner_x;
                    let mut cursor_y = inner_y;
                    let mut line_h = 0.0_f32;
                    let mut on_line = 0usize;
                    for &cid in &children {
                        let (exp_w, exp_h, int_w, int_h, offset) = {
                            let c = &self.nodes[cid];
                            (
                                c.style.width,
                                c.style.height,
                                c.intrinsic_w,
                                c.intrinsic_h,
                                c.style.offset,
                            )
                        };
                        let cw = exp_w.unwrap_or(int_w).max(0.0);
                        let ch = exp_h.unwrap_or(int_h).max(0.0);
                        if on_line > 0 && cursor_x + cw > inner_x + inner_w + 0.5 {
                            cursor_x = inner_x;
                            cursor_y += line_h + gap;
                            line_h = 0.0;
                            on_line = 0;
                        }
                        let [ox, oy] = offset.unwrap_or([0.0, 0.0]);
                        let child_rect = Rect::new(cursor_x + ox, cursor_y + oy, cw, ch);
                        self.layout_node(cid, child_rect, scale_factor, measurer);
                        cursor_x += cw + gap;
                        line_h = line_h.max(ch);
                        on_line += 1;
                    }
                } else {
                    let mut fixed_sum = 0.0_f32;
                    let mut total_grow = 0.0_f32;
                    for &cid in &children {
                        let c = &self.nodes[cid];
                        let mx = c.style.margin_l + c.style.margin_r;
                        if !is_scroll && c.style.grow > 0.0 {
                            total_grow += c.style.grow;
                            fixed_sum += c.style.width.unwrap_or(0.0) + mx;
                        } else {
                            fixed_sum += c.intrinsic_w + mx;
                        }
                    }
                    let clamped_offset_x = if is_scroll {
                        let total_w = fixed_sum + total_gap;
                        let max_x = (total_w - inner_w).max(0.0);
                        let sc = self.scroll_states.entry(scroll_id).or_default();
                        sc.max_x = max_x;
                        sc.total_w = total_w;
                        sc.inner_w = inner_w;
                        sc.target_x = sc.target_x.clamp(0.0, max_x);
                        sc.spring_x.target = sc.target_x;
                        if sc.spring_x.value > max_x {
                            sc.spring_x.value = max_x;
                        }
                        scroll_offset_y.clamp(0.0, max_x)
                    } else {
                        0.0
                    };
                    let remaining = if is_scroll {
                        0.0
                    } else {
                        (inner_w - total_gap - fixed_sum).max(0.0)
                    };
                    let (main_off, step_gap) = if !is_scroll && total_grow <= 0.0 {
                        distribute_main(justify, remaining, children.len(), gap)
                    } else {
                        (0.0, gap)
                    };
                    let mut cursor_x = if is_scroll {
                        inner_x - clamped_offset_x
                    } else {
                        inner_x + main_off
                    };
                    for &cid in &children {
                        let (
                            grow,
                            exp_w,
                            exp_h,
                            int_w,
                            int_h,
                            is_container,
                            is_divider,
                            offset,
                            ml,
                            mr,
                            mt,
                            mb,
                            align_self,
                        ) = {
                            let c = &self.nodes[cid];
                            (
                                c.style.grow,
                                c.style
                                    .width
                                    .or_else(|| c.style.width_frac.map(|f| (inner_w * f).max(0.0))),
                                c.style.height.or_else(|| {
                                    c.style.height_frac.map(|f| (inner_h * f).max(0.0))
                                }),
                                c.intrinsic_w,
                                c.intrinsic_h,
                                c.kind != AxisKind::Leaf,
                                c.role == Role::SplitDivider
                                    || c.role == Role::Divider
                                    || c.role == Role::Viewport,
                                c.style.offset,
                                c.style.margin_l,
                                c.style.margin_r,
                                c.style.margin_t,
                                c.style.margin_b,
                                c.style.align_self,
                            )
                        };
                        let cw = if grow > 0.0 && total_grow > 0.0 {
                            exp_w.unwrap_or(0.0) + remaining * (grow / total_grow)
                        } else {
                            int_w
                        };
                        let eff_align = align_self.unwrap_or(row_align);
                        let avail_cross_h = (inner_h - mt - mb).max(0.0);
                        let ch = if let Some(h) = exp_h {
                            h
                        } else if eff_align == Align::Stretch || is_container || is_divider {
                            avail_cross_h
                        } else {
                            int_h.min(avail_cross_h)
                        };
                        let [ox, oy] = offset.unwrap_or([0.0, 0.0]);
                        let cy = match eff_align {
                            Align::Start | Align::Stretch => inner_y + mt + oy,
                            Align::Center => {
                                inner_y + mt + ((avail_cross_h - ch).max(0.0) * 0.5) + oy
                            }
                            Align::End => inner_y + mt + (avail_cross_h - ch).max(0.0) + oy,
                        };
                        let child_rect =
                            Rect::new(cursor_x + ml + ox, cy, cw.max(0.0), ch.max(0.0));
                        self.layout_node(cid, child_rect, scale_factor, measurer);
                        cursor_x += ml + cw.max(0.0) + mr + step_gap;
                    }
                }
            }
            AxisKind::Col => {
                let mut fixed_sum = 0.0_f32;
                let mut total_grow = 0.0_f32;
                if !is_scroll {
                    for &cid in &children {
                        let c = &self.nodes[cid];
                        let my = c.style.margin_t + c.style.margin_b;
                        if c.style.grow > 0.0 {
                            total_grow += c.style.grow;
                            fixed_sum += c.style.height.unwrap_or(0.0) + my;
                        } else {
                            fixed_sum += c.intrinsic_h + my;
                        }
                    }
                }
                let remaining = (inner_h - total_gap - fixed_sum).max(0.0);
                let (mut content_y, step_gap) = if !is_scroll && total_grow <= 0.0 {
                    distribute_main(justify, remaining, children.len(), gap)
                } else {
                    (0.0_f32, gap)
                };
                let clamped_offset = if is_scroll {
                    let mut total_h = 0.0_f32;
                    for (i, &cid) in children.iter().enumerate() {
                        if i > 0 {
                            total_h += gap;
                        }
                        let c = &self.nodes[cid];
                        total_h += c.intrinsic_h + c.style.margin_t + c.style.margin_b;
                    }
                    let max_y = (total_h - inner_h).max(0.0);
                    let snap_bottom = self.pending_scroll_to_bottom.remove(&scroll_id);
                    let sc = self.scroll_states.entry(scroll_id).or_default();
                    sc.max_y = max_y;
                    sc.total_h = total_h;
                    sc.inner_h = inner_h;
                    if snap_bottom {
                        sc.target_y = max_y;
                        sc.spring.target = max_y;
                        sc.spring.value = max_y;
                        sc.spring.velocity = 0.0;
                        max_y
                    } else {
                        sc.target_y = sc.target_y.clamp(0.0, max_y);
                        sc.spring.target = sc.target_y;
                        if sc.spring.value > max_y {
                            sc.spring.value = max_y;
                        }
                        scroll_offset_y.clamp(0.0, max_y)
                    }
                } else {
                    0.0
                };

                for (i, &cid) in children.iter().enumerate() {
                    if i > 0 {
                        content_y += step_gap;
                    }
                    let (
                        grow,
                        exp_w,
                        exp_h,
                        int_w,
                        int_h,
                        is_container,
                        is_divider,
                        wrap,
                        pad_x,
                        pad_y,
                        explicit_pos,
                        offset,
                        sticky_top,
                        ml,
                        mr,
                        mt,
                        mb,
                        align_self,
                    ) = {
                        let c = &self.nodes[cid];
                        (
                            c.style.grow,
                            c.style
                                .width
                                .or_else(|| c.style.width_frac.map(|f| (inner_w * f).max(0.0))),
                            c.style
                                .height
                                .or_else(|| c.style.height_frac.map(|f| (inner_h * f).max(0.0))),
                            c.intrinsic_w,
                            c.intrinsic_h,
                            c.kind != AxisKind::Leaf,
                            c.role == Role::SplitDivider
                                || c.role == Role::Divider
                                || c.role == Role::Viewport
                                || c.role == Role::TextInput,
                            c.style.wrap,
                            c.style.pad_l + c.style.pad_r,
                            c.style.pad_t + c.style.pad_b,
                            c.style.pos,
                            c.style.offset,
                            c.style.sticky_top,
                            c.style.margin_l,
                            c.style.margin_r,
                            c.style.margin_t,
                            c.style.margin_b,
                            c.style.align_self,
                        )
                    };
                    let avail_cross_w = (inner_w - ml - mr).max(0.0);
                    let eff_align = align_self.unwrap_or(col_align);
                    let ch = if !is_scroll && grow > 0.0 && total_grow > 0.0 {
                        exp_h.unwrap_or(0.0) + remaining * (grow / total_grow)
                    } else if let Some(h) = exp_h {
                        h
                    } else if wrap && !is_container {
                        let cw_est = exp_w.unwrap_or(avail_cross_w);
                        let max_cw = (cw_est - pad_x).max(1.0);
                        let [_, measured_h] = Self::measure_leaf_content(
                            &self.nodes[cid],
                            self.theme.font_size,
                            Some(max_cw),
                            measurer,
                        );
                        measured_h + pad_y
                    } else {
                        int_h
                    };
                    let cw = if let Some(w) = exp_w {
                        w
                    } else if eff_align == Align::Stretch || is_container || wrap || is_divider {
                        avail_cross_w
                    } else {
                        int_w.min(avail_cross_w)
                    };
                    let (cx, cy) = if let Some([px, py]) = explicit_pos {
                        (px, py)
                    } else {
                        let [ox, oy] = offset.unwrap_or([0.0, 0.0]);
                        let cx = match eff_align {
                            Align::Start | Align::Stretch => inner_x + ml + ox,
                            Align::Center => {
                                inner_x + ml + ((avail_cross_w - cw).max(0.0) * 0.5) + ox
                            }
                            Align::End => inner_x + ml + (avail_cross_w - cw).max(0.0) + ox,
                        };
                        let raw_y = inner_y + content_y + mt - clamped_offset + oy;
                        let final_y = if is_scroll {
                            if let Some(top_off) = sticky_top {
                                let min_y = inner_y + top_off;
                                let max_y = (inner_y + inner_h - ch).max(min_y);
                                raw_y.max(min_y).min(max_y)
                            } else {
                                raw_y
                            }
                        } else {
                            raw_y
                        };
                        (cx, final_y)
                    };
                    let child_rect = Rect::new(cx, cy, cw.max(0.0), ch.max(0.0));
                    self.layout_node(cid, child_rect, scale_factor, measurer);
                    content_y += mt + ch.max(0.0) + mb;
                }
            }
            AxisKind::Grid => {
                let cols = if let Some(min_w) = self.nodes[idx].style.grid_min_col_w {
                    ((inner_w + gap) / (min_w + gap)).floor().max(1.0) as usize
                } else {
                    self.nodes[idx].style.grid_cols.unwrap_or(1).max(1)
                };
                let col_w =
                    ((inner_w - gap * (cols.saturating_sub(1)) as f32) / cols as f32).max(0.0);
                let mut cursor_y = inner_y;
                for row_chunk in children.chunks(cols) {
                    let mut row_h = 0.0_f32;
                    for &cid in row_chunk {
                        let c = &self.nodes[cid];
                        let ch = if let Some(h) = c
                            .style
                            .height
                            .or_else(|| c.style.height_frac.map(|f| (inner_h * f).max(0.0)))
                        {
                            h
                        } else if let Some(r) = c.style.aspect_ratio {
                            if r > 0.0 {
                                col_w / r
                            } else {
                                c.intrinsic_h
                            }
                        } else {
                            c.intrinsic_h
                        };
                        row_h = row_h.max(ch);
                    }
                    for (col_idx, &cid) in row_chunk.iter().enumerate() {
                        let c_exp_h = self.nodes[cid].style.height.or_else(|| {
                            self.nodes[cid]
                                .style
                                .height_frac
                                .map(|f| (inner_h * f).max(0.0))
                        });
                        let c_aspect = self.nodes[cid].style.aspect_ratio;
                        let ch = c_exp_h.unwrap_or_else(|| {
                            if let Some(r) = c_aspect {
                                if r > 0.0 {
                                    col_w / r
                                } else {
                                    row_h
                                }
                            } else {
                                row_h
                            }
                        });
                        let [ox, oy] = self.nodes[cid].style.offset.unwrap_or([0.0, 0.0]);
                        let cx = inner_x + (col_idx as f32) * (col_w + gap) + ox;
                        let child_rect = Rect::new(cx, cursor_y + oy, col_w, ch.max(0.0));
                        self.layout_node(cid, child_rect, scale_factor, measurer);
                    }
                    cursor_y += row_h + gap;
                }
            }
            AxisKind::ZStack => {
                for &cid in &children {
                    let (
                        grow,
                        exp_w,
                        exp_h,
                        int_w,
                        int_h,
                        is_container,
                        is_modal,
                        explicit_pos,
                        offset,
                    ) = {
                        let c = &self.nodes[cid];
                        (
                            c.style.grow,
                            c.style
                                .width
                                .or_else(|| c.style.width_frac.map(|f| (inner_w * f).max(0.0))),
                            c.style
                                .height
                                .or_else(|| c.style.height_frac.map(|f| (inner_h * f).max(0.0))),
                            c.intrinsic_w,
                            c.intrinsic_h,
                            c.kind != AxisKind::Leaf,
                            c.role == Role::Modal,
                            c.style.pos,
                            c.style.offset,
                        )
                    };
                    let has_pos = explicit_pos.is_some() || offset.is_some();
                    let cw = exp_w.unwrap_or_else(|| {
                        if grow > 0.0 || (is_container && !align_center && !is_modal && !has_pos) {
                            inner_w
                        } else {
                            int_w.min(inner_w)
                        }
                    });
                    let ch = exp_h.unwrap_or_else(|| {
                        if grow > 0.0 || (is_container && !justify_center && !is_modal && !has_pos)
                        {
                            inner_h
                        } else {
                            int_h.min(inner_h)
                        }
                    });
                    let (cx, cy) = if let Some([px, py]) = explicit_pos {
                        (px, py)
                    } else if let Some([ox, oy]) = offset {
                        (inner_x + ox, inner_y + oy)
                    } else {
                        let cx = if align_center {
                            inner_x + ((inner_w - cw).max(0.0) * 0.5)
                        } else {
                            inner_x
                        };
                        let cy = if justify_center {
                            inner_y + ((inner_h - ch).max(0.0) * 0.5)
                        } else {
                            inner_y
                        };
                        (cx, cy)
                    };
                    let child_rect = Rect::new(cx, cy, cw.max(0.0), ch.max(0.0));
                    self.layout_node(cid, child_rect, scale_factor, measurer);
                }
            }
        }
        self.nodes[idx].children = children;
    }

    pub(super) fn flush_interacts(
        &mut self,
        parent_idx: usize,
        child_pos: usize,
        active_clip: Option<Rect>,
    ) {
        for (p, c, hit) in &self.pending_interacts {
            if *p == parent_idx && *c == child_pos {
                let mut h = hit.clone();
                if h.clip.is_none() {
                    h.clip = active_clip;
                }
                self.pending_hits.push(h);
            }
        }
    }

    pub(super) fn emit_node(
        &mut self,
        nodes: &mut [LayoutNode],
        idx: usize,
        is_overlay: bool,
        active_clip: Option<Rect>,
        measurer: &mut Option<&mut dyn TextMeasure>,
    ) {
        let node = core::mem::take(&mut nodes[idx]);
        self.pending_rects.insert(node.id, node.rect);
        if !node.enabled {
            self.pending_disabled_ids.insert(node.id);
        }

        if node.enabled && self.hovered_id == Some(node.id) {
            if let Some(ref tip) = node.style.tooltip {
                if self.hover_elapsed_s >= TOOLTIP_DELAY_S {
                    self.pending_tooltip = Some((node.rect, tip.clone()));
                } else {
                    self.has_active_tooltip_timer = true;
                }
            }
        }

        let focused = node.enabled && self.focused_id == Some(node.id);
        if node.style.cursor_glow {
            self.has_active_cursor_glow = true;
        }
        if node.style.shimmer {
            self.has_active_shimmer = true;
        }

        self.pending_semantics.push(SemanticNode {
            id: node.id,
            role: node.role,
            label: node.label,
            value: node.semantic_value,
            bounds: node.rect,
            focused,
            enabled: node.enabled,
            focusable: node.focusable,
            is_overlay,
        });

        let vp_rect = Rect::new(0.0, 0.0, self.viewport_w, self.viewport_h);
        let visible_bounds = match active_clip {
            Some(c) => c.intersect(vp_rect),
            None => vp_rect,
        };
        let inter = node.rect.intersect(visible_bounds);
        if inter.w > 0.0 && inter.h > 0.0 {
            self.pending_in_view.insert(node.id);
        }

        let [tx_off, ty_off] = node.style.translate;
        let xform = [node.style.scale, node.style.rotate_rad, tx_off, ty_off];
        let hit_bounds = Rect::new(
            node.rect.x + tx_off,
            node.rect.y + ty_off,
            node.rect.w,
            node.rect.h,
        );
        let inv_xform =
            if (node.style.scale - 1.0).abs() > 1e-4 || node.style.rotate_rad.abs() > 1e-4 {
                Some([
                    hit_bounds.x + hit_bounds.w * 0.5,
                    hit_bounds.y + hit_bounds.h * 0.5,
                    node.style.scale,
                    node.style.rotate_rad,
                ])
            } else {
                None
            };

        if node.hit_testable && !node.style.passthrough {
            let default_cursor = match node.role {
                Role::Button | Role::Toggle | Role::Dropdown => Some(CursorIcon::Pointer),
                Role::TextInput => Some(CursorIcon::Text),
                Role::SplitDivider => Some(CursorIcon::ResizeH),
                _ => None,
            };
            self.pending_hits.push(HitNode {
                id: node.id,
                bounds: hit_bounds,
                clip: active_clip,
                focusable: node.focusable,
                enabled: node.enabled,
                is_overlay,
                cursor: node.style.cursor.or(default_cursor),
                inv_xform,
                ripple: node.style.ripple.unwrap_or(self.theme.ripple),
            });
        }

        let ripple = self
            .ripples
            .iter()
            .rfind(|r| r.id == node.id)
            .map(|r| (r.center, r.progress));
        let shimmer_phase = if node.style.shimmer {
            Some(self.shimmer_phase)
        } else {
            None
        };

        let target_cmds = if is_overlay {
            &mut self.draw_list.overlay_cmds
        } else {
            &mut self.draw_list.base_cmds
        };

        let hovered = node.enabled && self.hovered_id == Some(node.id);
        let pressed = node.enabled && self.pressed_id == Some(node.id);
        let (border_width, border_color) = if focused && node.style.focus_bd.is_some() {
            (
                node.style.border_width.max(2.0),
                node.style.focus_bd.unwrap_or(self.theme.focus_ring),
            )
        } else if hovered && node.style.hover_bd.is_some() {
            (
                node.style.border_width.max(1.0),
                node.style.hover_bd.unwrap_or(node.style.border_color),
            )
        } else {
            (node.style.border_width, node.style.border_color)
        };
        let resolved_corners = node.style.corner_radii.unwrap_or([node.style.radius; 4]);
        let draw_radius = if node.style.shape == RectShape::Bevel {
            if node.style.corner_fillet > 0.0 {
                node.style.corner_fillet
            } else if node.style.corner_radii.is_some()
                && node.style.radius > 0.0
                && node.style.corner_radii != Some([node.style.radius; 4])
            {
                node.style.radius
            } else {
                0.0
            }
        } else if matches!(
            node.style.shape,
            RectShape::Hexagon | RectShape::HexagonPointy
        ) && node.style.corner_fillet > 0.0
        {
            node.style.corner_fillet
        } else {
            node.style.radius
        };

        let eff_opacity = node.style.opacity.clamp(0.0, 1.0);
        let mul_alpha = |c: Color| -> Color {
            if eff_opacity >= 0.999 {
                c
            } else {
                Color::rgba(c.r, c.g, c.b, (f32::from(c.a) * eff_opacity).round() as u8)
            }
        };
        let resolved_bg = if !node.enabled {
            node.style.disabled_bg.or(node.style.bg)
        } else if pressed {
            node.style
                .active_bg
                .or(node.style.hover_bg)
                .or(node.style.bg)
        } else if hovered {
            node.style.hover_bg.or(node.style.bg)
        } else {
            node.style.bg
        }
        .map(mul_alpha);
        let resolved_fg = if !node.enabled {
            node.style.disabled_fg.or(node.style.fg)
        } else if pressed {
            node.style
                .active_fg
                .or(node.style.hover_fg)
                .or(node.style.fg)
        } else if hovered {
            node.style.hover_fg.or(node.style.fg)
        } else {
            node.style.fg
        };

        let resolved_bg_grad = node.style.bg_grad.map(|mut g| {
            g.c0 = mul_alpha(g.c0);
            g.c1 = mul_alpha(g.c1);
            g
        });
        let resolved_bd_grad = node.style.bd_grad.map(|mut g| {
            g.c0 = mul_alpha(g.c0);
            g.c1 = mul_alpha(g.c1);
            g
        });

        let has_image = node.image_tex.is_some();
        if !has_image
            && (resolved_bg.is_some()
                || resolved_bg_grad.is_some()
                || resolved_bd_grad.is_some()
                || border_width > 0.0
                || node.style.shadow_blur > 0.0
                || node.style.glass
                || node.style.cursor_glow
                || node.style.shimmer)
        {
            let bg_draw = resolved_bg.unwrap_or(Color::TRANSPARENT);
            target_cmds.push(DrawCmd::Rect {
                rect: node.rect,
                bg: bg_draw,
                bg_grad: resolved_bg_grad,
                border_width,
                border_color: mul_alpha(border_color),
                bd_grad: resolved_bd_grad,
                radius: draw_radius,
                corner_radii: resolved_corners,
                shadow_blur: node.style.shadow_blur,
                shadow_color: mul_alpha(node.style.shadow_color),
                shape: node.style.shape,
                glass: node.style.glass,
                cursor_glow: node.style.cursor_glow,
                ripple,
                shimmer_phase,
                transform: xform,
            });
        } else if has_image
            && (resolved_bg.is_some() || resolved_bg_grad.is_some() || node.style.shadow_blur > 0.0)
        {
            target_cmds.push(DrawCmd::Rect {
                rect: node.rect,
                bg: resolved_bg.unwrap_or(Color::TRANSPARENT),
                bg_grad: resolved_bg_grad,
                border_width: 0.0,
                border_color: Color::TRANSPARENT,
                bd_grad: None,
                radius: draw_radius,
                corner_radii: resolved_corners,
                shadow_blur: node.style.shadow_blur,
                shadow_color: mul_alpha(node.style.shadow_color),
                shape: node.style.shape,
                glass: false,
                cursor_glow: false,
                ripple: None,
                shimmer_phase: None,
                transform: xform,
            });
        }

        if let Some([bl, br, bt, bb]) = node.style.border_sides {
            let bcol = mul_alpha(node.style.border_sides_color);
            let sides = [
                (bl, Rect::new(node.rect.x, node.rect.y, bl, node.rect.h)),
                (
                    br,
                    Rect::new(node.rect.x + node.rect.w - br, node.rect.y, br, node.rect.h),
                ),
                (bt, Rect::new(node.rect.x, node.rect.y, node.rect.w, bt)),
                (
                    bb,
                    Rect::new(node.rect.x, node.rect.y + node.rect.h - bb, node.rect.w, bb),
                ),
            ];
            for (sw, sr) in sides {
                if sw > 0.0 && sr.w > 0.0 && sr.h > 0.0 {
                    target_cmds.push(DrawCmd::Rect {
                        rect: sr,
                        bg: bcol,
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
                        transform: [1.0, 0.0, tx_off, ty_off],
                    });
                }
            }
        }

        let show_focus_ring = focused
            && !node.style.no_focus_ring
            && node.role != Role::Toggle
            && node.style.focus_bd.is_none()
            && (self.focus_visible
                || node.style.focus_ring_width.is_some()
                || node.style.focus_ring_color.is_some());
        if show_focus_ring {
            let ring_w = node
                .style
                .focus_ring_width
                .unwrap_or(self.theme.focus_ring_width)
                .max(0.0);
            let ring_col = node.style.focus_ring_color.unwrap_or(self.theme.focus_ring);
            let ring_off = node
                .style
                .focus_ring_offset
                .unwrap_or(self.theme.focus_ring_offset);
            if ring_w > 0.0 && ring_col.a > 0 {
                let ring_rect = Rect::new(
                    node.rect.x - ring_off,
                    node.rect.y - ring_off,
                    (node.rect.w + 2.0 * ring_off).max(0.0),
                    (node.rect.h + 2.0 * ring_off).max(0.0),
                );
                let pad_r = ring_off.max(0.0);
                let ring_corners = [
                    (resolved_corners[0] + pad_r).max(0.0),
                    (resolved_corners[1] + pad_r).max(0.0),
                    (resolved_corners[2] + pad_r).max(0.0),
                    (resolved_corners[3] + pad_r).max(0.0),
                ];
                let ring_radius = (draw_radius + pad_r).max(0.0);
                target_cmds.push(DrawCmd::Rect {
                    rect: ring_rect,
                    bg: Color::TRANSPARENT,
                    bg_grad: None,
                    border_width: ring_w,
                    border_color: mul_alpha(ring_col),
                    bd_grad: None,
                    radius: ring_radius,
                    corner_radii: ring_corners,
                    shadow_blur: 0.0,
                    shadow_color: Color::TRANSPARENT,
                    shape: node.style.shape,
                    glass: false,
                    cursor_glow: false,
                    ripple: None,
                    shimmer_phase: None,
                    transform: xform,
                });
            }
        }

        let avail_w = (node.rect.w - node.style.pad_l - node.style.pad_r).max(0.0);
        let avail_h = (node.rect.h - node.style.pad_t - node.style.pad_b).max(0.0);
        let tx = if node.role == Role::Button && node.content_w > 0.0 && node.content_w < avail_w {
            node.rect.x + node.style.pad_l + (avail_w - node.content_w) * 0.5
        } else {
            node.rect.x + node.style.pad_l
        };
        let ty = if (node.role == Role::Button
            || (!node.style.wrap && matches!(node.role, Role::Label | Role::TextInput)))
            && node.content_h > 0.0
            && node.content_h < avail_h
        {
            node.rect.y + node.style.pad_t + (avail_h - node.content_h) * 0.5
        } else {
            node.rect.y + node.style.pad_t
        };
        let tw = (node.rect.x + node.rect.w - node.style.pad_r - tx).max(0.0);
        let th = (node.rect.y + node.rect.h - node.style.pad_b - ty).max(0.0);
        let text_rect = Rect::new(tx, ty, tw, th).snap(self.scale_factor);
        let font_sz = node.style.font_size.unwrap_or(self.theme.font_size);
        let font_weight = node.style.font_weight.unwrap_or(400);

        let caret_rect = if let (Some((cid, mut byte_pos)), Some(txt)) =
            (self.focused_caret, node.text.as_deref())
        {
            if cid == node.id {
                byte_pos = byte_pos.min(txt.len());
                while byte_pos > 0 && !txt.is_char_boundary(byte_pos) {
                    byte_pos -= 1;
                }
                let prefix_all = &txt[..byte_pos];
                let (line_prefix, line_idx) = if node.style.wrap {
                    let start = prefix_all.rfind('\n').map_or(0, |i| i + 1);
                    let count = prefix_all.bytes().filter(|&b| b == b'\n').count();
                    (&prefix_all[start..], count)
                } else {
                    (prefix_all, 0)
                };
                let advance_x = if line_prefix.is_empty() {
                    0.0
                } else if let Some(m) = measurer.as_deref_mut() {
                    (m.measure_plain_weighted(
                        line_prefix,
                        font_sz,
                        font_weight,
                        None,
                        false,
                        false,
                    )[0] - 2.0)
                        .max(0.0)
                } else {
                    line_prefix.chars().count() as f32 * font_sz * 0.6
                };
                let cx = (text_rect.x + advance_x).clamp(
                    text_rect.x,
                    (text_rect.x + text_rect.w - 2.0).max(text_rect.x),
                );
                let cy = (text_rect.y + (line_idx as f32) * (font_sz * 1.35)).clamp(
                    text_rect.y,
                    (text_rect.y + text_rect.h - font_sz).max(text_rect.y),
                );
                let ch = (font_sz * 1.18).min(text_rect.h.max(font_sz));
                let crect = Rect::new(cx, cy, 2.0, ch).snap(self.scale_factor);
                self.prev_caret_rects.insert(node.id, crect);
                Some(crect)
            } else {
                None
            }
        } else {
            None
        };

        let target_cmds = if is_overlay {
            &mut self.draw_list.overlay_cmds
        } else {
            &mut self.draw_list.base_cmds
        };

        if let Some(tex) = node.image_tex {
            let box_rect = Rect::new(
                node.rect.x + node.style.pad_l,
                node.rect.y + node.style.pad_t,
                avail_w,
                avail_h,
            );
            let [src_w, src_h] = node
                .style
                .image_size
                .or_else(|| self.texture_sizes.get(&tex).copied())
                .map_or([box_rect.w.max(1.0), box_rect.h.max(1.0)], |[w, h]| {
                    [(w as f32).max(1.0), (h as f32).max(1.0)]
                });
            let (draw_rect, uv) =
                compute_image_fit_rect_uv(box_rect, src_w, src_h, node.style.image_fit);
            target_cmds.push(DrawCmd::Image {
                rect: draw_rect.snap(self.scale_factor),
                tex,
                filter: node.style.image_filter,
                uv,
                tint: node.style.image_tint,
                border_width,
                border_color,
                radius: draw_radius,
                corner_radii: resolved_corners,
                shape: node.style.shape,
                transform: xform,
            });
        }

        if let Some(samples) = node.sparkline_samples {
            let fg_col = node.style.fg.unwrap_or(self.theme.primary);
            let fill_color = node.style.fill_color.or_else(|| {
                if node.style.fill_auto {
                    Some(Color::rgba(
                        fg_col.r,
                        fg_col.g,
                        fg_col.b,
                        (fg_col.a / 5).max(16),
                    ))
                } else {
                    None
                }
            });
            target_cmds.push(DrawCmd::Sparkline {
                rect: text_rect,
                samples,
                color: fg_col,
                fill_color,
                stroke_width: node.style.stroke_width,
            });
        }

        if let Some((mut canvas, scene)) = node.viewport_data {
            if node.style.svg_fg_tint {
                let fg_col = node.style.fg.unwrap_or(self.theme.text);
                for cmd in &mut canvas {
                    match cmd {
                        Canvas2DCmd::Tri { color, .. }
                        | Canvas2DCmd::Segment { color, .. }
                        | Canvas2DCmd::Rect { color, .. } => {
                            *color = fg_col;
                        }
                        _ => {}
                    }
                }
            }
            target_cmds.push(DrawCmd::Viewport {
                rect: node.rect,
                canvas,
                scene,
            });
        }

        if let Some((fid, s0_raw, s1_raw)) = self.focused_selection {
            if fid == node.id && s1_raw > s0_raw {
                if let Some(ref txt) = node.text {
                    let s0_char = txt[..s0_raw.min(txt.len())].chars().count();
                    let s1_char = txt[..s1_raw.min(txt.len())].chars().count();
                    let pcol = self.theme.primary;
                    let sel_bg = node
                        .style
                        .selection_bg
                        .unwrap_or_else(|| Color::rgba(pcol.r, pcol.g, pcol.b, 85));
                    emit_text_selection_rects(
                        target_cmds,
                        text_rect,
                        txt,
                        s0_char,
                        s1_char,
                        font_sz,
                        node.style.line_height,
                        sel_bg,
                    );
                }
            }
        }

        if node.style.selectable {
            let full_text: Option<String> = if let Some(ref t) = node.text {
                Some(t.clone())
            } else {
                node.rich_spans
                    .as_ref()
                    .map(|spans| spans.iter().map(|s| s.text.as_str()).collect::<String>())
            };
            if let Some(full) = full_text {
                let char_count = full.chars().count();
                let char_w = (font_sz * 0.56).max(1.0);
                let line_h = (font_sz * node.style.line_height.max(0.5)).max(1.0);
                let idx_at = |px: f32, py: f32| -> usize {
                    let row = ((py - text_rect.y) / line_h).floor().max(0.0) as usize;
                    let col = ((px - text_rect.x) / char_w).round().max(0.0) as usize;
                    if full.contains('\n') {
                        let mut idx = 0usize;
                        for (cur_row, line) in full.split('\n').enumerate() {
                            let len = line.chars().count();
                            if cur_row == row {
                                return (idx + col.min(len)).min(char_count);
                            }
                            idx += len + 1;
                        }
                        char_count
                    } else {
                        let cols_per_line = (text_rect.w / char_w).floor().max(1.0) as usize;
                        (row * cols_per_line + col).min(char_count)
                    }
                };
                if self.double_clicked_id == Some(node.id) {
                    self.text_select_anchor = Some((node.id, 0));
                    self.text_select_range = Some((node.id, 0, char_count));
                } else if self.just_pressed_id == Some(node.id) {
                    if let Some([px, py]) = self.pointer_pos {
                        let i = idx_at(px, py);
                        self.text_select_anchor = Some((node.id, i));
                        self.text_select_range = Some((node.id, i, i));
                    }
                } else if self.pressed_id == Some(node.id) {
                    if let (Some((aid, anchor)), Some([px, py])) =
                        (self.text_select_anchor, self.pointer_pos)
                    {
                        if aid == node.id {
                            let cur = idx_at(px, py);
                            self.text_select_range =
                                Some((node.id, anchor.min(cur), anchor.max(cur)));
                        }
                    }
                }
                if let Some((sid, s0, s1)) = self.text_select_range {
                    if sid == node.id && s1 > s0 {
                        let copy_pressed = self
                            .pending_key_events
                            .iter()
                            .any(|(k, m)| m.ctrl && matches!(k, Key::Char('c') | Key::Char('C')));
                        if copy_pressed && self.focused_id == Some(node.id) {
                            let slice: String = full.chars().skip(s0).take(s1 - s0).collect();
                            crate::os::clipboard::set_text(&slice);
                        }
                        let pcol = self.theme.primary;
                        let sel_bg = node
                            .style
                            .selection_bg
                            .unwrap_or_else(|| Color::rgba(pcol.r, pcol.g, pcol.b, 85));
                        emit_text_selection_rects(
                            target_cmds,
                            text_rect,
                            &full,
                            s0,
                            s1,
                            font_sz,
                            node.style.line_height,
                            sel_bg,
                        );
                    }
                }
            }
        }

        if let Some(text) = node.text {
            if text.is_empty() {
                if let Some(ph) = node.style.placeholder {
                    if !ph.is_empty() {
                        target_cmds.push(DrawCmd::Text {
                            rect: text_rect,
                            text: ph,
                            color: self.theme.text_muted,
                            size: font_sz,
                            weight: font_weight,
                            mono: node.style.mono,
                            font_family: node.style.font_family.clone(),
                            wrap: node.style.wrap,
                            ellipsis: true,
                            align: node.style.text_align,
                            tracking: node.style.tracking,
                            line_height: node.style.line_height,
                            transform: xform,
                        });
                    }
                }
            } else {
                target_cmds.push(DrawCmd::Text {
                    rect: text_rect,
                    text,
                    color: mul_alpha(resolved_fg.unwrap_or(self.theme.text)),
                    size: font_sz,
                    weight: font_weight,
                    mono: node.style.mono,
                    font_family: node.style.font_family.clone(),
                    wrap: node.style.wrap,
                    ellipsis: node.style.ellipsis,
                    align: node.style.text_align,
                    tracking: node.style.tracking,
                    line_height: node.style.line_height,
                    transform: xform,
                });
            }
        }

        if let Some(crect) = caret_rect {
            target_cmds.push(DrawCmd::Rect {
                rect: crect,
                bg: self.theme.primary,
                bg_grad: None,
                border_width: 0.0,
                border_color: Color::TRANSPARENT,
                bd_grad: None,
                radius: 1.0,
                corner_radii: [1.0; 4],
                shadow_blur: 0.0,
                shadow_color: Color::TRANSPARENT,
                shape: node.style.shape,
                glass: false,
                cursor_glow: false,
                ripple: None,
                shimmer_phase: None,
                transform: [1.0, 0.0, 0.0, 0.0],
            });
        }

        if let Some(spans) = node.rich_spans {
            let link_rects: Vec<(usize, Rect)> = if let Some(m) = measurer.as_deref_mut() {
                m.measure_rich_link_rects(
                    &spans,
                    text_rect,
                    font_sz,
                    node.style.font_family.as_deref(),
                    node.style.wrap,
                    node.style.text_align,
                    node.style.tracking,
                    node.style.line_height,
                )
            } else {
                let char_w = (font_sz * 0.58).max(1.0);
                let lh = (font_sz * node.style.line_height.max(0.5)).max(1.0);
                let mut cursor_x = text_rect.x;
                let mut cursor_y = text_rect.y;
                let max_x = if node.style.wrap && text_rect.w > 0.0 {
                    text_rect.x + text_rect.w
                } else {
                    f32::INFINITY
                };
                let mut out = Vec::new();
                for (si, span) in spans.iter().enumerate() {
                    let sw = span.text.chars().count() as f32 * char_w;
                    if node.style.wrap && cursor_x > text_rect.x && cursor_x + sw > max_x {
                        cursor_x = text_rect.x;
                        cursor_y += lh;
                    }
                    if span.kind == SpanKind::Link {
                        out.push((si, Rect::new(cursor_x, cursor_y, sw.max(8.0), lh)));
                    }
                    cursor_x += sw;
                }
                out
            };

            let mut link_idx = 0usize;
            for (si, lrect_raw) in link_rects {
                if let (Some(span), Some((link_wid, _))) =
                    (spans.get(si), node.rich_links.get(link_idx))
                {
                    let lrect = lrect_raw.snap(self.scale_factor);
                    let link_focused = self.focused_id == Some(*link_wid);
                    self.pending_semantics.push(SemanticNode {
                        id: *link_wid,
                        role: Role::Button,
                        label: span.text.clone(),
                        value: None,
                        bounds: lrect,
                        focused: link_focused,
                        enabled: true,
                        focusable: true,
                        is_overlay,
                    });
                    self.pending_hits.push(HitNode {
                        id: *link_wid,
                        bounds: lrect,
                        clip: active_clip,
                        focusable: true,
                        enabled: true,
                        is_overlay,
                        cursor: Some(CursorIcon::Pointer),
                        inv_xform: None,
                        ripple: false,
                    });
                    link_idx += 1;
                }
            }

            let target_cmds = if is_overlay {
                &mut self.draw_list.overlay_cmds
            } else {
                &mut self.draw_list.base_cmds
            };
            target_cmds.push(DrawCmd::RichText {
                rect: text_rect,
                spans,
                default_color: mul_alpha(resolved_fg.unwrap_or(self.theme.text)),
                code_bg: self.theme.surface_alt,
                mention_bg: Color::rgba(
                    self.theme.primary.r,
                    self.theme.primary.g,
                    self.theme.primary.b,
                    55,
                ),
                link_color: self.theme.primary,
                size: font_sz,
                font_family: node.style.font_family,
                wrap: node.style.wrap,
                ellipsis: node.style.ellipsis,
                align: node.style.text_align,
                tracking: node.style.tracking,
                line_height: node.style.line_height,
                transform: xform,
            });
        }

        let should_clip = node.is_scroll || node.style.clip;
        let child_clip = if should_clip {
            let clip_box = if node.is_scroll { text_rect } else { node.rect };
            let clip_rect = match active_clip {
                Some(prev) => prev.intersect(clip_box),
                None => clip_box,
            };
            let target_cmds = if is_overlay {
                &mut self.draw_list.overlay_cmds
            } else {
                &mut self.draw_list.base_cmds
            };
            target_cmds.push(DrawCmd::PushClip(clip_rect));
            Some(clip_rect)
        } else {
            active_clip
        };

        let [x_scale, rot_rad, _, _] = xform;
        let has_xform = (x_scale - 1.0).abs() > 0.001
            || rot_rad.abs() > 0.0001
            || tx_off.abs() > 0.001
            || ty_off.abs() > 0.001;
        let state_fg_override = (node.role == Role::Button
            && (!node.enabled || pressed || hovered)
            && (node.style.disabled_fg.is_some()
                || node.style.active_fg.is_some()
                || node.style.hover_fg.is_some()))
        .then_some(resolved_fg)
        .flatten();
        let mut sticky_children = Vec::new();
        self.flush_interacts(idx, 0, child_clip);
        for (child_pos, cid) in node.children.into_iter().enumerate() {
            if eff_opacity < 0.999 {
                let co = nodes[cid].style.opacity;
                nodes[cid].style.opacity = (co * eff_opacity).clamp(0.0, 1.0);
            }
            if let Some(fg_col) = state_fg_override {
                if nodes[cid].role == Role::Label {
                    nodes[cid].style.fg = Some(fg_col);
                }
            }
            if has_xform {
                let p_cx = node.rect.x + node.rect.w * 0.5;
                let p_cy = node.rect.y + node.rect.h * 0.5;
                let c_cx = nodes[cid].rect.x + nodes[cid].rect.w * 0.5;
                let c_cy = nodes[cid].rect.y + nodes[cid].rect.h * 0.5;
                let dx = c_cx - p_cx;
                let dy = c_cy - p_cy;
                let (sn, cs) = rot_rad.sin_cos();
                let rx = (dx * cs - dy * sn) * x_scale + p_cx;
                let ry = (dx * sn + dy * cs) * x_scale + p_cy;
                nodes[cid].style.translate[0] += (rx - c_cx) + tx_off;
                nodes[cid].style.translate[1] += (ry - c_cy) + ty_off;
                nodes[cid].style.scale *= x_scale;
                nodes[cid].style.rotate_rad += rot_rad;
            }
            if node.is_scroll && nodes[cid].style.sticky_top.is_some() {
                sticky_children.push(cid);
            } else {
                self.emit_node(nodes, cid, is_overlay, child_clip, measurer);
            }
            self.flush_interacts(idx, child_pos + 1, child_clip);
        }
        for cid in sticky_children {
            self.emit_node(nodes, cid, is_overlay, child_clip, measurer);
        }

        if should_clip {
            let target_cmds = if is_overlay {
                &mut self.draw_list.overlay_cmds
            } else {
                &mut self.draw_list.base_cmds
            };
            target_cmds.push(DrawCmd::PopClip);
        }

        if node.is_scroll && !node.style.no_scrollbar {
            if let Some(sc) = self.scroll_states.get(&node.id).copied() {
                let sb_id = scrollbar_widget_id(node.id);
                let sb_active = self.hovered_id == Some(sb_id) || self.pressed_id == Some(sb_id);
                let track_col = mul_alpha(Color::rgba(
                    self.theme.border.r,
                    self.theme.border.g,
                    self.theme.border.b,
                    70,
                ));
                let thumb_base = if sb_active {
                    self.theme.primary
                } else {
                    Color::rgba(
                        self.theme.text_muted.r,
                        self.theme.text_muted.g,
                        self.theme.text_muted.b,
                        165,
                    )
                };
                let thumb_col = mul_alpha(node.style.scrollbar_thumb.unwrap_or(thumb_base));
                let track_col = node
                    .style
                    .scrollbar_track
                    .map(mul_alpha)
                    .unwrap_or(track_col);
                let sb_size = node.style.scrollbar_w.unwrap_or(8.0).max(4.0);

                if node.kind == AxisKind::Col
                    && sc.max_y > 0.5
                    && node.rect.h > 16.0
                    && node.rect.w > 16.0
                {
                    let track_w = sb_size;
                    let track_rect = Rect::new(
                        node.rect.x + node.rect.w - track_w - 2.0,
                        node.rect.y + 2.0,
                        track_w,
                        (node.rect.h - 4.0).max(8.0),
                    )
                    .snap(self.scale_factor);
                    let ratio = (sc.inner_h / sc.total_h.max(1.0)).clamp(0.05, 1.0);
                    let thumb_h = (track_rect.h * ratio).clamp(20.0, track_rect.h);
                    let t = (sc.spring.value / sc.max_y).clamp(0.0, 1.0);
                    let thumb_y = track_rect.y + t * (track_rect.h - thumb_h);
                    let thumb_rect = Rect::new(
                        track_rect.x + 1.0,
                        thumb_y,
                        (track_w - 2.0).max(2.0),
                        thumb_h,
                    )
                    .snap(self.scale_factor);

                    self.pending_scrollbar_rects
                        .insert(node.id, (track_rect, thumb_rect));
                    self.pending_hits.push(HitNode {
                        id: sb_id,
                        bounds: track_rect,
                        clip: active_clip,
                        focusable: false,
                        enabled: node.enabled,
                        is_overlay,
                        cursor: Some(CursorIcon::Pointer),
                        inv_xform: None,
                        ripple: false,
                    });
                    let target_cmds = if is_overlay {
                        &mut self.draw_list.overlay_cmds
                    } else {
                        &mut self.draw_list.base_cmds
                    };
                    for (r, bg, rad) in [
                        (track_rect, track_col, sb_size * 0.5),
                        (thumb_rect, thumb_col, (sb_size - 2.0).max(2.0) * 0.5),
                    ] {
                        target_cmds.push(DrawCmd::Rect {
                            rect: r,
                            bg,
                            bg_grad: None,
                            border_width: 0.0,
                            border_color: Color::TRANSPARENT,
                            bd_grad: None,
                            radius: rad,
                            corner_radii: [rad; 4],
                            shadow_blur: 0.0,
                            shadow_color: Color::TRANSPARENT,
                            shape: RectShape::Box,
                            glass: false,
                            cursor_glow: false,
                            ripple: None,
                            shimmer_phase: None,
                            transform: [1.0, 0.0, 0.0, 0.0],
                        });
                    }
                } else if node.kind == AxisKind::Row
                    && sc.max_x > 0.5
                    && node.rect.w > 16.0
                    && node.rect.h > 16.0
                {
                    let track_h = sb_size;
                    let track_rect = Rect::new(
                        node.rect.x + 2.0,
                        node.rect.y + node.rect.h - track_h - 2.0,
                        (node.rect.w - 4.0).max(8.0),
                        track_h,
                    )
                    .snap(self.scale_factor);
                    let ratio = (sc.inner_w / sc.total_w.max(1.0)).clamp(0.05, 1.0);
                    let thumb_w = (track_rect.w * ratio).clamp(20.0, track_rect.w);
                    let t = (sc.spring_x.value / sc.max_x).clamp(0.0, 1.0);
                    let thumb_x = track_rect.x + t * (track_rect.w - thumb_w);
                    let thumb_rect = Rect::new(
                        thumb_x,
                        track_rect.y + 1.0,
                        thumb_w,
                        (track_h - 2.0).max(2.0),
                    )
                    .snap(self.scale_factor);

                    self.pending_scrollbar_rects
                        .insert(node.id, (track_rect, thumb_rect));
                    self.pending_hits.push(HitNode {
                        id: sb_id,
                        bounds: track_rect,
                        clip: active_clip,
                        focusable: false,
                        enabled: node.enabled,
                        is_overlay,
                        cursor: Some(CursorIcon::Pointer),
                        inv_xform: None,
                        ripple: false,
                    });
                    let target_cmds = if is_overlay {
                        &mut self.draw_list.overlay_cmds
                    } else {
                        &mut self.draw_list.base_cmds
                    };
                    for (r, bg, rad) in [
                        (track_rect, track_col, sb_size * 0.5),
                        (thumb_rect, thumb_col, (sb_size - 2.0).max(2.0) * 0.5),
                    ] {
                        target_cmds.push(DrawCmd::Rect {
                            rect: r,
                            bg,
                            bg_grad: None,
                            border_width: 0.0,
                            border_color: Color::TRANSPARENT,
                            bd_grad: None,
                            radius: rad,
                            corner_radii: [rad; 4],
                            shadow_blur: 0.0,
                            shadow_color: Color::TRANSPARENT,
                            shape: RectShape::Box,
                            glass: false,
                            cursor_glow: false,
                            ripple: None,
                            shimmer_phase: None,
                            transform: [1.0, 0.0, 0.0, 0.0],
                        });
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_text_selection_rects(
    target_cmds: &mut Vec<DrawCmd>,
    text_rect: Rect,
    full: &str,
    s0: usize,
    s1: usize,
    font_sz: f32,
    line_height_mult: f32,
    sel_bg: Color,
) {
    if s1 <= s0 || text_rect.w <= 0.0 {
        return;
    }
    let char_w = (font_sz * 0.56).max(1.0);
    let line_h = (font_sz * line_height_mult.max(0.5)).max(1.0);
    let mut push_rect = |rx: f32, ry: f32, rw: f32| {
        if rw <= 0.5 {
            return;
        }
        target_cmds.push(DrawCmd::Rect {
            rect: Rect::new(rx, ry, rw, line_h.min(text_rect.h.max(line_h))),
            bg: sel_bg,
            bg_grad: None,
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
            bd_grad: None,
            radius: 2.0,
            corner_radii: [2.0; 4],
            shadow_blur: 0.0,
            shadow_color: Color::TRANSPARENT,
            shape: RectShape::Box,
            glass: false,
            cursor_glow: false,
            ripple: None,
            shimmer_phase: None,
            transform: [1.0, 0.0, 0.0, 0.0],
        });
    };

    if full.contains('\n') {
        let mut line_start = 0usize;
        for (row, line) in full.split('\n').enumerate() {
            let line_len = line.chars().count();
            let line_end = line_start + line_len;
            let ov_start = s0.max(line_start);
            let ov_end = s1.min(line_end);
            if ov_end > ov_start {
                let ry = text_rect.y + (row as f32) * line_h;
                if ry < text_rect.y + text_rect.h.max(line_h) {
                    let sx =
                        text_rect.x + ((ov_start - line_start) as f32 * char_w).min(text_rect.w);
                    let sw = ((ov_end - ov_start) as f32 * char_w)
                        .min((text_rect.x + text_rect.w - sx).max(0.0));
                    push_rect(sx, ry, sw);
                }
            }
            line_start = line_end + 1;
        }
    } else {
        let cols_per_line = (text_rect.w / char_w).floor().max(1.0) as usize;
        let start_row = s0 / cols_per_line;
        let end_row = (s1 - 1) / cols_per_line;
        for row in start_row..=end_row {
            let row_start = row * cols_per_line;
            let row_end = row_start + cols_per_line;
            let ov_start = s0.max(row_start);
            let ov_end = s1.min(row_end);
            if ov_end > ov_start {
                let ry = text_rect.y + (row as f32) * line_h;
                let sx = text_rect.x + ((ov_start - row_start) as f32 * char_w).min(text_rect.w);
                let sw = ((ov_end - ov_start) as f32 * char_w)
                    .min((text_rect.x + text_rect.w - sx).max(0.0));
                push_rect(sx, ry, sw);
            }
        }
    }
}

fn compute_image_fit_rect_uv(
    box_rect: Rect,
    src_w: f32,
    src_h: f32,
    fit: ImageFit,
) -> (Rect, [f32; 4]) {
    if box_rect.w <= 0.0 || box_rect.h <= 0.0 || src_w <= 0.0 || src_h <= 0.0 {
        return (box_rect, [0.0, 0.0, 1.0, 1.0]);
    }
    match fit {
        ImageFit::Fill => (box_rect, [0.0, 0.0, 1.0, 1.0]),
        ImageFit::Contain => {
            let scale = (box_rect.w / src_w).min(box_rect.h / src_h);
            let dw = src_w * scale;
            let dh = src_h * scale;
            let dx = box_rect.x + (box_rect.w - dw) * 0.5;
            let dy = box_rect.y + (box_rect.h - dh) * 0.5;
            (Rect::new(dx, dy, dw, dh), [0.0, 0.0, 1.0, 1.0])
        }
        ImageFit::Cover => {
            let src_aspect = src_w / src_h;
            let box_aspect = box_rect.w / box_rect.h;
            let uv = if src_aspect > box_aspect {
                let u_span = (box_aspect / src_aspect).clamp(0.0, 1.0);
                let u0 = (1.0 - u_span) * 0.5;
                [u0, 0.0, 1.0 - u0, 1.0]
            } else {
                let v_span = (src_aspect / box_aspect).clamp(0.0, 1.0);
                let v0 = (1.0 - v_span) * 0.5;
                [0.0, v0, 1.0, 1.0 - v0]
            };
            (box_rect, uv)
        }
    }
}
