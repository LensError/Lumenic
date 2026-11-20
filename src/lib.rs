//! Lumenic (`lumenic`) cross-platform native and WebAssembly application framework.
//!
//! All third-party platform crates are strictly encapsulated behind internal modules.
//!
//! ## Quick Start
//! ```no_run
//! use lumenic::{LoopMode, Theme, WindowConfig};
//!
//! fn main() -> Result<(), lumenic::Error> {
//!     let mut enabled = true;
//!     let mut gain = 0.75_f32;
//!     let theme = Theme::default();
//!
//!     lumenic::run(WindowConfig::new("Lumenic App"), LoopMode::Idle, move |ui, app| {
//!         let csd = ui.csd_titlebar("Lumenic App");
//!         app.perform_csd(csd);
//!
//!         ui.col(|ui| {
//!             ui.txt("Audio Control").sz(20.0);
//!             ui.toggle("Enable Output", &mut enabled);
//!             ui.slider("Master Gain", &mut gain, 0.0..=1.0);
//!             if ui.btn("Reset").clicked() {
//!                 gain = 0.75;
//!             }
//!         })
//!         .p(16.0)
//!         .gap(10.0)
//!         .bg(theme.surface)
//!         .r(12.0);
//!     })
//! }
//! ```
//!
//! ## Architecture & Core Types
//! - **Entrypoint & Windowing:** [`run`], [`WindowConfig`], [`LoopMode`], [`Decorations`], [`App`] (window controls, `request_attention` / `clear_attention`, popovers, texture upload/destroy, `pick_file` / `save_file` / `pick_folder`), [`PopoverId`], [`CsdAction`], [`Waker`].
//! - **Layout & Styling:** [`UiContext`] (flex containers, split panes, virtualized lists, `table` / `data_grid`, `tabs` / `segmented_control`, `collapsing_header` / `tree_view`, `radio` / `radio_group`, `toggle` / `checkbox`, `slider` / `range_slider`, `number_input`, `color_picker`, `input` / `password_input` / `textarea`, `dropdown` / `combobox`, `context_menu`, `modal`, `progress_bar`, `spinner`, `badge`, `toasts`, `sparkline`, `viewport`), [`StyleBuilder`], [`ButtonResponse`], [`RichResponse`], [`TableColumn`], [`TableState`], [`ToastQueue`], [`ToastItem`], [`ToastLevel`], [`Theme`], [`Color`], [`Rect`], [`RectShape`], [`Span`].
//! - **Text Editing:** [`TextEditState`] (UTF-8 gap buffer with grapheme cursor, selection, clipboard, IME preedit, and [`History`] undo/redo).
//! - **2D & 3D Viewports:** [`Canvas2D`], [`Scene3D`], [`Camera3D`], [`MeshData`], [`Vertex3D`], [`Material`], [`Transform3D`], [`MeshId`], [`NodeId`], [`TextureId`], [`TextureFilter`].
//! - **Sandboxed Storage & Networking:** [`SafeDir`], [`StorageMode`], [`DebouncedSaver`], [`DialogHandle`], [`HttpClient`], [`HttpRequest`], [`HttpResponse`], [`HttpHandle`], [`HttpLimits`], [`HttpMethod`], [`WsClient`], [`WsMessage`], [`WsLimits`].
//! - **Inspection & Automation:** [`inspect_to_files`], [`InspectOptions`], [`run_mcp_stdio`], [`McpServer`], [`PerfStats`].
//!
//! ## Built-In CLI & Automation Flags (Native)
//! Any application launched via [`run`] automatically supports:
//! - `--inspect [--out-png <path>] [--out-json <path>]`: Renders one frame offscreen and writes
//!   the rasterized PNG (default `inspect.png`) and semantic layout tree (default `layout.json`).
//! - `--mcp`: Runs the headless stdio JSON-RPC 2.0 MCP server (`1024x700` or configured window size).
//! - `--mcp-live` or `--mcp --headed`: Opens the normal visible desktop window AND listens on
//!   stdio for live JSON-RPC 2.0 UI automation commands (`--mcp-exit-on-eof` closes the window when stdin closes).
//! - `--mcp-port <port>` (or `--mcp-port=<port>`): Opens the normal visible desktop window AND
//!   serves live JSON-RPC 2.0 UI automation commands over `127.0.0.1:<port>`.
//! - `--headless-smoke`: Steps two headless frames and exits `0` for CI smoke checks.
//!
//! ## MCP JSON-RPC 2.0 Tools
//! Requests may invoke tools directly (`{"jsonrpc":"2.0","id":1,"method":"ui_tree","params":{}}`)
//! or via standard MCP `tools/call` (`{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ui_click","arguments":{"label":"Run"}}}`):
//! - `ui_tree`: Returns the semantic layout tree (`width`, `height`, `scale_factor`, `nodes` with `id`, `role`, `label`, `bounds`, `focused`, `enabled`, `toggled`, `value`).
//! - `ui_screenshot`: Rasterizes the current frame (`{"path":"optional_out.png"}`) and returns `{"width","height","png_bytes"}`.
//! - `ui_click`: Clicks target by `"label"`, hex `"id"`, or `"x"`/`"y"` logical coordinates (optional `"button": "left"|"right"|"middle"`, `"double": true`).
//! - `ui_type`: Optional target (`"label"`, `"id"`, or `"x"`/`"y"`) to focus first, plus `"text"` string to commit.
//! - `ui_key`: Presses `"key"` (`"Tab"`, `"Enter"`, `"Escape"`, `"Backspace"`, `"Delete"`, `"Space"`, `"Left"`, `"Right"`, `"Up"`, `"Down"`, `"Home"`, `"End"`, `"F12"`, or single char) with optional `"shift"`, `"ctrl"`, `"alt"`, `"super_key"` booleans.
//! - `ui_scroll`: Scrolls by `"dx"`, `"dy"` (optional target `"label"`, `"id"`, or `"x"`/`"y"` to position cursor first).
//! - `ui_set_theme`: Switches active theme (`"theme": "dark" | "light"`, plus optional `"background"`, `"surface"`, `"primary"` hex strings and `"radius"`).
//! - `ui_step_frames`: Advances physics and UI frames (`"count": 1..=1000`, `"dt": 0.016`).
//! - `ui_perf`: Returns frame/layout/render timings, draw calls, SDF instance count, bump arena usage, and glyph atlas occupancy.
//! - `ui_close`: Requests clean application exit (`{"closing":true}`).

mod gfx;
mod os;
/// System utilities (bump arena, hierarchical widget IDs, history, panic flush, sandboxed storage, and MCP tooling).
pub mod sys;
/// Core immediate-mode UI types, context, layout engine, and composable widgets.
pub mod ui;

pub use gfx::image::{
    decode_gif_frames, decode_icon, decode_image, decode_png, encode_png, GifFrame, RgbaImage,
};
pub use gfx::inspect::{inspect_to_files, InspectOptions};
pub use gfx::text::{register_font, set_default_font_family, set_default_mono_font_family};
#[cfg(feature = "gltf")]
pub use gfx::viewport::LoadedGltf;
pub use gfx::viewport::{
    Canvas2D, GlassMaterial, Material, MeshData, Scene3D, TextureFilter, Transform3D, Vertex3D,
};
pub use os::dialogs::{DialogHandle, FileDialogRequest};
pub use os::http::{HttpClient, HttpHandle, HttpLimits, HttpMethod, HttpRequest, HttpResponse};
pub use os::ws::{WsClient, WsLimits, WsMessage};
pub use os::{attach_cli_console_if_needed, run, App, TrayConfig, TrayEvent, TrayMenuItem, Waker};
pub use sys::arena::FrameArena;
pub use sys::history::History;
pub use sys::id::{IdStack, WidgetId};
pub use sys::mcp::{run_mcp_stdio, McpServer};
pub use sys::panic_flush::{register_panic_flush, run_panic_flush_callbacks};
pub use sys::sandbox::{DebouncedSaver, SafeDir, StorageMode};
pub use ui::{
    vlist_range_binary_search, Align, ButtonResponse, Camera3D, Color, CsdAction, CsdStyle,
    CursorIcon, Decorations, DrawCmd, DrawList, Error, GestureResponse, Gradient, ImageFit,
    Justify, Key, LoopMode, MeshId, Modifiers, NodeId, OwnedSpan, PerfStats, PointerButton,
    PopoverId, Rect, RectShape, RichResponse, Role, SemanticNode, Span, SpanKind, StyleBuilder,
    TableColumn, TableState, TextAlign, TextEditState, TextureId, Theme, ToastCloseStyle,
    ToastCorner, ToastItem, ToastLevel, ToastQueue, ToastStyle, UiContext, WindowConfig,
};
