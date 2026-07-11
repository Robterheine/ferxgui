/// Atomic JSON persistence for all `~/.ferxgui/` data files.
///
/// Write strategy: serialize to a temp file in the same directory, then
/// rename over the target — avoids partial writes on crash.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::{ModelMeta, RunRecord};

// ---------------------------------------------------------------------------
// Directory resolution
// ---------------------------------------------------------------------------

/// Returns `~/.ferxgui/`, creating it if absent.
pub fn app_dir() -> Option<PathBuf> {
    let dir = dirs::home_dir()?.join(".ferxgui");

    #[cfg(unix)]
    {
        if let Ok(meta) = std::fs::symlink_metadata(&dir) {
            if meta.file_type().is_symlink() {
                return None; // refuse to use a symlinked app dir
            }
        }
    }

    std::fs::create_dir_all(&dir).ok()?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }

    Some(dir)
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Last-used working directory.
    pub working_directory: Option<PathBuf>,
    /// Path to the `Rscript` executable used to run the ferx R package.
    /// (ferx ships as an R extension, not a CLI binary, so this is the
    /// interpreter we spawn — not a ferx executable.)
    pub ferx_binary: Option<PathBuf>,
    /// Path to RStudio (optional; shows button in header when set).
    pub rstudio_path: Option<PathBuf>,
    pub theme: Theme,
    /// Whether the sidebar is collapsed to icon-only mode.
    #[serde(default)]
    pub sidebar_collapsed: bool,
    /// Active file-extension filter pills in the Files tab.
    #[serde(default)]
    pub file_extensions: Vec<String>,
    /// `true` when the user explicitly chose the binary path via the Browse button.
    /// `false` means the path was auto-detected and can be overwritten by the
    /// background R-package detection on the next launch.
    #[serde(default)]
    pub ferx_binary_custom: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            working_directory: None,
            ferx_binary: default_ferx_binary(),
            rstudio_path: None,
            theme: Theme::Dark,
            sidebar_collapsed: false,
            file_extensions: vec![
                "ferx".into(),
                "fitrx".into(),
                "csv".into(),
            ],
            ferx_binary_custom: false,
        }
    }
}

fn default_ferx_binary() -> Option<PathBuf> {
    // No synchronous default — the Rscript path is filled in by the background
    // `detect_ferx_from_r()` probe at startup.
    None
}

/// Detect whether ferx can be run via R.
///
/// ferx ships as an R extension package (`rextendr` — the Rust engine is
/// compiled into `ferx.so`/`.dll`), **not** a standalone CLI binary, so there
/// is nothing to "find on disk".  Instead we locate `Rscript` (working even
/// when the app was launched from Finder/Explorer with a bare PATH) and then
/// actually `library(ferx)` to confirm the *compiled* package loads — a mere
/// `requireNamespace` can pass while a broken shared library would fail at run
/// time.  On success we return the **Rscript path** (the executable ferxgui
/// spawns) and the ferx package version.
/// Returns `(rscript_path, ferx_version, r_version)` when ferx is loadable.
/// A single Rscript invocation returns both versions separated by `|`.
pub fn detect_ferx_from_r() -> Option<(PathBuf, String, String)> {
    let rscript = crate::io::r_extract::find_rscript()?;

    let probe = "tryCatch({\
        suppressMessages(library(ferx)); \
        cat(as.character(packageVersion('ferx')), '|', R.version.string, sep='')\
    }, error = function(e) cat(''))";

    let mut cmd = crate::io::r_extract::r_command(&rscript);
    cmd.args(["--vanilla", "-e", probe]);
    let output = cmd.output().ok()?;
    if !output.status.success() { return None; }

    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw.is_empty() { return None; }

    let mut parts = raw.splitn(2, '|');
    let ferx_ver = parts.next().unwrap_or("").trim().to_string();
    let r_ver    = parts.next().unwrap_or("").trim().to_string();
    if ferx_ver.is_empty() { return None; }

    Some((rscript, ferx_ver, r_ver))
}

/// Describes how the ferx binary was located.  Not persisted to disk;
/// recomputed each session by the background detection thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FerxBinarySource {
    /// Background detection has not yet finished.
    Detecting,
    /// Located via `Rscript / system.file('bin/ferx', package='ferx')`.
    RPackage,
    /// Found on the system `$PATH`.
    SystemPath,
    /// User explicitly chose a path with the Browse button.
    Custom,
    /// Not found by any method.
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

/// Returns `(Settings, Option<warning>)` — warning is `Some` when the file exists but is corrupt.
pub fn load_settings(app_dir: &Path) -> (Settings, Option<String>) {
    load_json_or_warn(app_dir.join("settings.json"))
}

pub fn save_settings(app_dir: &Path, s: &Settings) -> std::io::Result<()> {
    save_json(app_dir.join("settings.json"), s)
}

// ---------------------------------------------------------------------------
// Bookmarks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub label: String,
    pub path: PathBuf,
}

pub fn load_bookmarks(app_dir: &Path) -> Vec<Bookmark> {
    load_json(app_dir.join("bookmarks.json")).unwrap_or_default()
}

pub fn save_bookmarks(app_dir: &Path, bookmarks: &[Bookmark]) -> std::io::Result<()> {
    save_json(app_dir.join("bookmarks.json"), bookmarks)
}

// ---------------------------------------------------------------------------
// Model metadata
// ---------------------------------------------------------------------------

/// Per-user, per-workspace path for model annotations (starred, comment,
/// status, decision, tags, notes, lineage). Kept under the current user's
/// own `app_dir` — keyed by a hash of the workspace path — rather than in
/// the workspace directory itself, so multiple people pointed at the same
/// shared project each get their own annotations instead of overwriting
/// each other's.
fn model_meta_path(app_dir: &Path, workspace: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    workspace.hash(&mut h);
    app_dir.join("model_meta").join(format!("{:016x}.json", h.finish()))
}

/// Loads model annotations for `workspace`, scoped to the current user.
/// Keyed by model stem. On first use for a (user, workspace) pair that has
/// no per-user file yet, imports the legacy shared `model_meta.json` from
/// the workspace directory if one exists, so existing starred/tagged/
/// commented models don't appear to vanish on upgrade — the legacy file is
/// left in place untouched, not deleted, since other users on the same
/// workspace may not have upgraded yet.
pub fn load_model_meta(app_dir: &Path, workspace: &Path) -> HashMap<String, ModelMeta> {
    let path = model_meta_path(app_dir, workspace);
    if let Some(meta) = load_json::<HashMap<String, ModelMeta>>(path) {
        return meta;
    }
    load_json(workspace.join("model_meta.json")).unwrap_or_default()
}

pub fn save_model_meta(
    app_dir: &Path,
    workspace: &Path,
    meta: &HashMap<String, ModelMeta>,
) -> std::io::Result<()> {
    let path = model_meta_path(app_dir, workspace);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    save_json(path, meta)
}

// ---------------------------------------------------------------------------
// Run history
// ---------------------------------------------------------------------------

pub fn load_runs(app_dir: &Path) -> Vec<RunRecord> {
    load_json(app_dir.join("runs.json")).unwrap_or_default()
}

pub fn save_runs(app_dir: &Path, runs: &[RunRecord]) -> std::io::Result<()> {
    save_json(app_dir.join("runs.json"), runs)
}

// ---------------------------------------------------------------------------
// Atomic JSON helpers
// ---------------------------------------------------------------------------

/// Returns `(default, None)` when the file is absent, `(default, Some(warning))` when corrupt,
/// and `(value, None)` on success.
fn load_json_or_warn<T: for<'de> Deserialize<'de> + Default>(path: PathBuf) -> (T, Option<String>) {
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (T::default(), None),
        Err(_) => return (T::default(), None),
    };
    match serde_json::from_str::<T>(&text) {
        Ok(v) => (v, None),
        Err(e) => {
            let name = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("data file");
            // Back up the corrupt file so it can be inspected.
            let bak = path.with_extension("json.bak");
            let _ = std::fs::copy(&path, &bak);
            let warn = format!(
                "Warning: {name} was corrupted and could not be loaded (backed up as {}.bak). \
                 Defaults will be used. Error: {e}",
                path.file_stem().and_then(|s| s.to_str()).unwrap_or(name),
            );
            (T::default(), Some(warn))
        }
    }
}

fn load_json<T: for<'de> Deserialize<'de>>(path: PathBuf) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn save_json<T: Serialize + ?Sized>(path: PathBuf, value: &T) -> std::io::Result<()> {
    let dir = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "no parent directory")
    })?;
    let tmp = dir.join(format!(
        ".{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

#[cfg(test)]
mod model_meta_tests {
    use super::*;

    /// Scratch dirs unique across concurrent tests *and* across separate
    /// `cargo test` process invocations (PID + a per-process counter, not
    /// just the counter alone — a counter that resets to 0 on every fresh
    /// test-binary run can otherwise deterministically reuse the same path
    /// a previous run already left files under). Removed on drop so
    /// repeated local test runs don't accumulate junk in the OS temp dir.
    struct ScratchDirs {
        app_dir:   PathBuf,
        workspace: PathBuf,
    }

    impl Drop for ScratchDirs {
        fn drop(&mut self) {
            if let Some(base) = self.app_dir.parent() {
                let _ = std::fs::remove_dir_all(base);
            }
        }
    }

    fn scratch_dirs(tag: &str) -> ScratchDirs {
        use std::sync::atomic::{AtomicU32, Ordering};
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir()
            .join(format!("ferxgui_test_model_meta_{tag}_{}_{seq}", std::process::id()));
        let app_dir   = base.join("app_dir");
        let workspace = base.join("workspace");
        std::fs::create_dir_all(&app_dir).unwrap();
        std::fs::create_dir_all(&workspace).unwrap();
        ScratchDirs { app_dir, workspace }
    }

    #[test]
    fn save_then_load_round_trips_from_the_per_user_location() {
        let s = scratch_dirs("roundtrip");
        let mut meta = HashMap::new();
        meta.insert("model_a".to_string(), ModelMeta { starred: true, ..Default::default() });
        save_model_meta(&s.app_dir, &s.workspace, &meta).expect("save should succeed");

        // Written under the user's app_dir, not into the workspace directory.
        assert!(!s.workspace.join("model_meta.json").exists());

        let loaded = load_model_meta(&s.app_dir, &s.workspace);
        assert!(loaded.get("model_a").unwrap().starred);
    }

    #[test]
    fn first_load_imports_the_legacy_shared_file_once() {
        let s = scratch_dirs("legacy_import");

        // Simulate a pre-upgrade shared model_meta.json sitting in the
        // workspace directory (the old storage location).
        let mut legacy = HashMap::new();
        legacy.insert("legacy_model".to_string(), ModelMeta { comment: "from before".into(), ..Default::default() });
        save_json(s.workspace.join("model_meta.json"), &legacy).expect("legacy write should succeed");

        // No per-user file exists yet — load should import the legacy one.
        let loaded = load_model_meta(&s.app_dir, &s.workspace);
        assert_eq!(loaded.get("legacy_model").unwrap().comment, "from before");

        // The legacy file must be left untouched (not deleted/modified) —
        // other users on the same workspace may not have upgraded yet.
        assert!(s.workspace.join("model_meta.json").exists());

        // Once this user has their own saved data, subsequent loads must
        // read the per-user file, not keep re-importing the legacy one.
        let mut mine = HashMap::new();
        mine.insert("my_model".to_string(), ModelMeta { starred: true, ..Default::default() });
        save_model_meta(&s.app_dir, &s.workspace, &mine).expect("save should succeed");
        let reloaded = load_model_meta(&s.app_dir, &s.workspace);
        assert!(reloaded.contains_key("my_model"));
        assert!(!reloaded.contains_key("legacy_model"));
    }

    #[test]
    fn different_workspaces_for_the_same_user_get_independent_per_user_files() {
        let s_a = scratch_dirs("ws_a");
        let s_b = scratch_dirs("ws_b");

        let mut meta_a = HashMap::new();
        meta_a.insert("a".to_string(), ModelMeta::default());
        // Same app_dir (same user) as s_b, deliberately — this is testing
        // that two different *workspaces* under one user don't collide,
        // not that two different users don't collide (which is trivially
        // true and not what the hashing logic needs to get right).
        save_model_meta(&s_a.app_dir, &s_a.workspace, &meta_a).unwrap();

        let loaded_b = load_model_meta(&s_a.app_dir, &s_b.workspace);
        assert!(loaded_b.is_empty());
    }
}
