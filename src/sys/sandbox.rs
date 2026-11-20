#![forbid(unsafe_code)]

#[cfg(target_arch = "wasm32")]
use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::fs::{self, OpenOptions};
#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
use std::path::{Component, Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(target_arch = "wasm32")]
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};

use crate::sys::panic_flush::register_panic_flush;
use crate::ui::types::Error;

#[cfg(not(target_arch = "wasm32"))]
static TMP_COUNTER: AtomicU64 = AtomicU64::new(1);

#[cfg(target_arch = "wasm32")]
fn wasm_mem_storage() -> &'static Mutex<HashMap<String, Vec<u8>>> {
    static MEM: OnceLock<Mutex<HashMap<String, Vec<u8>>>> = OnceLock::new();
    MEM.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Storage root resolution strategy for `SafeDir`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StorageMode {
    /// Stores data under OS user config directory (`%APPDATA%` on Windows, `~/.config` on Unix).
    AppData {
        /// Application subdirectory or `localStorage` namespace prefix.
        app_name: String,
    },
    /// Stores data in an explicit portable directory (e.g. `./data`).
    Portable {
        /// Explicit base directory path.
        dir: PathBuf,
    },
}

/// Traversal-resistant directory sandbox enforcing canonical containment and atomic writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafeDir {
    root: PathBuf,
}

impl SafeDir {
    /// Opens or creates a sandbox rooted at canonicalized `root` (or namespaced `localStorage` on `wasm32`).
    pub fn open(root: impl AsRef<Path>) -> Result<Self, Error> {
        #[cfg(target_arch = "wasm32")]
        {
            let name = root.as_ref().to_string_lossy().into_owned();
            if name.trim().is_empty() {
                return Err(Error::InvalidInput(String::from(
                    "SafeDir root name must not be empty",
                )));
            }
            Ok(Self {
                root: PathBuf::from(name),
            })
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let root_ref = root.as_ref();
            fs::create_dir_all(root_ref).map_err(|e| {
                Error::Io(format!(
                    "failed to create SafeDir {}: {e}",
                    root_ref.display()
                ))
            })?;
            let canonical = fs::canonicalize(root_ref).map_err(|e| {
                Error::Io(format!(
                    "failed to canonicalize SafeDir {}: {e}",
                    root_ref.display()
                ))
            })?;
            Ok(Self { root: canonical })
        }
    }

    /// Opens a `SafeDir` using standard OS user config or portable directory mode.
    pub fn from_mode(mode: StorageMode) -> Result<Self, Error> {
        let dir = match mode {
            StorageMode::AppData { app_name } => {
                if app_name.is_empty() || app_name.contains(['/', '\\']) || app_name.contains("..")
                {
                    return Err(Error::InvalidInput(String::from(
                        "invalid application directory name",
                    )));
                }
                resolve_app_data_dir(&app_name)
            }
            StorageMode::Portable { dir } => dir,
        };
        Self::open(dir)
    }

    /// Returns the canonicalized sandbox root path.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolves a relative path strictly inside `root`, rejecting `..`, absolutes, and symlinks.
    pub fn resolve(&self, rel: impl AsRef<Path>) -> Result<PathBuf, Error> {
        let rel = rel.as_ref();
        if rel.as_os_str().is_empty() || rel.is_absolute() {
            return Err(Error::InvalidInput(String::from(
                "SafeDir path must be non-empty and relative",
            )));
        }

        let mut normal_parts = Vec::new();
        for comp in rel.components() {
            match comp {
                Component::Normal(part) => normal_parts.push(part),
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(Error::InvalidInput(format!(
                        "path traversal or root component blocked: {}",
                        rel.display()
                    )));
                }
            }
        }
        if normal_parts.is_empty() {
            return Err(Error::InvalidInput(String::from(
                "SafeDir path must reference a relative entry",
            )));
        }

        let mut cur = self.root.clone();
        #[cfg(target_arch = "wasm32")]
        {
            for part in normal_parts {
                cur.push(part);
            }
            Ok(cur)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            for part in normal_parts {
                cur.push(part);
                if let Ok(meta) = fs::symlink_metadata(&cur) {
                    if meta.file_type().is_symlink() {
                        return Err(Error::InvalidInput(format!(
                            "symlink blocked inside SafeDir: {}",
                            cur.display()
                        )));
                    }
                    let canon = fs::canonicalize(&cur).map_err(|e| {
                        Error::Io(format!("failed to canonicalize {}: {e}", cur.display()))
                    })?;
                    if !canon.starts_with(&self.root) {
                        return Err(Error::InvalidInput(format!(
                            "path escapes SafeDir root: {}",
                            canon.display()
                        )));
                    }
                }
            }

            Ok(cur)
        }
    }

    /// Reads an entire entry inside `SafeDir` (file on native, `localStorage` on `wasm32`).
    pub fn read(&self, rel: impl AsRef<Path>) -> Result<Vec<u8>, Error> {
        let target = self.resolve(rel)?;
        #[cfg(target_arch = "wasm32")]
        {
            let key = wasm_storage_key(&self.root, &target);
            if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten())
            {
                match storage.get_item(&key) {
                    Ok(Some(encoded)) => return decode_b64(&encoded),
                    Ok(None) => {
                        return Err(Error::Io(format!("key not found in localStorage: {key}")));
                    }
                    Err(_) => {}
                }
            }
            if let Ok(guard) = wasm_mem_storage().lock() {
                if let Some(bytes) = guard.get(&key) {
                    return Ok(bytes.clone());
                }
            }
            Err(Error::Io(format!("key not found: {key}")))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let canon = fs::canonicalize(&target)
                .map_err(|e| Error::Io(format!("failed to read {}: {e}", target.display())))?;
            if !canon.starts_with(&self.root) {
                return Err(Error::InvalidInput(String::from(
                    "canonical path escapes SafeDir root",
                )));
            }
            fs::read(&canon)
                .map_err(|e| Error::Io(format!("failed to read {}: {e}", canon.display())))
        }
    }

    /// Reads an entire UTF-8 text file inside `SafeDir`.
    pub fn read_to_string(&self, rel: impl AsRef<Path>) -> Result<String, Error> {
        let bytes = self.read(rel)?;
        String::from_utf8(bytes)
            .map_err(|e| Error::InvalidInput(format!("file is not valid UTF-8: {e}")))
    }

    /// Atomically writes `data` to `rel` using a synced sibling temp file + rename (or `localStorage` on `wasm32`).
    pub fn write_atomic(&self, rel: impl AsRef<Path>, data: &[u8]) -> Result<(), Error> {
        self.write_atomic_internal(rel.as_ref(), data, false)
    }

    fn write_atomic_internal(
        &self,
        rel: &Path,
        data: &[u8],
        fail_after_temp_write: bool,
    ) -> Result<(), Error> {
        let dst = self.resolve(rel)?;
        #[cfg(target_arch = "wasm32")]
        {
            if fail_after_temp_write {
                return Err(Error::Io(String::from("simulated crash before commit")));
            }
            let key = wasm_storage_key(&self.root, &dst);
            if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten())
            {
                let encoded = encode_b64(data);
                if storage.set_item(&key, &encoded).is_ok() {
                    return Ok(());
                }
            }
            if let Ok(mut guard) = wasm_mem_storage().lock() {
                guard.insert(key, data.to_vec());
            }
            Ok(())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let parent = dst.parent().ok_or_else(|| {
                Error::InvalidInput(String::from("destination has no parent directory"))
            })?;
            fs::create_dir_all(parent)
                .map_err(|e| Error::Io(format!("failed to create parent dirs: {e}")))?;

            let canon_parent = fs::canonicalize(parent)
                .map_err(|e| Error::Io(format!("failed to canonicalize parent: {e}")))?;
            if !canon_parent.starts_with(&self.root) {
                return Err(Error::InvalidInput(String::from(
                    "parent directory escapes SafeDir root",
                )));
            }

            let seq = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let file_name = dst.file_name().and_then(|n| n.to_str()).unwrap_or("data");
            let tmp_path = canon_parent.join(format!(".{file_name}.lumenic_tmp_{seq}"));

            {
                let mut f = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&tmp_path)
                    .map_err(|e| Error::Io(format!("failed to open temp file: {e}")))?;
                if let Err(e) = f.write_all(data) {
                    let _ = fs::remove_file(&tmp_path);
                    return Err(Error::Io(format!("failed to write temp file: {e}")));
                }
                if let Err(e) = f.sync_all() {
                    let _ = fs::remove_file(&tmp_path);
                    return Err(Error::Io(format!("failed to sync temp file: {e}")));
                }
            }

            if fail_after_temp_write {
                let _ = fs::remove_file(&tmp_path);
                return Err(Error::Io(String::from(
                    "simulated crash before atomic rename",
                )));
            }

            if let Err(e) = fs::rename(&tmp_path, &dst) {
                let _ = fs::remove_file(&tmp_path);
                return Err(Error::Io(format!("atomic rename failed: {e}")));
            }
            Ok(())
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn wasm_storage_key(root: &Path, full: &Path) -> String {
    let rel = full.strip_prefix(root).unwrap_or(full);
    let root_s = root.to_string_lossy().replace('\\', "/");
    let rel_s = rel.to_string_lossy().replace('\\', "/");
    format!("lumenic:{root_s}:{rel_s}")
}

#[cfg(any(target_arch = "wasm32", test))]
fn encode_b64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((triple >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(triple & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(any(target_arch = "wasm32", test))]
fn decode_b64(encoded: &str) -> Result<Vec<u8>, Error> {
    let clean: Vec<u8> = encoded
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if !clean.len().is_multiple_of(4) {
        return Err(Error::InvalidInput(String::from("invalid base64 length")));
    }
    let val = |b: u8| -> Option<u32> {
        match b {
            b'A'..=b'Z' => Some(u32::from(b - b'A')),
            b'a'..=b'z' => Some(u32::from(b - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(b - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let mut out = Vec::with_capacity((clean.len() / 4) * 3);
    for chunk in clean.as_chunks::<4>().0 {
        let a = val(chunk[0]).ok_or_else(|| Error::InvalidInput(String::from("invalid base64")))?;
        let b = val(chunk[1]).ok_or_else(|| Error::InvalidInput(String::from("invalid base64")))?;
        let c = if chunk[2] == b'=' {
            0
        } else {
            val(chunk[2]).ok_or_else(|| Error::InvalidInput(String::from("invalid base64")))?
        };
        let d = if chunk[3] == b'=' {
            0
        } else {
            val(chunk[3]).ok_or_else(|| Error::InvalidInput(String::from("invalid base64")))?
        };
        let triple = (a << 18) | (b << 12) | (c << 6) | d;
        out.push(((triple >> 16) & 0xFF) as u8);
        if chunk[2] != b'=' {
            out.push(((triple >> 8) & 0xFF) as u8);
        }
        if chunk[3] != b'=' {
            out.push((triple & 0xFF) as u8);
        }
    }
    Ok(out)
}

fn resolve_app_data_dir(app_name: &str) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(base) = std::env::var_os("APPDATA") {
            return PathBuf::from(base).join(app_name);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(xdg).join(app_name);
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".config").join(app_name);
        }
    }
    PathBuf::from("./data").join(app_name)
}

/// Coalesces frequent edits and atomically flushes state after `delay_s` of inactivity or on panic/drop.
#[derive(Clone, Debug)]
pub struct DebouncedSaver {
    safe_dir: SafeDir,
    rel_path: PathBuf,
    delay_s: f32,
    pending_data: Arc<Mutex<Option<Vec<u8>>>>,
    timer_s: f32,
}

impl DebouncedSaver {
    /// Creates a new debounced atomic writer inside `safe_dir` and registers it with the global `panic = "abort"` flush hook.
    pub fn new(safe_dir: SafeDir, rel_path: impl Into<PathBuf>, delay_s: f32) -> Self {
        let rel_path = rel_path.into();
        let pending_data: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
        let flush_dir = safe_dir.clone();
        let flush_path = rel_path.clone();
        let flush_pending = Arc::clone(&pending_data);
        register_panic_flush(move || {
            let mut guard = match flush_pending.lock() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
            if let Some(data) = guard.take() {
                let _ = flush_dir.write_atomic(&flush_path, &data);
            }
        });
        Self {
            safe_dir,
            rel_path,
            delay_s: delay_s.max(0.0),
            pending_data,
            timer_s: 0.0,
        }
    }

    /// Stages new payload bytes and resets the debounce countdown.
    pub fn stage(&mut self, data: Vec<u8>) {
        let mut guard = match self.pending_data.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        *guard = Some(data);
        self.timer_s = 0.0;
    }

    /// Returns `true` when unsaved changes are pending flush.
    pub fn is_dirty(&self) -> bool {
        let guard = match self.pending_data.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.is_some()
    }

    /// Advances the debounce clock by `dt` seconds and flushes if expired. Returns `true` if flushed.
    pub fn tick(&mut self, dt: f32) -> Result<bool, Error> {
        if !self.is_dirty() {
            return Ok(false);
        }
        if dt.is_finite() && dt > 0.0 {
            self.timer_s += dt;
        }
        if self.timer_s >= self.delay_s {
            return self.flush_now();
        }
        Ok(false)
    }

    /// Immediately flushes any pending payload via `SafeDir::write_atomic`.
    pub fn flush_now(&mut self) -> Result<bool, Error> {
        let taken = {
            let mut guard = match self.pending_data.lock() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
            guard.take()
        };
        let Some(data) = taken else {
            return Ok(false);
        };
        if let Err(e) = self.safe_dir.write_atomic(&self.rel_path, &data) {
            let mut guard = match self.pending_data.lock() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
            *guard = Some(data);
            return Err(e);
        }
        self.timer_s = 0.0;
        Ok(true)
    }
}

impl Drop for DebouncedSaver {
    fn drop(&mut self) {
        let _ = self.flush_now();
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{DebouncedSaver, SafeDir, StorageMode};

    static TEST_SEQ: AtomicU64 = AtomicU64::new(1);

    fn temp_test_dir(tag: &str) -> PathBuf {
        let n = TEST_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("lumenic_safedir_{tag}_{n}"));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn rejects_traversal_and_absolute_paths() {
        let base = temp_test_dir("traversal");
        let safe = SafeDir::open(&base).expect("open SafeDir");

        assert!(safe.resolve("../escape.txt").is_err());
        assert!(safe.resolve("sub/../../escape.txt").is_err());
        assert!(safe.resolve("").is_err());
        assert!(safe.resolve(".").is_err());
        assert!(safe.resolve("/etc/passwd").is_err());
        #[cfg(windows)]
        assert!(safe.resolve(r"C:\Windows\win.ini").is_err());

        let ok = safe.resolve("nested/config.json").expect("valid rel path");
        assert!(ok.starts_with(safe.root()));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn rejects_symlink_escaping_root() {
        let base = temp_test_dir("symlink");
        let root = base.join("sandbox");
        let outside = base.join("outside");
        fs::create_dir_all(&outside).expect("outside dir");
        fs::write(outside.join("secret.txt"), b"secret").expect("secret file");
        let safe = SafeDir::open(&root).expect("open SafeDir");

        let link_dir = safe.root().join("escape_link");
        let created = {
            #[cfg(windows)]
            {
                std::os::windows::fs::symlink_dir(&outside, &link_dir)
            }
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&outside, &link_dir)
            }
        };

        if created.is_ok() {
            assert!(safe.resolve("escape_link/secret.txt").is_err());
            assert!(safe.read("escape_link/secret.txt").is_err());
            assert!(safe.write_atomic("escape_link/pwn.txt", b"bad").is_err());
        } else {
            // Unprivileged Windows runner without Developer Mode: still validate canonical prefix reject.
            let outside_canon = fs::canonicalize(&outside).expect("canon");
            assert!(!outside_canon.starts_with(safe.root()));
        }

        let portable = SafeDir::from_mode(StorageMode::Portable {
            dir: base.join("portable_data"),
        })
        .expect("portable SafeDir");
        assert!(portable.root().exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn atomic_write_preserves_previous_contents_on_failure_and_debounced_saver_flushes() {
        let base = temp_test_dir("atomic");
        let safe = SafeDir::open(&base).expect("open SafeDir");

        safe.write_atomic("state.json", b"{\"v\":1}")
            .expect("initial write");
        assert_eq!(
            safe.read_to_string("state.json").expect("read"),
            "{\"v\":1}"
        );

        let fail_res = safe.write_atomic_internal(Path::new("state.json"), b"{\"v\":2}", true);
        assert!(fail_res.is_err());
        assert_eq!(
            safe.read_to_string("state.json").expect("preserved v1"),
            "{\"v\":1}"
        );

        let mut saver = DebouncedSaver::new(safe.clone(), "autosave.txt", 0.5);
        saver.stage(b"draft1".to_vec());
        assert!(saver.is_dirty());
        assert!(!saver.tick(0.25).expect("tick1"));
        assert!(saver.is_dirty());
        assert!(saver.tick(0.30).expect("tick2"));
        assert!(!saver.is_dirty());
        assert_eq!(
            safe.read_to_string("autosave.txt").expect("autosaved"),
            "draft1"
        );

        saver.stage(b"panic_saved".to_vec());
        assert!(saver.is_dirty());
        crate::sys::panic_flush::run_panic_flush_callbacks();
        assert!(!saver.is_dirty());
        assert_eq!(
            safe.read_to_string("autosave.txt")
                .expect("flushed on panic callback"),
            "panic_saved"
        );

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn wasm_base64_roundtrip_handles_arbitrary_bytes() {
        let samples: [&[u8]; 5] = [b"", b"a", b"ab", b"abc", &[0, 1, 2, 127, 128, 255]];
        for s in samples {
            let enc = super::encode_b64(s);
            let dec = super::decode_b64(&enc).expect("valid base64");
            assert_eq!(dec.as_slice(), s);
        }
        assert!(super::decode_b64("abc").is_err());
    }
}
