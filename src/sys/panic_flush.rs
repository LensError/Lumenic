#![forbid(unsafe_code)]

use std::sync::{Mutex, OnceLock};

type FlushCallback = Box<dyn Fn() + Send + Sync>;

fn registry() -> &'static Mutex<Vec<FlushCallback>> {
    static FLUSHERS: OnceLock<Mutex<Vec<FlushCallback>>> = OnceLock::new();
    FLUSHERS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Installs the global panic-flush hook once (compatible with `panic = "abort"`).
pub fn install_panic_flush_hook() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            run_panic_flush_callbacks();
            prev_hook(info);
        }));
    });
}

/// Registers a thread-safe dirty-state flush callback invoked automatically on panic.
pub fn register_panic_flush<F>(callback: F)
where
    F: Fn() + Send + Sync + 'static,
{
    install_panic_flush_hook();
    match registry().lock() {
        Ok(mut guard) => guard.push(Box::new(callback)),
        Err(poisoned) => poisoned.into_inner().push(Box::new(callback)),
    }
}

/// Runs all registered panic-flush callbacks (safe to invoke during clean shutdown too).
pub fn run_panic_flush_callbacks() {
    let guard = match registry().lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    for cb in guard.iter() {
        cb();
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use super::{register_panic_flush, run_panic_flush_callbacks};

    #[test]
    fn panic_flush_registry_invokes_callbacks_and_flushes_file() {
        let out_file = std::env::temp_dir().join("lumenic_panic_flush_test.txt");
        let _ = fs::remove_file(&out_file);

        let flag = Arc::new(AtomicBool::new(false));
        let flag_clone = Arc::clone(&flag);
        let path_clone = out_file.clone();

        register_panic_flush(move || {
            flag_clone.store(true, Ordering::SeqCst);
            let _ = fs::write(&path_clone, b"flushed_on_panic");
        });

        run_panic_flush_callbacks();
        assert!(flag.load(Ordering::SeqCst));
        assert_eq!(
            fs::read(&out_file).expect("flushed file"),
            b"flushed_on_panic"
        );
        let _ = fs::remove_file(&out_file);
    }
}
