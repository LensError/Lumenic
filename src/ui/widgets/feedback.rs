use super::*;

/// Severity level for [`ToastItem`] notifications.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToastLevel {
    /// Neutral informational toast.
    #[default]
    Info,
    /// Positive confirmation toast.
    Success,
    /// Caution / non-fatal warning toast.
    Warning,
    /// Error / failure alert toast.
    Error,
}

/// Single notification entry inside [`ToastQueue`].
#[derive(Clone, Debug, PartialEq)]
pub struct ToastItem {
    /// Unique monotonic toast identifier.
    pub id: u64,
    /// Visual severity category.
    pub level: ToastLevel,
    /// Toast body text message.
    pub message: String,
    /// Visible lifetime in seconds before automatic expiry.
    pub duration_s: f64,
    pub(crate) created_at_s: Option<f64>,
}

/// Viewport corner anchor for [`ToastQueue`] overlays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToastCorner {
    /// Bottom-right viewport corner (default).
    #[default]
    BottomRight,
    /// Top-right viewport corner.
    TopRight,
    /// Bottom-left viewport corner.
    BottomLeft,
    /// Top-left viewport corner.
    TopLeft,
}

/// Visual style for the dismiss button rendered on default [`ToastQueue`] cards.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ToastCloseStyle {
    /// Subtle borderless ghost icon button that highlights on hover (default).
    #[default]
    Ghost,
    /// Rounded tinted box button with centered vector close icon.
    Boxed,
    /// Circular button with centered vector close icon.
    Circle,
    /// Custom text label button (for example `"Dismiss"` or `"OK"`).
    Text(String),
    /// Custom SVG path `d` string rendered inside the centered close button.
    SvgPath(String),
    /// Omit the close button (toasts still auto-expire unless `duration_s <= 0.0`).
    Hidden,
}

/// Visual styling overrides for default toast cards rendered by [`UiContext::toasts`].
#[derive(Clone, Debug, PartialEq)]
pub struct ToastStyle {
    /// Card background color (defaults to `theme.surface`).
    pub bg: Option<Color>,
    /// Card border width in logical points (defaults to `1.0`).
    pub border_width: Option<f32>,
    /// Card border color (defaults to `theme.border`).
    pub border_color: Option<Color>,
    /// Card corner radius in logical points (defaults to `theme.radius`).
    pub radius: Option<f32>,
    /// Card inner padding in logical points (defaults to `10.0`).
    pub padding: Option<f32>,
    /// Spacing between accent bar, message, and close button in logical points (defaults to `8.0`).
    pub gap: Option<f32>,
    /// Message text color (defaults to `theme.text`).
    pub fg: Option<Color>,
    /// Message font size in logical points (defaults to `13.0`).
    pub font_size: Option<f32>,
    /// Drop shadow `(blur_radius, color)`. `None` disables shadow.
    pub shadow: Option<(f32, Color)>,
    /// Enables frosted-glass backdrop blur (`.glass()`) on each toast card.
    pub glass: bool,
    /// Whether to show the left-side severity accent bar (defaults to `true`).
    pub show_accent_bar: bool,
    /// Dismiss button style (defaults to [`ToastCloseStyle::Ghost`]).
    pub close_style: ToastCloseStyle,
    /// Optional close button background color override.
    pub close_bg: Option<Color>,
    /// Optional close button hover background color override.
    pub close_hover_bg: Option<Color>,
    /// Optional close button icon/text foreground color override (defaults to `theme.text_muted`).
    pub close_fg: Option<Color>,
    /// Optional close button size in logical points (defaults to `24.0`).
    pub close_size: Option<f32>,
}

impl Default for ToastStyle {
    fn default() -> Self {
        Self {
            bg: None,
            border_width: None,
            border_color: None,
            radius: None,
            padding: None,
            gap: None,
            fg: None,
            font_size: None,
            shadow: Some((12.0, Color::rgba(0, 0, 0, 120))),
            glass: false,
            show_accent_bar: true,
            close_style: ToastCloseStyle::Ghost,
            close_bg: None,
            close_hover_bg: None,
            close_fg: None,
            close_size: None,
        }
    }
}

impl ToastStyle {
    /// Overrides the toast card background color.
    pub fn bg(mut self, color: Color) -> Self {
        self.bg = Some(color);
        self
    }

    /// Overrides the toast card border width and color.
    pub fn bd(mut self, width: f32, color: Color) -> Self {
        self.border_width = Some(width.max(0.0));
        self.border_color = Some(color);
        self
    }

    /// Overrides the toast card corner radius.
    pub fn r(mut self, radius: f32) -> Self {
        self.radius = Some(radius.max(0.0));
        self
    }

    /// Overrides the toast card inner padding.
    pub fn p(mut self, padding: f32) -> Self {
        self.padding = Some(padding.max(0.0));
        self
    }

    /// Overrides the spacing between toast elements.
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = Some(gap.max(0.0));
        self
    }

    /// Overrides the toast message text color.
    pub fn fg(mut self, color: Color) -> Self {
        self.fg = Some(color);
        self
    }

    /// Overrides the toast message font size in logical points.
    pub fn sz(mut self, font_size: f32) -> Self {
        self.font_size = Some(font_size.max(8.0));
        self
    }

    /// Sets or disables (`None`) the toast card drop shadow.
    pub fn shadow(mut self, shadow: Option<(f32, Color)>) -> Self {
        self.shadow = shadow;
        self
    }

    /// Enables or disables frosted-glass backdrop blur on toast cards.
    pub fn glass(mut self, enabled: bool) -> Self {
        self.glass = enabled;
        self
    }

    /// Shows or hides the left-side severity accent bar.
    pub fn show_accent_bar(mut self, show: bool) -> Self {
        self.show_accent_bar = show;
        self
    }

    /// Sets the close button visual style.
    pub fn close_style(mut self, style: ToastCloseStyle) -> Self {
        self.close_style = style;
        self
    }

    /// Overrides the close button background color.
    pub fn close_bg(mut self, color: Color) -> Self {
        self.close_bg = Some(color);
        self
    }

    /// Overrides the close button hover background color.
    pub fn close_hover_bg(mut self, color: Color) -> Self {
        self.close_hover_bg = Some(color);
        self
    }

    /// Overrides the close button icon/text foreground color.
    pub fn close_fg(mut self, color: Color) -> Self {
        self.close_fg = Some(color);
        self
    }

    /// Overrides the close button size in logical points.
    pub fn close_size(mut self, size: f32) -> Self {
        self.close_size = Some(size.max(16.0));
        self
    }
}

/// Auto-dismissing stack of toast notifications rendered by [`UiContext::toasts`] or [`UiContext::toasts_with`].
#[derive(Clone, Debug, Default)]
pub struct ToastQueue {
    items: Vec<ToastItem>,
    next_id: u64,
    /// Anchor corner on the viewport.
    pub corner: ToastCorner,
    /// Optional maximum toast card width in logical points (defaults to `320.0`).
    pub max_width: Option<f32>,
    /// Optional accent color override for [`ToastLevel::Info`].
    pub info_color: Option<Color>,
    /// Optional accent color override for [`ToastLevel::Success`].
    pub success_color: Option<Color>,
    /// Optional accent color override for [`ToastLevel::Warning`].
    pub warning_color: Option<Color>,
    /// Optional accent color override for [`ToastLevel::Error`].
    pub error_color: Option<Color>,
    /// Card and close-button styling overrides used by [`UiContext::toasts`].
    pub style: ToastStyle,
}

impl ToastQueue {
    /// Creates an empty toast notification queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Pushes an informational toast lasting `3.5` seconds.
    pub fn push(&mut self, message: impl Into<String>) -> u64 {
        self.push_with(ToastLevel::Info, message, 3.5)
    }

    /// Pushes an informational toast lasting `3.5` seconds.
    pub fn info(&mut self, message: impl Into<String>) -> u64 {
        self.push_with(ToastLevel::Info, message, 3.5)
    }

    /// Pushes a success toast lasting `3.5` seconds.
    pub fn success(&mut self, message: impl Into<String>) -> u64 {
        self.push_with(ToastLevel::Success, message, 3.5)
    }

    /// Pushes a warning toast lasting `4.5` seconds.
    pub fn warn(&mut self, message: impl Into<String>) -> u64 {
        self.push_with(ToastLevel::Warning, message, 4.5)
    }

    /// Pushes an error toast lasting `5.5` seconds.
    pub fn error(&mut self, message: impl Into<String>) -> u64 {
        self.push_with(ToastLevel::Error, message, 5.5)
    }

    /// Pushes a toast with explicit severity level and auto-dismiss duration in seconds (`<= 0.0` persists until closed).
    pub fn push_with(
        &mut self,
        level: ToastLevel,
        message: impl Into<String>,
        duration_s: f64,
    ) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.items.push(ToastItem {
            id,
            level,
            message: message.into(),
            duration_s,
            created_at_s: None,
        });
        id
    }

    /// Removes a specific toast by ID. Returns `true` if a matching toast was removed.
    pub fn dismiss(&mut self, id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|item| item.id != id);
        self.items.len() != before
    }

    /// Returns a slice of currently active toasts.
    pub fn items(&self) -> &[ToastItem] {
        &self.items
    }

    /// Resolves the severity accent color for `level` using queue overrides or `theme`.
    pub fn accent_color(&self, level: ToastLevel, theme: &Theme) -> Color {
        match level {
            ToastLevel::Info => self.info_color.unwrap_or(theme.primary),
            ToastLevel::Success => self.success_color.unwrap_or(Color::from_hex(0x22C55E)),
            ToastLevel::Warning => self.warning_color.unwrap_or(Color::from_hex(0xF59E0B)),
            ToastLevel::Error => self.error_color.unwrap_or(Color::from_hex(0xEF4444)),
        }
    }

    /// Clears all active toasts.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Returns the number of currently queued toasts.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns `true` when no toasts are active.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl UiContext {
    /// Determinate (`0.0..=1.0`) or indeterminate (`< 0.0` or non-finite) progress bar.
    pub fn progress_bar(&mut self, progress: f32) -> StyleBuilder<'_> {
        let ordinal = self
            .container_stack
            .last()
            .map_or(0, |&p| self.nodes[p].children.len());
        self.ids.push_u64(ordinal as u64);
        let id = self.ids.id("__progress_bar__");
        self.ids.pop_id();

        let indeterminate = !progress.is_finite() || progress < 0.0;
        let primary = self.theme.primary;
        let surface_alt = self.theme.surface_alt;
        let border = self.theme.border;

        let builder = if indeterminate {
            self.has_active_indeterminate = true;
            let phase = ((self.elapsed_time_s() * 0.85) % 1.0) as f32;
            let lead = (phase * 1.35 - 0.35).clamp(0.0, 0.68);
            let fill_w = 0.32_f32;
            let trail = (1.0 - lead - fill_w).max(0.001);
            self.container_with_id(AxisKind::Row, "ProgressBar", id, |ui| {
                if lead > 0.001 {
                    ui.txt("").grow(lead).h(8.0);
                }
                ui.txt("").grow(fill_w).h(8.0).bg(primary).r(4.0);
                ui.txt("").grow(trail).h(8.0);
            })
        } else {
            let norm = progress.clamp(0.0, 1.0);
            self.container_with_id(AxisKind::Row, "ProgressBar", id, |ui| {
                if norm >= 0.999 {
                    ui.txt("").grow(1.0).h(8.0).bg(primary).r(4.0);
                } else if norm <= 0.001 {
                    ui.txt("").grow(1.0).h(8.0);
                } else {
                    ui.txt("").grow(norm).h(8.0).bg(primary).r(4.0);
                    ui.txt("").grow((1.0 - norm).max(0.001)).h(8.0);
                }
            })
        };

        let sem_val = if indeterminate {
            String::from("indeterminate")
        } else {
            format!("{:.0}%", progress.clamp(0.0, 1.0) * 100.0)
        };
        builder.ui.nodes[builder.node_idx].semantic_value = Some(sem_val);
        builder
            .w(180.0)
            .h(10.0)
            .p(1.0)
            .bg(surface_alt)
            .bd(1.0, border)
            .r(5.0)
            .clip()
    }

    /// Indeterminate animated progress bar convenience helper.
    pub fn progress_bar_indeterminate(&mut self) -> StyleBuilder<'_> {
        self.progress_bar(-1.0)
    }

    /// Animated vector loading spinner (`20x20` default).
    pub fn spinner(&mut self) -> StyleBuilder<'_> {
        self.has_active_indeterminate = true;
        let ordinal = self
            .container_stack
            .last()
            .map_or(0, |&p| self.nodes[p].children.len());
        self.ids.push_u64(ordinal as u64);
        let id = self.ids.id("__spinner__");
        self.ids.pop_id();

        let size = 20.0_f32;
        let center = [size * 0.5, size * 0.5];
        let r = 7.5_f32;
        let base_angle = (self.elapsed_time_s() * 5.2) as f32;
        let col = self.theme.primary;
        let mut c = Canvas2D::new([0.0, 0.0]);
        let segs = 10usize;
        let span = core::f32::consts::TAU * 0.72;
        for i in 0..segs {
            let t0 = base_angle + (i as f32 / segs as f32) * span;
            let t1 = base_angle + ((i + 1) as f32 / segs as f32) * span;
            let alpha = ((i + 1) as f32 / segs as f32 * f32::from(col.a)).round() as u8;
            let seg_col = Color::rgba(col.r, col.g, col.b, alpha.max(40));
            c.line(
                [center[0] + r * t0.cos(), center[1] + r * t0.sin()],
                [center[0] + r * t1.cos(), center[1] + r * t1.sin()],
                2.2,
                seg_col,
            );
        }

        let idx = self.alloc_node(
            LayoutNode {
                id,
                role: Role::Container,
                kind: AxisKind::Leaf,
                style: Style {
                    width: Some(size),
                    height: Some(size),
                    ..Style::default()
                },
                label: String::from("Spinner"),
                semantic_value: None,
                text: None,
                rich_spans: None,
                rich_links: Vec::new(),
                sparkline_samples: None,
                image_tex: None,
                viewport_data: Some((
                    c.cmds,
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
                intrinsic_w: size,
                intrinsic_h: size,
                content_w: size,
                content_h: size,
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

    /// Compact pill badge (or `10x10` status dot when `text` is empty).
    pub fn badge(&mut self, text: &str) -> StyleBuilder<'_> {
        let primary = self.theme.primary;
        let on_primary = self.theme.on_primary;
        if text.is_empty() {
            self.txt("").w(10.0).h(10.0).bg(primary).circle()
        } else {
            self.txt(text)
                .sz(11.0)
                .semibold()
                .px(8.0)
                .py(2.0)
                .bg(primary)
                .fg(on_primary)
                .r(999.0)
        }
    }

    /// Renders and auto-expires an overlay stack of toast notifications using a custom item renderer.
    ///
    /// `render_item` receives `(ui, item, accent_color)` inside a scoped per-toast ID frame and
    /// returns `true` when the toast should be dismissed.
    pub fn toasts_with(
        &mut self,
        queue: &mut ToastQueue,
        mut render_item: impl FnMut(&mut UiContext, &ToastItem, Color) -> bool,
    ) {
        let now = self.elapsed_time_s();
        for item in &mut queue.items {
            if item.created_at_s.is_none() {
                item.created_at_s = Some(now);
            }
        }
        queue.items.retain(|item| {
            item.duration_s <= 0.0
                || item
                    .created_at_s
                    .is_none_or(|t0| now - t0 < item.duration_s)
        });
        if queue.items.is_empty() {
            return;
        }
        if queue.items.iter().any(|item| item.duration_s > 0.0) {
            self.has_active_toasts = true;
        }

        let theme = self.theme;
        let vw = self.viewport_w;
        let vh = self.viewport_h;
        let max_w = queue.max_width.unwrap_or(320.0).max(180.0);
        let toast_w = (vw - 32.0).clamp(180.0, max_w);
        let est_h = (queue.items.len() as f32) * 52.0 + 16.0;
        let (pos_x, pos_y) = match queue.corner {
            ToastCorner::BottomRight => (
                (vw - toast_w - 16.0).max(8.0),
                (vh - est_h - 12.0).max(44.0),
            ),
            ToastCorner::TopRight => ((vw - toast_w - 16.0).max(8.0), 44.0),
            ToastCorner::BottomLeft => (16.0, (vh - est_h - 12.0).max(44.0)),
            ToastCorner::TopLeft => (16.0, 44.0),
        };

        let mut dismiss_ids: Vec<u64> = Vec::new();
        self.ids.push_id("__toast_stack__");
        self.overlay(|ui| {
            for item in &queue.items {
                ui.ids.push_u64(item.id);
                let accent = queue.accent_color(item.level, &theme);
                if render_item(ui, item, accent) {
                    dismiss_ids.push(item.id);
                }
                ui.ids.pop_id();
            }
        })
        .pos(pos_x, pos_y)
        .clamp_viewport()
        .w(toast_w)
        .gap(8.0);
        self.ids.pop_id();

        if !dismiss_ids.is_empty() {
            queue.items.retain(|i| !dismiss_ids.contains(&i.id));
        }
    }

    /// Renders and auto-expires an overlay stack of toast notifications styled by `queue.style`.
    pub fn toasts(&mut self, queue: &mut ToastQueue) {
        const CLOSE_ICON_PATH: &str = "M6 6 L18 18 M18 6 L6 18";
        let style = queue.style.clone();
        let theme = self.theme;
        let card_bg = style.bg.unwrap_or(theme.surface);
        let card_bd_w = style.border_width.unwrap_or(1.0);
        let card_bd_col = style.border_color.unwrap_or(theme.border);
        let card_r = style.radius.unwrap_or(theme.radius);
        let card_p = style.padding.unwrap_or(10.0);
        let card_gap = style.gap.unwrap_or(8.0);
        let msg_fg = style.fg.unwrap_or(theme.text);
        let msg_sz = style.font_size.unwrap_or(13.0);
        let close_fg = style.close_fg.unwrap_or(theme.text_muted);
        let btn_sz = style.close_size.unwrap_or(24.0).max(16.0);
        let icon_sz = (btn_sz * (2.0 / 3.0)).round().clamp(10.0, btn_sz - 4.0);

        self.toasts_with(queue, |ui, item, accent| {
            let mut dismissed = false;
            let mut card = ui
                .row(|ui| {
                    if style.show_accent_bar {
                        ui.txt("").w(4.0).h(20.0).bg(accent).r(2.0);
                    }
                    ui.txt(&item.message).sz(msg_sz).fg(msg_fg).wrap().grow(1.0);
                    match &style.close_style {
                        ToastCloseStyle::Hidden => {}
                        ToastCloseStyle::Text(label) => {
                            let bg = style.close_bg.unwrap_or(Color::TRANSPARENT);
                            let hover_bg = style.close_hover_bg.unwrap_or(theme.surface_alt);
                            let mut btn = ui
                                .btn(label)
                                .sz(12.0)
                                .px(8.0)
                                .py(4.0)
                                .bg(bg)
                                .hover_bg(hover_bg)
                                .fg(close_fg)
                                .r(6.0);
                            if let Some(h) = style.close_size {
                                btn = btn.h(h.max(16.0));
                            }
                            if btn.clicked() {
                                dismissed = true;
                            }
                        }
                        ToastCloseStyle::Ghost
                        | ToastCloseStyle::Boxed
                        | ToastCloseStyle::Circle
                        | ToastCloseStyle::SvgPath(_) => {
                            let (default_bg, default_hover, btn_r, svg_path) = match &style
                                .close_style
                            {
                                ToastCloseStyle::Ghost => {
                                    (Color::TRANSPARENT, theme.surface_alt, 6.0, CLOSE_ICON_PATH)
                                }
                                ToastCloseStyle::Boxed => {
                                    (theme.surface_alt, theme.border, 6.0, CLOSE_ICON_PATH)
                                }
                                ToastCloseStyle::Circle => {
                                    (theme.surface_alt, theme.border, 999.0, CLOSE_ICON_PATH)
                                }
                                ToastCloseStyle::SvgPath(path) => {
                                    (Color::TRANSPARENT, theme.surface_alt, 6.0, path.as_str())
                                }
                                ToastCloseStyle::Hidden | ToastCloseStyle::Text(_) => {
                                    unreachable!()
                                }
                            };
                            let bg = style.close_bg.unwrap_or(default_bg);
                            let hover_bg = style.close_hover_bg.unwrap_or(default_hover);
                            if ui
                                .btn_with("Dismiss", |ui| {
                                    ui.svg_icon(svg_path, icon_sz).fg(close_fg);
                                })
                                .w(btn_sz)
                                .h(btn_sz)
                                .p(0.0)
                                .gap(0.0)
                                .center()
                                .bg(bg)
                                .hover_bg(hover_bg)
                                .r(btn_r)
                                .clicked()
                            {
                                dismissed = true;
                            }
                        }
                    }
                })
                .align_center()
                .p(card_p)
                .gap(card_gap)
                .bg(card_bg)
                .r(card_r);
            if card_bd_w > 0.0 {
                card = card.bd(card_bd_w, card_bd_col);
            }
            if let Some((blur, col)) = style.shadow {
                if blur > 0.0 && col.a > 0 {
                    card = card.shadow(blur, col);
                }
            }
            if style.glass {
                card = card.glass();
            }
            let _ = card;
            dismissed
        });
    }
}
