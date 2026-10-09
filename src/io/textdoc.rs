//! Loading and saving user text files without silently changing them.
//!
//! The CSV editor used to drop unparsable records and write back a clean-looking file, which
//! turned an invalid dataset (ferx itself refuses non-UTF-8 bytes) into a valid one by deleting
//! an observation. Here a file that cannot be read exactly is refused with a position, rows the
//! user did not touch are written back byte for byte, and writes are atomic with a backup.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq)]
pub enum DocError {
    /// Invalid UTF-8 at this 1-based line and byte column.
    NotUtf8 { line: usize, col: usize },
    Parse { record: usize, msg: String },
    /// The header has no comma but does have this delimiter; ferx reads comma-separated only.
    Delimiter { found: char },
    Io(String),
}

impl std::fmt::Display for DocError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocError::NotUtf8 { line, col } => write!(
                f, "not valid UTF-8 at line {line}, byte {col}. ferx refuses such files too, so \
                    this file is not edited as-is. Use \"Convert to UTF-8\" (a backup is kept)."),
            DocError::Parse { record, msg } => write!(f, "record {record} cannot be parsed: {msg}"),
            DocError::Delimiter { found } => write!(
                f, "the header is {} separated; ferx reads comma-separated files only",
                match found { '\t' => "tab".to_string(), ';' => "semicolon".to_string(), c => format!("'{c}'") }),
            DocError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// A CSV file held in memory together with enough of its original bytes to write unchanged
/// rows back exactly.
#[derive(Debug, Clone)]
pub struct CsvDoc {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    raw: Vec<u8>,
    header_span: (usize, usize),
    spans: Vec<(usize, usize)>,
    orig_headers: Vec<String>,
    orig_rows: Vec<Vec<String>>,
    /// SHA-256 of the bytes as loaded, to detect an external change before saving.
    pub disk_hash: String,
    eol: &'static str,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// 1-based (line, byte column) of byte offset `at` in `raw`.
fn line_col(raw: &[u8], at: usize) -> (usize, usize) {
    let before = &raw[..at.min(raw.len())];
    let line = before.iter().filter(|&&b| b == b'\n').count() + 1;
    let col = before.iter().rev().take_while(|&&b| b != b'\n').count() + 1;
    (line, col)
}

pub fn load_csv(path: &Path) -> Result<CsvDoc, DocError> {
    let raw = std::fs::read(path).map_err(|e| DocError::Io(e.to_string()))?;
    parse_csv(raw)
}

pub fn parse_csv(raw: Vec<u8>) -> Result<CsvDoc, DocError> {
    if let Err(e) = std::str::from_utf8(&raw) {
        let (line, col) = line_col(&raw, e.valid_up_to());
        return Err(DocError::NotUtf8 { line, col });
    }
    let first_line_end = raw.iter().position(|&b| b == b'\n').unwrap_or(raw.len());
    let first_line = &raw[..first_line_end];
    if !first_line.contains(&b',') {
        if let Some(&d) = first_line.iter().find(|&&b| b == b'\t' || b == b';') {
            return Err(DocError::Delimiter { found: d as char });
        }
    }
    let eol = if raw.windows(2).any(|w| w == b"\r\n") { "\r\n" } else { "\n" };

    let mut rdr = csv::ReaderBuilder::new().has_headers(false).flexible(true).from_reader(&raw[..]);
    let mut starts: Vec<usize> = Vec::new();
    let mut all: Vec<Vec<String>> = Vec::new();
    let mut rec = csv::StringRecord::new();
    loop {
        match rdr.read_record(&mut rec) {
            Ok(true) => {
                let mut at = rec.position().map(|p| p.byte() as usize).unwrap_or(0);
                // The reader treats a lone `\r` as a terminator and can report a start between
                // the `\r` and `\n` of a CRLF pair; step over the `\n` so spans stay whole lines.
                if at > 0 && raw.get(at) == Some(&b'\n') && raw.get(at - 1) == Some(&b'\r') { at += 1; }
                starts.push(at);
                all.push(rec.iter().map(str::to_owned).collect());
            }
            Ok(false) => break,
            Err(e) => return Err(DocError::Parse { record: all.len() + 1, msg: e.to_string() }),
        }
    }
    if all.is_empty() { return Err(DocError::Parse { record: 0, msg: "the file is empty".into() }); }
    let mut spans = Vec::with_capacity(all.len());
    for i in 0..all.len() {
        let end = starts.get(i + 1).copied().unwrap_or(raw.len());
        spans.push((starts[i], end));
    }
    let header_span = spans.remove(0);
    let mut headers = all.remove(0);
    if let Some(h) = headers.first_mut() { *h = h.trim_start_matches('\u{feff}').to_string(); }
    let disk_hash = sha256_hex(&raw);
    Ok(CsvDoc {
        orig_headers: headers.clone(), orig_rows: all.clone(),
        headers, rows: all, raw, header_span, spans, disk_hash, eol,
    })
}

impl CsvDoc {
    /// The bytes to write: rows (and a header) the user did not change are copied from the
    /// original; changed rows are re-serialised. Fails if the row count changed.
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        if self.rows.len() != self.orig_rows.len() {
            return Err(format!("row count changed from {} to {}; refusing to save",
                self.orig_rows.len(), self.rows.len()));
        }
        let ser = |rec: &[String]| -> Result<Vec<u8>, String> {
            let mut w = csv::WriterBuilder::new().has_headers(false).flexible(true)
                .terminator(if self.eol == "\r\n" { csv::Terminator::CRLF } else { csv::Terminator::Any(b'\n') })
                .from_writer(Vec::new());
            w.write_record(rec).map_err(|e| e.to_string())?;
            w.into_inner().map_err(|e| e.to_string())
        };
        let mut out: Vec<u8> = Vec::with_capacity(self.raw.len());
        let ensure_eol = |out: &mut Vec<u8>| {
            if !out.is_empty() && !out.ends_with(b"\n") { out.extend_from_slice(self.eol.as_bytes()); }
        };
        out.extend_from_slice(&self.raw[..self.header_span.0]); // BOM, if any
        if self.headers == self.orig_headers {
            out.extend_from_slice(&self.raw[self.header_span.0..self.header_span.1]);
        } else {
            out.extend(ser(&self.headers)?);
        }
        for (i, row) in self.rows.iter().enumerate() {
            ensure_eol(&mut out);
            if *row == self.orig_rows[i] {
                out.extend_from_slice(&self.raw[self.spans[i].0..self.spans[i].1]);
            } else {
                out.extend(ser(row)?);
            }
        }
        Ok(out)
    }
}

// ── cp1252 → UTF-8 ───────────────────────────────────────────────────────────

/// Windows-1252 bytes 0x80..=0x9F; the undefined ones map to the matching C1 control.
const CP1252_HIGH: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}',
    '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
];

pub fn cp1252_to_utf8(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| match b {
        0x80..=0x9f => CP1252_HIGH[(b - 0x80) as usize],
        _ => b as char, // ASCII and 0xA0..=0xFF coincide with Latin-1 / Unicode
    }).collect()
}

/// Back up `path` (`.bak-<stamp>`, keeping the last five), returning the backup path.
pub fn backup_file(path: &Path) -> std::io::Result<PathBuf> {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis()).unwrap_or(0);
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let dir = path.parent().unwrap_or(Path::new("."));
    let bak = dir.join(format!("{name}.bak-{stamp}"));
    std::fs::copy(path, &bak)?;
    // Prune: keep the newest five.
    let prefix = format!("{name}.bak-");
    let mut baks: Vec<PathBuf> = std::fs::read_dir(dir)?.flatten().map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(&prefix)))
        .collect();
    baks.sort();
    while baks.len() > 5 { let _ = std::fs::remove_file(baks.remove(0)); }
    Ok(bak)
}

/// Explicit conversion of a cp1252 file to UTF-8: backup first, then atomic replace.
pub fn convert_cp1252_file(path: &Path) -> std::io::Result<PathBuf> {
    let bytes = std::fs::read(path)?;
    let bak = backup_file(path)?;
    super::fsutil::write_atomic(path, cp1252_to_utf8(&bytes).as_bytes())?;
    Ok(bak)
}

/// Atomic save with a backup of the file being replaced. Refuses (returns `Ok(false)`) when the
/// file on disk no longer hashes to `expected_disk_hash`, so an external change is never
/// overwritten unseen; the caller decides between overwrite, reload and save-as.
pub fn save_checked(path: &Path, bytes: &[u8], expected_disk_hash: Option<&str>) -> std::io::Result<bool> {
    if let (Some(want), Ok(now)) = (expected_disk_hash, std::fs::read(path)) {
        if sha256_hex(&now) != want { return Ok(false); }
    }
    if path.exists() { backup_file(path)?; }
    super::fsutil::write_atomic(path, bytes)?;
    Ok(true)
}

/// A path for a new export that never overwrites an existing file: `name.ext`, then
/// `name (2).ext`, `name (3).ext`, ...
pub fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() { return path.to_path_buf(); }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|e| e.to_str());
    let dir = path.parent().unwrap_or(Path::new("."));
    (2..).map(|n| dir.join(match ext {
        Some(e) => format!("{stem} ({n}).{e}"),
        None => format!("{stem} ({n})"),
    })).find(|p| !p.exists()).unwrap()
}

const EXPORT_MANIFEST: &str = ".ferxgui_created";

/// Where an export called `name` should be written inside `dir`: the plain name when it is free
/// or was created by this app earlier (re-export refreshes it), otherwise a `(2)` suffix. A file
/// the app did not create is never overwritten.
pub fn export_target(dir: &Path, name: &str) -> PathBuf {
    let ours = std::fs::read_to_string(dir.join(EXPORT_MANIFEST)).unwrap_or_default()
        .lines().any(|l| l == name);
    let p = dir.join(name);
    if ours || !p.exists() { p } else { unique_path(&p) }
}

/// Remember that the app created `path`, so a later export may refresh it.
pub fn record_export(path: &Path) {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else { return };
    let m = dir.join(EXPORT_MANIFEST);
    let mut cur = std::fs::read_to_string(&m).unwrap_or_default();
    if !cur.lines().any(|l| l == name) {
        cur.push_str(name); cur.push('\n');
        let _ = std::fs::write(&m, cur);
    }
}

fn draft_path(app_dir: &Path, original: &Path) -> PathBuf {
    let name = original.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let h = sha256_hex(original.to_string_lossy().as_bytes());
    app_dir.join("drafts").join(format!("{name}.{}.draft", &h[..8]))
}

/// Unsaved text is written here (in the app folder, not the project) so a crash loses little.
pub fn write_draft(app_dir: &Path, original: &Path, text: &str) {
    let p = draft_path(app_dir, original);
    if let Some(d) = p.parent() { let _ = std::fs::create_dir_all(d); }
    let _ = super::fsutil::write_atomic(&p, text.as_bytes());
}

pub fn remove_draft(app_dir: &Path, original: &Path) {
    let _ = std::fs::remove_file(draft_path(app_dir, original));
}

#[cfg(test)]
mod tests {
    #[test]
    fn export_target_respects_foreign_files() {
        let d = std::env::temp_dir().join(format!("ferxgui_exp_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("m_sdtab.csv"), b"user data").unwrap();
        let t = super::export_target(&d, "m_sdtab.csv");
        assert_eq!(t, d.join("m_sdtab (2).csv"));
        std::fs::write(&t, b"ours").unwrap();
        super::record_export(&t);
        // The suffixed name is ours; asking for it by its own name refreshes it.
        assert_eq!(super::export_target(&d, "m_sdtab (2).csv"), d.join("m_sdtab (2).csv"));
        assert_eq!(std::fs::read(d.join("m_sdtab.csv")).unwrap(), b"user data");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn draft_round_trip() {
        let d = std::env::temp_dir().join(format!("ferxgui_dr_{}", std::process::id()));
        let orig = std::path::Path::new("/proj/m.ferx");
        super::write_draft(&d, orig, "text");
        let n = std::fs::read_dir(d.join("drafts")).unwrap().count();
        assert_eq!(n, 1);
        super::remove_draft(&d, orig);
        assert_eq!(std::fs::read_dir(d.join("drafts")).unwrap().count(), 0);
        std::fs::remove_dir_all(&d).unwrap();
    }

    use super::*;

    fn tmpdir(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ferxgui_td_{n}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn csv_refuses_non_utf8_with_line() {
        let r = parse_csv(b"ID,TIME,DV\n1,0,1\n2,1,caf\xe9\n".to_vec());
        assert_eq!(r.unwrap_err(), DocError::NotUtf8 { line: 3, col: 8 });
    }

    #[test]
    fn untouched_rows_are_byte_identical_after_save() {
        let src = b"ID,TIME,DV\r\n1, 0 ,\"1,5\"\r\n2,1,2.0\r\n3,2,\r\n".to_vec();
        let mut doc = parse_csv(src.clone()).unwrap();
        assert_eq!(doc.to_bytes().unwrap(), src, "no edit -> identical bytes");
        doc.rows[1][2] = "9".into();
        let out = doc.to_bytes().unwrap();
        assert_eq!(out, b"ID,TIME,DV\r\n1, 0 ,\"1,5\"\r\n2,1,9\r\n3,2,\r\n".to_vec());
    }

    #[test]
    fn row_count_change_is_refused() {
        let mut doc = parse_csv(b"A,B\n1,2\n3,4\n".to_vec()).unwrap();
        doc.rows.pop();
        assert!(doc.to_bytes().is_err());
    }

    #[test]
    fn tab_file_names_the_delimiter() {
        let e = parse_csv(b"ID\tTIME\tDV\n1\t0\t1\n".to_vec()).unwrap_err();
        assert_eq!(e, DocError::Delimiter { found: '\t' });
        assert!(e.to_string().contains("tab"));
    }

    #[test]
    fn convert_cp1252_round_trip() {
        assert_eq!(cp1252_to_utf8(b"caf\xe9 \x80 \x93q\x94"), "café € “q”");
        let d = tmpdir("cp");
        let p = d.join("a.csv");
        std::fs::write(&p, b"ID,NOTE\n1,caf\xe9\n").unwrap();
        assert!(load_csv(&p).is_err());
        let bak = convert_cp1252_file(&p).unwrap();
        assert_eq!(std::fs::read(&bak).unwrap(), b"ID,NOTE\n1,caf\xe9\n");
        let doc = load_csv(&p).unwrap();
        assert_eq!(doc.rows[0][1], "café");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn csv_save_is_atomic_and_keeps_backup() {
        let d = tmpdir("save");
        let p = d.join("a.csv");
        std::fs::write(&p, b"A\n1\n").unwrap();
        assert!(save_checked(&p, b"A\n2\n", None).unwrap());
        assert_eq!(std::fs::read(&p).unwrap(), b"A\n2\n");
        let baks: Vec<_> = std::fs::read_dir(&d).unwrap().flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".bak-")).collect();
        assert_eq!(baks.len(), 1);
        assert!(!std::fs::read_dir(&d).unwrap().flatten().any(|e| e.file_name().to_string_lossy().contains(".tmp")));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn external_change_prompts_before_save() {
        let d = tmpdir("ext");
        let p = d.join("a.csv");
        std::fs::write(&p, b"A\n1\n").unwrap();
        let loaded = load_csv(&p).unwrap();
        std::fs::write(&p, b"A\n99\n").unwrap(); // someone else edits
        assert!(!save_checked(&p, b"A\n2\n", Some(&loaded.disk_hash)).unwrap());
        assert_eq!(std::fs::read(&p).unwrap(), b"A\n99\n", "must not have been overwritten");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn export_never_overwrites_user_file() {
        let d = tmpdir("uniq");
        let p = d.join("out.csv");
        assert_eq!(unique_path(&p), p);
        std::fs::write(&p, b"x").unwrap();
        assert_eq!(unique_path(&p), d.join("out (2).csv"));
        std::fs::write(d.join("out (2).csv"), b"x").unwrap();
        assert_eq!(unique_path(&p), d.join("out (3).csv"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn backups_are_pruned_to_five() {
        let d = tmpdir("prune");
        let p = d.join("a.csv");
        std::fs::write(&p, b"A\n").unwrap();
        for _ in 0..8 { backup_file(&p).unwrap(); std::thread::sleep(std::time::Duration::from_millis(3)); }
        let n = std::fs::read_dir(&d).unwrap().flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".bak-")).count();
        assert_eq!(n, 5);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
