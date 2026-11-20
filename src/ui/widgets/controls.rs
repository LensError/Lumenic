use super::*;

impl UiContext {
    /// Stateless themed toggle switch with spring-animated thumb.
    ///
    /// Returns a `StyleBuilder` (with `.changed()`, `.switch_size(w, h)`, `.track_bg(color)`,
    /// `.thumb_bg(color)`, `.track_bd(w, color)`, `.sz(pt)`, `.fg(color)`, `.enabled(bool)`).
    pub fn toggle(&mut self, label: &str, on: &mut bool) -> StyleBuilder<'_> {
        let id = self.ids.id(label);
        let _ = self.ids.register_interactive(id);
        let disabled_prev = self.prev_disabled_ids.contains(&id);
        let toggled =
            !disabled_prev && (self.clicked_id == Some(id) || self.activated_id == Some(id));
        if toggled {
            *on = !*on;
        }
        let focused = !disabled_prev && self.focused_id == Some(id);
        let hovered = !disabled_prev && self.hovered_id == Some(id);

        let target = if *on { 1.0 } else { 0.0 };
        self.ids.push_u64(id.0);
        let thumb_t = self.spring("toggle_thumb", target, 360.0, 28.0);
        self.ids.pop_id();

        let track_bg = if *on {
            self.theme.primary
        } else {
            self.theme.surface_alt
        };
        let thumb_col = if *on {
            self.theme.on_primary
        } else {
            self.theme.text
        };
        let border_col = self.theme.border;
        let focus_col = self.theme.focus_ring;
        let muted_col = self.theme.text_muted;
        let text_col = self.theme.text;
        let track_bd_w = if focused { 2.0 } else { 1.0 };
        let track_bd_col = if focused {
            focus_col
        } else if hovered {
            muted_col
        } else {
            border_col
        };

        let mut builder = self.container_with_id(AxisKind::Row, label, id, |ui| {
            ui.row(|ui| {
                ui.txt("").w(3.0 + thumb_t * 16.0).h(18.0);
                ui.txt("").w(18.0).h(18.0).bg(thumb_col).circle();
            })
            .w(42.0)
            .h(24.0)
            .py(3.0)
            .bg(track_bg)
            .bd(track_bd_w, track_bd_col)
            .r(12.0);

            if !label.is_empty() {
                ui.txt(label).fg(text_col);
            }
        });
        let node = &mut builder.ui.nodes[builder.node_idx];
        node.role = Role::Toggle;
        node.semantic_value = Some(String::from(if *on { "true" } else { "false" }));
        node.focusable = true;
        node.hit_testable = true;
        node.style.gap = 8.0;
        node.style.pad_l = 4.0;
        node.style.pad_r = 4.0;
        node.style.pad_t = 2.0;
        node.style.pad_b = 2.0;
        node.style.radius = 8.0;
        builder.changed = toggled;
        builder
    }

    /// Stateless checkbox with vector checkmark and customizable box/label styling.
    ///
    /// Returns a `StyleBuilder` (with `.changed()`, `.box_size(size)`, `.track_bg(color)`,
    /// `.thumb_bg(color)`, `.track_bd(w, color)`, `.sz(pt)`, `.fg(color)`, `.enabled(bool)`).
    pub fn checkbox(&mut self, label: &str, checked: &mut bool) -> StyleBuilder<'_> {
        let id = self.ids.id(label);
        let _ = self.ids.register_interactive(id);
        let disabled_prev = self.prev_disabled_ids.contains(&id);
        let toggled =
            !disabled_prev && (self.clicked_id == Some(id) || self.activated_id == Some(id));
        if toggled {
            *checked = !*checked;
        }
        let focused = !disabled_prev && self.focused_id == Some(id);
        let hovered = !disabled_prev && self.hovered_id == Some(id);

        let box_bg = if *checked {
            self.theme.primary
        } else {
            self.theme.surface_alt
        };
        let check_col = self.theme.on_primary;
        let border_col = self.theme.border;
        let focus_col = self.theme.focus_ring;
        let muted_col = self.theme.text_muted;
        let text_col = self.theme.text;
        let bd_w = if focused { 2.0 } else { 1.0 };
        let bd_col = if focused {
            focus_col
        } else if hovered {
            muted_col
        } else if *checked {
            self.theme.primary
        } else {
            border_col
        };
        let is_checked = *checked;

        let mut builder = self.container_with_id(AxisKind::Row, label, id, |ui| {
            let mark_cmds = if is_checked {
                checkbox_mark_cmds(18.0, 18.0, check_col)
            } else {
                Vec::new()
            };
            ui.alloc_node(
                LayoutNode {
                    id: ui.ids.id("__checkbox_box__"),
                    role: Role::Container,
                    kind: AxisKind::Leaf,
                    style: Style {
                        width: Some(18.0),
                        height: Some(18.0),
                        bg: Some(box_bg),
                        border_width: bd_w,
                        border_color: bd_col,
                        radius: 4.0,
                        ..Style::default()
                    },
                    label: String::from("CheckboxBox"),
                    semantic_value: None,
                    text: None,
                    rich_spans: None,
                    rich_links: Vec::new(),
                    sparkline_samples: None,
                    image_tex: None,
                    viewport_data: is_checked.then_some((
                        mark_cmds,
                        SceneSnapshot {
                            camera: Camera3D::default(),
                            draws: Vec::new(),
                            msaa_samples: 4,
                        },
                    )),
                    is_scroll: false,
                    scroll_offset_y: 0.0,
                    focusable: false,
                    hit_testable: false,
                    enabled: true,
                    is_overlay: false,
                    children: Vec::new(),
                    intrinsic_w: 18.0,
                    intrinsic_h: 18.0,
                    content_w: 18.0,
                    content_h: 18.0,
                    rect: Rect::ZERO,
                },
                true,
            );

            if !label.is_empty() {
                ui.txt(label).fg(text_col);
            }
        });
        let node = &mut builder.ui.nodes[builder.node_idx];
        node.role = Role::Toggle;
        node.semantic_value = Some(String::from(if *checked { "true" } else { "false" }));
        node.focusable = true;
        node.hit_testable = true;
        node.style.gap = 8.0;
        node.style.pad_l = 4.0;
        node.style.pad_r = 4.0;
        node.style.pad_t = 2.0;
        node.style.pad_b = 2.0;
        node.style.radius = 6.0;
        builder.changed = toggled;
        builder
    }

    /// Stateless horizontal slider over `range`. Returns a customizable `StyleBuilder` (with `.changed()`).
    ///
    /// # Applicable Modifiers
    /// - `.track_bg(color)`, `.track_bd(w, color)`, `.track_h(h)`, `.thumb_size(s)`
    /// - `.fill(color)` / `.fg(color)` (active fill / label color)
    /// - `.borderless()`, `.no_bg()`, `.compact()` (strip outer card wrapper chrome)
    /// - `.w(w)`, `.grow(weight)`, `.enabled(bool)`, `.focus_ring(w, color)`, `.no_focus_ring()`
    pub fn slider(
        &mut self,
        label: &str,
        value: &mut f32,
        range: RangeInclusive<f32>,
    ) -> StyleBuilder<'_> {
        let start = *range.start();
        let end = *range.end();
        let span = (end - start).max(1e-6);

        let id = self.ids.id(label);
        let _ = self.ids.register_interactive(id);
        let disabled_prev = self.prev_disabled_ids.contains(&id);

        let mut changed = false;
        if !disabled_prev && (self.pressed_id == Some(id) || self.clicked_id == Some(id)) {
            if let (Some([px, _]), Some(prev)) =
                (self.pointer_pos, self.prev_rects.get(&id).copied())
            {
                let t = ((px - prev.x) / prev.w.max(1.0)).clamp(0.0, 1.0);
                let next = start + t * span;
                if (next - *value).abs() > f32::EPSILON {
                    *value = next;
                    changed = true;
                }
            }
        } else if !disabled_prev && self.focused_id == Some(id) {
            let step = span * 0.02;
            match self.last_key_pressed {
                Some(Key::Right) => {
                    *value = (*value + step).min(end);
                    changed = true;
                }
                Some(Key::Left) => {
                    *value = (*value - step).max(start);
                    changed = true;
                }
                _ => {}
            }
        }

        *value = value.clamp(start.min(end), start.max(end));
        let norm = ((*value - start) / span).clamp(0.0, 1.0);

        let surface = self.theme.surface;
        let radius = self.theme.radius;
        let primary = self.theme.primary;
        let surface_alt = self.theme.surface_alt;
        let border_col = self.theme.border;
        let text_col = self.theme.text;
        let muted = self.theme.text_muted;
        let readout = format!("{value:.2}");

        let mut builder = self
            .col(|ui| {
                ui.row(|ui| {
                    ui.txt(label).fg(text_col).grow(1.0);
                    ui.txt(&readout).fg(muted).sz(12.0);
                })
                .gap(8.0);

                let focused = ui.focused_id == Some(id);
                let focus_col = ui.theme.focus_ring;
                let track = ui.container_with_id(AxisKind::Row, label, id, |ui| {
                    if norm >= 0.995 {
                        ui.txt("").grow(1.0).h(8.0).bg(primary).r(4.0);
                    } else {
                        let fill_g = norm.max(0.02);
                        ui.txt("").grow(fill_g).h(8.0).bg(primary).r(4.0);
                        ui.txt("").grow((1.0 - fill_g).max(0.001)).h(8.0);
                    }
                });
                let node = &mut track.ui.nodes[track.node_idx];
                node.role = Role::Slider;
                node.semantic_value = Some(readout);
                node.focusable = true;
                node.hit_testable = true;
                node.style.height = Some(16.0);
                node.style.pad_l = 4.0;
                node.style.pad_r = 4.0;
                node.style.pad_t = 4.0;
                node.style.pad_b = 4.0;
                node.style.bg = Some(surface_alt);
                node.style.border_width = if focused { 2.0 } else { 1.0 };
                node.style.border_color = if focused { focus_col } else { border_col };
                node.style.radius = 8.0;
            })
            .p(12.0)
            .gap(8.0)
            .bg(surface)
            .bd(1.0, border_col)
            .r(radius);

        builder.changed = changed;
        builder
    }

    /// Interactive keybind capture button. Returns a customizable `StyleBuilder` (with `.changed()`).
    pub fn keybind(&mut self, label: &str, key: &mut Key) -> StyleBuilder<'_> {
        let id = self.ids.id(label);
        let disabled_prev = self.prev_disabled_ids.contains(&id);
        let mut changed = false;
        if let Some((cap_id, new_key)) = self.captured_key {
            if cap_id == id && !disabled_prev {
                *key = new_key;
                self.captured_key = None;
                changed = true;
            }
        }

        let capturing = !disabled_prev && self.capturing_keybind_id == Some(id);
        let key_text = if capturing {
            String::from("Press key...")
        } else {
            format_key_name(*key)
        };

        let surface = self.theme.surface;
        let border = self.theme.border;
        let radius = self.theme.radius;
        let surface_alt = self.theme.surface_alt;
        let primary = self.theme.primary;
        let on_primary = self.theme.on_primary;
        let text_col = self.theme.text;
        let mut entered_capture = false;

        let mut builder = self
            .row(|ui| {
                ui.txt(label).fg(text_col).grow(1.0);
                let resp = ui
                    .btn(&key_text)
                    .w(if capturing { 92.0 } else { 64.0 })
                    .h(30.0)
                    .px(0.0)
                    .py(0.0)
                    .bg(if capturing { primary } else { surface_alt })
                    .fg(if capturing { on_primary } else { text_col })
                    .r(6.0);
                if !disabled_prev && resp.clicked() {
                    entered_capture = true;
                }
            })
            .p(12.0)
            .gap(10.0)
            .bg(surface)
            .bd(1.0, border)
            .r(radius);

        if entered_capture {
            builder.ui.capturing_keybind_id = Some(id);
        }
        builder.changed = changed;
        builder
    }

    /// Dropdown selector rendering its popup menu in the deferred global `overlay` pass.
    ///
    /// Returns a `StyleBuilder` targeting the trigger button so callers can customize `.w()`,
    /// `.h()`, `.bg()`, `.fg()`, `.bd()`, `.r()`, `.enabled()`, or check `.changed()`.
    pub fn dropdown(
        &mut self,
        id_label: &str,
        selected: &mut usize,
        options: &[&str],
    ) -> StyleBuilder<'_> {
        let id = self.ids.id(id_label);
        let disabled_prev = self.prev_disabled_ids.contains(&id);
        let mut changed = false;
        let current_label = options.get(*selected).copied().unwrap_or("Select");
        let text_col = self.theme.text;
        let surface_alt = self.theme.surface_alt;
        let border = self.theme.border;

        self.ids.push_u64(id.0);
        let radius = self.theme.radius;
        let trig_resp = self
            .btn_with("__dropdown_trig__", |ui| {
                ui.txt(current_label).fg(text_col);
                emit_chevron_icon(ui, "__dd_chev__", ChevronDir::Down, 12.0, text_col);
            })
            .bg(surface_alt)
            .fg(text_col)
            .bd(1.0, border)
            .r(radius);
        let trig_id = trig_resp.id();
        let trigger_btn_id = trig_id;
        let trigger_clicked = !disabled_prev && trig_resp.clicked();
        let trig_node_idx = self
            .nodes
            .iter()
            .rposition(|n| n.id == trig_id)
            .unwrap_or(0);
        self.nodes[trig_node_idx].label = String::from(current_label);
        self.ids.pop_id();

        if trigger_clicked {
            self.open_dropdown_id = if self.open_dropdown_id == Some(id) {
                None
            } else {
                Some(id)
            };
        }

        if self.open_dropdown_id == Some(id) {
            if self.dismiss_requested || disabled_prev {
                self.open_dropdown_id = None;
            } else {
                let trig_rect = self
                    .prev_rects
                    .get(&trigger_btn_id)
                    .copied()
                    .unwrap_or(Rect::new(0.0, 0.0, 160.0, 32.0));
                let surface = self.theme.surface;
                let surface_alt = self.theme.surface_alt;
                let border = self.theme.border;
                let primary = self.theme.primary;
                let on_primary = self.theme.on_primary;
                let text_col = self.theme.text;
                let radius = self.theme.radius;

                let menu_w = trig_rect
                    .w
                    .max(160.0)
                    .min((self.viewport_w - 16.0).max(80.0));
                let est_menu_h = (options.len() as f32) * 34.0 + 16.0;
                let menu_x = trig_rect
                    .x
                    .min((self.viewport_w - menu_w - 8.0).max(0.0))
                    .max(0.0);
                let menu_y = if trig_rect.y + trig_rect.h + 4.0 + est_menu_h > self.viewport_h
                    && trig_rect.y > est_menu_h + 8.0
                {
                    (trig_rect.y - est_menu_h - 4.0).max(0.0)
                } else {
                    trig_rect.y + trig_rect.h + 4.0
                };

                let mut next_selected: Option<usize> = None;
                self.overlay(|ui| {
                    ui.ids.push_u64(id.0);
                    for (i, &opt) in options.iter().enumerate() {
                        ui.ids.push_u64(i as u64);
                        let is_sel = i == *selected;
                        if ui
                            .btn(opt)
                            .bg(if is_sel { primary } else { surface_alt })
                            .fg(if is_sel { on_primary } else { text_col })
                            .r(6.0)
                            .clicked()
                        {
                            next_selected = Some(i);
                        }
                        ui.ids.pop_id();
                    }
                    ui.ids.pop_id();
                })
                .pos(menu_x, menu_y)
                .clamp_viewport()
                .w(menu_w)
                .p(6.0)
                .gap(4.0)
                .bg(surface)
                .bd(1.0, border)
                .r(radius)
                .shadow(12.0, Color::rgba(0, 0, 0, 110));

                if let Some(idx) = next_selected {
                    *selected = idx;
                    self.open_dropdown_id = None;
                    changed = true;
                }
            }
        }

        StyleBuilder {
            ui: self,
            node_idx: trig_node_idx,
            changed,
        }
    }

    /// Stateless single radio button option. Returns `StyleBuilder` with `.changed()` when clicked/activated.
    pub fn radio(&mut self, label: &str, selected: bool) -> StyleBuilder<'_> {
        let id = self.ids.id(label);
        let _ = self.ids.register_interactive(id);
        let disabled_prev = self.prev_disabled_ids.contains(&id);
        let clicked =
            !disabled_prev && (self.clicked_id == Some(id) || self.activated_id == Some(id));
        let focused = !disabled_prev && self.focused_id == Some(id);
        let hovered = !disabled_prev && self.hovered_id == Some(id);

        let primary = self.theme.primary;
        let border = self.theme.border;
        let focus_col = self.theme.focus_ring;
        let muted = self.theme.text_muted;
        let surface_alt = self.theme.surface_alt;
        let text_col = self.theme.text;

        let ring_bd_w = if focused || selected { 2.0 } else { 1.0 };
        let ring_bd_col = if focused {
            focus_col
        } else if selected {
            primary
        } else if hovered {
            muted
        } else {
            border
        };

        let mut builder = self.container_with_id(AxisKind::Row, label, id, |ui| {
            ui.zstack(|ui| {
                ui.txt("")
                    .w(18.0)
                    .h(18.0)
                    .bg(surface_alt)
                    .bd(ring_bd_w, ring_bd_col)
                    .circle();
                if selected {
                    ui.txt("").w(10.0).h(10.0).bg(primary).circle();
                }
            })
            .w(18.0)
            .h(18.0)
            .center();

            if !label.is_empty() {
                ui.txt(label).fg(text_col);
            }
        });
        let node = &mut builder.ui.nodes[builder.node_idx];
        node.role = Role::Toggle;
        node.semantic_value = Some(String::from(if selected { "true" } else { "false" }));
        node.focusable = true;
        node.hit_testable = true;
        node.style.gap = 8.0;
        node.style.pad_l = 4.0;
        node.style.pad_r = 4.0;
        node.style.pad_t = 2.0;
        node.style.pad_b = 2.0;
        node.style.radius = 6.0;
        builder.changed = clicked;
        builder
    }

    /// Vertical radio button group with Up/Down keyboard navigation. Returns `StyleBuilder` with `.changed()`.
    pub fn radio_group(
        &mut self,
        id_label: &str,
        selected: &mut usize,
        options: &[&str],
    ) -> StyleBuilder<'_> {
        let group_id = self.ids.id(id_label);
        let disabled_prev = self.prev_disabled_ids.contains(&group_id);
        let mut changed = false;

        self.ids.push_u64(group_id.0);
        let mut opt_ids = Vec::with_capacity(options.len());
        for (i, &opt) in options.iter().enumerate() {
            self.ids.push_u64(i as u64);
            opt_ids.push(self.ids.id(opt));
            self.ids.pop_id();
        }
        self.ids.pop_id();

        if !disabled_prev && !options.is_empty() {
            if *selected >= options.len() {
                *selected = options.len() - 1;
            }
            if self.focused_id.is_some_and(|fid| opt_ids.contains(&fid)) {
                match self.last_key_pressed {
                    Some(Key::Down) => {
                        let next = (*selected + 1) % options.len();
                        if next != *selected {
                            *selected = next;
                            self.focused_id = Some(opt_ids[next]);
                            changed = true;
                        }
                    }
                    Some(Key::Up) => {
                        let next = if *selected == 0 {
                            options.len() - 1
                        } else {
                            *selected - 1
                        };
                        if next != *selected {
                            *selected = next;
                            self.focused_id = Some(opt_ids[next]);
                            changed = true;
                        }
                    }
                    _ => {}
                }
            }
        }

        let mut picked = None;
        let mut builder = self.container_with_id(AxisKind::Col, id_label, group_id, |ui| {
            for (i, &opt) in options.iter().enumerate() {
                ui.ids.push_u64(i as u64);
                if ui.radio(opt, i == *selected).changed() && !disabled_prev {
                    picked = Some(i);
                }
                ui.ids.pop_id();
            }
        });
        if let Some(idx) = picked {
            if idx != *selected {
                *selected = idx;
                changed = true;
            }
        }
        builder = builder.gap(4.0);
        builder.changed = changed;
        builder
    }

    /// Searchable combobox dropdown filtering `options` live by `query`. Returns `StyleBuilder` with `.changed()`.
    pub fn combobox(
        &mut self,
        id_label: &str,
        selected: &mut usize,
        query: &mut TextEditState,
        options: &[&str],
    ) -> StyleBuilder<'_> {
        let id = self.ids.id(id_label);
        let disabled_prev = self.prev_disabled_ids.contains(&id);
        let mut changed = false;

        self.ids.push_u64(id.0);
        let input_id = self.ids.id("__combobox_input__");
        let toggle_btn_id = self.ids.id("__combo_toggle__");
        self.ids.pop_id();

        let placeholder_txt = options
            .get(*selected)
            .copied()
            .unwrap_or("Search...")
            .to_owned();
        let surface_alt = self.theme.surface_alt;
        let text_col = self.theme.text;
        let border = self.theme.border;
        let radius = self.theme.radius;

        let mut input_changed = false;
        let mut toggle_clicked = false;
        let combo_builder = self.container_with_id(AxisKind::Row, id_label, id, |ui| {
            let (input_idx, chg) = ui.text_input_node("__combobox_input__", query, false, false);
            ui.nodes[input_idx].style.placeholder = Some(placeholder_txt);
            ui.nodes[input_idx].style.grow = 1.0;
            input_changed = chg;
            if ui
                .btn_with("__combo_toggle__", |ui| {
                    emit_chevron_icon(ui, "__combo_chev__", ChevronDir::Down, 12.0, text_col);
                })
                .w(28.0)
                .px(0.0)
                .justify_center()
                .bg(surface_alt)
                .fg(text_col)
                .bd(1.0, border)
                .r(radius)
                .clicked()
            {
                toggle_clicked = true;
            }
        });
        let combo_node_idx = combo_builder.node_idx;
        combo_builder.gap(4.0).w(220.0);

        if input_changed {
            self.open_combobox_id = Some(id);
            self.combobox_highlight.insert(id, 0);
        }
        if !disabled_prev
            && (self.just_pressed_id == Some(input_id) || self.focused_id == Some(input_id))
        {
            self.open_combobox_id = Some(id);
        }
        if !disabled_prev && (toggle_clicked || self.clicked_id == Some(toggle_btn_id)) {
            self.open_combobox_id = if self.open_combobox_id == Some(id) {
                None
            } else {
                self.focused_id = Some(input_id);
                Some(id)
            };
        }

        let needle = query.text().to_ascii_lowercase();
        let filtered: Vec<(usize, &str)> = options
            .iter()
            .enumerate()
            .filter(|(_, opt)| needle.is_empty() || opt.to_ascii_lowercase().contains(&needle))
            .map(|(i, &o)| (i, o))
            .collect();

        if self.open_combobox_id == Some(id) {
            if self.dismiss_requested || disabled_prev {
                self.open_combobox_id = None;
            } else {
                if !filtered.is_empty() && self.focused_id == Some(input_id) {
                    let hl = self.combobox_highlight.entry(id).or_insert(0);
                    if *hl >= filtered.len() {
                        *hl = 0;
                    }
                    match self.last_key_pressed {
                        Some(Key::Down) => *hl = (*hl + 1) % filtered.len(),
                        Some(Key::Up) => {
                            *hl = if *hl == 0 {
                                filtered.len() - 1
                            } else {
                                *hl - 1
                            };
                        }
                        Some(Key::Enter) => {
                            let (orig_idx, _) = filtered[*hl];
                            if *selected != orig_idx {
                                *selected = orig_idx;
                                changed = true;
                            }
                            query.set_text("");
                            self.open_combobox_id = None;
                        }
                        _ => {}
                    }
                }

                if self.open_combobox_id == Some(id) {
                    let trig_rect = self
                        .prev_rects
                        .get(&id)
                        .or_else(|| self.prev_rects.get(&input_id))
                        .copied()
                        .unwrap_or(Rect::new(0.0, 0.0, 220.0, 34.0));
                    let surface = self.theme.surface;
                    let surface_alt = self.theme.surface_alt;
                    let border = self.theme.border;
                    let primary = self.theme.primary;
                    let on_primary = self.theme.on_primary;
                    let text_col = self.theme.text;
                    let muted = self.theme.text_muted;
                    let radius = self.theme.radius;
                    let hl_idx = self.combobox_highlight.get(&id).copied().unwrap_or(0);

                    let menu_w = trig_rect
                        .w
                        .max(180.0)
                        .min((self.viewport_w - 16.0).max(80.0));
                    let est_h = (filtered.len().clamp(1, 6) as f32) * 34.0 + 16.0;
                    let menu_x = trig_rect
                        .x
                        .min((self.viewport_w - menu_w - 8.0).max(0.0))
                        .max(0.0);
                    let menu_y = if trig_rect.y + trig_rect.h + 4.0 + est_h > self.viewport_h
                        && trig_rect.y > est_h + 8.0
                    {
                        (trig_rect.y - est_h - 4.0).max(0.0)
                    } else {
                        trig_rect.y + trig_rect.h + 4.0
                    };

                    let mut next_selected: Option<usize> = None;
                    self.overlay(|ui| {
                        ui.ids.push_u64(id.0);
                        if filtered.is_empty() {
                            ui.txt("No matches").sz(12.0).fg(muted).px(8.0).py(6.0);
                        } else {
                            ui.scroll("__combobox_list__", |ui| {
                                for (fi, &(orig_idx, opt)) in filtered.iter().enumerate() {
                                    ui.ids.push_u64(orig_idx as u64);
                                    let is_sel = orig_idx == *selected;
                                    let is_hl = fi == hl_idx;
                                    let bg = if is_sel {
                                        primary
                                    } else if is_hl {
                                        border
                                    } else {
                                        surface_alt
                                    };
                                    if ui
                                        .btn(opt)
                                        .bg(bg)
                                        .fg(if is_sel { on_primary } else { text_col })
                                        .r(6.0)
                                        .clicked()
                                    {
                                        next_selected = Some(orig_idx);
                                    }
                                    ui.ids.pop_id();
                                }
                            })
                            .max_h(210.0)
                            .gap(4.0);
                        }
                        ui.ids.pop_id();
                    })
                    .pos(menu_x, menu_y)
                    .clamp_viewport()
                    .w(menu_w)
                    .p(6.0)
                    .gap(4.0)
                    .bg(surface)
                    .bd(1.0, border)
                    .r(radius)
                    .shadow(12.0, Color::rgba(0, 0, 0, 110));

                    if let Some(orig_idx) = next_selected {
                        if *selected != orig_idx {
                            *selected = orig_idx;
                            changed = true;
                        }
                        query.set_text("");
                        self.open_combobox_id = None;
                    }
                }
            }
        }

        StyleBuilder {
            ui: self,
            node_idx: combo_node_idx,
            changed,
        }
    }

    /// Validated numeric input with `-` / `+` stepper buttons and keyboard arrow support.
    pub fn number_input(
        &mut self,
        id_label: &str,
        value: &mut f32,
        step: f32,
        range: RangeInclusive<f32>,
    ) -> StyleBuilder<'_> {
        let min_v = (*range.start()).min(*range.end());
        let max_v = (*range.start()).max(*range.end());
        *value = value.clamp(min_v, max_v);
        let step_amt = if step.is_finite() && step > 0.0 {
            step
        } else {
            1.0
        };

        let id = self.ids.id(id_label);
        let disabled_prev = self.prev_disabled_ids.contains(&id);
        self.ids.push_u64(id.0);
        let input_id = self.ids.id("__num_edit__");
        let dec_id = self.ids.id("-");
        let inc_id = self.ids.id("+");
        self.ids.pop_id();

        let mut changed = false;
        let dec_clicked = !disabled_prev
            && (self.clicked_id == Some(dec_id) || self.activated_id == Some(dec_id));
        let inc_clicked = !disabled_prev
            && (self.clicked_id == Some(inc_id) || self.activated_id == Some(inc_id));
        if dec_clicked {
            let next = (*value - step_amt).clamp(min_v, max_v);
            if (next - *value).abs() > f32::EPSILON {
                *value = next;
                changed = true;
            }
        }
        if inc_clicked {
            let next = (*value + step_amt).clamp(min_v, max_v);
            if (next - *value).abs() > f32::EPSILON {
                *value = next;
                changed = true;
            }
        }

        let input_focused = !disabled_prev && self.focused_id == Some(input_id);
        if input_focused {
            match self.last_key_pressed {
                Some(Key::Up) => {
                    let next = (*value + step_amt).clamp(min_v, max_v);
                    if (next - *value).abs() > f32::EPSILON {
                        *value = next;
                        changed = true;
                    }
                }
                Some(Key::Down) => {
                    let next = (*value - step_amt).clamp(min_v, max_v);
                    if (next - *value).abs() > f32::EPSILON {
                        *value = next;
                        changed = true;
                    }
                }
                _ => {}
            }
        }

        let mut edit_state = self
            .widget_text_states
            .remove(&id)
            .unwrap_or_else(|| TextEditState::with_text(&format_number_clean(*value)));
        if !input_focused || changed {
            let formatted = format_number_clean(*value);
            if edit_state.text() != formatted {
                edit_state.set_text(&formatted);
            }
        }

        let surface = self.theme.surface;
        let surface_alt = self.theme.surface_alt;
        let border = self.theme.border;
        let text_col = self.theme.text;
        let radius = self.theme.radius;

        let mut builder = self.container_with_id(AxisKind::Row, id_label, id, |ui| {
            ui.btn("-")
                .w(28.0)
                .h(30.0)
                .px(0.0)
                .py(0.0)
                .bg(surface_alt)
                .fg(text_col)
                .bd(1.0, border)
                .r((radius - 2.0).max(4.0));

            let (idx, text_changed) =
                ui.text_input_node("__num_edit__", &mut edit_state, false, false);
            ui.nodes[idx].style.grow = 1.0;
            ui.nodes[idx].style.height = Some(30.0);
            ui.nodes[idx].style.pad_t = 4.0;
            ui.nodes[idx].style.pad_b = 4.0;
            if text_changed {
                let filtered: String = edit_state
                    .text()
                    .chars()
                    .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
                    .collect();
                if filtered != edit_state.text() {
                    edit_state.set_text(&filtered);
                }
                if let Ok(parsed) = filtered.parse::<f32>() {
                    if parsed.is_finite() {
                        let clamped = parsed.clamp(min_v, max_v);
                        if (clamped - *value).abs() > f32::EPSILON {
                            *value = clamped;
                            changed = true;
                        }
                    }
                }
            }

            ui.btn("+")
                .w(28.0)
                .h(30.0)
                .px(0.0)
                .py(0.0)
                .bg(surface_alt)
                .fg(text_col)
                .bd(1.0, border)
                .r((radius - 2.0).max(4.0));
        });
        builder.ui.widget_text_states.insert(id, edit_state);
        builder = builder
            .w(160.0)
            .p(3.0)
            .gap(4.0)
            .bg(surface)
            .bd(1.0, border)
            .r(radius);
        builder.changed = changed;
        builder
    }

    /// Dual-thumb horizontal range slider (`low <= high`) over `range`. Returns `StyleBuilder` with `.changed()`.
    pub fn range_slider(
        &mut self,
        label: &str,
        low: &mut f32,
        high: &mut f32,
        range: RangeInclusive<f32>,
    ) -> StyleBuilder<'_> {
        let min_v = (*range.start()).min(*range.end());
        let max_v = (*range.start()).max(*range.end());
        let span = (max_v - min_v).max(1e-6);

        *low = low.clamp(min_v, max_v);
        *high = high.clamp(*low, max_v);

        let id = self.ids.id(label);
        let _ = self.ids.register_interactive(id);
        let disabled_prev = self.prev_disabled_ids.contains(&id);
        let mut changed = false;

        if !disabled_prev
            && (self.just_pressed_id == Some(id)
                || self.pressed_id == Some(id)
                || self.clicked_id == Some(id))
        {
            if let (Some([px, _]), Some(prev)) =
                (self.pointer_pos, self.prev_rects.get(&id).copied())
            {
                let t = ((px - prev.x) / prev.w.max(1.0)).clamp(0.0, 1.0);
                let val = min_v + t * span;
                let active_thumb = if self.just_pressed_id == Some(id)
                    || !self.range_drag_thumb.contains_key(&id)
                {
                    let d_low = (val - *low).abs();
                    let d_high = (val - *high).abs();
                    let pick = if d_low <= d_high { 0 } else { 1 };
                    self.range_drag_thumb.insert(id, pick);
                    pick
                } else {
                    self.range_drag_thumb.get(&id).copied().unwrap_or(0)
                };
                if active_thumb == 0 {
                    let next = val.min(*high);
                    if (next - *low).abs() > f32::EPSILON {
                        *low = next;
                        changed = true;
                    }
                } else {
                    let next = val.max(*low);
                    if (next - *high).abs() > f32::EPSILON {
                        *high = next;
                        changed = true;
                    }
                }
            }
        } else if !disabled_prev && self.focused_id == Some(id) {
            let step = span * 0.02;
            let active_thumb = self.range_drag_thumb.get(&id).copied().unwrap_or(1);
            match self.last_key_pressed {
                Some(Key::Right) => {
                    if active_thumb == 0 {
                        *low = (*low + step).min(*high);
                    } else {
                        *high = (*high + step).min(max_v);
                    }
                    changed = true;
                }
                Some(Key::Left) => {
                    if active_thumb == 0 {
                        *low = (*low - step).max(min_v);
                    } else {
                        *high = (*high - step).max(*low);
                    }
                    changed = true;
                }
                _ => {}
            }
        }

        let n_low = ((*low - min_v) / span).clamp(0.0, 1.0);
        let n_high = ((*high - min_v) / span).clamp(n_low, 1.0);
        let surface = self.theme.surface;
        let surface_alt = self.theme.surface_alt;
        let border_col = self.theme.border;
        let primary = self.theme.primary;
        let on_primary = self.theme.on_primary;
        let text_col = self.theme.text;
        let muted = self.theme.text_muted;
        let radius = self.theme.radius;
        let readout = format!("{low:.2} .. {high:.2}");

        let mut builder = self
            .col(|ui| {
                ui.row(|ui| {
                    ui.txt(label).fg(text_col).grow(1.0);
                    ui.txt(&readout).fg(muted).sz(12.0);
                })
                .gap(8.0);

                let focused = ui.focused_id == Some(id);
                let focus_col = ui.theme.focus_ring;
                let left_g = n_low.max(0.001);
                let mid_g = (n_high - n_low).max(0.02);
                let right_g = (1.0 - n_high).max(0.001);
                let track = ui.container_with_id(AxisKind::Row, label, id, |ui| {
                    ui.txt("").grow(left_g).h(8.0);
                    ui.txt("")
                        .w(10.0)
                        .h(14.0)
                        .bg(on_primary)
                        .bd(2.0, primary)
                        .circle();
                    ui.txt("").grow(mid_g).h(8.0).bg(primary).r(4.0);
                    ui.txt("")
                        .w(10.0)
                        .h(14.0)
                        .bg(on_primary)
                        .bd(2.0, primary)
                        .circle();
                    ui.txt("").grow(right_g).h(8.0);
                });
                let node = &mut track.ui.nodes[track.node_idx];
                node.role = Role::Slider;
                node.semantic_value = Some(readout);
                node.focusable = true;
                node.hit_testable = true;
                node.style.height = Some(18.0);
                node.style.pad_l = 4.0;
                node.style.pad_r = 4.0;
                node.style.pad_t = 2.0;
                node.style.pad_b = 2.0;
                node.style.bg = Some(surface_alt);
                node.style.border_width = if focused { 2.0 } else { 1.0 };
                node.style.border_color = if focused { focus_col } else { border_col };
                node.style.radius = 9.0;
            })
            .p(12.0)
            .gap(8.0)
            .bg(surface)
            .bd(1.0, border_col)
            .r(radius);
        builder.changed = changed;
        builder
    }

    /// Interactive RGBA / HSV color picker swatch with overlay SV plane, hue/alpha sliders, and hex input.
    pub fn color_picker(&mut self, id_label: &str, color: &mut Color) -> StyleBuilder<'_> {
        let id = self.ids.id(id_label);
        let disabled_prev = self.prev_disabled_ids.contains(&id);
        let mut changed = false;
        let current = *color;
        let hex_label = if current.a == 255 {
            format!("#{:02X}{:02X}{:02X}", current.r, current.g, current.b)
        } else {
            format!(
                "#{:02X}{:02X}{:02X}{:02X}",
                current.r, current.g, current.b, current.a
            )
        };

        let surface = self.theme.surface;
        let surface_alt = self.theme.surface_alt;
        let border = self.theme.border;
        let text_col = self.theme.text;
        let radius = self.theme.radius;

        let mut swatch_clicked = false;
        let builder = self.container_with_id(AxisKind::Row, id_label, id, |ui| {
            let mut btn = ui.btn_with("__color_swatch__", |ui| {
                ui.txt("")
                    .w(20.0)
                    .h(16.0)
                    .bg(current)
                    .bd(1.0, border)
                    .r(4.0);
                ui.txt(&hex_label).mono().sz(12.0).fg(text_col);
            });
            btn = btn
                .px(8.0)
                .py(5.0)
                .gap(8.0)
                .bg(surface_alt)
                .bd(1.0, border)
                .r(radius);
            if !disabled_prev && btn.clicked() {
                swatch_clicked = true;
            }
        });
        let trigger_idx = builder.node_idx;
        if swatch_clicked {
            builder.ui.open_color_picker_id = if builder.ui.open_color_picker_id == Some(id) {
                None
            } else {
                let hsv = rgb_to_hsv(current);
                builder.ui.color_picker_hsv.insert(id, hsv);
                Some(id)
            };
        }

        if self.open_color_picker_id == Some(id) {
            if self.dismiss_requested || disabled_prev {
                self.open_color_picker_id = None;
            } else {
                let trig_rect = self
                    .prev_rects
                    .get(&id)
                    .copied()
                    .unwrap_or(Rect::new(0.0, 0.0, 140.0, 30.0));
                let mut hsv = self
                    .color_picker_hsv
                    .get(&id)
                    .copied()
                    .unwrap_or_else(|| rgb_to_hsv(current));

                self.ids.push_u64(id.0);
                let pop_id = self.ids.id("__color_popover__");
                self.ids.push_u64(pop_id.0);
                let sv_id = self.ids.id("__color_sv__");
                let hue_id = self.ids.id("__color_hue__");
                let alpha_id = self.ids.id("__color_alpha__");
                let hex_id = self.ids.id("__color_hex__");
                let _ = self.ids.register_interactive(sv_id);
                let _ = self.ids.register_interactive(hue_id);
                let _ = self.ids.register_interactive(alpha_id);
                self.ids.pop_id();
                self.ids.pop_id();

                if let Some([px, py]) = self.pointer_pos {
                    if self.just_pressed_id == Some(sv_id) || self.pressed_id == Some(sv_id) {
                        if let Some(r) = self.prev_rects.get(&sv_id).copied() {
                            hsv[1] = ((px - r.x) / r.w.max(1.0)).clamp(0.0, 1.0);
                            hsv[2] = (1.0 - (py - r.y) / r.h.max(1.0)).clamp(0.0, 1.0);
                            let next = hsv_to_rgb(hsv[0], hsv[1], hsv[2], current.a);
                            if next != *color {
                                *color = next;
                                changed = true;
                            }
                        }
                    } else if self.just_pressed_id == Some(hue_id)
                        || self.pressed_id == Some(hue_id)
                    {
                        if let Some(r) = self.prev_rects.get(&hue_id).copied() {
                            hsv[0] = ((px - r.x) / r.w.max(1.0)).clamp(0.0, 1.0) * 360.0;
                            let next = hsv_to_rgb(hsv[0], hsv[1], hsv[2], current.a);
                            if next != *color {
                                *color = next;
                                changed = true;
                            }
                        }
                    } else if self.just_pressed_id == Some(alpha_id)
                        || self.pressed_id == Some(alpha_id)
                    {
                        if let Some(r) = self.prev_rects.get(&alpha_id).copied() {
                            let a =
                                (((px - r.x) / r.w.max(1.0)).clamp(0.0, 1.0) * 255.0).round() as u8;
                            let next = Color::rgba(color.r, color.g, color.b, a);
                            if next != *color {
                                *color = next;
                                changed = true;
                            }
                        }
                    }
                }
                self.color_picker_hsv.insert(id, hsv);

                let mut hex_state = self
                    .widget_text_states
                    .remove(&hex_id)
                    .unwrap_or_else(|| TextEditState::with_text(&hex_label));
                if self.focused_id != Some(hex_id) || changed {
                    let updated_hex = format!(
                        "#{:02X}{:02X}{:02X}{:02X}",
                        color.r, color.g, color.b, color.a
                    );
                    if hex_state.text() != updated_hex {
                        hex_state.set_text(&updated_hex);
                    }
                }

                // Build SV gradient + Hue bar Canvas2D commands.
                let sv_w = 180.0_f32;
                let sv_h = 120.0_f32;
                let mut sv_canvas = Canvas2D::new([0.0, 0.0]);
                let cols = 12usize;
                let rows = 8usize;
                let cw = sv_w / cols as f32;
                let ch = sv_h / rows as f32;
                for gy in 0..rows {
                    let v = 1.0 - (gy as f32 + 0.5) / rows as f32;
                    for gx in 0..cols {
                        let s = (gx as f32 + 0.5) / cols as f32;
                        sv_canvas.rect(
                            Rect::new(gx as f32 * cw, gy as f32 * ch, cw + 0.5, ch + 0.5),
                            hsv_to_rgb(hsv[0], s, v, 255),
                            0.0,
                        );
                    }
                }
                let hx = hsv[1] * sv_w;
                let hy = (1.0 - hsv[2]) * sv_h;
                sv_canvas.rect(Rect::new(hx - 4.0, hy - 4.0, 8.0, 8.0), Color::WHITE, 4.0);

                let hue_w = 180.0_f32;
                let hue_h = 14.0_f32;
                let mut hue_canvas = Canvas2D::new([0.0, 0.0]);
                let h_segs = 18usize;
                let hw = hue_w / h_segs as f32;
                for i in 0..h_segs {
                    let h_deg = (i as f32 + 0.5) / h_segs as f32 * 360.0;
                    hue_canvas.rect(
                        Rect::new(i as f32 * hw, 0.0, hw + 0.5, hue_h),
                        hsv_to_rgb(h_deg, 1.0, 1.0, 255),
                        0.0,
                    );
                }
                let hue_x = (hsv[0] / 360.0) * hue_w;
                hue_canvas.rect(Rect::new(hue_x - 2.0, 0.0, 4.0, hue_h), Color::WHITE, 2.0);

                let alpha_x = (f32::from(color.a) / 255.0) * hue_w;
                let mut alpha_canvas = Canvas2D::new([0.0, 0.0]);
                let a_segs = 12usize;
                let aw = hue_w / a_segs as f32;
                for i in 0..a_segs {
                    let a_val = ((i as f32 + 0.5) / a_segs as f32 * 255.0).round() as u8;
                    alpha_canvas.rect(
                        Rect::new(i as f32 * aw, 0.0, aw + 0.5, hue_h),
                        Color::rgba(color.r, color.g, color.b, a_val),
                        0.0,
                    );
                }
                alpha_canvas.rect(Rect::new(alpha_x - 2.0, 0.0, 4.0, hue_h), Color::WHITE, 2.0);

                let clicked_outside = if let (Some([px, py]), Some(pr)) =
                    (self.just_pressed_pos, self.prev_rects.get(&pop_id).copied())
                {
                    !pr.contains(px, py) && !trig_rect.contains(px, py)
                } else {
                    false
                };

                let mut hex_changed = false;
                let pop_y =
                    if trig_rect.y + trig_rect.h + 230.0 > self.viewport_h && trig_rect.y > 235.0 {
                        (trig_rect.y - 230.0).max(4.0)
                    } else {
                        trig_rect.y + trig_rect.h + 4.0
                    };

                let pop_builder =
                    self.build_container_overlay_with_id("__color_popover__", pop_id, |ui| {
                        for (cid, clabel, cw, ch, cmds) in [
                            (sv_id, "ColorSV", sv_w, sv_h, sv_canvas.cmds),
                            (hue_id, "ColorHue", hue_w, hue_h, hue_canvas.cmds),
                            (alpha_id, "ColorAlpha", hue_w, hue_h, alpha_canvas.cmds),
                        ] {
                            ui.alloc_node(
                                LayoutNode {
                                    id: cid,
                                    role: Role::Viewport,
                                    kind: AxisKind::Leaf,
                                    style: Style {
                                        width: Some(cw),
                                        height: Some(ch),
                                        radius: 4.0,
                                        border_width: 1.0,
                                        border_color: border,
                                        ..Style::default()
                                    },
                                    label: String::from(clabel),
                                    semantic_value: None,
                                    text: None,
                                    rich_spans: None,
                                    rich_links: Vec::new(),
                                    sparkline_samples: None,
                                    image_tex: None,
                                    viewport_data: Some((
                                        cmds,
                                        SceneSnapshot {
                                            camera: Camera3D::default(),
                                            draws: Vec::new(),
                                            msaa_samples: 4,
                                        },
                                    )),
                                    is_scroll: false,
                                    scroll_offset_y: 0.0,
                                    focusable: false,
                                    hit_testable: true,
                                    enabled: true,
                                    is_overlay: true,
                                    children: Vec::new(),
                                    intrinsic_w: cw,
                                    intrinsic_h: ch,
                                    content_w: cw,
                                    content_h: ch,
                                    rect: Rect::ZERO,
                                },
                                true,
                            );
                        }
                        let (h_idx, tc) =
                            ui.text_input_node("__color_hex__", &mut hex_state, false, false);
                        ui.nodes[h_idx].style.width = Some(180.0);
                        ui.nodes[h_idx].style.mono = true;
                        hex_changed = tc;
                    });
                pop_builder
                    .pos(trig_rect.x, pop_y)
                    .clamp_viewport()
                    .w(204.0)
                    .p(12.0)
                    .gap(8.0)
                    .bg(surface)
                    .bd(1.0, border)
                    .r(radius)
                    .shadow(14.0, Color::rgba(0, 0, 0, 130));

                if hex_changed {
                    if let Some(parsed) = parse_hex_color(&hex_state.text()) {
                        if parsed != *color {
                            *color = parsed;
                            self.color_picker_hsv.insert(id, rgb_to_hsv(parsed));
                            changed = true;
                        }
                    }
                }
                self.widget_text_states.insert(hex_id, hex_state);
                if clicked_outside {
                    self.open_color_picker_id = None;
                }
            }
        }

        StyleBuilder {
            ui: self,
            node_idx: trigger_idx,
            changed,
        }
    }
}

fn format_number_clean(v: f32) -> String {
    if (v.fract()).abs() < 1e-4 {
        format!("{:.0}", v)
    } else {
        let s = format!("{v:.3}");
        let trimmed = s.trim_end_matches('0').trim_end_matches('.');
        trimmed.to_owned()
    }
}

fn parse_hex_color(raw: &str) -> Option<Color> {
    let hex = raw.trim().strip_prefix('#').unwrap_or(raw.trim());
    match hex.len() {
        6 => u32::from_str_radix(hex, 16).ok().map(Color::from_hex),
        8 => u32::from_str_radix(hex, 16).ok().map(Color::from_hex_rgba),
        _ => None,
    }
}

fn rgb_to_hsv(c: Color) -> [f32; 3] {
    let r = f32::from(c.r) / 255.0;
    let g = f32::from(c.g) / 255.0;
    let b = f32::from(c.b) / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let h = if delta <= 1e-6 {
        0.0
    } else if (max - r).abs() <= 1e-6 {
        60.0 * (((g - b) / delta).rem_euclid(6.0))
    } else if (max - g).abs() <= 1e-6 {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    let s = if max <= 1e-6 { 0.0 } else { delta / max };
    [h, s, max]
}

fn hsv_to_rgb(h: f32, s: f32, v: f32, a: u8) -> Color {
    let h = h.rem_euclid(360.0);
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r1, g1, b1) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Color::rgba(
        ((r1 + m) * 255.0).round() as u8,
        ((g1 + m) * 255.0).round() as u8,
        ((b1 + m) * 255.0).round() as u8,
        a,
    )
}

fn format_key_name(key: Key) -> String {
    match key {
        Key::Tab => String::from("Tab"),
        Key::Enter => String::from("Enter"),
        Key::Space => String::from("Space"),
        Key::Escape => String::from("Esc"),
        Key::Backspace => String::from("Backspace"),
        Key::Delete => String::from("Delete"),
        Key::Insert => String::from("Insert"),
        Key::Left => String::from("Left"),
        Key::Right => String::from("Right"),
        Key::Up => String::from("Up"),
        Key::Down => String::from("Down"),
        Key::Home => String::from("Home"),
        Key::End => String::from("End"),
        Key::PageUp => String::from("PageUp"),
        Key::PageDown => String::from("PageDown"),
        Key::F1 => String::from("F1"),
        Key::F2 => String::from("F2"),
        Key::F3 => String::from("F3"),
        Key::F4 => String::from("F4"),
        Key::F5 => String::from("F5"),
        Key::F6 => String::from("F6"),
        Key::F7 => String::from("F7"),
        Key::F8 => String::from("F8"),
        Key::F9 => String::from("F9"),
        Key::F10 => String::from("F10"),
        Key::F11 => String::from("F11"),
        Key::F12 => String::from("F12"),
        Key::Char(c) => c.to_uppercase().to_string(),
        Key::Other(code) => format!("Key({code})"),
    }
}
