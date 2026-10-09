//! Filesystem helpers.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

type HashKey = (PathBuf, u64, Option<SystemTime>);

/// Files up to this size are always re-hashed: it costs milliseconds, and it keeps a same-size
/// edit with a restored mtime from returning a stale hash.
const MEMO_MIN_BYTES: u64 = 4 * 1024 * 1024;

/// SHA-256 (lowercase hex) of a file's bytes. Large files are memoised per (path, size, mtime)
/// so they are hashed once per change, not once per call; small files are always read.
/// None when the file cannot be read.
pub fn sha256_file_cached(path: &Path) -> Option<String> {
    static CACHE: OnceLock<Mutex<HashMap<HashKey, String>>> = OnceLock::new();
    let meta = std::fs::metadata(path).ok()?;
    let key = (path.to_path_buf(), meta.len(), meta.modified().ok());
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let memo = meta.len() >= MEMO_MIN_BYTES;
    if memo {
        if let Some(h) = cache.lock().ok()?.get(&key) { return Some(h.clone()); }
    }
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut f, &mut hasher).ok()?;
    let hex: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if memo {
        let mut g = cache.lock().ok()?;
        if g.len() > 256 { g.clear(); }
        g.insert(key, hex.clone());
    }
    Some(hex)
}

/// Write `bytes` to `path` atomically: write a temp file in the same
/// directory, `sync_all`, then rename over the target. A crash mid-write
/// leaves either the old file or the new one, never a truncated mix.
#[allow(dead_code)] // enabler for the data-integrity work package
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(format!(".{name}.tmp{}", std::process::id()));
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() { let _ = std::fs::remove_file(&tmp); }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_follows_content_not_mtime() {
        let dir = std::env::temp_dir().join(format!("ferxgui_hash_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("m.ferx");
        std::fs::write(&p, b"aaaa").unwrap();
        let t0 = std::fs::metadata(&p).unwrap().modified().unwrap();
        let h1 = sha256_file_cached(&p).unwrap();
        // Same size, different content, original mtime restored: must still be a new hash.
        std::fs::write(&p, b"bbbb").unwrap();
        std::fs::File::options().write(true).open(&p).unwrap().set_modified(t0).unwrap();
        let h2 = sha256_file_cached(&p).unwrap();
        assert_ne!(h1, h2);
        // Touch without content change: same hash.
        let t1 = t0 + std::time::Duration::from_secs(5);
        std::fs::File::options().write(true).open(&p).unwrap().set_modified(t1).unwrap();
        assert_eq!(sha256_file_cached(&p).unwrap(), h2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_atomic_replaces_and_leaves_no_temp() {
        let dir = std::env::temp_dir().join(format!("ferxgui_fsutil_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.csv");
        write_atomic(&p, b"one").unwrap();
        write_atomic(&p, b"two").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"two");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
