#![forbid(unsafe_code)]

use std::io::{self, BufRead, Write};

use crate::gfx::image::encode_png;
use crate::gfx::inspect::{rasterize_sdf_batch_cpu, serialize_layout_json};
use crate::gfx::sdf::SdfBatch;
use crate::gfx::text::GlyphAtlas;
use crate::os::App;
use crate::sys::id::WidgetId;
use crate::sys::sandbox::SafeDir;
use crate::ui::types::{Error, Key, Modifiers, PointerButton, Theme};
use crate::ui::UiContext;

/// Hard maximum JSON-RPC request line length accepted by `--mcp` (`64 KiB`).
pub const MAX_JSON_RPC_LINE_BYTES: usize = 64 * 1024;

/// Reads a single `\n`-delimited line without unbounded buffering, rejecting lines over `max_len`.
pub fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    max_len: usize,
) -> Result<Option<String>, Error> {
    buf.clear();
    loop {
        let avail = reader
            .fill_buf()
            .map_err(|e| Error::Io(format!("MCP stdin read error: {e}")))?;
        if avail.is_empty() {
            if buf.is_empty() {
                return Ok(None);
            }
            let s = String::from_utf8(core::mem::take(buf))
                .map_err(|e| Error::InvalidInput(format!("invalid UTF-8 JSON-RPC line: {e}")))?;
            return Ok(Some(s.trim_end_matches('\r').to_owned()));
        }

        if let Some(nl_pos) = avail.iter().position(|&b| b == b'\n') {
            if buf.len().saturating_add(nl_pos) > max_len {
                reader.consume(nl_pos + 1);
                buf.clear();
                return Err(Error::InvalidInput(format!(
                    "JSON-RPC line exceeds {max_len}-byte limit"
                )));
            }
            buf.extend_from_slice(&avail[..nl_pos]);
            reader.consume(nl_pos + 1);
            let s = String::from_utf8(core::mem::take(buf))
                .map_err(|e| Error::InvalidInput(format!("invalid UTF-8 JSON-RPC line: {e}")))?;
            return Ok(Some(s.trim_end_matches('\r').to_owned()));
        }

        let chunk_len = avail.len();
        if buf.len().saturating_add(chunk_len) > max_len {
            reader.consume(chunk_len);
            buf.clear();
            // Drain remainder of the oversized line until newline or EOF without keeping bytes.
            loop {
                let rem = reader
                    .fill_buf()
                    .map_err(|e| Error::Io(format!("MCP drain read error: {e}")))?;
                if rem.is_empty() {
                    break;
                }
                if let Some(pos) = rem.iter().position(|&b| b == b'\n') {
                    reader.consume(pos + 1);
                    break;
                }
                let rlen = rem.len();
                reader.consume(rlen);
            }
            return Err(Error::InvalidInput(format!(
                "JSON-RPC line exceeds {max_len}-byte limit"
            )));
        }
        buf.extend_from_slice(avail);
        reader.consume(chunk_len);
    }
}

#[derive(Clone, Debug, PartialEq)]
enum JsonVal {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<JsonVal>),
    Obj(Vec<(String, JsonVal)>),
}

impl JsonVal {
    fn get(&self, key: &str) -> Option<&JsonVal> {
        let JsonVal::Obj(entries) = self else {
            return None;
        };
        entries.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            JsonVal::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    fn as_f64(&self) -> Option<f64> {
        match self {
            JsonVal::Num(n) => Some(*n),
            _ => None,
        }
    }

    fn as_u64(&self) -> Option<u64> {
        match self {
            JsonVal::Num(n) if n.is_finite() && *n >= 0.0 => Some(*n as u64),
            _ => None,
        }
    }

    fn to_raw_json(&self) -> String {
        match self {
            JsonVal::Null => String::from("null"),
            JsonVal::Bool(b) => String::from(if *b { "true" } else { "false" }),
            JsonVal::Num(n) => {
                if n.fract() == 0.0 && *n >= i64::MIN as f64 && *n <= i64::MAX as f64 {
                    format!("{}", *n as i64)
                } else {
                    format!("{n}")
                }
            }
            JsonVal::Str(s) => json_quote(s),
            JsonVal::Arr(_) | JsonVal::Obj(_) => String::from("null"),
        }
    }
}

fn json_quote(s: &str) -> String {
    use core::fmt::Write;
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

struct JsonParser<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn parse(input: &'a str) -> Result<JsonVal, Error> {
        let mut p = Self {
            src: input.as_bytes(),
            pos: 0,
        };
        let val = p.parse_value(0)?;
        p.skip_ws();
        if p.pos != p.src.len() {
            return Err(Error::InvalidInput(String::from("trailing bytes in JSON")));
        }
        Ok(val)
    }

    fn skip_ws(&mut self) {
        while let Some(&b) = self.src.get(self.pos) {
            if matches!(b, b' ' | b'\t' | b'\r' | b'\n') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn parse_value(&mut self, depth: usize) -> Result<JsonVal, Error> {
        if depth > 32 {
            return Err(Error::InvalidInput(String::from("JSON nesting exceeded")));
        }
        self.skip_ws();
        let Some(&b) = self.src.get(self.pos) else {
            return Err(Error::InvalidInput(String::from("unexpected end of JSON")));
        };
        match b {
            b'{' => self.parse_object(depth + 1),
            b'[' => self.parse_array(depth + 1),
            b'"' => self.parse_string().map(JsonVal::Str),
            b't' => self.expect_bytes(b"true", JsonVal::Bool(true)),
            b'f' => self.expect_bytes(b"false", JsonVal::Bool(false)),
            b'n' => self.expect_bytes(b"null", JsonVal::Null),
            b'-' | b'0'..=b'9' => self.parse_number(),
            _ => Err(Error::InvalidInput(String::from("invalid JSON token"))),
        }
    }

    fn expect_bytes(&mut self, lit: &[u8], out: JsonVal) -> Result<JsonVal, Error> {
        if self.src.get(self.pos..self.pos + lit.len()) == Some(lit) {
            self.pos += lit.len();
            Ok(out)
        } else {
            Err(Error::InvalidInput(String::from("invalid JSON literal")))
        }
    }

    fn parse_u16_hex(&mut self) -> Result<u16, Error> {
        let slice = self
            .src
            .get(self.pos..self.pos + 4)
            .ok_or_else(|| Error::InvalidInput(String::from("truncated \\uXXXX escape")))?;
        self.pos += 4;
        let hex_str = core::str::from_utf8(slice)
            .map_err(|_| Error::InvalidInput(String::from("invalid \\uXXXX bytes")))?;
        u16::from_str_radix(hex_str, 16)
            .map_err(|_| Error::InvalidInput(String::from("invalid hex in \\uXXXX")))
    }

    fn parse_string(&mut self) -> Result<String, Error> {
        if self.src.get(self.pos) != Some(&b'"') {
            return Err(Error::InvalidInput(String::from("expected '\"'")));
        }
        self.pos += 1;
        let mut out = String::new();
        while self.pos < self.src.len() {
            let b = self.src[self.pos];
            if b == b'"' {
                self.pos += 1;
                return Ok(out);
            }
            if b == b'\\' {
                self.pos += 1;
                let esc = *self
                    .src
                    .get(self.pos)
                    .ok_or_else(|| Error::InvalidInput(String::from("truncated escape")))?;
                self.pos += 1;
                match esc {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000C}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let code = self.parse_u16_hex()?;
                        if (0xD800..=0xDBFF).contains(&code)
                            && self.src.get(self.pos..self.pos + 2) == Some(b"\\u")
                        {
                            self.pos += 2;
                            let low = self.parse_u16_hex()?;
                            let combined = 0x10000
                                + (((u32::from(code) - 0xD800) << 10) | (u32::from(low) - 0xDC00));
                            let ch = char::from_u32(combined).ok_or_else(|| {
                                Error::InvalidInput(String::from("invalid UTF-16 surrogate pair"))
                            })?;
                            out.push(ch);
                        } else {
                            let ch = char::from_u32(u32::from(code)).ok_or_else(|| {
                                Error::InvalidInput(String::from("invalid unicode scalar"))
                            })?;
                            out.push(ch);
                        }
                    }
                    _ => return Err(Error::InvalidInput(String::from("unknown JSON escape"))),
                }
                continue;
            }
            let tail = core::str::from_utf8(&self.src[self.pos..])
                .map_err(|_| Error::InvalidInput(String::from("invalid UTF-8 in JSON string")))?;
            let ch = tail
                .chars()
                .next()
                .ok_or_else(|| Error::InvalidInput(String::from("unexpected EOF in string")))?;
            out.push(ch);
            self.pos += ch.len_utf8();
        }
        Err(Error::InvalidInput(String::from(
            "unterminated JSON string",
        )))
    }

    fn parse_number(&mut self) -> Result<JsonVal, Error> {
        let start = self.pos;
        if self.src.get(self.pos) == Some(&b'-') {
            self.pos += 1;
        }
        while let Some(&b) = self.src.get(self.pos) {
            if matches!(b, b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-') {
                self.pos += 1;
            } else {
                break;
            }
        }
        let s = core::str::from_utf8(&self.src[start..self.pos])
            .map_err(|_| Error::InvalidInput(String::from("invalid number bytes")))?;
        let n = s
            .parse::<f64>()
            .map_err(|_| Error::InvalidInput(String::from("invalid JSON number")))?;
        Ok(JsonVal::Num(n))
    }

    fn parse_array(&mut self, depth: usize) -> Result<JsonVal, Error> {
        self.pos += 1;
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if self.src.get(self.pos) == Some(&b']') {
                self.pos += 1;
                return Ok(JsonVal::Arr(items));
            }
            items.push(self.parse_value(depth)?);
            self.skip_ws();
            match self.src.get(self.pos) {
                Some(&b',') => self.pos += 1,
                Some(&b']') => {
                    self.pos += 1;
                    return Ok(JsonVal::Arr(items));
                }
                _ => return Err(Error::InvalidInput(String::from("expected ',' or ']'"))),
            }
        }
    }

    fn parse_object(&mut self, depth: usize) -> Result<JsonVal, Error> {
        self.pos += 1;
        let mut fields = Vec::new();
        loop {
            self.skip_ws();
            if self.src.get(self.pos) == Some(&b'}') {
                self.pos += 1;
                return Ok(JsonVal::Obj(fields));
            }
            self.skip_ws();
            let key = self.parse_string()?;
            self.skip_ws();
            if self.src.get(self.pos) != Some(&b':') {
                return Err(Error::InvalidInput(String::from("expected ':'")));
            }
            self.pos += 1;
            let val = self.parse_value(depth)?;
            fields.push((key, val));
            self.skip_ws();
            match self.src.get(self.pos) {
                Some(&b',') => self.pos += 1,
                Some(&b'}') => {
                    self.pos += 1;
                    return Ok(JsonVal::Obj(fields));
                }
                _ => return Err(Error::InvalidInput(String::from("expected ',' or '}'"))),
            }
        }
    }
}

fn rpc_ok(id_raw: &str, result_json: &str) -> String {
    format!("{{\"jsonrpc\":\"2.0\",\"id\":{id_raw},\"result\":{result_json}}}")
}

fn rpc_err(id_raw: &str, code: i32, msg: &str) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{id_raw},\"error\":{{\"code\":{code},\"message\":{}}}}}",
        json_quote(msg)
    )
}

fn resolve_target_point_in_ui(ui: &UiContext, args: &JsonVal) -> Option<[f32; 2]> {
    if let (Some(x), Some(y)) = (
        args.get("x").and_then(JsonVal::as_f64),
        args.get("y").and_then(JsonVal::as_f64),
    ) {
        return Some([x as f32, y as f32]);
    }

    if let Some(id_str) = args.get("id").and_then(JsonVal::as_str) {
        let trimmed = id_str.trim_start_matches("0x").trim_start_matches("0X");
        if let Ok(raw) = u64::from_str_radix(trimmed, 16) {
            let target_id = WidgetId(raw);
            if let Some(n) = ui.semantics().iter().find(|n| n.id == target_id) {
                return Some([n.bounds.x + n.bounds.w * 0.5, n.bounds.y + n.bounds.h * 0.5]);
            }
        }
    }

    if let Some(lbl) = args.get("label").and_then(JsonVal::as_str) {
        if let Some(n) = ui.semantics().iter().find(|n| n.label == lbl) {
            return Some([n.bounds.x + n.bounds.w * 0.5, n.bounds.y + n.bounds.h * 0.5]);
        }
    }
    None
}

fn parse_key_name(name: &str) -> Option<Key> {
    match name {
        "Tab" | "tab" => Some(Key::Tab),
        "Enter" | "Return" | "enter" | "return" => Some(Key::Enter),
        "Space" | "space" | " " => Some(Key::Space),
        "Escape" | "Esc" | "escape" | "esc" => Some(Key::Escape),
        "Backspace" | "backspace" => Some(Key::Backspace),
        "Delete" | "Del" | "delete" | "del" => Some(Key::Delete),
        "Insert" | "Ins" | "insert" | "ins" => Some(Key::Insert),
        "Left" | "ArrowLeft" | "left" => Some(Key::Left),
        "Right" | "ArrowRight" | "right" => Some(Key::Right),
        "Up" | "ArrowUp" | "up" => Some(Key::Up),
        "Down" | "ArrowDown" | "down" => Some(Key::Down),
        "Home" | "home" => Some(Key::Home),
        "End" | "end" => Some(Key::End),
        "PageUp" | "PgUp" | "pageup" | "page_up" => Some(Key::PageUp),
        "PageDown" | "PgDn" | "pagedown" | "page_down" => Some(Key::PageDown),
        "F1" | "f1" => Some(Key::F1),
        "F2" | "f2" => Some(Key::F2),
        "F3" | "f3" => Some(Key::F3),
        "F4" | "f4" => Some(Key::F4),
        "F5" | "f5" => Some(Key::F5),
        "F6" | "f6" => Some(Key::F6),
        "F7" | "f7" => Some(Key::F7),
        "F8" | "f8" => Some(Key::F8),
        "F9" | "f9" => Some(Key::F9),
        "F10" | "f10" => Some(Key::F10),
        "F11" | "f11" => Some(Key::F11),
        "F12" | "f12" => Some(Key::F12),
        s if s.chars().count() == 1 => s.chars().next().map(Key::Char),
        _ => None,
    }
}

pub(crate) struct McpTarget<'a> {
    pub ui: &'a mut UiContext,
    pub app: &'a mut App,
    pub atlas: &'a mut GlyphAtlas,
    pub batch: &'a mut SdfBatch,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub last_png: &'a mut Vec<u8>,
}

fn dispatch_mcp_tool<S>(
    name: &str,
    args: &JsonVal,
    target: &mut McpTarget<'_>,
    step_fn: &mut S,
) -> Result<String, (i32, String)>
where
    S: FnMut(&mut UiContext, &mut App, &mut GlyphAtlas, f32),
{
    let ensure_frame = |t: &mut McpTarget<'_>, step_fn: &mut S| {
        if t.app.frame_count() == 0 {
            step_fn(t.ui, t.app, t.atlas, 0.0);
        }
    };

    match name {
        "ui_tree" => {
            ensure_frame(target, step_fn);
            Ok(serialize_layout_json(
                target.width,
                target.height,
                target.scale,
                target.ui.semantics(),
            )
            .trim_end()
            .to_owned())
        }
        "ui_screenshot" => {
            ensure_frame(target, step_fn);
            target.batch.build(
                target.ui.draw_list(),
                target.atlas,
                target.scale,
                target.ui.pointer_pos(),
                target.width,
                target.height,
            );
            target.ui.update_renderer_perf(
                0.0,
                target.batch.instances.len(),
                target.atlas.atlas_size(),
                target.atlas.occupancy_ratio(),
            );
            let rgba = rasterize_sdf_batch_cpu(
                target.width,
                target.height,
                target.ui.theme().background,
                target.batch,
                target.atlas,
            )
            .map_err(|e| (-32603, e.to_string()))?;
            let png = encode_png(target.width, target.height, &rgba)
                .map_err(|e| (-32603, e.to_string()))?;
            *target.last_png = png;
            if let Some(rel_out) = args.get("path").and_then(JsonVal::as_str) {
                let cwd = std::env::current_dir().map_err(|e| (-32603, e.to_string()))?;
                let safe = SafeDir::open(cwd).map_err(|e| (-32602, e.to_string()))?;
                safe.write_atomic(rel_out, target.last_png)
                    .map_err(|e| (-32602, e.to_string()))?;
            }
            Ok(format!(
                "{{\"width\":{},\"height\":{},\"png_bytes\":{}}}",
                target.width,
                target.height,
                target.last_png.len()
            ))
        }
        "ui_click" => {
            ensure_frame(target, step_fn);
            let [cx, cy] = resolve_target_point_in_ui(target.ui, args).ok_or_else(|| {
                (
                    -32602,
                    String::from("ui_click requires valid id, label, or x/y"),
                )
            })?;
            let btn = match args.get("button").and_then(JsonVal::as_str) {
                Some("right" | "Right") => PointerButton::Right,
                Some("middle" | "Middle") => PointerButton::Middle,
                _ => PointerButton::Left,
            };
            let double = matches!(args.get("double"), Some(JsonVal::Bool(true)));
            target.ui.on_pointer_move(cx, cy);
            target.ui.on_pointer_down(btn);
            target.ui.on_pointer_up(btn);
            if double && btn == PointerButton::Left {
                target.ui.on_pointer_down(btn);
                target.ui.on_pointer_up(btn);
            }
            step_fn(target.ui, target.app, target.atlas, 0.016);
            Ok(format!("{{\"clicked\":true,\"x\":{cx:.2},\"y\":{cy:.2}}}"))
        }
        "ui_type" => {
            ensure_frame(target, step_fn);
            if args.get("id").is_some()
                || args.get("label").is_some()
                || (args.get("x").is_some() && args.get("y").is_some())
            {
                if let Some([cx, cy]) = resolve_target_point_in_ui(target.ui, args) {
                    target.ui.on_pointer_move(cx, cy);
                    target.ui.on_pointer_down(PointerButton::Left);
                    target.ui.on_pointer_up(PointerButton::Left);
                    step_fn(target.ui, target.app, target.atlas, 0.016);
                }
            }
            let text = args
                .get("text")
                .and_then(JsonVal::as_str)
                .ok_or_else(|| (-32602, String::from("ui_type requires string field 'text'")))?;
            target.ui.on_ime_commit(String::from(text));
            step_fn(target.ui, target.app, target.atlas, 0.016);
            Ok(format!("{{\"typed_bytes\":{}}}", text.len()))
        }
        "ui_key" => {
            ensure_frame(target, step_fn);
            let key_str = args
                .get("key")
                .and_then(JsonVal::as_str)
                .ok_or_else(|| (-32602, String::from("ui_key requires string field 'key'")))?;
            let key = parse_key_name(key_str)
                .ok_or_else(|| (-32602, format!("unsupported key '{key_str}'")))?;
            let mods = Modifiers {
                shift: matches!(args.get("shift"), Some(JsonVal::Bool(true))),
                ctrl: matches!(args.get("ctrl"), Some(JsonVal::Bool(true))),
                alt: matches!(args.get("alt"), Some(JsonVal::Bool(true))),
                super_key: matches!(
                    args.get("super_key").or_else(|| args.get("logo")),
                    Some(JsonVal::Bool(true))
                ),
            };
            target.ui.on_key_down(key, mods);
            step_fn(target.ui, target.app, target.atlas, 0.016);
            Ok(format!("{{\"key\":{}}}", json_quote(key_str)))
        }
        "ui_scroll" => {
            ensure_frame(target, step_fn);
            if let Some([cx, cy]) = resolve_target_point_in_ui(target.ui, args) {
                target.ui.on_pointer_move(cx, cy);
            }
            let dx = args.get("dx").and_then(JsonVal::as_f64).unwrap_or(0.0) as f32;
            let dy = args.get("dy").and_then(JsonVal::as_f64).unwrap_or(0.0) as f32;
            target.ui.on_scroll(dx, dy);
            step_fn(target.ui, target.app, target.atlas, 0.016);
            Ok(format!(
                "{{\"scrolled\":true,\"dx\":{dx:.2},\"dy\":{dy:.2}}}"
            ))
        }
        "ui_set_theme" => {
            let name = args
                .get("theme")
                .and_then(JsonVal::as_str)
                .unwrap_or("dark");
            let mut theme = match name {
                "light" | "Light" => Theme::light(),
                _ => Theme::dark(),
            };
            let parse_hex_color = |key: &str| -> Option<crate::ui::types::Color> {
                let raw = args.get(key).and_then(JsonVal::as_str)?;
                let trimmed = raw
                    .trim_start_matches('#')
                    .trim_start_matches("0x")
                    .trim_start_matches("0X");
                u32::from_str_radix(trimmed, 16)
                    .ok()
                    .map(crate::ui::types::Color::from_hex)
            };
            if let Some(c) = parse_hex_color("background") {
                theme.background = c;
            }
            if let Some(c) = parse_hex_color("surface") {
                theme.surface = c;
            }
            if let Some(c) = parse_hex_color("primary") {
                theme.primary = c;
            }
            if let Some(r) = args.get("radius").and_then(JsonVal::as_f64) {
                theme.radius = (r as f32).max(0.0);
            }
            target.ui.set_theme(theme);
            step_fn(target.ui, target.app, target.atlas, 0.016);
            Ok(format!(
                "{{\"theme\":{},\"radius\":{:.1},\"is_dark\":{}}}",
                json_quote(name),
                theme.radius,
                theme.is_dark
            ))
        }
        "ui_step_frames" => {
            let count = args
                .get("count")
                .and_then(JsonVal::as_u64)
                .unwrap_or(1)
                .clamp(1, 1000) as usize;
            let dt = args
                .get("dt")
                .and_then(JsonVal::as_f64)
                .unwrap_or(0.016)
                .clamp(0.0001, 0.25) as f32;
            for _ in 0..count {
                step_fn(target.ui, target.app, target.atlas, dt);
            }
            Ok(format!(
                "{{\"frame_count\":{},\"has_active_animations\":{}}}",
                target.app.frame_count(),
                target.ui.has_active_animations()
            ))
        }
        "ui_perf" => {
            ensure_frame(target, step_fn);
            let s = target.ui.perf_stats();
            let gpu_ms_json = s
                .gpu_time_ms
                .map_or_else(|| String::from("null"), |v| format!("{v:.3}"));
            Ok(format!(
                "{{\"frame_time_ms\":{:.3},\"layout_time_ms\":{:.3},\"render_time_ms\":{:.3},\"gpu_time_ms\":{},\"gpu_buffer_bytes\":{},\"draw_calls\":{},\"sdf_instances\":{},\"node_count\":{},\"arena_used_bytes\":{},\"arena_capacity_bytes\":{},\"arena_high_water_bytes\":{},\"glyph_atlas_size\":[{},{}],\"glyph_atlas_occupancy\":{:.4}}}",
                s.frame_time_ms,
                s.layout_time_ms,
                s.render_time_ms,
                gpu_ms_json,
                s.gpu_buffer_bytes,
                s.draw_calls,
                s.sdf_instances,
                s.node_count,
                s.arena_used_bytes,
                s.arena_capacity_bytes,
                s.arena_high_water_bytes,
                s.glyph_atlas_size[0],
                s.glyph_atlas_size[1],
                s.glyph_atlas_occupancy
            ))
        }
        "ui_close" => {
            target.app.request_exit();
            Ok(String::from("{\"closing\":true}"))
        }
        _ => Err((-32601, format!("unknown MCP method '{name}'"))),
    }
}

pub(crate) fn execute_mcp_line<S>(line: &str, target: &mut McpTarget<'_>, step_fn: &mut S) -> String
where
    S: FnMut(&mut UiContext, &mut App, &mut GlyphAtlas, f32),
{
    let doc = match JsonParser::parse(line) {
        Ok(v) => v,
        Err(e) => return rpc_err("null", -32700, &e.to_string()),
    };
    let id_raw = doc
        .get("id")
        .map_or_else(|| String::from("null"), JsonVal::to_raw_json);
    let Some(method) = doc.get("method").and_then(JsonVal::as_str) else {
        return rpc_err(&id_raw, -32600, "missing JSON-RPC method");
    };
    let empty = JsonVal::Obj(Vec::new());
    let params = doc.get("params").unwrap_or(&empty);

    match method {
        "initialize" => rpc_ok(
            &id_raw,
            "{\"protocolVersion\":\"2024-11-05\",\"serverInfo\":{\"name\":\"lumenic\",\"version\":\"0.1.0\"},\"capabilities\":{\"tools\":{}}}",
        ),
        "tools/list" => rpc_ok(
            &id_raw,
            "{\"tools\":[{\"name\":\"ui_tree\"},{\"name\":\"ui_screenshot\"},{\"name\":\"ui_click\"},{\"name\":\"ui_type\"},{\"name\":\"ui_key\"},{\"name\":\"ui_scroll\"},{\"name\":\"ui_set_theme\"},{\"name\":\"ui_step_frames\"},{\"name\":\"ui_perf\"},{\"name\":\"ui_close\"}]}",
        ),
        "tools/call" => {
            let Some(tool_name) = params.get("name").and_then(JsonVal::as_str) else {
                return rpc_err(&id_raw, -32602, "tools/call requires 'name'");
            };
            let args = params.get("arguments").unwrap_or(&empty);
            match dispatch_mcp_tool(tool_name, args, target, step_fn) {
                Ok(payload) => rpc_ok(&id_raw, &payload),
                Err((code, msg)) => rpc_err(&id_raw, code, &msg),
            }
        }
        direct => match dispatch_mcp_tool(direct, params, target, step_fn) {
            Ok(payload) => rpc_ok(&id_raw, &payload),
            Err((code, msg)) => rpc_err(&id_raw, code, &msg),
        },
    }
}

/// Stateful JSON-RPC 2.0 `--mcp` UI driver operating offscreen without needing a display window.
pub struct McpServer {
    /// Offscreen immediate-mode UI context driven by MCP commands.
    pub ui: UiContext,
    /// Offscreen application loop handle exposed during MCP frame steps.
    pub app: App,
    atlas: GlyphAtlas,
    batch: SdfBatch,
    width: u32,
    height: u32,
    scale: f32,
    /// Latest PNG-encoded offscreen raster snapshot.
    pub last_png: Vec<u8>,
}

impl McpServer {
    /// Creates an offscreen MCP runtime sized at `(width, height)` physical pixels.
    pub fn new(width: u32, height: u32, scale: f32) -> Self {
        Self {
            ui: UiContext::new(),
            app: App::default(),
            atlas: GlyphAtlas::new_fallback_only(),
            batch: SdfBatch::default(),
            width: width.max(1),
            height: height.max(1),
            scale: if scale.is_finite() && scale > 0.0 {
                scale
            } else {
                1.0
            },
            last_png: Vec::new(),
        }
    }

    /// Parses and executes a single bounded JSON-RPC 2.0 request line.
    pub fn handle_line<F>(&mut self, line: &str, update: &mut F) -> String
    where
        F: FnMut(&mut UiContext, &mut App),
    {
        let width = self.width;
        let height = self.height;
        let scale = self.scale;
        let mut target = McpTarget {
            ui: &mut self.ui,
            app: &mut self.app,
            atlas: &mut self.atlas,
            batch: &mut self.batch,
            width,
            height,
            scale,
            last_png: &mut self.last_png,
        };
        let mut step_fn = |ui: &mut UiContext, app: &mut App, atlas: &mut GlyphAtlas, dt: f32| {
            let logical_w = width as f32 / scale;
            let logical_h = height as f32 / scale;
            app.begin_step(dt);
            ui.set_texture_sizes(app.texture_sizes());
            ui.step_physics(dt);
            ui.begin_frame(logical_w, logical_h, scale);
            update(ui, app);
            ui.end_frame_with(atlas);
        };
        execute_mcp_line(line, &mut target, &mut step_fn)
    }
}

/// Runs the blocking stdio JSON-RPC 2.0 `--mcp` server loop until EOF.
///
/// If `--headed`, `--mcp-live`, or `--mcp-port` is present in `std::env::args()` on native
/// targets, this automatically opens a visible window with live MCP control instead of running offscreen.
pub fn run_mcp_stdio<F>(mut update: F) -> Result<(), Error>
where
    F: FnMut(&mut UiContext, &mut App),
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        let args: Vec<String> = std::env::args().collect();
        let wants_headed = args
            .iter()
            .any(|a| a == "--headed" || a == "--mcp-live" || a.starts_with("--mcp-port"));
        if wants_headed {
            return crate::os::run_borrowed(
                crate::ui::types::WindowConfig::default(),
                crate::ui::types::LoopMode::Idle,
                update,
            );
        }
    }

    run_mcp_stdio_sized(1024, 700, &mut update)
}

pub(crate) fn run_mcp_stdio_sized<F>(width: u32, height: u32, update: &mut F) -> Result<(), Error>
where
    F: FnMut(&mut UiContext, &mut App),
{
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = io::BufReader::new(stdin.lock());
    let mut writer = stdout.lock();
    let mut buf = Vec::with_capacity(4096);
    let mut server = McpServer::new(width, height, 1.0);

    loop {
        match read_bounded_line(&mut reader, &mut buf, MAX_JSON_RPC_LINE_BYTES) {
            Ok(None) => break,
            Ok(Some(line)) => {
                if line.trim().is_empty() {
                    continue;
                }
                let resp = server.handle_line(&line, update);
                writeln!(writer, "{resp}")
                    .and_then(|_| writer.flush())
                    .map_err(|e| Error::Io(format!("MCP stdout write failed: {e}")))?;
                if server.app.is_exit_requested() {
                    break;
                }
            }
            Err(e) => {
                let resp = rpc_err("null", -32700, &e.to_string());
                writeln!(writer, "{resp}")
                    .and_then(|_| writer.flush())
                    .map_err(|ioe| Error::Io(format!("MCP stdout write failed: {ioe}")))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{self, BufReader, Read};

    use super::{read_bounded_line, JsonParser, McpServer, MAX_JSON_RPC_LINE_BYTES};
    use crate::ui::text_edit::TextEditState;

    struct SlowChunks<'a> {
        bytes: &'a [u8],
        pos: usize,
    }

    impl<'a> Read for SlowChunks<'a> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.pos >= self.bytes.len() {
                return Ok(0);
            }
            let take = (self.bytes.len() - self.pos).min(5).min(buf.len());
            buf[..take].copy_from_slice(&self.bytes[self.pos..self.pos + take]);
            self.pos += take;
            Ok(take)
        }
    }

    #[test]
    fn json_rpc_parser_escaped_strings_partial_lines_and_max_line_cap() {
        let input = b"{\"text\":\"a\\\"b\\\\c\\n\\u0041\"}\n{\"id\":2}\n";
        let mut rdr = BufReader::new(SlowChunks {
            bytes: input,
            pos: 0,
        });
        let mut buf = Vec::new();
        let line1 = read_bounded_line(&mut rdr, &mut buf, 1024)
            .expect("line1 ok")
            .expect("some line1");
        let parsed = JsonParser::parse(&line1).expect("parse escaped string");
        assert_eq!(
            parsed.get("text").and_then(|v| v.as_str()),
            Some("a\"b\\c\nA")
        );

        let line2 = read_bounded_line(&mut rdr, &mut buf, 1024)
            .expect("line2 ok")
            .expect("some line2");
        assert_eq!(
            JsonParser::parse(&line2)
                .expect("parse id")
                .get("id")
                .and_then(|v| v.as_u64()),
            Some(2)
        );
        assert_eq!(
            read_bounded_line(&mut rdr, &mut buf, 1024).expect("eof"),
            None
        );

        let oversized = format!(
            "{}\n{}",
            "a".repeat(MAX_JSON_RPC_LINE_BYTES + 32),
            "{\"ok\":true}\n"
        );
        let mut big_rdr = BufReader::new(io::Cursor::new(oversized.into_bytes()));
        assert!(read_bounded_line(&mut big_rdr, &mut buf, MAX_JSON_RPC_LINE_BYTES).is_err());
        let after = read_bounded_line(&mut big_rdr, &mut buf, MAX_JSON_RPC_LINE_BYTES)
            .expect("recovers next line")
            .expect("some next line");
        assert_eq!(after, "{\"ok\":true}");
    }

    #[test]
    fn mcp_all_seven_tools_smoke_test() {
        let mut server = McpServer::new(640, 480, 1.0);
        let mut clicks = 0u32;
        let mut edit = TextEditState::new();
        let mut demo = |ui: &mut crate::ui::UiContext, _app: &mut crate::os::App| {
            ui.col(|ui| {
                if ui.btn("Go").clicked() {
                    clicks += 1;
                }
                ui.input("Name", &mut edit);
            })
            .p(16.0)
            .gap(10.0);
        };

        let requests = [
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ui_tree\"}",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ui_screenshot\"}",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"ui_click\",\"params\":{\"label\":\"Go\"}}",
            "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"ui_type\",\"params\":{\"label\":\"Name\",\"text\":\"Ada\"}}",
            "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"ui_set_theme\",\"params\":{\"theme\":\"light\"}}",
            "{\"jsonrpc\":\"2.0\",\"id\":11,\"method\":\"ui_set_theme\",\"params\":{\"theme\":\"dark\",\"primary\":\"#2563EB\",\"radius\":8.0}}",
            "{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"ui_step_frames\",\"params\":{\"count\":3,\"dt\":0.016}}",
            "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ui_perf\"}",
        ];

        for req in requests {
            let resp = server.handle_line(req, &mut demo);
            assert!(resp.contains("\"result\":"), "failed response: {resp}");
            assert!(!resp.contains("\"error\":"), "unexpected error: {resp}");
        }

        let key_resp = server.handle_line(
            "{\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"ui_key\",\"params\":{\"key\":\"F12\"}}",
            &mut demo,
        );
        assert!(key_resp.contains("\"key\":\"F12\""), "key_resp: {key_resp}");
        assert!(server.ui.is_hud_visible());

        let scroll_resp = server.handle_line(
            "{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"ui_scroll\",\"params\":{\"dx\":0.0,\"dy\":-40.0}}",
            &mut demo,
        );
        assert!(
            scroll_resp.contains("\"scrolled\":true"),
            "scroll_resp: {scroll_resp}"
        );

        let close_resp = server.handle_line(
            "{\"jsonrpc\":\"2.0\",\"id\":10,\"method\":\"ui_close\"}",
            &mut demo,
        );
        assert!(
            close_resp.contains("\"closing\":true"),
            "close_resp: {close_resp}"
        );
        assert!(server.app.is_exit_requested());

        assert_eq!(clicks, 1);
        assert_eq!(edit.text(), "Ada");
        assert!(!server.last_png.is_empty());
    }
}
