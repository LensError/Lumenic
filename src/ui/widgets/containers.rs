use super::*;

impl UiContext {
    /// Horizontal resizable split pane with a draggable 6px divider (`ratio` clamped to `0.1..=0.9`).
    pub fn hsplit<L, R>(
        &mut self,
        id_label: &str,
        ratio: &mut f32,
        left: L,
        right: R,
    ) -> StyleBuilder<'_>
    where
        L: FnOnce(&mut UiContext),
        R: FnOnce(&mut UiContext),
    {
        let split_id = self.ids.id(id_label);
        self.ids.push_u64(split_id.0);
        let div_id = self.ids.id("__hsplit_div__");
        let _ = self.ids.register_interactive(div_id);

        if self.pressed_id == Some(div_id) {
            if let (Some([px, _]), Some(prev)) =
                (self.pointer_pos, self.prev_rects.get(&split_id).copied())
            {
                *ratio = ((px - prev.x) / prev.w.max(1.0)).clamp(0.1, 0.9);
            }
        }
        *ratio = ratio.clamp(0.1, 0.9);

        let r_left = *ratio;
        let r_right = 1.0 - r_left;
        let div_color = if self.hovered_id == Some(div_id) || self.pressed_id == Some(div_id) {
            self.theme.primary
        } else {
            self.theme.border
        };

        let builder = self.container_with_id(AxisKind::Row, id_label, split_id, |ui| {
            ui.col(left).grow(r_left);
            ui.alloc_node(
                LayoutNode {
                    id: div_id,
                    role: Role::SplitDivider,
                    kind: AxisKind::Leaf,
                    style: Style {
                        width: Some(6.0),
                        bg: Some(div_color),
                        ..Style::default()
                    },
                    label: String::from("HSplitDivider"),
                    semantic_value: Some(format!("{r_left:.2}")),
                    text: None,
                    rich_spans: None,
                    rich_links: Vec::new(),
                    sparkline_samples: None,
                    image_tex: None,
                    viewport_data: None,
                    is_scroll: false,
                    scroll_offset_y: 0.0,
                    focusable: true,
                    hit_testable: true,
                    enabled: true,
                    is_overlay: false,
                    children: Vec::new(),
                    intrinsic_w: 6.0,
                    intrinsic_h: 24.0,
                    content_w: 6.0,
                    content_h: 24.0,
                    rect: Rect::ZERO,
                },
                true,
            );
            ui.col(right).grow(r_right);
        });
        builder.ui.ids.pop_id();
        builder
    }

    /// Vertical resizable split pane with a draggable 6px divider (`ratio` clamped to `0.1..=0.9`).
    pub fn vsplit<T, B>(
        &mut self,
        id_label: &str,
        ratio: &mut f32,
        top: T,
        bottom: B,
    ) -> StyleBuilder<'_>
    where
        T: FnOnce(&mut UiContext),
        B: FnOnce(&mut UiContext),
    {
        let split_id = self.ids.id(id_label);
        self.ids.push_u64(split_id.0);
        let div_id = self.ids.id("__vsplit_div__");
        let _ = self.ids.register_interactive(div_id);

        if self.pressed_id == Some(div_id) {
            if let (Some([_, py]), Some(prev)) =
                (self.pointer_pos, self.prev_rects.get(&split_id).copied())
            {
                *ratio = ((py - prev.y) / prev.h.max(1.0)).clamp(0.1, 0.9);
            }
        }
        *ratio = ratio.clamp(0.1, 0.9);

        let r_top = *ratio;
        let r_bot = 1.0 - r_top;
        let div_color = if self.hovered_id == Some(div_id) || self.pressed_id == Some(div_id) {
            self.theme.primary
        } else {
            self.theme.border
        };

        let builder = self.container_with_id(AxisKind::Col, id_label, split_id, |ui| {
            ui.col(top).grow(r_top);
            ui.alloc_node(
                LayoutNode {
                    id: div_id,
                    role: Role::SplitDivider,
                    kind: AxisKind::Leaf,
                    style: Style {
                        height: Some(6.0),
                        bg: Some(div_color),
                        ..Style::default()
                    },
                    label: String::from("VSplitDivider"),
                    semantic_value: Some(format!("{r_top:.2}")),
                    text: None,
                    rich_spans: None,
                    rich_links: Vec::new(),
                    sparkline_samples: None,
                    image_tex: None,
                    viewport_data: None,
                    is_scroll: false,
                    scroll_offset_y: 0.0,
                    focusable: true,
                    hit_testable: true,
                    enabled: true,
                    is_overlay: false,
                    children: Vec::new(),
                    intrinsic_w: 24.0,
                    intrinsic_h: 6.0,
                    content_w: 24.0,
                    content_h: 6.0,
                    rect: Rect::ZERO,
                },
                true,
            );
            ui.col(bottom).grow(r_bot);
        });
        builder.ui.ids.pop_id();
        builder
    }

    /// Settings row card with title, muted description, and right-aligned control closure.
    pub fn setting<F: FnOnce(&mut UiContext)>(
        &mut self,
        title: &str,
        desc: &str,
        f: F,
    ) -> StyleBuilder<'_> {
        let surface = self.theme.surface;
        let border = self.theme.border;
        let radius = self.theme.radius;
        let text_col = self.theme.text;
        let muted = self.theme.text_muted;

        self.row(|ui| {
            ui.col(|ui| {
                ui.txt(title).fg(text_col).sz(14.0);
                if !desc.is_empty() {
                    ui.txt(desc).fg(muted).sz(12.0).wrap();
                }
            })
            .grow(1.0)
            .gap(2.0);
            f(ui);
        })
        .p(12.0)
        .gap(12.0)
        .bg(surface)
        .bd(1.0, border)
        .r(radius)
    }

    /// Anchored popup context menu rendered in the deferred global `overlay` pass.
    ///
    /// Automatically clamps to the viewport so it never clips off the bottom/right edges,
    /// and dismisses cleanly on `Escape` or outside click.
    pub fn context_menu<F: FnOnce(&mut UiContext)>(
        &mut self,
        id_label: &str,
        open: &mut bool,
        anchor: [f32; 2],
        f: F,
    ) -> StyleBuilder<'_> {
        let menu_id = self.ids.id(id_label);
        if !*open || self.dismiss_requested {
            if self.dismiss_requested {
                *open = false;
            }
            let dummy_idx = self.alloc_node(
                LayoutNode {
                    id: menu_id,
                    role: Role::Container,
                    kind: AxisKind::Col,
                    style: Style::default(),
                    label: String::from(id_label),
                    semantic_value: None,
                    text: None,
                    rich_spans: None,
                    rich_links: Vec::new(),
                    sparkline_samples: None,
                    image_tex: None,
                    viewport_data: None,
                    is_scroll: false,
                    scroll_offset_y: 0.0,
                    focusable: false,
                    hit_testable: false,
                    enabled: false,
                    is_overlay: true,
                    children: Vec::new(),
                    intrinsic_w: 0.0,
                    intrinsic_h: 0.0,
                    content_w: 0.0,
                    content_h: 0.0,
                    rect: Rect::ZERO,
                },
                false,
            );
            return StyleBuilder {
                ui: self,
                node_idx: dummy_idx,
                changed: false,
            };
        }

        let prev_rect = self.prev_rects.get(&menu_id).copied();
        let clicked_outside = if let (Some([px, py]), Some(r)) = (self.just_pressed_pos, prev_rect)
        {
            !r.contains(px, py)
        } else {
            false
        };

        let surface = self.theme.surface;
        let border = self.theme.border;
        let radius = self.theme.radius;

        let builder = self.build_container_overlay_with_id(id_label, menu_id, f);
        let node_idx = builder.node_idx;
        builder.ui.nodes[node_idx].hit_testable = true;
        if clicked_outside {
            *open = false;
        }
        builder
            .pos(anchor[0], anchor[1])
            .clamp_viewport()
            .w(180.0)
            .p(6.0)
            .gap(4.0)
            .bg(surface)
            .bd(1.0, border)
            .r(radius)
            .shadow(14.0, Color::rgba(0, 0, 0, 125))
    }

    /// Global overlay modal dialog that dismisses cleanly on `Escape` or backdrop click.
    ///
    /// Returns a `StyleBuilder` targeting the modal card so callers can configure `.w()`, `.h()`,
    /// `.p()`, `.gap()`, `.bg()`, `.r()`, or `.pos(x, y)` directly.
    pub fn modal<F: FnOnce(&mut UiContext)>(
        &mut self,
        id_label: &str,
        open: &mut bool,
        f: F,
    ) -> StyleBuilder<'_> {
        let modal_id = self.ids.id(id_label);
        if !*open || self.dismiss_requested {
            if self.dismiss_requested {
                *open = false;
            }
            let dummy_idx = self.alloc_node(
                LayoutNode {
                    id: modal_id,
                    role: Role::Modal,
                    kind: AxisKind::Col,
                    style: Style::default(),
                    label: String::from(id_label),
                    semantic_value: None,
                    text: None,
                    rich_spans: None,
                    rich_links: Vec::new(),
                    sparkline_samples: None,
                    image_tex: None,
                    viewport_data: None,
                    is_scroll: false,
                    scroll_offset_y: 0.0,
                    focusable: false,
                    hit_testable: false,
                    enabled: false,
                    is_overlay: true,
                    children: Vec::new(),
                    intrinsic_w: 0.0,
                    intrinsic_h: 0.0,
                    content_w: 0.0,
                    content_h: 0.0,
                    rect: Rect::ZERO,
                },
                false,
            );
            return StyleBuilder {
                ui: self,
                node_idx: dummy_idx,
                changed: false,
            };
        }

        self.ids.push_u64(modal_id.0);
        let backdrop_id = self.ids.id("__modal_backdrop__");
        let _ = self.ids.register_interactive(backdrop_id);
        let backdrop_clicked = self.clicked_id == Some(backdrop_id);

        let card_id = self.ids.id("__modal_card__");
        let prev_card_rect = self.prev_rects.get(&card_id).copied();
        let scroll_dy = self.frame_scroll_delta[1];
        let pointer_in_card = self
            .pointer_pos
            .zip(prev_card_rect)
            .is_some_and(|([px, py], r)| r.contains(px, py));
        let scroll_state = self.scroll_states.entry(card_id).or_default();
        if pointer_in_card && scroll_dy != 0.0 {
            scroll_state.target_y =
                (scroll_state.target_y - scroll_dy * 32.0).clamp(0.0, scroll_state.max_y);
            scroll_state.spring.target = scroll_state.target_y;
        }
        let offset_y = scroll_state.spring.value;

        let surface = self.theme.surface;
        let border = self.theme.border;
        let radius = self.theme.radius;

        let top_y = if self.has_csd_titlebar { 40.0 } else { 0.0 };
        let vw = self.viewport_w;
        let avail_h = (self.viewport_h - top_y).max(0.0);
        let modal_w = (vw - 32.0).clamp(160.0, 440.0);
        let mut card_node_idx = 0usize;

        self.overlay(|ui| {
            ui.zstack(|ui| {
                ui.alloc_node(
                    LayoutNode {
                        id: backdrop_id,
                        role: Role::Container,
                        kind: AxisKind::Leaf,
                        style: Style {
                            grow: 1.0,
                            bg: Some(Color::rgba(0, 0, 0, 145)),
                            ..Style::default()
                        },
                        label: String::from("ModalBackdrop"),
                        semantic_value: None,
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
                        enabled: true,
                        is_overlay: true,
                        children: Vec::new(),
                        intrinsic_w: 0.0,
                        intrinsic_h: 0.0,
                        content_w: 0.0,
                        content_h: 0.0,
                        rect: Rect::ZERO,
                    },
                    true,
                );

                let card = ui.container_with_id(AxisKind::Col, id_label, card_id, f);
                card_node_idx = card.node_idx;
                let card_node = &mut card.ui.nodes[card.node_idx];
                card_node.role = Role::Modal;
                card_node.hit_testable = true;
                card_node.is_scroll = true;
                card_node.scroll_offset_y = offset_y;
                card.p(20.0)
                    .gap(12.0)
                    .w(modal_w)
                    .bg(surface)
                    .bd(1.0, border)
                    .r(radius)
                    .shadow(18.0, Color::rgba(0, 0, 0, 140));
            })
            .grow(1.0)
            .center();
        })
        .pos(0.0, top_y)
        .w(vw)
        .h(avail_h);

        self.ids.pop_id();
        if backdrop_clicked {
            *open = false;
        }
        StyleBuilder {
            ui: self,
            node_idx: card_node_idx,
            changed: false,
        }
    }

    /// Horizontal tab bar with Left/Right keyboard navigation. Returns `StyleBuilder` with `.changed()`.
    pub fn tabs(
        &mut self,
        id_label: &str,
        selected: &mut usize,
        labels: &[&str],
    ) -> StyleBuilder<'_> {
        self.tabs_impl(id_label, selected, labels, false)
    }

    /// Pill-style segmented control switcher with Left/Right keyboard navigation.
    pub fn segmented_control(
        &mut self,
        id_label: &str,
        selected: &mut usize,
        labels: &[&str],
    ) -> StyleBuilder<'_> {
        self.tabs_impl(id_label, selected, labels, true)
    }

    fn tabs_impl(
        &mut self,
        id_label: &str,
        selected: &mut usize,
        labels: &[&str],
        segmented: bool,
    ) -> StyleBuilder<'_> {
        let group_id = self.ids.id(id_label);
        let disabled_prev = self.prev_disabled_ids.contains(&group_id);
        let mut changed = false;

        self.ids.push_u64(group_id.0);
        let mut tab_ids = Vec::with_capacity(labels.len());
        for (i, &lbl) in labels.iter().enumerate() {
            self.ids.push_u64(i as u64);
            tab_ids.push(self.ids.id(lbl));
            self.ids.pop_id();
        }
        self.ids.pop_id();

        if !disabled_prev && !labels.is_empty() {
            if *selected >= labels.len() {
                *selected = labels.len() - 1;
            }
            let any_focused = self
                .focused_id
                .is_some_and(|fid| tab_ids.contains(&fid) || fid == group_id);
            if any_focused {
                match self.last_key_pressed {
                    Some(Key::Right) => {
                        let next = (*selected + 1) % labels.len();
                        if next != *selected {
                            *selected = next;
                            self.focused_id = Some(tab_ids[next]);
                            changed = true;
                        }
                    }
                    Some(Key::Left) => {
                        let next = if *selected == 0 {
                            labels.len() - 1
                        } else {
                            *selected - 1
                        };
                        if next != *selected {
                            *selected = next;
                            self.focused_id = Some(tab_ids[next]);
                            changed = true;
                        }
                    }
                    _ => {}
                }
            }
        }

        let primary = self.theme.primary;
        let on_primary = self.theme.on_primary;
        let surface_alt = self.theme.surface_alt;
        let border = self.theme.border;
        let text_col = self.theme.text;
        let muted = self.theme.text_muted;
        let radius = self.theme.radius;
        let mut clicked_tab = None;

        let mut builder = self.container_with_id(AxisKind::Row, id_label, group_id, |ui| {
            for (i, &lbl) in labels.iter().enumerate() {
                ui.ids.push_u64(i as u64);
                let is_sel = i == *selected;
                let mut btn = ui.btn(lbl);
                if segmented {
                    btn = btn
                        .px(12.0)
                        .py(6.0)
                        .bg(if is_sel { primary } else { Color::TRANSPARENT })
                        .fg(if is_sel { on_primary } else { muted })
                        .r((radius - 2.0).max(4.0));
                } else {
                    btn = btn
                        .px(12.0)
                        .py(7.0)
                        .bg(if is_sel {
                            surface_alt
                        } else {
                            Color::TRANSPARENT
                        })
                        .fg(if is_sel { text_col } else { muted })
                        .r(0.0)
                        .bd_bottom(if is_sel { 2.0 } else { 0.0 }, primary);
                }
                if !disabled_prev && btn.clicked() {
                    clicked_tab = Some(i);
                }
                ui.ids.pop_id();
            }
        });

        if let Some(idx) = clicked_tab {
            if idx != *selected {
                *selected = idx;
                changed = true;
            }
        }
        if segmented {
            builder = builder
                .p(3.0)
                .gap(2.0)
                .bg(surface_alt)
                .bd(1.0, border)
                .r(radius);
        } else {
            builder = builder.gap(2.0).bd_bottom(1.0, border);
        }
        builder.changed = changed;
        builder
    }

    /// Collapsible section header with chevron indicator and child content body.
    pub fn collapsing_header<F: FnOnce(&mut UiContext)>(
        &mut self,
        label: &str,
        open: &mut bool,
        body: F,
    ) -> StyleBuilder<'_> {
        self.collapsing_header_inner(label, open, true, body)
    }

    fn collapsing_header_inner<F: FnOnce(&mut UiContext)>(
        &mut self,
        label: &str,
        open: &mut bool,
        self_toggle: bool,
        body: F,
    ) -> StyleBuilder<'_> {
        let id = self.ids.id(label);
        let disabled_prev = self.prev_disabled_ids.contains(&id);
        let surface = self.theme.surface;
        let surface_alt = self.theme.surface_alt;
        let border = self.theme.border;
        let text_col = self.theme.text;
        let muted = self.theme.text_muted;
        let radius = self.theme.radius;
        let mut toggled = false;

        let mut builder = self.container_with_id(AxisKind::Col, label, id, |ui| {
            let hdr_id = ui.ids.id("__collapsing_hdr__");
            if self_toggle
                && !disabled_prev
                && !ui.prev_disabled_ids.contains(&hdr_id)
                && (ui.clicked_id == Some(hdr_id) || ui.activated_id == Some(hdr_id))
            {
                *open = !*open;
                toggled = true;
            }
            let is_open = *open;
            let _ = ui
                .btn_with("__collapsing_hdr__", |ui| {
                    emit_chevron_icon(
                        ui,
                        "__ch_chev__",
                        if is_open {
                            ChevronDir::Down
                        } else {
                            ChevronDir::Right
                        },
                        12.0,
                        muted,
                    );
                    ui.txt(label).fg(text_col).semibold().grow(1.0);
                })
                .px(10.0)
                .py(7.0)
                .gap(6.0)
                .bg(surface_alt)
                .hover_bg(border)
                .r((radius - 2.0).max(4.0));
            if is_open {
                ui.col(body).pl(12.0).pt(6.0).gap(6.0);
            }
        });
        builder = builder
            .p(8.0)
            .gap(4.0)
            .bg(surface)
            .bd(1.0, border)
            .r(radius);
        builder.changed = toggled;
        builder
    }

    /// Exclusive single-open accordion stack over `headers`. Returns `StyleBuilder` with `.changed()`.
    pub fn accordion(
        &mut self,
        id_label: &str,
        open_idx: &mut Option<usize>,
        headers: &[&str],
        mut body_fn: impl FnMut(&mut UiContext, usize),
    ) -> StyleBuilder<'_> {
        let id = self.ids.id(id_label);
        let disabled_prev = self.prev_disabled_ids.contains(&id);
        let mut changed = false;

        let mut builder = self.container_with_id(AxisKind::Col, id_label, id, |ui| {
            if !disabled_prev {
                for (i, &hdr) in headers.iter().enumerate() {
                    ui.ids.push_u64(i as u64);
                    let sec_id = ui.ids.id(hdr);
                    ui.ids.push_u64(sec_id.0);
                    let hdr_id = ui.ids.id("__collapsing_hdr__");
                    ui.ids.pop_id();
                    ui.ids.pop_id();
                    if !ui.prev_disabled_ids.contains(&hdr_id)
                        && (ui.clicked_id == Some(hdr_id) || ui.activated_id == Some(hdr_id))
                    {
                        *open_idx = if *open_idx == Some(i) { None } else { Some(i) };
                        changed = true;
                        break;
                    }
                }
            }
            for (i, &hdr) in headers.iter().enumerate() {
                ui.ids.push_u64(i as u64);
                let mut is_open = *open_idx == Some(i);
                let _ = ui.collapsing_header_inner(hdr, &mut is_open, false, |ui| body_fn(ui, i));
                ui.ids.pop_id();
            }
        });
        builder = builder.gap(6.0);
        builder.changed = changed;
        builder
    }

    /// Vertical tree-view container managing indentation scope for nested [`UiContext::tree_node`] calls.
    pub fn tree_view<F: FnOnce(&mut UiContext)>(
        &mut self,
        id_label: &str,
        body: F,
    ) -> StyleBuilder<'_> {
        let id = self.ids.id(id_label);
        let prev_depth = self.tree_depth;
        self.tree_depth = 0;
        let builder = self.container_with_id(AxisKind::Col, id_label, id, body);
        builder.ui.tree_depth = prev_depth;
        builder.gap(2.0)
    }

    /// Hierarchical tree node that indents automatically by nesting depth and toggles `open` when `has_children` is `true`.
    pub fn tree_node<F: FnOnce(&mut UiContext)>(
        &mut self,
        label: &str,
        open: &mut bool,
        has_children: bool,
        body: F,
    ) -> StyleBuilder<'_> {
        let id = self.ids.id(label);
        let disabled_prev = self.prev_disabled_ids.contains(&id);
        let depth = self.tree_depth;
        let indent = f32::from(depth) * 16.0;
        let muted = self.theme.text_muted;
        let text_col = self.theme.text;
        let surface_alt = self.theme.surface_alt;
        let mut toggled = false;

        let mut builder = self.container_with_id(AxisKind::Col, label, id, |ui| {
            let row_id = ui.ids.id("__tree_row__");
            if !disabled_prev
                && has_children
                && !ui.prev_disabled_ids.contains(&row_id)
                && (ui.clicked_id == Some(row_id) || ui.activated_id == Some(row_id))
            {
                *open = !*open;
                toggled = true;
            }
            let is_open = *open;
            let _ = ui
                .btn_with("__tree_row__", |ui| {
                    if !has_children {
                        ui.txt("-").sz(11.0).fg(muted).w(12.0);
                    } else {
                        emit_chevron_icon(
                            ui,
                            "__tree_chev__",
                            if is_open {
                                ChevronDir::Down
                            } else {
                                ChevronDir::Right
                            },
                            12.0,
                            muted,
                        );
                    }
                    ui.txt(label).fg(text_col).grow(1.0);
                })
                .pl(8.0 + indent)
                .pr(8.0)
                .py(5.0)
                .gap(6.0)
                .bg(Color::TRANSPARENT)
                .hover_bg(surface_alt)
                .r(6.0);
            if has_children && is_open {
                ui.tree_depth = depth.saturating_add(1);
                body(ui);
                ui.tree_depth = depth;
            }
        });
        builder.changed = toggled;
        builder
    }
}
