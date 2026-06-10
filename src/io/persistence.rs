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
    std::fs::create_dir_all(&dir).ok()?;
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

/// Loads the `model_meta.json` for a given workspace directory.
/// Keyed by model stem.
pub fn load_model_meta(workspace: &Path) -> HashMap<String, ModelMeta> {
    let path = workspace.join("model_meta.json");
    load_json(path).unwrap_or_default()
}

pub fn save_model_meta(
    workspace: &Path,
    meta: &HashMap<String, ModelMeta>,
) -> std::io::Result<()> {
    save_json(workspace.join("model_meta.json"), meta)
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
