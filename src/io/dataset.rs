//! Reads the original NONMEM-style dataset and lines its observation records up with a
//! fit's prediction rows, so any dataset column (CMT, MDV, WT, ...) can be used to filter
//! or colour the Evaluation plots. `predictions.csv` itself only carries a few columns.

use std::path::{Path, PathBuf};

use crate::domain::EvalData;

/// Largest dataset read for the join.
const MAX_DATASET_BYTES: u64 = 200 * 1024 * 1024;

/// A dataset as raw text cells.
#[derive(Debug, Clone, Default)]
pub struct RawDataset {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Dataset columns aligned to the prediction rows (one value per prediction row).
pub type AlignedColumns = Vec<(String, Vec<String>)>;

pub fn read_dataset(path: &Path) -> Result<RawDataset, String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_DATASET_BYTES {
        return Err(format!("dataset is {} MB; the join is limited to {} MB",
            size / (1024 * 1024), MAX_DATASET_BYTES / (1024 * 1024)));
    }
    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_path(path)
        .map_err(|e| e.to_string())?;
    let headers: Vec<String> = rdr.headers().map_err(|e| e.to_string())?
        .iter().map(|h| h.trim().trim_start_matches('\u{feff}').to_string()).collect();
    let mut rows = Vec::new();
    for rec in rdr.records() {
        let rec = rec.map_err(|e| e.to_string())?;
        rows.push(rec.iter().map(str::to_string).collect());
    }
    Ok(RawDataset { headers, rows })
}

/// Finds the dataset file for a fit: a user-chosen path, the path recorded in the bundle
/// (as written, or relative to the bundle), the same file name next to the bundle, and the
/// model's own `[data]` path.
pub fn resolve_dataset_path(
    user_choice: Option<&Path>,
    recorded: Option<&str>,
    fitrx_path: Option<&Path>,
    model_declared: Option<&str>,
    model_dir: Option<&Path>,
) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = user_choice { candidates.push(p.to_path_buf()); }
    let bundle_dir = fitrx_path.and_then(Path::parent);
    if let Some(rec) = recorded.filter(|r| !r.is_empty()) {
        let p = PathBuf::from(rec);
        candidates.push(p.clone());
        if let Some(dir) = bundle_dir {
            if p.is_relative() { candidates.push(dir.join(&p)); }
            if let Some(name) = p.file_name() { candidates.push(dir.join(name)); }
        }
    }
    if let (Some(decl), Some(dir)) = (model_declared.filter(|d| !d.is_empty()), model_dir) {
        let p = PathBuf::from(decl);
        candidates.push(if p.is_absolute() { p } else { dir.join(p) });
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn col_index(headers: &[String], name: &str) -> Option<usize> {
    headers.iter().position(|h| h.eq_ignore_ascii_case(name))
}

fn parse_num(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() || t == "." { f64::NAN } else { t.parse().unwrap_or(f64::NAN) }
}

/// Canonical form of an ID cell, so "1", "1.0" and " 1" compare equal.
fn id_key(s: &str) -> String {
    let t = s.trim();
    match t.parse::<f64>() {
        Ok(v) if v.is_finite() => format!("{v}"),
        _ => t.to_string(),
    }
}

fn close(a: f64, b: f64) -> bool {
    if a.is_nan() || b.is_nan() { return a.is_nan() && b.is_nan(); }
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

fn cell(r: &[String], c: usize) -> &str {
    r.get(c).map(String::as_str).unwrap_or("")
}

/// Lines the dataset's observation records up with `eval.rows`, in order, matching each
/// prediction row on (ID, TIME, DV). Dataset records that are not predicted (doses, rows the
/// fit excluded) are skipped. Fails, naming how far it got, when a prediction row has no
/// matching record, which means the file is not the one the fit used.
pub fn align_to_predictions(raw: &RawDataset, eval: &EvalData) -> Result<AlignedColumns, String> {
    let (Some(c_id), Some(c_time), Some(c_dv)) = (
        col_index(&raw.headers, "ID"),
        col_index(&raw.headers, "TIME"),
        col_index(&raw.headers, "DV"),
    ) else {
        return Err("the dataset has no ID, TIME and DV columns".into());
    };
    let c_evid = col_index(&raw.headers, "EVID");

    let mut picked: Vec<usize> = Vec::with_capacity(eval.rows.len());
    let mut cursor = 0usize;
    for (n, p) in eval.rows.iter().enumerate() {
        let want_id = id_key(&p.id);
        let mut found = None;
        while cursor < raw.rows.len() {
            let r = &raw.rows[cursor];
            let is_obs = c_evid.is_none_or(|c| parse_num(cell(r, c)) == 0.0 || cell(r, c).is_empty());
            if is_obs
                && id_key(cell(r, c_id)) == want_id
                && close(parse_num(cell(r, c_time)), p.time)
                && close(parse_num(cell(r, c_dv)), p.dv)
            {
                found = Some(cursor);
                cursor += 1;
                break;
            }
            cursor += 1;
        }
        match found {
            Some(i) => picked.push(i),
            None => return Err(format!(
                "only {n} of {} prediction rows match the dataset (ID {}, TIME {}); \
                 it may have been edited since the fit",
                eval.rows.len(), p.id, p.time)),
        }
    }

    Ok(raw.headers.iter().enumerate()
        .filter(|(i, h)| *i != c_id && *i != c_time && *i != c_dv && !h.is_empty())
        .map(|(i, h)| (h.clone(), picked.iter().map(|&r| cell(&raw.rows[r], i).to_string()).collect()))
        .collect())
}

/// Loads and aligns in one step; the error string is shown to the user.
pub fn load_aligned(path: &Path, eval: &EvalData) -> Result<AlignedColumns, String> {
    let raw = read_dataset(path)?;
    align_to_predictions(&raw, eval)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::PredRow;

    fn pred(id: &str, time: f64, dv: f64) -> PredRow {
        PredRow { id: id.into(), time, dv, pred: 1.0, ipred: 1.0, cwres: 0.0, iwres: 0.0, ebe_ofv: f64::NAN, tad: f64::NAN }
    }

    fn raw(csv: &str) -> RawDataset {
        let mut lines = csv.trim().lines();
        let headers = lines.next().unwrap().split(',').map(str::to_string).collect();
        let rows = lines.map(|l| l.split(',').map(str::to_string).collect()).collect();
        RawDataset { headers, rows }
    }

    const DATA: &str = "ID,TIME,DV,EVID,AMT,CMT,MDV,WT\n\
        1,0,.,1,100,1,1,70\n\
        1,0.5,4.3,0,.,1,0,70\n\
        1,1,6.9,0,.,2,0,70\n\
        2,0,.,1,100,1,1,55\n\
        2,0.5,3.1,0,.,1,0,55\n";

    #[test]
    fn aligns_observations_and_skips_dose_records() {
        let eval = EvalData::from_rows(vec![pred("1", 0.5, 4.3), pred("1", 1.0, 6.9), pred("2", 0.5, 3.1)]);
        let cols = align_to_predictions(&raw(DATA), &eval).unwrap();
        let get = |n: &str| cols.iter().find(|(h, _)| h == n).map(|(_, v)| v.clone()).unwrap();
        assert_eq!(get("CMT"), vec!["1", "2", "1"]);
        assert_eq!(get("WT"), vec!["70", "70", "55"]);
        assert!(cols.iter().all(|(h, _)| h != "ID" && h != "TIME" && h != "DV"));
    }

    #[test]
    fn fails_with_progress_when_the_dataset_differs() {
        let eval = EvalData::from_rows(vec![pred("1", 0.5, 4.3), pred("1", 1.0, 9.9)]);
        let err = align_to_predictions(&raw(DATA), &eval).unwrap_err();
        assert!(err.contains("only 1 of 2"), "{err}");
    }

    #[test]
    fn id_formatting_differences_still_match() {
        let eval = EvalData::from_rows(vec![pred("1.0", 0.5, 4.3)]);
        assert!(align_to_predictions(&raw(DATA), &eval).is_ok());
    }

    #[test]
    fn duplicate_times_are_matched_in_order() {
        let d = "ID,TIME,DV,EVID,CMT\n1,1,5,0,1\n1,1,7,0,2\n";
        let eval = EvalData::from_rows(vec![pred("1", 1.0, 5.0), pred("1", 1.0, 7.0)]);
        let cols = align_to_predictions(&raw(d), &eval).unwrap();
        assert_eq!(cols.iter().find(|(h, _)| h == "CMT").unwrap().1, vec!["1", "2"]);
    }

    #[test]
    fn rows_the_fit_did_not_predict_are_skipped() {
        // MDV=1 observation row (e.g. excluded) sits between two predicted rows.
        let d = "ID,TIME,DV,EVID,MDV\n1,1,5,0,0\n1,2,6,0,1\n1,3,7,0,0\n";
        let eval = EvalData::from_rows(vec![pred("1", 1.0, 5.0), pred("1", 3.0, 7.0)]);
        let cols = align_to_predictions(&raw(d), &eval).unwrap();
        assert_eq!(cols.iter().find(|(h, _)| h == "MDV").unwrap().1, vec!["0", "0"]);
    }

    #[test]
    fn missing_key_columns_are_reported() {
        let d = "ID,TIME,X\n1,1,2\n";
        assert!(align_to_predictions(&raw(d), &EvalData::default()).unwrap_err().contains("no ID"));
    }

    #[test]
    fn path_resolution_prefers_the_user_choice_then_the_recorded_path() {
        let dir = std::env::temp_dir().join("ferxgui_ds_resolve");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let ds = dir.join("d.csv");
        std::fs::write(&ds, "ID,TIME,DV\n").unwrap();
        let fitrx = dir.join("m.fitrx");
        // Recorded path is stale but the same file name sits next to the bundle.
        let got = resolve_dataset_path(None, Some("/gone/elsewhere/d.csv"), Some(&fitrx), None, None);
        assert_eq!(got, Some(ds.clone()));
        // Relative to the model's own folder via its [data] block.
        let got = resolve_dataset_path(None, None, None, Some("d.csv"), Some(&dir));
        assert_eq!(got, Some(ds.clone()));
        assert_eq!(resolve_dataset_path(None, Some("/nope/x.csv"), None, None, None), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// With `FERX_TEST_BUNDLES` set: aligns real bundles to their real datasets.
    #[test]
    fn real_bundles_align_with_their_datasets() {
        let Ok(dir) = std::env::var("FERX_TEST_BUNDLES") else { return };
        let ex = "/opt/homebrew/lib/R/4.6/site-library/ferx/examples/data";
        for (bundle, data) in [("warfarin_iov", "warfarin_iov"), ("warfarin_block_omega", "warfarin_block_omega"), ("warfarin_bloq", "warfarin_bloq"), ("two_cpt_oral_cov", "two_cpt_oral_cov")] {
            let eval = crate::io::fitrx::read_predictions(&std::path::Path::new(&dir).join(format!("{bundle}.fitrx")))
                .unwrap().unwrap();
            let cols = load_aligned(&std::path::Path::new(ex).join(format!("{data}.csv")), &eval)
                .unwrap_or_else(|e| panic!("{bundle}: {e}"));
            assert!(cols.iter().any(|(h, _)| h == "CMT"), "{bundle}");
            assert!(cols.iter().all(|(_, v)| v.len() == eval.rows.len()));
        }
        // OCC from the dataset matches the OCC the bundle already carries.
        let eval = crate::io::fitrx::read_predictions(&std::path::Path::new(&dir).join("warfarin_iov.fitrx")).unwrap().unwrap();
        let cols = load_aligned(&std::path::Path::new(ex).join("warfarin_iov.csv"), &eval).unwrap();
        let occ_ds = &cols.iter().find(|(h, _)| h == "OCC").unwrap().1;
        let occ_pred = &eval.extras.iter().find(|(h, _)| h == "OCC").unwrap().1;
        assert_eq!(occ_ds, occ_pred);
    }
}
