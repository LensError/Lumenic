use crate::sys::history::History;

fn is_grapheme_extend(ch: char) -> bool {
    matches!(
        ch,
        '\u{0300}'..='\u{036F}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FE20}'..='\u{FE2F}'
            | '\u{1F3FB}'..='\u{1F3FF}'
            | '\u{E0100}'..='\u{E01EF}'
    )
}

fn prev_grapheme_start(left: &str) -> usize {
    if left.is_empty() {
        return 0;
    }
    if left.ends_with("\r\n") {
        return left.len() - 2;
    }
    let mut iter = left.char_indices().rev();
    let Some((mut start, mut ch)) = iter.next() else {
        return 0;
    };
    loop {
        while is_grapheme_extend(ch) {
            if let Some((prev_i, prev_ch)) = iter.next() {
                start = prev_i;
                ch = prev_ch;
            } else {
                return 0;
            }
        }
        // Check if preceded by a ZWJ joining sequence.
        if let Some((zwj_i, '\u{200D}')) = iter.clone().next() {
            iter.next();
            if let Some((prev_i, prev_ch)) = iter.next() {
                start = prev_i;
                ch = prev_ch;
                continue;
            }
            return zwj_i;
        }
        return start;
    }
}

fn next_grapheme_end(right: &str) -> usize {
    if right.is_empty() {
        return 0;
    }
    if right.starts_with("\r\n") {
        return 2;
    }
    let mut iter = right.char_indices();
    let Some((_, first_ch)) = iter.next() else {
        return right.len();
    };
    let mut end = first_ch.len_utf8();

    loop {
        while let Some((idx, ch)) = iter.clone().next() {
            if is_grapheme_extend(ch) {
                iter.next();
                end = idx + ch.len_utf8();
            } else {
                break;
            }
        }
        if let Some((_, '\u{200D}')) = iter.clone().next() {
            let _ = iter.next();
            if let Some((next_idx, next_ch)) = iter.next() {
                end = next_idx + next_ch.len_utf8();
                continue;
            }
            end += '\u{200D}'.len_utf8();
        }
        return end;
    }
}

fn floor_char_boundary(s: &str, mut idx: usize) -> usize {
    idx = idx.min(s.len());
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn prev_word_start(left: &str) -> usize {
    let mut iter = left.char_indices().rev();
    let Some((mut pos, mut ch)) = iter.next() else {
        return 0;
    };
    while !is_word_char(ch) {
        if let Some((p, c)) = iter.next() {
            pos = p;
            ch = c;
        } else {
            return 0;
        }
    }
    while is_word_char(ch) {
        if let Some((p, c)) = iter.next() {
            pos = p;
            ch = c;
        } else {
            return 0;
        }
    }
    pos + ch.len_utf8()
}

fn next_word_end(right: &str) -> usize {
    let mut iter = right.char_indices();
    let Some((mut pos, mut ch)) = iter.next() else {
        return 0;
    };
    if is_word_char(ch) {
        for (p, c) in iter {
            if !is_word_char(c) {
                return p;
            }
            pos = p;
            ch = c;
        }
        return pos + ch.len_utf8();
    }
    while !is_word_char(ch) {
        if let Some((p, c)) = iter.next() {
            pos = p;
            ch = c;
        } else {
            return right.len();
        }
    }
    for (p, c) in iter {
        if !is_word_char(c) {
            return p;
        }
        pos = p;
        ch = c;
    }
    pos + ch.len_utf8()
}

/// Gap-buffer text editor state with grapheme-aware cursor, selection, undo/redo, and IME preedit.
#[derive(Clone, Debug)]
pub struct TextEditState {
    buf: Vec<u8>,
    gap_start: usize,
    gap_end: usize,
    selection_anchor: Option<usize>,
    ime_preedit: Option<String>,
    history: History<(String, usize)>,
}

impl Default for TextEditState {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEditState {
    /// Creates an empty text buffer.
    pub fn new() -> Self {
        Self::with_text("")
    }

    /// Creates a text buffer initialized with `s` and cursor placed at the end.
    pub fn with_text(s: &str) -> Self {
        let cap = s.len().checked_add(64).unwrap_or(s.len());
        let mut buf = vec![0u8; cap];
        buf[..s.len()].copy_from_slice(s.as_bytes());
        Self {
            buf,
            gap_start: s.len(),
            gap_end: cap,
            selection_anchor: None,
            ime_preedit: None,
            history: History::new(128),
        }
    }

    /// Returns the total byte length of the logical text.
    pub fn len_bytes(&self) -> usize {
        self.buf.len() - (self.gap_end - self.gap_start)
    }

    /// Returns `true` when the logical buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.len_bytes() == 0
    }

    /// Returns the current cursor byte offset within `[0, len_bytes()]`.
    pub fn cursor(&self) -> usize {
        self.gap_start
    }

    /// Materializes the current buffer contents as a UTF-8 `String`.
    pub fn text(&self) -> String {
        let mut out = String::with_capacity(self.len_bytes());
        out.push_str(self.left_str());
        out.push_str(self.right_str());
        out
    }

    /// Replaces the entire buffer contents and places the cursor at the end.
    pub fn set_text(&mut self, s: &str) {
        self.record_history();
        self.buf.clear();
        let cap = s.len().checked_add(64).unwrap_or(s.len());
        self.buf.resize(cap, 0);
        self.buf[..s.len()].copy_from_slice(s.as_bytes());
        self.gap_start = s.len();
        self.gap_end = cap;
        self.selection_anchor = None;
        self.ime_preedit = None;
    }

    fn left_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.gap_start]).unwrap_or("")
    }

    fn right_str(&self) -> &str {
        core::str::from_utf8(&self.buf[self.gap_end..]).unwrap_or("")
    }

    fn record_history(&mut self) {
        let snap = (self.text(), self.cursor());
        self.history.record(snap);
    }

    fn ensure_gap(&mut self, needed: usize) {
        let avail = self.gap_end - self.gap_start;
        if avail >= needed {
            return;
        }
        let extra = needed.saturating_sub(avail).max(64);
        let old_len = self.buf.len();
        let new_len = old_len
            .checked_add(extra)
            .expect("gap buffer capacity overflow");
        let right_len = old_len - self.gap_end;
        self.buf.resize(new_len, 0);
        if right_len > 0 {
            let new_gap_end = new_len - right_len;
            self.buf.copy_within(self.gap_end..old_len, new_gap_end);
            self.gap_end = new_gap_end;
        } else {
            self.gap_end = new_len;
        }
    }

    fn move_gap_to(&mut self, byte_pos: usize) {
        let target = byte_pos.min(self.len_bytes());
        if target < self.gap_start {
            let safe_target = floor_char_boundary(self.left_str(), target);
            let delta = self.gap_start - safe_target;
            let new_end = self.gap_end - delta;
            self.buf.copy_within(safe_target..self.gap_start, new_end);
            self.gap_start = safe_target;
            self.gap_end = new_end;
        } else if target > self.gap_start {
            let rel = target - self.gap_start;
            let safe_rel = floor_char_boundary(self.right_str(), rel);
            let src_start = self.gap_end;
            let src_end = self.gap_end + safe_rel;
            self.buf.copy_within(src_start..src_end, self.gap_start);
            self.gap_start += safe_rel;
            self.gap_end += safe_rel;
        }
    }

    /// Moves the cursor to `byte_offset` (snapped to a valid UTF-8 boundary) and clears selection.
    pub fn set_cursor(&mut self, byte_offset: usize) {
        self.selection_anchor = None;
        self.move_gap_to(byte_offset);
    }

    /// Extends or starts selection from the current anchor to `byte_offset`.
    pub fn select_to(&mut self, byte_offset: usize) {
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor());
        }
        self.move_gap_to(byte_offset);
        if self.selection_anchor == Some(self.cursor()) {
            self.selection_anchor = None;
        }
    }

    /// Returns the sorted `(start, end)` byte selection range if non-empty.
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.selection_anchor?;
        let cur = self.cursor();
        if anchor == cur {
            None
        } else {
            Some((anchor.min(cur), anchor.max(cur)))
        }
    }

    /// Returns the currently selected text slice, if any.
    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection_range()?;
        let full = self.text();
        full.get(start..end).map(String::from)
    }

    /// Selects the entire buffer contents.
    pub fn select_all(&mut self) {
        let len = self.len_bytes();
        if len > 0 {
            self.selection_anchor = Some(0);
            self.move_gap_to(len);
        }
    }

    fn delete_selection_internal(&mut self) -> bool {
        let Some((start, end)) = self.selection_range() else {
            self.selection_anchor = None;
            return false;
        };
        self.move_gap_to(start);
        let del_len = end - start;
        self.gap_end = (self.gap_end + del_len).min(self.buf.len());
        self.selection_anchor = None;
        true
    }

    /// Inserts UTF-8 text at the cursor, replacing any active selection.
    pub fn insert_str(&mut self, s: &str) {
        if s.is_empty() && self.selection_range().is_none() {
            return;
        }
        self.record_history();
        self.delete_selection_internal();
        if !s.is_empty() {
            self.ensure_gap(s.len());
            self.buf[self.gap_start..self.gap_start + s.len()].copy_from_slice(s.as_bytes());
            self.gap_start += s.len();
        }
    }

    /// Deletes the active selection, or the grapheme cluster immediately before the cursor.
    pub fn backspace(&mut self) {
        if self.selection_range().is_some() {
            self.record_history();
            self.delete_selection_internal();
            return;
        }
        if self.gap_start == 0 {
            return;
        }
        self.record_history();
        let prev_start = prev_grapheme_start(self.left_str());
        self.gap_start = prev_start;
    }

    /// Deletes the active selection, or the grapheme cluster immediately after the cursor.
    pub fn delete_forward(&mut self) {
        if self.selection_range().is_some() {
            self.record_history();
            self.delete_selection_internal();
            return;
        }
        if self.gap_end >= self.buf.len() {
            return;
        }
        self.record_history();
        let next_len = next_grapheme_end(self.right_str());
        self.gap_end = (self.gap_end + next_len).min(self.buf.len());
    }

    /// Moves the cursor left by one grapheme cluster, optionally extending selection.
    pub fn move_left(&mut self, extend_selection: bool) {
        if extend_selection && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor());
        } else if !extend_selection {
            if let Some((start, _)) = self.selection_range() {
                self.selection_anchor = None;
                self.move_gap_to(start);
                return;
            }
            self.selection_anchor = None;
        }
        let target = prev_grapheme_start(self.left_str());
        self.move_gap_to(target);
    }

    /// Moves the cursor right by one grapheme cluster, optionally extending selection.
    pub fn move_right(&mut self, extend_selection: bool) {
        if extend_selection && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor());
        } else if !extend_selection {
            if let Some((_, end)) = self.selection_range() {
                self.selection_anchor = None;
                self.move_gap_to(end);
                return;
            }
            self.selection_anchor = None;
        }
        let advance = next_grapheme_end(self.right_str());
        let target = self.gap_start + advance;
        self.move_gap_to(target);
    }

    /// Moves the cursor to the start of the buffer.
    pub fn move_home(&mut self, extend_selection: bool) {
        if extend_selection && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor());
        } else if !extend_selection {
            self.selection_anchor = None;
        }
        self.move_gap_to(0);
    }

    /// Moves the cursor to the end of the buffer.
    pub fn move_end(&mut self, extend_selection: bool) {
        if extend_selection && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor());
        } else if !extend_selection {
            self.selection_anchor = None;
        }
        let len = self.len_bytes();
        self.move_gap_to(len);
    }

    /// Moves the cursor left by one word boundary, optionally extending selection.
    pub fn move_word_left(&mut self, extend_selection: bool) {
        if extend_selection && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor());
        } else if !extend_selection {
            self.selection_anchor = None;
        }
        let target = prev_word_start(self.left_str());
        self.move_gap_to(target);
    }

    /// Moves the cursor right by one word boundary, optionally extending selection.
    pub fn move_word_right(&mut self, extend_selection: bool) {
        if extend_selection && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor());
        } else if !extend_selection {
            self.selection_anchor = None;
        }
        let adv = next_word_end(self.right_str());
        let target = self.gap_start + adv;
        self.move_gap_to(target);
    }

    /// Deletes the active selection or the word immediately before the cursor.
    pub fn backspace_word(&mut self) {
        if self.selection_range().is_some() {
            self.backspace();
            return;
        }
        if self.gap_start == 0 {
            return;
        }
        self.record_history();
        self.gap_start = prev_word_start(self.left_str());
    }

    /// Deletes the active selection or the word immediately after the cursor.
    pub fn delete_word_forward(&mut self) {
        if self.selection_range().is_some() {
            self.delete_forward();
            return;
        }
        if self.gap_end >= self.buf.len() {
            return;
        }
        self.record_history();
        let adv = next_word_end(self.right_str());
        self.gap_end = (self.gap_end + adv).min(self.buf.len());
    }

    /// Moves the cursor up one `\n`-delimited line while preserving column offset.
    pub fn move_up(&mut self, extend_selection: bool) {
        if extend_selection && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor());
        } else if !extend_selection {
            self.selection_anchor = None;
        }
        let left = self.left_str();
        let cur_line_start = left.rfind('\n').map_or(0, |i| i + 1);
        if cur_line_start == 0 {
            self.move_gap_to(0);
            return;
        }
        let col_chars = left[cur_line_start..].chars().count();
        let prev_line_end = cur_line_start - 1;
        let prev_line_start = left[..prev_line_end].rfind('\n').map_or(0, |i| i + 1);
        let prev_line = &left[prev_line_start..prev_line_end];
        let rel_byte = prev_line
            .char_indices()
            .nth(col_chars)
            .map_or(prev_line.len(), |(idx, _)| idx);
        self.move_gap_to(prev_line_start + rel_byte);
    }

    /// Moves the cursor down one `\n`-delimited line while preserving column offset.
    pub fn move_down(&mut self, extend_selection: bool) {
        if extend_selection && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor());
        } else if !extend_selection {
            self.selection_anchor = None;
        }
        let left = self.left_str();
        let cur_line_start = left.rfind('\n').map_or(0, |i| i + 1);
        let col_chars = left[cur_line_start..].chars().count();

        let right = self.right_str();
        let Some(nl_rel) = right.find('\n') else {
            let len = self.len_bytes();
            self.move_gap_to(len);
            return;
        };
        let next_start_rel = nl_rel + 1;
        let next_end_rel = right[next_start_rel..]
            .find('\n')
            .map_or(right.len(), |i| next_start_rel + i);
        let next_line = &right[next_start_rel..next_end_rel];
        let rel_byte = next_line
            .char_indices()
            .nth(col_chars)
            .map_or(next_line.len(), |(idx, _)| idx);
        let target = self.gap_start + next_start_rel + rel_byte;
        self.move_gap_to(target);
    }

    /// Undoes the last edit. Returns `true` if state changed.
    pub fn undo(&mut self) -> bool {
        let cur = (self.text(), self.cursor());
        let Some((prev_text, prev_cur)) = self.history.undo(cur) else {
            return false;
        };
        self.apply_snapshot(&prev_text, prev_cur);
        true
    }

    /// Re-applies the last undone edit. Returns `true` if state changed.
    pub fn redo(&mut self) -> bool {
        let cur = (self.text(), self.cursor());
        let Some((next_text, next_cur)) = self.history.redo(cur) else {
            return false;
        };
        self.apply_snapshot(&next_text, next_cur);
        true
    }

    fn apply_snapshot(&mut self, s: &str, cursor: usize) {
        self.buf.clear();
        let cap = s.len().checked_add(64).unwrap_or(s.len());
        self.buf.resize(cap, 0);
        self.buf[..s.len()].copy_from_slice(s.as_bytes());
        self.gap_start = s.len();
        self.gap_end = cap;
        self.selection_anchor = None;
        self.ime_preedit = None;
        self.move_gap_to(cursor);
    }

    /// Returns selected text without mutating the buffer.
    pub fn copy_selection(&self) -> Option<String> {
        self.selected_text()
    }

    /// Cuts and returns selected text.
    pub fn cut_selection(&mut self) -> Option<String> {
        let selected = self.selected_text()?;
        self.record_history();
        self.delete_selection_internal();
        Some(selected)
    }

    /// Pastes `text` at the cursor, replacing any active selection.
    pub fn paste(&mut self, text: &str) {
        self.insert_str(text);
    }

    /// Sets active IME composition preedit text.
    pub fn set_ime_preedit(&mut self, preedit: String) {
        if preedit.is_empty() {
            self.ime_preedit = None;
        } else {
            self.ime_preedit = Some(preedit);
        }
    }

    /// Clears active IME preedit text.
    pub fn clear_ime_preedit(&mut self) {
        self.ime_preedit = None;
    }

    /// Commits IME text and clears any pending preedit string.
    pub fn commit_ime(&mut self, committed: &str) {
        self.ime_preedit = None;
        self.insert_str(committed);
    }

    /// Returns the display string including active IME preedit at the cursor.
    pub fn display_text(&self) -> String {
        let Some(ref pre) = self.ime_preedit else {
            return self.text();
        };
        let mut out = String::with_capacity(self.len_bytes() + pre.len());
        out.push_str(self.left_str());
        out.push_str(pre);
        out.push_str(self.right_str());
        out
    }

    /// Returns active IME preedit slice if present.
    pub fn ime_preedit(&self) -> Option<&str> {
        self.ime_preedit.as_deref()
    }
}

impl From<&str> for TextEditState {
    fn from(value: &str) -> Self {
        Self::with_text(value)
    }
}

#[cfg(test)]
mod tests {
    use super::TextEditState;

    #[test]
    fn gap_buffer_grapheme_selection_undo_and_ime() {
        let mut ed = TextEditState::with_text("ab");
        ed.move_left(false);
        ed.insert_str("e\u{0301}");
        assert_eq!(ed.text(), "ae\u{0301}b");

        // Backspace removes the entire grapheme cluster "e\u{0301}".
        ed.backspace();
        assert_eq!(ed.text(), "ab");

        // Undo restores "ae\u{0301}b".
        assert!(ed.undo());
        assert_eq!(ed.text(), "ae\u{0301}b");
        assert!(ed.redo());
        assert_eq!(ed.text(), "ab");

        // ZWJ emoji deletion as a single grapheme.
        ed.set_text("x\u{1F468}\u{200D}\u{1F4BB}y");
        ed.move_left(false);
        ed.backspace();
        assert_eq!(ed.text(), "xy");

        // Selection cut/copy/paste.
        ed.set_text("hello world");
        ed.move_home(false);
        for _ in 0..5 {
            ed.move_right(true);
        }
        assert_eq!(ed.copy_selection().as_deref(), Some("hello"));
        let cut = ed.cut_selection().expect("cut");
        assert_eq!(cut, "hello");
        assert_eq!(ed.text(), " world");
        ed.paste("hi");
        assert_eq!(ed.text(), "hi world");

        // IME preedit & commit.
        ed.set_ime_preedit(String::from("nih"));
        assert!(ed.display_text().contains("nih"));
        ed.commit_ime("你好");
        assert_eq!(ed.ime_preedit(), None);
        assert!(ed.text().contains("你好"));

        // Word navigation/deletion & multiline Up/Down.
        let mut ml = TextEditState::with_text("alpha beta\ngamma delta");
        ml.move_up(false);
        assert_eq!(ml.cursor(), 10);
        ml.move_word_left(false);
        assert_eq!(ml.cursor(), 6);
        ml.move_word_right(true);
        assert_eq!(ml.selected_text().as_deref(), Some("beta"));
        ml.move_down(false);
        assert!(ml.cursor() > 11);
        ml.move_end(false);
        ml.backspace_word();
        assert_eq!(ml.text(), "alpha beta\ngamma ");
    }
}
