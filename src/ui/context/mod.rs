mod input_router;
mod layout;
mod style;

pub use style::{ButtonResponse, RichResponse, StyleBuilder};

use std::collections::{HashMap, HashSet};

use crate::gfx::viewport::{Canvas2D, Canvas2DCmd, Scene3D, SceneSnapshot, TextureFilter};
use crate::sys::arena::FrameArena;
use crate::sys::id::{IdStack, WidgetId};
use crate::ui::draw::{DrawCmd, DrawList, OwnedSpan, Role, SemanticNode, SpanKind};
use crate::ui::physics::{FlipState, RippleEffect, SpringState, TweenState};
use crate::ui::types::{
    Align, Camera3D, Color, CsdAction, CsdStyle, CursorIcon, GestureResponse, Gradient, ImageFit,
    Justify, Key, Modifiers, PerfStats, PointerButton, Rect, RectShape, Span, TextAlign, TextureId,
    Theme,
};

const DEFAULT_ARENA_BYTES: usize = 256 * 1024;
const DOUBLE_CLICK_WINDOW_S: f64 = 0.4;
const TOOLTIP_DELAY_S: f32 = 0.4;

/// Internal text measurement hook implemented by `gfx::text::GlyphAtlas`.
pub(crate) trait TextMeasure {
    fn measure_plain(
        &mut self,
        text: &str,
        font_size: f32,
        max_width: Option<f32>,
        wrap: bool,
        ellipsis: bool,
    ) -> [f32; 2];

    fn measure_plain_weighted(
        &mut self,
        text: &str,
        font_size: f32,
        weight: u16,
        max_width: Option<f32>,
        wrap: bool,
        ellipsis: bool,
    ) -> [f32; 2] {
        let _ = weight;
        self.measure_plain(text, font_size, max_width, wrap, ellipsis)
    }

    #[allow(clippy::too_many_arguments)]
    fn measure_plain_styled(
        &mut self,
        text: &str,
        font_size: f32,
        weight: u16,
        mono: bool,
        max_width: Option<f32>,
        wrap: bool,
        ellipsis: bool,
    ) -> [f32; 2] {
        let _ = mono;
        self.measure_plain_weighted(text, font_size, weight, max_width, wrap, ellipsis)
    }

    #[allow(clippy::too_many_arguments)]
    fn measure_plain_formatted(
        &mut self,
        text: &str,
        font_size: f32,
        weight: u16,
        mono: bool,
        font_family: Option<&str>,
        tracking: f32,
        line_height: f32,
        max_width: Option<f32>,
        wrap: bool,
        ellipsis: bool,
    ) -> [f32; 2] {
        let _ = (font_family, tracking, line_height);
        self.measure_plain_styled(text, font_size, weight, mono, max_width, wrap, ellipsis)
    }

    fn measure_rich(
        &mut self,
        spans: &[OwnedSpan],
        font_size: f32,
        max_width: Option<f32>,
        wrap: bool,
        ellipsis: bool,
    ) -> [f32; 2];

    #[allow(clippy::too_many_arguments)]
    fn measure_rich_formatted(
        &mut self,
        spans: &[OwnedSpan],
        font_size: f32,
        font_family: Option<&str>,
        tracking: f32,
        line_height: f32,
        max_width: Option<f32>,
        wrap: bool,
        ellipsis: bool,
    ) -> [f32; 2] {
        let _ = (font_family, tracking, line_height);
        self.measure_rich(spans, font_size, max_width, wrap, ellipsis)
    }

    #[allow(clippy::too_many_arguments)]
    fn measure_rich_link_rects(
        &mut self,
        spans: &[OwnedSpan],
        rect: Rect,
        font_size: f32,
        font_family: Option<&str>,
        wrap: bool,
        align: TextAlign,
        tracking: f32,
        line_height: f32,
    ) -> Vec<(usize, Rect)> {
        let _ = (font_family, align, tracking);
        let char_w = (font_size * 0.58).max(1.0);
        let lh = (font_size * line_height.max(0.5)).max(1.0);
        let mut cursor_x = rect.x;
        let mut cursor_y = rect.y;
        let max_x = if wrap && rect.w > 0.0 {
            rect.x + rect.w
        } else {
            f32::INFINITY
        };
        let mut out = Vec::new();
        for (idx, sp) in spans.iter().enumerate() {
            let sw = sp.text.chars().count() as f32 * char_w;
            if wrap && cursor_x > rect.x && cursor_x + sw > max_x {
                cursor_x = rect.x;
                cursor_y += lh;
            }
            if sp.kind == SpanKind::Link {
                out.push((idx, Rect::new(cursor_x, cursor_y, sw.max(8.0), lh)));
            }
            cursor_x += sw;
        }
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AxisKind {
    Row,
    Col,
    Grid,
    ZStack,
    Leaf,
}

#[derive(Clone, Debug)]
pub(crate) struct Style {
    pub pad_l: f32,
    pub pad_r: f32,
    pub pad_t: f32,
    pub pad_b: f32,
    pub margin_l: f32,
    pub margin_r: f32,
    pub margin_t: f32,
    pub margin_b: f32,
    pub gap: f32,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub grow: f32,
    pub bg: Option<Color>,
    pub fg: Option<Color>,
    pub border_width: f32,
    pub border_color: Color,
    pub radius: f32,
    pub shadow_blur: f32,
    pub shadow_color: Color,
    pub shape: RectShape,
    pub font_size: Option<f32>,
    pub font_weight: Option<u16>,
    pub mono: bool,
    pub glass: bool,
    pub wrap: bool,
    pub ellipsis: bool,
    pub anim_layout_id: Option<WidgetId>,
    pub cursor_glow: bool,
    pub shimmer: bool,
    pub pos: Option<[f32; 2]>,
    pub offset: Option<[f32; 2]>,
    pub justify: Justify,
    pub align: Option<Align>,
    pub align_self: Option<Align>,
    pub clamp_viewport: bool,
    pub placeholder: Option<String>,
    pub tooltip: Option<String>,
    pub image_tint: Color,
    pub image_fit: ImageFit,
    pub image_filter: TextureFilter,
    pub image_size: Option<[u32; 2]>,
    pub fill_color: Option<Color>,
    pub fill_auto: bool,
    pub stroke_width: f32,
    pub selectable: bool,
    pub hover_bg: Option<Color>,
    pub active_bg: Option<Color>,
    pub hover_fg: Option<Color>,
    pub active_fg: Option<Color>,
    pub hover_bd: Option<Color>,
    pub focus_bd: Option<Color>,
    pub focus_ring_width: Option<f32>,
    pub focus_ring_offset: Option<f32>,
    pub focus_ring_color: Option<Color>,
    pub no_focus_ring: bool,
    pub disabled_bg: Option<Color>,
    pub disabled_fg: Option<Color>,
    pub ripple: Option<bool>,
    pub scrollbar_w: Option<f32>,
    pub scrollbar_track: Option<Color>,
    pub scrollbar_thumb: Option<Color>,
    pub min_w: Option<f32>,
    pub max_w: Option<f32>,
    pub min_h: Option<f32>,
    pub max_h: Option<f32>,
    pub opacity: f32,
    pub clip: bool,
    pub flex_wrap: bool,
    pub corner_radii: Option<[f32; 4]>,
    pub border_sides: Option<[f32; 4]>,
    pub border_sides_color: Color,
    pub no_scrollbar: bool,
    pub width_frac: Option<f32>,
    pub height_frac: Option<f32>,
    pub aspect_ratio: Option<f32>,
    pub sticky_top: Option<f32>,
    pub grid_cols: Option<usize>,
    pub grid_min_col_w: Option<f32>,
    pub text_align: TextAlign,
    pub tracking: f32,
    pub line_height: f32,
    pub selection_bg: Option<Color>,
    pub cursor: Option<CursorIcon>,
    pub passthrough: bool,
    pub bg_grad: Option<Gradient>,
    pub bd_grad: Option<Gradient>,
    pub scale: f32,
    pub rotate_rad: f32,
    pub translate: [f32; 2],
    pub svg_fg_tint: bool,
    pub font_family: Option<String>,
    pub corner_fillet: f32,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            pad_l: 0.0,
            pad_r: 0.0,
            pad_t: 0.0,
            pad_b: 0.0,
            margin_l: 0.0,
            margin_r: 0.0,
            margin_t: 0.0,
            margin_b: 0.0,
            gap: 0.0,
            width: None,
            height: None,
            grow: 0.0,
            bg: None,
            fg: None,
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
            radius: 0.0,
            shadow_blur: 0.0,
            shadow_color: Color::TRANSPARENT,
            shape: RectShape::Box,
            font_size: None,
            font_weight: None,
            mono: false,
            glass: false,
            wrap: false,
            ellipsis: false,
            anim_layout_id: None,
            cursor_glow: false,
            shimmer: false,
            pos: None,
            offset: None,
            justify: Justify::Start,
            align: None,
            align_self: None,
            clamp_viewport: false,
            placeholder: None,
            tooltip: None,
            image_tint: Color::WHITE,
            image_fit: ImageFit::Fill,
            image_filter: TextureFilter::Linear,
            image_size: None,
            fill_color: None,
            fill_auto: false,
            stroke_width: 2.0,
            selectable: false,
            hover_bg: None,
            active_bg: None,
            hover_fg: None,
            active_fg: None,
            hover_bd: None,
            focus_bd: None,
            focus_ring_width: None,
            focus_ring_offset: None,
            focus_ring_color: None,
            no_focus_ring: false,
            disabled_bg: None,
            disabled_fg: None,
            ripple: None,
            scrollbar_w: None,
            scrollbar_track: None,
            scrollbar_thumb: None,
            min_w: None,
            max_w: None,
            min_h: None,
            max_h: None,
            opacity: 1.0,
            clip: false,
            flex_wrap: false,
            corner_radii: None,
            border_sides: None,
            border_sides_color: Color::TRANSPARENT,
            no_scrollbar: false,
            width_frac: None,
            height_frac: None,
            aspect_ratio: None,
            sticky_top: None,
            grid_cols: None,
            grid_min_col_w: None,
            text_align: TextAlign::Left,
            tracking: 0.0,
            line_height: 1.35,
            selection_bg: None,
            cursor: None,
            passthrough: false,
            bg_grad: None,
            bd_grad: None,
            scale: 1.0,
            rotate_rad: 0.0,
            translate: [0.0, 0.0],
            svg_fg_tint: false,
            font_family: None,
            corner_fillet: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct HitNode {
    pub id: WidgetId,
    pub bounds: Rect,
    pub clip: Option<Rect>,
    pub focusable: bool,
    pub enabled: bool,
    pub is_overlay: bool,
    pub cursor: Option<CursorIcon>,
    pub inv_xform: Option<[f32; 4]>,
    pub ripple: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollState {
    pub target_y: f32,
    pub spring: SpringState,
    pub max_y: f32,
    pub target_x: f32,
    pub spring_x: SpringState,
    pub max_x: f32,
    pub total_h: f32,
    pub inner_h: f32,
    pub total_w: f32,
    pub inner_w: f32,
    pub scrollbar_drag_offset: Option<f32>,
}

impl Default for ScrollState {
    fn default() -> Self {
        Self {
            target_y: 0.0,
            spring: SpringState::new(0.0),
            max_y: 0.0,
            target_x: 0.0,
            spring_x: SpringState::new(0.0),
            max_x: 0.0,
            total_h: 0.0,
            inner_h: 0.0,
            total_w: 0.0,
            inner_w: 0.0,
            scrollbar_drag_offset: None,
        }
    }
}

pub(crate) fn scrollbar_widget_id(scroll_id: WidgetId) -> WidgetId {
    WidgetId(scroll_id.0 ^ 0x517C_C1B7_2722_0A95)
}

#[derive(Clone, Debug)]
pub(crate) struct LayoutNode {
    pub id: WidgetId,
    pub role: Role,
    pub kind: AxisKind,
    pub style: Style,
    pub label: String,
    pub semantic_value: Option<String>,
    pub text: Option<String>,
    pub rich_spans: Option<Vec<OwnedSpan>>,
    pub rich_links: Vec<(WidgetId, String)>,
    pub sparkline_samples: Option<Vec<f32>>,
    pub image_tex: Option<TextureId>,
    pub viewport_data: Option<(Vec<Canvas2DCmd>, SceneSnapshot)>,
    pub is_scroll: bool,
    pub scroll_offset_y: f32,
    pub focusable: bool,
    pub hit_testable: bool,
    pub enabled: bool,
    pub is_overlay: bool,
    pub children: Vec<usize>,
    pub intrinsic_w: f32,
    pub intrinsic_h: f32,
    pub content_w: f32,
    pub content_h: f32,
    pub rect: Rect,
}

impl LayoutNode {
    pub(crate) fn new_leaf(id: WidgetId, role: Role, label: String) -> Self {
        Self {
            id,
            role,
            kind: AxisKind::Leaf,
            style: Style::default(),
            label,
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
            enabled: true,
            is_overlay: false,
            children: Vec::new(),
            intrinsic_w: 0.0,
            intrinsic_h: 0.0,
            content_w: 0.0,
            content_h: 0.0,
            rect: Rect::ZERO,
        }
    }
}

impl Default for LayoutNode {
    fn default() -> Self {
        Self::new_leaf(WidgetId(0), Role::Container, String::new())
    }
}

/// Immediate-mode UI frame builder, 2-pass layout engine, physics runtime, and input router.
pub struct UiContext {
    arena: FrameArena,
    pub(crate) ids: IdStack,
    pub(crate) theme: Theme,
    draw_list: DrawList,
    pending_semantics: Vec<SemanticNode>,
    semantics: Vec<SemanticNode>,
    pending_hits: Vec<HitNode>,
    pending_interacts: Vec<(usize, usize, HitNode)>,
    hits: Vec<HitNode>,
    pending_rects: HashMap<WidgetId, Rect>,
    pub(crate) prev_rects: HashMap<WidgetId, Rect>,
    pub(crate) nodes: Vec<LayoutNode>,
    pub(crate) container_stack: Vec<usize>,
    overlay_roots: Vec<usize>,
    pub(crate) viewport_w: f32,
    pub(crate) viewport_h: f32,
    scale_factor: f32,
    pub(crate) pointer_pos: Option<[f32; 2]>,
    prev_pointer_pos: Option<[f32; 2]>,
    pub(crate) frame_pointer_delta: [f32; 2],
    pub(crate) frame_scroll_delta: [f32; 2],
    pub(crate) hovered_id: Option<WidgetId>,
    pub(crate) just_pressed_id: Option<WidgetId>,
    pub(crate) just_pressed_pos: Option<[f32; 2]>,
    pub(crate) pressed_id: Option<WidgetId>,
    pub(crate) clicked_id: Option<WidgetId>,
    pressed_right_id: Option<WidgetId>,
    pub(crate) right_clicked_id: Option<WidgetId>,
    last_click_id: Option<WidgetId>,
    last_click_time_s: f64,
    pub(crate) double_clicked_id: Option<WidgetId>,
    pub(crate) focused_id: Option<WidgetId>,
    pub(crate) focus_visible: bool,
    pub(crate) focused_is_text: bool,
    pub(crate) focused_caret: Option<(WidgetId, usize)>,
    pub(crate) focused_selection: Option<(WidgetId, usize, usize)>,
    pub(crate) activated_id: Option<WidgetId>,
    pub(crate) last_key_pressed: Option<Key>,
    pub(crate) last_modifiers: Modifiers,
    pub(crate) pending_key_events: Vec<(Key, Modifiers)>,
    pub(crate) pending_ime_preedit: Option<String>,
    pub(crate) pending_ime_commit: Option<String>,
    pub(crate) capturing_keybind_id: Option<WidgetId>,
    pub(crate) captured_key: Option<(WidgetId, Key)>,
    pub(crate) open_dropdown_id: Option<WidgetId>,
    pub(crate) dismiss_requested: bool,
    pub(crate) has_csd_titlebar: bool,
    pub(crate) window_icon_tex: Option<TextureId>,
    pub(crate) prev_disabled_ids: HashSet<WidgetId>,
    pending_disabled_ids: HashSet<WidgetId>,
    pub(crate) prev_submit_on_enter_ids: HashSet<WidgetId>,
    pub(crate) pending_submit_on_enter_ids: HashSet<WidgetId>,
    pub(crate) submitted_ids: HashSet<WidgetId>,
    pub(crate) prev_caret_rects: HashMap<WidgetId, Rect>,
    has_active_cursor_glow: bool,
    has_active_shimmer: bool,
    has_active_tooltip_timer: bool,
    hover_anchor_id: Option<WidgetId>,
    hover_elapsed_s: f32,
    pending_tooltip: Option<(Rect, String)>,
    dirty: bool,
    elapsed_time_s: f64,
    shimmer_phase: f32,
    springs: HashMap<WidgetId, (SpringState, f32, f32)>,
    tweens: HashMap<WidgetId, TweenState>,
    flips: HashMap<WidgetId, FlipState>,
    ripples: Vec<RippleEffect>,
    pub(crate) scroll_states: HashMap<WidgetId, ScrollState>,
    pending_scroll_to_bottom: HashSet<WidgetId>,
    pending_scrollbar_rects: HashMap<WidgetId, (Rect, Rect)>,
    pub(crate) prev_scrollbar_rects: HashMap<WidgetId, (Rect, Rect)>,
    pending_in_view: HashSet<WidgetId>,
    prev_in_view: HashSet<WidgetId>,
    pub(crate) widget_text_states: HashMap<WidgetId, crate::ui::text_edit::TextEditState>,
    pub(crate) open_combobox_id: Option<WidgetId>,
    pub(crate) combobox_highlight: HashMap<WidgetId, usize>,
    pub(crate) open_color_picker_id: Option<WidgetId>,
    pub(crate) color_picker_hsv: HashMap<WidgetId, [f32; 3]>,
    pub(crate) range_drag_thumb: HashMap<WidgetId, u8>,
    pub(crate) table_resize_drag: Option<(WidgetId, f32, f32)>,
    pub(crate) tree_depth: u16,
    pub(crate) has_active_indeterminate: bool,
    pub(crate) has_active_toasts: bool,
    text_select_anchor: Option<(WidgetId, usize)>,
    text_select_range: Option<(WidgetId, usize, usize)>,
    pub(crate) texture_sizes: HashMap<TextureId, [u32; 2]>,
    pub(crate) animated_frames: HashMap<TextureId, usize>,
    pub(crate) code_blocks: HashMap<WidgetId, (String, Option<String>, bool)>,
    pub(crate) viewport_scenes: HashMap<WidgetId, Scene3D>,
    pub(crate) viewport_raw_fbo_id: Option<u32>,
    hud_visible: bool,
    hud_hotkey_enabled: bool,
    perf_stats: PerfStats,
    frame_gen: u64,
    state_last_seen: HashMap<WidgetId, u64>,
    pub(crate) vlist_prefix_scratch: Vec<f32>,
}

impl Default for UiContext {
    fn default() -> Self {
        Self::new()
    }
}

impl UiContext {
    /// Creates a new `UiContext` with a default 256 KiB per-frame bump arena.
    pub fn new() -> Self {
        Self::with_arena_capacity(DEFAULT_ARENA_BYTES)
    }

    /// Creates a new `UiContext` with an explicit frame arena byte capacity.
    pub fn with_arena_capacity(arena_bytes: usize) -> Self {
        Self {
            arena: FrameArena::new(arena_bytes),
            ids: IdStack::new(),
            theme: Theme::default(),
            draw_list: DrawList::default(),
            pending_semantics: Vec::new(),
            semantics: Vec::new(),
            pending_hits: Vec::new(),
            pending_interacts: Vec::new(),
            hits: Vec::new(),
            pending_rects: HashMap::new(),
            prev_rects: HashMap::new(),
            nodes: Vec::new(),
            container_stack: Vec::new(),
            overlay_roots: Vec::new(),
            viewport_w: 1024.0,
            viewport_h: 700.0,
            scale_factor: 1.0,
            pointer_pos: None,
            prev_pointer_pos: None,
            frame_pointer_delta: [0.0, 0.0],
            frame_scroll_delta: [0.0, 0.0],
            hovered_id: None,
            just_pressed_id: None,
            just_pressed_pos: None,
            pressed_id: None,
            clicked_id: None,
            pressed_right_id: None,
            right_clicked_id: None,
            last_click_id: None,
            last_click_time_s: -10.0,
            double_clicked_id: None,
            focused_id: None,
            focus_visible: false,
            focused_is_text: false,
            focused_caret: None,
            focused_selection: None,
            activated_id: None,
            last_key_pressed: None,
            last_modifiers: Modifiers::default(),
            pending_key_events: Vec::new(),
            pending_ime_preedit: None,
            pending_ime_commit: None,
            capturing_keybind_id: None,
            captured_key: None,
            open_dropdown_id: None,
            dismiss_requested: false,
            has_csd_titlebar: false,
            window_icon_tex: None,
            prev_disabled_ids: HashSet::new(),
            pending_disabled_ids: HashSet::new(),
            prev_submit_on_enter_ids: HashSet::new(),
            pending_submit_on_enter_ids: HashSet::new(),
            submitted_ids: HashSet::new(),
            prev_caret_rects: HashMap::new(),
            has_active_cursor_glow: false,
            has_active_shimmer: false,
            has_active_tooltip_timer: false,
            hover_anchor_id: None,
            hover_elapsed_s: 0.0,
            pending_tooltip: None,
            dirty: true,
            elapsed_time_s: 0.0,
            shimmer_phase: 0.0,
            springs: HashMap::new(),
            tweens: HashMap::new(),
            flips: HashMap::new(),
            ripples: Vec::new(),
            scroll_states: HashMap::new(),
            pending_scroll_to_bottom: HashSet::new(),
            pending_scrollbar_rects: HashMap::new(),
            prev_scrollbar_rects: HashMap::new(),
            pending_in_view: HashSet::new(),
            prev_in_view: HashSet::new(),
            widget_text_states: HashMap::new(),
            open_combobox_id: None,
            combobox_highlight: HashMap::new(),
            open_color_picker_id: None,
            color_picker_hsv: HashMap::new(),
            range_drag_thumb: HashMap::new(),
            table_resize_drag: None,
            tree_depth: 0,
            has_active_indeterminate: false,
            has_active_toasts: false,
            text_select_anchor: None,
            text_select_range: None,
            texture_sizes: HashMap::new(),
            animated_frames: HashMap::new(),
            code_blocks: HashMap::new(),
            viewport_scenes: HashMap::new(),
            viewport_raw_fbo_id: None,
            hud_visible: false,
            hud_hotkey_enabled: true,
            perf_stats: PerfStats::default(),
            frame_gen: 0,
            state_last_seen: HashMap::new(),
            vlist_prefix_scratch: Vec::new(),
        }
    }

    /// Returns whether the built-in `F12` telemetry overlay HUD is currently visible.
    pub fn is_hud_visible(&self) -> bool {
        self.hud_visible
    }

    /// Alias for [`UiContext::is_hud_visible`]: returns whether the built-in telemetry HUD is visible.
    pub fn hud_visible(&self) -> bool {
        self.hud_visible
    }

    /// Explicitly shows or hides the built-in `F12` telemetry overlay HUD.
    pub fn set_hud_visible(&mut self, visible: bool) {
        if self.hud_visible != visible {
            self.hud_visible = visible;
            self.dirty = true;
        }
    }

    /// Returns whether pressing `F12` automatically toggles the built-in telemetry HUD.
    pub fn hud_hotkey_enabled(&self) -> bool {
        self.hud_hotkey_enabled
    }

    /// Enables or disables the built-in `F12` hotkey that toggles the telemetry HUD.
    ///
    /// Defaults to `true`. Set to `false` in embedded hosts where the host application owns `F12`.
    pub fn set_hud_hotkey_enabled(&mut self, enabled: bool) {
        self.hud_hotkey_enabled = enabled;
    }

    /// Returns the latest frame performance telemetry counters.
    pub fn perf_stats(&self) -> PerfStats {
        self.perf_stats
    }

    /// Records GPU batch and atlas metrics measured by `Renderer`.
    pub fn update_renderer_perf(
        &mut self,
        render_ms: f32,
        sdf_instances: usize,
        atlas_size: [u32; 2],
        atlas_occupancy: f32,
    ) {
        self.perf_stats.render_time_ms = render_ms;
        self.perf_stats.sdf_instances = sdf_instances;
        self.perf_stats.glyph_atlas_size = atlas_size;
        self.perf_stats.glyph_atlas_occupancy = atlas_occupancy;
    }

    /// Records GPU timer query and vertex/instance buffer byte telemetry.
    pub(crate) fn update_gpu_perf(&mut self, gpu_time_ms: Option<f32>, gpu_buffer_bytes: usize) {
        self.perf_stats.gpu_time_ms = gpu_time_ms;
        self.perf_stats.gpu_buffer_bytes = gpu_buffer_bytes;
    }

    /// Returns a reference to the active theme.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Replaces the active theme and marks the UI dirty when changed.
    pub fn set_theme(&mut self, theme: Theme) {
        if self.theme != theme {
            self.theme = theme;
            self.dirty = true;
        }
    }

    /// Returns the last known pointer position in logical coordinates.
    pub fn pointer_pos(&self) -> Option<[f32; 2]> {
        self.pointer_pos
    }

    /// Returns the current viewport `[width, height]` in logical points.
    pub fn viewport_size(&self) -> [f32; 2] {
        [self.viewport_w, self.viewport_h]
    }

    /// Computes the deterministic `WidgetId` for `label` within the current ID scope.
    pub fn id(&self, label: &str) -> WidgetId {
        self.ids.id(label)
    }

    /// Returns the laid-out `Rect` of `id` from the last completed frame, if present.
    /// Returns the last computed caret screen rectangle for a focused `input` or `textarea` by `id_label`.
    pub fn caret_rect(&self, id_label: &str) -> Option<Rect> {
        let id = self.ids.id(id_label);
        self.prev_caret_rects.get(&id).copied()
    }

    /// Returns the laid-out `Rect` of widget `id` from the last completed frame, if present.
    pub fn prev_rect(&self, id: WidgetId) -> Option<Rect> {
        self.prev_rects.get(&id).copied()
    }

    /// Returns the laid-out `Rect` of the scoped `label` from the last completed frame, if present.
    pub fn prev_rect_by_label(&self, label: &str) -> Option<Rect> {
        self.prev_rects.get(&self.ids.id(label)).copied()
    }

    /// Returns the per-frame bump arena for transient allocations.
    pub fn arena(&mut self) -> &mut FrameArena {
        &mut self.arena
    }

    /// Returns the high-water mark of the frame bump arena in bytes.
    pub fn arena_high_water_mark(&self) -> usize {
        self.arena.high_water_mark()
    }

    /// Pushes a scoped identifier onto the deterministic FNV-1a stack.
    pub fn push_id(&mut self, label: &str) {
        self.ids.push_id(label);
    }

    /// Pushes a numeric loop index onto the identifier stack.
    pub fn push_u64(&mut self, value: u64) {
        self.ids.push_u64(value);
    }

    /// Pops the current identifier scope.
    pub fn pop_id(&mut self) {
        self.ids.pop_id();
    }

    /// Returns duplicate interactive IDs detected during the current frame.
    pub fn id_collisions(&self) -> &[WidgetId] {
        self.ids.collisions()
    }

    /// Returns the laid-out `DrawList` produced by the last completed frame.
    pub fn draw_list(&self) -> &DrawList {
        &self.draw_list
    }

    /// Returns the semantic accessibility nodes from the last completed frame.
    pub fn semantics(&self) -> &[SemanticNode] {
        &self.semantics
    }

    /// Returns the currently keyboard-focused widget ID, if any.
    pub fn focused_id(&self) -> Option<WidgetId> {
        self.focused_id
    }

    /// Returns `true` when keyboard focus is currently held by a text-entry widget (`TextInput`).
    pub fn is_text_input_focused(&self) -> bool {
        self.focused_is_text
    }

    /// Clears keyboard focus, active text-input caret/IME state, and any pending keybind capture.
    pub fn clear_focus(&mut self) {
        let had_focus = self.focused_id.take().is_some();
        let had_capture = self.capturing_keybind_id.take().is_some();
        let had_text = self.focused_is_text;
        self.focus_visible = false;
        self.focused_is_text = false;
        self.focused_caret = None;
        self.focused_selection = None;
        self.pending_ime_preedit = None;
        self.pending_ime_commit = None;
        if had_focus || had_capture || had_text {
            self.dirty = true;
        }
    }

    /// Returns the interactive widget ID currently under the pointer, if any.
    pub fn hovered_id(&self) -> Option<WidgetId> {
        self.hovered_id
    }

    /// Returns `true` if the pointer is currently over an interactive widget or any visible UI surface.
    ///
    /// Returns `false` when the pointer is outside the viewport (after [`UiContext::on_pointer_leave`])
    /// or over transparent click-through gaps between floating panels.
    pub fn is_pointer_over_ui(&self) -> bool {
        if self.hovered_id.is_some() {
            return true;
        }
        let Some([px, py]) = self.pointer_pos else {
            return false;
        };
        self.draw_list_contains_point(px, py)
    }

    /// Returns `true` if `key` was pressed this frame outside of text input and keybind capture.
    pub fn shortcut_pressed(&self, key: Key) -> bool {
        !self.focused_is_text
            && self.capturing_keybind_id.is_none()
            && self.last_key_pressed == Some(key)
    }

    /// Returns `true` if `Escape` dismissal was triggered before or during this frame.
    pub fn dismiss_requested(&self) -> bool {
        self.dismiss_requested
    }

    /// Returns `true` if the UI has pending state changes that require a redraw without clearing the latch.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Returns `true` if the UI needs a redraw and clears the dirty latch.
    pub fn take_dirty(&mut self) -> bool {
        let was_dirty = self.dirty;
        self.dirty = false;
        was_dirty
    }

    /// Marks the UI dirty so the next idle cycle schedules a redraw.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Registers raw TTF/OTF/TTC font bytes globally using the font's embedded OpenType family names.
    pub fn register_font_bytes(&mut self, bytes: &[u8]) {
        crate::gfx::text::register_font_bytes(bytes);
        self.dirty = true;
    }

    /// Registers raw TTF/OTF/TTC font bytes globally under `name` (in addition to its embedded OpenType family names).
    pub fn register_font(&mut self, name: &str, bytes: &[u8]) {
        crate::gfx::text::register_font(name, bytes);
        self.dirty = true;
    }

    pub(crate) fn set_texture_sizes(&mut self, sizes: &HashMap<TextureId, [u32; 2]>) {
        self.texture_sizes.clear();
        for (&id, &sz) in sizes {
            self.texture_sizes.insert(id, sz);
        }
    }

    /// Advances all active springs, tweens, FLIP transitions, scroll springs, and ripples by `dt`.
    pub fn step_physics(&mut self, dt: f32) {
        let dt_pos = if dt.is_finite() && dt > 0.0 {
            dt.min(0.05)
        } else {
            0.0
        };
        self.elapsed_time_s += f64::from(dt_pos);
        if dt_pos > 0.0 {
            self.perf_stats.frame_time_ms = dt_pos * 1000.0;
        }
        if dt_pos <= 0.0 {
            return;
        }

        for (spring, k, c) in self.springs.values_mut() {
            spring.step(dt_pos, *k, *c);
        }
        for tw in self.tweens.values_mut() {
            tw.step(dt_pos);
        }
        for flip in self.flips.values_mut() {
            flip.step(dt_pos);
        }
        for sc in self.scroll_states.values_mut() {
            sc.spring.step(dt_pos, 320.0, 34.0);
            sc.spring_x.step(dt_pos, 320.0, 34.0);
        }
        for ripple in &mut self.ripples {
            ripple.progress += dt_pos * 2.4;
        }
        self.ripples.retain(|r| r.progress < 1.0);

        if self.has_active_shimmer {
            self.shimmer_phase = (self.shimmer_phase + dt_pos * 1.1) % 1.0;
        }
        if self.pointer_pos.is_some()
            && self.hovered_id == self.hover_anchor_id
            && (self.hovered_id.is_some() || self.has_active_tooltip_timer)
        {
            self.hover_elapsed_s = (self.hover_elapsed_s + dt_pos).min(10.0);
        }
    }

    /// Returns total accumulated UI time in seconds.
    pub fn elapsed_time_s(&self) -> f64 {
        self.elapsed_time_s
    }

    /// Returns `true` while any spring, tween, FLIP layout, scroll, ripple, shimmer, spinner, toast, or tooltip timer is active.
    pub fn has_active_animations(&self) -> bool {
        self.has_active_shimmer
            || self.has_active_indeterminate
            || self.has_active_toasts
            || self.has_active_tooltip_timer
            || !self.ripples.is_empty()
            || self.springs.values().any(|(s, _, _)| !s.is_settled())
            || self.tweens.values().any(|t| !t.spring.is_settled())
            || self.flips.values().any(|f| {
                !f.x.is_settled() || !f.y.is_settled() || !f.w.is_settled() || !f.h.is_settled()
            })
            || self
                .scroll_states
                .values()
                .any(|s| !s.spring.is_settled() || !s.spring_x.is_settled())
    }

    /// Interruptible damped harmonic spring keyed by `id_label` within the current ID scope.
    pub fn spring(&mut self, id_label: &str, target: f32, stiffness: f32, damping: f32) -> f32 {
        let id = self.ids.id(id_label);
        let entry = self
            .springs
            .entry(id)
            .or_insert_with(|| (SpringState::new(target), stiffness, damping));
        entry.0.target = target;
        entry.1 = stiffness;
        entry.2 = damping;
        entry.0.value
    }

    /// Smooth velocity-preserving property tween over `duration_ms` keyed by `id_label`.
    pub fn anim(&mut self, id_label: &str, target: f32, duration_ms: f32) -> f32 {
        let id = self.ids.id(id_label);
        let entry = self
            .tweens
            .entry(id)
            .or_insert_with(|| TweenState::new(target, duration_ms));
        entry.spring.target = target;
        entry.duration_ms = duration_ms;
        entry.spring.value
    }

    /// Reusable low-level pointer and scroll gesture recognizer on an explicit `rect`.
    ///
    /// Participates in tree/Z-order relative to sibling widgets and enclosing containers.
    pub fn interact(&mut self, id: WidgetId, rect: Rect) -> GestureResponse {
        let _ = self.ids.register_interactive(id);
        let parent_idx = self.container_stack.last().copied().unwrap_or(0);
        let child_pos = self.nodes[parent_idx].children.len();
        let is_overlay = self.nodes[parent_idx].is_overlay;
        self.pending_interacts.push((
            parent_idx,
            child_pos,
            HitNode {
                id,
                bounds: rect,
                clip: None,
                focusable: false,
                enabled: true,
                is_overlay,
                cursor: None,
                inv_xform: None,
                ripple: false,
            },
        ));
        let hovered = self.hovered_id == Some(id);
        let dragging = self.pressed_id == Some(id);
        GestureResponse {
            hovered,
            clicked: self.clicked_id == Some(id) || self.activated_id == Some(id),
            double_clicked: self.double_clicked_id == Some(id),
            right_clicked: self.right_clicked_id == Some(id),
            dragging,
            delta: if dragging {
                self.frame_pointer_delta
            } else {
                [0.0, 0.0]
            },
            scroll_delta: if hovered {
                self.frame_scroll_delta
            } else {
                [0.0, 0.0]
            },
        }
    }

    /// Begins a new layout frame for a viewport sized in logical points at `scale_factor`.
    pub fn begin_frame(&mut self, width: f32, height: f32, scale_factor: f32) {
        self.arena.reset();
        self.ids.clear_frame();
        self.draw_list.clear();
        self.pending_semantics.clear();
        self.pending_hits.clear();
        self.pending_interacts.clear();
        self.pending_rects.clear();
        self.pending_scrollbar_rects.clear();
        self.pending_in_view.clear();
        self.pending_disabled_ids.clear();
        self.nodes.clear();
        self.container_stack.clear();
        self.overlay_roots.clear();
        self.focused_caret = None;
        self.focused_selection = None;
        self.has_csd_titlebar = false;
        self.has_active_tooltip_timer = false;
        self.has_active_indeterminate = false;
        self.has_active_toasts = false;
        self.tree_depth = 0;
        self.pending_tooltip = None;

        self.viewport_w = width.max(0.0);
        self.viewport_h = height.max(0.0);
        self.scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };

        let root_id = self.ids.id("__root__");
        self.nodes.push(LayoutNode {
            id: root_id,
            role: Role::Window,
            kind: AxisKind::Col,
            style: Style {
                width: Some(self.viewport_w),
                height: Some(self.viewport_h),
                ..Style::default()
            },
            label: String::from("Window"),
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
            enabled: true,
            is_overlay: false,
            children: Vec::new(),
            intrinsic_w: self.viewport_w,
            intrinsic_h: self.viewport_h,
            content_w: self.viewport_w,
            content_h: self.viewport_h,
            rect: Rect::new(0.0, 0.0, self.viewport_w, self.viewport_h),
        });
        self.container_stack.push(0);
    }

    /// Finalizes 2-pass layout using heuristic text metrics.
    pub fn end_frame(&mut self) {
        self.end_frame_internal(None);
    }

    /// Finalizes 2-pass layout using a `TextMeasure` implementation (`GlyphAtlas`).
    pub(crate) fn end_frame_with(&mut self, measurer: &mut dyn TextMeasure) {
        self.end_frame_internal(Some(measurer));
    }

    fn end_frame_internal(&mut self, mut measurer: Option<&mut dyn TextMeasure>) {
        #[cfg(not(target_arch = "wasm32"))]
        let t_layout = std::time::Instant::now();

        self.container_stack.truncate(1);

        self.measure_node(
            0,
            self.theme.font_size,
            Some(self.viewport_w),
            &mut measurer,
        );
        let root_rect = Rect::new(0.0, 0.0, self.viewport_w, self.viewport_h);
        self.layout_node(0, root_rect, self.scale_factor, &mut measurer);
        self.apply_flip_transitions(0);

        self.has_active_cursor_glow = false;
        self.has_active_shimmer = false;
        let mut nodes = core::mem::take(&mut self.nodes);
        let root_children_len = nodes[0].children.len();
        self.flush_interacts(0, 0, None);
        for i in 0..root_children_len {
            let cid = nodes[0].children[i];
            self.emit_node(&mut nodes, cid, false, None, &mut measurer);
            self.flush_interacts(0, i + 1, None);
        }
        self.nodes = nodes;

        if self.hud_visible {
            self.emit_telemetry_hud();
        }
        if let Some((anchor, tip_text)) = self.pending_tooltip.take() {
            self.emit_tooltip_overlay(anchor, &tip_text);
        }

        let overlay_roots = core::mem::take(&mut self.overlay_roots);
        for &oid in &overlay_roots {
            let overlay_avail_w = {
                let node = &self.nodes[oid];
                let x = node.style.pos.map_or(0.0, |p| p[0]);
                if let Some(w) = node.style.width {
                    Some(w.min(self.viewport_w))
                } else if node.style.grow > 0.0 {
                    Some((self.viewport_w - x).max(0.0))
                } else {
                    None
                }
            };
            self.measure_node(oid, self.theme.font_size, overlay_avail_w, &mut measurer);
            let overlay_rect = {
                let node = &self.nodes[oid];
                let mut x = node.style.pos.map_or(0.0, |p| p[0]);
                let mut y = node.style.pos.map_or(0.0, |p| p[1]);
                let w = if node.style.grow > 0.0 && node.style.width.is_none() {
                    (self.viewport_w - x).max(0.0)
                } else {
                    node.style
                        .width
                        .unwrap_or(node.intrinsic_w)
                        .min(self.viewport_w)
                };
                let h = if node.style.grow > 0.0 && node.style.height.is_none() {
                    (self.viewport_h - y).max(0.0)
                } else {
                    node.style
                        .height
                        .unwrap_or(node.intrinsic_h)
                        .min(self.viewport_h)
                };
                if node.style.clamp_viewport {
                    let min_y = if self.has_csd_titlebar { 40.0 } else { 4.0 };
                    x = x.min((self.viewport_w - w - 6.0).max(4.0)).max(4.0);
                    y = y.min((self.viewport_h - h - 6.0).max(min_y)).max(min_y);
                }
                Rect::new(x, y, w, h)
            };
            self.layout_node(oid, overlay_rect, self.scale_factor, &mut measurer);
            self.apply_flip_transitions(oid);
            let mut nodes = core::mem::take(&mut self.nodes);
            self.emit_node(&mut nodes, oid, true, None, &mut measurer);
            self.nodes = nodes;
        }
        self.overlay_roots = overlay_roots;

        self.perf_stats.draw_calls =
            self.draw_list.base_cmds.len() + self.draw_list.overlay_cmds.len();
        self.perf_stats.node_count = self.nodes.len();
        self.perf_stats.arena_used_bytes = self.arena.used();
        self.perf_stats.arena_capacity_bytes = self.arena.capacity();
        self.perf_stats.arena_high_water_bytes = self.arena.high_water_mark();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.perf_stats.layout_time_ms = t_layout.elapsed().as_secs_f32() * 1000.0;
        }

        core::mem::swap(&mut self.semantics, &mut self.pending_semantics);
        self.pending_semantics.clear();
        core::mem::swap(&mut self.hits, &mut self.pending_hits);
        self.pending_hits.clear();
        self.pending_interacts.clear();
        self.prune_stale_state();
        core::mem::swap(&mut self.prev_rects, &mut self.pending_rects);
        self.pending_rects.clear();
        core::mem::swap(
            &mut self.prev_scrollbar_rects,
            &mut self.pending_scrollbar_rects,
        );
        self.pending_scrollbar_rects.clear();
        core::mem::swap(&mut self.prev_in_view, &mut self.pending_in_view);
        self.pending_in_view.clear();
        core::mem::swap(&mut self.prev_disabled_ids, &mut self.pending_disabled_ids);
        self.pending_disabled_ids.clear();
        core::mem::swap(
            &mut self.prev_submit_on_enter_ids,
            &mut self.pending_submit_on_enter_ids,
        );
        self.pending_submit_on_enter_ids.clear();
        self.submitted_ids.clear();

        if let Some(fid) = self.focused_id {
            let still_valid = self
                .semantics
                .iter()
                .any(|n| n.id == fid && n.focusable && n.enabled);
            if !still_valid {
                self.focused_id = None;
            }
        }
        self.sync_focused_is_text();

        if let Some([px, py]) = self.pointer_pos {
            self.hovered_id = self.hit_test(px, py);
            if self.hovered_id != self.hover_anchor_id {
                self.hover_anchor_id = self.hovered_id;
                self.hover_elapsed_s = 0.0;
            }
        }

        self.frame_pointer_delta = [0.0, 0.0];
        self.frame_scroll_delta = [0.0, 0.0];
        self.just_pressed_id = None;
        self.just_pressed_pos = None;
        self.clicked_id = None;
        self.right_clicked_id = None;
        self.double_clicked_id = None;
        self.activated_id = None;
        self.last_key_pressed = None;
        self.pending_key_events.clear();
        self.pending_ime_preedit = None;
        self.pending_ime_commit = None;
        self.captured_key = None;
        self.dismiss_requested = false;
    }

    fn prune_stale_state(&mut self) {
        self.frame_gen = self.frame_gen.wrapping_add(1);
        if !self.frame_gen.is_multiple_of(60) {
            return;
        }
        let gen = self.frame_gen;
        let seen = &mut self.state_last_seen;
        let pending = &self.pending_rects;

        for id in self
            .springs
            .keys()
            .chain(self.tweens.keys())
            .chain(self.flips.keys())
            .chain(self.prev_caret_rects.keys())
            .chain(self.range_drag_thumb.keys())
            .chain(self.combobox_highlight.keys())
            .chain(self.code_blocks.keys())
            .chain(self.viewport_scenes.keys())
            .chain(self.scroll_states.keys())
            .chain(self.widget_text_states.keys())
            .chain(self.color_picker_hsv.keys())
            .copied()
        {
            if pending.contains_key(&id) {
                seen.insert(id, gen);
            } else {
                seen.entry(id).or_insert(gen);
            }
        }

        let is_live = |id: &WidgetId, ttl: u64| -> bool {
            gen.saturating_sub(seen.get(id).copied().unwrap_or(gen)) <= ttl
        };

        self.springs.retain(|id, _| is_live(id, 300));
        self.tweens.retain(|id, _| is_live(id, 300));
        self.flips.retain(|id, _| is_live(id, 300));
        self.prev_caret_rects.retain(|id, _| is_live(id, 300));
        self.range_drag_thumb.retain(|id, _| is_live(id, 300));
        self.combobox_highlight.retain(|id, _| is_live(id, 300));
        self.code_blocks.retain(|id, _| is_live(id, 600));
        self.viewport_scenes.retain(|id, _| is_live(id, 600));

        if self.scroll_states.len() > 256 {
            self.scroll_states.retain(|id, _| is_live(id, 1800));
        }
        if self.widget_text_states.len() > 256 {
            self.widget_text_states.retain(|id, _| is_live(id, 1800));
        }
        if self.color_picker_hsv.len() > 256 {
            self.color_picker_hsv.retain(|id, _| is_live(id, 1800));
        }

        seen.retain(|_, &mut last| gen.saturating_sub(last) <= 1800);
    }

    fn emit_telemetry_hud(&mut self) {
        let s = self.perf_stats;
        let pre_draws = self.draw_list.base_cmds.len() + self.draw_list.overlay_cmds.len();
        let arena_used = self.arena.used();
        let arena_cap = self.arena.capacity();
        let arena_hwm = self.arena.high_water_mark();
        let hud_x = (self.viewport_w - 278.0).max(8.0);
        self.ids.push_id("__f12_telemetry_hud__");
        self.overlay(|ui| {
            ui.txt("Lumenic Telemetry (F12)")
                .sz(12.0)
                .fg(Color::from_hex(0x8AB4F8));
            ui.txt(&format!(
                "Frame {:.2}ms | Layout {:.2}ms | Render {:.2}ms",
                s.frame_time_ms, s.layout_time_ms, s.render_time_ms
            ))
            .sz(11.0);
            ui.txt(&format!(
                "Draws {} | SDF {} | Nodes {}",
                pre_draws,
                s.sdf_instances,
                ui.nodes.len()
            ))
            .sz(11.0);
            ui.txt(&format!(
                "Arena {arena_used}/{arena_cap} B (HWM {arena_hwm})"
            ))
            .sz(11.0);
            ui.txt(&format!(
                "Atlas {}x{} ({:.1}%)",
                s.glyph_atlas_size[0],
                s.glyph_atlas_size[1],
                s.glyph_atlas_occupancy * 100.0
            ))
            .sz(11.0);
            let gpu_ms = s
                .gpu_time_ms
                .map_or_else(|| "n/a".to_owned(), |v| format!("{v:.2}ms"));
            let buf_kib = s.gpu_buffer_bytes as f32 / 1024.0;
            ui.txt(&format!("GPU {gpu_ms} | VBO {buf_kib:.1} KiB"))
                .sz(11.0);
        })
        .pos(hud_x, 12.0)
        .w(266.0)
        .p(10.0)
        .gap(4.0)
        .bg(Color::rgba(14, 17, 24, 228))
        .bd(1.0, Color::from_hex(0x475569))
        .r(8.0);
        self.ids.pop_id();
    }

    fn emit_tooltip_overlay(&mut self, anchor: Rect, text: &str) {
        if text.is_empty() {
            return;
        }
        let surface = self.theme.surface;
        let border = self.theme.border;
        let text_col = self.theme.text;
        let mut ty = anchor.y + anchor.h + 6.0;
        if ty + 32.0 > self.viewport_h && anchor.y > 40.0 {
            ty = (anchor.y - 32.0).max(4.0);
        }
        self.ids.push_id("__tooltip_overlay__");
        self.overlay(|ui| {
            ui.txt(text).sz(12.0).fg(text_col);
        })
        .pos(anchor.x, ty)
        .clamp_viewport()
        .px(8.0)
        .py(5.0)
        .bg(surface)
        .bd(1.0, border)
        .r(6.0)
        .shadow(10.0, Color::rgba(0, 0, 0, 125));
        self.ids.pop_id();
    }

    /// Shows a hover-delayed floating tooltip near `anchor_rect` in the overlay pass.
    pub fn tooltip(&mut self, anchor_rect: Rect, text: &str) {
        if text.is_empty() {
            return;
        }
        let hovered = self
            .pointer_pos
            .is_some_and(|[px, py]| anchor_rect.contains(px, py));
        if hovered {
            if self.hover_elapsed_s >= TOOLTIP_DELAY_S {
                self.pending_tooltip = Some((anchor_rect, String::from(text)));
            } else {
                self.has_active_tooltip_timer = true;
            }
        }
    }

    /// Shows a hover-delayed custom UI tooltip near `anchor_rect` in the overlay pass.
    pub fn tooltip_ui<F: FnOnce(&mut UiContext)>(&mut self, anchor_rect: Rect, f: F) {
        let hovered = self
            .pointer_pos
            .is_some_and(|[px, py]| anchor_rect.contains(px, py));
        if !hovered {
            return;
        }
        if self.hover_elapsed_s < TOOLTIP_DELAY_S {
            self.has_active_tooltip_timer = true;
            return;
        }
        let surface = self.theme.surface;
        let border = self.theme.border;
        let mut ty = anchor_rect.y + anchor_rect.h + 6.0;
        if ty + 48.0 > self.viewport_h && anchor_rect.y > 56.0 {
            ty = (anchor_rect.y - 48.0).max(4.0);
        }
        self.ids.push_id("__tooltip_custom_overlay__");
        self.overlay(f)
            .pos(anchor_rect.x, ty)
            .clamp_viewport()
            .p(8.0)
            .gap(4.0)
            .bg(surface)
            .bd(1.0, border)
            .r(6.0)
            .shadow(10.0, Color::rgba(0, 0, 0, 125));
        self.ids.pop_id();
    }

    fn apply_flip_transitions(&mut self, idx: usize) {
        if let Some(fid) = self.nodes[idx].style.anim_layout_id {
            let target = self.nodes[idx].rect;
            let flip = self
                .flips
                .entry(fid)
                .or_insert_with(|| FlipState::new(target));
            flip.set_target(target);
            self.nodes[idx].rect = flip.rect().snap(self.scale_factor);
        }
        let children_len = self.nodes[idx].children.len();
        for i in 0..children_len {
            let cid = self.nodes[idx].children[i];
            self.apply_flip_transitions(cid);
        }
    }

    pub(crate) fn alloc_node(&mut self, mut node: LayoutNode, attach_to_parent: bool) -> usize {
        let idx = self.nodes.len();
        if attach_to_parent {
            if let Some(&parent_idx) = self.container_stack.last() {
                node.is_overlay = self.nodes[parent_idx].is_overlay;
                self.nodes[parent_idx].children.push(idx);
            }
        }
        self.nodes.push(node);
        idx
    }

    fn build_container<F: FnOnce(&mut UiContext)>(
        &mut self,
        kind: AxisKind,
        label: &str,
        explicit_id: Option<WidgetId>,
        is_overlay_root: bool,
        f: F,
    ) -> StyleBuilder<'_> {
        let id = explicit_id.unwrap_or_else(|| {
            let ordinal = self
                .container_stack
                .last()
                .map_or(0, |&p| self.nodes[p].children.len());
            self.ids.push_u64(ordinal as u64);
            let derived = self.ids.id(label);
            self.ids.pop_id();
            derived
        });

        let node = LayoutNode {
            id,
            role: Role::Container,
            kind,
            style: Style::default(),
            label: String::from(label),
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
            enabled: true,
            is_overlay: is_overlay_root,
            children: Vec::new(),
            intrinsic_w: 0.0,
            intrinsic_h: 0.0,
            content_w: 0.0,
            content_h: 0.0,
            rect: Rect::ZERO,
        };

        let idx = self.alloc_node(node, !is_overlay_root);
        if is_overlay_root {
            self.overlay_roots.push(idx);
        }

        self.ids.push_u64(id.0);
        self.container_stack.push(idx);
        f(self);
        self.container_stack.pop();
        self.ids.pop_id();

        StyleBuilder {
            ui: self,
            node_idx: idx,
            changed: false,
        }
    }

    pub(crate) fn container_with_id<F: FnOnce(&mut UiContext)>(
        &mut self,
        kind: AxisKind,
        label: &str,
        id: WidgetId,
        f: F,
    ) -> StyleBuilder<'_> {
        self.build_container(kind, label, Some(id), false, f)
    }

    pub(crate) fn build_container_overlay_with_id<F: FnOnce(&mut UiContext)>(
        &mut self,
        label: &str,
        id: WidgetId,
        f: F,
    ) -> StyleBuilder<'_> {
        self.build_container(AxisKind::Col, label, Some(id), true, f)
    }

    /// Horizontal flex row container.
    pub fn row<F: FnOnce(&mut UiContext)>(&mut self, f: F) -> StyleBuilder<'_> {
        self.build_container(AxisKind::Row, "row", None, false, f)
    }

    /// Wrapping horizontal flex container that flows children onto new lines when exceeding width.
    pub fn row_wrap<F: FnOnce(&mut UiContext)>(&mut self, f: F) -> StyleBuilder<'_> {
        let b = self.build_container(AxisKind::Row, "row_wrap", None, false, f);
        b.ui.nodes[b.node_idx].style.flex_wrap = true;
        b
    }

    /// Vertical flex column container.
    pub fn col<F: FnOnce(&mut UiContext)>(&mut self, f: F) -> StyleBuilder<'_> {
        self.build_container(AxisKind::Col, "col", None, false, f)
    }

    /// Layered Z-axis stack container.
    ///
    /// Children align to top-left (`Justify::Start`, `Align::Start`) unless `.center()`,
    /// `.pos(...)`, or `.offset(...)` is applied; container children without explicit size or
    /// position stretch to fill the stack.
    pub fn zstack<F: FnOnce(&mut UiContext)>(&mut self, f: F) -> StyleBuilder<'_> {
        self.build_container(AxisKind::ZStack, "zstack", None, false, f)
    }

    /// Deferred global top-layer overlay drawn outside parent scroll clips and hit-tested first.
    pub fn overlay<F: FnOnce(&mut UiContext)>(&mut self, f: F) -> StyleBuilder<'_> {
        self.build_container(AxisKind::Col, "overlay", None, true, f)
    }

    /// Spring-damped vertically scrollable clipped container with interactive draggable scrollbar.
    pub fn scroll<F: FnOnce(&mut UiContext)>(&mut self, id_label: &str, f: F) -> StyleBuilder<'_> {
        let id = self.ids.id(id_label);
        let sb_id = scrollbar_widget_id(id);
        let prev_rect = self.prev_rects.get(&id).copied();
        let prev_sb = self.prev_scrollbar_rects.get(&id).copied();
        let scroll_dy = self.frame_scroll_delta[1];
        let pointer_in_scroll = self
            .pointer_pos
            .zip(prev_rect)
            .is_some_and(|([px, py], r)| r.contains(px, py));
        let pointer_blocked_by_overlay = self.pointer_pos.is_some_and(|[px, py]| {
            self.hits
                .iter()
                .any(|h| h.is_overlay && h.enabled && h.bounds.contains(px, py))
        });

        let just_pressed_sb = self.just_pressed_id == Some(sb_id);
        let pressing_sb = self.pressed_id == Some(sb_id);
        let pointer_pos = self.pointer_pos;
        let state = self.scroll_states.entry(id).or_default();

        if just_pressed_sb {
            if let (Some([_, py]), Some((_, thumb))) = (pointer_pos, prev_sb) {
                if py >= thumb.y && py <= thumb.y + thumb.h {
                    state.scrollbar_drag_offset = Some(py - thumb.y);
                } else {
                    state.scrollbar_drag_offset = Some(thumb.h * 0.5);
                }
            }
        }
        if pressing_sb {
            if let (Some([_, py]), Some((track, thumb))) = (pointer_pos, prev_sb) {
                let grab = state.scrollbar_drag_offset.unwrap_or(thumb.h * 0.5);
                let avail = (track.h - thumb.h).max(1.0);
                let t = ((py - grab - track.y) / avail).clamp(0.0, 1.0);
                state.target_y = t * state.max_y;
                state.spring.target = state.target_y;
                state.spring.value = state.target_y;
                state.spring.velocity = 0.0;
            }
        } else {
            state.scrollbar_drag_offset = None;
            if pointer_in_scroll && !pointer_blocked_by_overlay && scroll_dy != 0.0 {
                state.target_y = (state.target_y - scroll_dy * 32.0).clamp(0.0, state.max_y);
                state.spring.target = state.target_y;
            }
        }
        let offset_y = state.spring.value;

        let builder = self.build_container(AxisKind::Col, id_label, Some(id), false, f);
        let node = &mut builder.ui.nodes[builder.node_idx];
        node.is_scroll = true;
        node.scroll_offset_y = offset_y;
        builder
    }

    /// Horizontally scrollable `Row` container with smooth spring scrolling, clipping, and draggable scrollbar.
    pub fn hscroll(&mut self, id_label: &str, f: impl FnOnce(&mut UiContext)) -> StyleBuilder<'_> {
        let id = self.ids.id(id_label);
        let sb_id = scrollbar_widget_id(id);
        let prev_rect = self.prev_rects.get(&id).copied();
        let prev_sb = self.prev_scrollbar_rects.get(&id).copied();
        let scroll_dx = self.frame_scroll_delta[0];
        let scroll_dy = self.frame_scroll_delta[1];
        let pointer_in_scroll = self
            .pointer_pos
            .zip(prev_rect)
            .is_some_and(|([px, py], r)| r.contains(px, py));
        let pointer_blocked_by_overlay = self.pointer_pos.is_some_and(|[px, py]| {
            self.hits
                .iter()
                .any(|h| h.is_overlay && h.enabled && h.bounds.contains(px, py))
        });

        let delta = if scroll_dx != 0.0 {
            scroll_dx
        } else {
            scroll_dy
        };
        let just_pressed_sb = self.just_pressed_id == Some(sb_id);
        let pressing_sb = self.pressed_id == Some(sb_id);
        let pointer_pos = self.pointer_pos;
        let state = self.scroll_states.entry(id).or_default();

        if just_pressed_sb {
            if let (Some([px, _]), Some((_, thumb))) = (pointer_pos, prev_sb) {
                if px >= thumb.x && px <= thumb.x + thumb.w {
                    state.scrollbar_drag_offset = Some(px - thumb.x);
                } else {
                    state.scrollbar_drag_offset = Some(thumb.w * 0.5);
                }
            }
        }
        if pressing_sb {
            if let (Some([px, _]), Some((track, thumb))) = (pointer_pos, prev_sb) {
                let grab = state.scrollbar_drag_offset.unwrap_or(thumb.w * 0.5);
                let avail = (track.w - thumb.w).max(1.0);
                let t = ((px - grab - track.x) / avail).clamp(0.0, 1.0);
                state.target_x = t * state.max_x;
                state.spring_x.target = state.target_x;
                state.spring_x.value = state.target_x;
                state.spring_x.velocity = 0.0;
            }
        } else {
            state.scrollbar_drag_offset = None;
            if pointer_in_scroll && !pointer_blocked_by_overlay && delta != 0.0 {
                state.target_x = (state.target_x - delta * 32.0).clamp(0.0, state.max_x);
                state.spring_x.target = state.target_x;
            }
        }
        let offset_x = state.spring_x.value;

        let builder = self.build_container(AxisKind::Row, id_label, Some(id), false, f);
        let node = &mut builder.ui.nodes[builder.node_idx];
        node.is_scroll = true;
        node.scroll_offset_y = offset_x;
        builder
    }

    /// Returns the previous frame's scrollbar track `Rect` for `id_label`, if visible.
    pub fn scrollbar_rect(&self, id_label: &str) -> Option<Rect> {
        let id = self.ids.id(id_label);
        self.prev_scrollbar_rects.get(&id).map(|(track, _)| *track)
    }

    /// Returns the current vertical scroll target offset (in logical pixels) for `id_label`.
    pub fn scroll_offset(&self, id_label: &str) -> f32 {
        let id = self.ids.id(id_label);
        self.scroll_states.get(&id).map_or(0.0, |s| s.target_y)
    }

    /// Returns the maximum vertical scroll offset (in logical pixels) for `id_label`.
    pub fn scroll_max_y(&self, id_label: &str) -> f32 {
        let id = self.ids.id(id_label);
        self.scroll_states.get(&id).map_or(0.0, |s| s.max_y)
    }

    /// Programmatically scrolls `id_label` to vertical offset `y` (clamped to `[0, max_y]`).
    pub fn scroll_to(&mut self, id_label: &str, y: f32) {
        let id = self.ids.id(id_label);
        let sc = self.scroll_states.entry(id).or_default();
        sc.target_y = y.clamp(0.0, sc.max_y.max(y).max(0.0));
        sc.spring.target = sc.target_y;
        sc.spring.value = sc.target_y;
        sc.spring.velocity = 0.0;
        self.dirty = true;
    }

    /// Programmatically scrolls `id_label` to the bottom on this frame (even on first layout).
    pub fn scroll_to_bottom(&mut self, id_label: &str) {
        let id = self.ids.id(id_label);
        self.pending_scroll_to_bottom.insert(id);
        let sc = self.scroll_states.entry(id).or_default();
        sc.target_y = sc.max_y;
        sc.spring.target = sc.max_y;
        sc.spring.value = sc.max_y;
        sc.spring.velocity = 0.0;
        self.dirty = true;
    }

    /// Returns the current horizontal scroll target offset (in logical pixels) for `id_label`.
    pub fn scroll_offset_x(&self, id_label: &str) -> f32 {
        let id = self.ids.id(id_label);
        self.scroll_states.get(&id).map_or(0.0, |s| s.target_x)
    }

    /// Returns the maximum horizontal scroll offset (in logical pixels) for `id_label`.
    pub fn scroll_max_x(&self, id_label: &str) -> f32 {
        let id = self.ids.id(id_label);
        self.scroll_states.get(&id).map_or(0.0, |s| s.max_x)
    }

    /// Returns the pointer cursor icon requested by the currently pressed or hovered node.
    pub fn active_cursor(&self) -> CursorIcon {
        let target = self.pressed_id.or(self.hovered_id);
        let Some(tid) = target else {
            return CursorIcon::Default;
        };
        self.hits
            .iter()
            .rev()
            .find(|h| h.id == tid)
            .and_then(|h| h.cursor)
            .unwrap_or(CursorIcon::Default)
    }

    /// Returns `true` when the viewport logical width is below `640.0` points.
    pub fn is_mobile(&self) -> bool {
        self.viewport_w < 640.0
    }

    /// Selects a responsive value by viewport width (`< 640 => sm`, `< 1024 => md`, else `lg`).
    pub fn bp<T>(&self, sm: T, md: T, lg: T) -> T {
        if self.viewport_w < 640.0 {
            sm
        } else if self.viewport_w < 1024.0 {
            md
        } else {
            lg
        }
    }

    /// Responsive fixed-column CSS-style grid container (`cols >= 1`).
    pub fn grid<F: FnOnce(&mut UiContext)>(&mut self, cols: usize, f: F) -> StyleBuilder<'_> {
        let b = self.build_container(AxisKind::Grid, "grid", None, false, f);
        b.ui.nodes[b.node_idx].style.grid_cols = Some(cols.max(1));
        b
    }

    /// Auto-fit responsive grid container computing column count from `min_col_w`.
    pub fn grid_auto<F: FnOnce(&mut UiContext)>(
        &mut self,
        min_col_w: f32,
        f: F,
    ) -> StyleBuilder<'_> {
        let b = self.build_container(AxisKind::Grid, "grid_auto", None, false, f);
        b.ui.nodes[b.node_idx].style.grid_min_col_w = Some(min_col_w.max(24.0));
        b
    }

    /// Programmatically scrolls `id_label` smoothly using the spring animation to `y`.
    pub fn scroll_to_smooth(&mut self, id_label: &str, y: f32) {
        let id = self.ids.id(id_label);
        let sc = self.scroll_states.entry(id).or_default();
        sc.target_y = y.clamp(0.0, sc.max_y.max(y).max(0.0));
        sc.spring.target = sc.target_y;
        self.dirty = true;
    }

    /// Returns `true` if `id_label` was visible inside the viewport and any enclosing scroll/clip rect on the last frame.
    pub fn in_view(&self, id_label: &str) -> bool {
        let id = self.ids.id(id_label);
        self.prev_in_view.contains(&id)
    }

    /// Returns `true` if `id` was visible inside the viewport and any enclosing scroll/clip rect on the last frame.
    pub fn in_view_id(&self, id: WidgetId) -> bool {
        self.prev_in_view.contains(&id)
    }

    /// Returns normalized vertical scroll progress (`0.0..=1.0`) for `id_label`.
    pub fn scroll_progress(&self, id_label: &str) -> f32 {
        let id = self.ids.id(id_label);
        self.scroll_states.get(&id).map_or(0.0, |s| {
            if s.max_y > 0.5 {
                (s.spring.value / s.max_y).clamp(0.0, 1.0)
            } else {
                0.0
            }
        })
    }
    /// Programmatically scrolls `id_label` horizontally to offset `x` (clamped to `[0, max_x]`).
    pub fn scroll_to_x(&mut self, id_label: &str, x: f32) {
        let id = self.ids.id(id_label);
        let sc = self.scroll_states.entry(id).or_default();
        sc.target_x = x.clamp(0.0, sc.max_x.max(x).max(0.0));
        sc.spring_x.target = sc.target_x;
        sc.spring_x.value = sc.target_x;
        sc.spring_x.velocity = 0.0;
        self.dirty = true;
    }

    #[allow(clippy::too_many_arguments)]
    fn csd_control_btn(
        &mut self,
        id_label: &str,
        semantic_label: &str,
        size: (f32, f32),
        radius: f32,
        bg: Color,
        hover_bg: Color,
        draw_icon: impl FnOnce(&mut Canvas2D, bool, f32, f32),
    ) -> bool {
        let id = self.ids.id(id_label);
        let _ = self.ids.register_interactive(id);
        let hovered = self.hovered_id == Some(id);
        let clicked = self.clicked_id == Some(id) || self.activated_id == Some(id);
        let bw = size.0.max(20.0);
        let bh = size.1.max(18.0);
        let mut canvas = Canvas2D::new([0.0, 0.0]);
        draw_icon(&mut canvas, hovered, bw, bh);
        self.alloc_node(
            LayoutNode {
                id,
                role: Role::Button,
                kind: AxisKind::Leaf,
                style: Style {
                    width: Some(bw),
                    height: Some(bh),
                    bg: Some(if hovered { hover_bg } else { bg }),
                    radius,
                    ..Style::default()
                },
                label: String::from(semantic_label),
                semantic_value: None,
                text: None,
                rich_spans: None,
                rich_links: Vec::new(),
                sparkline_samples: None,
                image_tex: None,
                viewport_data: Some((
                    canvas.cmds,
                    SceneSnapshot {
                        camera: Camera3D::default(),
                        draws: Vec::new(),
                        msaa_samples: 4,
                    },
                )),
                is_scroll: false,
                scroll_offset_y: 0.0,
                focusable: true,
                hit_testable: true,
                enabled: true,
                is_overlay: false,
                children: Vec::new(),
                intrinsic_w: bw,
                intrinsic_h: bh,
                content_w: bw,
                content_h: bh,
                rect: Rect::ZERO,
            },
            true,
        );
        clicked
    }

    /// Sets the window icon texture rendered in `csd_titlebar`.
    pub fn set_window_icon(&mut self, tex: Option<TextureId>) {
        if self.window_icon_tex != tex {
            self.window_icon_tex = tex;
            self.dirty = true;
        }
    }

    /// Renders a custom SDF client-side titlebar (`CSD`) with drag bar and Minimize/Maximize/Close buttons.
    pub fn csd_titlebar(&mut self, title: &str) -> CsdAction {
        self.csd_titlebar_with(title, 28.0, |_| {})
    }

    /// Renders a customizable client-side titlebar (`CSD`) with configurable drag bar height and custom center slot.
    pub fn csd_titlebar_with(
        &mut self,
        title: &str,
        bar_h: f32,
        custom_center: impl FnOnce(&mut UiContext),
    ) -> CsdAction {
        self.csd_titlebar_styled(title, bar_h, CsdStyle::default(), custom_center)
    }

    /// Renders a styled client-side titlebar (`CSD`) with configurable [`CsdStyle`], drag bar height, and custom center slot.
    pub fn csd_titlebar_styled(
        &mut self,
        title: &str,
        bar_h: f32,
        style: CsdStyle,
        custom_center: impl FnOnce(&mut UiContext),
    ) -> CsdAction {
        let bar_h = bar_h.max(20.0);
        self.has_csd_titlebar = true;
        let mut action = CsdAction::None;
        let drag_id = self.ids.id("__csd_drag__");
        let _ = self.ids.register_interactive(drag_id);
        let drag_pressed = self.just_pressed_id == Some(drag_id);
        let drag_double_clicked = self.double_clicked_id == Some(drag_id);
        let surface_col = style.bg.unwrap_or(self.theme.surface);
        let surface_alt = self.theme.surface_alt;
        let text_col = self.theme.text;
        let border_col = self.theme.border;
        let icon_tex = self.window_icon_tex;

        let ctrl_bg = style.control_bg.unwrap_or(if style.ghost_controls {
            Color::TRANSPARENT
        } else {
            surface_alt
        });
        let ctrl_hover_bg = style.control_hover_bg.unwrap_or(if style.ghost_controls {
            Color::rgba(text_col.r, text_col.g, text_col.b, 20)
        } else {
            border_col
        });
        let ctrl_fg = style.control_fg.unwrap_or(text_col);

        let close_bg = style.close_bg.unwrap_or(if style.ghost_controls {
            Color::TRANSPARENT
        } else {
            Color::from_hex(0x9E2A2B)
        });
        let close_hover_bg = style.close_hover_bg.unwrap_or(if style.ghost_controls {
            Color::from_hex(0xC42B1C)
        } else {
            Color::from_hex(0xC63B3C)
        });

        let row_b = self
            .row(|ui| {
                if let Some(tex) = icon_tex {
                    ui.img(tex).w(18.0).h(18.0).fit(ImageFit::Contain);
                }
                if !title.is_empty() {
                    ui.txt(title).sz(13.0).fg(text_col);
                }
                custom_center(ui);

                ui.alloc_node(
                    LayoutNode {
                        id: drag_id,
                        role: Role::Container,
                        kind: AxisKind::Leaf,
                        style: Style {
                            grow: 1.0,
                            height: Some(bar_h),
                            ..Style::default()
                        },
                        label: String::from("TitlebarDrag"),
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
                        is_overlay: false,
                        children: Vec::new(),
                        intrinsic_w: 24.0,
                        intrinsic_h: bar_h,
                        content_w: 24.0,
                        content_h: bar_h,
                        rect: Rect::ZERO,
                    },
                    true,
                );
                if drag_double_clicked {
                    action = CsdAction::ToggleMaximize;
                } else if drag_pressed {
                    action = CsdAction::StartDrag;
                }

                if ui.csd_control_btn(
                    "__csd_min__",
                    "Minimize",
                    style.control_size,
                    style.control_radius,
                    ctrl_bg,
                    ctrl_hover_bg,
                    |c, _, bw, bh| {
                        let cx = bw * 0.5;
                        let cy = bh * 0.5;
                        c.line([cx - 5.0, cy], [cx + 5.0, cy], 1.4, ctrl_fg);
                    },
                ) {
                    action = CsdAction::Minimize;
                }

                if ui.csd_control_btn(
                    "__csd_max__",
                    "Maximize",
                    style.control_size,
                    style.control_radius,
                    ctrl_bg,
                    ctrl_hover_bg,
                    |c, _, bw, bh| {
                        let cx = bw * 0.5;
                        let cy = bh * 0.5;
                        c.poly(
                            &[
                                [cx - 4.5, cy - 4.5],
                                [cx + 4.5, cy - 4.5],
                                [cx + 4.5, cy + 4.5],
                                [cx - 4.5, cy + 4.5],
                            ],
                            true,
                            1.35,
                            ctrl_fg,
                        );
                    },
                ) {
                    action = CsdAction::ToggleMaximize;
                }

                if ui.csd_control_btn(
                    "__csd_close__",
                    "Close",
                    style.control_size,
                    style.control_radius,
                    close_bg,
                    close_hover_bg,
                    |c, hovered, bw, bh| {
                        let cx = bw * 0.5;
                        let cy = bh * 0.5;
                        let fg = style
                            .close_fg
                            .unwrap_or(if style.ghost_controls && !hovered {
                                text_col
                            } else {
                                Color::WHITE
                            });
                        c.line([cx - 4.5, cy - 4.5], [cx + 4.5, cy + 4.5], 1.5, fg);
                        c.line([cx + 4.5, cy - 4.5], [cx - 4.5, cy + 4.5], 1.5, fg);
                    },
                ) {
                    action = CsdAction::Close;
                }
            })
            .px(style.padding.0)
            .py(style.padding.1)
            .gap(style.gap)
            .bg(surface_col);

        if let Some((bw, bcol)) = style.border {
            if bw > 0.0 {
                row_b.bd(bw, bcol);
            }
        } else {
            row_b.bd(1.0, border_col);
        }

        action
    }

    /// Renders a static text label.
    ///
    /// # Applicable Modifiers
    /// - Typography: `.sz(pt)`, `.fg(color)`, `.bold()`, `.semibold()`, `.medium()`, `.weight(u16)`, `.mono()`, `.font(family)`, `.tracking(em)`, `.line_height(mult)`, `.align(TextAlign)`
    /// - Wrapping & selection: `.wrap()`, `.ellipsis()`, `.selectable()`, `.selection_bg(color)`
    /// - Box model: `.w()`, `.h()`, `.max_w()`, `.p()`, `.m()`, `.bg()`, `.r()`, `.opacity()`
    pub fn txt(&mut self, text: &str) -> StyleBuilder<'_> {
        let ordinal = self
            .container_stack
            .last()
            .map_or(0, |&p| self.nodes[p].children.len());
        self.ids.push_u64(ordinal as u64);
        let id = self.ids.id(text);
        self.ids.pop_id();

        let style = Style {
            fg: Some(self.theme.text),
            font_size: Some(self.theme.font_size),
            ..Style::default()
        };

        let idx = self.alloc_node(
            LayoutNode {
                id,
                role: Role::Label,
                kind: AxisKind::Leaf,
                style,
                label: String::from(text),
                semantic_value: None,
                text: Some(String::from(text)),
                rich_spans: None,
                rich_links: Vec::new(),
                sparkline_samples: None,
                image_tex: None,
                viewport_data: None,
                is_scroll: false,
                scroll_offset_y: 0.0,
                focusable: false,
                hit_testable: false,
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

        StyleBuilder {
            ui: self,
            node_idx: idx,
            changed: false,
        }
    }

    /// Renders inline rich text supporting bold, italic, code pills, `@mention` pills, and clickable links.
    ///
    /// Wrapping is off by default; chain `.wrap()` (and `.max_w(...)` inside unbounded parents)
    /// to wrap within a grid column or container width.
    pub fn rich<'a>(&mut self, spans: &[Span<'a>]) -> RichResponse<'_> {
        let owned: Vec<OwnedSpan> = spans.iter().map(OwnedSpan::from).collect();
        self.rich_owned(owned)
    }

    pub(crate) fn rich_owned(&mut self, owned: Vec<OwnedSpan>) -> RichResponse<'_> {
        let ordinal = self
            .container_stack
            .last()
            .map_or(0, |&p| self.nodes[p].children.len());
        self.ids.push_u64(ordinal as u64);
        let id = self.ids.id("rich");

        let mut links = Vec::new();
        let mut clicked_link: Option<String> = None;
        let mut plain_summary = String::new();

        for os in &owned {
            plain_summary.push_str(&os.text);
            if os.kind == SpanKind::Link {
                if let Some(ref link_id) = os.link_id {
                    let link_wid = self.ids.id(link_id);
                    let _ = self.ids.register_interactive(link_wid);
                    if self.clicked_id == Some(link_wid) || self.activated_id == Some(link_wid) {
                        clicked_link = Some(link_id.clone());
                    }
                    links.push((link_wid, link_id.clone()));
                }
            }
        }
        self.ids.pop_id();

        let style = Style {
            fg: Some(self.theme.text),
            font_size: Some(self.theme.font_size),
            ..Style::default()
        };

        let idx = self.alloc_node(
            LayoutNode {
                id,
                role: Role::Label,
                kind: AxisKind::Leaf,
                style,
                label: plain_summary,
                semantic_value: None,
                text: None,
                rich_spans: Some(owned),
                rich_links: links,
                sparkline_samples: None,
                image_tex: None,
                viewport_data: None,
                is_scroll: false,
                scroll_offset_y: 0.0,
                focusable: false,
                hit_testable: false,
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

        RichResponse {
            ui: self,
            node_idx: idx,
            clicked_link,
        }
    }

    /// Renders an interactive button with theme defaults and deterministic scoped ID.
    pub fn btn(&mut self, label: &str) -> ButtonResponse<'_> {
        let id = self.ids.id(label);
        let _ = self.ids.register_interactive(id);

        let disabled_prev = self.prev_disabled_ids.contains(&id);
        let hovered = !disabled_prev && self.hovered_id == Some(id);
        let focused = !disabled_prev && self.focused_id == Some(id);
        let clicked =
            !disabled_prev && (self.clicked_id == Some(id) || self.activated_id == Some(id));

        let style = Style {
            pad_l: 12.0,
            pad_r: 12.0,
            pad_t: 8.0,
            pad_b: 8.0,
            bg: Some(self.theme.primary),
            fg: Some(self.theme.on_primary),
            hover_bg: Some(self.theme.surface_alt),
            hover_fg: Some(self.theme.text),
            radius: self.theme.radius,
            font_size: Some(self.theme.font_size),
            ..Style::default()
        };

        let mut node = LayoutNode::new_leaf(id, Role::Button, String::from(label));
        node.style = style;
        node.text = Some(String::from(label));
        node.focusable = true;
        node.hit_testable = true;
        let idx = self.alloc_node(node, true);

        ButtonResponse {
            ui: self,
            node_idx: idx,
            clicked,
            hovered,
            focused,
        }
    }

    /// Interactive custom-content button container (`Role::Button`) composing arbitrary child widgets.
    pub fn btn_with<F: FnOnce(&mut UiContext)>(
        &mut self,
        id_label: &str,
        f: F,
    ) -> ButtonResponse<'_> {
        let id = self.ids.id(id_label);
        let _ = self.ids.register_interactive(id);

        let disabled_prev = self.prev_disabled_ids.contains(&id);
        let hovered = !disabled_prev && self.hovered_id == Some(id);
        let focused = !disabled_prev && self.focused_id == Some(id);
        let clicked =
            !disabled_prev && (self.clicked_id == Some(id) || self.activated_id == Some(id));

        let bg = self.theme.surface;
        let hover_bg = self.theme.surface_alt;
        let fg = self.theme.text;
        let radius = self.theme.radius;
        let font_size = self.theme.font_size;

        let builder = self.container_with_id(AxisKind::Row, id_label, id, f);
        let idx = builder.node_idx;
        let node = &mut builder.ui.nodes[idx];
        node.role = Role::Button;
        node.focusable = true;
        node.hit_testable = true;
        node.style.pad_l = 10.0;
        node.style.pad_r = 10.0;
        node.style.pad_t = 6.0;
        node.style.pad_b = 6.0;
        node.style.gap = 8.0;
        node.style.bg = Some(bg);
        node.style.hover_bg = Some(hover_bg);
        node.style.fg = Some(fg);
        node.style.radius = radius;
        node.style.font_size = Some(font_size);

        ButtonResponse {
            ui: builder.ui,
            node_idx: idx,
            clicked,
            hovered,
            focused,
        }
    }

    /// Horizontal 1px separator line themed with `theme.border` (stretches across parent column width).
    pub fn divider(&mut self) -> StyleBuilder<'_> {
        let ordinal = self
            .container_stack
            .last()
            .map_or(0, |&p| self.nodes[p].children.len());
        self.ids.push_u64(ordinal as u64);
        let id = self.ids.id("__divider__");
        self.ids.pop_id();

        let mut node = LayoutNode::new_leaf(id, Role::Divider, String::from("Divider"));
        node.style.height = Some(1.0);
        node.style.bg = Some(self.theme.border);
        let idx = self.alloc_node(node, true);
        StyleBuilder {
            ui: self,
            node_idx: idx,
            changed: false,
        }
    }

    /// Vertical 1px separator line themed with `theme.border` (stretches across parent row height).
    pub fn vdivider(&mut self) -> StyleBuilder<'_> {
        let ordinal = self
            .container_stack
            .last()
            .map_or(0, |&p| self.nodes[p].children.len());
        self.ids.push_u64(ordinal as u64);
        let id = self.ids.id("__vdivider__");
        self.ids.pop_id();

        let mut node = LayoutNode::new_leaf(id, Role::Divider, String::from("VDivider"));
        node.style.width = Some(1.0);
        node.style.bg = Some(self.theme.border);
        let idx = self.alloc_node(node, true);
        StyleBuilder {
            ui: self,
            node_idx: idx,
            changed: false,
        }
    }

    /// Inline GPU texture widget participating in flex layout (`Row`, `Col`, `ZStack`).
    ///
    /// Supports `.w()`, `.h()`, `.r()`, `.circle()`, `.squircle()`, `.bd()`, `.bg()`, `.fit()`,
    /// `.filter()`, `.tint()`, and `.tex_size()`.
    pub fn img(&mut self, tex: TextureId) -> StyleBuilder<'_> {
        let ordinal = self
            .container_stack
            .last()
            .map_or(0, |&p| self.nodes[p].children.len());
        self.ids.push_u64(ordinal as u64);
        self.ids.push_u64(tex.0);
        let id = self.ids.id("__img__");
        self.ids.pop_id();
        self.ids.pop_id();

        let mut node = LayoutNode::new_leaf(id, Role::Image, String::from("Image"));
        node.image_tex = Some(tex);
        node.style.image_size = self.texture_sizes.get(&tex).copied();
        let idx = self.alloc_node(node, true);
        StyleBuilder {
            ui: self,
            node_idx: idx,
            changed: false,
        }
    }

    /// Alias for [`Self::img`].
    pub fn image(&mut self, tex: TextureId) -> StyleBuilder<'_> {
        self.img(tex)
    }
}

#[cfg(test)]
mod tests {
    use super::UiContext;
    use crate::sys::id::WidgetId;
    use crate::ui::draw::{DrawCmd, Role};
    use crate::ui::types::{Color, Key, Modifiers, PointerButton, Rect, Span};

    #[test]
    fn two_pass_flex_layout_and_pixel_snapping() {
        let mut ui = UiContext::new();
        ui.begin_frame(400.0, 200.0, 1.0);
        ui.col(|ui| {
            ui.row(|ui| {
                ui.btn("A").grow(1.0).h(40.0);
                ui.btn("B").grow(2.0).h(40.0);
            })
            .w(300.0)
            .h(50.0)
            .p(10.0)
            .gap(10.0);

            ui.zstack(|ui| {
                ui.txt("Back").w(80.0).h(20.0);
                ui.txt("Front").w(120.0).h(30.0);
            })
            .p(5.0);
        })
        .p(10.0)
        .gap(5.0);
        ui.end_frame();

        let buttons: Vec<_> = ui
            .semantics()
            .iter()
            .filter(|n| n.role == Role::Button)
            .collect();
        assert_eq!(buttons.len(), 2);
        assert!((buttons[0].bounds.x - 20.0).abs() < 0.01);
        assert!((buttons[0].bounds.w - 90.0).abs() < 0.01);
        assert!((buttons[1].bounds.x - 120.0).abs() < 0.01);
        assert!((buttons[1].bounds.w - 180.0).abs() < 0.01);

        let labels: Vec<_> = ui
            .semantics()
            .iter()
            .filter(|n| n.role == Role::Label)
            .collect();
        assert_eq!(labels.len(), 2);
        assert!((labels[0].bounds.x - labels[1].bounds.x).abs() < 0.01);
        assert!((labels[0].bounds.y - labels[1].bounds.y).abs() < 0.01);

        for scale in [1.0_f32, 1.25_f32, 1.5_f32, 2.0_f32] {
            let mut dpi_ui = UiContext::new();
            dpi_ui.begin_frame(200.0, 200.0, scale);
            dpi_ui.btn("Snap").px(10.3).py(5.3).w(20.3).h(15.3);
            dpi_ui.end_frame();
            let b = dpi_ui.semantics()[0].bounds;
            for edge in [b.x, b.y, b.x + b.w, b.y + b.h] {
                let phys = edge * scale;
                assert!((phys - phys.round()).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn overlay_draws_last_and_captures_pointer_first() {
        let mut ui = UiContext::new();
        let mut base_clicked = false;
        let mut overlay_clicked = false;

        let run_frame = |ui: &mut UiContext, base_flag: &mut bool, ov_flag: &mut bool| {
            ui.begin_frame(400.0, 300.0, 1.0);
            if ui
                .btn("Base")
                .w(120.0)
                .h(40.0)
                .bg(Color::from_hex(0x112233))
                .clicked()
            {
                *base_flag = true;
            }
            ui.overlay(|ui| {
                if ui
                    .btn("Menu")
                    .w(120.0)
                    .h(40.0)
                    .bg(Color::from_hex(0x445566))
                    .clicked()
                {
                    *ov_flag = true;
                }
            });
            ui.end_frame();
        };

        run_frame(&mut ui, &mut base_clicked, &mut overlay_clicked);
        assert_eq!(ui.draw_list().base_cmds.len(), 2);
        assert_eq!(ui.draw_list().overlay_cmds.len(), 2);

        let last_cmd = ui.draw_list().all_commands().last().expect("has commands");
        match last_cmd {
            DrawCmd::Text { text, .. } => assert_eq!(text, "Menu"),
            other => panic!("expected overlay text last, got {other:?}"),
        }

        ui.on_pointer_move(20.0, 20.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);

        run_frame(&mut ui, &mut base_clicked, &mut overlay_clicked);
        assert!(!base_clicked);
        assert!(overlay_clicked);
    }

    #[test]
    fn keyboard_focus_cycle_activation_and_escape() {
        let mut ui = UiContext::new();
        let mut clicks = [false; 3];

        let run_frame = |ui: &mut UiContext, clicks: &mut [bool; 3]| {
            ui.begin_frame(400.0, 300.0, 1.0);
            ui.col(|ui| {
                if ui.btn("One").clicked() {
                    clicks[0] = true;
                }
                ui.txt("Not Focusable");
                if ui.btn("Two").clicked() {
                    clicks[1] = true;
                }
                if ui.btn("Three").clicked() {
                    clicks[2] = true;
                }
            });
            ui.end_frame();
        };

        run_frame(&mut ui, &mut clicks);

        let none = Modifiers::default();
        let shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };

        ui.on_key_down(Key::Tab, none);
        ui.on_key_down(Key::Tab, none);
        ui.on_key_down(Key::Enter, none);
        run_frame(&mut ui, &mut clicks);
        assert_eq!(clicks, [false, true, false]);

        clicks = [false; 3];
        ui.on_key_down(Key::Tab, shift);
        ui.on_key_down(Key::Space, none);
        run_frame(&mut ui, &mut clicks);
        assert_eq!(clicks, [true, false, false]);

        ui.on_key_down(Key::Escape, none);
        assert!(ui.dismiss_requested());
        run_frame(&mut ui, &mut clicks);
        assert!(!ui.dismiss_requested());
    }

    #[test]
    fn cursor_move_skips_redraw_unless_hover_changes_or_glow_active() {
        let mut ui = UiContext::new();
        ui.begin_frame(400.0, 300.0, 1.0);
        ui.btn("Btn").w(100.0).h(40.0);
        ui.end_frame();
        assert!(ui.take_dirty());

        assert!(!ui.on_pointer_move(300.0, 200.0));
        assert!(!ui.take_dirty());

        assert!(ui.on_pointer_move(20.0, 20.0));
        assert!(ui.take_dirty());

        assert!(!ui.on_pointer_move(25.0, 25.0));
        assert!(!ui.take_dirty());
    }

    #[test]
    fn rich_text_link_can_be_focused_and_clicked() {
        let mut ui = UiContext::new();
        let mut opened: Option<String> = None;

        let run_frame = |ui: &mut UiContext, opened: &mut Option<String>| {
            ui.begin_frame(400.0, 120.0, 1.0);
            let r = ui.rich(&[
                Span::text("Hi "),
                Span::mention("@team"),
                Span::text(" check "),
                Span::code("v1.0"),
                Span::text(" "),
                Span::link("docs", "open_docs"),
            ]);
            if let Some(id) = r.clicked_link() {
                *opened = Some(String::from(id));
            }
            ui.end_frame();
        };

        run_frame(&mut ui, &mut opened);
        ui.on_key_down(Key::Tab, Modifiers::default());
        ui.on_key_down(Key::Enter, Modifiers::default());
        run_frame(&mut ui, &mut opened);
        assert_eq!(opened.as_deref(), Some("open_docs"));
    }

    #[test]
    fn interact_scroll_and_flip_anim_layout() {
        let mut ui = UiContext::new();
        let mut got_double = false;
        let mut got_right = false;
        let mut got_drag_dx = 0.0_f32;
        let gid = WidgetId(42);

        let run = |ui: &mut UiContext, d: &mut bool, r: &mut bool, dx: &mut f32| {
            ui.begin_frame(400.0, 300.0, 1.0);
            let g = ui.interact(gid, Rect::new(10.0, 10.0, 80.0, 40.0));
            if g.double_clicked {
                *d = true;
            }
            if g.right_clicked {
                *r = true;
            }
            if g.dragging {
                *dx += g.delta[0];
            }
            ui.end_frame();
        };

        run(&mut ui, &mut got_double, &mut got_right, &mut got_drag_dx);

        // Two fast left clicks -> double click
        ui.on_pointer_move(20.0, 20.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        run(&mut ui, &mut got_double, &mut got_right, &mut got_drag_dx);
        ui.step_physics(0.05);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        run(&mut ui, &mut got_double, &mut got_right, &mut got_drag_dx);
        assert!(got_double);

        // Right click
        ui.on_pointer_down(PointerButton::Right);
        ui.on_pointer_up(PointerButton::Right);
        run(&mut ui, &mut got_double, &mut got_right, &mut got_drag_dx);
        assert!(got_right);

        // Drag delta
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_move(35.0, 20.0);
        run(&mut ui, &mut got_double, &mut got_right, &mut got_drag_dx);
        assert!((got_drag_dx - 15.0).abs() < 0.01);
        ui.on_pointer_up(PointerButton::Left);

        // FLIP anim_layout smoothly interpolates position change and settles
        for _ in 0..20 {
            ui.step_physics(0.05);
        }
        let flip_id = WidgetId(99);
        ui.begin_frame(400.0, 300.0, 1.0);
        ui.txt("Tab").w(60.0).h(24.0).anim_layout(flip_id);
        ui.end_frame();
        assert!(!ui.has_active_animations());

        ui.begin_frame(400.0, 300.0, 1.0);
        ui.col(|ui| {
            ui.txt("Spacer").h(80.0);
            ui.txt("Tab").w(120.0).h(24.0).anim_layout(flip_id);
        });
        ui.end_frame();
        assert!(ui.has_active_animations());
        for _ in 0..300 {
            ui.step_physics(0.016);
            if !ui.has_active_animations() {
                break;
            }
        }
        assert!(!ui.has_active_animations());
    }

    #[test]
    fn f12_toggles_telemetry_hud_and_updates_perf_stats() {
        let mut ui = UiContext::new();
        assert!(!ui.is_hud_visible());
        ui.on_key_down(Key::F12, Modifiers::default());
        assert!(ui.is_hud_visible());

        ui.step_physics(0.016);
        ui.begin_frame(800.0, 600.0, 1.0);
        ui.txt("Main Body");
        ui.end_frame();

        let stats = ui.perf_stats();
        assert!(stats.draw_calls >= 5);
        assert!(stats.node_count >= 5);
        assert!(stats.arena_high_water_bytes >= stats.arena_used_bytes);
        assert!(ui
            .semantics()
            .iter()
            .any(|n| n.is_overlay && n.label.contains("Telemetry")));

        ui.on_key_down(Key::F12, Modifiers::default());
        assert!(!ui.is_hud_visible());
    }

    #[test]
    fn wrapped_text_in_fixed_col_and_zstack_offset_layout() {
        use crate::ui::types::WindowConfig;

        let mut ui = UiContext::new();
        ui.begin_frame(800.0, 600.0, 1.0);
        ui.col(|ui| {
            ui.row(|ui| {
                ui.col(|ui| {
                    ui.txt("Wrapped paragraph: Lumenic lays out text across multiple lines inside bounded containers without overflowing.")
                        .sz(13.0)
                        .wrap();
                })
                .w(280.0)
                .p(8.0);
            });
            ui.txt("Next line");

            ui.zstack(|ui| {
                ui.col(|ui| {
                    ui.txt("Badge");
                })
                .w(24.0)
                .h(24.0)
                .offset(12.0, 64.0);
            })
            .w(360.0)
            .h(160.0)
            .p(4.0);
        })
        .p(12.0)
        .gap(8.0);
        ui.end_frame();

        let wrapped = ui
            .semantics()
            .iter()
            .find(|n| n.label.starts_with("Wrapped paragraph:"))
            .expect("wrapped label")
            .bounds;
        let next_line = ui
            .semantics()
            .iter()
            .find(|n| n.label == "Next line")
            .expect("next line label")
            .bounds;
        assert!(
            wrapped.h > 30.0,
            "expected wrapped multi-line height, got {wrapped:?}"
        );
        assert!(
            next_line.y >= wrapped.y + wrapped.h + 8.0,
            "next_line={next_line:?} overlapped wrapped={wrapped:?}"
        );

        let zstack_bounds = ui
            .semantics()
            .iter()
            .find(|n| n.label == "zstack")
            .expect("zstack")
            .bounds;
        let badge_bounds = ui
            .semantics()
            .iter()
            .find(|n| n.label == "Badge")
            .expect("Badge")
            .bounds;
        assert!((badge_bounds.x - (zstack_bounds.x + 4.0 + 12.0)).abs() < 0.01);
        assert!((badge_bounds.y - (zstack_bounds.y + 4.0 + 64.0)).abs() < 0.01);

        // Verify F1..F11, PageUp, PageDown, Insert keys and WindowConfig::icon_rgba / icon_png.
        ui.on_key_down(Key::F5, Modifiers::default());
        assert!(ui.shortcut_pressed(Key::F5));
        ui.on_key_down(Key::PageDown, Modifiers::default());
        assert!(ui.shortcut_pressed(Key::PageDown));
        ui.on_key_down(Key::Insert, Modifiers::default());
        assert!(ui.shortcut_pressed(Key::Insert));

        let rgba = vec![255u8; 4 * 4 * 4];
        let cfg = WindowConfig::new("IconApp").icon_rgba(4, 4, rgba.clone());
        assert_eq!(
            cfg.icon_rgba.as_ref().map(|(w, h, _)| (*w, *h)),
            Some((4, 4))
        );
        let png = crate::gfx::image::encode_png(4, 4, &rgba).expect("encode icon png");
        let cfg_png = WindowConfig::new("IconPng").icon_png(&png);
        assert_eq!(
            cfg_png.icon_rgba.as_ref().map(|(w, h, _)| (*w, *h)),
            Some((4, 4))
        );
    }

    #[test]
    fn img_btn_with_tooltip_divider_and_font_weight() {
        use crate::gfx::sdf::SdfBatch;
        use crate::gfx::text::GlyphAtlas;
        use crate::gfx::viewport::TextureFilter;
        use crate::ui::types::{ImageFit, RectShape, TextureId};

        let mut ui = UiContext::new();
        let mut atlas = GlyphAtlas::new_fallback_only();
        let mut channel_clicks = 0u32;
        let tex = TextureId(77);

        let run = |ui: &mut UiContext, atlas: &mut GlyphAtlas, clicks: &mut u32| {
            ui.begin_frame(500.0, 300.0, 1.0);
            ui.col(|ui| {
                ui.txt("Heading").sz(18.0).bold();
                ui.txt("Subheading").sz(14.0).semibold();
                ui.divider();
                ui.row(|ui| {
                    ui.img(tex)
                        .w(40.0)
                        .h(40.0)
                        .tex_size(80, 40)
                        .fit(ImageFit::Cover)
                        .filter(TextureFilter::Nearest)
                        .circle()
                        .bd(2.0, Color::WHITE);
                    ui.vdivider();
                    if ui
                        .btn_with("chan_general", |ui| {
                            ui.txt("#").bold();
                            ui.txt("general");
                        })
                        .tooltip("Open #general")
                        .clicked()
                    {
                        *clicks += 1;
                    }
                })
                .h(44.0)
                .gap(8.0);
            })
            .w(300.0)
            .p(10.0)
            .gap(8.0);
            ui.end_frame_with(atlas);
        };

        run(&mut ui, &mut atlas, &mut channel_clicks);

        // Font weights recorded on DrawCmd::Text
        assert!(ui.draw_list().base_cmds.iter().any(|c| matches!(
            c,
            DrawCmd::Text { text, weight: 700, .. } if text == "Heading"
        )));
        assert!(ui.draw_list().base_cmds.iter().any(|c| matches!(
            c,
            DrawCmd::Text { text, weight: 600, .. } if text == "Subheading"
        )));

        // Divider stretches horizontally to inner_w (280px), VDivider vertically to row inner_h (44px).
        let hdiv = ui
            .semantics()
            .iter()
            .find(|n| n.label == "Divider")
            .expect("hdivider")
            .bounds;
        assert!((hdiv.w - 280.0).abs() < 0.01 && (hdiv.h - 1.0).abs() < 0.01);
        let vdiv = ui
            .semantics()
            .iter()
            .find(|n| n.label == "VDivider")
            .expect("vdivider")
            .bounds;
        assert!((vdiv.w - 1.0).abs() < 0.01 && (vdiv.h - 44.0).abs() < 0.01);

        // Image DrawCmd::Image with Cover UV (80x40 into 40x40 -> u in 0.25..0.75) and Circle shape.
        let img_cmd = ui
            .draw_list()
            .base_cmds
            .iter()
            .find_map(|c| match c {
                DrawCmd::Image {
                    rect,
                    tex: t,
                    filter,
                    uv,
                    shape,
                    ..
                } => Some((*rect, *t, *filter, *uv, *shape)),
                _ => None,
            })
            .expect("image draw cmd");
        assert_eq!(img_cmd.1, tex);
        assert_eq!(img_cmd.2, TextureFilter::Nearest);
        assert_eq!(img_cmd.4, RectShape::Circle);
        assert!((img_cmd.3[0] - 0.25).abs() < 0.01 && (img_cmd.3[2] - 0.75).abs() < 0.01);

        // SdfBatch emits a custom_tex run for the image.
        let mut batch = SdfBatch::default();
        batch.build(ui.draw_list(), &mut atlas, 1.0, None, 500, 300);
        assert!(batch
            .runs
            .iter()
            .any(|r| r.custom_tex == Some((tex, TextureFilter::Nearest))));

        // btn_with is focusable and activates via Tab + Enter.
        let btn_node = ui
            .semantics()
            .iter()
            .find(|n| n.label == "chan_general" && n.role == Role::Button)
            .expect("btn_with semantic node")
            .clone();
        assert!(btn_node.focusable);
        ui.on_key_down(Key::Tab, Modifiers::default());
        ui.on_key_down(Key::Enter, Modifiers::default());
        run(&mut ui, &mut atlas, &mut channel_clicks);
        assert_eq!(channel_clicks, 1);

        // Hover btn_with: tooltip timer active before 0.4s, overlay emitted after >= 0.4s.
        ui.on_pointer_move(btn_node.bounds.x + 8.0, btn_node.bounds.y + 8.0);
        run(&mut ui, &mut atlas, &mut channel_clicks);
        assert!(ui.has_active_animations());
        assert!(!ui
            .semantics()
            .iter()
            .any(|n| n.is_overlay && n.label == "Open #general"));

        for _ in 0..10 {
            ui.step_physics(0.05);
        }
        run(&mut ui, &mut atlas, &mut channel_clicks);
        assert!(!ui.has_active_animations());
        assert!(ui
            .semantics()
            .iter()
            .any(|n| n.is_overlay && n.label == "Open #general"));

        // Moving pointer away clears the tooltip immediately.
        ui.on_pointer_move(490.0, 290.0);
        run(&mut ui, &mut atlas, &mut channel_clicks);
        assert!(!ui
            .semantics()
            .iter()
            .any(|n| n.is_overlay && n.label == "Open #general"));
    }

    #[test]
    fn interact_discards_idle_pre_press_pointer_delta() {
        let mut ui = UiContext::new();
        let gid = WidgetId(777);
        let mut deltas: Vec<[f32; 2]> = Vec::new();

        let run = |ui: &mut UiContext, deltas: &mut Vec<[f32; 2]>| {
            ui.begin_frame(400.0, 300.0, 1.0);
            let g = ui.interact(gid, Rect::new(10.0, 10.0, 120.0, 80.0));
            if g.dragging {
                deltas.push(g.delta);
            }
            ui.end_frame();
        };

        run(&mut ui, &mut deltas);
        assert!(ui.on_pointer_move(20.0, 20.0));
        run(&mut ui, &mut deltas);

        // Simulate idle pointer moves inside already-hovered region (no redraw/end_frame).
        assert!(!ui.on_pointer_move(60.0, 50.0));
        assert!(!ui.on_pointer_move(90.0, 70.0));

        // Pressing must discard pre-press delta on the press frame.
        ui.on_pointer_down(PointerButton::Left);
        run(&mut ui, &mut deltas);
        assert_eq!(deltas.last().copied(), Some([0.0, 0.0]));

        // Post-press drag movement is reported accurately.
        assert!(ui.on_pointer_move(105.0, 75.0));
        run(&mut ui, &mut deltas);
        assert_eq!(deltas.last().copied(), Some([15.0, 5.0]));
    }

    #[test]
    fn prune_stale_state_evicts_unused_animation_caches_only() {
        let mut ui = UiContext::new();
        ui.begin_frame(800.0, 600.0, 1.0);
        ui.ids.push_id("active");
        let b = ui.col(|_| {});
        let active_id = b.id();
        let _ = b.anim_layout(active_id);
        ui.ids.pop_id();
        ui.ids.push_id("stale");
        let b2 = ui.col(|_| {});
        let stale_id = b2.id();
        let _ = b2.anim_layout(stale_id);
        ui.ids.pop_id();
        ui.end_frame();

        assert!(ui.flips.contains_key(&active_id));
        assert!(ui.flips.contains_key(&stale_id));

        for _ in 0..420 {
            ui.begin_frame(800.0, 600.0, 1.0);
            ui.ids.push_id("active");
            let b = ui.col(|_| {});
            let id = b.id();
            let _ = b.anim_layout(id);
            ui.ids.pop_id();
            ui.end_frame();
        }

        assert!(ui.flips.contains_key(&active_id));
        assert!(!ui.flips.contains_key(&stale_id));
    }

    #[test]
    fn css_box_model_flex_alignment_and_state_modifiers() {
        let mut ui = UiContext::new();
        let custom_bg = Color::from_hex(0x10B981);
        let custom_hover = Color::from_hex(0x059669);
        let custom_track = Color::from_hex(0x334155);
        let mut val = 0.5_f32;

        // 1. Margins, justify_between, align_end, align_self, btn hover preservation, slider track_bg + compact.
        ui.begin_frame(400.0, 200.0, 1.0);
        ui.row(|ui| {
            ui.btn("A")
                .w(60.0)
                .h(30.0)
                .m(10.0)
                .bg(custom_bg)
                .hover_bg(custom_hover)
                .ripple(false);
            ui.txt("B").w(40.0).h(20.0).align_self(crate::Align::End);
        })
        .w(400.0)
        .h(100.0)
        .justify_between()
        .align_start();
        ui.slider("Vol", &mut val, 0.0..=1.0)
            .track_bg(custom_track)
            .compact();
        ui.end_frame();

        let btn_rect = ui
            .semantics()
            .iter()
            .find(|n| n.label == "A")
            .unwrap()
            .bounds;
        assert!((btn_rect.x - 10.0).abs() < 0.5);
        assert!((btn_rect.y - 10.0).abs() < 0.5);

        let b_rect = ui
            .semantics()
            .iter()
            .find(|n| n.label == "B")
            .unwrap()
            .bounds;
        assert!((b_rect.x - 360.0).abs() < 0.5);
        assert!((b_rect.y - 80.0).abs() < 0.5);

        // Compact slider omits header label ("Vol") while keeping Role::Slider accessible.
        assert!(!ui
            .semantics()
            .iter()
            .any(|n| n.role == Role::Label && n.label == "Vol"));

        // Hovering button uses custom_hover while preserving custom_bg when not hovered.
        ui.on_pointer_move(20.0, 20.0);
        ui.on_pointer_down(PointerButton::Left);
        assert!(ui.ripples.is_empty());
        ui.begin_frame(400.0, 200.0, 1.0);
        ui.btn("C").w(60.0).h(30.0).bg(custom_bg);
        ui.end_frame();
    }

    #[test]
    fn focus_ring_customization_and_focus_visible() {
        let mut ui = UiContext::new();
        let ring_col = Color::from_hex(0xFF44AA);
        let mut tab_idx = 0usize;

        let build = |ui: &mut UiContext, tab_idx: &mut usize| {
            ui.begin_frame(400.0, 200.0, 1.0);
            ui.col(|ui| {
                ui.tabs("nav", tab_idx, &["One", "Two"]);
                ui.btn("Custom")
                    .w(100.0)
                    .h(30.0)
                    .focus_ring(3.0, ring_col)
                    .focus_ring_offset(4.0);
                ui.btn("NoRing").w(100.0).h(30.0).no_focus_ring();
            })
            .gap(8.0);
            ui.end_frame();
        };

        // Frame 1: layout widgets.
        build(&mut ui, &mut tab_idx);
        let one_bounds = ui
            .semantics()
            .iter()
            .find(|n| n.label == "One")
            .unwrap()
            .bounds;

        // Pointer click on tab "One" focuses it for input, but does NOT draw a focus ring (`focus_visible == false`).
        ui.on_pointer_move(one_bounds.x + 8.0, one_bounds.y + 8.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        build(&mut ui, &mut tab_idx);
        let has_theme_ring = ui.draw_list().base_cmds.iter().any(|c| {
            matches!(
                c,
                DrawCmd::Rect {
                    bg,
                    border_width,
                    border_color,
                    ..
                } if *bg == Color::TRANSPARENT && (*border_width - 2.0).abs() < 0.01 && *border_color == ui.theme().focus_ring
            )
        });
        assert!(!has_theme_ring);

        // Keyboard Tab moves focus to "Two" (`focus_visible == true`) and draws the focus ring without clobbering tab border.
        ui.on_key_down(Key::Tab, Modifiers::default());
        build(&mut ui, &mut tab_idx);
        let has_theme_ring = ui.draw_list().base_cmds.iter().any(|c| {
            matches!(
                c,
                DrawCmd::Rect {
                    bg,
                    border_width,
                    border_color,
                    ..
                } if *bg == Color::TRANSPARENT && (*border_width - 2.0).abs() < 0.01 && *border_color == ui.theme().focus_ring
            )
        });
        assert!(has_theme_ring);

        // Tab to "Custom": draws 3px `ring_col` expanded by 4px offset.
        ui.on_key_down(Key::Tab, Modifiers::default());
        build(&mut ui, &mut tab_idx);
        let custom_bounds = ui
            .semantics()
            .iter()
            .find(|n| n.label == "Custom")
            .unwrap()
            .bounds;
        let has_custom_ring = ui.draw_list().base_cmds.iter().any(|c| {
            matches!(
                c,
                DrawCmd::Rect {
                    rect,
                    bg,
                    border_width,
                    border_color,
                    ..
                } if *bg == Color::TRANSPARENT
                    && (*border_width - 3.0).abs() < 0.01
                    && *border_color == ring_col
                    && (rect.x - (custom_bounds.x - 4.0)).abs() < 0.5
                    && (rect.w - (custom_bounds.w + 8.0)).abs() < 0.5
            )
        });
        assert!(has_custom_ring);

        // Tab to "NoRing": no focus ring is emitted.
        ui.on_key_down(Key::Tab, Modifiers::default());
        build(&mut ui, &mut tab_idx);
        let has_any_ring = ui.draw_list().base_cmds.iter().any(|c| {
            matches!(
                c,
                DrawCmd::Rect {
                    bg,
                    border_width,
                    ..
                } if *bg == Color::TRANSPARENT && *border_width > 0.0
            )
        });
        assert!(!has_any_ring);
    }

    #[test]
    fn interact_respects_tree_order_inside_hit_testable_container() {
        let mut ui = UiContext::new();
        let gid = ui.id("overlay_gesture");

        // Frame 1: zstack itself is hit_testable (hover_bg), with a hit_testable backdrop child
        // before interact(), and interact() on top of both.
        ui.begin_frame(400.0, 300.0, 1.0);
        ui.zstack(|ui| {
            ui.btn("Backdrop").w(400.0).h(300.0);
            let _ = ui.interact(gid, Rect::new(20.0, 20.0, 100.0, 60.0));
        })
        .w(400.0)
        .h(300.0)
        .hover_bg(Color::rgba(255, 255, 255, 10));
        ui.end_frame();

        // Pointer inside interact rect must hit `gid`, not the enclosing zstack or earlier Backdrop.
        ui.on_pointer_move(50.0, 40.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);

        ui.begin_frame(400.0, 300.0, 1.0);
        let mut clicked = false;
        ui.zstack(|ui| {
            ui.btn("Backdrop").w(400.0).h(300.0);
            let g = ui.interact(gid, Rect::new(20.0, 20.0, 100.0, 60.0));
            clicked = g.clicked;
        })
        .w(400.0)
        .h(300.0)
        .hover_bg(Color::rgba(255, 255, 255, 10));
        ui.end_frame();

        assert!(
            clicked,
            "interact() on top in tree order should receive click"
        );
    }

    #[test]
    fn rich_wrap_respects_max_w_in_unbounded_row_and_grid_col_w() {
        let mut ui = UiContext::new();
        let long_text =
            "This is a long rich text span that must wrap inside its max_w or grid column.";

        ui.begin_frame(600.0, 400.0, 1.0);
        ui.row(|ui| {
            ui.rich(&[Span::text(long_text)]).wrap().max_w(140.0);
        });
        ui.grid(2, |ui| {
            ui.rich(&[Span::text(long_text)]).wrap();
            ui.rich(&[Span::text("Short")]);
        })
        .w(300.0)
        .gap(10.0);
        ui.end_frame();

        let labels: Vec<Rect> = ui
            .semantics()
            .iter()
            .filter(|n| n.role == Role::Label)
            .map(|n| n.bounds)
            .collect();
        assert_eq!(labels.len(), 3);
        assert!(
            labels[0].w <= 140.5,
            "row rich with max_w(140) overflowed: {}",
            labels[0].w
        );
        assert!(
            labels[0].h > 25.0,
            "row rich with max_w(140) did not wrap vertically"
        );
        assert!(
            labels[1].w <= 145.5,
            "grid rich with wrap() overflowed col_w=145: {}",
            labels[1].w
        );
    }

    #[test]
    fn embedded_host_helpers_leave_cancel_focus_hover_and_hud_hotkey() {
        let mut ui = UiContext::new();
        let mut text = crate::ui::text_edit::TextEditState::new();
        let mut clicks = 0usize;

        let render = |ui: &mut UiContext,
                          text: &mut crate::ui::text_edit::TextEditState,
                          clicks: &mut usize| {
            ui.begin_frame(400.0, 300.0, 1.0);
            ui.col(|ui| {
                if ui.btn("Action").w(120.0).h(32.0).clicked() {
                    *clicks += 1;
                }
                ui.input("field", text).w(160.0).h(32.0);
                // Non-interactive opaque panel for is_pointer_over_ui check.
                ui.col(|_| {}).w(120.0).h(40.0).bg(Color::rgb(20, 30, 40));
            })
            .w(180.0)
            .gap(8.0);
            ui.end_frame();
        };

        render(&mut ui, &mut text, &mut clicks);
        let btn_bounds = ui
            .semantics()
            .iter()
            .find(|n| n.label == "Action")
            .unwrap()
            .bounds;
        let input_bounds = ui
            .semantics()
            .iter()
            .find(|n| n.role == Role::TextInput)
            .unwrap()
            .bounds;

        // Hovering button sets hovered_id and is_pointer_over_ui.
        assert!(ui.on_pointer_move(btn_bounds.x + 10.0, btn_bounds.y + 10.0));
        assert!(ui.hovered_id().is_some());
        assert!(ui.is_pointer_over_ui());

        // Pressing then cancelling must NOT fire a click on the next frame.
        ui.on_pointer_down(PointerButton::Left);
        assert!(ui.on_pointer_cancel());
        assert!(!ui.on_pointer_cancel());
        ui.on_pointer_up(PointerButton::Left);
        render(&mut ui, &mut text, &mut clicks);
        assert_eq!(clicks, 0);

        // Moving over the opaque non-interactive panel: hovered_id is None, is_pointer_over_ui is true.
        let panel_y = input_bounds.y + input_bounds.h + 16.0;
        ui.on_pointer_move(20.0, panel_y);
        assert_eq!(ui.hovered_id(), None);
        assert!(ui.is_pointer_over_ui());

        // Moving into transparent empty area outside panels: is_pointer_over_ui is false.
        ui.on_pointer_move(350.0, 250.0);
        assert_eq!(ui.hovered_id(), None);
        assert!(!ui.is_pointer_over_ui());

        // Hovering button again, then leaving clears pointer_pos, hovered_id, and returns true.
        assert!(ui.on_pointer_move(btn_bounds.x + 10.0, btn_bounds.y + 10.0));
        assert!(ui.hovered_id().is_some());
        assert!(ui.on_pointer_leave());
        assert_eq!(ui.pointer_pos(), None);
        assert_eq!(ui.hovered_id(), None);
        assert!(!ui.is_pointer_over_ui());
        assert!(!ui.on_pointer_leave());

        // Focus text input -> is_text_input_focused() == true; clear_focus() clears it.
        ui.on_pointer_move(input_bounds.x + 10.0, input_bounds.y + 10.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        render(&mut ui, &mut text, &mut clicks);
        assert!(ui.is_text_input_focused());
        assert!(ui.focused_id().is_some());
        ui.clear_focus();
        assert!(!ui.is_text_input_focused());
        assert_eq!(ui.focused_id(), None);

        // HUD hotkey defaults to enabled; disabling prevents F12 from toggling HUD.
        assert!(ui.hud_hotkey_enabled());
        assert!(!ui.hud_visible());
        ui.on_key_down(Key::F12, Modifiers::default());
        assert!(ui.hud_visible());
        ui.set_hud_visible(false);
        assert!(!ui.hud_visible());

        ui.set_hud_hotkey_enabled(false);
        assert!(!ui.hud_hotkey_enabled());
        ui.on_key_down(Key::F12, Modifiers::default());
        assert!(!ui.hud_visible());
        assert!(ui.shortcut_pressed(Key::F12));
    }
}
