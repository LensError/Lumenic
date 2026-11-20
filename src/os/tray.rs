#![forbid(unsafe_code)]

use crate::gfx::image::decode_image;

/// Menu item entry for a native system tray icon ([`TrayConfig`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayMenuItem {
    /// Standard clickable menu item with a caller-defined `id` and display `label`.
    Item {
        id: String,
        label: String,
        enabled: bool,
    },
    /// Checkable menu item with a caller-defined `id`, `label`, and `checked` state.
    Check {
        id: String,
        label: String,
        checked: bool,
        enabled: bool,
    },
    /// Horizontal menu separator.
    Separator,
}

impl TrayMenuItem {
    /// Creates an enabled clickable tray menu item.
    pub fn item(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::Item {
            id: id.into(),
            label: label.into(),
            enabled: true,
        }
    }

    /// Creates a disabled clickable tray menu item.
    pub fn disabled(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::Item {
            id: id.into(),
            label: label.into(),
            enabled: false,
        }
    }

    /// Creates an enabled checkable tray menu item.
    pub fn check(id: impl Into<String>, label: impl Into<String>, checked: bool) -> Self {
        Self::Check {
            id: id.into(),
            label: label.into(),
            checked,
            enabled: true,
        }
    }

    /// Creates a horizontal separator entry.
    pub fn separator() -> Self {
        Self::Separator
    }
}

/// Configuration for creating or updating a native system tray icon via [`crate::App::set_tray_icon`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrayConfig {
    /// Hover tooltip text displayed by the OS tray.
    pub tooltip: Option<String>,
    /// Optional RGBA8 icon pixels `(width, height, rgba_bytes)`.
    pub icon_rgba: Option<(u32, u32, Vec<u8>)>,
    /// Context menu items shown on tray menu activation.
    pub menu: Vec<TrayMenuItem>,
}

impl TrayConfig {
    /// Creates an empty tray icon configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the OS tray tooltip text.
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Sets the tray icon from raw RGBA8 pixel data (`width * height * 4` bytes).
    pub fn icon_rgba(mut self, width: u32, height: u32, rgba: Vec<u8>) -> Self {
        self.icon_rgba = Some((width, height, rgba));
        self
    }

    /// Sets the tray icon by decoding an encoded image (PNG, ICO, BMP, JPEG, WebP, or GIF).
    pub fn icon_png(mut self, bytes: &[u8]) -> Self {
        if let Ok(img) = decode_image(bytes) {
            self.icon_rgba = Some((img.width, img.height, img.data));
        }
        self
    }

    /// Appends a clickable menu item to the tray context menu.
    pub fn item(mut self, id: impl Into<String>, label: impl Into<String>) -> Self {
        self.menu.push(TrayMenuItem::item(id, label));
        self
    }

    /// Appends a checkable menu item to the tray context menu.
    pub fn check(mut self, id: impl Into<String>, label: impl Into<String>, checked: bool) -> Self {
        self.menu.push(TrayMenuItem::check(id, label, checked));
        self
    }

    /// Appends a horizontal separator to the tray context menu.
    pub fn separator(mut self) -> Self {
        self.menu.push(TrayMenuItem::separator());
        self
    }

    /// Replaces the full tray context menu item list.
    pub fn menu(mut self, items: Vec<TrayMenuItem>) -> Self {
        self.menu = items;
        self
    }
}

/// Event emitted by the native system tray icon and drained via [`crate::App::take_tray_events`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    /// Primary (left) click on the tray icon.
    Click,
    /// Double-click on the tray icon.
    DoubleClick,
    /// Secondary (right) click on the tray icon.
    RightClick,
    /// A tray context menu item with the given `id` was activated.
    Menu(String),
}

#[cfg_attr(any(target_arch = "wasm32", not(feature = "tray")), allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TrayCommand {
    Set(TrayConfig),
    UpdateMenu(Vec<TrayMenuItem>),
    SetTooltip(Option<String>),
    Remove,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "tray"))]
pub(crate) mod backend {
    use std::sync::{Arc, Mutex};

    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{
        Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    };

    use super::{TrayCommand, TrayConfig, TrayEvent, TrayMenuItem};
    use crate::os::Waker;

    pub(crate) struct NativeTray {
        icon: Option<TrayIcon>,
        events: Arc<Mutex<Vec<TrayEvent>>>,
    }

    impl NativeTray {
        pub(crate) fn new(waker: Waker) -> Self {
            let events = Arc::new(Mutex::new(Vec::new()));

            let ev_tray = Arc::clone(&events);
            let w_tray = waker.clone();
            TrayIconEvent::set_event_handler(Some(move |ev: TrayIconEvent| {
                let mapped = match ev {
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } => Some(TrayEvent::Click),
                    TrayIconEvent::Click {
                        button: MouseButton::Right,
                        button_state: MouseButtonState::Up,
                        ..
                    } => Some(TrayEvent::RightClick),
                    TrayIconEvent::DoubleClick {
                        button: MouseButton::Left,
                        ..
                    } => Some(TrayEvent::DoubleClick),
                    _ => None,
                };
                if let Some(te) = mapped {
                    if let Ok(mut q) = ev_tray.lock() {
                        q.push(te);
                    }
                    w_tray.wake();
                }
            }));

            let ev_menu = Arc::clone(&events);
            MenuEvent::set_event_handler(Some(move |ev: MenuEvent| {
                if let Ok(mut q) = ev_menu.lock() {
                    q.push(TrayEvent::Menu(ev.id.0));
                }
                waker.wake();
            }));

            Self { icon: None, events }
        }

        pub(crate) fn take_events(&self) -> Vec<TrayEvent> {
            match self.events.lock() {
                Ok(mut q) => core::mem::take(&mut *q),
                Err(_) => Vec::new(),
            }
        }

        pub(crate) fn apply(&mut self, cmd: TrayCommand) {
            match cmd {
                TrayCommand::Set(cfg) => self.apply_config(cfg),
                TrayCommand::UpdateMenu(items) => {
                    if let Some(ref tray) = self.icon {
                        let menu = build_menu(&items);
                        tray.set_menu(Some(Box::new(menu)));
                    }
                }
                TrayCommand::SetTooltip(tooltip) => {
                    if let Some(ref tray) = self.icon {
                        let _ = tray.set_tooltip(tooltip);
                    }
                }
                TrayCommand::Remove => {
                    self.icon = None;
                }
            }
        }

        fn apply_config(&mut self, cfg: TrayConfig) {
            let menu = (!cfg.menu.is_empty()).then(|| build_menu(&cfg.menu));
            let icon = cfg
                .icon_rgba
                .and_then(|(w, h, rgba)| Icon::from_rgba(rgba, w, h).ok())
                .or_else(default_fallback_icon);

            if let Some(ref tray) = self.icon {
                if let Some(ic) = icon {
                    let _ = tray.set_icon(Some(ic));
                }
                let _ = tray.set_tooltip(cfg.tooltip);
                tray.set_menu(menu.map(|m| Box::new(m) as Box<dyn tray_icon::menu::ContextMenu>));
                return;
            }

            let mut builder = TrayIconBuilder::new();
            if let Some(ic) = icon {
                builder = builder.with_icon(ic);
            }
            if let Some(tip) = cfg.tooltip {
                builder = builder.with_tooltip(tip);
            }
            if let Some(m) = menu {
                builder = builder.with_menu(Box::new(m));
            }
            if let Ok(tray) = builder.build() {
                self.icon = Some(tray);
            }
        }
    }

    fn build_menu(items: &[TrayMenuItem]) -> Menu {
        let menu = Menu::new();
        for item in items {
            match item {
                TrayMenuItem::Item { id, label, enabled } => {
                    let entry = MenuItem::with_id(id.as_str(), label.as_str(), *enabled, None);
                    let _ = menu.append(&entry);
                }
                TrayMenuItem::Check {
                    id,
                    label,
                    checked,
                    enabled,
                } => {
                    let entry = CheckMenuItem::with_id(
                        id.as_str(),
                        label.as_str(),
                        *enabled,
                        *checked,
                        None,
                    );
                    let _ = menu.append(&entry);
                }
                TrayMenuItem::Separator => {
                    let _ = menu.append(&PredefinedMenuItem::separator());
                }
            }
        }
        menu
    }

    fn default_fallback_icon() -> Option<Icon> {
        let mut rgba = vec![0u8; 16 * 16 * 4];
        for y in 0..16u32 {
            for x in 0..16u32 {
                let idx = ((y * 16 + x) * 4) as usize;
                let border = x == 0 || y == 0 || x == 15 || y == 15;
                if border {
                    rgba[idx..idx + 4].copy_from_slice(&[56, 130, 246, 255]);
                } else {
                    rgba[idx..idx + 4].copy_from_slice(&[30, 41, 59, 255]);
                }
            }
        }
        Icon::from_rgba(rgba, 16, 16).ok()
    }
}
