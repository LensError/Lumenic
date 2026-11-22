use crate::gfx::viewport::{Canvas2DCmd, SceneSnapshot, TextureFilter};
use crate::sys::id::WidgetId;
use crate::ui::types::{Color, Gradient, Rect, RectShape, Span, TextAlign, TextureId};

/// Accessibility and structural role emitted alongside `DrawList`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Root window frame.
    Window,
    /// Layout grouping container (`row`, `col`, `grid`, `zstack`).
    Container,
    /// Clickable button or interactive action element.
    Button,
    /// Binary switch or checkbox control.
    Toggle,
    /// Continuous numeric range slider.
    Slider,
    /// Editable single-line or multi-line text field.
    TextInput,
    /// Collapsible choice menu / combobox.
    Dropdown,
    /// Topmost blocking dialog or popup overlay.
    Modal,
    /// Draggable split-pane divider handle.
    SplitDivider,
    /// Horizontal or vertical hairline separator.
    Divider,
    /// Static or selectable text block.
    Label,
    /// Raster or vector image element.
    Image,
    /// Custom 2D/3D hardware-accelerated viewport surface.
    Viewport,
}

/// Owned inline span style kind stored inside `DrawCmd::RichText`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanKind {
    /// Regular body text span.
    Text,
    /// Bold-weight inline span.
    Bold,
    /// Italicized inline span.
    Italic,
    /// Monospace inline pill background span.
    Code,
    /// Highlighted `@mention` badge span.
    Mention,
    /// Clickable underlined hyperlink span.
    Link,
    /// Plain monospace inline span without pill background.
    Mono,
}

/// Owned inline rich-text span recorded into `DrawList`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedSpan {
    /// Visual and semantic span classification.
    pub kind: SpanKind,
    /// Display text content of the span.
    pub text: String,
    /// Target identifier or URL for [`SpanKind::Link`] spans.
    pub link_id: Option<String>,
    /// Optional explicit foreground color override.
    pub color: Option<Color>,
}

impl<'a> From<&Span<'a>> for OwnedSpan {
    fn from(span: &Span<'a>) -> Self {
        match *span {
            Span::Text(t) => Self {
                kind: SpanKind::Text,
                text: String::from(t),
                link_id: None,
                color: None,
            },
            Span::Bold(t) => Self {
                kind: SpanKind::Bold,
                text: String::from(t),
                link_id: None,
                color: None,
            },
            Span::Italic(t) => Self {
                kind: SpanKind::Italic,
                text: String::from(t),
                link_id: None,
                color: None,
            },
            Span::Code(t) => Self {
                kind: SpanKind::Code,
                text: String::from(t),
                link_id: None,
                color: None,
            },
            Span::Mention(t) => Self {
                kind: SpanKind::Mention,
                text: String::from(t),
                link_id: None,
                color: None,
            },
            Span::Link { text, id } => Self {
                kind: SpanKind::Link,
                text: String::from(text),
                link_id: Some(String::from(id)),
                color: None,
            },
            Span::Colored { text, color } => Self {
                kind: SpanKind::Text,
                text: String::from(text),
                link_id: None,
                color: Some(color),
            },
            Span::Mono(t) => Self {
                kind: SpanKind::Mono,
                text: String::from(t),
                link_id: None,
                color: None,
            },
            Span::MonoColored { text, color } => Self {
                kind: SpanKind::Mono,
                text: String::from(text),
                link_id: None,
                color: Some(color),
            },
        }
    }
}

/// Lightweight semantic metadata node for `--mcp` inspection and accessibility.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticNode {
    /// Deterministic scoped widget identifier.
    pub id: WidgetId,
    /// Accessibility and UI interaction role.
    pub role: Role,
    /// Accessible label or displayed text caption.
    pub label: String,
    /// Optional current widget value (such as toggle state, text value, or slider value).
    pub value: Option<String>,
    /// Logical layout bounding box `[x, y, w, h]`.
    pub bounds: Rect,
    /// Whether this node currently holds keyboard focus.
    pub focused: bool,
    /// Whether interaction is enabled for this node.
    pub enabled: bool,
    /// Whether this node participates in `Tab` focus traversal.
    pub focusable: bool,
    /// Whether this node lives inside a global overlay layer.
    pub is_overlay: bool,
}

/// Backend-agnostic GPU draw command emitted by `UiContext`.
#[derive(Clone, Debug, PartialEq)]
pub enum DrawCmd {
    /// Signed-distance-field rounded rectangle, circle, squircle, shadow, or frosted glass quad.
    Rect {
        /// Logical rectangle bounds before `transform`.
        rect: Rect,
        /// Flat fill background color.
        bg: Color,
        /// Optional two-stop background linear/radial gradient.
        bg_grad: Option<Gradient>,
        /// Border thickness in logical points.
        border_width: f32,
        /// Flat border stroke color.
        border_color: Color,
        /// Optional linear border gradient.
        bd_grad: Option<Gradient>,
        /// Corner rounding radius in logical points (or fillet radius when `shape == RectShape::Bevel`).
        radius: f32,
        /// Independent corner radii `[tl, tr, br, bl]` in logical points (or chamfer sizes when `shape == RectShape::Bevel`).
        corner_radii: [f32; 4],
        /// Drop-shadow Gaussian blur radius in logical points.
        shadow_blur: f32,
        /// Drop-shadow color.
        shadow_color: Color,
        /// Corner geometry mode (`Box`, `Circle`, `Squircle`, `Bevel`, `Hexagon`, or `HexagonPointy`).
        shape: RectShape,
        /// Enables Dual-Kawase frosted glass backdrop blur.
        glass: bool,
        /// Enables radial pointer-proximity border illumination (`Reveal`).
        cursor_glow: bool,
        /// Active pointer ripple origin and progress `(center, t)`.
        ripple: Option<([f32; 2], f32)>,
        /// Active diagonal loading shimmer phase `0.0..=1.0`.
        shimmer_phase: Option<f32>,
        /// 2D transform `[scale, rotate_rad, translate_x, translate_y]`.
        transform: [f32; 4],
    },
    /// Shaped plain text run rasterized through `GlyphAtlas`.
    Text {
        /// Logical layout text bounds.
        rect: Rect,
        /// UTF-8 string to shape and draw.
        text: String,
        /// Foreground glyph color.
        color: Color,
        /// Font size in logical points.
        size: f32,
        /// Numeric font weight (`100..=900`).
        weight: u16,
        /// Forces monospace fallback when no explicit `font_family` is set.
        mono: bool,
        /// Optional registered custom or system font family name.
        font_family: Option<String>,
        /// Enables multi-line word/glyph wrapping inside `rect.w`.
        wrap: bool,
        /// Truncates overflowing single-line text with `...`.
        ellipsis: bool,
        /// Horizontal text alignment inside `rect`.
        align: TextAlign,
        /// Extra letter spacing in `em` units.
        tracking: f32,
        /// Line-height multiplier relative to `size`.
        line_height: f32,
        /// 2D transform `[scale, rotate_rad, translate_x, translate_y]`.
        transform: [f32; 4],
    },
    /// Textured quad sampled from an uploaded GPU RGBA8 texture.
    Image {
        /// Destination rectangle in logical coordinates.
        rect: Rect,
        /// Texture handle registered on the GPU backend.
        tex: TextureId,
        /// Texture sampling filter (`Nearest` or `Linear`).
        filter: TextureFilter,
        /// Normalized texture UV rectangle `[u0, v0, u1, v1]`.
        uv: [f32; 4],
        /// Multiplicative RGBA tint color.
        tint: Color,
        /// Optional border stroke thickness in logical points.
        border_width: f32,
        /// Border stroke color.
        border_color: Color,
        /// Corner rounding radius in logical points.
        radius: f32,
        /// Independent corner radii `[tl, tr, br, bl]` in logical points.
        corner_radii: [f32; 4],
        /// Clip mask shape (`Box`, `Circle`, `Squircle`, `Bevel`, `Hexagon`, or `HexagonPointy`).
        shape: RectShape,
        /// 2D transform `[scale, rotate_rad, translate_x, translate_y]`.
        transform: [f32; 4],
    },
    /// Shaped multi-span rich text run supporting bold, italic, code pills, mentions, and links.
    RichText {
        /// Logical layout text bounds.
        rect: Rect,
        /// Ordered inline styled spans.
        spans: Vec<OwnedSpan>,
        /// Default text color for unstyled spans.
        default_color: Color,
        /// Pill background color for [`SpanKind::Code`] spans.
        code_bg: Color,
        /// Pill background color for [`SpanKind::Mention`] spans.
        mention_bg: Color,
        /// Color used for [`SpanKind::Link`] text and underlines.
        link_color: Color,
        /// Font size in logical points.
        size: f32,
        /// Optional registered custom or system font family name.
        font_family: Option<String>,
        /// Enables multi-line wrapping inside `rect.w`.
        wrap: bool,
        /// Truncates overflowing single-line text with `...`.
        ellipsis: bool,
        /// Horizontal alignment inside `rect`.
        align: TextAlign,
        /// Extra letter spacing in `em` units.
        tracking: f32,
        /// Line-height multiplier relative to `size`.
        line_height: f32,
        /// 2D transform `[scale, rotate_rad, translate_x, translate_y]`.
        transform: [f32; 4],
    },
    /// GPU `sdSegment` polyline sparkline curve with optional under-curve fill.
    Sparkline {
        /// Logical plot area rectangle.
        rect: Rect,
        /// Normalized or arbitrary sample values (`min..max` scaled cleanly).
        samples: Vec<f32>,
        /// Polyline stroke color.
        color: Color,
        /// Optional shaded area fill color under the curve.
        fill_color: Option<Color>,
        /// Stroke width in logical points.
        stroke_width: f32,
    },
    /// Embedded 2D canvas primitives and/or 3D scene snapshot rendered inside `rect`.
    Viewport {
        /// Logical viewport bounds.
        rect: Rect,
        /// Batched 2D primitives and custom shader/instanced passes.
        canvas: Vec<Canvas2DCmd>,
        /// Resolved 3D camera + mesh draw list snapshot.
        scene: SceneSnapshot,
    },
    /// Pushes an intersection scissor rectangle onto the active clip stack.
    PushClip(Rect),
    /// Pops the most recent scissor rectangle from the clip stack.
    PopClip,
}

/// Flat per-frame command buffer separating base UI from deferred global overlays.
#[derive(Clone, Debug, Default)]
pub struct DrawList {
    /// Commands emitted in base document flow order.
    pub base_cmds: Vec<DrawCmd>,
    /// Commands emitted in deferred global overlay order.
    pub overlay_cmds: Vec<DrawCmd>,
}

impl DrawList {
    /// Clears draw commands while preserving allocated capacity.
    pub fn clear(&mut self) {
        self.base_cmds.clear();
        self.overlay_cmds.clear();
    }

    /// Iterates all draw commands in render order (base commands followed by overlays).
    pub fn all_commands(&self) -> impl Iterator<Item = &DrawCmd> {
        self.base_cmds.iter().chain(self.overlay_cmds.iter())
    }
}
