use winit::event::{MouseButton, MouseScrollDelta};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};
use winit::window::CursorIcon as WinitCursorIcon;

use crate::ui::types::{CursorIcon, Key, Modifiers, PointerButton};

pub fn map_cursor_icon(cursor: CursorIcon) -> WinitCursorIcon {
    match cursor {
        CursorIcon::Default => WinitCursorIcon::Default,
        CursorIcon::Pointer => WinitCursorIcon::Pointer,
        CursorIcon::Text => WinitCursorIcon::Text,
        CursorIcon::Grab => WinitCursorIcon::Grab,
        CursorIcon::Grabbing => WinitCursorIcon::Grabbing,
        CursorIcon::Crosshair => WinitCursorIcon::Crosshair,
        CursorIcon::ResizeH => WinitCursorIcon::EwResize,
        CursorIcon::ResizeV => WinitCursorIcon::NsResize,
        CursorIcon::ResizeNwse => WinitCursorIcon::NwseResize,
        CursorIcon::ResizeNesw => WinitCursorIcon::NeswResize,
        CursorIcon::NotAllowed => WinitCursorIcon::NotAllowed,
    }
}

pub fn map_scroll_delta(delta: MouseScrollDelta, scale_factor: f32) -> [f32; 2] {
    match delta {
        MouseScrollDelta::LineDelta(x, y) => [x, y],
        MouseScrollDelta::PixelDelta(pos) => {
            let s = f64::from(scale_factor.max(1.0)) * 24.0;
            [(pos.x / s) as f32, (pos.y / s) as f32]
        }
    }
}

pub fn map_modifiers(state: ModifiersState) -> Modifiers {
    Modifiers {
        shift: state.shift_key(),
        ctrl: state.control_key(),
        alt: state.alt_key(),
        super_key: state.super_key(),
    }
}

pub fn map_mouse_button(button: MouseButton) -> Option<PointerButton> {
    match button {
        MouseButton::Left => Some(PointerButton::Left),
        MouseButton::Right => Some(PointerButton::Right),
        MouseButton::Middle => Some(PointerButton::Middle),
        _ => None,
    }
}

pub fn map_key(key: &WinitKey) -> Key {
    match key {
        WinitKey::Named(named) => match named {
            NamedKey::Tab => Key::Tab,
            NamedKey::Enter => Key::Enter,
            NamedKey::Space => Key::Space,
            NamedKey::Escape => Key::Escape,
            NamedKey::Backspace => Key::Backspace,
            NamedKey::Delete => Key::Delete,
            NamedKey::Insert => Key::Insert,
            NamedKey::ArrowLeft => Key::Left,
            NamedKey::ArrowRight => Key::Right,
            NamedKey::ArrowUp => Key::Up,
            NamedKey::ArrowDown => Key::Down,
            NamedKey::Home => Key::Home,
            NamedKey::End => Key::End,
            NamedKey::PageUp => Key::PageUp,
            NamedKey::PageDown => Key::PageDown,
            NamedKey::F1 => Key::F1,
            NamedKey::F2 => Key::F2,
            NamedKey::F3 => Key::F3,
            NamedKey::F4 => Key::F4,
            NamedKey::F5 => Key::F5,
            NamedKey::F6 => Key::F6,
            NamedKey::F7 => Key::F7,
            NamedKey::F8 => Key::F8,
            NamedKey::F9 => Key::F9,
            NamedKey::F10 => Key::F10,
            NamedKey::F11 => Key::F11,
            NamedKey::F12 => Key::F12,
            _ => Key::Other(0),
        },
        WinitKey::Character(s) => s.chars().next().map_or(Key::Other(0), |ch| {
            if ch == ' ' {
                Key::Space
            } else {
                Key::Char(ch)
            }
        }),
        _ => Key::Other(0),
    }
}
