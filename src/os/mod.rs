pub(crate) mod clipboard;
pub mod dialogs;
mod gl;
pub mod http;
mod input;
pub mod tray;
pub mod ws;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod wasm;

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use gl::GlBackend;
use tray::TrayCommand;
pub use tray::{TrayConfig, TrayEvent, TrayMenuItem};

use crate::gfx::image::decode_png;
use crate::os::dialogs::{
    spawn_pick_file, spawn_pick_folder, spawn_save_file, DialogHandle, FileDialogRequest,
};
use crate::ui::types::{CsdAction, Error, LoopMode, Modifiers, PopoverId, TextureId, WindowConfig};
use crate::ui::UiContext;

#[cfg(target_os = "windows")]
extern "system" {
    fn AttachConsole(dw_process_id: u32) -> i32;
}

/// Attaches stdout/stderr to the parent terminal on Windows GUI subsystem runs when CLI flags exist.
pub fn attach_cli_console_if_needed(args: &[String]) -> bool {
    let wants_cli = args.iter().any(|a| {
        matches!(
            a.as_str(),
            "--mcp"
                | "--mcp-live"
                | "--headed"
                | "--mcp-port"
                | "--inspect"
                | "--headless-smoke"
                | "--help"
                | "-h"
        ) || a.starts_with("--mcp-port=")
    });
    if !wants_cli {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
        // SAFETY: `AttachConsole` with `ATTACH_PARENT_PROCESS` has no pointer or memory invariants.
        unsafe { AttachConsole(ATTACH_PARENT_PROCESS) != 0 }
    }
    #[cfg(not(target_os = "windows"))]
    {
        true
    }
}

#[derive(Clone, Debug)]
pub enum UserEvent {
    Wake,
    #[cfg(not(target_arch = "wasm32"))]
    McpRequest {
        line: String,
        reply: std::sync::mpsc::Sender<String>,
    },
    #[cfg(target_arch = "wasm32")]
    RouteChanged(String),
    #[cfg(target_arch = "wasm32")]
    Paste(String),
    #[cfg(target_arch = "wasm32")]
    ResizeCanvas,
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WindowCommand {
    Drag,
    Minimize,
    ToggleMaximize,
    SetVisible(bool),
    Focus,
    RequestAttention(Option<bool>),
    OpenPopover { id: PopoverId, config: WindowConfig },
    ClosePopover(PopoverId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TextureOp {
    Upload {
        id: TextureId,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    UpdateSub {
        id: TextureId,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    Destroy(TextureId),
}

/// Thread-safe handle to wake the UI event loop from background threads.
#[derive(Clone, Default)]
pub struct Waker {
    inner: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl Waker {
    pub(crate) fn from_fn(f: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            inner: Some(Arc::new(f)),
        }
    }

    /// Wakes the UI event loop so pending state changes are drawn.
    pub fn wake(&self) {
        if let Some(f) = &self.inner {
            f();
        }
    }
}

impl fmt::Debug for Waker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Waker")
            .field("active", &self.inner.is_some())
            .finish()
    }
}

/// Application lifecycle and loop handle exposed to frame callbacks.
#[derive(Debug, Default)]
pub struct App {
    exit_requested: bool,
    redraw_requested: bool,
    frame_count: u64,
    dt: f32,
    waker: Waker,
    window_cmds: Vec<WindowCommand>,
    tray_cmds: Vec<TrayCommand>,
    tray_events: Vec<TrayEvent>,
    has_tray_icon: bool,
    hovered_file: Option<PathBuf>,
    dropped_files: Vec<PathBuf>,
    open_popovers: Vec<PopoverId>,
    active_popover: Option<PopoverId>,
    next_texture_id: u64,
    texture_sizes: HashMap<TextureId, [u32; 2]>,
    texture_ops: Vec<TextureOp>,
    route_path: String,
    route_history: Vec<String>,
    pub(crate) modifiers: Modifiers,
}

impl App {
    /// Requests clean application shutdown after the current frame.
    pub fn request_exit(&mut self) {
        self.exit_requested = true;
    }

    /// Returns `true` if exit has been requested.
    pub fn is_exit_requested(&self) -> bool {
        self.exit_requested
    }

    /// Schedules an immediate redraw of active windows.
    pub fn request_redraw(&mut self) {
        self.redraw_requested = true;
    }

    /// Initiates native window dragging from a custom titlebar.
    pub fn drag_window(&mut self) {
        self.window_cmds.push(WindowCommand::Drag);
    }

    /// Minimizes the active window.
    pub fn minimize_window(&mut self) {
        self.window_cmds.push(WindowCommand::Minimize);
    }

    /// Toggles maximized state on the active window.
    pub fn toggle_maximize_window(&mut self) {
        self.window_cmds.push(WindowCommand::ToggleMaximize);
    }

    /// Hides the active OS window (removing its taskbar entry while keeping the event loop alive).
    pub fn hide_window(&mut self) {
        self.window_cmds.push(WindowCommand::SetVisible(false));
    }

    /// Shows and focuses the active OS window (restoring from hidden state).
    pub fn show_window(&mut self) {
        self.window_cmds.push(WindowCommand::SetVisible(true));
        self.window_cmds.push(WindowCommand::Focus);
    }

    /// Sets OS window visibility (`true` visible, `false` hidden).
    pub fn set_window_visible(&mut self, visible: bool) {
        self.window_cmds.push(WindowCommand::SetVisible(visible));
    }

    /// Requests OS keyboard focus on the active window.
    pub fn focus_window(&mut self) {
        self.window_cmds.push(WindowCommand::Focus);
    }

    /// Requests OS taskbar / window attention (`critical = true` for urgent flash, `false` for subtle).
    pub fn request_attention(&mut self, critical: bool) {
        self.window_cmds
            .push(WindowCommand::RequestAttention(Some(critical)));
    }

    /// Clears any pending OS taskbar / window attention request.
    pub fn clear_attention(&mut self) {
        self.window_cmds.push(WindowCommand::RequestAttention(None));
    }

    /// Opens a secondary OS popover window sharing the application GL context.
    pub fn open_popover(&mut self, id: PopoverId, config: WindowConfig) {
        if !self.open_popovers.contains(&id) {
            self.open_popovers.push(id);
        }
        self.window_cmds
            .push(WindowCommand::OpenPopover { id, config });
    }

    /// Closes a secondary popover window by `id`.
    pub fn close_popover(&mut self, id: PopoverId) {
        self.mark_popover_closed(id);
        self.window_cmds.push(WindowCommand::ClosePopover(id));
    }

    /// Returns `true` while popover window `id` is tracked as open.
    pub fn is_popover_open(&self, id: PopoverId) -> bool {
        self.open_popovers.contains(&id)
    }

    /// Returns the popover ID being rendered in the current callback, or `None` for the main window.
    pub fn active_popover(&self) -> Option<PopoverId> {
        self.active_popover
    }

    /// Returns the file path currently hovering over the OS window, if any.
    pub fn hovered_file(&self) -> Option<&Path> {
        self.hovered_file.as_deref()
    }

    /// Drains files dropped onto the window via OS drag-and-drop.
    pub fn take_dropped_files(&mut self) -> Vec<PathBuf> {
        core::mem::take(&mut self.dropped_files)
    }

    /// Opens a non-blocking OS file picker on a worker thread and wakes the UI on completion.
    pub fn pick_file(&self, title: &str, extensions: &[&str]) -> DialogHandle {
        spawn_pick_file(
            FileDialogRequest {
                title: String::from(title),
                default_name: None,
                extensions: extensions.iter().map(|s| String::from(*s)).collect(),
            },
            self.waker.clone(),
        )
    }

    /// Opens a non-blocking OS save-file dialog on a worker thread and wakes the UI on completion.
    pub fn save_file(&self, title: &str, default_name: &str, extensions: &[&str]) -> DialogHandle {
        spawn_save_file(
            FileDialogRequest {
                title: String::from(title),
                default_name: Some(String::from(default_name)),
                extensions: extensions.iter().map(|s| String::from(*s)).collect(),
            },
            self.waker.clone(),
        )
    }

    /// Opens a non-blocking OS folder/directory picker on a worker thread and wakes the UI on completion.
    pub fn pick_folder(&self, title: &str) -> DialogHandle {
        spawn_pick_folder(title, self.waker.clone())
    }

    /// Applies an action returned by `UiContext::csd_titlebar`.
    pub fn perform_csd(&mut self, action: CsdAction) {
        match action {
            CsdAction::None => {}
            CsdAction::Minimize => self.minimize_window(),
            CsdAction::ToggleMaximize => self.toggle_maximize_window(),
            CsdAction::Close => self.request_exit(),
            CsdAction::StartDrag => self.drag_window(),
        }
    }

    /// Registers an RGBA8 texture (`width * height * 4` bytes) for `Canvas2D::tex` or `Material.texture`.
    pub fn create_texture_rgba(
        &mut self,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<TextureId, Error> {
        if width == 0 || height == 0 {
            return Err(Error::InvalidInput(String::from(
                "texture dimensions must be non-zero",
            )));
        }
        let expected = usize::try_from(width)
            .ok()
            .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
            .and_then(|px| px.checked_mul(4))
            .ok_or_else(|| Error::InvalidInput(String::from("texture byte size overflow")))?;
        if rgba.len() != expected {
            return Err(Error::InvalidInput(format!(
                "expected {expected} RGBA8 bytes for {width}x{height} texture, got {}",
                rgba.len()
            )));
        }
        self.next_texture_id = self.next_texture_id.saturating_add(1);
        let id = TextureId(self.next_texture_id);
        self.texture_sizes.insert(id, [width, height]);
        self.texture_ops.push(TextureOp::Upload {
            id,
            width,
            height,
            rgba: rgba.to_vec(),
        });
        self.redraw_requested = true;
        Ok(id)
    }

    /// Decodes a PNG image slice and uploads it as an RGBA8 texture, returning its `TextureId`.
    pub fn create_texture_png(&mut self, png_bytes: &[u8]) -> Result<TextureId, Error> {
        let img = decode_png(png_bytes)?;
        self.create_texture_rgba(img.width, img.height, &img.data)
    }

    /// Streams updated RGBA8 pixel bytes into an existing `TextureId` (`glTexSubImage2D` when dimensions match).
    pub fn update_texture_rgba(
        &mut self,
        id: TextureId,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<(), Error> {
        if width == 0 || height == 0 {
            return Err(Error::InvalidInput(String::from(
                "texture dimensions must be non-zero",
            )));
        }
        let expected = usize::try_from(width)
            .ok()
            .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
            .and_then(|px| px.checked_mul(4))
            .ok_or_else(|| Error::InvalidInput(String::from("texture byte size overflow")))?;
        if rgba.len() != expected {
            return Err(Error::InvalidInput(format!(
                "expected {expected} RGBA8 bytes for {width}x{height} texture, got {}",
                rgba.len()
            )));
        }
        self.texture_sizes.insert(id, [width, height]);
        self.texture_ops.push(TextureOp::UpdateSub {
            id,
            width,
            height,
            rgba: rgba.to_vec(),
        });
        self.redraw_requested = true;
        Ok(())
    }

    /// Releases a previously created GPU texture.
    pub fn destroy_texture(&mut self, id: TextureId) {
        self.texture_sizes.remove(&id);
        self.texture_ops.push(TextureOp::Destroy(id));
        self.redraw_requested = true;
    }

    /// Returns `[width, height]` in pixels for a live `TextureId`, if registered.
    pub fn texture_dimensions(&self, id: TextureId) -> Option<[u32; 2]> {
        self.texture_sizes.get(&id).copied()
    }

    pub(crate) fn texture_sizes(&self) -> &HashMap<TextureId, [u32; 2]> {
        &self.texture_sizes
    }

    pub(crate) fn take_texture_ops(&mut self) -> Vec<TextureOp> {
        core::mem::take(&mut self.texture_ops)
    }

    /// Creates or updates the native system tray icon, tooltip, and context menu.
    pub fn set_tray_icon(&mut self, config: TrayConfig) {
        self.has_tray_icon = true;
        self.tray_cmds.push(TrayCommand::Set(config));
    }

    /// Replaces the native system tray context menu items.
    pub fn update_tray_menu(&mut self, items: Vec<TrayMenuItem>) {
        self.tray_cmds.push(TrayCommand::UpdateMenu(items));
    }

    /// Updates the native system tray hover tooltip text.
    pub fn set_tray_tooltip(&mut self, tooltip: Option<&str>) {
        self.tray_cmds
            .push(TrayCommand::SetTooltip(tooltip.map(String::from)));
    }

    /// Removes the native system tray icon.
    pub fn remove_tray_icon(&mut self) {
        self.has_tray_icon = false;
        self.tray_cmds.push(TrayCommand::Remove);
    }

    /// Returns `true` if a system tray icon is currently active.
    pub fn has_tray_icon(&self) -> bool {
        self.has_tray_icon
    }

    /// Drains pending native system tray click and menu events.
    pub fn take_tray_events(&mut self) -> Vec<TrayEvent> {
        core::mem::take(&mut self.tray_events)
    }

    #[cfg_attr(any(target_arch = "wasm32", not(feature = "tray")), allow(dead_code))]
    pub(crate) fn take_tray_commands(&mut self) -> Vec<TrayCommand> {
        core::mem::take(&mut self.tray_cmds)
    }

    #[cfg_attr(any(target_arch = "wasm32", not(feature = "tray")), allow(dead_code))]
    pub(crate) fn push_tray_events(&mut self, mut events: Vec<TrayEvent>) {
        self.tray_events.append(&mut events);
    }

    /// Returns a thread-safe `Waker` for waking the event loop from worker threads.
    pub fn waker(&self) -> Waker {
        self.waker.clone()
    }

    pub(crate) fn set_waker(&mut self, waker: Waker) {
        self.waker = waker;
    }

    pub(crate) fn push_hovered_file(&mut self, path: PathBuf) {
        self.hovered_file = Some(path);
    }

    pub(crate) fn clear_hovered_file(&mut self) {
        self.hovered_file = None;
    }

    pub(crate) fn push_dropped_file(&mut self, path: PathBuf) {
        self.dropped_files.push(path);
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn set_active_popover(&mut self, id: Option<PopoverId>) {
        self.active_popover = id;
    }

    pub(crate) fn mark_popover_closed(&mut self, id: PopoverId) {
        self.open_popovers.retain(|p| *p != id);
    }

    /// Returns the total rendered frame count since startup.
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Returns the elapsed seconds since the previous frame.
    pub fn dt(&self) -> f32 {
        self.dt
    }

    pub(crate) fn begin_step(&mut self, dt: f32) {
        self.dt = dt;
        self.frame_count = self.frame_count.saturating_add(1);
    }

    pub(crate) fn take_redraw_request(&mut self) -> bool {
        let r = self.redraw_requested;
        self.redraw_requested = false;
        r
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn take_window_commands(&mut self) -> Vec<WindowCommand> {
        core::mem::take(&mut self.window_cmds)
    }

    /// Writes `text` to the system or browser clipboard.
    pub fn copy(&mut self, text: &str) {
        clipboard::set_text(text);
    }

    /// Reads UTF-8 text from the system or browser clipboard (if available).
    pub fn paste(&self) -> Option<String> {
        clipboard::get_text()
    }

    /// Launches `url` (`http://`, `https://`, or `mailto:`) in a new browser tab or OS default browser.
    pub fn open_url(&mut self, url: &str) -> Result<(), Error> {
        let trimmed = url.trim();
        let allowed = trimmed.starts_with("https://")
            || trimmed.starts_with("http://")
            || trimmed.starts_with("mailto:");
        if !allowed || trimmed.bytes().any(|b| b.is_ascii_control()) {
            return Err(Error::InvalidInput(String::from(
                "open_url requires an http://, https://, or mailto: URL without control characters",
            )));
        }
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(win) = web_sys::window() {
                win.open_with_url_and_target(trimmed, "_blank")
                    .map_err(|_| Error::Io(String::from("window.open failed")))?;
                return Ok(());
            }
            Err(Error::Io(String::from("browser window unavailable")))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            #[cfg(target_os = "windows")]
            let status = std::process::Command::new("rundll32")
                .args(["url.dll,FileProtocolHandler", trimmed])
                .spawn();
            #[cfg(target_os = "macos")]
            let status = std::process::Command::new("open").arg(trimmed).spawn();
            #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
            let status = std::process::Command::new("xdg-open").arg(trimmed).spawn();
            status
                .map(|_| ())
                .map_err(|e| Error::Io(format!("failed to launch URL: {e}")))
        }
    }

    /// Returns the active normalized URL route path (defaults to `"/"`).
    pub fn route(&self) -> &str {
        if self.route_path.is_empty() {
            "/"
        } else {
            &self.route_path
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn set_route_internal(&mut self, path: String) {
        self.route_path = normalize_route(&path);
        self.redraw_requested = true;
    }

    /// Navigates to `path_or_hash`, updating browser history on `wasm32` and in-memory route state on native.
    pub fn navigate(&mut self, path_or_hash: &str) -> Result<(), Error> {
        let norm = normalize_route(path_or_hash);
        if self.route() != norm {
            self.route_history.push(String::from(self.route()));
        }
        self.route_path = norm.clone();
        self.redraw_requested = true;

        #[cfg(target_arch = "wasm32")]
        if let Some(win) = web_sys::window() {
            if let Ok(hist) = win.history() {
                if hist
                    .push_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&norm))
                    .is_err()
                {
                    let _ = win.location().set_hash(&format!("#{norm}"));
                }
            } else {
                let _ = win.location().set_hash(&format!("#{norm}"));
            }
        }
        Ok(())
    }

    /// Navigates back to the previous recorded route if history is non-empty, returning the active route.
    pub fn navigate_back(&mut self) -> &str {
        if let Some(prev) = self.route_history.pop() {
            self.route_path = prev;
            self.redraw_requested = true;
            #[cfg(target_arch = "wasm32")]
            if let Some(win) = web_sys::window() {
                if let Ok(hist) = win.history() {
                    let _ = hist.back();
                }
            }
        }
        self.route()
    }
}

fn normalize_route(raw: &str) -> String {
    let stripped = raw.trim().trim_start_matches('#');
    if stripped.is_empty() || stripped == "/" {
        String::from("/")
    } else if stripped.starts_with('/') {
        String::from(stripped)
    } else {
        format!("/{stripped}")
    }
}

pub(crate) fn apply_window_config_fonts(config: &WindowConfig) {
    for (name, bytes) in &config.fonts {
        crate::gfx::text::register_font(name, bytes);
    }
    if let Some(ref fam) = config.default_font {
        crate::gfx::text::set_default_font_family(fam);
    }
    if let Some(ref mono) = config.default_mono_font {
        crate::gfx::text::set_default_mono_font_family(mono);
    }
}

/// Starts the platform event loop and drives `update` according to `loop_mode`.
pub fn run<F>(config: WindowConfig, loop_mode: LoopMode, update: F) -> Result<(), Error>
where
    F: FnMut(&mut UiContext, &mut App) + 'static,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        native::run_native(config, loop_mode, update)
    }
    #[cfg(target_arch = "wasm32")]
    {
        wasm::run_wasm(config, loop_mode, update)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn run_borrowed<F>(
    config: WindowConfig,
    loop_mode: LoopMode,
    update: F,
) -> Result<(), Error>
where
    F: FnMut(&mut UiContext, &mut App),
{
    native::run_native_borrowed(config, loop_mode, update)
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RedrawCoalescer {
    pending: bool,
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
impl RedrawCoalescer {
    /// Returns `true` only on the idle-to-pending transition so wasm32 callers do not
    /// repeatedly cancel and re-queue an in-flight `requestAnimationFrame` handle.
    pub(crate) fn request(&mut self) -> bool {
        if self.pending {
            false
        } else {
            self.pending = true;
            true
        }
    }

    /// Marks the in-flight frame callback as delivered so the next frame may be scheduled.
    pub(crate) fn begin_redraw(&mut self) {
        self.pending = false;
    }

    #[cfg(test)]
    pub(crate) fn is_pending(&self) -> bool {
        self.pending
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{attach_cli_console_if_needed, App, WindowCommand};
    use crate::ui::types::{PopoverId, WindowConfig};

    #[test]
    fn app_dnd_popovers_and_cli_console_helper() {
        let mut app = App::default();
        assert_eq!(app.hovered_file(), None);
        app.push_hovered_file(PathBuf::from("a.png"));
        assert_eq!(app.hovered_file(), Some(PathBuf::from("a.png").as_path()));
        app.clear_hovered_file();
        assert_eq!(app.hovered_file(), None);
        app.push_dropped_file(PathBuf::from("b.glb"));
        assert_eq!(app.take_dropped_files(), vec![PathBuf::from("b.glb")]);
        assert!(app.take_dropped_files().is_empty());

        let pid = PopoverId(7);
        app.open_popover(pid, WindowConfig::new("Tool Palette"));
        assert!(app.is_popover_open(pid));
        app.close_popover(pid);
        assert!(!app.is_popover_open(pid));
        assert!(app
            .window_cmds
            .iter()
            .any(|c| matches!(c, WindowCommand::ClosePopover(PopoverId(7)))));

        assert!(!attach_cli_console_if_needed(&[String::from("lumenic")]));

        let rgba = [
            255u8, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        assert!(app.create_texture_rgba(0, 2, &rgba).is_err());
        assert!(app.create_texture_rgba(2, 2, &rgba[..12]).is_err());
        let tex_id = app
            .create_texture_rgba(2, 2, &rgba)
            .expect("valid 2x2 rgba");
        assert_eq!(app.texture_dimensions(tex_id), Some([2, 2]));

        let png = crate::gfx::image::encode_png(2, 2, &rgba).expect("encode png");
        let png_id = app.create_texture_png(&png).expect("decode and upload png");
        assert_eq!(app.texture_dimensions(png_id), Some([2, 2]));

        app.destroy_texture(tex_id);
        assert_eq!(app.texture_dimensions(tex_id), None);
        assert_eq!(app.take_texture_ops().len(), 3);

        // Phase 9 helpers: clipboard copy/paste, URL validation, and routing stack.
        app.copy("hello browser");
        assert_eq!(app.paste().as_deref(), Some("hello browser"));
        assert!(app.open_url("javascript:alert(1)").is_err());
        assert!(app.open_url("file:///etc/passwd").is_err());
        assert_eq!(app.route(), "/");
        app.navigate("docs/intro").unwrap();
        assert_eq!(app.route(), "/docs/intro");
        assert_eq!(app.navigate_back(), "/");
    }

    #[test]
    fn redraw_coalescer_deduplicates_in_flight_requests() {
        let mut gate = super::RedrawCoalescer::default();
        assert!(!gate.is_pending());
        // First request schedules a single animation frame.
        assert!(gate.request());
        assert!(gate.is_pending());
        // Mid-frame input events (e.g. pointer move/release) must not re-request and cancel rAF.
        assert!(!gate.request());
        assert!(!gate.request());
        // Once RedrawRequested fires, the next frame may be scheduled once.
        gate.begin_redraw();
        assert!(!gate.is_pending());
        assert!(gate.request());
        assert!(!gate.request());
    }

    #[test]
    fn window_visibility_commands_hide_and_show() {
        let mut app = App::default();
        app.hide_window();
        app.show_window();
        app.set_window_visible(false);
        app.focus_window();
        assert_eq!(
            app.window_cmds,
            vec![
                WindowCommand::SetVisible(false),
                WindowCommand::SetVisible(true),
                WindowCommand::Focus,
                WindowCommand::SetVisible(false),
                WindowCommand::Focus,
            ]
        );
    }

    #[test]
    fn tray_icon_commands_and_event_queue() {
        let mut app = App::default();
        assert!(!app.has_tray_icon());
        let cfg = super::TrayConfig::new()
            .tooltip("DeltaDrop Sync")
            .item("show", "Show Window")
            .check("pause", "Pause Sync", false)
            .separator()
            .item("quit", "Quit");
        app.set_tray_icon(cfg.clone());
        assert!(app.has_tray_icon());
        app.set_tray_tooltip(Some("Idle"));
        app.update_tray_menu(vec![super::TrayMenuItem::disabled("busy", "Syncing")]);
        app.remove_tray_icon();
        assert!(!app.has_tray_icon());
        assert_eq!(app.take_tray_commands().len(), 4);

        app.push_tray_events(vec![
            super::TrayEvent::Click,
            super::TrayEvent::Menu(String::from("show")),
        ]);
        assert_eq!(
            app.take_tray_events(),
            vec![
                super::TrayEvent::Click,
                super::TrayEvent::Menu(String::from("show")),
            ]
        );
        assert!(app.take_tray_events().is_empty());
    }
}
