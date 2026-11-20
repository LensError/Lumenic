#![forbid(unsafe_code)]

/// Immediate-mode UI context, 2-pass layout engine, and fluent style builders.
pub mod context;
/// Backend-agnostic GPU draw list and accessibility semantic tree types.
pub mod draw;
/// Spring, tween, FLIP layout animation, and ripple physics helpers.
pub mod physics;
/// Gap-buffer UTF-8 text editing engine with grapheme cursor and selection support.
pub mod text_edit;
/// Core geometry, color, input, theme, configuration, and error types.
pub mod types;
/// Higher-level composable UI controls and layout widgets.
pub mod widgets;

pub use context::{ButtonResponse, RichResponse, StyleBuilder, UiContext};
pub use draw::{DrawCmd, DrawList, OwnedSpan, Role, SemanticNode, SpanKind};
pub use text_edit::TextEditState;
pub use types::{
    Align, Camera3D, Color, CsdAction, CsdStyle, CursorIcon, Decorations, Error, GestureResponse,
    Gradient, ImageFit, Justify, Key, LoopMode, MeshId, Modifiers, NodeId, PerfStats,
    PointerButton, PopoverId, Rect, RectShape, Span, TextAlign, TextureId, Theme, WindowConfig,
};
pub use widgets::{
    vlist_range_binary_search, TableColumn, TableState, ToastCloseStyle, ToastCorner, ToastItem,
    ToastLevel, ToastQueue, ToastStyle,
};
