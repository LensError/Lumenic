//! Native headless/offscreen OpenGL 3.3 Core GPU surface host (no `winit` event loop).
//!
//! Designed for embedded plugin hosts (game overlays, Unity/Unreal native `cdylib`s, and
//! multi-surface offscreen UI rendering) with deterministic resource teardown in `Drop`.

use std::collections::HashMap;
use std::marker::PhantomData;

use crate::gfx::Renderer;
use crate::os::{App, GlBackend, OffscreenTarget};
use crate::ui::types::{Color, Error};
use crate::ui::UiContext;

/// Opaque handle identifying an offscreen GPU surface owned by an [`OffscreenHost`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OffscreenSurfaceId(u64);

/// Configuration used when creating an [`OffscreenSurface`] on an [`OffscreenHost`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OffscreenSurfaceConfig {
    /// Physical framebuffer width in pixels (must be non-zero).
    pub width: u32,
    /// Physical framebuffer height in pixels (must be non-zero).
    pub height: u32,
    /// DPI scale factor mapping logical points to physical pixels (defaults to `1.0`).
    pub scale_factor: f32,
    /// Background clear color for the surface FBO (defaults to [`Color::TRANSPARENT`]).
    pub clear_color: Color,
}

impl OffscreenSurfaceConfig {
    /// Creates a new offscreen surface configuration with `scale_factor = 1.0` and transparent clear color.
    pub const fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            scale_factor: 1.0,
            clear_color: Color::TRANSPARENT,
        }
    }

    /// Sets the DPI scale factor (non-finite or non-positive values fall back to `1.0`).
    pub const fn scale_factor(mut self, scale_factor: f32) -> Self {
        self.scale_factor = scale_factor;
        self
    }

    /// Sets the FBO clear color (use [`Color::TRANSPARENT`] for floating overlays).
    ///
    /// Note: Drawing translucent or anti-aliased UI over [`Color::TRANSPARENT`] uses separate alpha
    /// compositing (`SRC_ALPHA, ONE_MINUS_SRC_ALPHA; ONE, ONE_MINUS_SRC_ALPHA`), keeping opaque
    /// panels at `alpha = 1.0` and outputting premultiplied RGB on semi-transparent edges.
    pub const fn clear_color(mut self, clear_color: Color) -> Self {
        self.clear_color = clear_color;
        self
    }
}

/// Independent offscreen UI surface holding its own `UiContext`, `App`, and GPU color FBO.
pub struct OffscreenSurface {
    ui: UiContext,
    app: App,
    width: u32,
    height: u32,
    scale_factor: f32,
    clear_color: Color,
    target: OffscreenTarget,
    dirty: bool,
}

impl OffscreenSurface {
    /// Returns a shared reference to this surface's [`UiContext`].
    pub fn ui(&self) -> &UiContext {
        &self.ui
    }

    /// Returns a mutable reference to this surface's [`UiContext`] for feeding input or theme updates.
    pub fn ui_mut(&mut self) -> &mut UiContext {
        &mut self.ui
    }

    /// Returns a shared reference to this surface's [`App`].
    pub fn app(&self) -> &App {
        &self.app
    }

    /// Returns a mutable reference to this surface's [`App`] for texture uploads or redraw requests.
    pub fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }

    /// Returns simultaneous mutable references to this surface's [`UiContext`] and [`App`].
    pub fn ui_and_app_mut(&mut self) -> (&mut UiContext, &mut App) {
        (&mut self.ui, &mut self.app)
    }

    /// Returns the physical framebuffer width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the physical framebuffer height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns the physical framebuffer dimensions `[width, height]` in pixels.
    pub fn size(&self) -> [u32; 2] {
        [self.width, self.height]
    }

    /// Returns the DPI scale factor mapping logical points to physical pixels.
    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// Returns the background clear color used for this surface's FBO.
    pub fn clear_color(&self) -> Color {
        self.clear_color
    }

    /// Updates the background clear color and marks the surface dirty if changed.
    pub fn set_clear_color(&mut self, color: Color) {
        if self.clear_color != color {
            self.clear_color = color;
            self.dirty = true;
        }
    }

    /// Marks this surface dirty so the next [`OffscreenHost::step_frame`] re-renders it.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Registers raw TTF/OTF/TTC font bytes into the shared font database using the font's embedded OpenType family names.
    pub fn register_font_bytes(&mut self, bytes: &[u8]) {
        crate::gfx::text::register_font_bytes(bytes);
        self.dirty = true;
    }

    /// Registers raw TTF/OTF/TTC font bytes into the shared font database under `name`.
    pub fn register_font(&mut self, name: &str, bytes: &[u8]) {
        crate::gfx::text::register_font(name, bytes);
        self.dirty = true;
    }

    /// Returns `true` if this surface has pending changes or active animations that require rendering.
    pub fn needs_render(&self) -> bool {
        self.dirty
            || self.ui.is_dirty()
            || self.ui.has_active_animations()
            || self.app.has_pending_redraw_or_textures()
    }

    /// Returns the exact RGBA8 byte size (`width * height * 4`) required for pixel readback.
    pub fn rgba_byte_len(&self) -> usize {
        (self.width as usize)
            .saturating_mul(self.height as usize)
            .saturating_mul(4)
    }

    /// Returns the underlying OpenGL color texture handle (`GLuint`) backing this surface's FBO.
    pub fn gl_texture_id(&self) -> u32 {
        self.target.raw_texture_id()
    }

    /// Returns the underlying OpenGL framebuffer object handle (`GLuint`) backing this surface.
    pub fn gl_fbo_id(&self) -> u32 {
        self.target.raw_fbo_id()
    }
}

/// Native offscreen OpenGL 3.3 Core GPU host driving one or more independent [`OffscreenSurface`]s.
///
/// Shares a single OpenGL context, shader pipeline, and glyph atlas across all created surfaces.
/// Performs deterministic GPU and OS resource teardown in `Drop` with zero background threads.
pub struct OffscreenHost {
    surfaces: HashMap<OffscreenSurfaceId, OffscreenSurface>,
    renderer: Renderer,
    gl: GlBackend,
    next_surface_id: u64,
    // Declared last so `NativeOffscreenGl` drops strictly after `surfaces`, `renderer`, and `gl`.
    gl_ctx: platform::NativeOffscreenGl,
    // OpenGL contexts and Win32 HWNDs have strict thread affinity.
    _not_send_sync: PhantomData<*mut ()>,
}

impl OffscreenHost {
    /// Creates an offscreen OpenGL 3.3 Core (or GLES 3.0 fallback on Linux) GPU host without a `winit` event loop.
    pub fn new() -> Result<Self, Error> {
        let (gl_ctx, glow_ctx) = platform::NativeOffscreenGl::new()?;
        Ok(Self {
            surfaces: HashMap::new(),
            renderer: Renderer::new(),
            gl: GlBackend::new(glow_ctx),
            next_surface_id: 1,
            gl_ctx,
            _not_send_sync: PhantomData,
        })
    }

    /// Registers raw TTF/OTF/TTC font bytes into the host's font database using the font's embedded OpenType family names.
    pub fn register_font_bytes(&mut self, bytes: &[u8]) {
        crate::gfx::text::register_font_bytes(bytes);
        for surf in self.surfaces.values_mut() {
            surf.dirty = true;
        }
    }

    /// Registers raw TTF/OTF/TTC font bytes into the host's font database under `name`.
    pub fn register_font(&mut self, name: &str, bytes: &[u8]) {
        crate::gfx::text::register_font(name, bytes);
        for surf in self.surfaces.values_mut() {
            surf.dirty = true;
        }
    }

    /// Creates a new independent offscreen UI surface with its own FBO, `UiContext`, and `App`.
    pub fn create_surface(
        &mut self,
        config: OffscreenSurfaceConfig,
    ) -> Result<OffscreenSurfaceId, Error> {
        if config.width == 0 || config.height == 0 {
            return Err(Error::InvalidInput(String::from(
                "offscreen surface dimensions must be non-zero",
            )));
        }
        let scale = sanitize_scale(config.scale_factor);
        self.gl_ctx.make_current()?;
        let target = self
            .gl
            .create_offscreen_target(config.width, config.height)?;

        let seq = self.next_surface_id;
        self.next_surface_id = self.next_surface_id.saturating_add(1);
        let id = OffscreenSurfaceId(seq);
        // Partition per-surface `TextureId` namespaces so independent `App` handles never collide in `GlBackend`.
        let tex_base = (seq & 0xFFFF_FFFF) << 32;

        self.surfaces.insert(
            id,
            OffscreenSurface {
                ui: UiContext::new(),
                app: App::with_texture_id_base(tex_base),
                width: config.width,
                height: config.height,
                scale_factor: scale,
                clear_color: config.clear_color,
                target,
                dirty: true,
            },
        );
        Ok(id)
    }

    /// Destroys a surface, deleting its FBO, color texture, and any `App` user textures it created.
    pub fn destroy_surface(&mut self, id: OffscreenSurfaceId) -> bool {
        let Some(mut surf) = self.surfaces.remove(&id) else {
            return false;
        };
        if self.gl_ctx.make_current().is_ok() {
            let destroy_ops = surf.app.take_all_texture_destroy_ops();
            self.gl.sync_textures(destroy_ops);
            self.gl.destroy_offscreen_target(surf.target);
        }
        true
    }

    /// Returns a shared reference to the [`OffscreenSurface`] identified by `id`.
    pub fn surface(&self, id: OffscreenSurfaceId) -> Option<&OffscreenSurface> {
        self.surfaces.get(&id)
    }

    /// Returns a mutable reference to the [`OffscreenSurface`] identified by `id`.
    pub fn surface_mut(&mut self, id: OffscreenSurfaceId) -> Option<&mut OffscreenSurface> {
        self.surfaces.get_mut(&id)
    }

    /// Resizes `id` to `width x height` physical pixels at `scale_factor`.
    ///
    /// Reallocates the underlying FBO only when physical dimensions change; marks the surface dirty.
    pub fn resize_surface(
        &mut self,
        id: OffscreenSurfaceId,
        width: u32,
        height: u32,
        scale_factor: f32,
    ) -> Result<(), Error> {
        if width == 0 || height == 0 {
            return Err(Error::InvalidInput(String::from(
                "offscreen surface dimensions must be non-zero",
            )));
        }
        let scale = sanitize_scale(scale_factor);
        let Some(surf) = self.surfaces.get_mut(&id) else {
            return Err(Error::InvalidInput(String::from(
                "unknown offscreen surface id",
            )));
        };
        if surf.width == width && surf.height == height {
            if (surf.scale_factor - scale).abs() > f32::EPSILON {
                surf.scale_factor = scale;
                surf.dirty = true;
            }
            return Ok(());
        }

        self.gl_ctx.make_current()?;
        let new_target = self.gl.create_offscreen_target(width, height)?;
        let old_target = core::mem::replace(&mut surf.target, new_target);
        self.gl.destroy_offscreen_target(old_target);
        surf.width = width;
        surf.height = height;
        surf.scale_factor = scale;
        surf.dirty = true;
        Ok(())
    }

    /// Advances physics, runs `update`, and GPU-renders `id` only when dirty or animating.
    ///
    /// Returns `Ok(true)` if a new frame was rendered into the surface's FBO, or `Ok(false)` when idle.
    pub fn step_frame<F>(
        &mut self,
        id: OffscreenSurfaceId,
        dt: f32,
        update: F,
    ) -> Result<bool, Error>
    where
        F: FnMut(&mut UiContext, &mut App),
    {
        self.step_frame_inner(id, dt, false, update)
    }

    /// Unconditionally runs `update` and GPU-renders `id` even when idle, returning `Ok(true)`.
    pub fn step_frame_forced<F>(
        &mut self,
        id: OffscreenSurfaceId,
        dt: f32,
        update: F,
    ) -> Result<bool, Error>
    where
        F: FnMut(&mut UiContext, &mut App),
    {
        self.step_frame_inner(id, dt, true, update)
    }

    fn step_frame_inner<F>(
        &mut self,
        id: OffscreenSurfaceId,
        dt: f32,
        force: bool,
        mut update: F,
    ) -> Result<bool, Error>
    where
        F: FnMut(&mut UiContext, &mut App),
    {
        let Some(surf) = self.surfaces.get_mut(&id) else {
            return Err(Error::InvalidInput(String::from(
                "unknown offscreen surface id",
            )));
        };
        if surf.width == 0 || surf.height == 0 {
            return Ok(false);
        }
        if !force && !surf.needs_render() {
            return Ok(false);
        }

        self.gl_ctx.make_current()?;

        let dt_clamped = if dt.is_finite() && dt > 0.0 {
            dt.min(0.05)
        } else {
            0.0
        };
        let scale = surf.scale_factor;
        let logical_w = surf.width as f32 / scale;
        let logical_h = surf.height as f32 / scale;

        surf.dirty = false;
        let _ = surf.ui.take_dirty();
        let _ = surf.app.take_redraw_request();

        surf.app.begin_step(dt_clamped);
        surf.ui.set_viewport_fbo_id(self.gl.viewport_fbo_id());
        surf.ui.set_texture_sizes(surf.app.texture_sizes());
        let (rm, si, asz, aocc) = self.renderer.perf_snapshot();
        surf.ui.update_renderer_perf(rm, si, asz, aocc);
        let (gms, gbytes) = self.renderer.gpu_telemetry();
        surf.ui.update_gpu_perf(gms, gbytes);

        surf.ui.step_physics(dt_clamped);
        surf.ui.begin_frame(logical_w, logical_h, scale);
        update(&mut surf.ui, &mut surf.app);
        surf.ui.end_frame_with(&mut self.renderer.glyph_atlas);

        self.gl.sync_textures(surf.app.take_texture_ops());
        self.gl.frame_time_s = surf.ui.elapsed_time_s() as f32;
        self.gl.frame_pointer = surf.ui.pointer_pos().unwrap_or([0.0, 0.0]);
        self.renderer.render_frame_to(
            &mut self.gl,
            surf.target.handle(),
            [surf.width, surf.height],
            scale,
            surf.ui.pointer_pos(),
            surf.clear_color,
            surf.ui.draw_list(),
        );

        let (rm, si, asz, aocc) = self.renderer.perf_snapshot();
        surf.ui.update_renderer_perf(rm, si, asz, aocc);
        let (gms, gbytes) = self.renderer.gpu_telemetry();
        surf.ui.update_gpu_perf(gms, gbytes);

        Ok(true)
    }

    /// Reads the rendered RGBA8 pixels of `id` directly into caller-owned `dst` with zero allocations.
    ///
    /// Pass `bottom_up = true` for raw OpenGL / Unity `Texture2D.LoadRawTextureData` row order
    /// (zero row copies). Pass `bottom_up = false` to flip rows in-place into top-down order.
    pub fn read_pixels_rgba_into(
        &self,
        id: OffscreenSurfaceId,
        dst: &mut [u8],
        bottom_up: bool,
    ) -> Result<(), Error> {
        let Some(surf) = self.surfaces.get(&id) else {
            return Err(Error::InvalidInput(String::from(
                "unknown offscreen surface id",
            )));
        };
        self.gl_ctx.make_current()?;
        self.gl
            .read_target_pixels_rgba_into(&surf.target, dst, bottom_up)
    }

    /// Makes the host's OpenGL context current on the calling thread.
    pub fn make_current(&self) -> Result<(), Error> {
        self.gl_ctx.make_current()
    }

    /// Detaches the host's OpenGL context from the calling thread.
    pub fn make_not_current(&self) -> Result<(), Error> {
        self.gl_ctx.make_not_current()
    }

    /// Flushes queued OpenGL commands on the host context.
    pub fn gl_flush(&self) -> Result<(), Error> {
        self.gl_ctx.make_current()?;
        self.gl.flush();
        Ok(())
    }

    /// Waits for all queued OpenGL commands on the host context to finish (useful before GPU interop handoff).
    pub fn gl_finish(&self) -> Result<(), Error> {
        self.gl_ctx.make_current()?;
        self.gl.finish();
        Ok(())
    }

    /// Returns the raw platform GL context handle (`HGLRC` on Windows, `EGLContext` on Linux) for GPU interop.
    pub fn gl_context_handle(&self) -> usize {
        self.gl_ctx.raw_context_handle()
    }

    /// Returns the raw Win32 device context handle (`HDC`) on Windows, or `0` on non-Windows platforms.
    pub fn gl_device_context_handle(&self) -> usize {
        self.gl_ctx.raw_device_context_handle()
    }
}

impl Drop for OffscreenHost {
    fn drop(&mut self) {
        if self.gl_ctx.make_current().is_ok() {
            for (_, surf) in self.surfaces.drain() {
                self.gl.destroy_offscreen_target(surf.target);
            }
            self.gl.destroy_gl_resources();
        } else {
            self.surfaces.clear();
        }
        // `self.gl_ctx` drops next: releases/deletes WGL/EGL context, destroys dummy HWND, unregisters class.
    }
}

fn sanitize_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use core::ffi::{c_char, c_void, CStr};
    use core::mem::{size_of, zeroed};
    use core::ptr::{null, null_mut};
    use std::sync::atomic::{AtomicU64, Ordering};

    use glow::HasContext;

    use crate::ui::types::Error;

    const CS_OWNDC: u32 = 0x0020;
    const WS_POPUP: u32 = 0x8000_0000;
    const WS_CLIPSIBLINGS: u32 = 0x0400_0000;
    const WS_CLIPCHILDREN: u32 = 0x0200_0000;

    const PFD_DRAW_TO_WINDOW: u32 = 0x0000_0004;
    const PFD_SUPPORT_OPENGL: u32 = 0x0000_0020;
    const PFD_DOUBLEBUFFER: u32 = 0x0000_0001;
    const PFD_TYPE_RGBA: u8 = 0;
    const PFD_MAIN_PLANE: u8 = 0;

    const GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT: u32 = 0x0000_0002;
    const GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS: u32 = 0x0000_0004;

    const WGL_CONTEXT_MAJOR_VERSION_ARB: i32 = 0x2091;
    const WGL_CONTEXT_MINOR_VERSION_ARB: i32 = 0x2092;
    const WGL_CONTEXT_PROFILE_MASK_ARB: i32 = 0x9126;
    const WGL_CONTEXT_CORE_PROFILE_BIT_ARB: i32 = 0x0000_0001;

    static CLASS_SEQ: AtomicU64 = AtomicU64::new(1);

    #[repr(C)]
    struct WndClassExW {
        cb_size: u32,
        style: u32,
        lpfn_wnd_proc: Option<unsafe extern "system" fn(isize, u32, usize, isize) -> isize>,
        cb_cls_extra: i32,
        cb_wnd_extra: i32,
        h_instance: isize,
        h_icon: isize,
        h_cursor: isize,
        hbr_background: isize,
        lpsz_menu_name: *const u16,
        lpsz_class_name: *const u16,
        h_icon_sm: isize,
    }

    #[repr(C)]
    struct PixelFormatDescriptor {
        n_size: u16,
        n_version: u16,
        dw_flags: u32,
        i_pixel_type: u8,
        c_color_bits: u8,
        c_red_bits: u8,
        c_red_shift: u8,
        c_green_bits: u8,
        c_green_shift: u8,
        c_blue_bits: u8,
        c_blue_shift: u8,
        c_alpha_bits: u8,
        c_alpha_shift: u8,
        c_accum_bits: u8,
        c_accum_red_bits: u8,
        c_accum_green_bits: u8,
        c_accum_blue_bits: u8,
        c_accum_alpha_bits: u8,
        c_depth_bits: u8,
        c_stencil_bits: u8,
        c_aux_buffers: u8,
        i_layer_type: u8,
        b_reserved: u8,
        dw_layer_mask: u32,
        dw_visible_mask: u32,
        dw_damage_mask: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleExW(dw_flags: u32, lp_module_name: *const c_void, ph_module: *mut isize)
            -> i32;
        fn GetModuleHandleW(lp_module_name: *const u16) -> isize;
        fn LoadLibraryW(lp_lib_file_name: *const u16) -> isize;
        fn FreeLibrary(h_lib_module: isize) -> i32;
        fn GetProcAddress(h_module: isize, lp_proc_name: *const c_char) -> *const c_void;
    }

    #[link(name = "user32")]
    extern "system" {
        fn DefWindowProcW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize;
        fn RegisterClassExW(lpwcx: *const WndClassExW) -> u16;
        fn UnregisterClassW(lp_class_name: *const u16, h_instance: isize) -> i32;
        fn CreateWindowExW(
            dw_ex_style: u32,
            lp_class_name: *const u16,
            lp_window_name: *const u16,
            dw_style: u32,
            x: i32,
            y: i32,
            n_width: i32,
            n_height: i32,
            hwnd_parent: isize,
            h_menu: isize,
            h_instance: isize,
            lp_param: *mut c_void,
        ) -> isize;
        fn DestroyWindow(hwnd: isize) -> i32;
        fn GetDC(hwnd: isize) -> isize;
        fn ReleaseDC(hwnd: isize, hdc: isize) -> i32;
    }

    #[link(name = "gdi32")]
    extern "system" {
        fn ChoosePixelFormat(hdc: isize, ppfd: *const PixelFormatDescriptor) -> i32;
        fn SetPixelFormat(hdc: isize, format: i32, ppfd: *const PixelFormatDescriptor) -> i32;
    }

    #[link(name = "opengl32")]
    extern "system" {
        fn wglCreateContext(hdc: isize) -> isize;
        fn wglDeleteContext(hglrc: isize) -> i32;
        fn wglMakeCurrent(hdc: isize, hglrc: isize) -> i32;
        fn wglGetCurrentContext() -> isize;
        fn wglGetProcAddress(lpsz_proc: *const c_char) -> *const c_void;
    }

    type WndProcFn = unsafe extern "system" fn(isize, u32, usize, isize) -> isize;
    type WglCreateContextAttribsArbFn =
        unsafe extern "system" fn(isize, isize, *const i32) -> isize;

    fn wgl_module_anchor() {}

    fn encode_wide_nul(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(core::iter::once(0)).collect()
    }

    fn is_valid_wgl_proc(ptr: *const c_void) -> bool {
        let addr = ptr as usize;
        addr != 0 && addr != 1 && addr != 2 && addr != 3 && addr != usize::MAX
    }

    pub(super) struct NativeOffscreenGl {
        hinstance: isize,
        class_name: Vec<u16>,
        class_registered: bool,
        hwnd: isize,
        hdc: isize,
        hglrc: isize,
        opengl32: isize,
    }

    impl NativeOffscreenGl {
        pub(super) fn new() -> Result<(Self, glow::Context), Error> {
            let mut hinstance: isize = 0;
            // SAFETY: passing the address of a function inside this binary with UNCHANGED_REFCOUNT
            // resolves the exact EXE or DLL `HMODULE` without incrementing its reference count.
            let got_mod = unsafe {
                GetModuleHandleExW(
                    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                        | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                    wgl_module_anchor as *const () as *const c_void,
                    &mut hinstance,
                )
            };
            if got_mod == 0 || hinstance == 0 {
                // SAFETY: null requests the process executable module handle.
                hinstance = unsafe { GetModuleHandleW(null()) };
            }

            let seq = CLASS_SEQ.fetch_add(1, Ordering::Relaxed);
            let class_name = encode_wide_nul(&format!(
                "LumenicOffscreen_{}_{}",
                std::process::id(),
                seq
            ));
            let opengl32_name = encode_wide_nul("opengl32.dll");
            // SAFETY: null-terminated UTF-16 DLL name.
            let opengl32 = unsafe { LoadLibraryW(opengl32_name.as_ptr()) };
            if opengl32 == 0 {
                return Err(Error::Graphics(String::from(
                    "LoadLibraryW(opengl32.dll) failed",
                )));
            }

            let mut state = Self {
                hinstance,
                class_name,
                class_registered: false,
                hwnd: 0,
                hdc: 0,
                hglrc: 0,
                opengl32,
            };

            // Resolve `user32!DefWindowProcW` directly rather than using this DLL's IAT thunk
            // so the class window procedure remains valid inside `user32.dll` across DLL reloads.
            let user32_name = encode_wide_nul("user32.dll");
            let def_wnd_proc: unsafe extern "system" fn(isize, u32, usize, isize) -> isize = {
                // SAFETY: `user32.dll` is already loaded in this process via `#[link(name = "user32")]`.
                let user32_mod = unsafe { GetModuleHandleW(user32_name.as_ptr()) };
                let proc_ptr = if user32_mod != 0 {
                    // SAFETY: `user32_mod` is valid and symbol name is NUL-terminated.
                    unsafe { GetProcAddress(user32_mod, c"DefWindowProcW".as_ptr()) }
                } else {
                    null()
                };
                if !proc_ptr.is_null() {
                    // SAFETY: `DefWindowProcW` in `user32.dll` has the exact Win32 `WNDPROC` signature.
                    unsafe { core::mem::transmute::<*const c_void, WndProcFn>(proc_ptr) }
                } else {
                    DefWindowProcW
                }
            };

            let wc = WndClassExW {
                cb_size: size_of::<WndClassExW>() as u32,
                style: CS_OWNDC,
                lpfn_wnd_proc: Some(def_wnd_proc),
                cb_cls_extra: 0,
                cb_wnd_extra: 0,
                h_instance: state.hinstance,
                h_icon: 0,
                h_cursor: 0,
                hbr_background: 0,
                lpsz_menu_name: null(),
                lpsz_class_name: state.class_name.as_ptr(),
                h_icon_sm: 0,
            };

            // SAFETY: `wc` is fully initialized with `user32!DefWindowProcW` and null-terminated `class_name`.
            let atom = unsafe { RegisterClassExW(&wc) };
            if atom == 0 {
                return Err(Error::Graphics(format!(
                    "RegisterClassExW failed: {}",
                    std::io::Error::last_os_error()
                )));
            }
            state.class_registered = true;

            let title = encode_wide_nul("LumenicOffscreen");
            // SAFETY: registered class name and null-terminated title; hidden 1x1 popup window.
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    state.class_name.as_ptr(),
                    title.as_ptr(),
                    WS_POPUP | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
                    0,
                    0,
                    1,
                    1,
                    0,
                    0,
                    state.hinstance,
                    null_mut(),
                )
            };
            if hwnd == 0 {
                return Err(Error::Graphics(format!(
                    "CreateWindowExW failed: {}",
                    std::io::Error::last_os_error()
                )));
            }
            state.hwnd = hwnd;

            // SAFETY: `hwnd` is a valid live window owned by `state`.
            let hdc = unsafe { GetDC(hwnd) };
            if hdc == 0 {
                return Err(Error::Graphics(String::from("GetDC failed on dummy HWND")));
            }
            state.hdc = hdc;

            // SAFETY: zero-initialized `PixelFormatDescriptor` with standard RGBA8 OpenGL fields.
            let mut pfd: PixelFormatDescriptor = unsafe { zeroed() };
            pfd.n_size = size_of::<PixelFormatDescriptor>() as u16;
            pfd.n_version = 1;
            pfd.dw_flags = PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER;
            pfd.i_pixel_type = PFD_TYPE_RGBA;
            pfd.c_color_bits = 32;
            pfd.c_alpha_bits = 8;
            pfd.c_depth_bits = 24;
            pfd.c_stencil_bits = 8;
            pfd.i_layer_type = PFD_MAIN_PLANE;

            // SAFETY: `hdc` is valid and `pfd` is properly sized.
            let pf = unsafe { ChoosePixelFormat(hdc, &pfd) };
            if pf == 0 {
                return Err(Error::Graphics(format!(
                    "ChoosePixelFormat failed: {}",
                    std::io::Error::last_os_error()
                )));
            }
            // SAFETY: `pf` was returned by `ChoosePixelFormat` for `hdc`.
            if unsafe { SetPixelFormat(hdc, pf, &pfd) } == 0 {
                return Err(Error::Graphics(format!(
                    "SetPixelFormat failed: {}",
                    std::io::Error::last_os_error()
                )));
            }

            // SAFETY: `hdc` has a valid pixel format set; create bootstrap context to query WGL ARB extensions.
            let boot_rc = unsafe { wglCreateContext(hdc) };
            if boot_rc == 0 {
                return Err(Error::Graphics(format!(
                    "wglCreateContext failed: {}",
                    std::io::Error::last_os_error()
                )));
            }
            state.hglrc = boot_rc;

            // SAFETY: `hdc` and `boot_rc` are valid handles created above.
            if unsafe { wglMakeCurrent(hdc, boot_rc) } == 0 {
                return Err(Error::Graphics(format!(
                    "wglMakeCurrent(bootstrap) failed: {}",
                    std::io::Error::last_os_error()
                )));
            }

            // SAFETY: `boot_rc` is current on this thread; query `wglCreateContextAttribsARB`.
            let arb_ptr =
                unsafe { wglGetProcAddress(c"wglCreateContextAttribsARB".as_ptr()) };
            if !is_valid_wgl_proc(arb_ptr) {
                return Err(Error::Graphics(String::from(
                    "WGL_ARB_create_context is unavailable (OpenGL 3.3 Core required)",
                )));
            }
            // SAFETY: `wglCreateContextAttribsARB` follows the standard WGL_ARB_create_context ABI.
            let create_attribs = unsafe {
                core::mem::transmute::<*const c_void, WglCreateContextAttribsArbFn>(arb_ptr)
            };
            let core_attrs = [
                WGL_CONTEXT_MAJOR_VERSION_ARB,
                3,
                WGL_CONTEXT_MINOR_VERSION_ARB,
                3,
                WGL_CONTEXT_PROFILE_MASK_ARB,
                WGL_CONTEXT_CORE_PROFILE_BIT_ARB,
                0,
            ];
            // SAFETY: `hdc` is valid and `core_attrs` is 0-terminated.
            let core_rc = unsafe { create_attribs(hdc, 0, core_attrs.as_ptr()) };
            if core_rc == 0 {
                return Err(Error::Graphics(format!(
                    "wglCreateContextAttribsARB(OpenGL 3.3 Core) failed: {}",
                    std::io::Error::last_os_error()
                )));
            }
            // SAFETY: switch current context to `core_rc`, then destroy `boot_rc`.
            if unsafe { wglMakeCurrent(hdc, core_rc) } == 0 {
                let os_err = std::io::Error::last_os_error();
                unsafe {
                    let _ = wglDeleteContext(core_rc);
                }
                return Err(Error::Graphics(format!(
                    "wglMakeCurrent(OpenGL 3.3 Core) failed: {os_err}"
                )));
            }
            state.hglrc = core_rc;
            // SAFETY: `boot_rc` is no longer current and is owned by us.
            unsafe {
                let _ = wglDeleteContext(boot_rc);
            }

            let gl_dll = state.opengl32;
            // SAFETY: `state.hglrc` is current on this thread and `gl_dll` is a live `opengl32.dll` handle.
            let glow_ctx = unsafe {
                glow::Context::from_loader_function_cstr(|sym: &CStr| {
                    let p = wglGetProcAddress(sym.as_ptr());
                    if is_valid_wgl_proc(p) {
                        p
                    } else {
                        GetProcAddress(gl_dll, sym.as_ptr())
                    }
                })
            };
            let ver = glow_ctx.version();
            if ver.is_embedded || (ver.major, ver.minor) < (3, 3) {
                return Err(Error::Graphics(format!(
                    "offscreen WGL context version {}.{} is below required OpenGL 3.3 Core",
                    ver.major, ver.minor
                )));
            }

            Ok((state, glow_ctx))
        }

        pub(super) fn make_current(&self) -> Result<(), Error> {
            // SAFETY: `self.hdc` and `self.hglrc` remain valid until `Drop`.
            if unsafe { wglMakeCurrent(self.hdc, self.hglrc) } == 0 {
                return Err(Error::Graphics(format!(
                    "wglMakeCurrent failed: {}",
                    std::io::Error::last_os_error()
                )));
            }
            Ok(())
        }

        pub(super) fn make_not_current(&self) -> Result<(), Error> {
            // SAFETY: detaching the current WGL context on this thread is always valid.
            if unsafe { wglMakeCurrent(0, 0) } == 0 {
                return Err(Error::Graphics(format!(
                    "wglMakeCurrent(0, 0) failed: {}",
                    std::io::Error::last_os_error()
                )));
            }
            Ok(())
        }

        pub(super) fn raw_context_handle(&self) -> usize {
            self.hglrc as usize
        }

        pub(super) fn raw_device_context_handle(&self) -> usize {
            self.hdc as usize
        }
    }

    impl Drop for NativeOffscreenGl {
        fn drop(&mut self) {
            // SAFETY: resources are torn down strictly in reverse creation order while valid.
            unsafe {
                if self.hglrc != 0 {
                    if wglGetCurrentContext() == self.hglrc {
                        let _ = wglMakeCurrent(0, 0);
                    }
                    let _ = wglDeleteContext(self.hglrc);
                    self.hglrc = 0;
                }
                if self.hdc != 0 && self.hwnd != 0 {
                    let _ = ReleaseDC(self.hwnd, self.hdc);
                    self.hdc = 0;
                }
                if self.hwnd != 0 {
                    let _ = DestroyWindow(self.hwnd);
                    self.hwnd = 0;
                }
                if self.class_registered {
                    let _ = UnregisterClassW(self.class_name.as_ptr(), self.hinstance);
                    self.class_registered = false;
                }
                if self.opengl32 != 0 {
                    let _ = FreeLibrary(self.opengl32);
                    self.opengl32 = 0;
                }
            }
        }
    }
}

#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "windows"),
    not(any(target_os = "macos", target_os = "ios"))
))]
mod platform {
    use std::num::NonZeroU32;

    use glutin::api::egl::context::PossiblyCurrentContext as EglPossiblyCurrentContext;
    use glutin::api::egl::device::Device as EglDevice;
    use glutin::api::egl::display::Display as EglDisplay;
    use glutin::api::egl::surface::Surface as EglSurface;
    use glutin::config::{Api, ConfigSurfaceTypes, ConfigTemplateBuilder};
    use glutin::context::{
        AsRawContext, ContextApi, ContextAttributesBuilder, NotCurrentGlContext,
        PossiblyCurrentGlContext, RawContext, Version,
    };
    use glutin::display::{GetDisplayExtensions, GlDisplay};
    use glutin::surface::{PbufferSurface, SurfaceAttributesBuilder};
    use raw_window_handle::{RawDisplayHandle, XlibDisplayHandle};

    use crate::ui::types::Error;

    pub(super) struct NativeOffscreenGl {
        context: EglPossiblyCurrentContext,
        pbuffer: Option<EglSurface<PbufferSurface>>,
        _display: EglDisplay,
    }

    impl NativeOffscreenGl {
        pub(super) fn new() -> Result<(Self, glow::Context), Error> {
            let mut last_err = String::from("no EGL display available");

            if let Ok(devices) = EglDevice::query_devices() {
                for dev in devices {
                    // SAFETY: `dev` is an enumerated `EGLDeviceEXT` and no DRM fd handle is passed.
                    match unsafe { EglDisplay::with_device(&dev, None) } {
                        Ok(display) => match init_egl_display(display) {
                            Ok(res) => return Ok(res),
                            Err(e) => last_err = e.to_string(),
                        },
                        Err(e) => last_err = e.to_string(),
                    }
                }
            }

            // SAFETY: `XlibDisplayHandle::new(None, 0)` passes `EGL_DEFAULT_DISPLAY`.
            let default_disp =
                unsafe { EglDisplay::new(RawDisplayHandle::Xlib(XlibDisplayHandle::new(None, 0))) }
                    .map_err(|e| {
                        Error::Graphics(format!(
                            "failed to initialize offscreen EGL display ({last_err}; default: {e})"
                        ))
                    })?;
            init_egl_display(default_disp)
        }

        pub(super) fn make_current(&self) -> Result<(), Error> {
            let res = if let Some(ref pb) = self.pbuffer {
                self.context.make_current(pb)
            } else {
                self.context.make_current_surfaceless()
            };
            res.map_err(|e| Error::Graphics(format!("EGL make_current failed: {e}")))
        }

        pub(super) fn make_not_current(&self) -> Result<(), Error> {
            self.context
                .make_not_current_in_place()
                .map_err(|e| Error::Graphics(format!("EGL make_not_current failed: {e}")))
        }

        pub(super) fn raw_context_handle(&self) -> usize {
            match self.context.raw_context() {
                RawContext::Egl(ptr) => ptr as usize,
                #[allow(unreachable_patterns)]
                _ => 0,
            }
        }

        pub(super) fn raw_device_context_handle(&self) -> usize {
            0
        }
    }

    impl Drop for NativeOffscreenGl {
        fn drop(&mut self) {
            if self.context.is_current() {
                let _ = self.context.make_not_current_in_place();
            }
        }
    }

    fn init_egl_display(display: EglDisplay) -> Result<(NativeOffscreenGl, glow::Context), Error> {
        let supports_surfaceless = display
            .extensions()
            .contains("EGL_KHR_surfaceless_context");

        let mut templates = vec![
            ConfigTemplateBuilder::new()
                .with_alpha_size(8)
                .with_depth_size(0)
                .with_stencil_size(0)
                .with_api(Api::OPENGL)
                .with_surface_type(ConfigSurfaceTypes::PBUFFER)
                .build(),
            ConfigTemplateBuilder::new()
                .with_alpha_size(8)
                .with_depth_size(0)
                .with_stencil_size(0)
                .with_surface_type(ConfigSurfaceTypes::PBUFFER)
                .build(),
        ];
        if supports_surfaceless {
            templates.push(
                ConfigTemplateBuilder::new()
                    .with_alpha_size(8)
                    .with_depth_size(0)
                    .with_stencil_size(0)
                    .with_api(Api::OPENGL)
                    .with_surface_type(ConfigSurfaceTypes::empty())
                    .build(),
            );
            templates.push(
                ConfigTemplateBuilder::new()
                    .with_alpha_size(8)
                    .with_depth_size(0)
                    .with_stencil_size(0)
                    .with_surface_type(ConfigSurfaceTypes::empty())
                    .build(),
            );
        }

        // SAFETY: templates contain no window handle pointers.
        let config = templates
            .into_iter()
            .find_map(|tpl| unsafe { display.find_configs(tpl).ok().and_then(|mut it| it.next()) })
            .ok_or_else(|| Error::Graphics(String::from("no compatible EGL config found")))?;

        let core_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
            .build(None);
        let gles_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::Gles(Some(Version::new(3, 0))))
            .build(None);

        // SAFETY: `config` belongs to `display` and no raw window handle is passed.
        let not_current = unsafe {
            display
                .create_context(&config, &core_attrs)
                .or_else(|_| display.create_context(&config, &gles_attrs))
        }
        .map_err(|e| {
            Error::Graphics(format!(
                "offscreen EGL OpenGL 3.3 / GLES 3.0 context creation failed: {e}"
            ))
        })?;

        let (context, pbuffer) = if supports_surfaceless {
            let ctx = not_current.make_current_surfaceless().map_err(|e| {
                Error::Graphics(format!("EGL make_current_surfaceless failed: {e}"))
            })?;
            (ctx, None)
        } else {
            let one = NonZeroU32::new(1).expect("nonzero");
            let pb_attrs = SurfaceAttributesBuilder::<PbufferSurface>::new().build(one, one);
            // SAFETY: `config` belongs to `display` and `pb_attrs` specifies valid 1x1 dimensions.
            let pb = unsafe { display.create_pbuffer_surface(&config, &pb_attrs) }
                .map_err(|e| Error::Graphics(format!("EGL pbuffer surface creation failed: {e}")))?;
            let ctx = not_current
                .make_current(&pb)
                .map_err(|e| Error::Graphics(format!("EGL make_current failed: {e}")))?;
            (ctx, Some(pb))
        };

        // SAFETY: `context` is current on this thread and `display` resolves valid GL entrypoints.
        let glow_ctx = unsafe {
            glow::Context::from_loader_function_cstr(|sym| display.get_proc_address(sym))
        };

        Ok((
            NativeOffscreenGl {
                context,
                pbuffer,
                _display: display,
            },
            glow_ctx,
        ))
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod platform {
    use crate::ui::types::Error;

    pub(super) struct NativeOffscreenGl;

    impl NativeOffscreenGl {
        pub(super) fn new() -> Result<(Self, glow::Context), Error> {
            Err(Error::Graphics(String::from(
                "offscreen GPU host is currently implemented for Windows (WGL) and Linux (EGL)",
            )))
        }

        pub(super) fn make_current(&self) -> Result<(), Error> {
            Ok(())
        }

        pub(super) fn make_not_current(&self) -> Result<(), Error> {
            Ok(())
        }

        pub(super) fn raw_context_handle(&self) -> usize {
            0
        }

        pub(super) fn raw_device_context_handle(&self) -> usize {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offscreen_multi_surface_alpha_compositing_readback_resize_and_hot_reload_drop() {
        // Repeat creation + teardown twice to verify deterministic Win32 class unregistration
        // and clean reload without leaks or class-registration collisions.
        for cycle in 0..2 {
            let host_res = OffscreenHost::new();
            let mut host = match host_res {
                Ok(h) => h,
                Err(e) => {
                    // Headless CI runners without a GPU/EGL/WGL driver may return Graphics error.
                    eprintln!("skipping GPU offscreen test (no GL driver available): {e}");
                    return;
                }
            };
            assert_ne!(host.gl_context_handle(), 0);

            let s1 = host
                .create_surface(
                    OffscreenSurfaceConfig::new(128, 96)
                        .scale_factor(1.0)
                        .clear_color(Color::TRANSPARENT),
                )
                .expect("create surface 1");
            let s2 = host
                .create_surface(
                    OffscreenSurfaceConfig::new(64, 64)
                        .scale_factor(2.0)
                        .clear_color(Color::rgba(10, 20, 30, 255)),
                )
                .expect("create surface 2");

            assert_ne!(host.surface(s1).unwrap().gl_texture_id(), 0);
            assert_ne!(host.surface(s1).unwrap().gl_fbo_id(), 0);
            assert_ne!(
                host.surface(s1).unwrap().gl_texture_id(),
                host.surface(s2).unwrap().gl_texture_id()
            );

            // Surface 1: draw an opaque card at top-left (0,0)-(64,64) with a translucent
            // overlay (alpha 51 = 0.2) on top. Verify `blend_func_separate` keeps alpha == 255
            // (no semi-transparent holes punched into opaque cards!) while outside stays alpha == 0.
            let rendered_1 = host
                .step_frame(s1, 0.016, |ui, _app| {
                    ui.zstack(|ui| {
                        ui.col(|_| {}).w(64.0).h(64.0).bg(Color::rgb(200, 40, 40));
                        ui.col(|_| {})
                            .w(64.0)
                            .h(64.0)
                            .bg(Color::rgba(255, 255, 255, 51));
                    })
                    .w(64.0)
                    .h(64.0);
                })
                .expect("step s1 frame 1");
            assert!(rendered_1, "first frame must render (cycle {cycle})");

            // Idle second frame must skip rendering and return Ok(false).
            let idle_1 = host
                .step_frame(s1, 0.016, |ui, _app| {
                    ui.col(|_| {}).w(64.0).h(64.0).bg(Color::rgb(200, 40, 40));
                })
                .expect("step s1 idle");
            assert!(!idle_1, "idle frame must return false");

            // Verify top-down readback (bottom_up = false) and bottom-up readback (bottom_up = true).
            let byte_len = host.surface(s1).unwrap().rgba_byte_len();
            let mut top_down = vec![0u8; byte_len];
            let mut bottom_up = vec![0u8; byte_len];
            host.read_pixels_rgba_into(s1, &mut top_down, false)
                .expect("read top-down");
            host.read_pixels_rgba_into(s1, &mut bottom_up, true)
                .expect("read bottom-up");

            // Top-down pixel (16, 16) is inside the opaque card + translucent overlay.
            let td_inside = (16 * 128 + 16) * 4;
            let px_in = &top_down[td_inside..td_inside + 4];
            assert!(
                px_in[0] >= 200 && px_in[3] == 255,
                "expected opaque blended red/white with alpha 255 (no alpha hole), got {px_in:?}"
            );

            // Top-down pixel (100, 80) is in the transparent cleared area outside the card.
            let td_outside = (80 * 128 + 100) * 4;
            let px_out = &top_down[td_outside..td_outside + 4];
            assert_eq!(px_out, &[0, 0, 0, 0], "transparent background corrupted");

            // Bottom-up row (95 - 16) must equal top-down row 16.
            let bu_inside = ((95 - 16) * 128 + 16) * 4;
            assert_eq!(
                &bottom_up[bu_inside..bu_inside + 4],
                px_in,
                "bottom_up and top_down row orientations disagreed"
            );

            // Surface 2: render with independent opaque clear color and verify texture upload isolation.
            let tex_rgba = [0u8, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255];
            let tid = host
                .surface_mut(s2)
                .unwrap()
                .app_mut()
                .create_texture_rgba(2, 2, &tex_rgba)
                .expect("create surface 2 texture");
            let rendered_2 = host
                .step_frame(s2, 0.016, |ui, _app| {
                    ui.image(tid).w(32.0).h(32.0);
                })
                .expect("step s2");
            assert!(rendered_2);

            let mut s2_buf = vec![0u8; host.surface(s2).unwrap().rgba_byte_len()];
            host.read_pixels_rgba_into(s2, &mut s2_buf, false)
                .expect("read s2");
            let s2_px = &s2_buf[(8 * 64 + 8) * 4..(8 * 64 + 8) * 4 + 4];
            assert_eq!(s2_px, &[0, 255, 0, 255], "uploaded green texture not rendered");

            // Resize s1 dynamically and verify Dual-Kawase glass renders into offscreen FBO cleanly.
            host.resize_surface(s1, 80, 48, 1.5).expect("resize s1");
            assert_eq!(host.surface(s1).unwrap().size(), [80, 48]);
            assert!((host.surface(s1).unwrap().scale_factor() - 1.5).abs() < 1e-4);
            let glass_rendered = host
                .step_frame(s1, 0.016, |ui, _app| {
                    ui.zstack(|ui| {
                        ui.col(|_| {}).w(40.0).h(30.0).bg(Color::rgb(40, 120, 240));
                        ui.col(|_| {})
                            .w(30.0)
                            .h(20.0)
                            .bg(Color::rgba(255, 255, 255, 40))
                            .glass();
                    })
                    .w(40.0)
                    .h(30.0);
                })
                .expect("step s1 glass");
            assert!(glass_rendered);
            let mut glass_buf = vec![0u8; host.surface(s1).unwrap().rgba_byte_len()];
            host.read_pixels_rgba_into(s1, &mut glass_buf, false)
                .expect("read glass s1");
            let g_in = (10 * 80 + 10) * 4;
            assert_eq!(
                glass_buf[g_in + 3],
                255,
                "glass panel over opaque card must keep alpha == 255, got {:?}",
                &glass_buf[g_in..g_in + 4]
            );
            let g_out = (40 * 80 + 70) * 4;
            assert_eq!(
                &glass_buf[g_out..g_out + 4],
                &[0, 0, 0, 0],
                "transparent region outside glass panel must remain transparent"
            );

            let mut small = vec![0u8; 16];
            assert!(host.read_pixels_rgba_into(s1, &mut small, true).is_err());

            assert!(host.destroy_surface(s2));
            assert!(!host.destroy_surface(s2));
            drop(host);
        }
    }
}
