#![forbid(unsafe_code)]

use std::sync::{Mutex, OnceLock};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(inline_js = r#"
export function lumenic_write_clipboard(text) {
    try {
        const nav = globalThis.navigator;
        if (nav && nav.clipboard && typeof nav.clipboard.writeText === 'function') {
            Promise.resolve(nav.clipboard.writeText(text)).catch(() => {});
        }
    } catch (_) {}
}
"#)]
extern "C" {
    fn lumenic_write_clipboard(text: &str);
}

fn fallback_clipboard() -> &'static Mutex<String> {
    static FALLBACK: OnceLock<Mutex<String>> = OnceLock::new();
    FALLBACK.get_or_init(|| Mutex::new(String::new()))
}

#[cfg(all(not(target_arch = "wasm32"), feature = "clipboard"))]
fn native_clipboard() -> &'static Mutex<Option<arboard::Clipboard>> {
    static CLIPBOARD: OnceLock<Mutex<Option<arboard::Clipboard>>> = OnceLock::new();
    CLIPBOARD.get_or_init(|| Mutex::new(arboard::Clipboard::new().ok()))
}

/// Caches pasted text from a platform event (such as Wasm DOM `paste`) into the fallback buffer.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn cache_text(text: &str) {
    if let Ok(mut guard) = fallback_clipboard().lock() {
        guard.clear();
        guard.push_str(text);
    }
}

/// Reads UTF-8 text from the OS clipboard (or in-memory / DOM-cached fallback on Wasm/headless).
pub fn get_text() -> Option<String> {
    #[cfg(all(not(target_arch = "wasm32"), feature = "clipboard"))]
    {
        if let Ok(mut guard) = native_clipboard().lock() {
            if guard.is_none() {
                *guard = arboard::Clipboard::new().ok();
            }
            if let Some(cb) = guard.as_mut() {
                if let Ok(text) = cb.get_text() {
                    return Some(text);
                }
            }
        }
    }
    fallback_clipboard()
        .lock()
        .ok()
        .map(|s| s.clone())
        .filter(|s| !s.is_empty())
}

/// Writes UTF-8 text to the OS or browser Clipboard API and mirrors to the fallback buffer.
pub fn set_text(text: &str) {
    cache_text(text);
    #[cfg(target_arch = "wasm32")]
    lumenic_write_clipboard(text);
    #[cfg(all(not(target_arch = "wasm32"), feature = "clipboard"))]
    {
        if let Ok(mut guard) = native_clipboard().lock() {
            if guard.is_none() {
                *guard = arboard::Clipboard::new().ok();
            }
            if let Some(cb) = guard.as_mut() {
                let _ = cb.set_text(text.to_owned());
            }
        }
    }
}
