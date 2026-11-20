use super::*;

impl UiContext {
    /// Multi-line preformatted monospace code block card with selectable text.
    ///
    /// # Applicable Modifiers
    /// - `.highlight(lang)` (syntax highlighting, e.g. `"rust"`, `"json"`, `"glsl"`, `"toml"`)
    /// - `.line_numbers()` (render gutter line numbers)
    /// - `.copyable()` (add copy-to-clipboard button)
    /// - `.mono()`, `.sz(pt)`, `.selection_bg(color)`, `.bg(color)`, `.bd(w, color)`, `.r(radius)`, `.p(pad)`
    pub fn code_block(&mut self, code: &str) -> StyleBuilder<'_> {
        let bg = self.theme.surface_alt;
        let bd = self.theme.border;
        let r = self.theme.radius;
        let fg = self.theme.text;
        let card = self
            .col(|ui| {
                ui.txt(code).mono().sz(13.0).fg(fg).wrap().selectable();
            })
            .gap(6.0)
            .p(10.0)
            .bg(bg)
            .bd(1.0, bd)
            .r(r);
        let card_id = card.ui.nodes[card.node_idx].id;
        card.ui
            .code_blocks
            .insert(card_id, (String::from(code), None, false));
        card
    }

    /// Syntax-highlighted multi-line monospace code block card for `lang`.
    pub fn code_block_highlighted(&mut self, lang: &str, code: &str) -> StyleBuilder<'_> {
        self.code_block(code).highlight(lang)
    }

    /// Document H1 heading (`28pt` bold, wrapped).
    pub fn h1(&mut self, text: &str) -> StyleBuilder<'_> {
        self.txt(text).sz(28.0).bold().wrap()
    }

    /// Document H2 heading (`22pt` semibold, wrapped).
    pub fn h2(&mut self, text: &str) -> StyleBuilder<'_> {
        self.txt(text).sz(22.0).semibold().wrap()
    }

    /// Document H3 heading (`17.5pt` semibold, wrapped).
    pub fn h3(&mut self, text: &str) -> StyleBuilder<'_> {
        self.txt(text).sz(17.5).semibold().wrap()
    }

    /// Bulleted vertical list supporting inline markdown spans (`**bold**`, `*italic*`, `` `code` ``, `[link](id)`).
    pub fn bullets(&mut self, items: &[&str]) -> StyleBuilder<'_> {
        let muted = self.theme.text_muted;
        self.col(|ui| {
            for item in items {
                ui.row(|ui| {
                    ui.txt("•").fg(muted);
                    let spans = parse_inline_markdown(item);
                    ui.rich_owned(spans).wrap();
                })
                .gap(6.0);
            }
        })
        .gap(4.0)
    }

    /// Lightweight Markdown block renderer (`#`/`##`/`###`, `-`/`*`/`1.` lists, ```` ```lang ```` code blocks, `>` quotes, `---`, and inline spans).
    pub fn markdown(&mut self, src: &str) -> StyleBuilder<'_> {
        let primary = self.theme.primary;
        let muted = self.theme.text_muted;
        let surface_alt = self.theme.surface_alt;
        self.col(|ui| {
            let lines: Vec<&str> = src.lines().collect();
            let mut i = 0usize;
            while i < lines.len() {
                let raw = lines[i];
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    i += 1;
                    continue;
                }
                if let Some(rest) = trimmed.strip_prefix("```") {
                    let lang = rest.trim();
                    let mut code_lines = Vec::new();
                    i += 1;
                    while i < lines.len() && !lines[i].trim_start().starts_with("```") {
                        code_lines.push(lines[i]);
                        i += 1;
                    }
                    if i < lines.len() {
                        i += 1;
                    }
                    let joined = code_lines.join("\n");
                    if lang.is_empty() {
                        ui.code_block(&joined);
                    } else {
                        ui.code_block_highlighted(lang, &joined);
                    }
                    continue;
                }
                if trimmed == "---" || trimmed == "***" || trimmed == "___" {
                    ui.divider();
                    i += 1;
                    continue;
                }
                if let Some(h) = trimmed.strip_prefix("### ") {
                    ui.h3(h.trim());
                    i += 1;
                    continue;
                }
                if let Some(h) = trimmed.strip_prefix("## ") {
                    ui.h2(h.trim());
                    i += 1;
                    continue;
                }
                if let Some(h) = trimmed.strip_prefix("# ") {
                    ui.h1(h.trim());
                    i += 1;
                    continue;
                }
                if let Some(q) = trimmed.strip_prefix("> ") {
                    let spans = parse_inline_markdown(q.trim());
                    ui.col(|ui| {
                        ui.rich_owned(spans).wrap().fg(muted);
                    })
                    .pl(12.0)
                    .py(4.0)
                    .bd_left(3.0, primary)
                    .bg(Color::rgba(
                        surface_alt.r,
                        surface_alt.g,
                        surface_alt.b,
                        90,
                    ));
                    i += 1;
                    continue;
                }
                if let Some(item) = trimmed
                    .strip_prefix("- ")
                    .or_else(|| trimmed.strip_prefix("* "))
                {
                    let spans = parse_inline_markdown(item.trim());
                    ui.row(|ui| {
                        ui.txt("•").fg(muted);
                        ui.rich_owned(spans).wrap();
                    })
                    .gap(6.0);
                    i += 1;
                    continue;
                }
                if let Some((num_prefix, item)) = parse_ordered_list_line(trimmed) {
                    let spans = parse_inline_markdown(item);
                    ui.row(|ui| {
                        ui.txt(num_prefix).fg(muted);
                        ui.rich_owned(spans).wrap();
                    })
                    .gap(6.0);
                    i += 1;
                    continue;
                }

                let mut para_lines = vec![trimmed];
                i += 1;
                while i < lines.len() {
                    let next = lines[i].trim();
                    if next.is_empty()
                        || next.starts_with('#')
                        || next.starts_with("```")
                        || next.starts_with("> ")
                        || next.starts_with("- ")
                        || next.starts_with("* ")
                        || next == "---"
                        || next == "***"
                        || parse_ordered_list_line(next).is_some()
                    {
                        break;
                    }
                    para_lines.push(next);
                    i += 1;
                }
                let para = para_lines.join(" ");
                let spans = parse_inline_markdown(&para);
                ui.rich_owned(spans).wrap();
            }
        })
        .gap(6.0)
    }
}

impl StyleBuilder<'_> {
    /// Applies syntax highlighting (`rust`, `js`/`ts`, `json`, `glsl`/`wgsl`, `python`, `toml`, `sh`) to a `code_block`.
    pub fn highlight(self, lang: &str) -> Self {
        let card_id = self.ui.nodes[self.node_idx].id;
        if let Some(entry) = self.ui.code_blocks.get_mut(&card_id) {
            entry.1 = Some(lang.to_ascii_lowercase());
            rebuild_code_block_leaf(self.ui, self.node_idx, card_id);
        }
        self
    }

    /// Prepends aligned line numbers (` 1 | ...`) to a `code_block` card.
    pub fn line_numbers(self) -> Self {
        let card_id = self.ui.nodes[self.node_idx].id;
        if let Some(entry) = self.ui.code_blocks.get_mut(&card_id) {
            entry.2 = true;
            rebuild_code_block_leaf(self.ui, self.node_idx, card_id);
        }
        self
    }

    /// Adds a header bar with a one-click `Copy` button to a `code_block` card.
    pub fn copyable(self) -> Self {
        let card_id = self.ui.nodes[self.node_idx].id;
        let Some((raw_code, lang, _)) = self.ui.code_blocks.get(&card_id).cloned() else {
            return self;
        };

        self.ui.ids.push_u64(card_id.0);
        let header_id = self.ui.ids.id("__code_header__");
        let lang_id = self.ui.ids.id("__code_lang__");
        let copy_id = self.ui.ids.id("__code_copy__");
        self.ui.ids.pop_id();
        let _ = self.ui.ids.register_interactive(copy_id);

        if self.ui.clicked_id == Some(copy_id) || self.ui.activated_id == Some(copy_id) {
            crate::os::clipboard::set_text(&raw_code);
        }

        let theme = self.ui.theme;
        let is_hovered = self.ui.hovered_id == Some(copy_id);
        let is_pressed = self.ui.pressed_id == Some(copy_id);
        let mut copy_node = LayoutNode::new_leaf(copy_id, Role::Button, String::from("Copy"));
        copy_node.text = Some(String::from("Copy"));
        copy_node.focusable = true;
        copy_node.hit_testable = true;
        copy_node.style = Style {
            pad_l: 8.0,
            pad_r: 8.0,
            pad_t: 2.0,
            pad_b: 2.0,
            font_size: Some(11.0),
            fg: Some(theme.text),
            bg: Some(if is_pressed || is_hovered {
                theme.surface
            } else {
                theme.background
            }),
            border_width: 1.0,
            border_color: theme.border,
            radius: 4.0,
            ..Style::default()
        };
        let copy_idx = self.ui.alloc_node(copy_node, false);

        let lang_label = lang.unwrap_or_else(|| String::from("code"));
        let mut lang_node = LayoutNode::new_leaf(lang_id, Role::Label, lang_label.clone());
        lang_node.text = Some(lang_label);
        lang_node.style = Style {
            font_size: Some(11.0),
            mono: true,
            grow: 1.0,
            fg: Some(theme.text_muted),
            ..Style::default()
        };
        let lang_idx = self.ui.alloc_node(lang_node, false);

        let mut header_node =
            LayoutNode::new_leaf(header_id, Role::Container, String::from("CodeHeader"));
        header_node.kind = AxisKind::Row;
        header_node.children = vec![lang_idx, copy_idx];
        header_node.style = Style {
            gap: 8.0,
            align: Some(Align::Center),
            border_sides: Some([0.0, 0.0, 0.0, 1.0]),
            border_sides_color: theme.border,
            pad_b: 4.0,
            ..Style::default()
        };
        let header_idx = self.ui.alloc_node(header_node, false);
        self.ui.nodes[self.node_idx].children.insert(0, header_idx);
        self
    }
}

fn rebuild_code_block_leaf(ui: &mut UiContext, card_idx: usize, card_id: crate::sys::id::WidgetId) {
    let Some((raw_code, lang, with_line_numbers)) = ui.code_blocks.get(&card_id).cloned() else {
        return;
    };
    let Some(&leaf_idx) = ui.nodes[card_idx]
        .children
        .iter()
        .rev()
        .find(|&&cid| ui.nodes[cid].role == Role::Label)
    else {
        return;
    };
    let theme = ui.theme;
    let base_spans = if let Some(ref l) = lang {
        highlight_code_spans(l, &raw_code, &theme)
    } else {
        vec![OwnedSpan {
            kind: SpanKind::Mono,
            text: raw_code,
            link_id: None,
            color: Some(theme.text),
        }]
    };
    let final_spans = if with_line_numbers {
        prepend_line_numbers(base_spans, theme.text_muted)
    } else {
        base_spans
    };
    ui.nodes[leaf_idx].text = None;
    ui.nodes[leaf_idx].rich_spans = Some(final_spans);
}

fn prepend_line_numbers(spans: Vec<OwnedSpan>, gutter_col: Color) -> Vec<OwnedSpan> {
    let total_lines = spans
        .iter()
        .map(|s| s.text.bytes().filter(|&b| b == b'\n').count())
        .sum::<usize>()
        + 1;
    let digits = total_lines.to_string().len().max(2);
    let mut out = Vec::with_capacity(spans.len() + total_lines);
    let mut line_no = 1usize;
    let push_num = |out: &mut Vec<OwnedSpan>, n: usize| {
        out.push(OwnedSpan {
            kind: SpanKind::Mono,
            text: format!("{n:>digits$} | "),
            link_id: None,
            color: Some(gutter_col),
        });
    };
    push_num(&mut out, line_no);
    for sp in spans {
        let mut start = 0usize;
        for (idx, b) in sp.text.bytes().enumerate() {
            if b == b'\n' {
                out.push(OwnedSpan {
                    kind: sp.kind,
                    text: String::from(&sp.text[start..=idx]),
                    link_id: None,
                    color: sp.color,
                });
                line_no += 1;
                push_num(&mut out, line_no);
                start = idx + 1;
            }
        }
        if start < sp.text.len() {
            out.push(OwnedSpan {
                kind: sp.kind,
                text: String::from(&sp.text[start..]),
                link_id: None,
                color: sp.color,
            });
        }
    }
    out
}

struct SyntaxPalette {
    keyword: Color,
    string: Color,
    comment: Color,
    number: Color,
    ty: Color,
    text: Color,
}

impl SyntaxPalette {
    fn from_theme(t: &Theme) -> Self {
        if t.is_dark {
            Self {
                keyword: Color::from_hex(0xC084FC),
                string: Color::from_hex(0x4ADE80),
                comment: t.text_muted,
                number: Color::from_hex(0xFBBF24),
                ty: Color::from_hex(0x38BDF8),
                text: t.text,
            }
        } else {
            Self {
                keyword: Color::from_hex(0x7C3AED),
                string: Color::from_hex(0x15803D),
                comment: t.text_muted,
                number: Color::from_hex(0xB45309),
                ty: Color::from_hex(0x0369A1),
                text: t.text,
            }
        }
    }
}

fn is_lang_keyword(lang: &str, word: &str) -> bool {
    match lang {
        "rust" | "rs" => matches!(
            word,
            "fn" | "let"
                | "mut"
                | "pub"
                | "struct"
                | "enum"
                | "impl"
                | "trait"
                | "for"
                | "while"
                | "loop"
                | "if"
                | "else"
                | "match"
                | "return"
                | "use"
                | "mod"
                | "crate"
                | "self"
                | "Self"
                | "const"
                | "static"
                | "where"
                | "async"
                | "await"
                | "true"
                | "false"
        ),
        "js" | "javascript" | "ts" | "typescript" => matches!(
            word,
            "function"
                | "const"
                | "let"
                | "var"
                | "return"
                | "if"
                | "else"
                | "for"
                | "while"
                | "import"
                | "export"
                | "from"
                | "async"
                | "await"
                | "class"
                | "interface"
                | "type"
                | "new"
                | "true"
                | "false"
                | "null"
                | "undefined"
        ),
        "py" | "python" => matches!(
            word,
            "def"
                | "class"
                | "return"
                | "if"
                | "elif"
                | "else"
                | "for"
                | "while"
                | "in"
                | "import"
                | "from"
                | "as"
                | "with"
                | "try"
                | "except"
                | "async"
                | "await"
                | "True"
                | "False"
                | "None"
        ),
        "glsl" | "wgsl" => matches!(
            word,
            "void"
                | "vec2"
                | "vec3"
                | "vec4"
                | "mat3"
                | "mat4"
                | "float"
                | "int"
                | "uint"
                | "bool"
                | "uniform"
                | "layout"
                | "in"
                | "out"
                | "if"
                | "else"
                | "for"
                | "return"
                | "fn"
                | "let"
                | "var"
                | "struct"
        ),
        "json" => matches!(word, "true" | "false" | "null"),
        "toml" => matches!(word, "true" | "false"),
        "sh" | "bash" | "shell" | "ps1" | "powershell" => matches!(
            word,
            "if" | "then" | "else" | "fi" | "for" | "do" | "done" | "export" | "echo" | "exit"
        ),
        _ => matches!(
            word,
            "fn" | "let" | "const" | "if" | "else" | "for" | "while" | "return" | "true" | "false"
        ),
    }
}

fn highlight_code_spans(lang: &str, code: &str, theme: &Theme) -> Vec<OwnedSpan> {
    let pal = SyntaxPalette::from_theme(theme);
    let hash_comments = matches!(
        lang,
        "py" | "python" | "toml" | "sh" | "bash" | "shell" | "ps1" | "powershell"
    );
    let slash_comments = !matches!(lang, "json" | "toml" | "py" | "python");
    let bytes = code.as_bytes();
    let mut out: Vec<OwnedSpan> = Vec::new();
    let push_tok = |out: &mut Vec<OwnedSpan>, text: &str, color: Color| {
        if text.is_empty() {
            return;
        }
        if let Some(last) = out.last_mut() {
            if last.color == Some(color) {
                last.text.push_str(text);
                return;
            }
        }
        out.push(OwnedSpan {
            kind: SpanKind::Mono,
            text: String::from(text),
            link_id: None,
            color: Some(color),
        });
    };

    let mut i = 0usize;
    while i < bytes.len() {
        if slash_comments && i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            let start = i;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            push_tok(&mut out, &code[start..i], pal.comment);
            continue;
        }
        if slash_comments && i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            push_tok(&mut out, &code[start..i], pal.comment);
            continue;
        }
        if hash_comments && bytes[i] == b'#' {
            let start = i;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            push_tok(&mut out, &code[start..i], pal.comment);
            continue;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                let b = bytes[i];
                i += 1;
                if b == quote || b == b'\n' {
                    break;
                }
            }
            push_tok(&mut out, &code[start..i], pal.string);
            continue;
        }
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.' || bytes[i] == b'_')
            {
                i += 1;
            }
            push_tok(&mut out, &code[start..i], pal.number);
            continue;
        }
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &code[start..i];
            let col = if is_lang_keyword(lang, word) {
                pal.keyword
            } else if word.chars().next().is_some_and(char::is_uppercase) {
                pal.ty
            } else {
                pal.text
            };
            push_tok(&mut out, word, col);
            continue;
        }
        let ch_len = code[i..].chars().next().map_or(1, char::len_utf8);
        push_tok(&mut out, &code[i..i + ch_len], pal.text);
        i += ch_len;
    }
    out
}

fn parse_ordered_list_line(trimmed: &str) -> Option<(&str, &str)> {
    let dot = trimmed.find(". ")?;
    if dot == 0 || dot > 4 || !trimmed[..dot].bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((&trimmed[..=dot], trimmed[dot + 2..].trim()))
}

fn parse_inline_markdown(input: &str) -> Vec<OwnedSpan> {
    let mut out: Vec<OwnedSpan> = Vec::new();
    let mut plain = String::new();
    let flush_plain = |out: &mut Vec<OwnedSpan>, plain: &mut String| {
        if !plain.is_empty() {
            out.push(OwnedSpan {
                kind: SpanKind::Text,
                text: core::mem::take(plain),
                link_id: None,
                color: None,
            });
        }
    };

    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            if let Some(rel_end) = input[i + 1..].find('`') {
                flush_plain(&mut out, &mut plain);
                out.push(OwnedSpan {
                    kind: SpanKind::Code,
                    text: String::from(&input[i + 1..i + 1 + rel_end]),
                    link_id: None,
                    color: None,
                });
                i += 1 + rel_end + 1;
                continue;
            }
        }
        if bytes[i..].starts_with(b"**") {
            if let Some(rel_end) = input[i + 2..].find("**") {
                flush_plain(&mut out, &mut plain);
                out.push(OwnedSpan {
                    kind: SpanKind::Bold,
                    text: String::from(&input[i + 2..i + 2 + rel_end]),
                    link_id: None,
                    color: None,
                });
                i += 2 + rel_end + 2;
                continue;
            }
        }
        if bytes[i] == b'*' {
            if let Some(rel_end) = input[i + 1..].find('*') {
                if rel_end > 0 {
                    flush_plain(&mut out, &mut plain);
                    out.push(OwnedSpan {
                        kind: SpanKind::Italic,
                        text: String::from(&input[i + 1..i + 1 + rel_end]),
                        link_id: None,
                        color: None,
                    });
                    i += 1 + rel_end + 1;
                    continue;
                }
            }
        }
        if bytes[i] == b'[' {
            if let Some(close_br) = input[i + 1..].find("](") {
                let after = i + 1 + close_br + 2;
                if let Some(close_par) = input[after..].find(')') {
                    flush_plain(&mut out, &mut plain);
                    out.push(OwnedSpan {
                        kind: SpanKind::Link,
                        text: String::from(&input[i + 1..i + 1 + close_br]),
                        link_id: Some(String::from(&input[after..after + close_par])),
                        color: None,
                    });
                    i = after + close_par + 1;
                    continue;
                }
            }
        }
        let ch = input[i..].chars().next().unwrap_or('\0');
        plain.push(ch);
        i += ch.len_utf8();
    }
    flush_plain(&mut out, &mut plain);
    if out.is_empty() {
        out.push(OwnedSpan {
            kind: SpanKind::Text,
            text: String::new(),
            link_id: None,
            color: None,
        });
    }
    out
}
