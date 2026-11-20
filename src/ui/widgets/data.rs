use super::*;

/// Column descriptor for [`UiContext::table`] / [`UiContext::data_grid`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TableColumn<'a> {
    /// Column header caption text.
    pub header: &'a str,
    /// Initial column width in logical points.
    pub width: f32,
    /// Whether clicking the header cycles sort direction.
    pub sortable: bool,
    /// Whether dragging the column divider resizes `col_widths`.
    pub resizable: bool,
}

impl<'a> TableColumn<'a> {
    /// Creates a table column with default sortable and resizable behavior enabled.
    pub const fn new(header: &'a str, width: f32) -> Self {
        Self {
            header,
            width,
            sortable: true,
            resizable: true,
        }
    }

    /// Enables or disables click-to-sort on this column header.
    pub const fn sortable(mut self, sortable: bool) -> Self {
        self.sortable = sortable;
        self
    }

    /// Enables or disables drag resizing on this column header.
    pub const fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }
}

/// Persistent interactive state for [`UiContext::table`] / [`UiContext::data_grid`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableState {
    /// Currently sorted column index, if any.
    pub sort_col: Option<usize>,
    /// Sort order (`true` ascending, `false` descending).
    pub sort_asc: bool,
    /// Current per-column widths persisted across drag resizes.
    pub col_widths: Vec<f32>,
    /// Currently selected row index, if any.
    pub selected_row: Option<usize>,
    /// Optional header row height override in logical points (defaults to `32.0`).
    pub header_height: Option<f32>,
    /// Optional body row height override in logical points (defaults to `30.0`).
    pub row_height: Option<f32>,
    /// Optional alternating zebra row background color.
    pub zebra_bg: Option<Color>,
    /// Whether to draw 1px vertical column dividers between header and body cells.
    pub col_dividers: bool,
}

impl UiContext {
    /// GPU-accelerated `sdSegment` sparkline curve over normalized `samples`.
    ///
    /// # Applicable Modifiers
    /// - `.fg(color)` (stroke color)
    /// - `.stroke_width(w)` (curve thickness in points)
    /// - `.filled()` / `.fill(color)` (area fill beneath the curve)
    /// - `.bg(color)`, `.bd(w, color)`, `.r(radius)`, `.p(pad)`, `.w(w)`, `.h(h)`, `.grow(weight)`
    pub fn sparkline(&mut self, samples: &[f32]) -> StyleBuilder<'_> {
        let ordinal = self
            .container_stack
            .last()
            .map_or(0, |&p| self.nodes[p].children.len());
        self.ids.push_u64(ordinal as u64);
        let id = self.ids.id("sparkline");
        self.ids.pop_id();

        let idx = self.alloc_node(
            LayoutNode {
                id,
                role: Role::Label,
                kind: AxisKind::Leaf,
                style: Style {
                    fg: Some(self.theme.primary),
                    ..Style::default()
                },
                label: String::from("Sparkline"),
                semantic_value: None,
                text: None,
                rich_spans: None,
                rich_links: Vec::new(),
                sparkline_samples: Some(samples.to_vec()),
                image_tex: None,
                viewport_data: None,
                is_scroll: false,
                scroll_offset_y: 0.0,
                focusable: false,
                hit_testable: false,
                enabled: true,
                is_overlay: false,
                children: Vec::new(),
                intrinsic_w: 180.0,
                intrinsic_h: 40.0,
                content_w: 180.0,
                content_h: 40.0,
                rect: Rect::ZERO,
            },
            true,
        );

        StyleBuilder {
            ui: self,
            node_idx: idx,
            changed: false,
        }
    }

    /// Virtualized fixed-height scrollable list rendering only rows intersecting the viewport.
    pub fn vlist(
        &mut self,
        id_label: &str,
        count: usize,
        row_height: f32,
        mut row_fn: impl FnMut(&mut UiContext, usize),
    ) -> StyleBuilder<'_> {
        let rh = row_height.max(1.0);
        let scroll_id = self.ids.id(id_label);
        let scroll_y = self
            .scroll_states
            .get(&scroll_id)
            .map_or(0.0, |s| s.spring.value.max(0.0));
        let fallback_h = self.viewport_h.max(240.0);
        let viewport_h = self.prev_rects.get(&scroll_id).map_or(fallback_h, |r| {
            if r.h > 1.0 {
                r.h
            } else {
                fallback_h
            }
        });
        let start = ((scroll_y / rh).floor() as usize).min(count);
        let end = (((scroll_y + viewport_h) / rh).ceil() as usize)
            .max(start)
            .min(count);
        let top_h = start as f32 * rh;
        let bottom_h = count.saturating_sub(end) as f32 * rh;

        self.scroll(id_label, |ui| {
            emit_vlist_spacer(ui, "__vlist_top__", top_h);
            for i in start..end {
                ui.ids.push_u64(i as u64);
                ui.col(|ui| row_fn(ui, i)).h(rh);
                ui.ids.pop_id();
            }
            emit_vlist_spacer(ui, "__vlist_bottom__", bottom_h);
        })
    }

    /// Virtualized variable-height scrollable list using cumulative heights and `O(log N)` binary search.
    pub fn vlist_dynamic(
        &mut self,
        id_label: &str,
        count: usize,
        est_h: f32,
        mut height_fn: impl FnMut(usize) -> f32,
        mut row_fn: impl FnMut(&mut UiContext, usize),
    ) -> StyleBuilder<'_> {
        let fallback_row = est_h.max(1.0);
        let mut prefix = core::mem::take(&mut self.vlist_prefix_scratch);
        prefix.clear();
        prefix.reserve(count.saturating_add(1));
        prefix.push(0.0_f32);
        let mut acc = 0.0_f32;
        for i in 0..count {
            let h = height_fn(i);
            let rh = if h.is_finite() && h > 0.0 {
                h
            } else {
                fallback_row
            };
            acc += rh;
            prefix.push(acc);
        }

        let scroll_id = self.ids.id(id_label);
        let scroll_y = self
            .scroll_states
            .get(&scroll_id)
            .map_or(0.0, |s| s.spring.value.max(0.0));
        let fallback_h = self.viewport_h.max(240.0);
        let viewport_h = self.prev_rects.get(&scroll_id).map_or(fallback_h, |r| {
            if r.h > 1.0 {
                r.h
            } else {
                fallback_h
            }
        });

        let (start, end) = vlist_range_binary_search(&prefix, scroll_y, viewport_h);
        let top_h = prefix.get(start).copied().unwrap_or(0.0);
        let total_h = prefix.last().copied().unwrap_or(0.0);
        let bottom_h = (total_h - prefix.get(end).copied().unwrap_or(total_h)).max(0.0);

        let builder = self.scroll(id_label, |ui| {
            emit_vlist_spacer(ui, "__vlist_top__", top_h);
            for i in start..end {
                let rh = (prefix[i + 1] - prefix[i]).max(1.0);
                ui.ids.push_u64(i as u64);
                ui.col(|ui| row_fn(ui, i)).h(rh);
                ui.ids.pop_id();
            }
            emit_vlist_spacer(ui, "__vlist_bottom__", bottom_h);
        });
        let node_idx = builder.node_idx;
        let changed = builder.changed;
        let _ = builder;
        self.vlist_prefix_scratch = prefix;
        StyleBuilder {
            ui: self,
            node_idx,
            changed,
        }
    }

    /// Embedded custom 2D/3D viewport backed by `Canvas2D` primitives and a persistent `Scene3D` graph.
    ///
    /// # Applicable Modifiers
    /// - `.passthrough()` (lets pointer clicks/scroll pass through to overlapping UI)
    /// - `.w(w)`, `.h(h)`, `.w_frac(f)`, `.h_frac(f)`, `.aspect(ratio)`, `.grow(weight)`
    /// - `.bg(color)`, `.bd(w, color)`, `.r(radius)`, `.clip()`
    pub fn viewport(
        &mut self,
        id_label: &str,
        f: impl FnOnce(&mut Canvas2D, &mut Scene3D),
    ) -> StyleBuilder<'_> {
        let id = self.ids.id(id_label);
        let raw_fbo = self.viewport_raw_fbo_id;
        let scene = self.viewport_scenes.entry(id).or_default();
        scene.begin_frame();
        scene.set_raw_fbo_id(raw_fbo);

        let prev_size = self
            .prev_rects
            .get(&id)
            .map_or([320.0, 200.0], |r| [r.w.max(1.0), r.h.max(1.0)]);
        let mut canvas = Canvas2D::new([0.0, 0.0]);
        canvas.size = prev_size;
        f(&mut canvas, scene);
        let snapshot = scene.snapshot();

        let idx = self.alloc_node(
            LayoutNode {
                id,
                role: Role::Viewport,
                kind: AxisKind::Leaf,
                style: Style {
                    radius: self.theme.radius,
                    border_width: 1.0,
                    border_color: self.theme.border,
                    ..Style::default()
                },
                label: String::from(id_label),
                semantic_value: None,
                text: None,
                rich_spans: None,
                rich_links: Vec::new(),
                sparkline_samples: None,
                image_tex: None,
                viewport_data: Some((canvas.cmds, snapshot)),
                is_scroll: false,
                scroll_offset_y: 0.0,
                focusable: false,
                hit_testable: true,
                enabled: true,
                is_overlay: false,
                children: Vec::new(),
                intrinsic_w: 320.0,
                intrinsic_h: 200.0,
                content_w: 320.0,
                content_h: 200.0,
                rect: Rect::ZERO,
            },
            true,
        );

        StyleBuilder {
            ui: self,
            node_idx: idx,
            changed: false,
        }
    }

    /// Virtualized table / data grid with sticky sortable and resizable headers.
    ///
    /// `cell_fn(ui, row_index, col_index)` renders each cell's content inside a clipped fixed-width cell.
    /// Returns `StyleBuilder` with `.changed()` when the sort column or direction changes.
    pub fn table(
        &mut self,
        id_label: &str,
        columns: &[TableColumn<'_>],
        state: &mut TableState,
        row_count: usize,
        cell_fn: impl FnMut(&mut UiContext, usize, usize),
    ) -> StyleBuilder<'_> {
        let primary = self.theme.primary;
        let text_col = self.theme.text;
        let sort_asc = state.sort_asc;
        self.table_with(
            id_label,
            columns,
            state,
            row_count,
            |ui, c_idx, is_sorted| {
                let hdr_fg = if is_sorted { primary } else { text_col };
                ui.txt(columns[c_idx].header)
                    .sz(12.0)
                    .semibold()
                    .fg(hdr_fg)
                    .ellipsis()
                    .grow(1.0);
                if is_sorted {
                    emit_chevron_icon(
                        ui,
                        "__tbl_sort_chev__",
                        if sort_asc {
                            ChevronDir::Up
                        } else {
                            ChevronDir::Down
                        },
                        10.0,
                        hdr_fg,
                    );
                }
            },
            cell_fn,
        )
    }

    /// Virtualized table / data grid with custom header cell rendering (`header_fn(ui, col_idx, is_sorted)`).
    ///
    /// Supports multi-line custom headers, optional vertical column dividers (`TableState::col_dividers`),
    /// full-row hover/selection across all columns, and sticky sortable/resizable headers.
    pub fn table_with(
        &mut self,
        id_label: &str,
        columns: &[TableColumn<'_>],
        state: &mut TableState,
        row_count: usize,
        mut header_fn: impl FnMut(&mut UiContext, usize, bool),
        mut cell_fn: impl FnMut(&mut UiContext, usize, usize),
    ) -> StyleBuilder<'_> {
        let id = self.ids.id(id_label);
        if state.col_widths.len() != columns.len() {
            state.col_widths = columns.iter().map(|c| c.width.max(40.0)).collect();
        }
        let disabled_prev = self.prev_disabled_ids.contains(&id);
        let mut sort_changed = false;

        if self.pressed_id.is_none() {
            self.table_resize_drag = None;
        }
        self.ids.push_u64(id.0);
        let mut resize_ids = Vec::with_capacity(columns.len());
        for (c_idx, col) in columns.iter().enumerate() {
            self.ids.push_u64(c_idx as u64);
            let resize_id = self.ids.id("__tbl_resize__");
            let _ = self.ids.register_interactive(resize_id);
            if !disabled_prev && col.resizable {
                if self.just_pressed_id == Some(resize_id) {
                    if let Some([start_px, _]) = self.just_pressed_pos.or(self.pointer_pos) {
                        self.table_resize_drag =
                            Some((resize_id, start_px, state.col_widths[c_idx]));
                    }
                }
                if self.pressed_id == Some(resize_id) {
                    if let (Some((rid, start_px, start_w)), Some([px, _])) =
                        (self.table_resize_drag, self.pointer_pos)
                    {
                        if rid == resize_id {
                            state.col_widths[c_idx] = (start_w + (px - start_px)).max(40.0);
                        }
                    }
                }
            }
            resize_ids.push(resize_id);
            self.ids.pop_id();
        }
        self.ids.pop_id();

        let surface = self.theme.surface;
        let surface_alt = self.theme.surface_alt;
        let border = self.theme.border;
        let primary = self.theme.primary;
        let muted = self.theme.text_muted;
        let radius = self.theme.radius;
        let row_h = state.row_height.unwrap_or(30.0).max(16.0);
        let hdr_h = state.header_height.unwrap_or(32.0).max(16.0);
        let col_dividers = state.col_dividers;
        let divider_col = Color::rgba(border.r, border.g, border.b, 140);
        let total_w: f32 = state.col_widths.iter().sum();
        let zebra_bg = state
            .zebra_bg
            .unwrap_or_else(|| Color::rgba(surface_alt.r, surface_alt.g, surface_alt.b, 90));

        let scroll_id = {
            self.ids.push_u64(id.0);
            let sid = self.ids.id("__tbl_body__");
            self.ids.pop_id();
            sid
        };
        let scroll_y = self
            .scroll_states
            .get(&scroll_id)
            .map_or(0.0, |s| s.spring.value.max(0.0));
        let fallback_h = 220.0_f32;
        let viewport_h = self.prev_rects.get(&scroll_id).map_or(fallback_h, |r| {
            if r.h > 1.0 {
                r.h
            } else {
                fallback_h
            }
        });
        let start = ((scroll_y / row_h).floor() as usize).min(row_count);
        let end = (((scroll_y + viewport_h) / row_h).ceil() as usize)
            .max(start)
            .min(row_count);
        let top_h = start as f32 * row_h;
        let bottom_h = row_count.saturating_sub(end) as f32 * row_h;

        let mut clicked_sort: Option<usize> = None;
        let mut clicked_row: Option<usize> = None;

        let mut builder = self.container_with_id(AxisKind::Col, id_label, id, |ui| {
            // Sticky header row outside scroll container.
            ui.row(|ui| {
                for (c_idx, col) in columns.iter().enumerate() {
                    let cw = state.col_widths[c_idx];
                    ui.ids.push_u64(c_idx as u64);
                    let res_w = if col.resizable { 5.0 } else { 0.0 };
                    let div_w = if !col.resizable && col_dividers && c_idx + 1 < columns.len() {
                        1.0
                    } else {
                        0.0
                    };
                    let btn_w = (cw - res_w - div_w).max(20.0);
                    let is_sorted = state.sort_col == Some(c_idx);

                    let hdr = ui
                        .btn_with("__tbl_hdr__", |ui| {
                            header_fn(ui, c_idx, is_sorted);
                        })
                        .w(btn_w)
                        .h(hdr_h)
                        .px(8.0)
                        .py(6.0)
                        .bg(surface_alt)
                        .hover_bg(border)
                        .r(0.0);
                    if !disabled_prev && col.sortable && hdr.clicked() {
                        clicked_sort = Some(c_idx);
                    }

                    if col.resizable {
                        let resize_id = resize_ids[c_idx];
                        let res_active =
                            ui.hovered_id == Some(resize_id) || ui.pressed_id == Some(resize_id);
                        ui.alloc_node(
                            LayoutNode {
                                id: resize_id,
                                role: Role::SplitDivider,
                                kind: AxisKind::Leaf,
                                style: Style {
                                    width: Some(5.0),
                                    height: Some(hdr_h),
                                    bg: Some(if res_active { primary } else { divider_col }),
                                    ..Style::default()
                                },
                                label: format!("ColResize_{c_idx}"),
                                semantic_value: Some(format!("{cw:.0}")),
                                text: None,
                                rich_spans: None,
                                rich_links: Vec::new(),
                                sparkline_samples: None,
                                image_tex: None,
                                viewport_data: None,
                                is_scroll: false,
                                scroll_offset_y: 0.0,
                                focusable: false,
                                hit_testable: true,
                                enabled: !disabled_prev,
                                is_overlay: false,
                                children: Vec::new(),
                                intrinsic_w: 5.0,
                                intrinsic_h: hdr_h,
                                content_w: 5.0,
                                content_h: hdr_h,
                                rect: Rect::ZERO,
                            },
                            true,
                        );
                    } else if div_w > 0.0 {
                        ui.vdivider().h(hdr_h).bg(divider_col);
                    }
                    ui.ids.pop_id();
                }
            })
            .min_w(total_w)
            .bg(surface_alt)
            .bd_bottom(1.0, border);

            // Virtualized scrollable body.
            ui.scroll("__tbl_body__", |ui| {
                emit_vlist_spacer(ui, "__tbl_top__", top_h);
                for r_idx in start..end {
                    ui.ids.push_u64(r_idx as u64);
                    let row_id = ui.ids.id("__tbl_row__");
                    let _ = ui.ids.register_interactive(row_id);
                    let prev_row_rect = ui.prev_rects.get(&row_id).copied();
                    let ptr_in_row = prev_row_rect
                        .is_some_and(|r| ui.pointer_pos.is_some_and(|p| r.contains(p[0], p[1])));
                    let is_sel = state.selected_row == Some(r_idx);
                    let row_hovered =
                        !disabled_prev && (ui.hovered_id == Some(row_id) || ptr_in_row);
                    let row_bg = if is_sel {
                        Color::rgba(primary.r, primary.g, primary.b, 42)
                    } else if row_hovered {
                        Color::rgba(primary.r, primary.g, primary.b, 24)
                    } else if r_idx % 2 == 1 {
                        zebra_bg
                    } else {
                        Color::TRANSPARENT
                    };
                    let mut row_b =
                        ui.container_with_id(AxisKind::Row, "__tbl_row__", row_id, |ui| {
                            for (c_idx, &cw) in
                                state.col_widths.iter().enumerate().take(columns.len())
                            {
                                ui.ids.push_u64(c_idx as u64);
                                let mut cell_b = ui.row(|ui| {
                                    cell_fn(ui, r_idx, c_idx);
                                });
                                cell_b = cell_b.w(cw).h(row_h).px(8.0).py(4.0).clip();
                                if col_dividers && c_idx + 1 < columns.len() {
                                    cell_b.bd_right(
                                        1.0,
                                        Color::rgba(border.r, border.g, border.b, 60),
                                    );
                                }
                                ui.ids.pop_id();
                            }
                        });
                    let row_idx_node = row_b.node_idx;
                    row_b.ui.nodes[row_idx_node].hit_testable = !disabled_prev;
                    row_b = row_b
                        .min_w(total_w)
                        .h(row_h)
                        .bg(row_bg)
                        .bd_bottom(1.0, Color::rgba(border.r, border.g, border.b, 70));
                    let row_clicked = !disabled_prev
                        && (row_b.clicked()
                            || (row_b.ui.clicked_id.is_some()
                                && ptr_in_row
                                && row_b.ui.just_pressed_pos.is_some_and(|jp| {
                                    prev_row_rect.is_some_and(|r| r.contains(jp[0], jp[1]))
                                })));
                    if row_clicked {
                        clicked_row = Some(r_idx);
                    }
                    ui.ids.pop_id();
                }
                emit_vlist_spacer(ui, "__tbl_bottom__", bottom_h);
                if row_count == 0 {
                    ui.txt("No rows").sz(12.0).fg(muted).p(12.0);
                }
            })
            .grow(1.0);
        });

        if let Some(c_idx) = clicked_sort {
            if state.sort_col == Some(c_idx) {
                state.sort_asc = !state.sort_asc;
            } else {
                state.sort_col = Some(c_idx);
                state.sort_asc = true;
            }
            sort_changed = true;
        }
        if let Some(r_idx) = clicked_row {
            state.selected_row = Some(r_idx);
        }

        builder = builder.bg(surface).bd(1.0, border).r(radius).clip();
        builder.changed = sort_changed;
        builder
    }

    /// Alias for [`UiContext::table`].
    pub fn data_grid(
        &mut self,
        id_label: &str,
        columns: &[TableColumn<'_>],
        state: &mut TableState,
        row_count: usize,
        cell_fn: impl FnMut(&mut UiContext, usize, usize),
    ) -> StyleBuilder<'_> {
        self.table(id_label, columns, state, row_count, cell_fn)
    }

    /// Vector icon decoded from an SVG path `d` string at `size x size` pixels.
    ///
    /// Supports `.fg(color)` to tint the vector strokes and fills.
    pub fn svg_icon(&mut self, d: &str, size: f32) -> StyleBuilder<'_> {
        let ordinal = self
            .container_stack
            .last()
            .map_or(0, |&p| self.nodes[p].children.len());
        self.ids.push_u64(ordinal as u64);
        let id = self.ids.id("__svg_icon__");
        self.ids.pop_id();

        let sz = size.max(4.0);
        let mut c = Canvas2D::new([0.0, 0.0]);
        c.svg_icon_path(d, sz, self.theme.text);

        let mut node = LayoutNode::new_leaf(id, Role::Container, String::from("SvgIcon"));
        node.style = Style {
            width: Some(sz),
            height: Some(sz),
            svg_fg_tint: true,
            ..Style::default()
        };
        node.viewport_data = Some((
            c.cmds,
            SceneSnapshot {
                camera: Camera3D::default(),
                draws: Vec::new(),
                msaa_samples: 4,
            },
        ));
        node.intrinsic_w = sz;
        node.intrinsic_h = sz;
        node.content_w = sz;
        node.content_h = sz;
        let idx = self.alloc_node(node, true);
        StyleBuilder {
            ui: self,
            node_idx: idx,
            changed: false,
        }
    }

    /// Animated GIF/image sequence widget streaming active frame pixels into `tex` via `app`.
    pub fn animated_image(
        &mut self,
        frames: &[GifFrame],
        tex: TextureId,
        app: &mut App,
    ) -> StyleBuilder<'_> {
        if frames.is_empty() {
            return self.img(tex);
        }
        let total_ms: u32 = frames
            .iter()
            .map(|f| f.delay_ms.max(20))
            .fold(0u32, |acc, d| acc.saturating_add(d))
            .max(1);
        let now_ms = (self.elapsed_time_s() * 1000.0).rem_euclid(f64::from(total_ms)) as u32;
        let mut cursor = 0u32;
        let mut active_idx = 0usize;
        for (i, f) in frames.iter().enumerate() {
            cursor = cursor.saturating_add(f.delay_ms.max(20));
            if now_ms < cursor {
                active_idx = i;
                break;
            }
        }
        if frames.len() > 1 {
            self.has_active_indeterminate = true;
        }
        let frame = &frames[active_idx];
        let prev = self.animated_frames.insert(tex, active_idx);
        if prev != Some(active_idx) {
            let _ = app.update_texture_rgba(
                tex,
                frame.image.width,
                frame.image.height,
                &frame.image.data,
            );
        }
        let (w, h) = (frame.image.width.max(1), frame.image.height.max(1));
        self.img(tex).tex_size(w, h)
    }
}
