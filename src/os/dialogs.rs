#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::os::Waker;

/// Parameters for an asynchronous native file open or save dialog.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileDialogRequest {
    /// Dialog window title shown by the OS picker.
    pub title: String,
    /// Suggested default file name or path.
    pub default_name: Option<String>,
    /// Allowed file extensions (without leading dot, e.g. `["png", "jpg"]`).
    pub extensions: Vec<String>,
}

/// Non-blocking handle for polling completion of a worker-thread OS file dialog.
#[derive(Clone, Debug)]
pub struct DialogHandle {
    result: Arc<Mutex<Option<Option<PathBuf>>>>,
}

impl DialogHandle {
    fn new_pending() -> Self {
        Self {
            result: Arc::new(Mutex::new(None)),
        }
    }

    #[cfg(test)]
    pub(crate) fn completed(result: Option<PathBuf>) -> Self {
        Self {
            result: Arc::new(Mutex::new(Some(result))),
        }
    }

    /// Returns `true` while the file dialog worker thread has not completed yet.
    pub fn is_pending(&self) -> bool {
        match self.result.lock() {
            Ok(g) => g.is_none(),
            Err(p) => p.into_inner().is_none(),
        }
    }

    /// Polls once for completion: `None` means still pending, `Some(choice)` returns the result.
    pub fn poll(&self) -> Option<Option<PathBuf>> {
        match self.result.lock() {
            Ok(g) => g.clone(),
            Err(p) => p.into_inner().clone(),
        }
    }

    fn complete(&self, val: Option<PathBuf>, waker: &Waker) {
        match self.result.lock() {
            Ok(mut g) => *g = Some(val),
            Err(p) => *p.into_inner() = Some(val),
        }
        waker.wake();
    }
}

fn spawn_file_dialog(req: FileDialogRequest, waker: Waker, save: bool) -> DialogHandle {
    let handle = DialogHandle::new_pending();
    #[cfg(all(not(target_arch = "wasm32"), feature = "dialogs"))]
    {
        let h = handle.clone();
        std::thread::spawn(move || {
            let mut dlg = rfd::FileDialog::new().set_title(&req.title);
            if save {
                if let Some(ref name) = req.default_name {
                    dlg = dlg.set_file_name(name);
                }
            }
            if !req.extensions.is_empty() {
                let exts: Vec<&str> = req.extensions.iter().map(String::as_str).collect();
                dlg = dlg.add_filter("Files", &exts);
            }
            let picked = if save {
                dlg.save_file()
            } else {
                dlg.pick_file()
            };
            h.complete(picked, &waker);
        });
    }
    #[cfg(not(all(not(target_arch = "wasm32"), feature = "dialogs")))]
    {
        let _ = (req, save);
        handle.complete(None, &waker);
    }
    handle
}

/// Spawns a non-blocking open-file picker on a background thread and wakes the UI via `waker`.
pub fn spawn_pick_file(req: FileDialogRequest, waker: Waker) -> DialogHandle {
    spawn_file_dialog(req, waker, false)
}

/// Spawns a non-blocking save-file dialog on a background thread and wakes the UI via `waker`.
pub fn spawn_save_file(req: FileDialogRequest, waker: Waker) -> DialogHandle {
    spawn_file_dialog(req, waker, true)
}

/// Spawns a non-blocking folder picker dialog on a background thread and wakes the UI via `waker`.
pub fn spawn_pick_folder(title: &str, waker: Waker) -> DialogHandle {
    let handle = DialogHandle::new_pending();
    #[cfg(all(not(target_arch = "wasm32"), feature = "dialogs"))]
    {
        let h = handle.clone();
        let title_owned = String::from(title);
        std::thread::spawn(move || {
            let picked = rfd::FileDialog::new().set_title(&title_owned).pick_folder();
            h.complete(picked, &waker);
        });
    }
    #[cfg(not(all(not(target_arch = "wasm32"), feature = "dialogs")))]
    {
        let _ = title;
        handle.complete(None, &waker);
    }
    handle
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use super::DialogHandle;
    use crate::os::Waker;

    #[test]
    fn dialog_handle_transitions_and_invokes_waker() {
        let handle = DialogHandle::new_pending();
        assert!(handle.is_pending());
        assert_eq!(handle.poll(), None);

        let woken = Arc::new(AtomicBool::new(false));
        let w_flag = Arc::clone(&woken);
        let waker = Waker::from_fn(move || {
            w_flag.store(true, Ordering::SeqCst);
        });

        let expected = PathBuf::from("demo.txt");
        handle.complete(Some(expected.clone()), &waker);
        assert!(woken.load(Ordering::SeqCst));
        assert!(!handle.is_pending());
        assert_eq!(handle.poll(), Some(Some(expected)));

        let ready = DialogHandle::completed(None);
        assert!(!ready.is_pending());
        assert_eq!(ready.poll(), Some(None));
    }
}
