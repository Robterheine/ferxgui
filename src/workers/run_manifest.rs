/// Run manifest — written atomically to disk before a ferx process is
/// launched so that FerxGUI can reconnect to it after a restart.
///
/// Layout:  `{app_dir}/running/{run_id}.runmfst`
/// Strategy: write to `{path}.tmp`, then `fs::rename` (atomic on same FS).

use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Manifest struct
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunManifest {
    /// Schema version — bump when fields change.
    pub version: u32,
    /// Unique run identifier (stem + unix timestamp).
    pub run_id: String,
    pub model_stem: String,
    /// OS process ID of the detached ferx process.
    pub pid: u32,
    /// Absolute path to the log file (stdout + stderr combined).
    pub log_path: PathBuf,
    /// Full command string (for display).
    pub command: String,
    /// Working directory the process was launched from.
    pub directory: PathBuf,
}

impl RunManifest {
    pub const VERSION: u32 = 1;

    /// Write atomically.  If the write fails, no partial file is left.
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        let tmp = path.with_extension("tmp");
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)
    }

    /// Parse an existing manifest file, returning `None` on any error.
    pub fn load(path: &Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        let m: Self = serde_json::from_slice(&bytes).ok()?;
        if m.version != Self::VERSION { return None; }
        Some(m)
    }

    /// Delete the manifest; silently ignores "not found".
    pub fn remove(path: &Path) {
        let _ = std::fs::remove_file(path);
        // Also clean up any stale .tmp
        let _ = std::fs::remove_file(path.with_extension("tmp"));
    }

    /// True if a process with this PID appears to be running.
    ///
    /// Uses `kill(pid, 0)` on Unix (signal 0 = liveness probe).
    /// On Windows falls back to `tasklist`.
    pub fn is_pid_alive(pid: u32) -> bool {
        pid_alive(pid)
    }
}

// ---------------------------------------------------------------------------
// Directory helpers
// ---------------------------------------------------------------------------

/// Returns `{app_dir}/running/`, creating it if needed.
pub fn running_dir(app_dir: &Path) -> Option<PathBuf> {
    let dir = app_dir.join("running");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Returns the manifest path for a given run ID.
pub fn manifest_path(app_dir: &Path, run_id: &str) -> Option<PathBuf> {
    Some(running_dir(app_dir)?.join(format!("{run_id}.runmfst")))
}

/// Scan `{app_dir}/running/` and return all successfully parsed manifests
/// together with their on-disk paths.
pub fn scan_manifests(app_dir: &Path) -> Vec<(PathBuf, RunManifest)> {
    let dir = match running_dir(app_dir) {
        Some(d) => d,
        None => return vec![],
    };
    let rd = match std::fs::read_dir(&dir) {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    let mut out = vec![];
    for entry in rd.flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("runmfst") {
            continue;
        }
        if let Some(m) = RunManifest::load(&p) {
            out.push((p, m));
        } else {
            // Corrupt or stale .tmp — remove.
            let _ = std::fs::remove_file(&p);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Platform-specific PID liveness check
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
fn pid_alive(pid: u32) -> bool {
    // Read /proc/{pid}/status to distinguish running from zombie.
    // kill(pid,0)==0 succeeds for zombies too, so we check the State field.
    if let Ok(text) = std::fs::read_to_string(format!("/proc/{pid}/status")) {
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("State:") {
                // 'Z' = zombie (exited but not reaped) — treat as dead.
                return !rest.trim_start().starts_with('Z');
            }
        }
        return true; // status file present but no State line → alive
    }
    false // /proc/{pid}/status missing → process is gone
}

#[cfg(all(unix, not(target_os = "linux")))]
fn pid_alive(pid: u32) -> bool {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
        fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
    }
    const WNOHANG: i32 = 1;
    unsafe {
        if kill(pid as i32, 0) != 0 {
            return false; // ESRCH or EPERM — gone or inaccessible
        }
        // kill(0) succeeded: process exists but may be a zombie.
        // Non-blocking waitpid to reap zombies when we're the parent.
        let mut status: i32 = 0;
        let r = waitpid(pid as i32, &mut status, WNOHANG);
        if r == pid as i32 {
            return false; // We reaped it — was a zombie, now gone.
        }
        // r == 0: still running.  r == -1 (ECHILD): not our child but kill(0)
        // succeeded, so it's alive (possibly a reused PID from another process).
        true
    }
}

#[cfg(windows)]
fn pid_alive(pid: u32) -> bool {
    // Query tasklist for the exact PID.  Use CSV output and exact token match
    // so PID 123 doesn't spuriously match 1234, and suppress the console window.
    let mut cmd = std::process::Command::new("tasklist");
    cmd.args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"]);
    let mut cmd = crate::io::r_extract::apply_no_window(cmd);
    cmd.output()
        .map(|o| {
            let s = String::from_utf8_lossy(&o.stdout);
            let target = pid.to_string();
            // CSV fields are quoted & comma-separated, e.g. "Rscript.exe","123",...
            s.split(|c: char| c == ',' || c == '"' || c.is_whitespace())
                .any(|tok| tok == target)
        })
        .unwrap_or(false)
}

#[cfg(not(any(unix, windows)))]
fn pid_alive(_pid: u32) -> bool { false }
