use std::collections::HashMap;
use std::io::{self, BufReader, Write};
use std::net::TcpListener;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use glutin::config::{Config, ConfigTemplateBuilder};
use glutin::context::{
    ContextApi, ContextAttributesBuilder, NotCurrentGlContext, PossiblyCurrentContext, Version,
};
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{Surface, SurfaceAttributesBuilder, SwapInterval, WindowSurface};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Ime, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{ResizeDirection, Window, WindowAttributes, WindowId};

use crate::gfx::inspect::{inspect_to_files, InspectOptions};
use crate::gfx::sdf::SdfBatch;
use crate::gfx::text::GlyphAtlas;
use crate::gfx::Renderer;
use crate::os::input::{
    map_cursor_icon, map_key, map_modifiers, map_mouse_button, map_scroll_delta,
};
use crate::os::{attach_cli_console_if_needed, App, GlBackend, UserEvent, Waker, WindowCommand};
use crate::sys::mcp::{
    execute_mcp_line, read_bounded_line, run_mcp_stdio_sized, McpServer, McpTarget,
    MAX_JSON_RPC_LINE_BYTES,
};
use crate::ui::types::{Decorations, Error, LoopMode, PopoverId, WindowConfig};
use crate::ui::UiContext;

fn apply_window_cmd(
    window: &winit::window::Window,
    cmd: WindowCommand,
    deferred_popovers: &mut Vec<WindowCommand>,
) {
    match cmd {
        WindowCommand::Drag => {
            let _ = window.drag_window();
        }
        WindowCommand::Minimize => {
            window.set_minimized(true);
        }
        WindowCommand::ToggleMaximize => {
            let max = window.is_maximized();
            window.set_maximized(!max);
        }
        WindowCommand::SetVisible(visible) => {
            window.set_visible(visible);
        }
        WindowCommand::Focus => {
            window.set_minimized(false);
            window.focus_window();
        }
        WindowCommand::RequestAttention(level) => {
            let att = level.map(|critical| {
                if critical {
                    winit::window::UserAttentionType::Critical
                } else {
                    winit::window::UserAttentionType::Informational
                }
            });
            window.request_user_attention(att);
        }
        other @ (WindowCommand::OpenPopover { .. } | WindowCommand::ClosePopover(_)) => {
            deferred_popovers.push(other);
        }
    }
}

fn detect_resize_direction(
    lx: f32,
    ly: f32,
    logical_w: f32,
    logical_h: f32,
) -> Option<(ResizeDirection, crate::ui::types::CursorIcon)> {
    const EDGE: f32 = 5.0;
    const CORNER: f32 = 10.0;
    if logical_w < 32.0 || logical_h < 32.0 {
        return None;
    }
    let near_left = (0.0..=EDGE).contains(&lx);
    let near_right = lx <= logical_w && lx >= logical_w - EDGE;
    let near_top = (0.0..=EDGE).contains(&ly);
    let near_bottom = ly <= logical_h && ly >= logical_h - EDGE;
    if !(near_left || near_right || near_top || near_bottom) {
        return None;
    }
    let corner_left = lx <= CORNER;
    let corner_right = lx >= logical_w - CORNER;
    let corner_top = ly <= CORNER;
    let corner_bottom = ly >= logical_h - CORNER;

    use crate::ui::types::CursorIcon;
    match (
        near_top || (corner_top && (near_left || near_right)),
        near_bottom || (corner_bottom && (near_left || near_right)),
        near_left || (corner_left && (near_top || near_bottom)),
        near_right || (corner_right && (near_top || near_bottom)),
    ) {
        (true, false, true, false) => Some((ResizeDirection::NorthWest, CursorIcon::ResizeNwse)),
        (true, false, false, true) => Some((ResizeDirection::NorthEast, CursorIcon::ResizeNesw)),
        (false, true, true, false) => Some((ResizeDirection::SouthWest, CursorIcon::ResizeNesw)),
        (false, true, false, true) => Some((ResizeDirection::SouthEast, CursorIcon::ResizeNwse)),
        (true, false, false, false) => Some((ResizeDirection::North, CursorIcon::ResizeV)),
        (false, true, false, false) => Some((ResizeDirection::South, CursorIcon::ResizeV)),
        (false, false, true, false) => Some((ResizeDirection::West, CursorIcon::ResizeH)),
        (false, false, false, true) => Some((ResizeDirection::East, CursorIcon::ResizeH)),
        _ => None,
    }
}

struct WindowState {
    window: Window,
    surface: Surface<WindowSurface>,
    ui: UiContext,
    physical_size: [u32; 2],
    scale_factor: f32,
    dirty: bool,
    continuous_redraw: bool,
    last_cursor: crate::ui::types::CursorIcon,
    popover_id: Option<PopoverId>,
}

struct OsAppRunner<F> {
    config: WindowConfig,
    loop_mode: LoopMode,
    update: F,
    app: App,
    renderer: Renderer,
    gl_config: Option<Config>,
    gl_context: Option<PossiblyCurrentContext>,
    gl: Option<GlBackend>,
    windows: HashMap<WindowId, WindowState>,
    window_icon_tex: Option<crate::ui::types::TextureId>,
    last_frame: Instant,
    mcp_batch: SdfBatch,
    mcp_last_png: Vec<u8>,
    #[cfg(feature = "tray")]
    tray: Option<crate::os::tray::backend::NativeTray>,
}

impl<F: FnMut(&mut UiContext, &mut App)> OsAppRunner<F> {
    fn new(config: WindowConfig, loop_mode: LoopMode, update: F) -> Self {
        crate::os::apply_window_config_fonts(&config);
        let mut app = App::default();
        let window_icon_tex = config
            .icon_rgba
            .as_ref()
            .and_then(|(w, h, rgba)| app.create_texture_rgba(*w, *h, rgba).ok());
        Self {
            config,
            loop_mode,
            update,
            app,
            renderer: Renderer::new(),
            gl_config: None,
            gl_context: None,
            gl: None,
            windows: HashMap::new(),
            window_icon_tex,
            last_frame: Instant::now(),
            mcp_batch: SdfBatch::default(),
            mcp_last_png: Vec::new(),
            #[cfg(feature = "tray")]
            tray: None,
        }
    }

    #[cfg(feature = "tray")]
    fn init_tray(&mut self, waker: Waker) {
        self.tray = Some(crate::os::tray::backend::NativeTray::new(waker));
    }
}

fn apply_tray_commands(
    #[cfg(feature = "tray")] tray: &mut Option<crate::os::tray::backend::NativeTray>,
    app: &mut App,
) {
    #[cfg(feature = "tray")]
    if let Some(ref mut tray) = tray {
        for tcmd in app.take_tray_commands() {
            tray.apply(tcmd);
        }
    }
    #[cfg(not(feature = "tray"))]
    let _ = app.take_tray_commands();
}

impl<F: FnMut(&mut UiContext, &mut App)> OsAppRunner<F> {
    fn build_window_attrs(config: &WindowConfig) -> WindowAttributes {
        let mut attrs = Window::default_attributes()
            .with_title(&config.title)
            .with_inner_size(LogicalSize::new(config.width, config.height))
            .with_decorations(config.decorations == Decorations::Native)
            .with_transparent(config.transparent)
            .with_resizable(config.resizable);
        if let (Some(mw), Some(mh)) = (config.min_width, config.min_height) {
            attrs = attrs.with_min_inner_size(LogicalSize::new(mw, mh));
        }
        if let Some((w, h, ref rgba)) = config.icon_rgba {
            if let Ok(icon) = winit::window::Icon::from_rgba(rgba.clone(), w, h) {
                attrs = attrs.with_window_icon(Some(icon));
            }
        }
        attrs
    }

    fn create_initial_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Error> {
        let template = ConfigTemplateBuilder::new().with_alpha_size(8);
        let display_builder = DisplayBuilder::new()
            .with_window_attributes(Some(Self::build_window_attrs(&self.config)));

        let (window_opt, gl_config) = display_builder
            .build(event_loop, template, |configs| {
                configs
                    .reduce(|accum, cfg| {
                        if cfg.num_samples() < accum.num_samples() {
                            cfg
                        } else {
                            accum
                        }
                    })
                    .expect("no GL config returned")
            })
            .map_err(|e| Error::Graphics(format!("GL display setup failed: {e}")))?;

        let window =
            window_opt.ok_or_else(|| Error::Window(String::from("failed to build window")))?;
        window.set_ime_allowed(true);
        let raw_window_handle = window
            .window_handle()
            .map_err(|e| Error::Window(format!("missing raw window handle: {e}")))?
            .as_raw();
        apply_platform_window_effects(
            raw_window_handle,
            self.config.decorations,
            self.config.transparent,
        );

        let gl_display = gl_config.display();

        let core_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
            .build(Some(raw_window_handle));
        let gles_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::Gles(Some(Version::new(3, 0))))
            .build(Some(raw_window_handle));

        // SAFETY: raw_window_handle remains valid for the lifetime of `window`, and gl_config was
        // returned from the matching glutin DisplayBuilder for this event loop.
        let not_current = unsafe {
            gl_display
                .create_context(&gl_config, &core_attrs)
                .or_else(|_| gl_display.create_context(&gl_config, &gles_attrs))
        }
        .map_err(|e| {
            Error::Graphics(format!(
                "OpenGL 3.3 / GLES 3.0 context creation failed: {e}"
            ))
        })?;

        let surface_attrs = window
            .build_surface_attributes(SurfaceAttributesBuilder::<WindowSurface>::new())
            .map_err(|e| Error::Graphics(format!("surface attributes failed: {e}")))?;

        // SAFETY: `gl_config` and `surface_attrs` belong to `gl_display` and the live `window`.
        let surface = unsafe { gl_display.create_window_surface(&gl_config, &surface_attrs) }
            .map_err(|e| Error::Graphics(format!("window surface creation failed: {e}")))?;

        let gl_context = not_current
            .make_current(&surface)
            .map_err(|e| Error::Graphics(format!("make_current failed: {e}")))?;

        let swap_interval = if self.config.vsync {
            SwapInterval::Wait(NonZeroU32::new(1).expect("nonzero"))
        } else {
            SwapInterval::DontWait
        };
        let _ = surface.set_swap_interval(&gl_context, swap_interval);

        // SAFETY: `gl_context` is current on this thread and `gl_display` resolves valid GL entrypoints.
        let glow_ctx = unsafe {
            glow::Context::from_loader_function_cstr(|sym| gl_display.get_proc_address(sym))
        };

        let physical = window.inner_size();
        let scale = window.scale_factor() as f32;
        let id = window.id();

        self.gl_config = Some(gl_config);
        self.gl_context = Some(gl_context);
        self.gl = Some(GlBackend::new(glow_ctx));
        let mut ui = UiContext::new();
        ui.set_window_icon(self.window_icon_tex);
        self.windows.insert(
            id,
            WindowState {
                window,
                surface,
                ui,
                physical_size: [physical.width, physical.height],
                scale_factor: scale,
                dirty: true,
                continuous_redraw: false,
                last_cursor: crate::ui::types::CursorIcon::Default,
                popover_id: None,
            },
        );
        Ok(())
    }

    fn create_popover_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        popover_id: PopoverId,
        config: &WindowConfig,
    ) -> Result<(), Error> {
        if self
            .windows
            .values()
            .any(|w| w.popover_id == Some(popover_id))
        {
            return Ok(());
        }
        let (Some(gl_config), Some(gl_context)) = (&self.gl_config, &self.gl_context) else {
            return Err(Error::Graphics(String::from(
                "cannot open popover before primary GL context is ready",
            )));
        };

        let window = event_loop
            .create_window(Self::build_window_attrs(config))
            .map_err(|e| Error::Window(format!("popover window creation failed: {e}")))?;
        window.set_ime_allowed(true);
        if let Ok(raw) = window.window_handle() {
            apply_platform_window_effects(raw.as_raw(), config.decorations, config.transparent);
        }

        let surface_attrs = window
            .build_surface_attributes(SurfaceAttributesBuilder::<WindowSurface>::new())
            .map_err(|e| Error::Graphics(format!("popover surface attributes failed: {e}")))?;

        let gl_display = gl_config.display();
        // SAFETY: `gl_config` and `surface_attrs` belong to `gl_display` and the live popover `window`.
        let surface = unsafe { gl_display.create_window_surface(gl_config, &surface_attrs) }
            .map_err(|e| Error::Graphics(format!("popover surface creation failed: {e}")))?;

        if gl_context.make_current(&surface).is_ok() {
            let swap_interval = if config.vsync {
                SwapInterval::Wait(NonZeroU32::new(1).expect("nonzero"))
            } else {
                SwapInterval::DontWait
            };
            let _ = surface.set_swap_interval(gl_context, swap_interval);
        }

        let physical = window.inner_size();
        let scale = window.scale_factor() as f32;
        let id = window.id();
        self.windows.insert(
            id,
            WindowState {
                window,
                surface,
                ui: UiContext::new(),
                physical_size: [physical.width, physical.height],
                scale_factor: scale,
                dirty: true,
                continuous_redraw: false,
                last_cursor: crate::ui::types::CursorIcon::Default,
                popover_id: Some(popover_id),
            },
        );
        Ok(())
    }

    fn render_window(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId) {
        let mut deferred_popovers = Vec::new();
        {
            let Some(win) = self.windows.get_mut(&window_id) else {
                return;
            };
            let [pw, ph] = win.physical_size;
            if pw == 0 || ph == 0 {
                return;
            }
            let (Some(ctx), Some(gl)) = (&self.gl_context, &mut self.gl) else {
                return;
            };

            if ctx.make_current(&win.surface).is_err() {
                return;
            }

            let now = Instant::now();
            let raw_dt = now.duration_since(self.last_frame).as_secs_f32();
            self.last_frame = now;
            let dt = if win.continuous_redraw {
                raw_dt.clamp(0.0001, 0.05)
            } else {
                raw_dt.clamp(0.0001, 1.0 / 60.0)
            };
            self.app.begin_step(dt);
            self.app.set_active_popover(win.popover_id);

            let scale = if win.scale_factor > 0.0 {
                win.scale_factor
            } else {
                1.0
            };
            let logical_w = pw as f32 / scale;
            let logical_h = ph as f32 / scale;

            let _ = win.ui.take_dirty();
            win.ui.set_viewport_fbo_id(gl.viewport_fbo_id());
            win.ui.set_texture_sizes(self.app.texture_sizes());
            let (rm, si, asz, aocc) = self.renderer.perf_snapshot();
            win.ui.update_renderer_perf(rm, si, asz, aocc);
            let (gms, gbytes) = self.renderer.gpu_telemetry();
            win.ui.update_gpu_perf(gms, gbytes);
            win.ui.step_physics(dt);
            win.ui.begin_frame(logical_w, logical_h, scale);
            (self.update)(&mut win.ui, &mut self.app);
            win.ui.end_frame_with(&mut self.renderer.glyph_atlas);
            let next_cursor = win.ui.active_cursor();
            if next_cursor != win.last_cursor {
                win.last_cursor = next_cursor;
                win.window.set_cursor(map_cursor_icon(next_cursor));
            }
            self.app.set_active_popover(None);

            for cmd in self.app.take_window_commands() {
                apply_window_cmd(&win.window, cmd, &mut deferred_popovers);
            }
            apply_tray_commands(
                #[cfg(feature = "tray")]
                &mut self.tray,
                &mut self.app,
            );

            gl.sync_textures(self.app.take_texture_ops());
            gl.frame_time_s = win.ui.elapsed_time_s() as f32;
            gl.frame_pointer = win.ui.pointer_pos().unwrap_or([0.0, 0.0]);
            self.renderer.render_frame(
                gl,
                [pw, ph],
                scale,
                win.ui.pointer_pos(),
                win.ui.theme(),
                win.ui.draw_list(),
            );
            let (rm, si, asz, aocc) = self.renderer.perf_snapshot();
            win.ui.update_renderer_perf(rm, si, asz, aocc);
            let (gms, gbytes) = self.renderer.gpu_telemetry();
            win.ui.update_gpu_perf(gms, gbytes);

            let _ = win.surface.swap_buffers(ctx);
            win.dirty = false;
            let anim_or_req = win.ui.has_active_animations() || self.app.take_redraw_request();
            let fixed_step = matches!(self.loop_mode, LoopMode::FixedTimestep(_));
            win.continuous_redraw = anim_or_req || fixed_step;
            // In `FixedTimestep`, let `about_to_wait`'s `ControlFlow::WaitUntil` pace the next
            // redraw instead of immediately re-queuing every swap.
            if anim_or_req && !fixed_step {
                win.window.request_redraw();
            }
        }

        for cmd in deferred_popovers {
            match cmd {
                WindowCommand::OpenPopover { id, config } => {
                    let _ = self.create_popover_window(event_loop, id, &config);
                }
                WindowCommand::ClosePopover(id) => {
                    self.windows.retain(|_, w| w.popover_id != Some(id));
                    self.app.mark_popover_closed(id);
                    for w in self.windows.values_mut() {
                        w.ui.mark_dirty();
                        w.dirty = true;
                    }
                }
                WindowCommand::Drag
                | WindowCommand::Minimize
                | WindowCommand::ToggleMaximize
                | WindowCommand::SetVisible(_)
                | WindowCommand::Focus
                | WindowCommand::RequestAttention(_) => {}
            }
        }
    }

    fn handle_live_mcp(&mut self, event_loop: &ActiveEventLoop, line: &str) -> String {
        if self.windows.is_empty() {
            if let Err(err) = self.create_initial_window(event_loop) {
                return format!(
                    "{{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{{\"code\":-32603,\"message\":\"{}\"}}}}",
                    err
                );
            }
        }

        let Some(win_id) = self
            .windows
            .iter()
            .find(|(_, w)| w.popover_id.is_none())
            .or_else(|| self.windows.iter().next())
            .map(|(id, _)| *id)
        else {
            return String::from(
                "{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{\"code\":-32603,\"message\":\"no active window\"}}",
            );
        };

        if self.app.frame_count() == 0 {
            self.render_window(event_loop, win_id);
        }

        let mut deferred_popovers = Vec::new();
        let response = {
            let Some(win) = self.windows.get_mut(&win_id) else {
                return String::from(
                    "{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{\"code\":-32603,\"message\":\"window closed\"}}",
                );
            };
            let [pw, ph] = win.physical_size;
            let width = pw.max(1);
            let height = ph.max(1);
            let scale = if win.scale_factor > 0.0 {
                win.scale_factor
            } else {
                1.0
            };
            let logical_w = width as f32 / scale;
            let logical_h = height as f32 / scale;
            let update_fn = &mut self.update;
            let mut stepped = false;

            let resp = {
                let mut target = McpTarget {
                    ui: &mut win.ui,
                    app: &mut self.app,
                    atlas: &mut self.renderer.glyph_atlas,
                    batch: &mut self.mcp_batch,
                    width,
                    height,
                    scale,
                    last_png: &mut self.mcp_last_png,
                };
                let mut step_fn =
                    |ui: &mut UiContext, app: &mut App, atlas: &mut GlyphAtlas, dt: f32| {
                        stepped = true;
                        app.begin_step(dt);
                        ui.set_texture_sizes(app.texture_sizes());
                        ui.step_physics(dt);
                        ui.begin_frame(logical_w, logical_h, scale);
                        (update_fn)(ui, app);
                        ui.end_frame_with(atlas);
                    };

                execute_mcp_line(line, &mut target, &mut step_fn)
            };

            if stepped {
                self.last_frame = Instant::now();
                for cmd in self.app.take_window_commands() {
                    apply_window_cmd(&win.window, cmd, &mut deferred_popovers);
                }
                apply_tray_commands(
                    #[cfg(feature = "tray")]
                    &mut self.tray,
                    &mut self.app,
                );

                if pw > 0 && ph > 0 {
                    if let (Some(ctx), Some(gl)) = (&self.gl_context, &mut self.gl) {
                        if ctx.make_current(&win.surface).is_ok() {
                            gl.sync_textures(self.app.take_texture_ops());
                            self.renderer.render_frame(
                                gl,
                                [pw, ph],
                                scale,
                                win.ui.pointer_pos(),
                                win.ui.theme(),
                                win.ui.draw_list(),
                            );
                            let (rm, si, asz, aocc) = self.renderer.perf_snapshot();
                            win.ui.update_renderer_perf(rm, si, asz, aocc);
                            let _ = win.surface.swap_buffers(ctx);
                            win.dirty = false;
                            let keep_going = win.ui.has_active_animations()
                                || self.app.take_redraw_request()
                                || matches!(self.loop_mode, LoopMode::FixedTimestep(_));
                            win.continuous_redraw = keep_going;
                            if keep_going {
                                win.window.request_redraw();
                            }
                        }
                    }
                }
            }
            resp
        };

        for cmd in deferred_popovers {
            match cmd {
                WindowCommand::OpenPopover { id, config } => {
                    let _ = self.create_popover_window(event_loop, id, &config);
                }
                WindowCommand::ClosePopover(id) => {
                    self.windows.retain(|_, w| w.popover_id != Some(id));
                    self.app.mark_popover_closed(id);
                    for w in self.windows.values_mut() {
                        w.ui.mark_dirty();
                        w.dirty = true;
                    }
                }
                WindowCommand::Drag
                | WindowCommand::Minimize
                | WindowCommand::ToggleMaximize
                | WindowCommand::SetVisible(_)
                | WindowCommand::Focus
                | WindowCommand::RequestAttention(_) => {}
            }
        }

        if self.app.is_exit_requested() {
            event_loop.exit();
        }

        response
    }
}

impl<F: FnMut(&mut UiContext, &mut App)> ApplicationHandler<UserEvent> for OsAppRunner<F> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.windows.is_empty() {
            if let Err(err) = self.create_initial_window(event_loop) {
                eprintln!("lumenic startup error: {err}");
                event_loop.exit();
                return;
            }
            let ids: Vec<WindowId> = self.windows.keys().copied().collect();
            for id in ids {
                self.render_window(event_loop, id);
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Wake => {
                #[cfg(feature = "tray")]
                if let Some(ref tray) = self.tray {
                    let evs = tray.take_events();
                    if !evs.is_empty() {
                        self.app.push_tray_events(evs);
                    }
                }
                let mut hidden_main_id: Option<WindowId> = None;
                for (&wid, win) in &mut self.windows {
                    win.ui.mark_dirty();
                    win.dirty = true;
                    if win.popover_id.is_none() && !win.window.is_visible().unwrap_or(true) {
                        hidden_main_id = Some(wid);
                    }
                }
                // Hidden windows do not receive OS RedrawRequested events, so tick the
                // callback directly on Wake so tray-icon restore/exit actions execute immediately.
                if let Some(wid) = hidden_main_id {
                    let mut deferred_popovers = Vec::new();
                    if let Some(win) = self.windows.get_mut(&wid) {
                        let scale = win.scale_factor.max(1.0);
                        let lw = (win.physical_size[0] as f32 / scale).max(1.0);
                        let lh = (win.physical_size[1] as f32 / scale).max(1.0);
                        self.app.set_active_popover(None);
                        self.app.begin_step(0.0);
                        win.ui.begin_frame(lw, lh, scale);
                        (self.update)(&mut win.ui, &mut self.app);
                        win.ui.end_frame();
                        for cmd in self.app.take_window_commands() {
                            apply_window_cmd(&win.window, cmd, &mut deferred_popovers);
                        }
                        if win.window.is_visible().unwrap_or(true) {
                            win.window.request_redraw();
                        }
                    }
                    apply_tray_commands(
                        #[cfg(feature = "tray")]
                        &mut self.tray,
                        &mut self.app,
                    );
                    if self.app.is_exit_requested() {
                        event_loop.exit();
                    }
                }
            }
            UserEvent::McpRequest { line, reply } => {
                let resp = self.handle_live_mcp(event_loop, &line);
                let _ = reply.send(resp);
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                let closed_pop = self.windows.remove(&window_id).and_then(|w| w.popover_id);
                if let Some(pop_id) = closed_pop {
                    self.app.mark_popover_closed(pop_id);
                    for win in self.windows.values_mut() {
                        win.ui.mark_dirty();
                        win.dirty = true;
                    }
                    if self.windows.is_empty() {
                        event_loop.exit();
                    }
                } else {
                    self.windows.clear();
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(win) = self.windows.get_mut(&window_id) {
                    win.physical_size = [size.width, size.height];
                    if let (Some(ctx), Some(w), Some(h)) = (
                        &self.gl_context,
                        NonZeroU32::new(size.width),
                        NonZeroU32::new(size.height),
                    ) {
                        if ctx.make_current(&win.surface).is_ok() {
                            win.surface.resize(ctx, w, h);
                        }
                        win.dirty = true;
                        win.ui.mark_dirty();
                    }
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(win) = self.windows.get_mut(&window_id) {
                    win.scale_factor = scale_factor as f32;
                    win.dirty = true;
                    win.ui.mark_dirty();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let custom_resizable =
                    self.config.decorations == Decorations::Custom && self.config.resizable;
                if let Some(win) = self.windows.get_mut(&window_id) {
                    let scale = f64::from(win.scale_factor.max(1.0));
                    let lx = (position.x / scale) as f32;
                    let ly = (position.y / scale) as f32;
                    let lw = (win.physical_size[0] as f64 / scale) as f32;
                    let lh = (win.physical_size[1] as f64 / scale) as f32;
                    if win.ui.on_pointer_move(lx, ly) {
                        win.dirty = true;
                    }
                    let resize_cursor = if custom_resizable && !win.window.is_maximized() {
                        detect_resize_direction(lx, ly, lw, lh).map(|(_, c)| c)
                    } else {
                        None
                    };
                    let next_cursor = resize_cursor.unwrap_or_else(|| win.ui.active_cursor());
                    if next_cursor != win.last_cursor {
                        win.last_cursor = next_cursor;
                        win.window.set_cursor(map_cursor_icon(next_cursor));
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(win) = self.windows.get_mut(&window_id) {
                    if win.ui.on_pointer_leave() {
                        win.dirty = true;
                    }
                    let next_cursor = win.ui.active_cursor();
                    if next_cursor != win.last_cursor {
                        win.last_cursor = next_cursor;
                        win.window.set_cursor(map_cursor_icon(next_cursor));
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(btn) = map_mouse_button(button) {
                    let custom_resizable =
                        self.config.decorations == Decorations::Custom && self.config.resizable;
                    if let Some(win) = self.windows.get_mut(&window_id) {
                        if state == ElementState::Pressed
                            && btn == crate::ui::types::PointerButton::Left
                            && custom_resizable
                            && !win.window.is_maximized()
                        {
                            if let Some([lx, ly]) = win.ui.pointer_pos() {
                                let scale = win.scale_factor.max(1.0);
                                let lw = win.physical_size[0] as f32 / scale;
                                let lh = win.physical_size[1] as f32 / scale;
                                if let Some((dir, _)) = detect_resize_direction(lx, ly, lw, lh) {
                                    let _ = win.window.drag_resize_window(dir);
                                    return;
                                }
                            }
                        }
                        match state {
                            ElementState::Pressed => win.ui.on_pointer_down(btn),
                            ElementState::Released => win.ui.on_pointer_up(btn),
                        }
                        win.dirty = true;
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(win) = self.windows.get_mut(&window_id) {
                    let scale = win.scale_factor.max(1.0);
                    let [dx, dy] = map_scroll_delta(delta, scale);
                    win.ui.on_scroll(dx, dy);
                    win.dirty = true;
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.app.modifiers = map_modifiers(mods.state());
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    let key = map_key(&event.logical_key);
                    if let Some(win) = self.windows.get_mut(&window_id) {
                        win.ui.on_key_down(key, self.app.modifiers);
                        win.dirty = true;
                    }
                }
            }
            WindowEvent::Ime(ime) => match ime {
                Ime::Preedit(text, _) => {
                    if let Some(win) = self.windows.get_mut(&window_id) {
                        win.ui.on_ime_preedit(text);
                        win.dirty = true;
                    }
                }
                Ime::Commit(text) => {
                    if let Some(win) = self.windows.get_mut(&window_id) {
                        win.ui.on_ime_commit(text);
                        win.dirty = true;
                    }
                }
                Ime::Enabled | Ime::Disabled => {}
            },
            WindowEvent::HoveredFile(path) => {
                self.app.push_hovered_file(path);
                if let Some(win) = self.windows.get_mut(&window_id) {
                    win.ui.mark_dirty();
                    win.dirty = true;
                }
            }
            WindowEvent::HoveredFileCancelled => {
                self.app.clear_hovered_file();
                if let Some(win) = self.windows.get_mut(&window_id) {
                    win.ui.mark_dirty();
                    win.dirty = true;
                }
            }
            WindowEvent::DroppedFile(path) => {
                self.app.clear_hovered_file();
                self.app.push_dropped_file(path);
                if let Some(win) = self.windows.get_mut(&window_id) {
                    win.ui.mark_dirty();
                    win.dirty = true;
                }
            }
            WindowEvent::RedrawRequested => {
                self.render_window(event_loop, window_id);
                if self.app.is_exit_requested() {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.app.is_exit_requested() {
            event_loop.exit();
            return;
        }

        let redraw_requested = self.app.take_redraw_request();
        match self.loop_mode {
            LoopMode::Idle => {
                for win in self.windows.values_mut() {
                    let ui_dirty = win.ui.take_dirty();
                    let anim_active = win.ui.has_active_animations();
                    if ui_dirty || anim_active || redraw_requested || win.dirty {
                        win.window.request_redraw();
                    }
                }
                event_loop.set_control_flow(ControlFlow::Wait);
            }
            LoopMode::FixedTimestep(hz) => {
                let hz = hz.max(1);
                let step = Duration::from_secs_f64(1.0 / f64::from(hz));
                let elapsed = self.last_frame.elapsed() >= step;
                for win in self.windows.values_mut() {
                    let ui_dirty = win.ui.take_dirty();
                    if elapsed || ui_dirty || redraw_requested || win.dirty {
                        win.window.request_redraw();
                    }
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(self.last_frame + step));
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn apply_platform_window_effects(
    raw: RawWindowHandle,
    decorations: Decorations,
    transparent: bool,
) {
    let RawWindowHandle::Win32(handle) = raw else {
        return;
    };
    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWCP_ROUND: u32 = 2;
    const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;
    const DWMSBT_TRANSIENTWINDOW: u32 = 3;

    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: isize,
            dw_attribute: u32,
            pv_attribute: *const core::ffi::c_void,
            cb_attribute: u32,
        ) -> i32;
    }

    if decorations == Decorations::Custom {
        let preference: u32 = DWMWCP_ROUND;
        // SAFETY: `handle.hwnd` is a live Win32 `HWND` owned by the newly created `winit` window,
        // and `preference` is a valid 4-byte `u32` stack value matching `DWMWA_WINDOW_CORNER_PREFERENCE`.
        unsafe {
            let _ = DwmSetWindowAttribute(
                handle.hwnd.get(),
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&preference as *const u32).cast::<core::ffi::c_void>(),
                4,
            );
        }
    }
    if transparent {
        let backdrop: u32 = DWMSBT_TRANSIENTWINDOW;
        // SAFETY: `handle.hwnd` is a live Win32 `HWND` and `backdrop` is a valid 4-byte `u32` stack value.
        unsafe {
            let _ = DwmSetWindowAttribute(
                handle.hwnd.get(),
                DWMWA_SYSTEMBACKDROP_TYPE,
                (&backdrop as *const u32).cast::<core::ffi::c_void>(),
                4,
            );
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn apply_platform_window_effects(
    _raw: RawWindowHandle,
    _decorations: Decorations,
    _transparent: bool,
) {
}

fn spawn_live_stdio_mcp_thread(proxy: EventLoopProxy<UserEvent>, exit_on_eof: bool) {
    let _ = std::thread::Builder::new()
        .name(String::from("lumenic-mcp-stdio"))
        .spawn(move || {
            let stdin = io::stdin();
            let stdout = io::stdout();
            let mut reader = BufReader::new(stdin.lock());
            let mut writer = stdout.lock();
            let mut buf = Vec::with_capacity(4096);

            loop {
                match read_bounded_line(&mut reader, &mut buf, MAX_JSON_RPC_LINE_BYTES) {
                    Ok(None) => {
                        if exit_on_eof {
                            let (tx, rx) = mpsc::channel();
                            let _ = proxy.send_event(UserEvent::McpRequest {
                                line: String::from(
                                    "{\"jsonrpc\":\"2.0\",\"id\":0,\"method\":\"ui_close\"}",
                                ),
                                reply: tx,
                            });
                            let _ = rx.recv();
                        }
                        break;
                    }
                    Ok(Some(line)) => {
                        if line.trim().is_empty() {
                            continue;
                        }
                        let (tx, rx) = mpsc::channel();
                        if proxy
                            .send_event(UserEvent::McpRequest { line, reply: tx })
                            .is_err()
                        {
                            break;
                        }
                        let Ok(resp) = rx.recv() else {
                            break;
                        };
                        if writeln!(writer, "{resp}").and_then(|_| writer.flush()).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = writeln!(
                            writer,
                            "{{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{{\"code\":-32700,\"message\":\"{}\"}}}}",
                            e
                        )
                        .and_then(|_| writer.flush());
                    }
                }
            }
        });
}

fn spawn_live_tcp_mcp_thread(listener: TcpListener, proxy: EventLoopProxy<UserEvent>) {
    let _ = std::thread::Builder::new()
        .name(String::from("lumenic-mcp-tcp"))
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let Ok(write_half) = stream.try_clone() else {
                    continue;
                };
                let mut reader = BufReader::new(stream);
                let mut writer = write_half;
                let mut buf = Vec::with_capacity(4096);
                loop {
                    match read_bounded_line(&mut reader, &mut buf, MAX_JSON_RPC_LINE_BYTES) {
                        Ok(None) => break,
                        Ok(Some(line)) => {
                            if line.trim().is_empty() {
                                continue;
                            }
                            let (tx, rx) = mpsc::channel();
                            if proxy
                                .send_event(UserEvent::McpRequest { line, reply: tx })
                                .is_err()
                            {
                                return;
                            }
                            let Ok(resp) = rx.recv() else {
                                return;
                            };
                            if writeln!(writer, "{resp}").and_then(|_| writer.flush()).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            if writeln!(
                                writer,
                                "{{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{{\"code\":-32700,\"message\":\"{}\"}}}}",
                                e
                            )
                            .and_then(|_| writer.flush())
                            .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
            }
        });
}

fn parse_mcp_port(args: &[String]) -> Result<Option<u16>, Error> {
    let mut i = 1;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--mcp-port" {
            let val = args.get(i + 1).ok_or_else(|| {
                Error::InvalidInput(String::from("--mcp-port requires a port number"))
            })?;
            let port = val
                .parse::<u16>()
                .map_err(|_| Error::InvalidInput(format!("invalid --mcp-port '{val}'")))?;
            return Ok(Some(port));
        }
        if let Some(rest) = a.strip_prefix("--mcp-port=") {
            let port = rest
                .parse::<u16>()
                .map_err(|_| Error::InvalidInput(format!("invalid --mcp-port '{rest}'")))?;
            return Ok(Some(port));
        }
        i += 1;
    }
    Ok(None)
}

pub fn run_native<F>(config: WindowConfig, loop_mode: LoopMode, update: F) -> Result<(), Error>
where
    F: FnMut(&mut UiContext, &mut App) + 'static,
{
    run_native_borrowed(config, loop_mode, update)
}

pub fn run_native_borrowed<F>(
    config: WindowConfig,
    loop_mode: LoopMode,
    mut update: F,
) -> Result<(), Error>
where
    F: FnMut(&mut UiContext, &mut App),
{
    crate::os::apply_window_config_fonts(&config);
    let args: Vec<String> = std::env::args().collect();
    attach_cli_console_if_needed(&args);

    if args.iter().any(|a| a == "--headless-smoke") {
        let mut srv = McpServer::new(config.width.max(1), config.height.max(1), 1.0);
        let _ = srv.handle_line(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ui_step_frames\",\"params\":{\"count\":2}}",
            &mut update,
        );
        return Ok(());
    }

    if args.iter().any(|a| a == "--inspect") {
        let mut opts = InspectOptions {
            width: config.width.max(1),
            height: config.height.max(1),
            scale_factor: 1.0,
            ..Default::default()
        };
        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--out-png" if i + 1 < args.len() => {
                    opts.out_png = PathBuf::from(&args[i + 1]);
                    i += 2;
                }
                "--out-json" if i + 1 < args.len() => {
                    opts.out_json = PathBuf::from(&args[i + 1]);
                    i += 2;
                }
                _ => i += 1,
            }
        }
        return inspect_to_files(&opts, update);
    }

    let mcp_port = parse_mcp_port(&args)?;
    let has_mcp = args.iter().any(|a| a == "--mcp");
    let has_live = args.iter().any(|a| a == "--mcp-live" || a == "--headed");
    let exit_on_eof = args.iter().any(|a| a == "--mcp-exit-on-eof");

    if has_mcp && !has_live && mcp_port.is_none() {
        return run_mcp_stdio_sized(config.width.max(1), config.height.max(1), &mut update);
    }

    let tcp_listener = if let Some(port) = mcp_port {
        Some(
            TcpListener::bind(("127.0.0.1", port))
                .map_err(|e| Error::Io(format!("failed to bind 127.0.0.1:{port}: {e}")))?,
        )
    } else {
        None
    };

    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .map_err(|e| Error::Window(format!("event loop init failed: {e}")))?;
    let proxy = event_loop.create_proxy();

    if has_live {
        spawn_live_stdio_mcp_thread(proxy.clone(), exit_on_eof);
    }
    if let Some(listener) = tcp_listener {
        spawn_live_tcp_mcp_thread(listener, proxy.clone());
    }

    let mut runner = OsAppRunner::new(config, loop_mode, update);
    let waker = Waker::from_fn(move || {
        let _ = proxy.send_event(UserEvent::Wake);
    });
    runner.app.set_waker(waker.clone());
    #[cfg(feature = "tray")]
    runner.init_tray(waker);
    event_loop
        .run_app(&mut runner)
        .map_err(|e| Error::Window(format!("event loop exited with error: {e}")))
}

#[cfg(test)]
mod tests {
    use super::detect_resize_direction;
    use crate::ui::types::CursorIcon;
    use winit::window::ResizeDirection;

    #[test]
    fn frameless_resize_edge_and_corner_detection() {
        let w = 800.0;
        let h = 600.0;
        assert_eq!(
            detect_resize_direction(2.0, 2.0, w, h),
            Some((ResizeDirection::NorthWest, CursorIcon::ResizeNwse))
        );
        assert_eq!(
            detect_resize_direction(798.0, 2.0, w, h),
            Some((ResizeDirection::NorthEast, CursorIcon::ResizeNesw))
        );
        assert_eq!(
            detect_resize_direction(2.0, 598.0, w, h),
            Some((ResizeDirection::SouthWest, CursorIcon::ResizeNesw))
        );
        assert_eq!(
            detect_resize_direction(798.0, 598.0, w, h),
            Some((ResizeDirection::SouthEast, CursorIcon::ResizeNwse))
        );
        assert_eq!(
            detect_resize_direction(400.0, 1.0, w, h),
            Some((ResizeDirection::North, CursorIcon::ResizeV))
        );
        assert_eq!(
            detect_resize_direction(400.0, 599.0, w, h),
            Some((ResizeDirection::South, CursorIcon::ResizeV))
        );
        assert_eq!(
            detect_resize_direction(1.0, 300.0, w, h),
            Some((ResizeDirection::West, CursorIcon::ResizeH))
        );
        assert_eq!(
            detect_resize_direction(799.0, 300.0, w, h),
            Some((ResizeDirection::East, CursorIcon::ResizeH))
        );
        // Interior and CSD titlebar buttons (>= 6px from edges) do not trigger resize.
        assert_eq!(detect_resize_direction(400.0, 300.0, w, h), None);
        assert_eq!(detect_resize_direction(770.0, 12.0, w, h), None);
    }
}
