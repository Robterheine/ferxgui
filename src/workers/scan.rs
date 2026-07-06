use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use crate::domain::{FerxModel, ModelEntry, ModelMeta};
use crate::io::{fitrx, ferx_file};
use super::messages::WorkerMsg;

/// Scans `directory` for `.ferx` files, reads paired `.fitrx` bundles, and
/// sends a `WorkerMsg::ScanComplete` with the full model list.
///
/// Runs on a background thread — never called from the egui update loop.
pub fn scan_directory(
    directory: PathBuf,
    meta_map: HashMap<String, ModelMeta>,
    tx: Sender<WorkerMsg>,
) {
    let entries = collect_models(&directory, &meta_map);
    let _ = tx.send(WorkerMsg::ScanComplete(entries));
}

fn collect_models(dir: &Path, meta_map: &HashMap<String, ModelMeta>) -> Vec<ModelEntry> {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return vec![],
    };

    let mut entries: Vec<ModelEntry> = read_dir
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "ferx")
                .unwrap_or(false)
        })
        .filter_map(|e| build_entry(e.path(), meta_map))
        .collect();

    // Sort alphabetically by stem.
    entries.sort_by(|a, b| a.model.stem.cmp(&b.model.stem));
    entries
}

fn build_entry(ferx_path: PathBuf, meta_map: &HashMap<String, ModelMeta>) -> Option<ModelEntry> {
    let stem = ferx_path.file_stem()?.to_string_lossy().to_string();
    let bytes = std::fs::read(&ferx_path).ok()?;
    let source = String::from_utf8_lossy(&bytes).into_owned();
    let params = ferx_file::parse_params(&source);

    // File creation time (falls back to modified time on Linux which lacks st_birthtime).
    let created_at = std::fs::metadata(&ferx_path)
        .ok()
        .and_then(|m| {
            m.created().or_else(|_| m.modified()).ok()
        })
        .and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH).ok()
        })
        .map(|d| crate::workers::run::unix_to_datetime(d.as_secs()));

    let model = FerxModel {
        path: ferx_path.clone(),
        stem: stem.clone(),
        source,
        params,
        created_at,
    };

    // Look for a paired .fitrx bundle in the same directory.
    let fitrx_path = ferx_path.with_extension("fitrx");
    let (fit, fit_parse_error, is_stale) = if fitrx_path.exists() {
        let (fit, err) = match fitrx::read_fit_summary(&fitrx_path) {
            Ok(f) => (Some(f), None),
            // A bundle exists but couldn't be parsed — keep the error instead
            // of silently discarding it, so the GUI can tell this apart from
            // "never run" (e.g. an incompatible ferx schema, not a missing run).
            Err(e) => (None, Some(e.to_string())),
        };
        let stale = is_stale(&ferx_path, &fitrx_path);
        (fit, err, stale)
    } else {
        (None, None, false)
    };

    let meta = meta_map.get(&stem).cloned().unwrap_or_default();

    Some(ModelEntry {
        model,
        fitrx_path: if fitrx_path.exists() { Some(fitrx_path) } else { None },
        fit,
        fit_parse_error,
        meta,
        is_stale,
    })
}

/// Returns true if the `.ferx` file is newer than the `.fitrx` bundle.
fn is_stale(ferx: &Path, fitrx: &Path) -> bool {
    let ferx_mt = std::fs::metadata(ferx).and_then(|m| m.modified()).ok();
    let fitrx_mt = std::fs::metadata(fitrx).and_then(|m| m.modified()).ok();
    match (ferx_mt, fitrx_mt) {
        (Some(fm), Some(rm)) => fm > rm,
        _ => false,
    }
}
