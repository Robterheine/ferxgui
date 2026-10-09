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
/// `trace_path` in `fit.json` points to the convergence trace CSV as it was
/// written during the run — an external temp file that usually doesn't
/// survive past it. ferx-r (>= 0.2.0) additionally bundles the same data as
/// `trace.csv` inside the zip; prefer that (`read_trace_csv_from_bundle`)
/// and fall back to the external path only for older bundles.
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

// estimates / names / se / fixed are declared as `Value`, not `Vec<T>`: R's
// jsonlite `auto_unbox = TRUE` collapses a length-1 vector to a bare scalar
// instead of a single-element array (e.g. a model with exactly one theta
// serializes `estimates` as `0.134`, not `[0.134]`), which a plain `Vec<T>`
// field rejects outright and fails the *entire* fit.json parse. Converted
// via `json_val_to_f64_vec` / `json_val_to_str_vec` in `wire_to_summary`.
#[derive(Debug, Deserialize, Default)]
struct ThetaWire {
    #[serde(default)] estimates: serde_json::Value,
    #[serde(default)] names:     serde_json::Value,
    #[serde(default)] se:        serde_json::Value,
    // Never read downstream (pre-existing); kept parse-safe for the same
    // single-theta collapse risk as the fields above.
    #[serde(default)] fixed: serde_json::Value,
}

#[derive(Debug, Deserialize, Default)]
struct OmegaMatrixWire {
    #[serde(default)] data: Vec<f64>,
    #[serde(default)] cols: usize,
}

#[derive(Debug, Deserialize, Default)]
struct OmegaWire {
    #[serde(default)] matrix:    OmegaMatrixWire,
    // Same single-element auto_unbox collapse risk as ThetaWire (a model
    // with exactly one ETA) — see the comment there.
    #[serde(default)] names:     serde_json::Value,
    #[serde(default)] se:        serde_json::Value,
    #[serde(default)] shrinkage: serde_json::Value,
    #[serde(default)] fixed:     serde_json::Value,
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
    // Same single-element auto_unbox collapse risk (a model with exactly
    // one kappa) — see the ThetaWire comment above.
    #[serde(default)] kappa_names:     serde_json::Value,
    #[serde(default)] se_kappa:        serde_json::Value,
    #[serde(default)] shrinkage_kappa: serde_json::Value,
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

    // A single warning collapses to a bare string under jsonlite auto_unbox —
    // same risk as the fields above. Converted via `json_val_to_str_vec`.
    #[serde(default)] warnings: serde_json::Value,
    #[serde(default)] warnings_structured: Vec<crate::domain::StructuredWarning>,
    #[serde(default)] trace_path: Option<String>,

    // ferx >= 0.4.0.
    #[serde(default)] ferx_version:      Option<String>,
    #[serde(default)] data_path:         Option<String>,
    #[serde(default)] omega_is_diagonal: Option<bool>,
    #[serde(default)] kappa_is_diagonal: Option<bool>,
    // Prior split — absent (or null) for models without a prior.
    #[serde(default)] ofv_data:          Option<f64>,
    #[serde(default)] ofv_prior:         Option<f64>,
    #[serde(default)] prior_summary:     serde_json::Value,
    // Fit identity: SHA-256 of the model / data file bytes.
    #[serde(default)] model_hash:        Option<String>,
    #[serde(default)] data_hash:         Option<String>,
    // ferx-r extras: boundary verdict, stall flag, max |correlation|.
    #[serde(default)] r_extras:          serde_json::Value,
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

/// Wrap a zip entry so it can never yield more than `MAX_ENTRY_BYTES` bytes,
/// regardless of what the entry's declared size (an attacker-controlled
/// field in the zip central directory) claims. Also fails fast with a clear
/// error when the declared size already exceeds the limit, so a hostile
/// bundle is rejected before any decompression happens rather than silently
/// truncated.
fn bound_entry<'a>(
    name: &str,
    entry: zip::read::ZipFile<'a>,
) -> Result<std::io::Take<zip::read::ZipFile<'a>>, FitrxError> {
    if entry.size() > MAX_ENTRY_BYTES {
        return Err(FitrxError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{name} entry exceeds {MAX_ENTRY_BYTES} byte limit"),
        )));
    }
    Ok(entry.take(MAX_ENTRY_BYTES))
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
    #[error("fit.json is missing the required key \"{0}\"")]
    MissingKey(String),
    #[error("missing required entry: {0}")]
    MissingEntry(String),
}

/// Where a run's `.fitrx` bundle is written: next to the model file, same stem
/// (`models/warfarin.ferx` -> `models/warfarin.fitrx`). Always absolute, so the path
/// handed to R never depends on the working directory, and it is exactly the file the
/// directory scanner pairs with the model.
pub fn bundle_path_for(model_path: &Path) -> PathBuf {
    let abs = std::path::absolute(model_path).unwrap_or_else(|_| model_path.to_path_buf());
    abs.with_extension("fitrx")
}

/// Reads the `FitSummary` from a `.fitrx` bundle at `path`.
pub fn read_fit_summary(path: &Path) -> Result<FitSummary, FitrxError> {
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file)?;
    let wire = read_fit_json(&mut zip)?;
    let warnings = read_warnings(&mut zip).unwrap_or_default();
    let mut summary = wire_to_summary(wire, warnings);
    // Declared bounds come from the model stored in the bundle (what the fit actually used).
    if let Ok(src) = read_text_entry(&mut zip, "model.ferx") {
        let pp = crate::io::ferx_file::parse_params(&src);
        if pp.theta_lower.len() == summary.theta.len() {
            summary.theta_lower = pp.theta_lower;
            summary.theta_upper = pp.theta_upper;
        }
    }
    // ETAbar is not in fit.json; compute it from the EBEs (mean and t-test p per ETA).
    if let Ok(Some(ebes)) = read_ebes(path) {
        for b in ebes.eta_bar() {
            summary.etabar.push(b.as_ref().map(|b| b.mean).unwrap_or(f64::NAN));
            summary.etabar_pvalue.push(b.as_ref().map(|b| b.p).unwrap_or(f64::NAN));
        }
    }
    Ok(summary)
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
    let entry = bound_entry("predictions.csv", entry)?;

    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(entry);

    let headers = rdr.headers()
        .map_err(|e| FitrxError::Io(std::io::Error::other(e)))?
        .clone();

    let col = |name: &str| -> Option<usize> {
        headers.iter().position(|h| h.eq_ignore_ascii_case(name))
    };
    let parse = |rec: &csv::StringRecord, c: usize| -> f64 {
        rec.get(c).and_then(|s| s.trim().parse().ok()).unwrap_or(f64::NAN)
    };

    let col_id     = col("ID");
    // These four drive every Evaluation plot; a missing one must not become a column of NaN.
    let need = |name: &str| col(name).ok_or_else(|| FitrxError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData, format!("predictions.csv has no {name} column"))));
    let col_time   = need("TIME")?;
    let col_dv     = need("DV")?;
    let col_pred   = need("PRED")?;
    let col_ipred  = need("IPRED")?;
    let col_cwres  = col("CWRES");
    let col_iwres  = col("IWRES");
    let col_ebeofv = col("EBE_OFV");
    let col_tad    = col("TAD");

    // Columns beyond the core set are kept as raw strings so the Evaluation tab can
    // filter / colour by them (OCC, N_OBS, TAFD, any column ferx adds later).
    const CORE: [&str; 9] = ["ID", "TIME", "DV", "PRED", "IPRED", "CWRES", "IWRES", "EBE_OFV", "TAD"];
    let extra_cols: Vec<(usize, String)> = headers.iter().enumerate()
        .filter(|(_, h)| !CORE.iter().any(|c| c.eq_ignore_ascii_case(h)))
        .map(|(i, h)| (i, h.to_string()))
        .collect();
    let mut extra_vals: Vec<Vec<String>> = vec![Vec::new(); extra_cols.len()];

    let mut rows = Vec::new();
    for result in rdr.records() {
        let rec = result.map_err(|e| FitrxError::Io(
            std::io::Error::other(e)))?;
        for (k, (c, _)) in extra_cols.iter().enumerate() {
            extra_vals[k].push(rec.get(*c).unwrap_or("").to_string());
        }
        rows.push(PredRow {
            id:      col_id.and_then(|c| rec.get(c)).unwrap_or("").to_string(),
            time:    parse(&rec, col_time),
            dv:      parse(&rec, col_dv),
            pred:    parse(&rec, col_pred),
            ipred:   parse(&rec, col_ipred),
            cwres:   col_cwres.map(|c| parse(&rec, c)).unwrap_or(f64::NAN),
            iwres:   col_iwres.map(|c| parse(&rec, c)).unwrap_or(f64::NAN),
            ebe_ofv: col_ebeofv.map(|c| parse(&rec, c)).unwrap_or(f64::NAN),
            tad:     col_tad.map(|c| parse(&rec, c)).unwrap_or(f64::NAN),
        });
    }

    let mut data = EvalData::from_rows(rows);
    data.extras = extra_cols.into_iter().map(|(_, h)| h).zip(extra_vals).collect();
    Ok(Some(data))
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
    let entry = bound_entry("ebes.csv", entry)?;

    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(entry);

    let headers = rdr.headers()
        .map_err(|e| FitrxError::Io(std::io::Error::other(e)))?
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
            std::io::Error::other(e)))?;
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

/// Read `covtab.csv` from a `.fitrx` bundle — the model's own declared
/// `[covariates]` block, one row per (ID, TIME). Returns `Ok(None)` when the
/// entry is absent (models with no declared covariates, or older bundles
/// predating this column).
pub fn read_covtab(fitrx_path: &Path) -> Result<Option<crate::domain::CovTabData>, FitrxError> {
    let file = std::fs::File::open(fitrx_path)?;
    let mut zip = zip::ZipArchive::new(file)?;

    let entry = match zip.by_name("covtab.csv") {
        Ok(e)  => e,
        Err(_) => return Ok(None),
    };
    let entry = bound_entry("covtab.csv", entry)?;

    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(entry);

    let headers = rdr.headers()
        .map_err(|e| FitrxError::Io(std::io::Error::other(e)))?
        .clone();

    let col = |name: &str| headers.iter().position(|h| h.eq_ignore_ascii_case(name));

    let col_id   = col("ID");
    let col_time = col("TIME");

    // Every column besides ID/TIME/EVID is a declared covariate.
    let covariate_names: Vec<String> = headers.iter()
        .filter(|h| {
            !h.eq_ignore_ascii_case("ID")
            && !h.eq_ignore_ascii_case("TIME")
            && !h.eq_ignore_ascii_case("EVID")
        })
        .map(|h| h.to_owned())
        .collect();
    let covariate_cols: Vec<(String, usize)> = covariate_names.iter()
        .filter_map(|n| col(n).map(|c| (n.clone(), c)))
        .collect();

    let mut rows = Vec::new();
    for result in rdr.records() {
        let rec = result.map_err(|e| FitrxError::Io(std::io::Error::other(e)))?;
        let values = covariate_cols.iter()
            .filter_map(|(name, c)| rec.get(*c)
                .and_then(|s| s.trim().parse::<f64>().ok())
                .map(|v| (name.clone(), v)))
            .collect();
        rows.push(crate::domain::CovTabRow {
            id:   col_id.and_then(|c| rec.get(c)).unwrap_or("").to_string(),
            time: col_time.and_then(|c| rec.get(c))
                          .and_then(|s| s.trim().parse().ok())
                          .unwrap_or(f64::NAN),
            values,
        });
    }

    Ok(Some(crate::domain::CovTabData::from_rows(rows, covariate_names)))
}

/// Read `conddist.csv` from a `.fitrx` bundle — per-subject per-ETA conditional
/// distribution summary from the SAEM `conddist` post-fit pass. Returns
/// `Ok(None)` when the entry is absent (older bundle, non-SAEM fit, or
/// `conddist` not enabled for this run).
pub fn read_conddist(fitrx_path: &Path) -> Result<Option<crate::domain::CondDistData>, FitrxError> {
    let file = std::fs::File::open(fitrx_path)?;
    let mut zip = zip::ZipArchive::new(file)?;

    let entry = match zip.by_name("conddist.csv") {
        Ok(e)  => e,
        Err(_) => return Ok(None),
    };
    let entry = bound_entry("conddist.csv", entry)?;

    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(entry);

    let headers = rdr.headers()
        .map_err(|e| FitrxError::Io(std::io::Error::other(e)))?
        .clone();

    let col = |name: &str| headers.iter().position(|h| h.eq_ignore_ascii_case(name));
    let parse = |rec: &csv::StringRecord, c: usize| -> f64 {
        rec.get(c).and_then(|s| s.trim().parse().ok()).unwrap_or(f64::NAN)
    };

    let col_id   = col("ID");
    let col_eta  = col("ETA");
    let col_mean = col("COND_MEAN");
    let col_sd   = col("COND_SD");
    let col_mode = col("COND_MODE");

    let mut rows = Vec::new();
    let mut eta_names: Vec<String> = Vec::new();
    let mut subject_ids: Vec<String> = Vec::new();
    let mut seen_etas = std::collections::HashSet::new();
    let mut seen_ids  = std::collections::HashSet::new();

    for result in rdr.records() {
        let rec = result.map_err(|e| FitrxError::Io(std::io::Error::other(e)))?;
        let id  = col_id.and_then(|c| rec.get(c)).unwrap_or("").to_string();
        let eta = col_eta.and_then(|c| rec.get(c)).unwrap_or("").to_string();
        if seen_ids.insert(id.clone())   { subject_ids.push(id.clone()); }
        if seen_etas.insert(eta.clone()) { eta_names.push(eta.clone()); }
        rows.push(crate::domain::CondDistRow {
            id,
            eta_name:  eta,
            cond_mean: col_mean.map(|c| parse(&rec, c)).unwrap_or(f64::NAN),
            cond_sd:   col_sd.map(|c| parse(&rec, c)).unwrap_or(f64::NAN),
            cond_mode: col_mode.map(|c| parse(&rec, c)).unwrap_or(f64::NAN),
        });
    }

    Ok(Some(crate::domain::CondDistData { rows, eta_names, subject_ids }))
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
    // Exports live in `ferx_outputs/` beside the bundle, never among the user's own files.
    let dir  = fitrx_path.parent().unwrap_or(std::path::Path::new(".")).join("ferx_outputs");
    std::fs::create_dir_all(&dir)?;
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
        let entry = match zip.by_name(entry_name) {
            Ok(e)  => e,
            Err(_) => continue, // not in this bundle
        };
        let mut entry = bound_entry(entry_name, entry)?;
        let mut buf = String::new();
        entry.read_to_string(&mut buf)?;
        let out_path = crate::io::textdoc::export_target(&dir, out_name);
        std::fs::write(&out_path, buf.as_bytes())
            .map_err(FitrxError::Io)?;
        crate::io::textdoc::record_export(&out_path);
        written.push(out_path);
    }
    Ok(written)
}

/// Read a convergence trace CSV from disk (lives outside the .fitrx zip).
/// Parses all columns written by ferx-core: iter, method, phase, ofv,
/// grad_norm, mh_accept_rate, lm_lambda.  Unknown/missing columns are NaN.
pub fn read_trace_csv(path: &Path) -> std::io::Result<Vec<TraceRow>> {
    let file = std::fs::File::open(path)?;
    parse_trace_csv(file)
}

/// Read `trace.csv` directly from a `.fitrx` bundle. ferx-r (>= 0.2.0) always
/// bundles the trace alongside the fit when `optimizer_trace = TRUE` was
/// used, since the external `trace_path` temp file usually doesn't survive
/// past the run. Returns `Ok(None)` when absent (older bundles, or the trace
/// was never enabled) — callers should fall back to `resolve_trace_path` +
/// `read_trace_csv` for those.
pub fn read_trace_csv_from_bundle(fitrx_path: &Path) -> Result<Option<Vec<TraceRow>>, FitrxError> {
    let file = std::fs::File::open(fitrx_path)?;
    let mut zip = zip::ZipArchive::new(file)?;
    let entry = match zip.by_name("trace.csv") {
        Ok(e)  => e,
        Err(_) => return Ok(None),
    };
    let entry = bound_entry("trace.csv", entry)?;
    Ok(Some(parse_trace_csv(entry)?))
}

/// Shared CSV-parsing body for the convergence trace, used by both the
/// external-file and in-bundle read paths.
fn parse_trace_csv<R: Read>(reader: R) -> std::io::Result<Vec<TraceRow>> {
    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(reader);

    let headers = rdr.headers()
        .map_err(std::io::Error::other)?
        .clone();

    let col = |names: &[&str]| -> Option<usize> {
        names.iter().find_map(|n| {
            headers.iter().position(|h| h.eq_ignore_ascii_case(n))
        })
    };
    let col_iter   = col(&["iter", "ITER", "ITERATION", "STEP"]).unwrap_or(0);
    let col_ofv    = col(&["ofv",  "OFV",  "OBJV", "OBJECTIVE"]).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "trace has no OFV column")
    })?;
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
        let rec = result.map_err(std::io::Error::other)?;
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
    let entry = zip.by_name("fit.json").map_err(|_| {
        FitrxError::MissingEntry("fit.json".to_string())
    })?;
    let mut entry = bound_entry("fit.json", entry)?;
    let mut buf = String::new();
    entry.read_to_string(&mut buf)?;
    let json_err = |e| FitrxError::Json { entry: "fit.json".to_string(), source: e };
    let raw: serde_json::Value = serde_json::from_str(&buf).map_err(json_err)?;
    // A missing headline key must be an error, not a silent 0 / false / NaN.
    for key in REQUIRED_FIT_KEYS {
        if raw.get(*key).is_none() {
            return Err(FitrxError::MissingKey((*key).to_string()));
        }
    }
    serde_json::from_value(raw).map_err(json_err)
}

/// Keys whose absence would otherwise be read as a plausible value (OFV 0, not converged).
const REQUIRED_FIT_KEYS: &[&str] = &["ofv", "aic", "bic", "converged", "method"];

fn read_warnings(zip: &mut zip::ZipArchive<std::fs::File>) -> Option<Vec<String>> {
    let entry = zip.by_name("warnings.txt").ok()?;
    let mut entry = bound_entry("warnings.txt", entry).ok()?;
    let mut buf = String::new();
    entry.read_to_string(&mut buf).ok()?;
    Some(buf.lines().filter(|l| !l.is_empty()).map(str::to_owned).collect())
}

fn read_text_entry(
    zip: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> Result<String, FitrxError> {
    // Validate that the requested name is a safe path component.
    safe_entry_name(name).ok_or_else(|| FitrxError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!("unsafe ZIP entry name: {name}"),
    )))?;
    let entry = zip
        .by_name(name)
        .map_err(|_| FitrxError::MissingEntry(name.to_string()))?;
    let mut entry = bound_entry(name, entry)?;
    let mut buf = String::new();
    entry.read_to_string(&mut buf)?;
    Ok(buf)
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
///
/// A parameter with zero (or non-finite) variance is *held*: a FIXed theta, or a covariance
/// pinned at zero. It has no correlation with anything, so its row and column are `NaN`
/// (the heatmap draws them as "n/a") while every other pair keeps its value. Previously one
/// held parameter discarded the whole matrix. Empty only when the data itself is unusable.
fn build_correlation_matrix(data: &[f64], n: usize) -> Vec<f64> {
    if n == 0 || data.len() < n * n { return vec![]; }
    let std_devs: Vec<f64> = (0..n).map(|i| data[i * n + i].sqrt()).collect();
    let ok = |s: f64| s.is_finite() && s > 0.0;
    if !std_devs.iter().any(|&s| ok(s)) { return vec![]; }
    (0..n * n).map(|k| {
        let i = k / n; let j = k % n;
        if ok(std_devs[i]) && ok(std_devs[j]) {
            data[k] / (std_devs[i] * std_devs[j])
        } else {
            f64::NAN
        }
    }).collect()
}

/// The free (non-held) indices of a correlation matrix, i.e. those with a finite diagonal.
fn free_indices(corr: &[f64], n: usize) -> Vec<usize> {
    (0..n).filter(|&i| corr[i * n + i].is_finite()).collect()
}

/// Compute the condition number of a covariance matrix (largest / smallest
/// eigenvalue of its correlation matrix) using the Jacobi eigenvalue algorithm.
///
/// Returns `Some(cn)` on success, `None` when the data is unusable.
/// Works without any linear-algebra dependency; accurate for n ≤ ~20.
fn condition_number_from_covariance(data: &[f64], n: usize) -> Option<f64> {
    // Build the correlation matrix first; reuse the shared helper.
    let full = build_correlation_matrix(data, n);
    if full.is_empty() { return None; }
    // Held parameters carry no information: take the eigenvalues of the free block only.
    let free = free_indices(&full, n);
    let m = free.len();
    if m < 2 { return None; }
    let mut a: Vec<f64> = free.iter().flat_map(|&i| free.iter().map(move |&j| (i, j)))
        .map(|(i, j)| full[i * n + j]).collect();
    let n = m;

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
    // theta / omega: names, SEs, and estimates all collapse to a bare
    // scalar when the model has exactly one theta/ETA (jsonlite
    // auto_unbox) — convert every field via the scalar-or-array helpers,
    // never accessed as a plain Vec directly off the wire structs.
    let theta_estimates = json_val_to_f64_vec(&w.theta.estimates);
    let theta_names     = json_val_to_str_vec(&w.theta.names);
    let se_theta        = json_val_to_f64_vec(&w.theta.se);
    let omega_names     = json_val_to_str_vec(&w.omega.names);
    let se_omega        = json_val_to_f64_vec(&w.omega.se);
    // ferx writes shrinkage as a fraction (0.316 = 31.6%), in every version through 0.4.0;
    // FitSummary and every display work in percent.
    let to_pct = |v: Vec<f64>| -> Vec<f64> { v.into_iter().map(|x| x * 100.0).collect() };
    let eta_shrinkage   = to_pct(json_val_to_f64_vec(&w.omega.shrinkage));

    // Merge warnings from fit.json (itself scalar-or-array, same collapse
    // risk) and warnings.txt (deduplicate).
    for warn in json_val_to_str_vec(&w.warnings) {
        if !warnings.contains(&warn) {
            warnings.push(warn);
        }
    }

    // method_chain: accept both a plain string and an array of strings.
    let method_chain = json_val_to_str_vec(&w.method_chain);

    // Omega: ferx stores a full N×N row-major matrix; FitSummary wants the
    // flattened lower triangle [v00, v10, v11, v20, v21, v22, ...].
    let n_eta = w.omega.matrix.cols;
    let omega = full_matrix_to_lower_triangle(&w.omega.matrix.data, n_eta);

    // Sigma: estimates / names / se can each be a scalar or an array.
    let sigma       = json_val_to_f64_vec(w.sigma.get("estimates").unwrap_or(&serde_json::Value::Null));
    let sigma_names = json_val_to_str_vec(w.sigma.get("names").unwrap_or(&serde_json::Value::Null));
    let se_sigma    = json_val_to_f64_vec(w.sigma.get("se").unwrap_or(&serde_json::Value::Null));

    // Eps shrinkage: scalar when there is one sigma component.
    let eps_shrinkage = to_pct(json_val_to_f64_vec(&w.shrinkage_eps));

    let theta_fixed = json_val_to_bool_vec(&w.theta.fixed);
    let omega_fixed = json_val_to_bool_vec(&w.omega.fixed);

    // Fitted block_sigma correlations (sigma.residual_correlations + .se_ + _fixed).
    let residual_correlations = parse_residual_correlations(&w.sigma);

    // IOV / kappa — present only when the model has kappa parameters.
    let (iov_kappa, iov_kappa_names, iov_n_kappa, iov_se_kappa, iov_shrinkage) =
        if let Some(iov) = w.iov {
            let n = iov.omega_iov.cols;
            let kappa = full_matrix_to_lower_triangle(&iov.omega_iov.data, n);
            (kappa,
             json_val_to_str_vec(&iov.kappa_names),
             n,
             json_val_to_f64_vec(&iov.se_kappa),
             to_pct(json_val_to_f64_vec(&iov.shrinkage_kappa)))
        } else {
            (vec![], vec![], 0, vec![], vec![])
        };

    // Parameter correlation matrix — derived from covariance_matrix when present.
    let (cov_corr_flat, cov_corr_n, cov_corr_names) =
        if let Some(ref cm) = w.covariance_matrix {
            let n = cm.cols;
            let corr = build_correlation_matrix(&cm.data, n);
            let fixed = json_val_to_bool_vec(&w.theta.fixed);
            let names = covariance_names(
                &theta_names, &fixed, &omega_names,
                packed_layout(&se_omega, n_eta, w.omega_is_diagonal),
                &iov_kappa_names,
                packed_layout(&iov_se_kappa, iov_n_kappa, w.kappa_is_diagonal),
                &sigma_names, &residual_correlations, n,
            );
            (corr, n, names)
        } else {
            (vec![], 0, vec![])
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
        theta:       theta_estimates,
        theta_names,
        theta_lower: vec![], // not in fit.json; available from ModelEntry.model.params
        theta_upper: vec![], // same
        omega,
        omega_names,
        n_eta,
        // IOV: the `iov` sub-object carries a full N×N row-major omega_iov matrix.
        kappa:           iov_kappa,
        kappa_names:     iov_kappa_names,
        n_kappa:         iov_n_kappa,
        se_kappa:        iov_se_kappa,
        kappa_shrinkage: iov_shrinkage,
        sigma,
        sigma_names,
        se_theta,
        se_omega,
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
        eta_shrinkage,
        eps_shrinkage,
        etabar:        vec![], // filled from ebes.csv by read_fit_summary
        etabar_pvalue: vec![],
        near_boundary:   w.r_extras.get("estimate_near_boundary").and_then(|v| v.as_bool()),
        stalled_at_init: w.r_extras.get("stalled_at_init").and_then(|v| v.as_bool()),
        max_abs_corr:    w.r_extras.get("max_abs_correlation").and_then(|v| v.as_f64()),
        model_hash:      w.model_hash.filter(|v| !v.is_empty()),
        data_hash:       w.data_hash.filter(|v| !v.is_empty()),
        theta_fixed,
        omega_fixed,
        warnings,
        trace_path: w.trace_path,
        dw_statistic: w.dw_statistic,
        iwres_lag1_r: w.iwres_lag1_r,
        warnings_structured: w.warnings_structured,
        eta_param_types,
        ferx_version: w.ferx_version.filter(|v| !v.is_empty()),
        data_path: w.data_path.filter(|v| !v.is_empty()),
        omega_is_diagonal: w.omega_is_diagonal,
        kappa_is_diagonal: w.kappa_is_diagonal,
        ofv_data: w.ofv_data,
        ofv_prior: w.ofv_prior,
        prior_summary: parse_prior_summary(&w.prior_summary),
        residual_correlations,
    }
}

/// True when an SE vector for an `n`-dimensional random-effect matrix is the packed
/// lower triangle (block layout) rather than one entry per diagonal element.
fn packed_layout(se: &[f64], n: usize, is_diag: Option<bool>) -> bool {
    match is_diag {
        Some(d) => !d && n > 1,
        None => n > 1 && se.len() == n * (n + 1) / 2,
    }
}

fn json_val_to_bool_vec(v: &serde_json::Value) -> Vec<bool> {
    match v {
        serde_json::Value::Bool(b) => vec![*b],
        serde_json::Value::Array(a) => a.iter().filter_map(|x| x.as_bool()).collect(),
        _ => vec![],
    }
}

fn as_f64_or_nan(v: Option<&serde_json::Value>) -> f64 {
    v.and_then(|x| x.as_f64()).unwrap_or(f64::NAN)
}

/// `prior_summary`: array of objects (one per priored parameter). A single row may
/// arrive as a bare object under jsonlite auto_unbox.
fn parse_prior_summary(v: &serde_json::Value) -> Vec<crate::domain::PriorRow> {
    let rows: Vec<&serde_json::Value> = match v {
        serde_json::Value::Array(a) => a.iter().collect(),
        serde_json::Value::Object(_) => vec![v],
        _ => vec![],
    };
    rows.into_iter().filter_map(|r| {
        Some(crate::domain::PriorRow {
            name: r.get("name")?.as_str()?.to_string(),
            prior_value: as_f64_or_nan(r.get("prior_value")),
            estimate: as_f64_or_nan(r.get("estimate")),
            shift_in_prior_sds: as_f64_or_nan(r.get("shift_in_prior_sds")),
            penalty: as_f64_or_nan(r.get("penalty")),
            family: r.get("family").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            prior_lower_95: as_f64_or_nan(r.get("prior_lower_95")),
            prior_upper_95: as_f64_or_nan(r.get("prior_upper_95")),
        })
    }).collect()
}

/// `sigma.residual_correlations` (+ `residual_correlation_fixed`, `se_residual_correlations`).
fn parse_residual_correlations(sigma: &serde_json::Value) -> Vec<crate::domain::ResidualCorr> {
    let rows: Vec<&serde_json::Value> = match sigma.get("residual_correlations") {
        Some(serde_json::Value::Array(a)) => a.iter().collect(),
        Some(o @ serde_json::Value::Object(_)) => vec![o],
        _ => return vec![],
    };
    let fixed = json_val_to_bool_vec(sigma.get("residual_correlation_fixed").unwrap_or(&serde_json::Value::Null));
    let se = json_val_to_f64_vec(sigma.get("se_residual_correlations").unwrap_or(&serde_json::Value::Null));
    rows.into_iter().enumerate().map(|(k, r)| crate::domain::ResidualCorr {
        sigma_i: r.get("sigma_i").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
        sigma_j: r.get("sigma_j").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
        rho: as_f64_or_nan(r.get("rho")),
        fixed: fixed.get(k).copied().unwrap_or(false),
        se: se.get(k).copied().unwrap_or(f64::NAN),
    }).collect()
}

/// Names for the rows/columns of the covariance matrix, in the engine's packing order:
/// theta, omega (diagonal, or packed lower triangle column by column for a block),
/// kappa (same), sigma, then `block_sigma` correlations last. Returns `P1..Pn` when
/// no candidate list matches the matrix size.
#[allow(clippy::too_many_arguments)]
fn covariance_names(
    theta: &[String], theta_fixed: &[bool],
    omega: &[String], omega_packed: bool,
    kappa: &[String], kappa_packed: bool,
    sigma: &[String], rcorr: &[crate::domain::ResidualCorr],
    n: usize,
) -> Vec<String> {
    fn re_names(names: &[String], packed: bool) -> Vec<String> {
        if !packed { return names.to_vec(); }
        let mut out = Vec::new();
        for c in 0..names.len() {
            for r in c..names.len() {
                out.push(if r == c { names[c].clone() }
                         else { format!("COV({},{})", names[r], names[c]) });
            }
        }
        out
    }
    let mut tail: Vec<String> = re_names(omega, omega_packed);
    tail.extend(re_names(kappa, kappa_packed));
    tail.extend_from_slice(sigma);
    for rc in rcorr {
        let a = sigma.get(rc.sigma_i).map(String::as_str).unwrap_or("?");
        let b = sigma.get(rc.sigma_j).map(String::as_str).unwrap_or("?");
        tail.push(format!("RHO({a},{b})"));
    }
    // Candidate 1: every theta. Candidate 2: only the estimated (non-fixed) thetas.
    let mut all = theta.to_vec();
    all.extend(tail.iter().cloned());
    if all.len() == n { return all; }
    let mut free: Vec<String> = theta.iter().enumerate()
        .filter(|(i, _)| !theta_fixed.get(*i).copied().unwrap_or(false))
        .map(|(_, t)| t.clone()).collect();
    free.extend(tail);
    if free.len() == n { return free; }
    (1..=n).map(|i| format!("P{i}")).collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A `.fitrx` whose `predictions.csv` entry declares a size over
    /// `MAX_ENTRY_BYTES` must be rejected before the reader tries to hold
    /// the whole thing in memory. Built in-memory/on-disk from highly
    /// compressible filler (real DEFLATE data, not a forged header) so the
    /// test is fast and has no external fixture dependency.
    #[test]
    fn read_predictions_rejects_oversized_entry() {
        use std::io::Write;

        let tmp_path = std::env::temp_dir().join("ferxgui_test_oversized_predictions.fitrx");
        {
            let file = std::fs::File::create(&tmp_path).expect("create temp zip");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("predictions.csv", options).expect("start entry");

            let chunk = vec![0u8; 1024 * 1024]; // 1 MB of zeros — compresses to almost nothing.
            let target_bytes = (MAX_ENTRY_BYTES + 1024) as usize;
            let mut written = 0usize;
            while written < target_bytes {
                zip.write_all(&chunk).expect("write filler chunk");
                written += chunk.len();
            }
            zip.finish().expect("finish zip");
        }

        let result = read_predictions(&tmp_path);
        let _ = std::fs::remove_file(&tmp_path);

        match result {
            Err(FitrxError::Io(e)) => assert_eq!(e.kind(), std::io::ErrorKind::InvalidData),
            Ok(_)          => panic!("expected a size-limit rejection, got Ok"),
            Err(other)     => panic!("expected FitrxError::Io, got: {other}"),
        }
    }

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
    fn single_element_fields_do_not_collapse_the_parse() {
        // R's jsonlite `auto_unbox = TRUE` serializes a length-1 vector as a
        // bare scalar instead of a single-element array — e.g. a model with
        // exactly one theta, one ETA, one kappa, and one warning (the
        // single-method, non-chained case is also the *common* case, not an
        // edge case). This reproduces that shape end-to-end and must not
        // panic or fail the parse; every field must still resolve to a
        // length-1 vector with the right value.
        const FIXTURE: &str = r#"{
            "method": "foce",
            "method_chain": "foce",
            "converged": true,
            "ofv": -280.36,
            "theta": {"estimates": 0.134, "names": "TVCL", "se": 0.0012, "fixed": false},
            "omega": {"matrix": {"data": [0.07], "cols": 1}, "names": "ETA_CL",
                      "se": 0.02, "shrinkage": 0.125},
            "sigma": {"estimates": 0.05, "names": "PROP", "se": 0.004},
            "iov": {"kappa_names": "OCC1", "se_kappa": 0.01, "shrinkage_kappa": 0.05,
                    "omega_iov": {"rows": 1, "cols": 1, "data": [0.02]}},
            "warnings": "Negative IWRES autocorrelation detected.",
            "covariance_status": "computed"
        }"#;
        let wire: FitWire = serde_json::from_str(FIXTURE).expect("parse FitWire");
        let s = wire_to_summary(wire, vec![]);
        assert_eq!(s.method_chain, vec!["foce"]);
        assert_eq!(s.theta, vec![0.134]);
        assert_eq!(s.theta_names, vec!["TVCL"]);
        assert_eq!(s.se_theta, vec![0.0012]);
        assert_eq!(s.omega_names, vec!["ETA_CL"]);
        assert_eq!(s.se_omega, vec![0.02]);
        assert_eq!(s.eta_shrinkage, vec![12.5]);
        assert_eq!(s.sigma, vec![0.05]);
        assert_eq!(s.sigma_names, vec!["PROP"]);
        assert_eq!(s.kappa_names, vec!["OCC1"]);
        assert_eq!(s.se_kappa, vec![0.01]);
        assert_eq!(s.kappa_shrinkage, vec![5.0]);
        assert_eq!(s.warnings, vec!["Negative IWRES autocorrelation detected."]);
    }

    #[test]
    fn warfarin_fitrx_parses_correctly() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/warfarin.fitrx");
        let s = super::read_fit_summary(&path).expect("parse should succeed");
        assert!(s.converged, "expected converged");
        // This file is regenerated by whatever run last happened on this
        // machine (different method/settings each time), so a pinned OFV
        // would make this test flaky against legitimate re-fits — check a
        // spacious sanity range for the warfarin example instead.
        assert!(s.ofv.is_finite() && (-400.0..-200.0).contains(&s.ofv), "ofv={}", s.ofv);
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
        let fit = FitSummary { omega: vec![1.0, 0.3, 0.5, 0.1, 0.2, 0.8], n_eta: 3, ..Default::default() };
        assert_eq!(fit.omega_value(0, 0), Some(1.0));
        assert_eq!(fit.omega_value(1, 0), Some(0.3));
        assert_eq!(fit.omega_value(2, 1), Some(0.2));
        // Symmetric access
        assert_eq!(fit.omega_value(0, 1), Some(0.3));
    }

    /// Numbers from a real `warfarin_block_omega` bundle written by ferx 0.4.0:
    /// omega.se is the packed lower triangle, column-major.
    const BLOCK_OMEGA: &str = r#"{
        "method": "foce", "converged": true, "ofv": -280.4858,
        "ferx_version": "0.4.0",
        "theta": {"names": ["TVCL","TVV","TVKA"], "estimates": [0.133,7.73,0.725],
                  "se": [0.0067,0.236,0.124], "fixed": [false,false,false]},
        "omega": {"names": ["ETA_CL","ETA_V","ETA_KA"],
                  "matrix": {"rows":3,"cols":3,"data":[0.0286,0.0018,0,0.0018,0.0096,0,0,0,0.349]},
                  "se": [0.0128, 0.0053, 0.0, 0.0043, 0.0, 0.1608]},
        "sigma": {"names": ["PROP_ERR"], "estimates": [0.0107], "se": [0.00095]},
        "omega_is_diagonal": false,
        "covariance_status": "computed",
        "covariance_matrix": {"rows":10,"cols":10,"data": []}
    }"#;

    #[test]
    fn block_omega_diagonal_ses_come_from_packed_layout() {
        let wire: FitWire = serde_json::from_str(BLOCK_OMEGA).unwrap();
        let s = wire_to_summary(wire, vec![]);
        assert_eq!(s.omega_is_diagonal, Some(false));
        assert_eq!(s.ferx_version.as_deref(), Some("0.4.0"));
        assert_eq!(s.se_omega_diag_vec(), vec![0.0128, 0.0043, 0.1608]);
        assert_eq!(s.se_omega_offdiag(1, 0), Some(0.0053));
        assert_eq!(s.se_omega_offdiag(0, 1), Some(0.0053));
        assert_eq!(s.se_omega_offdiag(2, 0), Some(0.0));
    }

    #[test]
    fn diagonal_omega_se_is_unchanged() {
        let mut s = FitSummary {
            n_eta: 3,
            se_omega: vec![0.1, 0.2, 0.3],
            omega_is_diagonal: Some(true),
            ..Default::default()
        };
        assert_eq!(s.se_omega_diag_vec(), vec![0.1, 0.2, 0.3]);
        s.omega_is_diagonal = None; // older bundle: inferred from length
        assert_eq!(s.se_omega_diag_vec(), vec![0.1, 0.2, 0.3]);
        assert_eq!(s.se_omega_offdiag(1, 0), None);
    }

    #[test]
    fn packed_index_matches_column_major() {
        // n = 3: (0,0)(1,0)(2,0)(1,1)(2,1)(2,2) -> 0..6
        assert_eq!(crate::domain::packed_col_major_index(3, 0, 0), 0);
        assert_eq!(crate::domain::packed_col_major_index(3, 1, 0), 1);
        assert_eq!(crate::domain::packed_col_major_index(3, 2, 0), 2);
        assert_eq!(crate::domain::packed_col_major_index(3, 1, 1), 3);
        assert_eq!(crate::domain::packed_col_major_index(3, 2, 1), 4);
        assert_eq!(crate::domain::packed_col_major_index(3, 2, 2), 5);
    }

    #[test]
    fn covariance_names_cover_block_omega_kappa_and_rho() {
        let t = vec!["TVCL".to_string(), "TVV".to_string()];
        let om = vec!["ETA_CL".to_string(), "ETA_V".to_string()];
        let kp = vec!["KAPPA_CL".to_string()];
        let sg = vec!["PROP".to_string(), "ADD".to_string()];
        let rc = vec![crate::domain::ResidualCorr { sigma_i: 1, sigma_j: 0, rho: 0.1, fixed: false, se: 0.1 }];
        // 2 theta + 3 packed omega + 1 kappa + 2 sigma + 1 rho = 9
        let n = covariance_names(&t, &[false, false], &om, true, &kp, false, &sg, &rc, 9);
        assert_eq!(n, vec!["TVCL","TVV","ETA_CL","COV(ETA_V,ETA_CL)","ETA_V","KAPPA_CL","PROP","ADD","RHO(ADD,PROP)"]);
        // Size mismatch falls back to P1..Pn.
        let n = covariance_names(&t, &[false, false], &om, true, &kp, false, &sg, &rc, 4);
        assert_eq!(n, vec!["P1","P2","P3","P4"]);
        // Fixed thetas dropped from the matrix.
        let n = covariance_names(&t, &[true, false], &om, false, &[], false, &sg[..1], &[], 4);
        assert_eq!(n, vec!["TVV","ETA_CL","ETA_V","PROP"]);
    }

    #[test]
    fn prior_split_and_residual_correlations_parse() {
        const J: &str = r#"{
            "method": "foce", "ofv": -279.1978, "ofv_data": -280.1263, "ofv_prior": 0.9285,
            "prior_summary": [{"name":"TVCL","prior_value":0.15,"estimate":0.1363,
                "shift_in_prior_sds":-0.96,"penalty":0.9285,"family":"lognormal",
                "prior_lower_95":0.1234,"prior_upper_95":0.1824}],
            "sigma": {"names":["PROP_ERR","ADD_ERR"],"estimates":[0.01,0.0003],"se":[0.001,0.009],
                "residual_correlations":[{"sigma_i":1,"sigma_j":0,"rho":0.995}],
                "residual_correlation_fixed":[false],"se_residual_correlations":[29.2]}
        }"#;
        let s = wire_to_summary(serde_json::from_str(J).unwrap(), vec![]);
        assert!(s.has_prior());
        assert_eq!(s.prior_summary.len(), 1);
        assert_eq!(s.prior_summary[0].family, "lognormal");
        assert!((s.ofv_cmp() + 280.1263).abs() < 1e-9);
        assert_eq!(s.residual_correlations.len(), 1);
        assert_eq!(s.residual_correlations[0].sigma_i, 1);
        assert!((s.residual_correlations[0].se - 29.2).abs() < 1e-9);
    }

    #[test]
    fn unpriored_fit_has_no_prior_and_ofv_cmp_is_ofv() {
        let s = wire_to_summary(serde_json::from_str(r#"{"ofv": -10.0, "ofv_prior": 0.0, "prior_summary": null}"#).unwrap(), vec![]);
        assert!(!s.has_prior());
        assert_eq!(s.ofv_cmp(), -10.0);
    }

    #[test]
    fn older_ferx_versions_are_flagged_only_when_known() {
        let mut s = FitSummary::default();
        assert!(!s.fitted_before((0, 4, 0)));              // key absent: no notice
        s.ferx_version = Some("0.3.0".into());
        assert!(s.fitted_before((0, 4, 0)));
        s.ferx_version = Some("0.4.0".into());
        assert!(!s.fitted_before((0, 4, 0)));
        s.ferx_version = Some("0.4.0.9000".into());
        assert!(!s.fitted_before((0, 4, 0)));
    }

    /// Real ferx 0.4.0 bundles (committed fixtures): block omega, IOV and a MAP prior.
    #[test]
    fn real_040_bundles_parse() {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let b = read_fit_summary(&d.join("warfarin_block_omega.fitrx")).unwrap();
        assert_eq!(b.omega_is_diagonal, Some(false));
        assert_eq!(b.se_omega_diag_vec().len(), 3);
        assert_eq!(b.cov_corr_n, 10); // 3 theta + 6 packed omega + 1 sigma
        assert_eq!(b.cov_corr_names[9], "PROP_ERR");
        assert_eq!(b.cov_corr_names.len(), 10);
        let i = read_fit_summary(&d.join("warfarin_iov.fitrx")).unwrap();
        assert_eq!(i.n_kappa, 1);
        let p = read_fit_summary(&d.join("warfarin_prior.fitrx")).unwrap();
        assert!(p.has_prior());
        assert_eq!(p.prior_summary[0].name, "TVCL");
    }

    #[test]
    fn bundle_is_written_next_to_the_model_with_the_same_stem() {
        let p = bundle_path_for(Path::new("/proj/models/warfarin.ferx"));
        assert_eq!(p, PathBuf::from("/proj/models/warfarin.fitrx"));
        // Dotted stems keep everything before the final extension.
        assert_eq!(bundle_path_for(Path::new("/p/m.v2.ferx")), PathBuf::from("/p/m.v2.fitrx"));
        // A relative model path still yields an absolute bundle path.
        assert!(bundle_path_for(Path::new("rel/m.ferx")).is_absolute());
    }

    #[test]
    fn a_held_parameter_blanks_its_own_row_not_the_whole_matrix() {
        // Variance-covariance of 3 parameters where the middle one is fixed (variance 0),
        // the shape of a real bundle with a FIXed theta (13x13 with one zero on the diagonal).
        let data = [
            4.0, 0.0, 1.2,
            0.0, 0.0, 0.0,
            1.2, 0.0, 9.0,
        ];
        let c = build_correlation_matrix(&data, 3);
        assert_eq!(c.len(), 9);
        assert!((c[0] - 1.0).abs() < 1e-12 && (c[8] - 1.0).abs() < 1e-12);
        assert!((c[2] - 1.2 / 6.0).abs() < 1e-12, "free pair keeps its correlation");
        assert!(c[1].is_nan() && c[3].is_nan() && c[4].is_nan() && c[5].is_nan() && c[7].is_nan());
        // Condition number comes from the free 2x2 block only: (1+r)/(1-r).
        let r = 0.2_f64;
        let cn = condition_number_from_covariance(&data, 3).unwrap();
        assert!((cn - (1.0 + r) / (1.0 - r)).abs() < 1e-6, "{cn}");
    }

    #[test]
    fn all_held_or_malformed_data_still_gives_no_matrix() {
        assert!(build_correlation_matrix(&[0.0, 0.0, 0.0, 0.0], 2).is_empty());
        assert!(build_correlation_matrix(&[1.0], 2).is_empty());
    }

    /// A bundle with a FIXed theta keeps its correlation matrix, with that parameter's row and
    /// column blank, and the fixed flag is read from fit.json.
    #[test]
    fn real_bundle_with_a_fixed_theta_keeps_its_correlation_matrix() {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/warfarin_fix.fitrx");
        let s = read_fit_summary(&p).unwrap();
        assert!(s.cov_corr_n >= 6, "matrix survives: {}", s.cov_corr_n);
        assert_eq!(s.cov_corr_flat.len(), s.cov_corr_n * s.cov_corr_n);
        assert_eq!(s.theta_fixed, vec![false, false, true]);
        let k = s.cov_corr_names.iter().position(|n| n == "TVKA");
        if let Some(k) = k {
            assert!(s.cov_corr_flat[k * s.cov_corr_n + k].is_nan(), "fixed theta's diagonal is blank");
        }
        assert!((s.cov_corr_flat[0] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn shrinkage_is_converted_from_ferx_fractions_to_percent() {
        // Values from a real 0.4.0 bundle: ETA_EMAX 0.3156 is the "32%" in its own warning.
        const J: &str = r#"{"omega":{"names":["A","B"],"matrix":{"rows":2,"cols":2,"data":[0.1,0,0,0.1]},
            "shrinkage":[0.201,0.3156]},"shrinkage_eps":0.1117,
            "iov":{"kappa_names":"K","shrinkage_kappa":0.374,"omega_iov":{"rows":1,"cols":1,"data":[0.02]}}}"#;
        let s = wire_to_summary(serde_json::from_str(J).unwrap(), vec![]);
        assert!((s.eta_shrinkage[1] - 31.56).abs() < 1e-9);
        assert!((s.eta_shrinkage[0] - 20.1).abs() < 1e-9);
        assert!((s.eps_shrinkage[0] - 11.17).abs() < 1e-9);
        assert!((s.kappa_shrinkage[0] - 37.4).abs() < 1e-9);
    }

    fn write_bundle(name: &str, fit_json: &str) -> PathBuf {
        use std::io::Write;
        let path = std::env::temp_dir().join(format!("ferxgui_{name}_{}.fitrx", std::process::id()));
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("fit.json", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(fit_json.as_bytes()).unwrap();
        zip.finish().unwrap();
        path
    }

    #[test]
    fn missing_ofv_is_parse_error() {
        let p = write_bundle("noofv", r#"{"method":"focei","aic":1,"bic":2,"converged":true}"#);
        let r = read_fit_summary(&p);
        let _ = std::fs::remove_file(&p);
        match r {
            Err(FitrxError::MissingKey(k)) => assert_eq!(k, "ofv"),
            other => panic!("expected MissingKey(ofv), got {other:?}"),
        }
    }

    const BASE: &str = r#""method":"focei","ofv":1.5,"aic":2,"bic":3,"converged":true"#;

    #[test]
    fn near_boundary_from_r_extras() {
        let t = |extra: &str| {
            let p = write_bundle("nb", &format!("{{{BASE}{extra}}}"));
            let s = read_fit_summary(&p).unwrap();
            let _ = std::fs::remove_file(&p);
            s
        };
        let s = t(r#","r_extras":{"estimate_near_boundary":true,"stalled_at_init":false,"max_abs_correlation":0.93}"#);
        assert_eq!(s.near_boundary, Some(true));
        assert!(s.has_boundary_hit());
        assert_eq!(s.stalled_at_init, Some(false));
        assert_eq!(s.max_abs_corr, Some(0.93));
        assert_eq!(t(r#","r_extras":{"estimate_near_boundary":false}"#).near_boundary, Some(false));
        let absent = t("");
        assert_eq!(absent.near_boundary, None, "absent must not read as false");
        assert!(!absent.has_boundary_hit());
    }

    #[test]
    fn fixed_flags_and_hashes_from_fit_json() {
        let p = write_bundle("fx", &format!(
            r#"{{{BASE},"model_hash":"aa","data_hash":"bb",
               "theta":{{"estimates":[1,2],"names":["A","B"],"fixed":[false,true]}},
               "omega":{{"names":["E"],"fixed":false}}}}"#));
        let s = read_fit_summary(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        assert_eq!(s.theta_fixed, vec![false, true]);
        assert!(s.is_theta_fixed(1) && !s.is_theta_fixed(0));
        assert_eq!(s.omega_fixed, vec![false]);
        assert!(s.has_identity());
        assert!(s.estimates_fingerprint().is_some());
    }

    #[test]
    fn fingerprint_ignores_standard_error_changes() {
        let mut a = FitSummary { model_hash: Some("m".into()), data_hash: Some("d".into()),
            theta: vec![1.0, 2.0], se_theta: vec![0.1, 0.2], ..Default::default() };
        let f1 = a.estimates_fingerprint().unwrap();
        a.se_theta = vec![9.0, 9.0];
        assert_eq!(f1, a.estimates_fingerprint().unwrap());
        a.theta[0] = 1.0000001;
        assert_ne!(f1, a.estimates_fingerprint().unwrap());
        let legacy = FitSummary::default();
        assert!(legacy.estimates_fingerprint().is_none());
    }

    #[test]
    fn trace_without_ofv_is_error() {
        let r = parse_trace_csv("iter,foo\n1,2\n".as_bytes());
        assert!(r.is_err());
        assert!(parse_trace_csv("iter,ofv\n1,2\n".as_bytes()).is_ok());
    }

    #[test]
    fn predictions_missing_column_is_error() {
        use std::io::Write;
        let path = std::env::temp_dir().join(format!("ferxgui_predmiss_{}.fitrx", std::process::id()));
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("predictions.csv", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"ID,TIME,DV,PRED\n1,0,1,1\n").unwrap();
        zip.finish().unwrap();
        let r = read_predictions(&path);
        let _ = std::fs::remove_file(&path);
        assert!(matches!(r, Err(FitrxError::Io(ref e)) if e.to_string().contains("IPRED")), "{r:?}");
    }
}

#[cfg(test)]
mod fixture_tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
    }

    #[test]
    fn warfarin_identity_matches_the_file_bytes() {
        let s = read_fit_summary(&fixture("warfarin.fitrx")).unwrap();
        assert_eq!(s.model_hash.as_deref(), Some("2ea0ff6e94efed4d9487b6c327d9bc317795cc0c9aa862d0d41b1e6965d9252a"));
        assert_eq!(s.data_hash.as_deref(), Some("a6ab771d516f67906c5ed509c4b667a8585d2c752a519eb7dc15d40731c2d0ed"));
        assert_eq!(crate::io::fsutil::sha256_file_cached(&fixture("warfarin.ferx")), s.model_hash);
        assert_eq!(crate::io::fsutil::sha256_file_cached(&fixture("warfarin.csv")), s.data_hash);
        assert!(s.has_identity());
    }

    #[test]
    fn etabar_matches_r_golden_warfarin() {
        // Plan §5.3: one-sample t test of the EBEs, warfarin FOCEI, n = 10.
        let ebes = read_ebes(&fixture("warfarin.fitrx")).unwrap().unwrap();
        let bars = ebes.eta_bar();
        let want = [("ETA_CL", 4.28e-6, 0.99994), ("ETA_V", -4.07e-5, 0.99903), ("ETA_KA", -1.39e-4, 0.99944)];
        assert_eq!(bars.len(), 3);
        for (k, (name, mean, p)) in want.iter().enumerate() {
            let b = bars[k].as_ref().unwrap();
            assert!(ebes.eta_names[k].eq_ignore_ascii_case(name) || ebes.eta_names[k].contains("ETA"), "{name}");
            assert_eq!(b.df, 9.0);
            assert!((b.mean - mean).abs() < 0.005 * mean.abs(), "{name} mean {} vs {mean}", b.mean);
            assert!((b.p - p).abs() < 2e-4, "{name} p {} vs {p}", b.p);
        }
    }

    #[test]
    fn warfarin_log_scale_ci_and_cv_goldens() {
        use crate::domain::stats::{cv_pct_lognormal, wald_ci95};
        let s = read_fit_summary(&fixture("warfarin.fitrx")).unwrap();
        // Plan §5.6 (TVCL, TVV, TVKA) natural vs log-scale targets.
        let want = [([0.11879, 0.14661], [0.11949, 0.14736]),
                    ([7.26720, 8.20821], [7.28122, 8.22281]),
                    ([0.51946, 1.10213], [0.56606, 1.16135])];
        for (i, (nat, log)) in want.iter().enumerate() {
            assert!(s.theta_is_positive(i), "theta {i} has a positive lower bound");
            let (nlo, nhi) = wald_ci95(s.theta[i], s.se_theta[i], false).unwrap();
            let (llo, lhi) = wald_ci95(s.theta[i], s.se_theta[i], true).unwrap();
            for (g, w) in [(nlo, nat[0]), (nhi, nat[1]), (llo, log[0]), (lhi, log[1])] {
                assert!((g - w).abs() / w < 2e-3, "theta {i}: {g} vs {w}");
            }
        }
        // §5.4: CV% of the three log-normal ETAs.
        for (k, cv) in [17.03, 9.82, 63.18].iter().enumerate() {
            let w2 = s.omega_value(k, k).unwrap();
            assert!((cv_pct_lognormal(w2) - cv).abs() < 0.01, "eta {k}: {}", cv_pct_lognormal(w2));
        }
        // §5.6: the ETA_KA variance interval must not cross zero on the log scale.
        let (nat_lo, _) = wald_ci95(s.omega_value(2, 2).unwrap(), s.se_omega_diag(2).unwrap(), false).unwrap();
        let (log_lo, log_hi) = wald_ci95(s.omega_value(2, 2).unwrap(), s.se_omega_diag(2).unwrap(), true).unwrap();
        assert!(nat_lo < 0.05 && log_lo > 0.12 && log_hi < 0.9, "{nat_lo} {log_lo} {log_hi}");
    }
}
