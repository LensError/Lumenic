use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{ClipboardEvent, HtmlCanvasElement, HtmlElement, WebGl2RenderingContext};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Ime, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys, WindowExtWebSys};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::gfx::Renderer;
use crate::os::input::{
    map_cursor_icon, map_key, map_modifiers, map_mouse_button, map_scroll_delta,
};
use crate::os::{App, GlBackend, RedrawCoalescer, UserEvent, Waker};
use crate::ui::draw::Role;
use crate::ui::types::{CursorIcon, Decorations, Error, LoopMode, PointerButton, WindowConfig};
use crate::ui::UiContext;

struct WasmWindowState {
    window: Window,
    physical_size: [u32; 2],
    scale_factor: f32,
    last_cursor: CursorIcon,
    active_touches: HashMap<u64, [f32; 2]>,
    last_pinch_dist: Option<f32>,
    last_aria_digest: u64,
    dirty: bool,
    redraw_gate: RedrawCoalescer,
}

impl WasmWindowState {
    fn request_redraw_once(&mut self) {
        if self.redraw_gate.request() {
            self.window.request_redraw();
        }
    }
}

struct WasmAppRunner<F> {
    config: WindowConfig,
    loop_mode: LoopMode,
    update: F,
    ui: UiContext,
    app: App,
    renderer: Renderer,
    gl: Option<GlBackend>,
    proxy: EventLoopProxy<UserEvent>,
    listeners_attached: bool,
    last_frame_ms: Option<f64>,
    windows: HashMap<WindowId, WasmWindowState>,
}

impl<F: FnMut(&mut UiContext, &mut App)> WasmAppRunner<F> {
    fn new(
        config: WindowConfig,
        loop_mode: LoopMode,
        update: F,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Self {
        crate::os::apply_window_config_fonts(&config);
        let mut app = App::default();
        app.set_route_internal(read_browser_route());
        let mut ui = UiContext::new();
        if let Some((w, h, ref rgba)) = config.icon_rgba {
            if let Ok(tex) = app.create_texture_rgba(w, h, rgba) {
                ui.set_window_icon(Some(tex));
            }
        }
        Self {
            config,
            loop_mode,
            update,
            ui,
            app,
            renderer: Renderer::new_fallback_only(),
            gl: None,
            proxy,
            listeners_attached: false,
            last_frame_ms: None,
            windows: HashMap::new(),
        }
    }

    fn init_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Error> {
        let (init_w, init_h) = if self.config.fluid {
            fluid_viewport_logical().unwrap_or((self.config.width, self.config.height))
        } else {
            (self.config.width, self.config.height)
        };
        let attrs = WindowAttributes::default()
            .with_title(&self.config.title)
            .with_inner_size(LogicalSize::new(init_w, init_h))
            .with_decorations(self.config.decorations == Decorations::Native)
            .with_append(true);

        let window = event_loop
            .create_window(attrs)
            .map_err(|e| Error::Window(format!("wasm window creation failed: {e}")))?;
        window.set_ime_allowed(true);

        let canvas: HtmlCanvasElement = window
            .canvas()
            .ok_or_else(|| Error::Window(String::from("missing web canvas")))?;

        if canvas.parent_element().is_none() {
            if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                if let Some(body) = doc.body() {
                    let _ = body.append_child(&canvas);
                }
            }
        }

        if let Some(html_el) = canvas.dyn_ref::<HtmlElement>() {
            let style = html_el.style();
            let _ = style.set_property("touch-action", "none");
            if self.config.fluid {
                let _ = style.set_property("width", "100vw");
                let _ = style.set_property("height", "100dvh");
                let _ = style.set_property("display", "block");
            }
        }

        if !self.listeners_attached {
            self.listeners_attached = true;
            attach_browser_listeners(&self.proxy, self.config.fluid);
        }

        let context_obj = canvas
            .get_context("webgl2")
            .map_err(|_| Error::Graphics(String::from("failed to query webgl2 context")))?
            .ok_or_else(|| Error::Graphics(String::from("WebGL 2 is unavailable")))?;

        let webgl2 = context_obj
            .dyn_into::<WebGl2RenderingContext>()
            .map_err(|_| Error::Graphics(String::from("invalid WebGl2RenderingContext")))?;

        let glow_ctx = glow::Context::from_webgl2_context(webgl2);
        let size = window.inner_size();
        let scale = browser_dpr(window.scale_factor() as f32);
        let (pw, ph) = if self.config.fluid {
            (
                ((init_w as f32) * scale).round().max(1.0) as u32,
                ((init_h as f32) * scale).round().max(1.0) as u32,
            )
        } else if size.width > 0 && size.height > 0 {
            (size.width, size.height)
        } else {
            (
                ((self.config.width as f32) * scale).round().max(1.0) as u32,
                ((self.config.height as f32) * scale).round().max(1.0) as u32,
            )
        };
        if canvas.width() != pw {
            canvas.set_width(pw);
        }
        if canvas.height() != ph {
            canvas.set_height(ph);
        }
        let id = window.id();

        self.gl = Some(GlBackend::new(glow_ctx));
        self.windows.insert(
            id,
            WasmWindowState {
                window,
                physical_size: [pw, ph],
                scale_factor: scale,
                last_cursor: CursorIcon::Default,
                active_touches: HashMap::new(),
                last_pinch_dist: None,
                last_aria_digest: 0,
                dirty: true,
                redraw_gate: RedrawCoalescer::default(),
            },
        );
        Ok(())
    }
}

impl<F: FnMut(&mut UiContext, &mut App)> ApplicationHandler<UserEvent> for WasmAppRunner<F> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.windows.is_empty() {
            if let Err(err) = self.init_window(event_loop) {
                web_sys::console::error_1(&format!("lumenic wasm init failed: {err}").into());
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Wake => {}
            UserEvent::RouteChanged(path) => {
                self.app.set_route_internal(path);
            }
            UserEvent::Paste(text) => {
                crate::os::clipboard::cache_text(&text);
                self.ui.on_ime_commit(text);
            }
            UserEvent::ResizeCanvas => {
                if self.config.fluid {
                    if let Some((lw, lh)) = fluid_viewport_logical() {
                        for win in self.windows.values_mut() {
                            let scale = browser_dpr(win.window.scale_factor() as f32);
                            win.scale_factor = scale;
                            let pw = ((lw as f32) * scale).round().max(1.0) as u32;
                            let ph = ((lh as f32) * scale).round().max(1.0) as u32;
                            win.physical_size = [pw, ph];
                            if let Some(canvas) = win.window.canvas() {
                                if canvas.width() != pw {
                                    canvas.set_width(pw);
                                }
                                if canvas.height() != ph {
                                    canvas.set_height(ph);
                                }
                            }
                            win.dirty = true;
                        }
                    }
                }
            }
        }
        self.ui.mark_dirty();
        for win in self.windows.values_mut() {
            win.dirty = true;
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
                self.windows.remove(&window_id);
                if self.windows.is_empty() {
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(win) = self.windows.get_mut(&window_id) {
                    win.physical_size = [size.width, size.height];
                    if let Some(canvas) = win.window.canvas() {
                        if size.width > 0 && canvas.width() != size.width {
                            canvas.set_width(size.width);
                        }
                        if size.height > 0 && canvas.height() != size.height {
                            canvas.set_height(size.height);
                        }
                    }
                    win.dirty = true;
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(win) = self.windows.get_mut(&window_id) {
                    win.scale_factor = scale_factor as f32;
                    win.dirty = true;
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(win) = self.windows.get_mut(&window_id) {
                    let scale = f64::from(win.scale_factor.max(1.0));
                    if self
                        .ui
                        .on_pointer_move((position.x / scale) as f32, (position.y / scale) as f32)
                    {
                        win.dirty = true;
                    }
                    let next_cursor = self.ui.active_cursor();
                    if next_cursor != win.last_cursor {
                        win.last_cursor = next_cursor;
                        win.window.set_cursor(map_cursor_icon(next_cursor));
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(win) = self.windows.get_mut(&window_id) {
                    if self.ui.on_pointer_leave() {
                        win.dirty = true;
                    }
                    let next_cursor = self.ui.active_cursor();
                    if next_cursor != win.last_cursor {
                        win.last_cursor = next_cursor;
                        win.window.set_cursor(map_cursor_icon(next_cursor));
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(btn) = map_mouse_button(button) {
                    match state {
                        ElementState::Pressed => self.ui.on_pointer_down(btn),
                        ElementState::Released => self.ui.on_pointer_up(btn),
                    }
                    if let Some(win) = self.windows.get_mut(&window_id) {
                        win.dirty = true;
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let scale = self
                    .windows
                    .get(&window_id)
                    .map_or(1.0, |w| w.scale_factor.max(1.0));
                let [dx, dy] = map_scroll_delta(delta, scale);
                self.ui.on_scroll(dx, dy);
                if let Some(win) = self.windows.get_mut(&window_id) {
                    win.dirty = true;
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.app.modifiers = map_modifiers(mods.state());
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    self.ui
                        .on_key_down(map_key(&event.logical_key), self.app.modifiers);
                    if let Some(win) = self.windows.get_mut(&window_id) {
                        win.dirty = true;
                    }
                }
            }
            WindowEvent::Ime(ime) => match ime {
                Ime::Preedit(text, _) => {
                    self.ui.on_ime_preedit(text);
                    if let Some(win) = self.windows.get_mut(&window_id) {
                        win.dirty = true;
                    }
                }
                Ime::Commit(text) => {
                    self.ui.on_ime_commit(text);
                    if let Some(win) = self.windows.get_mut(&window_id) {
                        win.dirty = true;
                    }
                }
                Ime::Enabled | Ime::Disabled => {}
            },
            WindowEvent::HoveredFile(path) => {
                self.app.push_hovered_file(path);
                self.ui.mark_dirty();
                if let Some(win) = self.windows.get_mut(&window_id) {
                    win.dirty = true;
                }
            }
            WindowEvent::HoveredFileCancelled => {
                self.app.clear_hovered_file();
                self.ui.mark_dirty();
                if let Some(win) = self.windows.get_mut(&window_id) {
                    win.dirty = true;
                }
            }
            WindowEvent::DroppedFile(path) => {
                self.app.clear_hovered_file();
                self.app.push_dropped_file(path);
                self.ui.mark_dirty();
                if let Some(win) = self.windows.get_mut(&window_id) {
                    win.dirty = true;
                }
            }
            WindowEvent::Touch(touch) => {
                if let Some(win) = self.windows.get_mut(&window_id) {
                    let scale = f64::from(win.scale_factor.max(1.0));
                    let lx = (touch.location.x / scale) as f32;
                    let ly = (touch.location.y / scale) as f32;
                    match touch.phase {
                        TouchPhase::Started => {
                            win.active_touches.insert(touch.id, [lx, ly]);
                            if win.active_touches.len() == 1 {
                                self.ui.on_pointer_move(lx, ly);
                                self.ui.on_pointer_down(PointerButton::Left);
                            } else if win.active_touches.len() == 2 {
                                win.last_pinch_dist = two_finger_dist(&win.active_touches);
                            }
                        }
                        TouchPhase::Moved => {
                            let prev = win.active_touches.insert(touch.id, [lx, ly]);
                            if win.active_touches.len() == 2 {
                                if let (Some(prev_d), Some(curr_d)) =
                                    (win.last_pinch_dist, two_finger_dist(&win.active_touches))
                                {
                                    let delta = curr_d - prev_d;
                                    if delta.abs() > 0.25 {
                                        self.ui.on_scroll(0.0, delta * 0.4);
                                    }
                                    win.last_pinch_dist = Some(curr_d);
                                }
                            } else if win.active_touches.len() == 1 {
                                if let Some([px, py]) = prev {
                                    let dx = lx - px;
                                    let dy = ly - py;
                                    if dx.abs() > 0.5 || dy.abs() > 0.5 {
                                        self.ui.on_scroll(dx, dy);
                                    }
                                }
                                self.ui.on_pointer_move(lx, ly);
                            }
                        }
                        TouchPhase::Ended | TouchPhase::Cancelled => {
                            let was_single = win.active_touches.len() <= 1;
                            win.active_touches.remove(&touch.id);
                            if win.active_touches.len() < 2 {
                                win.last_pinch_dist = None;
                            }
                            if was_single {
                                self.ui.on_pointer_move(lx, ly);
                                self.ui.on_pointer_up(PointerButton::Left);
                            }
                        }
                    }
                    win.dirty = true;
                }
            }
            WindowEvent::RedrawRequested => {
                let Some(win) = self.windows.get_mut(&window_id) else {
                    return;
                };
                win.redraw_gate.begin_redraw();
                let [pw, ph] = win.physical_size;
                if pw == 0 || ph == 0 {
                    return;
                }
                let Some(gl) = &mut self.gl else {
                    return;
                };
                let scale = win.scale_factor.max(1.0);
                let _ = self.ui.take_dirty();
                let (rm, si, asz, aocc) = self.renderer.perf_snapshot();
                self.ui.update_renderer_perf(rm, si, asz, aocc);
                let (gms, gbytes) = self.renderer.gpu_telemetry();
                self.ui.update_gpu_perf(gms, gbytes);
                let dt = {
                    let now_ms = web_sys::window()
                        .and_then(|w| w.performance())
                        .map(|p| p.now());
                    let step = match (self.last_frame_ms, now_ms) {
                        (Some(prev), Some(now)) if now > prev => {
                            (((now - prev) * 0.001) as f32).clamp(0.001, 0.05)
                        }
                        _ => 1.0 / 60.0,
                    };
                    if now_ms.is_some() {
                        self.last_frame_ms = now_ms;
                    }
                    step
                };
                self.app.begin_step(dt);
                self.ui.set_texture_sizes(self.app.texture_sizes());
                self.ui.step_physics(dt);
                self.ui
                    .begin_frame(pw as f32 / scale, ph as f32 / scale, scale);
                (self.update)(&mut self.ui, &mut self.app);
                self.ui.end_frame_with(&mut self.renderer.glyph_atlas);
                let next_cursor = self.ui.active_cursor();
                if next_cursor != win.last_cursor {
                    win.last_cursor = next_cursor;
                    win.window.set_cursor(map_cursor_icon(next_cursor));
                }
                gl.sync_textures(self.app.take_texture_ops());
                gl.frame_time_s = self.ui.elapsed_time_s() as f32;
                gl.frame_pointer = self.ui.pointer_pos().unwrap_or([0.0, 0.0]);
                self.renderer.render_frame(
                    gl,
                    [pw, ph],
                    scale,
                    self.ui.pointer_pos(),
                    self.ui.theme(),
                    self.ui.draw_list(),
                );
                let (rm, si, asz, aocc) = self.renderer.perf_snapshot();
                self.ui.update_renderer_perf(rm, si, asz, aocc);
                let (gms, gbytes) = self.renderer.gpu_telemetry();
                self.ui.update_gpu_perf(gms, gbytes);
                let aria_digest = semantic_aria_digest(self.ui.semantics());
                if aria_digest != win.last_aria_digest {
                    win.last_aria_digest = aria_digest;
                    sync_aria_dom(self.ui.semantics());
                }
                win.dirty = false;
                if self.ui.has_active_animations()
                    || self.app.take_redraw_request()
                    || matches!(self.loop_mode, LoopMode::FixedTimestep(_))
                {
                    win.request_redraw_once();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let dirty = self.ui.take_dirty()
            || self.ui.has_active_animations()
            || self.app.take_redraw_request();
        let continuous = matches!(self.loop_mode, LoopMode::FixedTimestep(_));
        for win in self.windows.values_mut() {
            if continuous || dirty || win.dirty {
                win.request_redraw_once();
            }
        }
        // On wasm32, `Window::request_redraw()` schedules `requestAnimationFrame`.
        // Always yield via `ControlFlow::Wait` instead of `ControlFlow::Poll`
        // (`scheduler.postTask` / `setTimeout(0)`), which otherwise spins the main thread
        // and repeatedly cancels pending animation frames before vsync can fire.
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

pub fn run_wasm<F>(config: WindowConfig, loop_mode: LoopMode, update: F) -> Result<(), Error>
where
    F: FnMut(&mut UiContext, &mut App) + 'static,
{
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .map_err(|e| Error::Window(format!("wasm event loop init failed: {e}")))?;
    let proxy = event_loop.create_proxy();
    let waker_proxy = proxy.clone();
    let mut runner = WasmAppRunner::new(config, loop_mode, update, proxy);
    runner.app.set_waker(Waker::from_fn(move || {
        let _ = waker_proxy.send_event(UserEvent::Wake);
    }));
    event_loop.spawn_app(runner);
    Ok(())
}

fn browser_dpr(winit_scale: f32) -> f32 {
    let win_dpr = web_sys::window()
        .map(|w| w.device_pixel_ratio() as f32)
        .filter(|d| d.is_finite() && *d > 0.0)
        .unwrap_or(1.0);
    winit_scale.max(win_dpr).max(1.0)
}

fn fluid_viewport_logical() -> Option<(u32, u32)> {
    let win = web_sys::window()?;
    let w = win.inner_width().ok()?.as_f64()? as f32;
    let h = win.inner_height().ok()?.as_f64()? as f32;
    if w > 0.0 && h > 0.0 {
        Some((w.round() as u32, h.round() as u32))
    } else {
        None
    }
}

fn read_browser_route() -> String {
    let Some(win) = web_sys::window() else {
        return String::from("/");
    };
    let loc = win.location();
    if let Ok(hash) = loc.hash() {
        let trimmed = hash.trim_start_matches('#');
        if !trimmed.is_empty() {
            return if trimmed.starts_with('/') {
                trimmed.to_owned()
            } else {
                format!("/{trimmed}")
            };
        }
    }
    if let Ok(path) = loc.pathname() {
        if !path.is_empty() {
            return path;
        }
    }
    String::from("/")
}

fn two_finger_dist(touches: &HashMap<u64, [f32; 2]>) -> Option<f32> {
    let mut iter = touches.values();
    let a = iter.next()?;
    let b = iter.next()?;
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    Some((dx * dx + dy * dy).sqrt())
}

fn attach_browser_listeners(proxy: &EventLoopProxy<UserEvent>, fluid: bool) {
    let Some(win) = web_sys::window() else {
        return;
    };

    if fluid {
        let p = proxy.clone();
        let cb = Closure::wrap(Box::new(move || {
            let _ = p.send_event(UserEvent::ResizeCanvas);
        }) as Box<dyn FnMut()>);
        let _ = win.add_event_listener_with_callback("resize", cb.as_ref().unchecked_ref());
        cb.forget();
    }

    for ev_name in ["popstate", "hashchange"] {
        let p = proxy.clone();
        let cb = Closure::wrap(Box::new(move || {
            let _ = p.send_event(UserEvent::RouteChanged(read_browser_route()));
        }) as Box<dyn FnMut()>);
        let _ = win.add_event_listener_with_callback(ev_name, cb.as_ref().unchecked_ref());
        cb.forget();
    }

    let p = proxy.clone();
    let cb = Closure::wrap(Box::new(move |ev: ClipboardEvent| {
        if let Some(dt) = ev.clipboard_data() {
            if let Ok(text) = dt.get_data("text/plain") {
                if !text.is_empty() {
                    let _ = p.send_event(UserEvent::Paste(text));
                }
            }
        }
    }) as Box<dyn FnMut(ClipboardEvent)>);
    let _ = win.add_event_listener_with_callback("paste", cb.as_ref().unchecked_ref());
    cb.forget();
}

fn semantic_aria_digest(nodes: &[crate::ui::draw::SemanticNode]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    nodes.len().hash(&mut h);
    for n in nodes {
        n.id.0.hash(&mut h);
        (n.role as u8).hash(&mut h);
        n.label.hash(&mut h);
        n.value.hash(&mut h);
        n.focused.hash(&mut h);
        n.enabled.hash(&mut h);
    }
    h.finish()
}

fn sync_aria_dom(nodes: &[crate::ui::draw::SemanticNode]) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(body) = doc.body() else {
        return;
    };
    let root = if let Some(existing) = doc.get_element_by_id("lumenic-aria-root") {
        existing
    } else {
        let Ok(el) = doc.create_element("div") else {
            return;
        };
        let _ = el.set_attribute("id", "lumenic-aria-root");
        let _ = el.set_attribute(
            "style",
            "position:fixed;left:-9999px;top:0;width:1px;height:1px;overflow:hidden;opacity:0;pointer-events:none;",
        );
        let _ = body.append_child(&el);
        el
    };

    root.set_inner_html("");
    for n in nodes.iter().take(256) {
        if n.label.is_empty() && n.value.is_none() {
            continue;
        }
        let Ok(el) = doc.create_element("div") else {
            continue;
        };
        let role = match n.role {
            Role::Window => "application",
            Role::Button => "button",
            Role::Toggle => "switch",
            Role::TextInput => "textbox",
            Role::Slider => "slider",
            Role::Dropdown => "combobox",
            Role::Modal => "dialog",
            Role::Divider => "separator",
            Role::Image => "img",
            Role::Label => "note",
            Role::Container | Role::Viewport | Role::SplitDivider => "region",
        };
        let _ = el.set_attribute("role", role);
        let _ = el.set_attribute("aria-label", &n.label);
        if let Some(ref val) = n.value {
            let _ = el.set_attribute("aria-valuetext", val);
            if n.role == Role::Toggle {
                let _ = el.set_attribute(
                    "aria-checked",
                    if val == "true" || val == "on" {
                        "true"
                    } else {
                        "false"
                    },
                );
            }
        }
        if !n.enabled {
            let _ = el.set_attribute("aria-disabled", "true");
        }
        if n.focused {
            let _ = el.set_attribute("data-focused", "true");
        }
        let _ = root.append_child(&el);
    }
}
