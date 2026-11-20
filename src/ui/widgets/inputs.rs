use super::*;

fn apply_text_input_events(
    state: &mut TextEditState,
    preedit: Option<&str>,
    commit: Option<&str>,
    events: &[(Key, Modifiers)],
    multiline: bool,
    masked: bool,
    submit_on_enter: bool,
) -> (bool, bool) {
    let before = state.text();
    let mut submitted = false;

    if let Some(pre) = preedit {
        if pre.is_empty() {
            state.clear_ime_preedit();
        } else {
            state.set_ime_preedit(String::from(pre));
        }
    }
    if let Some(committed) = commit {
        state.commit_ime(committed);
    }

    for &(key, mods) in events {
        let accel = (mods.ctrl || mods.super_key) && !mods.alt;
        if accel {
            match key {
                Key::Char('a' | 'A') => state.select_all(),
                Key::Char('c' | 'C') if !masked => {
                    if let Some(sel) = state.copy_selection() {
                        crate::os::clipboard::set_text(&sel);
                    }
                }
                Key::Char('x' | 'X') if !masked => {
                    if let Some(sel) = state.cut_selection() {
                        crate::os::clipboard::set_text(&sel);
                    }
                }
                Key::Char('v' | 'V') => {
                    if let Some(clip) = crate::os::clipboard::get_text() {
                        state.paste(&clip);
                    }
                }
                Key::Char('z' | 'Z') => {
                    if mods.shift {
                        let _ = state.redo();
                    } else {
                        let _ = state.undo();
                    }
                }
                Key::Char('y' | 'Y') => {
                    let _ = state.redo();
                }
                Key::Left => state.move_word_left(mods.shift),
                Key::Right => state.move_word_right(mods.shift),
                Key::Backspace => state.backspace_word(),
                Key::Delete => state.delete_word_forward(),
                Key::Home => state.move_home(mods.shift),
                Key::End => state.move_end(mods.shift),
                _ => {}
            }
            continue;
        }

        if mods.ctrl || mods.alt || mods.super_key {
            continue;
        }

        match key {
            Key::Backspace => state.backspace(),
            Key::Delete => state.delete_forward(),
            Key::Left => state.move_left(mods.shift),
            Key::Right => state.move_right(mods.shift),
            Key::Up if multiline => state.move_up(mods.shift),
            Key::Down if multiline => state.move_down(mods.shift),
            Key::Home => state.move_home(mods.shift),
            Key::End => state.move_end(mods.shift),
            Key::Space => state.insert_str(" "),
            Key::Enter if multiline && (!submit_on_enter || mods.shift) => state.insert_str("\n"),
            Key::Enter if !multiline || submit_on_enter => {
                submitted = true;
            }
            Key::Char(ch) if !ch.is_control() => {
                let mut utf8 = [0u8; 4];
                state.insert_str(ch.encode_utf8(&mut utf8));
            }
            _ => {}
        }
    }

    (state.text() != before, submitted)
}

impl UiContext {
    pub(super) fn text_input_node(
        &mut self,
        id_label: &str,
        state: &mut TextEditState,
        multiline: bool,
        masked: bool,
    ) -> (usize, bool) {
        let id = self.ids.id(id_label);
        let _ = self.ids.register_interactive(id);
        let disabled_prev = self.prev_disabled_ids.contains(&id);
        let focused = !disabled_prev && self.focused_id == Some(id);
        let hovered = !disabled_prev && self.hovered_id == Some(id);

        let mut changed = false;
        if !disabled_prev {
            if self.double_clicked_id == Some(id) {
                state.select_all();
            } else if let (Some([px, py]), Some(prev_r)) =
                (self.pointer_pos, self.prev_rects.get(&id).copied())
            {
                if self.just_pressed_id == Some(id) {
                    let off = byte_offset_at_point(
                        &state.text(),
                        prev_r,
                        px,
                        py,
                        self.theme.font_size,
                        multiline,
                    );
                    if self.last_modifiers.shift {
                        state.select_to(off);
                    } else {
                        state.set_cursor(off);
                    }
                } else if self.pressed_id == Some(id) {
                    let off = byte_offset_at_point(
                        &state.text(),
                        prev_r,
                        px,
                        py,
                        self.theme.font_size,
                        multiline,
                    );
                    state.select_to(off);
                }
            }
        }
        if focused {
            self.focused_is_text = true;
            let preedit = self.pending_ime_preedit.take();
            let commit = self.pending_ime_commit.take();
            let events = core::mem::take(&mut self.pending_key_events);
            let submit_on_enter = self.prev_submit_on_enter_ids.contains(&id);
            let (text_changed, submitted) = apply_text_input_events(
                state,
                preedit.as_deref(),
                commit.as_deref(),
                &events,
                multiline,
                masked,
                submit_on_enter,
            );
            changed = text_changed;
            if submitted {
                self.submitted_ids.insert(id);
            }
            let caret_pos = if masked {
                let raw = state.text();
                let clamped = state.cursor().min(raw.len());
                let prefix_chars = raw[..clamped].chars().count();
                let preedit_chars = state.ime_preedit().map_or(0, |p| p.chars().count());
                prefix_chars + preedit_chars
            } else {
                state.cursor() + state.ime_preedit().map_or(0, str::len)
            };
            self.focused_caret = Some((id, caret_pos));
            self.focused_selection = state.selection_range().map(|(s0, s1)| {
                if masked {
                    let raw = state.text();
                    let c0 = raw[..s0.min(raw.len())].chars().count();
                    let c1 = raw[..s1.min(raw.len())].chars().count();
                    (id, c0, c1)
                } else {
                    (id, s0, s1)
                }
            });
        }

        let (display, semantic_val) = if masked {
            let disp_len = state.display_text().chars().count();
            let sem_len = state.text().chars().count();
            ("*".repeat(disp_len), "*".repeat(sem_len))
        } else {
            (state.display_text(), state.text())
        };
        let border_col = if focused {
            self.theme.primary
        } else if hovered {
            self.theme.text_muted
        } else {
            self.theme.border
        };

        let style = Style {
            pad_l: 10.0,
            pad_r: 10.0,
            pad_t: 7.0,
            pad_b: 7.0,
            bg: Some(self.theme.surface_alt),
            fg: Some(self.theme.text),
            border_width: if focused { 2.0 } else { 1.0 },
            border_color: border_col,
            radius: self.theme.radius,
            font_size: Some(self.theme.font_size),
            wrap: multiline,
            ellipsis: !multiline && !focused,
            ..Style::default()
        };

        let idx = self.alloc_node(
            LayoutNode {
                id,
                role: Role::TextInput,
                kind: AxisKind::Leaf,
                style,
                label: String::from(id_label),
                semantic_value: Some(semantic_val),
                text: Some(display),
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
                intrinsic_w: 0.0,
                intrinsic_h: 0.0,
                content_w: 0.0,
                content_h: 0.0,
                rect: Rect::ZERO,
            },
            true,
        );

        (idx, changed)
    }

    /// Single-line focusable text input backed by a UTF-8 gap buffer, IME, clipboard, and undo history.
    ///
    /// Returns a `StyleBuilder` (with `.changed()`, `.placeholder(text)`, `.w()`, `.h()`, `.bg()`, `.bd()`, `.r()`, `.enabled()`).
    pub fn input(&mut self, id_label: &str, state: &mut TextEditState) -> StyleBuilder<'_> {
        let (idx, changed) = self.text_input_node(id_label, state, false, false);
        StyleBuilder {
            ui: self,
            node_idx: idx,
            changed,
        }
    }

    /// Single-line focusable password input masking displayed characters with `*` and disabling clipboard copy/cut.
    ///
    /// Returns a `StyleBuilder` (with `.changed()`, `.placeholder(text)`, `.w()`, `.h()`, `.bg()`, `.bd()`, `.r()`, `.enabled()`).
    pub fn password_input(
        &mut self,
        id_label: &str,
        state: &mut TextEditState,
    ) -> StyleBuilder<'_> {
        let (idx, changed) = self.text_input_node(id_label, state, false, true);
        StyleBuilder {
            ui: self,
            node_idx: idx,
            changed,
        }
    }

    /// Multi-line focusable text editor backed by a UTF-8 gap buffer, IME, clipboard, and undo history.
    ///
    /// # Applicable Modifiers
    /// - `.changed()`, `.submit_on_enter()`, `.submitted()`, `.caret_rect()`
    /// - `.placeholder(text)`, `.mono()`, `.sz(pt)`, `.selection_bg(color)`
    /// - `.w(w)`, `.h(h)`, `.bg(color)`, `.bd(w, color)`, `.focus_bd(color)`, `.r(radius)`, `.enabled(bool)`
    pub fn textarea(&mut self, id_label: &str, state: &mut TextEditState) -> StyleBuilder<'_> {
        let (idx, changed) = self.text_input_node(id_label, state, true, false);
        StyleBuilder {
            ui: self,
            node_idx: idx,
            changed,
        }
    }
}

fn byte_offset_at_point(
    txt: &str,
    rect: Rect,
    px: f32,
    py: f32,
    font_sz: f32,
    multiline: bool,
) -> usize {
    let char_w = (font_sz * 0.56).max(1.0);
    let line_h = (font_sz * 1.35).max(1.0);
    let text_x = rect.x + 10.0;
    let text_y = if multiline {
        rect.y + 7.0
    } else {
        rect.y + ((rect.h - line_h).max(0.0) * 0.5)
    };
    let col = ((px - text_x) / char_w).round().max(0.0) as usize;
    if !multiline {
        return txt
            .char_indices()
            .nth(col)
            .map_or(txt.len(), |(idx, _)| idx);
    }
    let row = ((py - text_y) / line_h).floor().max(0.0) as usize;
    let mut line_start = 0usize;
    for (cur_row, line) in txt.split('\n').enumerate() {
        if cur_row == row {
            let rel = line
                .char_indices()
                .nth(col)
                .map_or(line.len(), |(idx, _)| idx);
            return line_start + rel;
        }
        line_start += line.len() + 1;
    }
    txt.len()
}
