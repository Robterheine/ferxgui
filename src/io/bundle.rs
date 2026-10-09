//! Inspecting a `.fitrx` bundle for the Files tab: what is inside it, whether the model and
//! dataset it records still match the files on disk, and previews of its entries.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::domain::FitSummary;

/// Most bytes read from one entry for inspection.
const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;
/// Most rows / lines shown in an entry preview.
pub const PREVIEW_ROWS: usize = 200;
/// Largest data file hashed on request.
const MAX_HASH_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct BundleEntry {
    pub name: String,
    pub size: u64,
    pub compressed: u64,
}

/// Outcome of comparing something the bundle recorded with a file on disk.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum FileCheck {
    /// Not checked yet (data hashing is on request).
    #[default]
    NotChecked,
    /// The bundle recorded nothing to compare against.
    NoRecord,
    /// No file found at the recorded or neighbouring location.
    Missing(String),
    /// Same content; the path is where it was found.
    Match(String),
    /// The file exists but its content differs from what the fit used.
    Differs(String),
}

#[derive(Debug, Clone)]
pub struct BundleInfo {
    pub path: PathBuf,
    pub file_size: u64,
    pub entries: Vec<BundleEntry>,
    pub manifest: Value,
    pub fit: Value,
    pub model_source: Option<String>,
    pub warnings: Vec<String>,
    pub summary: Option<FitSummary>,
    pub model_check: FileCheck,
}

/// A preview of one bundle entry.
#[derive(Debug, Clone)]
pub enum EntryPreview {
    Table { headers: Vec<String>, rows: Vec<Vec<String>>, total_rows: usize },
    Text { text: String, truncated: bool },
}

impl BundleInfo {
    pub fn str_field(&self, key: &str) -> Option<&str> {
        self.fit.get(key).and_then(Value::as_str)
    }

    pub fn recorded_model_hash(&self) -> Option<&str> {
        self.str_field("model_hash")
    }

    pub fn recorded_data_hash(&self) -> Option<&str> {
        self.str_field("data_hash")
    }

    pub fn recorded_data_path(&self) -> Option<&str> {
        self.str_field("data_path").filter(|s| !s.is_empty())
    }
}

fn read_entry_bytes(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Result<Vec<u8>, String> {
    let entry = zip.by_name(name).map_err(|e| format!("{name}: {e}"))?;
    if entry.size() > MAX_ENTRY_BYTES {
        return Err(format!("{name} is larger than {} MB", MAX_ENTRY_BYTES / (1024 * 1024)));
    }
    let mut buf = Vec::with_capacity(entry.size() as usize);
    entry.take(MAX_ENTRY_BYTES).read_to_end(&mut buf).map_err(|e| format!("{name}: {e}"))?;
    Ok(buf)
}

/// Opens a bundle and gathers everything the viewer shows. Fails only when the file is not a
/// readable zip; a missing or malformed `fit.json` still yields an inspectable bundle.
pub fn inspect(path: &Path) -> Result<BundleInfo, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("not a readable zip archive: {e}"))?;

    let entries: Vec<BundleEntry> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|e| BundleEntry {
            name: e.name().to_string(), size: e.size(), compressed: e.compressed_size(),
        }))
        .filter(|e| !e.name.ends_with('/'))
        .collect();

    let json = |zip: &mut zip::ZipArchive<std::fs::File>, name: &str| -> Value {
        read_entry_bytes(zip, name).ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(Value::Null)
    };
    let manifest = json(&mut zip, "manifest.json");
    let fit = json(&mut zip, "fit.json");
    let model_source = read_entry_bytes(&mut zip, "model.ferx").ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned());
    let warnings = read_entry_bytes(&mut zip, "warnings.txt").ok()
        .map(|b| String::from_utf8_lossy(&b).lines()
            .map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
        .unwrap_or_default();
    let summary = crate::io::fitrx::read_fit_summary(path).ok();

    let mut info = BundleInfo {
        path: path.to_path_buf(), file_size, entries, manifest, fit, model_source, warnings,
        summary, model_check: FileCheck::NotChecked,
    };
    info.model_check = check_model(&info);
    Ok(info)
}

/// Compares the bundled `model.ferx` with the model file on disk (the recorded `model_path`,
/// or `<stem>.ferx` next to the bundle).
pub fn check_model(info: &BundleInfo) -> FileCheck {
    let Some(bundled) = info.model_source.as_deref() else { return FileCheck::NoRecord };
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = info.str_field("model_path").filter(|p| !p.is_empty()) {
        candidates.push(PathBuf::from(p));
    }
    candidates.push(info.path.with_extension("ferx"));
    let Some(found) = candidates.iter().find(|p| p.is_file()) else {
        return FileCheck::Missing(candidates.first().map(|p| p.display().to_string()).unwrap_or_default());
    };
    let norm = |s: &str| s.replace("\r\n", "\n");
    match std::fs::read_to_string(found) {
        Ok(disk) if norm(&disk) == norm(bundled) => FileCheck::Match(found.display().to_string()),
        Ok(_) => FileCheck::Differs(found.display().to_string()),
        Err(_) => FileCheck::Missing(found.display().to_string()),
    }
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    if f.metadata()?.len() > MAX_HASH_BYTES {
        return Err(std::io::Error::other("file is too large to hash"));
    }
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Hashes the dataset the fit used and compares it with the recorded `data_hash`. The file is
/// looked for at the recorded path, then under the same name next to the bundle.
pub fn verify_data(info: &BundleInfo) -> FileCheck {
    let (Some(recorded), Some(want)) = (info.recorded_data_path(), info.recorded_data_hash()) else {
        return FileCheck::NoRecord;
    };
    let rec = PathBuf::from(recorded);
    let mut candidates = vec![rec.clone()];
    if let (Some(dir), Some(name)) = (info.path.parent(), rec.file_name()) {
        candidates.push(dir.join(name));
    }
    let Some(found) = candidates.iter().find(|p| p.is_file()) else {
        return FileCheck::Missing(recorded.to_string());
    };
    match sha256_file(found) {
        Ok(h) if h.eq_ignore_ascii_case(want) => FileCheck::Match(found.display().to_string()),
        Ok(_) => FileCheck::Differs(found.display().to_string()),
        Err(_) => FileCheck::Missing(found.display().to_string()),
    }
}

/// Reads the start of one entry: a table for CSV entries, text otherwise.
pub fn read_entry_preview(path: &Path, name: &str) -> Result<EntryPreview, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let bytes = read_entry_bytes(&mut zip, name)?;
    let text = String::from_utf8_lossy(&bytes);
    if name.to_ascii_lowercase().ends_with(".csv") {
        let mut rdr = csv::ReaderBuilder::new().flexible(true).from_reader(text.as_bytes());
        let headers: Vec<String> = rdr.headers().map_err(|e| e.to_string())?
            .iter().map(str::to_string).collect();
        let mut rows = Vec::new();
        let mut total = 0usize;
        for rec in rdr.records().flatten() {
            total += 1;
            if rows.len() < PREVIEW_ROWS {
                rows.push(rec.iter().map(str::to_string).collect());
            }
        }
        return Ok(EntryPreview::Table { headers, rows, total_rows: total });
    }
    // Pretty-print JSON entries so they are readable.
    let shown = if name.to_ascii_lowercase().ends_with(".json") {
        serde_json::from_slice::<Value>(&bytes).ok()
            .and_then(|v| serde_json::to_string_pretty(&v).ok())
            .unwrap_or_else(|| text.to_string())
    } else {
        text.to_string()
    };
    let lines: Vec<&str> = shown.lines().collect();
    let truncated = lines.len() > PREVIEW_ROWS * 5;
    Ok(EntryPreview::Text { text: lines.iter().take(PREVIEW_ROWS * 5).copied().collect::<Vec<_>>().join("\n"), truncated })
}

/// Writes one entry's bytes to `dest`.
pub fn extract_entry(path: &Path, name: &str, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let bytes = read_entry_bytes(&mut zip, name)?;
    std::fs::write(dest, bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const FIT: &str = r#"{"method":"foce","converged":true,"ofv":-10.5,
        "theta":{"names":["TVCL"],"estimates":[0.13],"se":[0.01]},
        "model_hash":"HASH_MODEL","data_hash":"HASH_DATA","data_path":"DATAPATH","model_path":"MODELPATH"}"#;

    fn make_bundle(dir: &Path, model: &str, fit: &str) -> PathBuf {
        let p = dir.join("m.fitrx");
        let f = std::fs::File::create(&p).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, body) in [
            ("manifest.json", r#"{"format_version":"1","ferx_version":"0.4.0","entries":[]}"#),
            ("fit.json", fit),
            ("model.ferx", model),
            ("warnings.txt", "first warning\n\nsecond warning\n"),
            ("predictions.csv", "ID,TIME,DV\n1,0.5,3.2\n1,1,4.1\n2,0.5,2.9\n"),
        ] {
            z.start_file(name, o).unwrap();
            z.write_all(body.as_bytes()).unwrap();
        }
        z.finish().unwrap();
        p
    }

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ferxgui_bundle_{tag}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn inspect_lists_entries_and_parses_the_parts() {
        let d = tmpdir("inspect");
        let p = make_bundle(&d, "MODEL TEXT", FIT);
        let i = inspect(&p).unwrap();
        assert_eq!(i.entries.len(), 5);
        assert!(i.entries.iter().any(|e| e.name == "predictions.csv" && e.size > 0));
        assert_eq!(i.manifest["ferx_version"], "0.4.0");
        assert_eq!(i.fit["method"], "foce");
        assert_eq!(i.model_source.as_deref(), Some("MODEL TEXT"));
        assert_eq!(i.warnings, vec!["first warning", "second warning"]);
        assert!(i.file_size > 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_bundle_without_a_readable_fit_json_is_still_inspectable() {
        let d = tmpdir("nofit");
        let p = make_bundle(&d, "M", "not json");
        let i = inspect(&p).unwrap();
        assert!(i.fit.is_null());
        assert_eq!(i.model_source.as_deref(), Some("M"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_non_zip_file_is_a_clear_error() {
        let d = tmpdir("notzip");
        let p = d.join("x.fitrx");
        std::fs::write(&p, "plain text").unwrap();
        assert!(inspect(&p).unwrap_err().contains("zip"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn model_check_compares_with_the_file_next_to_the_bundle() {
        let d = tmpdir("modelcheck");
        let p = make_bundle(&d, "line1\nline2\n", FIT);
        let sibling = d.join("m.ferx");
        assert!(matches!(inspect(&p).unwrap().model_check, FileCheck::Missing(_)));
        std::fs::write(&sibling, "line1\r\nline2\r\n").unwrap(); // CRLF is not a difference
        assert!(matches!(inspect(&p).unwrap().model_check, FileCheck::Match(_)));
        std::fs::write(&sibling, "edited\n").unwrap();
        assert!(matches!(inspect(&p).unwrap().model_check, FileCheck::Differs(_)));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn data_verification_uses_the_recorded_sha256() {
        let d = tmpdir("dataverify");
        let data = d.join("data.csv");
        std::fs::write(&data, "ID,TIME,DV\n1,0,1\n").unwrap();
        let hash = sha256_file(&data).unwrap();
        // Known SHA-256 of the empty input confirms the implementation.
        let empty = d.join("empty");
        std::fs::write(&empty, "").unwrap();
        assert_eq!(sha256_file(&empty).unwrap(),
                   "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");

        let fit = FIT.replace("HASH_DATA", &hash).replace("DATAPATH", &data.display().to_string());
        let p = make_bundle(&d, "M", &fit);
        let info = inspect(&p).unwrap();
        assert!(matches!(verify_data(&info), FileCheck::Match(_)));
        std::fs::write(&data, "ID,TIME,DV\n1,0,2\n").unwrap();
        assert!(matches!(verify_data(&info), FileCheck::Differs(_)));
        // Moved: found by file name next to the bundle.
        let moved = d.join("sub");
        std::fs::create_dir_all(&moved).unwrap();
        std::fs::rename(&data, d.join("data2.csv")).unwrap();
        std::fs::remove_dir_all(&moved).unwrap();
        assert!(matches!(verify_data(&info), FileCheck::Missing(_)));
        // No record at all.
        let bare = make_bundle(&d, "M", r#"{"method":"foce"}"#);
        assert_eq!(verify_data(&inspect(&bare).unwrap()), FileCheck::NoRecord);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn previews_tables_and_pretty_json_and_extracts_entries() {
        let d = tmpdir("preview");
        let p = make_bundle(&d, "M", FIT);
        match read_entry_preview(&p, "predictions.csv").unwrap() {
            EntryPreview::Table { headers, rows, total_rows } => {
                assert_eq!(headers, vec!["ID", "TIME", "DV"]);
                assert_eq!(rows.len(), 3);
                assert_eq!(total_rows, 3);
            }
            other => panic!("expected a table, got {other:?}"),
        }
        match read_entry_preview(&p, "fit.json").unwrap() {
            EntryPreview::Text { text, .. } => assert!(text.contains("\n  \"method\": \"foce\"")),
            other => panic!("expected text, got {other:?}"),
        }
        assert!(read_entry_preview(&p, "nope.csv").is_err());
        let out = d.join("out.ferx");
        extract_entry(&p, "model.ferx", &out).unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "M");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The real (committed) bundle's recorded hashes verify against its bundled files.
    #[test]
    fn real_bundle_hashes_verify() {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/warfarin_fix.fitrx");
        let info = inspect(&p).unwrap();
        assert!(info.entries.len() >= 6);
        assert!(info.summary.is_some());
        let bundled = info.model_source.clone().unwrap();
        assert_eq!(sha256_file_bytes(bundled.as_bytes()), info.recorded_model_hash().unwrap());
    }

    fn sha256_file_bytes(b: &[u8]) -> String {
        Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
    }
}
