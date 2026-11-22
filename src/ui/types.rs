use core::fmt;

/// RGBA color with 8-bit unsigned channels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    /// Red channel (`0..=255`).
    pub r: u8,
    /// Green channel (`0..=255`).
    pub g: u8,
    /// Blue channel (`0..=255`).
    pub b: u8,
    /// Alpha channel (`0` transparent to `255` opaque).
    pub a: u8,
}

impl Color {
    /// Fully transparent black (`rgba(0, 0, 0, 0)`).
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);
    /// Solid opaque white (`rgb(255, 255, 255)`).
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    /// Solid opaque black (`rgb(0, 0, 0)`).
    pub const BLACK: Self = Self::rgb(0, 0, 0);

    /// Creates a color from explicit RGBA components.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Creates an opaque color from RGB components.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Creates an opaque color from a `0xRRGGBB` integer literal.
    pub const fn from_hex(hex: u32) -> Self {
        Self {
            r: ((hex >> 16) & 0xff) as u8,
            g: ((hex >> 8) & 0xff) as u8,
            b: (hex & 0xff) as u8,
            a: 255,
        }
    }

    /// Creates a color from a `0xRRGGBBAA` integer literal.
    pub const fn from_hex_rgba(hex: u32) -> Self {
        Self {
            r: ((hex >> 24) & 0xff) as u8,
            g: ((hex >> 16) & 0xff) as u8,
            b: ((hex >> 8) & 0xff) as u8,
            a: (hex & 0xff) as u8,
        }
    }

    /// Converts channels to normalized linear-range `[f32; 4]` floats in `0.0..=1.0`.
    pub fn to_normalized_f32(self) -> [f32; 4] {
        [
            f32::from(self.r) / 255.0,
            f32::from(self.g) / 255.0,
            f32::from(self.b) / 255.0,
            f32::from(self.a) / 255.0,
        ]
    }
}

/// Axis-aligned rectangle in logical UI coordinates (origin at top-left).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// Left X coordinate in logical points.
    pub x: f32,
    /// Top Y coordinate in logical points.
    pub y: f32,
    /// Width in logical points.
    pub w: f32,
    /// Height in logical points.
    pub h: f32,
}

impl Rect {
    /// Zero-sized rectangle at `(0.0, 0.0)`.
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    };

    /// Creates a rectangle at `(x, y)` with width `w` and height `h`.
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Returns `true` if point `(px, py)` lies inside `[x, x + w) x [y, y + h)`.
    pub fn contains(self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    /// Returns the intersection of two rectangles, clamped to non-negative extent.
    pub fn intersect(self, other: Self) -> Self {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = (self.x + self.w).min(other.x + other.w);
        let y1 = (self.y + self.h).min(other.y + other.h);
        Self {
            x: x0,
            y: y0,
            w: (x1 - x0).max(0.0),
            h: (y1 - y0).max(0.0),
        }
    }

    /// Snaps all four edges to physical framebuffer pixels for crisp rendering at fractional DPI.
    pub fn snap(self, scale_factor: f32) -> Self {
        if !scale_factor.is_finite() || scale_factor <= 0.0 {
            return self;
        }
        let left = (self.x * scale_factor).round() / scale_factor;
        let top = (self.y * scale_factor).round() / scale_factor;
        let right = ((self.x + self.w) * scale_factor).round() / scale_factor;
        let bottom = ((self.y + self.h) * scale_factor).round() / scale_factor;
        Self {
            x: left,
            y: top,
            w: (right - left).max(0.0),
            h: (bottom - top).max(0.0),
        }
    }
}

/// Framework-owned keyboard key representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// Tab / focus navigation key.
    Tab,
    /// Enter / Return activation key.
    Enter,
    /// Spacebar key.
    Space,
    /// Escape cancellation/dismiss key.
    Escape,
    /// Backspace delete-backward key.
    Backspace,
    /// Forward Delete key.
    Delete,
    /// Insert key.
    Insert,
    /// Left arrow key.
    Left,
    /// Right arrow key.
    Right,
    /// Up arrow key.
    Up,
    /// Down arrow key.
    Down,
    /// Home navigation key.
    Home,
    /// End navigation key.
    End,
    /// Page Up key.
    PageUp,
    /// Page Down key.
    PageDown,
    /// Function key `F1`.
    F1,
    /// Function key `F2`.
    F2,
    /// Function key `F3`.
    F3,
    /// Function key `F4`.
    F4,
    /// Function key `F5`.
    F5,
    /// Function key `F6`.
    F6,
    /// Function key `F7`.
    F7,
    /// Function key `F8`.
    F8,
    /// Function key `F9`.
    F9,
    /// Function key `F10`.
    F10,
    /// Function key `F11`.
    F11,
    /// Function key `F12` (also toggles built-in telemetry HUD).
    F12,
    /// Printable character key.
    Char(char),
    /// Unmapped platform scan/key code.
    Other(u32),
}

/// Keyboard modifier state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    /// Whether either `Shift` key is held.
    pub shift: bool,
    /// Whether either `Ctrl` key is held.
    pub ctrl: bool,
    /// Whether either `Alt` / `Option` key is held.
    pub alt: bool,
    /// Whether the OS `Super` / `Win` / `Cmd` key is held.
    pub super_key: bool,
}

/// Pointer / mouse cursor icon requested by hovered UI nodes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CursorIcon {
    /// Standard OS arrow cursor.
    #[default]
    Default,
    /// Interactive hand pointer cursor for links and buttons.
    Pointer,
    /// Text I-beam selection cursor.
    Text,
    /// Open-hand grab cursor.
    Grab,
    /// Closed-hand dragging cursor.
    Grabbing,
    /// Precision crosshair cursor.
    Crosshair,
    /// Horizontal east-west resize cursor.
    ResizeH,
    /// Vertical north-south resize cursor.
    ResizeV,
    /// Diagonal northwest-southeast resize cursor.
    ResizeNwse,
    /// Diagonal northeast-southwest resize cursor.
    ResizeNesw,
    /// Prohibited / disabled action cursor.
    NotAllowed,
}

/// Horizontal text alignment inside a `txt` or `rich` layout rect.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextAlign {
    /// Align text flush left.
    #[default]
    Left,
    /// Center each line horizontally inside available bounds.
    Center,
    /// Align text flush right.
    Right,
}

/// Mouse / pointer button identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PointerButton {
    /// Primary (typically left) pointer button.
    Left,
    /// Secondary (typically right) context pointer button.
    Right,
    /// Middle / wheel-click pointer button.
    Middle,
}

/// Opaque GPU texture handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureId(pub u64);

/// Aspect-ratio scaling mode for inline `img` / `image` widgets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageFit {
    /// Stretches texture to fill the widget bounds.
    #[default]
    Fill,
    /// Scales uniformly to fit inside bounds while preserving aspect ratio.
    Contain,
    /// Scales uniformly to cover bounds while preserving aspect ratio (cropped UV).
    Cover,
}

/// Opaque 3D mesh handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MeshId(pub u64);

/// Opaque 3D scene-graph node handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub u64);

/// Opaque secondary popover window identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PopoverId(pub u64);

/// Perspective 3D camera parameters for `Scene3D` viewports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera3D {
    /// World-space camera eye position `[x, y, z]`.
    pub position: [f32; 3],
    /// World-space look-at target `[x, y, z]`.
    pub target: [f32; 3],
    /// Upward direction vector `[x, y, z]`.
    pub up: [f32; 3],
    /// Vertical field of view in degrees.
    pub fov_deg: f32,
    /// Near clipping plane distance.
    pub near: f32,
    /// Far clipping plane distance.
    pub far: f32,
}

impl Default for Camera3D {
    fn default() -> Self {
        Self {
            position: [0.0, 1.5, 4.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov_deg: 60.0,
            near: 0.1,
            far: 200.0,
        }
    }
}

/// Inline rich-text span element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Span<'a> {
    /// Plain text slice.
    Text(&'a str),
    /// Bold-weight text slice.
    Bold(&'a str),
    /// Italicized text slice.
    Italic(&'a str),
    /// Inline monospace code pill slice.
    Code(&'a str),
    /// Highlighted `@mention` pill slice.
    Mention(&'a str),
    /// Interactive hyperlink span with display text and ID/URL.
    Link {
        /// Visible hyperlink label.
        text: &'a str,
        /// Click identifier or destination URL.
        id: &'a str,
    },
    /// Foreground-colored text span without background pill.
    Colored {
        /// Text slice.
        text: &'a str,
        /// Foreground text color.
        color: Color,
    },
    /// Plain monospace text slice without pill background.
    Mono(&'a str),
    /// Colored monospace text slice without pill background.
    MonoColored {
        /// Text slice.
        text: &'a str,
        /// Foreground text color.
        color: Color,
    },
}

impl<'a> Span<'a> {
    /// Creates a plain text inline span.
    pub const fn text(text: &'a str) -> Self {
        Self::Text(text)
    }

    /// Creates a bold text inline span.
    pub const fn bold(text: &'a str) -> Self {
        Self::Bold(text)
    }

    /// Creates an italic text inline span.
    pub const fn italic(text: &'a str) -> Self {
        Self::Italic(text)
    }

    /// Creates an inline monospace code pill span.
    pub const fn code(text: &'a str) -> Self {
        Self::Code(text)
    }

    /// Creates a `@mention` accent pill span.
    pub const fn mention(text: &'a str) -> Self {
        Self::Mention(text)
    }

    /// Creates an interactive link span.
    pub const fn link(text: &'a str, id: &'a str) -> Self {
        Self::Link { text, id }
    }

    /// Creates a colored text span without a pill background.
    pub const fn colored(text: &'a str, color: Color) -> Self {
        Self::Colored { text, color }
    }

    /// Creates a monospace text span without a pill background.
    pub const fn mono(text: &'a str) -> Self {
        Self::Mono(text)
    }

    /// Creates a colored monospace text span without a pill background.
    pub const fn mono_colored(text: &'a str, color: Color) -> Self {
        Self::MonoColored { text, color }
    }

    /// Returns the display text slice of this span.
    pub const fn as_str(&self) -> &'a str {
        match *self {
            Self::Text(s)
            | Self::Bold(s)
            | Self::Italic(s)
            | Self::Code(s)
            | Self::Mention(s)
            | Self::Mono(s)
            | Self::Colored { text: s, .. }
            | Self::MonoColored { text: s, .. }
            | Self::Link { text: s, .. } => s,
        }
    }
}

/// SDF primitive shape mode for rectangle commands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RectShape {
    /// Rounded or sharp box rectangle.
    #[default]
    Box,
    /// Circle/capsule SDF pill.
    Circle,
    /// Superellipse squircle shape.
    Squircle,
    /// 45-degree chamfered / beveled corner box.
    Bevel,
    /// Regular flat-top hexagon inscribed in the node bounds.
    Hexagon,
    /// Regular pointy-top hexagon inscribed in the node bounds.
    HexagonPointy,
}

/// Main-axis distribution mode for `row` and `col` containers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Justify {
    /// Pack items at the start of the main axis.
    #[default]
    Start,
    /// Center items along the main axis.
    Center,
    /// Pack items at the end of the main axis.
    End,
    /// Distribute remaining space evenly between items.
    SpaceBetween,
    /// Distribute remaining space evenly around items.
    SpaceAround,
}

/// Cross-axis alignment mode for containers (`align_*`) and children (`align_self`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    /// Align to the start of the cross axis.
    #[default]
    Start,
    /// Center along the cross axis.
    Center,
    /// Align to the end of the cross axis.
    End,
    /// Stretch to fill the available cross-axis dimension.
    Stretch,
}

/// Action triggered by a custom client-side titlebar (`CSD`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CsdAction {
    /// No window action requested this frame.
    #[default]
    None,
    /// Minimize the host window.
    Minimize,
    /// Toggle window maximize/restore.
    ToggleMaximize,
    /// Request clean window close.
    Close,
    /// Initiate OS window drag move.
    StartDrag,
}

/// Visual style configuration for [`UiContext::csd_titlebar_styled`](crate::UiContext::csd_titlebar_styled).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CsdStyle {
    /// Titlebar background color (`None` uses `theme.surface`).
    pub bg: Option<Color>,
    /// Titlebar border `(width, color)` (`None` uses `(1.0, theme.border)`, `(0.0, _)` disables border).
    pub border: Option<(f32, Color)>,
    /// Titlebar horizontal and vertical padding `(px, py)` (defaults to `(12.0, 6.0)`).
    pub padding: (f32, f32),
    /// Gap between titlebar items (defaults to `6.0`).
    pub gap: f32,
    /// Window control button dimensions `(width, height)` (defaults to `(34.0, 26.0)`).
    pub control_size: (f32, f32),
    /// Window control button corner radius (defaults to `6.0`).
    pub control_radius: f32,
    /// Minimize/Maximize idle background (`None` uses transparent in ghost mode or `theme.surface_alt`).
    pub control_bg: Option<Color>,
    /// Minimize/Maximize hover background (`None` uses `theme.border`).
    pub control_hover_bg: Option<Color>,
    /// Minimize/Maximize icon stroke color (`None` uses `theme.text`).
    pub control_fg: Option<Color>,
    /// Close button idle background (`None` uses transparent in ghost mode or `0x9E2A2B`).
    pub close_bg: Option<Color>,
    /// Close button hover background (`None` uses `0xC42B1C` in ghost mode or `0xC63B3C`).
    pub close_hover_bg: Option<Color>,
    /// Close button icon stroke color (`None` uses `theme.text` idle / `Color::WHITE` hover in ghost mode, or `Color::WHITE`).
    pub close_fg: Option<Color>,
    /// Enables Windows 11-style ghost controls (transparent idle background, hover-only fill).
    pub ghost_controls: bool,
}

impl Default for CsdStyle {
    fn default() -> Self {
        Self {
            bg: None,
            border: None,
            padding: (12.0, 6.0),
            gap: 6.0,
            control_size: (34.0, 26.0),
            control_radius: 6.0,
            control_bg: None,
            control_hover_bg: None,
            control_fg: None,
            close_bg: None,
            close_hover_bg: None,
            close_fg: None,
            ghost_controls: false,
        }
    }
}

impl CsdStyle {
    /// Windows 11-style transparent ghost window controls (hover-only fill, subtle close hover red).
    pub fn ghost() -> Self {
        Self {
            ghost_controls: true,
            control_size: (40.0, 28.0),
            control_radius: 4.0,
            padding: (12.0, 4.0),
            border: Some((0.0, Color::TRANSPARENT)),
            ..Self::default()
        }
    }
}

/// Unified pointer and gesture state returned by `UiContext::interact`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GestureResponse {
    /// Whether the pointer is currently hovering the target bounds.
    pub hovered: bool,
    /// Whether the target was clicked or keyboard-activated this frame.
    pub clicked: bool,
    /// Whether a double-click occurred inside the target this frame.
    pub double_clicked: bool,
    /// Whether a secondary/right-click occurred inside the target this frame.
    pub right_clicked: bool,
    /// Whether primary button drag is active on this target.
    pub dragging: bool,
    /// Logical pointer movement `[dx, dy]` since the previous frame.
    pub delta: [f32; 2],
    /// Accumulated scroll delta `[dx, dy]` over the target this frame.
    pub scroll_delta: [f32; 2],
}

/// Window decoration mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Decorations {
    /// Native OS titlebar and window frame.
    Native,
    /// Seamless frameless surface with custom SDF client-side titlebar (default).
    #[default]
    Custom,
}

/// Application event-loop scheduling policy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoopMode {
    /// Sleeps at 0.0% CPU via `ControlFlow::Wait` when input and animations are idle.
    #[default]
    Idle,
    /// Drives fixed-frequency ticks (e.g. 60 or 144 Hz) for real-time games and viewports.
    FixedTimestep(u32),
}

/// Window creation configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowConfig {
    /// Window title or web document title.
    pub title: String,
    /// Initial logical window width in points.
    pub width: u32,
    /// Initial logical window height in points.
    pub height: u32,
    /// Native chrome vs. custom frameless SDF decorations.
    pub decorations: Decorations,
    /// Enables VSync swap interval when supported.
    pub vsync: bool,
    /// Enables translucent/frosted alpha window surface where supported.
    pub transparent: bool,
    /// Whether the window can be resized by the user.
    pub resizable: bool,
    /// Optional minimum window width in logical points.
    pub min_width: Option<u32>,
    /// Optional minimum window height in logical points.
    pub min_height: Option<u32>,
    /// Optional `(width, height, rgba8)` window and taskbar icon payload.
    pub icon_rgba: Option<(u32, u32, Vec<u8>)>,
    /// Bundled custom font `(family_name, ttf_or_otf_bytes)` entries registered at startup.
    pub fonts: Vec<(String, Vec<u8>)>,
    /// Optional global default sans-serif font family name or registered alias.
    pub default_font: Option<String>,
    /// Optional global default monospace font family name or registered alias.
    pub default_mono_font: Option<String>,
    /// Responsive `100vw x 100dvh` full-viewport canvas mode on `wasm32`.
    pub fluid: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: String::from("Lumenic"),
            width: 1024,
            height: 700,
            decorations: Decorations::Custom,
            vsync: true,
            transparent: false,
            resizable: true,
            min_width: Some(320),
            min_height: Some(240),
            icon_rgba: None,
            fonts: Vec::new(),
            default_font: None,
            default_mono_font: None,
            fluid: false,
        }
    }
}

impl WindowConfig {
    /// Creates a window configuration with `Decorations::Custom` enabled by default.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    /// Sets initial logical width and height in points.
    pub fn size(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Sets minimum logical width and height in points (pass `0, 0` to clear).
    pub fn min_size(mut self, width: u32, height: u32) -> Self {
        self.min_width = (width > 0).then_some(width);
        self.min_height = (height > 0).then_some(height);
        self
    }

    /// Enables or disables user window resizing.
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Chooses native OS chrome vs. custom frameless SDF decorations.
    pub fn decorations(mut self, decorations: Decorations) -> Self {
        self.decorations = decorations;
        self
    }

    /// Enables or disables VSync swap interval.
    pub fn vsync(mut self, vsync: bool) -> Self {
        self.vsync = vsync;
        self
    }

    /// Enables a translucent window surface for frosted/alpha compositing.
    pub fn transparent(mut self, transparent: bool) -> Self {
        self.transparent = transparent;
        self
    }

    /// Sets the window and taskbar icon from raw RGBA8 pixels (`width * height * 4` bytes).
    pub fn icon_rgba(mut self, width: u32, height: u32, rgba: impl Into<Vec<u8>>) -> Self {
        let data = rgba.into();
        let valid = width > 0
            && height > 0
            && usize::try_from(width)
                .ok()
                .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
                .and_then(|px| px.checked_mul(4))
                .is_some_and(|expected| data.len() == expected);
        self.icon_rgba = valid.then_some((width, height, data));
        self
    }

    /// Alias for [`Self::icon_rgba`].
    pub fn icon(self, width: u32, height: u32, rgba: impl Into<Vec<u8>>) -> Self {
        self.icon_rgba(width, height, rgba)
    }

    /// Sets the window and taskbar icon by decoding PNG, `.ico`, BMP, or GIF bytes.
    pub fn icon_png(mut self, png_or_ico_bytes: &[u8]) -> Self {
        if let Ok(img) = crate::gfx::image::decode_image(png_or_ico_bytes) {
            self.icon_rgba = Some((img.width, img.height, img.data));
        }
        self
    }

    /// Sets the window and taskbar icon by decoding Windows `.ico` or PNG bytes.
    pub fn icon_ico(self, ico_bytes: &[u8]) -> Self {
        self.icon_png(ico_bytes)
    }

    /// Bundles a custom TTF/OTF/TTC font under `name` for cross-platform and Wasm rendering.
    ///
    /// Chain multiple `.font(same_name, bytes)` calls to register bold/semibold weights under one family.
    pub fn font(mut self, name: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        self.fonts.push((name.into(), bytes.into()));
        self
    }

    /// Bundles a single TTF/OTF/TTC face under `name` and sets it as the global default UI sans-serif font.
    ///
    /// On `wasm32`, system fonts are unavailable and only the embedded `LumenicSans` regular face is loaded
    /// by default; bold/semibold/mono weights fall back to regular unless extra faces are chained with
    /// `.font(...)` / `.default_mono_font(...)` (italic is synthesized automatically when missing).
    pub fn default_font(mut self, name: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        let fam = name.into();
        self.fonts.push((fam.clone(), bytes.into()));
        self.default_font = Some(fam);
        self
    }

    /// Sets the global default UI sans-serif font family name or registered alias.
    pub fn default_font_family(mut self, name: impl Into<String>) -> Self {
        self.default_font = Some(name.into());
        self
    }

    /// Bundles a custom TTF/OTF/TTC font under `name` and sets it as the global default monospace font.
    pub fn default_mono_font(mut self, name: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        let fam = name.into();
        self.fonts.push((fam.clone(), bytes.into()));
        self.default_mono_font = Some(fam);
        self
    }

    /// Sets the global default UI monospace font family name or registered alias.
    pub fn default_mono_font_family(mut self, name: impl Into<String>) -> Self {
        self.default_mono_font = Some(name.into());
        self
    }

    /// Enables responsive full-viewport canvas sizing synced to CSS `100vw x 100dvh` and `devicePixelRatio` on `wasm32`.
    pub fn fluid(mut self) -> Self {
        self.fluid = true;
        self
    }
}

/// Plain data-driven default color and metric tokens read by widgets when not overridden per-widget.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    /// Primary application backdrop color.
    pub background: Color,
    /// Elevated panel / card surface color.
    pub surface: Color,
    /// Secondary/recessed surface fill color (inputs, headers, code blocks).
    pub surface_alt: Color,
    /// Primary interactive accent color.
    pub primary: Color,
    /// Contrasting foreground color over `primary` fills.
    pub on_primary: Color,
    /// Default high-contrast text color.
    pub text: Color,
    /// Secondary muted caption/placeholder text color.
    pub text_muted: Color,
    /// Hairline divider and control border color.
    pub border: Color,
    /// Keyboard focus outline highlight color.
    pub focus_ring: Color,
    /// Keyboard focus outline stroke width in logical points.
    pub focus_ring_width: f32,
    /// Keyboard focus outline outward offset in logical points.
    pub focus_ring_offset: f32,
    /// Default corner rounding radius in logical points.
    pub radius: f32,
    /// Default body font size in logical points.
    pub font_size: f32,
    /// Whether this theme uses a dark background palette.
    pub is_dark: bool,
    /// Whether pointer-down ripples are enabled globally (`false` by default; opt-in per-widget or via theme).
    pub ripple: bool,
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

impl Theme {
    /// Neutral dark fallback palette.
    pub const fn dark() -> Self {
        Self {
            background: Color::from_hex(0x16161A),
            surface: Color::from_hex(0x202026),
            surface_alt: Color::from_hex(0x2A2A32),
            primary: Color::from_hex(0x4DA3FF),
            on_primary: Color::WHITE,
            text: Color::from_hex(0xF4F4F6),
            text_muted: Color::from_hex(0xA8A8B3),
            border: Color::from_hex(0x3A3A46),
            focus_ring: Color::from_hex(0x7CC4FF),
            focus_ring_width: 2.0,
            focus_ring_offset: 0.0,
            radius: 10.0,
            font_size: 14.0,
            is_dark: true,
            ripple: false,
        }
    }

    /// Neutral light fallback palette.
    pub const fn light() -> Self {
        Self {
            background: Color::from_hex(0xF4F5F7),
            surface: Color::WHITE,
            surface_alt: Color::from_hex(0xE9ECEF),
            primary: Color::from_hex(0x0A84FF),
            on_primary: Color::WHITE,
            text: Color::from_hex(0x111827),
            text_muted: Color::from_hex(0x6B7280),
            border: Color::from_hex(0xD1D5DB),
            focus_ring: Color::from_hex(0x0A84FF),
            focus_ring_width: 2.0,
            focus_ring_offset: 0.0,
            radius: 10.0,
            font_size: 14.0,
            is_dark: false,
            ripple: false,
        }
    }
}

/// Two-stop linear or radial gradient definition for `StyleBuilder::bg_grad` / `bd_grad`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gradient {
    /// Start stop color (or center color when `radial` is `true`).
    pub c0: Color,
    /// End stop color (or outer edge color when `radial` is `true`).
    pub c1: Color,
    /// Linear gradient orientation angle in radians.
    pub angle_rad: f32,
    /// Selects radial center-to-edge interpolation instead of directional linear interpolation.
    pub radial: bool,
}

impl Gradient {
    /// Creates a linear gradient between `c0` and `c1` at `angle_deg` degrees (`0` = left-to-right, `90` = top-to-bottom).
    pub fn linear(c0: Color, c1: Color, angle_deg: f32) -> Self {
        Self {
            c0,
            c1,
            angle_rad: angle_deg.to_radians(),
            radial: false,
        }
    }

    /// Creates a center-out radial gradient from `c0` (center) to `c1` (edge).
    pub const fn radial(c0: Color, c1: Color) -> Self {
        Self {
            c0,
            c1,
            angle_rad: 0.0,
            radial: true,
        }
    }
}

/// Real-time frame telemetry displayed in the built-in `F12` overlay HUD and `ui_perf`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PerfStats {
    /// Total frame duration on CPU in milliseconds.
    pub frame_time_ms: f32,
    /// UI tree measure + layout CPU duration in milliseconds.
    pub layout_time_ms: f32,
    /// Renderer CPU submission duration in milliseconds.
    pub render_time_ms: f32,
    /// GPU `TIME_ELAPSED` query duration in milliseconds when supported.
    pub gpu_time_ms: Option<f32>,
    /// Total uploaded GPU vertex/instance/atlas buffer bytes tracked this frame.
    pub gpu_buffer_bytes: usize,
    /// Number of emitted GPU draw passes/calls this frame.
    pub draw_calls: usize,
    /// Number of instanced SDF quads submitted this frame.
    pub sdf_instances: usize,
    /// Total layout tree node count allocated this frame.
    pub node_count: usize,
    /// Per-frame bump arena bytes currently in use.
    pub arena_used_bytes: usize,
    /// Pre-allocated fixed capacity of the per-frame bump arena in bytes.
    pub arena_capacity_bytes: usize,
    /// High-water mark of bump arena usage across frames in bytes.
    pub arena_high_water_bytes: usize,
    /// Width and height of the RGBA8 glyph atlas texture in pixels.
    pub glyph_atlas_size: [u32; 2],
    /// Fraction (`0.0..=1.0`) of glyph atlas pixels currently populated.
    pub glyph_atlas_occupancy: f32,
}

/// Framework error type isolated from third-party dependency errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Window creation or platform event-loop error.
    Window(String),
    /// OpenGL / WebGL 2 shader compilation, context, or buffer error.
    Graphics(String),
    /// Filesystem, storage, image I/O, or network error.
    Io(String),
    /// Validation error such as unsafe paths, malformed URLs, or bad payload bounds.
    InvalidInput(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Window(msg) => write!(f, "window error: {msg}"),
            Self::Graphics(msg) => write!(f, "graphics error: {msg}"),
            Self::Io(msg) => write!(f, "I/O error: {msg}"),
            Self::InvalidInput(msg) => write!(f, "invalid input: {msg}"),
        }
    }
}

impl std::error::Error for Error {}
