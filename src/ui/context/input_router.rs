use super::*;

impl HitNode {
    fn contains_point(&self, px: f32, py: f32) -> bool {
        let clip_ok = self.clip.is_none_or(|c| c.contains(px, py));
        if !clip_ok {
            return false;
        }
        if let Some([cx, cy, sc, rot]) = self.inv_xform {
            let safe_sc = if sc.is_finite() && sc > 0.001 {
                sc
            } else {
                1.0
            };
            let dx = (px - cx) / safe_sc;
            let dy = (py - cy) / safe_sc;
            let cs = rot.cos();
            let sn = rot.sin();
            let lx = cx + (dx * cs + dy * sn);
            let ly = cy + (-dx * sn + dy * cs);
            self.bounds.contains(lx, ly)
        } else {
            self.bounds.contains(px, py)
        }
    }
}

impl UiContext {
    /// Processes a pointer move event in logical points.
    ///
    /// Returns `true` only when hover changes, dragging is active, or `cursor_glow` is visible.
    pub fn on_pointer_move(&mut self, x: f32, y: f32) -> bool {
        if let Some([px, py]) = self.prev_pointer_pos {
            self.frame_pointer_delta[0] += x - px;
            self.frame_pointer_delta[1] += y - py;
        }
        self.prev_pointer_pos = Some([x, y]);
        self.pointer_pos = Some([x, y]);

        let next_hover = self.hit_test(x, y);
        let hover_changed = next_hover != self.hovered_id;
        self.hovered_id = next_hover;
        if next_hover != self.hover_anchor_id {
            self.hover_anchor_id = next_hover;
            self.hover_elapsed_s = 0.0;
        }
        let needs_redraw =
            hover_changed || self.pressed_id.is_some() || self.has_active_cursor_glow;
        if needs_redraw {
            self.dirty = true;
        }
        needs_redraw
    }

    /// Clears active pointer coordinates, hover state, and tooltip timers when the cursor leaves the surface.
    ///
    /// Returns `true` if clearing hover, cursor glow, or tooltip state requires a redraw.
    pub fn on_pointer_leave(&mut self) -> bool {
        let had_pointer = self.pointer_pos.take().is_some();
        self.prev_pointer_pos = None;
        let had_hover = self.hovered_id.take().is_some();
        let had_anchor = self.hover_anchor_id.take().is_some();
        let had_tooltip = self.pending_tooltip.take().is_some() || self.has_active_tooltip_timer;
        self.hover_elapsed_s = 0.0;
        self.has_active_tooltip_timer = false;
        let needs_redraw =
            had_hover || had_tooltip || (had_pointer && self.has_active_cursor_glow) || had_anchor;
        if needs_redraw {
            self.dirty = true;
        }
        needs_redraw
    }

    /// Cancels any active pointer press or drag interaction without emitting click callbacks.
    ///
    /// Returns `true` if an active press or drag state was cleared and requires a redraw.
    pub fn on_pointer_cancel(&mut self) -> bool {
        let mut had_scrollbar_drag = false;
        for sc in self.scroll_states.values_mut() {
            if sc.scrollbar_drag_offset.take().is_some() {
                had_scrollbar_drag = true;
            }
        }
        let had_pressed = self.pressed_id.take().is_some();
        let had_right = self.pressed_right_id.take().is_some();
        let had_just_pressed = self.just_pressed_id.take().is_some();
        let had_table_drag = self.table_resize_drag.take().is_some();
        let had_range_drag = !self.range_drag_thumb.is_empty();
        let had_text_drag = self.text_select_anchor.take().is_some();
        self.just_pressed_pos = None;
        self.frame_pointer_delta = [0.0, 0.0];
        self.range_drag_thumb.clear();
        let changed = had_pressed
            || had_right
            || had_just_pressed
            || had_table_drag
            || had_range_drag
            || had_text_drag
            || had_scrollbar_drag;
        if changed {
            self.dirty = true;
        }
        changed
    }

    /// Processes a scroll wheel delta in logical scroll lines/units.
    pub fn on_scroll(&mut self, dx: f32, dy: f32) {
        self.frame_scroll_delta[0] += dx;
        self.frame_scroll_delta[1] += dy;
        self.dirty = true;
    }

    /// Processes a pointer button press at the current cursor position.
    pub fn on_pointer_down(&mut self, button: PointerButton) {
        self.hover_elapsed_s = 0.0;
        self.frame_pointer_delta = [0.0, 0.0];
        let Some([px, py]) = self.pointer_pos else {
            return;
        };
        let hit = self.hit_test_node(px, py);
        match button {
            PointerButton::Left => {
                self.just_pressed_id = hit.as_ref().map(|h| h.id);
                self.just_pressed_pos = Some([px, py]);
                self.pressed_id = hit.as_ref().map(|h| h.id);
                if let Some(node) = hit {
                    if node.focusable && node.enabled {
                        self.focused_id = Some(node.id);
                        self.focus_visible = false;
                        self.sync_focused_is_text();
                    }
                    if node.ripple {
                        self.ripples.push(RippleEffect {
                            id: node.id,
                            center: [px, py],
                            progress: 0.0,
                        });
                    }
                }
            }
            PointerButton::Right => {
                self.pressed_right_id = hit.as_ref().map(|h| h.id);
            }
            PointerButton::Middle => {}
        }
        self.dirty = true;
    }

    /// Processes a pointer button release at the current cursor position.
    pub fn on_pointer_up(&mut self, button: PointerButton) {
        match button {
            PointerButton::Left => {
                let pressed = self.pressed_id.take();
                if let (Some(pressed_id), Some([px, py])) = (pressed, self.pointer_pos) {
                    if self.hit_test(px, py) == Some(pressed_id) {
                        self.clicked_id = Some(pressed_id);
                        if self.last_click_id == Some(pressed_id)
                            && (self.elapsed_time_s - self.last_click_time_s)
                                <= DOUBLE_CLICK_WINDOW_S
                        {
                            self.double_clicked_id = Some(pressed_id);
                            self.last_click_id = None;
                        } else {
                            self.last_click_id = Some(pressed_id);
                            self.last_click_time_s = self.elapsed_time_s;
                        }
                    }
                }
                self.dirty = true;
            }
            PointerButton::Right => {
                let pressed = self.pressed_right_id.take();
                if let (Some(pressed_id), Some([px, py])) = (pressed, self.pointer_pos) {
                    if self.hit_test(px, py) == Some(pressed_id) {
                        self.right_clicked_id = Some(pressed_id);
                    }
                }
                self.dirty = true;
            }
            PointerButton::Middle => {}
        }
    }

    /// Processes a keyboard press for keybind capture, text input, focus traversal, and `Esc`.
    pub fn on_key_down(&mut self, key: Key, modifiers: Modifiers) {
        self.last_modifiers = modifiers;
        if let Some(cap_id) = self.capturing_keybind_id.take() {
            if key != Key::Escape {
                self.captured_key = Some((cap_id, key));
            }
            self.dirty = true;
            return;
        }

        if key == Key::F12 && self.hud_hotkey_enabled {
            self.hud_visible = !self.hud_visible;
            self.dirty = true;
            return;
        }

        self.last_key_pressed = Some(key);
        self.pending_key_events.push((key, modifiers));

        match key {
            Key::Tab => {
                self.cycle_focus(modifiers.shift);
                self.dirty = true;
            }
            Key::Enter | Key::Space => {
                if !self.focused_is_text {
                    if let Some(fid) = self.focused_id {
                        self.activated_id = Some(fid);
                        self.dirty = true;
                    }
                } else {
                    self.dirty = true;
                }
            }
            Key::Escape => {
                self.dismiss_requested = true;
                self.dirty = true;
            }
            _ => {
                self.dirty = true;
            }
        }
    }

    /// Processes an IME composition preedit string.
    pub fn on_ime_preedit(&mut self, text: String) {
        self.pending_ime_preedit = Some(text);
        self.dirty = true;
    }

    /// Processes an IME committed text string.
    pub fn on_ime_commit(&mut self, text: String) {
        self.pending_ime_preedit = None;
        self.pending_ime_commit = Some(text);
        self.dirty = true;
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn set_viewport_fbo_id(&mut self, fbo_id: Option<u32>) {
        self.viewport_raw_fbo_id = fbo_id;
    }

    pub(super) fn sync_focused_is_text(&mut self) {
        self.focused_is_text = self.focused_id.is_some_and(|fid| {
            self.semantics
                .iter()
                .any(|n| n.id == fid && n.role == Role::TextInput)
        });
    }

    fn cycle_focus(&mut self, reverse: bool) {
        let focusable: Vec<WidgetId> = self
            .semantics
            .iter()
            .filter(|n| n.focusable && n.enabled)
            .map(|n| n.id)
            .collect();

        if focusable.is_empty() {
            self.focused_id = None;
            self.focused_is_text = false;
            return;
        }

        let current_idx = self
            .focused_id
            .and_then(|fid| focusable.iter().position(|&id| id == fid));

        let next_idx = match (current_idx, reverse) {
            (None, false) => 0,
            (None, true) => focusable.len() - 1,
            (Some(i), false) => (i + 1) % focusable.len(),
            (Some(0), true) => focusable.len() - 1,
            (Some(i), true) => i - 1,
        };
        self.focused_id = Some(focusable[next_idx]);
        self.focus_visible = true;
        self.sync_focused_is_text();
    }

    fn hit_test_node(&self, px: f32, py: f32) -> Option<HitNode> {
        for node in self.hits.iter().rev() {
            if node.is_overlay && node.enabled && node.contains_point(px, py) {
                return Some(node.clone());
            }
        }
        for node in self.hits.iter().rev() {
            if !node.is_overlay && node.enabled && node.contains_point(px, py) {
                return Some(node.clone());
            }
        }
        None
    }

    pub(super) fn hit_test(&self, px: f32, py: f32) -> Option<WidgetId> {
        self.hit_test_node(px, py).map(|n| n.id)
    }

    pub(super) fn draw_list_contains_point(&self, px: f32, py: f32) -> bool {
        fn rect_contains_transformed(rect: Rect, transform: [f32; 4], px: f32, py: f32) -> bool {
            let [scale, rot, tx, ty] = transform;
            let moved = Rect::new(rect.x + tx, rect.y + ty, rect.w, rect.h);
            if (scale - 1.0).abs() <= 1e-4 && rot.abs() <= 1e-4 {
                return moved.contains(px, py);
            }
            let safe_sc = if scale.is_finite() && scale > 0.001 {
                scale
            } else {
                1.0
            };
            let cx = moved.x + moved.w * 0.5;
            let cy = moved.y + moved.h * 0.5;
            let dx = (px - cx) / safe_sc;
            let dy = (py - cy) / safe_sc;
            let cs = rot.cos();
            let sn = rot.sin();
            let lx = cx + (dx * cs + dy * sn);
            let ly = cy + (-dx * sn + dy * cs);
            moved.contains(lx, ly)
        }

        fn cmds_hit(cmds: &[DrawCmd], px: f32, py: f32) -> bool {
            let mut clips: Vec<Rect> = Vec::new();
            for cmd in cmds {
                match cmd {
                    DrawCmd::PushClip(r) => {
                        let next = clips.last().map_or(*r, |prev| prev.intersect(*r));
                        clips.push(next);
                    }
                    DrawCmd::PopClip => {
                        clips.pop();
                    }
                    DrawCmd::Rect {
                        rect,
                        bg,
                        bg_grad,
                        border_width,
                        border_color,
                        bd_grad,
                        glass,
                        transform,
                        ..
                    } => {
                        let visible = bg.a > 0
                            || bg_grad.is_some()
                            || *glass
                            || (*border_width > 0.0 && (border_color.a > 0 || bd_grad.is_some()));
                        if !visible {
                            continue;
                        }
                        if clips.last().is_none_or(|c| c.contains(px, py))
                            && rect_contains_transformed(*rect, *transform, px, py)
                        {
                            return true;
                        }
                    }
                    DrawCmd::Image {
                        rect,
                        tint,
                        border_width,
                        border_color,
                        transform,
                        ..
                    } => {
                        let visible = tint.a > 0 || (*border_width > 0.0 && border_color.a > 0);
                        if visible
                            && clips.last().is_none_or(|c| c.contains(px, py))
                            && rect_contains_transformed(*rect, *transform, px, py)
                        {
                            return true;
                        }
                    }
                    DrawCmd::Text {
                        rect,
                        color,
                        transform,
                        ..
                    } => {
                        if color.a > 0
                            && clips.last().is_none_or(|c| c.contains(px, py))
                            && rect_contains_transformed(*rect, *transform, px, py)
                        {
                            return true;
                        }
                    }
                    DrawCmd::RichText {
                        rect, transform, ..
                    } => {
                        if clips.last().is_none_or(|c| c.contains(px, py))
                            && rect_contains_transformed(*rect, *transform, px, py)
                        {
                            return true;
                        }
                    }
                    DrawCmd::Sparkline { rect, .. } | DrawCmd::Viewport { rect, .. } => {
                        if clips.last().is_none_or(|c| c.contains(px, py)) && rect.contains(px, py)
                        {
                            return true;
                        }
                    }
                }
            }
            false
        }

        cmds_hit(&self.draw_list.overlay_cmds, px, py)
            || cmds_hit(&self.draw_list.base_cmds, px, py)
    }
}
