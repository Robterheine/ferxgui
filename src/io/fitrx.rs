/// Reader for `.fitrx` zip bundles produced by ferx-core.
///
/// Bundle layout (deflate-compressed zip):
///   manifest.json   — format version, ferx version, timestamp
///   fit.json        — all scalar/vector/matrix results
///   ebes.csv        — per-subject EBEs  (ID, eta_*, ofv_contribution, n_obs)
///   predictions.csv — per-observation  (TIME, DV, PRED, IPRED, CWRES, IWRES, ETA_*)
///   model.ferx      — verbatim model source
///   warnings.txt    — one warning per line
///   data.csv        — optionally embedded input dataset
///
/// `trace_path` in `fit.json` points to the convergence trace CSV, which lives
/// **outside** the zip in the same directory as the `.fitrx` file.
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::domain::{EvalData, FitSummary, PredRow, TraceRow};

// ---------------------------------------------------------------------------
// Wire types — match the JSON keys written by ferx-core io/fitrx.rs
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Wire types — match the actual fit.json schema produced by ferx 0.1.5.
//
// ferx nests parameters inside objects:
//   theta  = { estimates: [...], names: [...], se: [...], fixed: [...], ... }
//   omega  = { matrix: { data: [...], cols: N }, names: [...], se: [...],
//              shrinkage: [...], ... }
//   sigma  = { estimates: f64|[...], names: str|[...], se: f64|[...], ... }
// Scalar-or-array fields (sigma, shrinkage_eps) are kept as serde_json::Value
// so the deserialiser never rejects them on a type mismatch.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
struct ThetaWire {
    #[serde(default)] estimates: Vec<f64>,
    #[serde(default)] names:     Vec<String>,
    #[serde(default)] se:        Vec<f64>,
    #[serde(default)] #[allow(dead_code)] fixed: Vec<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct OmegaMatrixWire {
    #[serde(default)] data: Vec<f64>,
    #[serde(default)] cols: usize,
}

#[derive(Debug, Deserialize, Default)]
struct OmegaWire {
    #[serde(default)] matrix:    OmegaMatrixWire,
    #[serde(default)] names:     Vec<String>,
    #[serde(default)] se:        Vec<f64>,
    #[serde(default)] shrinkage: Vec<f64>,
}

/// The `iov` sub-object inside `fit.json` — present only for IOV models.
#[derive(Debug, Deserialize, Default)]
struct IovMatrixWire {
    // `rows` == `cols` for a square matrix; only `cols` is used to derive n_kappa.
    #[allow(dead_code)]
    #[serde(default)] rows: usize,
    #[serde(default)] cols: usize,
    #[serde(default)] data: Vec<f64>,
}

#[derive(Debug, Deserialize, Default)]
struct IovWire {
    #[serde(default)] kappa_names:     Vec<String>,
    #[serde(default)] se_kappa:        Vec<f64>,
    #[serde(default)] shrinkage_kappa: Vec<f64>,
    #[serde(default)] omega_iov:       IovMatrixWire,
}

/// Top-level `fit.json` deserialiser.
/// Every field is `#[serde(default)]` so unknown / missing keys are ignored
/// and a partial bundle never fails the whole parse.
#[derive(Debug, Deserialize, Default)]
struct FitWire {
    #[serde(default)] method:       String,
    // method_chain can be a plain string or an array — keep as Value.
    #[serde(default)] method_chain: serde_json::Value,
    #[serde(default)] converged:    bool,
    #[serde(default)] ofv:          f64,
    #[serde(default)] aic:          f64,
    #[serde(default)] bic:          f64,
    #[serde(default)] n_obs:        usize,
    #[serde(default)] n_subjects:   usize,
    #[serde(default)] n_parameters: usize,
    #[serde(default)] n_iterations: usize,
    #[serde(default)] wall_time_secs: f64,

    // Nested parameter objects.
    #[serde(default)] theta: ThetaWire,
    #[serde(default)] omega: OmegaWire,
    // sigma.estimates / .names / .se can be scalar or array.
    #[serde(default)] sigma: serde_json::Value,

    // Covariance — ferx uses a status string, not a bool.
    // cov_condition_number is null when not computed, so Option<f64>.
    #[serde(default)] covariance_status:   String,
    #[serde(default)] cov_condition_number: Option<f64>,

    // Shrinkage — eps can be a scalar when there is one sigma.
    #[serde(default)] shrinkage_eps: serde_json::Value,

    // Full covariance matrix of estimated parameters (rows × cols, row-major).
    // Used to derive the condition number when cov_condition_number is null
    // (ferx-r bug: the R bridge renames the field before persist.R reads it).
    #[serde(default)] covariance_matrix: Option<IovMatrixWire>,

    // IOV block — present only for models with kappa parameters.
    #[serde(default)] iov: Option<IovWire>,

    // Diagnostics (ferx >= 0.1.5).
    #[serde(default)] dw_statistic: Option<f64>,
    #[serde(default)] iwres_lag1_r: Option<f64>,

    // eta_param_info: array of {name, param_type, ...} (ferx >= 0.1.5).
    #[serde(default)] eta_param_info: serde_json::Value,

    #[serde(default)] warnings: Vec<String>,
    #[serde(default)] warnings_structured: Vec<crate::domain::StructuredWarning>,
    #[serde(default)] trace_path: Option<String>,
}


// ---------------------------------------------------------------------------
// Safety helpers
// ---------------------------------------------------------------------------

/// Maximum bytes read from a single ZIP entry into memory.
const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024; // 256 MB

/// Validate a ZIP entry name before using it as a filesystem path component.
///
/// Rejects names that contain `..` segments or are absolute paths — both could
/// allow path traversal outside the intended directory.
fn safe_entry_name(name: &str) -> Option<&str> {
    if name.contains("..") { return None; }
    if name.starts_with('/') || name.starts_with('\\') { return None; }
    // Windows: reject names starting with a drive letter (e.g. "C:")
    if name.len() >= 2 && name.as_bytes()[1] == b':' { return None; }
    Some(name)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum FitrxError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("JSON error in {entry}: {source}")]
    Json {
        entry: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("missing required entry: {0}")]
    MissingEntry(String),
}

/// Reads the `FitSummary` from a `.fitrx` bundle at `path`.
pub fn read_fit_summary(path: &Path) -> Result<FitSummary, FitrxError> {
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file)?;
    let wire = read_fit_json(&mut zip)?;
    let warnings = read_warnings(&mut zip).unwrap_or_default();
    Ok(wire_to_summary(wire, warnings))
}

/// Reads the raw model source stored inside the bundle.
#[allow(dead_code)]
pub fn read_model_source(path: &Path) -> Result<String, FitrxError> {
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file)?;
    read_text_entry(&mut zip, "model.ferx")
}

/// Returns the path to the convergence trace CSV, resolved relative to the
/// `.fitrx` file's parent directory when `trace_path` is a relative path.
pub fn resolve_trace_path(fitrx_path: &Path, fit: &FitSummary) -> Option<PathBuf> {
    let raw = fit.trace_path.as_deref()?;
    let p = PathBuf::from(raw);
    if p.is_absolute() {
        Some(p)
    } else {
        fitrx_path.parent().map(|parent| parent.join(&p))
    }
}

/// Read `predictions.csv` from a `.fitrx` bundle, returning an `EvalData`.
/// Returns `Ok(None)` when the entry is absent (older bundles).
pub fn read_predictions(fitrx_path: &Path) -> Result<Option<EvalData>, FitrxError> {
    let file = std::fs::File::open(fitrx_path)?;
    let mut zip = zip::ZipArchive::new(file)?;

    let entry = match zip.by_name("predictions.csv") {
        Ok(e) => e,
        Err(_) => return Ok(None), // not present in this bundle
    };

    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(entry);

    let headers = rdr.headers()
        .map_err(|e| FitrxError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?
        .clone();

    let col = |name: &str| -> Option<usize> {
        headers.iter().position(|h| h.eq_ignore_ascii_case(name))
    };
    let parse = |rec: &csv::StringRecord, c: usize| -> f64 {
        rec.get(c).and_then(|s| s.trim().parse().ok()).unwrap_or(f64::NAN)
    };

    let col_id     = col("ID");
    let col_time   = col("TIME").unwrap_or(usize::MAX);
    let col_dv     = col("DV").unwrap_or(usize::MAX);
    let col_pred   = col("PRED").unwrap_or(usize::MAX);
    let col_ipred  = col("IPRED").unwrap_or(usize::MAX);
    let col_cwres  = col("CWRES");
    let col_iwres  = col("IWRES");
    let col_ebeofv = col("EBE_OFV");

    let mut rows = Vec::new();
    for result in rdr.records() {
        let rec = result.map_err(|e| FitrxError::Io(
            std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        rows.push(PredRow {
            id:      col_id.and_then(|c| rec.get(c)).unwrap_or("").to_string(),
            time:    parse(&rec, col_time),
            dv:      parse(&rec, col_dv),
            pred:    parse(&rec, col_pred),
            ipred:   parse(&rec, col_ipred),
            cwres:   col_cwres.map(|c| parse(&rec, c)).unwrap_or(f64::NAN),
            iwres:   col_iwres.map(|c| parse(&rec, c)).unwrap_or(f64::NAN),
            ebe_ofv: col_ebeofv.map(|c| parse(&rec, c)).unwrap_or(f64::NAN),
        });
    }

    Ok(Some(EvalData::from_rows(rows)))
}

/// Read `ebes.csv` from a `.fitrx` bundle — per-subject EBEs and iOFV.
/// Returns `Ok(None)` when the entry is absent.
pub fn read_ebes(fitrx_path: &Path) -> Result<Option<crate::domain::EbesData>, FitrxError> {
    let file = std::fs::File::open(fitrx_path)?;
    let mut zip = zip::ZipArchive::new(file)?;

    let entry = match zip.by_name("ebes.csv") {
        Ok(e)  => e,
        Err(_) => return Ok(None),
    };

    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(entry);

    let headers = rdr.headers()
        .map_err(|e| FitrxError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?
        .clone();

    let col = |name: &str| headers.iter().position(|h| h.eq_ignore_ascii_case(name));
    let parse = |rec: &csv::StringRecord, c: usize| -> f64 {
        rec.get(c).and_then(|s| s.trim().parse().ok()).unwrap_or(f64::NAN)
    };

    let col_id   = col("ID");
    let col_ofv  = col("ofv_contribution").or_else(|| col("OFV_CONTRIBUTION"));
    let col_nobs = col("n_obs").or_else(|| col("N_OBS"));

    // Collect ETA column indices (all columns that aren't ID/ofv/n_obs).
    let eta_names: Vec<String> = headers.iter().enumerate()
        .filter(|(_i, h)| {
            !h.eq_ignore_ascii_case("ID")
            && !h.eq_ignore_ascii_case("ofv_contribution")
            && !h.eq_ignore_ascii_case("OFV_CONTRIBUTION")
            && !h.eq_ignore_ascii_case("n_obs")
            && !h.eq_ignore_ascii_case("N_OBS")
        })
        .map(|(_, h)| h.to_owned())
        .collect();
    let eta_cols: Vec<usize> = eta_names.iter()
        .filter_map(|n| col(n))
        .collect();

    let mut rows = Vec::new();
    for result in rdr.records() {
        let rec = result.map_err(|e| FitrxError::Io(
            std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        rows.push(crate::domain::EbesRow {
            id:              col_id.and_then(|c| rec.get(c)).unwrap_or("").to_string(),
            ofv_contribution: col_ofv.map(|c| parse(&rec, c)).unwrap_or(f64::NAN),
            n_obs:           col_nobs.and_then(|c| rec.get(c))
                                     .and_then(|s| s.trim().parse().ok())
                                     .unwrap_or(0),
            etas:            eta_cols.iter().map(|&c| parse(&rec, c)).collect(),
        });
    }

    let total_ofv = rows.iter()
        .filter(|r| r.ofv_contribution.is_finite())
        .map(|r| r.ofv_contribution)
        .sum();

    Ok(Some(crate::domain::EbesData { rows, total_ofv, eta_names }))
}

/// Extract per-observation and per-subject output tables from a `.fitrx` bundle,
/// writing them as standalone CSVs next to the bundle file.
///
/// Written files (when the entry exists inside the zip):
///   - `{stem}_sdtab.csv`  ← predictions.csv  (ID, TIME, DV, PRED, IPRED, CWRES, IWRES, …)
///   - `{stem}_patab.csv`  ← ebes.csv          (ID, ETA_*, ofv_contribution, n_obs)
///   - `{stem}_patab_kappa.csv` ← ebes_kappa.csv (IOV models only)
///
/// Returns the paths that were actually written.
pub fn extract_output_tables(fitrx_path: &Path) -> Result<Vec<PathBuf>, FitrxError> {
    let dir  = fitrx_path.parent().unwrap_or(std::path::Path::new("."));
    let stem = fitrx_path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("model");

    let file = std::fs::File::open(fitrx_path)?;
    let mut zip = zip::ZipArchive::new(file)?;

    let entries = [
        ("predictions.csv",  format!("{stem}_sdtab.csv")),
        ("ebes.csv",         format!("{stem}_patab.csv")),
        ("ebes_kappa.csv",   format!("{stem}_patab_kappa.csv")),
    ];

    let mut written = Vec::new();
    for (entry_name, out_name) in &entries {
        let mut entry = match zip.by_name(entry_name) {
            Ok(e)  => e,
            Err(_) => continue, // not in this bundle
        };
        if entry.size() > MAX_ENTRY_BYTES {
            return Err(FitrxError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{entry_name} exceeds {MAX_ENTRY_BYTES} bytes"),
            )));
        }
        let mut buf = String::new();
        entry.read_to_string(&mut buf)?;
        let out_path = dir.join(out_name);
        std::fs::write(&out_path, buf.as_bytes())
            .map_err(FitrxError::Io)?;
        written.push(out_path);
    }
    Ok(written)
}

/// Read a convergence trace CSV from disk (lives outside the .fitrx zip).
/// Parses all columns written by ferx-core: iter, method, phase, ofv,
/// grad_norm, mh_accept_rate, lm_lambda.  Unknown/missing columns are NaN.
pub fn read_trace_csv(path: &Path) -> std::io::Result<Vec<TraceRow>> {
    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_path(path)?;

    let headers = rdr.headers()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
        .clone();

    let col = |names: &[&str]| -> Option<usize> {
        names.iter().find_map(|n| {
            headers.iter().position(|h| h.eq_ignore_ascii_case(n))
        })
    };
    let col_iter   = col(&["iter", "ITER", "ITERATION", "STEP"]).unwrap_or(0);
    let col_ofv    = col(&["ofv",  "OFV",  "OBJV", "OBJECTIVE"]).unwrap_or(1);
    let col_method = col(&["method"]);
    let col_phase  = col(&["phase"]);
    let col_grad   = col(&["grad_norm"]);
    let col_mh     = col(&["mh_accept_rate"]);
    let col_lm     = col(&["lm_lambda"]);

    let parse = |rec: &csv::StringRecord, c: usize| -> f64 {
        rec.get(c).and_then(|s| s.trim().parse().ok()).unwrap_or(f64::NAN)
    };
    let parse_opt = |rec: &csv::StringRecord, c: Option<usize>| -> f64 {
        c.map(|i| parse(rec, i)).unwrap_or(f64::NAN)
    };
    let str_col = |rec: &csv::StringRecord, c: Option<usize>| -> String {
        c.and_then(|i| rec.get(i)).unwrap_or("").to_owned()
    };

    let mut rows = Vec::new();
    for result in rdr.records() {
        let rec = result.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        rows.push(TraceRow {
            iteration:      parse(&rec, col_iter),
            ofv:            parse(&rec, col_ofv),
            method:         str_col(&rec, col_method),
            phase:          str_col(&rec, col_phase),
            grad_norm:      parse_opt(&rec, col_grad),
            mh_accept_rate: parse_opt(&rec, col_mh),
            lm_lambda:      parse_opt(&rec, col_lm),
        });
    }
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn read_fit_json(zip: &mut zip::ZipArchive<std::fs::File>) -> Result<FitWire, FitrxError> {
    let mut entry = zip.by_name("fit.json").map_err(|_| {
        FitrxError::MissingEntry("fit.json".to_string())
    })?;
    if entry.size() > MAX_ENTRY_BYTES {
        return Err(FitrxError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "fit.json entry exceeds size limit",
        )));
    }
    let mut buf = String::new();
    entry.read_to_string(&mut buf)?;
    serde_json::from_str(&buf).map_err(|e| FitrxError::Json {
        entry: "fit.json".to_string(),
        source: e,
    })
}

fn read_warnings(zip: &mut zip::ZipArchive<std::fs::File>) -> Option<Vec<String>> {
    let mut entry = zip.by_name("warnings.txt").ok()?;
    if entry.size() > MAX_ENTRY_BYTES { return None; }
    let mut buf = String::new();
    entry.read_to_string(&mut buf).ok()?;
    Some(buf.lines().filter(|l| !l.is_empty()).map(str::to_owned).collect())
}

#[allow(dead_code)]
fn read_text_entry(
    zip: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> Result<String, FitrxError> {
    // Validate that the requested name is a safe path component.
    safe_entry_name(name).ok_or_else(|| FitrxError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!("unsafe ZIP entry name: {name}"),
    )))?;
    let mut entry = zip
        .by_name(name)
        .map_err(|_| FitrxError::MissingEntry(name.to_string()))?;
    if entry.size() > MAX_ENTRY_BYTES {
        return Err(FitrxError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{name} entry exceeds size limit"),
        )));
    }
    let mut buf = String::new();
    entry.read_to_string(&mut buf)?;
    Ok(buf)
}

/// Accept `method_chain` as either a JSON string or a JSON array of strings.
fn json_to_string_vec(v: &serde_json::Value) -> Vec<String> {
    match v {
        serde_json::Value::String(s) => vec![s.clone()],
        serde_json::Value::Array(arr) => arr.iter()
            .filter_map(|x| x.as_str().map(str::to_owned))
            .collect(),
        _ => vec![],
    }
}

/// Accept a JSON number or array of numbers and return `Vec<f64>`.
fn json_val_to_f64_vec(v: &serde_json::Value) -> Vec<f64> {
    match v {
        serde_json::Value::Number(n) => vec![n.as_f64().unwrap_or(f64::NAN)],
        serde_json::Value::Array(arr) => arr.iter()
            .map(|x| x.as_f64().unwrap_or(f64::NAN))
            .collect(),
        _ => vec![],
    }
}

/// Accept a JSON string or array of strings and return `Vec<String>`.
fn json_val_to_str_vec(v: &serde_json::Value) -> Vec<String> {
    match v {
        serde_json::Value::String(s) => vec![s.clone()],
        serde_json::Value::Array(arr) => arr.iter()
            .filter_map(|x| x.as_str().map(str::to_owned))
            .collect(),
        _ => vec![],
    }
}

/// Convert an N×N covariance matrix (row-major) to its correlation matrix.
/// Returns an empty Vec when the input is unusable.
fn build_correlation_matrix(data: &[f64], n: usize) -> Vec<f64> {
    if n == 0 || data.len() < n * n { return vec![]; }
    let std_devs: Vec<f64> = (0..n).map(|i| data[i * n + i].sqrt()).collect();
    if std_devs.iter().any(|&s| !s.is_finite() || s <= 0.0) { return vec![]; }
    (0..n * n).map(|k| {
        let i = k / n; let j = k % n;
        data[k] / (std_devs[i] * std_devs[j])
    }).collect()
}

/// Compute the condition number of a covariance matrix (largest / smallest
/// eigenvalue of its correlation matrix) using the Jacobi eigenvalue algorithm.
///
/// Returns `Some(cn)` on success, `None` when the data is unusable.
/// Works without any linear-algebra dependency; accurate for n ≤ ~20.
fn condition_number_from_covariance(data: &[f64], n: usize) -> Option<f64> {
    // Build the correlation matrix first; reuse the shared helper.
    let corr = build_correlation_matrix(data, n);
    if corr.is_empty() { return None; }
    let mut a = corr;

    // Jacobi eigenvalue algorithm for real symmetric matrices.
    // Sweeps until the largest off-diagonal element is < 1e-10.
    let max_sweeps = n * n * 20;
    for _ in 0..max_sweeps {
        // Find the largest off-diagonal element (upper triangle).
        let (mut p, mut q) = (0usize, 1usize);
        let mut max_off = 0.0_f64;
        for i in 0..n {
            for j in (i + 1)..n {
                let v = a[i * n + j].abs();
                if v > max_off { max_off = v; p = i; q = j; }
            }
        }
        if max_off < 1e-10 { break; }

        // Compute the Jacobi rotation angle.
        let theta = (a[q * n + q] - a[p * n + p]) / (2.0 * a[p * n + q]);
        let t = if theta >= 0.0 {
            1.0 / (theta + (1.0 + theta * theta).sqrt())
        } else {
            1.0 / (theta - (1.0 + theta * theta).sqrt())
        };
        let c = 1.0 / (1.0 + t * t).sqrt();
        let s = t * c;

        // Update diagonal and the (p, q) entry.
        let app = a[p * n + p];
        let aqq = a[q * n + q];
        let apq = a[p * n + q];
        a[p * n + p] = app - t * apq;
        a[q * n + q] = aqq + t * apq;
        a[p * n + q] = 0.0;
        a[q * n + p] = 0.0;

        // Update remaining rows / columns.
        for r in 0..n {
            if r != p && r != q {
                let arp = a[r * n + p];
                let arq = a[r * n + q];
                let new_rp = c * arp - s * arq;
                let new_rq = s * arp + c * arq;
                a[r * n + p] = new_rp; a[p * n + r] = new_rp;
                a[r * n + q] = new_rq; a[q * n + r] = new_rq;
            }
        }
    }

    // Eigenvalues are now on the diagonal.
    let mut eigs: Vec<f64> = (0..n).map(|i| a[i * n + i]).collect();
    eigs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let min_ev = eigs[0];
    let max_ev = eigs[n - 1];
    if min_ev > 1e-10 {
        Some(max_ev / min_ev)
    } else {
        Some(f64::INFINITY)
    }
}

/// Convert a full N×N row-major matrix to a flattened lower triangle
/// [v(0,0), v(1,0), v(1,1), v(2,0), v(2,1), v(2,2), ...].
fn full_matrix_to_lower_triangle(data: &[f64], n: usize) -> Vec<f64> {
    if n == 0 || data.len() < n * n { return vec![]; }
    let mut out = Vec::with_capacity(n * (n + 1) / 2);
    for row in 0..n {
        for col in 0..=row {
            out.push(data[row * n + col]);
        }
    }
    out
}

fn wire_to_summary(w: FitWire, mut warnings: Vec<String>) -> FitSummary {
    // Merge warnings from fit.json and warnings.txt (deduplicate).
    for warn in &w.warnings {
        if !warnings.contains(warn) {
            warnings.push(warn.clone());
        }
    }

    // method_chain: accept both a plain string and an array of strings.
    let method_chain = json_to_string_vec(&w.method_chain);

    // Omega: ferx stores a full N×N row-major matrix; FitSummary wants the
    // flattened lower triangle [v00, v10, v11, v20, v21, v22, ...].
    let n_eta = w.omega.matrix.cols;
    let omega = full_matrix_to_lower_triangle(&w.omega.matrix.data, n_eta);

    // Sigma: estimates / names / se can each be a scalar or an array.
    let sigma       = json_val_to_f64_vec(w.sigma.get("estimates").unwrap_or(&serde_json::Value::Null));
    let sigma_names = json_val_to_str_vec(w.sigma.get("names").unwrap_or(&serde_json::Value::Null));
    let se_sigma    = json_val_to_f64_vec(w.sigma.get("se").unwrap_or(&serde_json::Value::Null));

    // Eps shrinkage: scalar when there is one sigma component.
    let eps_shrinkage = json_val_to_f64_vec(&w.shrinkage_eps);

    // Parameter correlation matrix — derived from covariance_matrix when present.
    let (cov_corr_flat, cov_corr_n, cov_corr_names) =
        if let Some(ref cm) = w.covariance_matrix {
            let n = cm.cols;
            let corr = build_correlation_matrix(&cm.data, n);
            // Build canonical name list: theta (non-fixed) → omega diagonal → sigma.
            // Falls back to "P1…Pn" when count doesn't match n_parameters.
            let mut names: Vec<String> = w.theta.names.clone();
            names.extend_from_slice(&w.omega.names);
            names.extend(json_val_to_str_vec(
                w.sigma.get("names").unwrap_or(&serde_json::Value::Null)
            ));
            if names.len() != n {
                names = (1..=n).map(|i| format!("P{i}")).collect();
            }
            (corr, n, names)
        } else {
            (vec![], 0, vec![])
        };

    // IOV / kappa — present only when the model has kappa parameters.
    let (iov_kappa, iov_kappa_names, iov_n_kappa, iov_se_kappa, iov_shrinkage) =
        if let Some(iov) = w.iov {
            let n = iov.omega_iov.cols;
            let kappa = full_matrix_to_lower_triangle(&iov.omega_iov.data, n);
            (kappa, iov.kappa_names, n, iov.se_kappa, iov.shrinkage_kappa)
        } else {
            (vec![], vec![], 0, vec![], vec![])
        };

    // eta_param_info: array of objects with a "param_type" field.
    let eta_param_types: Vec<String> = if let serde_json::Value::Array(arr) = &w.eta_param_info {
        arr.iter()
            .filter_map(|el| el.get("param_type").and_then(|v| v.as_str()).map(str::to_owned))
            .collect()
    } else {
        vec![]
    };

    FitSummary {
        method: w.method,
        method_chain,
        converged: w.converged,
        ofv: w.ofv,
        aic: w.aic,
        bic: w.bic,
        n_obs: w.n_obs,
        n_subjects: w.n_subjects,
        n_parameters: w.n_parameters,
        n_iterations: w.n_iterations,
        wall_time_secs: w.wall_time_secs,
        theta:       w.theta.estimates,
        theta_names: w.theta.names,
        theta_lower: vec![], // not in fit.json; available from ModelEntry.model.params
        theta_upper: vec![], // same
        omega,
        omega_names: w.omega.names,
        n_eta,
        // IOV: the `iov` sub-object carries a full N×N row-major omega_iov matrix.
        kappa:           iov_kappa,
        kappa_names:     iov_kappa_names,
        n_kappa:         iov_n_kappa,
        se_kappa:        iov_se_kappa,
        kappa_shrinkage: iov_shrinkage,
        sigma,
        sigma_names,
        se_theta: w.theta.se,
        se_omega: w.omega.se,
        se_sigma,
        cov_corr_flat,
        cov_corr_n,
        cov_corr_names,
        cov_condition_number: w.cov_condition_number
            .or_else(|| w.covariance_matrix.as_ref().and_then(|m| {
                condition_number_from_covariance(&m.data, m.cols)
            }))
            .unwrap_or(f64::NAN),
        covariance_ok: w.covariance_status == "computed",
        eta_shrinkage: w.omega.shrinkage,
        eps_shrinkage,
        etabar:        vec![], // not in fit.json
        etabar_pvalue: vec![],
        at_lower_bound: vec![], // not in fit.json
        warnings,
        trace_path: w.trace_path,
        dw_statistic: w.dw_statistic,
        iwres_lag1_r: w.iwres_lag1_r,
        warnings_structured: w.warnings_structured,
        eta_param_types,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_defaults_produce_valid_summary() {
        let wire = FitWire {
            method: "focei".to_string(),
            converged: true,
            ofv: -123.4,
            aic: 10.0,
            bic: 12.0,
            n_obs: 100,
            n_subjects: 20,
            covariance_status: "computed".to_string(),
            ..Default::default()
        };
        let s = wire_to_summary(wire, vec![]);
        assert_eq!(s.method, "focei");
        assert!(s.converged);
        assert!((s.ofv + 123.4).abs() < 1e-9);
        assert!(s.covariance_ok); // "computed" → true
    }

    #[test]
    fn warfarin_fitrx_parses_correctly() {
        let path = std::path::Path::new(
            "/Users/robterheine/Downloads/ferx_inspect/ferx-r/ferx-r-main/\
             inst/examples/models/warfarin.fitrx"
        );
        if !path.exists() { return; } // skip on other machines
        let s = super::read_fit_summary(path).expect("parse should succeed");
        assert!(s.converged, "expected converged");
        assert!((s.ofv + 286.0).abs() < 1.0, "ofv={}", s.ofv);
        assert_eq!(s.theta_names, vec!["TVCL", "TVV", "TVKA"]);
        assert_eq!(s.n_eta, 3);
        assert!(s.covariance_ok, "covariance should be ok");
        assert!(s.dw_statistic.is_some());
        // Condition number computed from covariance_matrix (cov_condition_number
        // is null in fit.json due to a ferx-r naming bug — we derive it ourselves).
        assert!(s.cov_condition_number.is_finite(), "CN should be finite");
        assert!(s.cov_condition_number > 1.0,       "CN must be ≥ 1");
        assert!(s.cov_condition_number < 1000.0,    "warfarin CN should be well-conditioned");
    }

    #[test]
    fn full_matrix_to_lower_triangle_3x3() {
        // Diagonal 3×3: only diagonal entries should survive in the lower triangle.
        let data = vec![1.0, 0.0, 0.0,
                        0.0, 2.0, 0.0,
                        0.0, 0.0, 3.0];
        let lt = full_matrix_to_lower_triangle(&data, 3);
        assert_eq!(lt, vec![1.0, 0.0, 2.0, 0.0, 0.0, 3.0]);
    }

    #[test]
    fn omega_index_round_trip() {
        // 3x3 lower triangle: [v00, v10, v11, v20, v21, v22]
        let mut fit = FitSummary::default();
        fit.omega = vec![1.0, 0.3, 0.5, 0.1, 0.2, 0.8];
        fit.n_eta = 3;
        assert_eq!(fit.omega_value(0, 0), Some(1.0));
        assert_eq!(fit.omega_value(1, 0), Some(0.3));
        assert_eq!(fit.omega_value(2, 1), Some(0.2));
        // Symmetric access
        assert_eq!(fit.omega_value(0, 1), Some(0.3));
    }
}
