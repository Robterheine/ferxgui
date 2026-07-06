/// Prediction-level data loaded from `predictions.csv` inside a `.fitrx` bundle.
/// One row of `predictions.csv`.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PredRow {
    pub id:      String,
    pub time:    f64,
    pub dv:      f64,
    pub pred:    f64,
    pub ipred:   f64,
    pub cwres:   f64,
    pub iwres:   f64,
    /// Individual OFV contribution (EBE_OFV column).  Same for every
    /// observation of a given subject; NaN when unavailable.
    pub ebe_ofv: f64,
}

// ---------------------------------------------------------------------------
// Per-subject EBE / iOFV data  (ebes.csv)
// ---------------------------------------------------------------------------

/// One row of `ebes.csv` — per-subject empirical Bayes estimates and
/// individual OFV contribution.
#[derive(Debug, Clone)]
pub struct EbesRow {
    pub id:               String,
    pub ofv_contribution: f64,
    #[allow(dead_code)] pub n_obs: usize,
    #[allow(dead_code)] pub etas:  Vec<f64>,
}

/// Full per-subject dataset loaded from `ebes.csv`.
#[derive(Debug, Clone, Default)]
pub struct EbesData {
    pub rows:      Vec<EbesRow>,
    /// Total OFV (sum of contributions).
    pub total_ofv: f64,
    #[allow(dead_code)] pub eta_names: Vec<String>,
}

impl EbesData {
    /// Rows sorted by `ofv_contribution` descending (worst subject first).
    pub fn sorted_by_iofv(&self) -> Vec<&EbesRow> {
        let mut v: Vec<&EbesRow> = self.rows.iter().collect();
        v.sort_by(|a, b| b.ofv_contribution.partial_cmp(&a.ofv_contribution)
                          .unwrap_or(std::cmp::Ordering::Equal));
        v
    }
}

/// Pre-computed evaluation dataset — cached in `UiState` so the .fitrx is
/// not re-read on every frame.
#[derive(Debug, Clone, Default)]
pub struct EvalData {
    pub rows: Vec<PredRow>,
    /// Unique subject IDs in order of first appearance.
    pub subject_ids: Vec<String>,
}

impl EvalData {
    pub fn from_rows(rows: Vec<PredRow>) -> Self {
        let mut subject_ids: Vec<String> = Vec::new();
        for r in &rows {
            if !subject_ids.contains(&r.id) {
                subject_ids.push(r.id.clone());
            }
        }
        Self { rows, subject_ids }
    }

    /// Rows belonging to a given subject.
    #[allow(dead_code)]
    pub fn rows_for(&self, id: &str) -> Vec<&PredRow> {
        self.rows.iter().filter(|r| r.id == id).collect()
    }

    /// [min, max] range across both DV and PRED/IPRED for axis-matching.
    pub fn dv_pred_range(&self) -> [f64; 2] {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for r in &self.rows {
            for &v in &[r.dv, r.pred, r.ipred] {
                if v.is_finite() {
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
        }
        if lo.is_infinite() { [0.0, 1.0] } else { [lo, hi] }
    }

    #[allow(dead_code)]
    pub fn time_range(&self) -> [f64; 2] {
        let lo = self.rows.iter().filter_map(|r| r.time.is_finite().then_some(r.time)).fold(f64::INFINITY, f64::min);
        let hi = self.rows.iter().filter_map(|r| r.time.is_finite().then_some(r.time)).fold(f64::NEG_INFINITY, f64::max);
        if lo.is_infinite() { [0.0, 24.0] } else { [lo, hi] }
    }
}

// ---------------------------------------------------------------------------
// Per-subject conditional distribution (conddist.csv, SAEM only)
// ---------------------------------------------------------------------------

/// One row of `conddist.csv` — per-subject per-ETA conditional distribution
/// summary from FeRx's SAEM post-fit conditional-distribution pass (MCMC at
/// fixed population parameters). Only present when `conddist = true` was set
/// in `[fit_options]` on a SAEM fit.
#[derive(Debug, Clone)]
pub struct CondDistRow {
    pub id:        String,
    pub eta_name:  String,
    /// E[eta_i | y_i] — shrinkage-aware conditional mean.
    pub cond_mean: f64,
    /// SD[eta_i | y_i] — per-subject uncertainty.
    pub cond_sd:   f64,
    /// MAP of eta_i (the EBE) — for the Mode vs. Mean shrinkage check.
    pub cond_mode: f64,
}

/// Full conditional-distribution dataset loaded from `conddist.csv` in a
/// `.fitrx` bundle.
#[derive(Debug, Clone, Default)]
pub struct CondDistData {
    pub rows:        Vec<CondDistRow>,
    /// Unique ETA names in order of first appearance.
    pub eta_names:   Vec<String>,
    /// Unique subject IDs in order of first appearance.
    #[allow(dead_code)]
    pub subject_ids: Vec<String>,
}

impl CondDistData {
    /// Rows for a single ETA name.
    pub fn rows_for_eta(&self, eta: &str) -> Vec<&CondDistRow> {
        self.rows.iter().filter(|r| r.eta_name == eta).collect()
    }

    /// Distribution-based shrinkage for one ETA: `1 - SD(cond_mean) / sqrt(omega_jj)`.
    /// This is the shrinkage-unbiased analogue of the usual EBE-based shrinkage.
    /// `NaN` when `omega_jj` isn't positive or fewer than two subjects have a
    /// finite conditional mean.
    pub fn shrinkage_for_eta(&self, eta: &str, omega_jj: f64) -> f64 {
        let means: Vec<f64> = self.rows_for_eta(eta).iter()
            .map(|r| r.cond_mean).filter(|v| v.is_finite()).collect();
        if means.len() < 2 || omega_jj <= 0.0 { return f64::NAN; }
        let mean_of_means = means.iter().sum::<f64>() / means.len() as f64;
        let var = means.iter().map(|v| (v - mean_of_means).powi(2)).sum::<f64>()
            / (means.len() - 1) as f64;
        1.0 - var.sqrt() / omega_jj.sqrt()
    }
}

/// One row from a convergence trace CSV.
#[derive(Debug, Clone)]
pub struct TraceRow {
    pub iteration:      f64,
    pub ofv:            f64,
    /// Estimation method for this row ("focei", "saem", "gn", …).
    pub method:         String,
    /// Sub-phase label (empty = single-phase method).
    pub phase:          String,
    /// L2 gradient norm — finite for FOCE/FOCEI, NaN for SAEM/GN.
    pub grad_norm:      f64,
    /// Metropolis-Hastings acceptance rate — finite for SAEM only.
    pub mh_accept_rate: f64,
    /// Levenberg-Marquardt lambda — finite for GN only.
    pub lm_lambda:      f64,
}

/// Running-minimum (`cummin`) OFV over the FOCE/FOCEI rows only, leaving any
/// other method's rows (e.g. a preceding SAEM/GN stage in a method chain)
/// unchanged. Mirrors ferx-r's own `plot(fit)` default (`monotonic = TRUE`),
/// which filters out the OFV upticks from rejected line-search trial steps
/// that the raw per-evaluation trace otherwise includes.
pub fn monotonic_ofv(rows: &[TraceRow]) -> Vec<f64> {
    let mut ofv: Vec<f64> = rows.iter().map(|r| r.ofv).collect();
    let mut running_min = f64::INFINITY;
    for (i, r) in rows.iter().enumerate() {
        if r.method.starts_with("foce") {
            running_min = running_min.min(ofv[i]);
            ofv[i] = running_min;
        }
    }
    ofv
}

#[cfg(test)]
mod monotonic_ofv_tests {
    use super::{monotonic_ofv, TraceRow};

    fn row(method: &str, ofv: f64) -> TraceRow {
        TraceRow {
            iteration: 0.0, ofv, method: method.to_string(), phase: String::new(),
            grad_norm: f64::NAN, mh_accept_rate: f64::NAN, lm_lambda: f64::NAN,
        }
    }

    #[test]
    fn applies_running_minimum_to_focei_rows_only() {
        let rows = vec![
            row("focei", 100.0),
            row("focei", 95.0),
            row("focei", 98.0), // rejected trial step — should be smoothed to 95.0
            row("focei", 90.0),
        ];
        let ofv = monotonic_ofv(&rows);
        assert_eq!(ofv, vec![100.0, 95.0, 95.0, 90.0]);
    }

    #[test]
    fn leaves_non_foce_rows_untouched() {
        // A preceding SAEM stage in a method chain: cummin must not apply to
        // it, and must not be perturbed by it either (independent running
        // minimum per the foce/focei subset only).
        let rows = vec![
            row("saem", 200.0),
            row("saem", 150.0),
            row("focei", 100.0),
            row("focei", 105.0), // rejected — smoothed to 100.0
        ];
        let ofv = monotonic_ofv(&rows);
        assert_eq!(ofv, vec![200.0, 150.0, 100.0, 100.0]);
    }

    #[test]
    fn matches_foce_prefix_case_sensitively() {
        // Mirrors ferx-r's `grepl("^foce", method)` — matches "foce"/"focei"
        // by prefix, does not match other methods.
        let rows = vec![row("foce", 50.0), row("gn", 40.0), row("foce", 45.0)];
        let ofv = monotonic_ofv(&rows);
        // gn row (40.0) is untouched; the foce running min is 50 -> 45.
        assert_eq!(ofv, vec![50.0, 40.0, 45.0]);
    }
}

#[cfg(test)]
mod conddist_tests {
    use super::{CondDistData, CondDistRow};

    fn make(rows: Vec<(&str, &str, f64, f64, f64)>) -> CondDistData {
        let rows: Vec<CondDistRow> = rows.into_iter()
            .map(|(id, eta, mean, sd, mode)| CondDistRow {
                id: id.to_string(), eta_name: eta.to_string(),
                cond_mean: mean, cond_sd: sd, cond_mode: mode,
            })
            .collect();
        let eta_names: Vec<String> = {
            let mut v = Vec::new();
            for r in &rows { if !v.contains(&r.eta_name) { v.push(r.eta_name.clone()); } }
            v
        };
        let subject_ids: Vec<String> = {
            let mut v = Vec::new();
            for r in &rows { if !v.contains(&r.id) { v.push(r.id.clone()); } }
            v
        };
        CondDistData { rows, eta_names, subject_ids }
    }

    #[test]
    fn rows_for_eta_filters_correctly() {
        let cd = make(vec![
            ("S1", "ETA_CL", 0.1, 0.05, 0.08),
            ("S1", "ETA_V",  -0.2, 0.06, -0.15),
            ("S2", "ETA_CL", -0.3, 0.04, -0.25),
        ]);
        let cl = cd.rows_for_eta("ETA_CL");
        assert_eq!(cl.len(), 2);
        assert!(cl.iter().all(|r| r.eta_name == "ETA_CL"));
    }

    #[test]
    fn shrinkage_no_shrinkage_when_sd_matches_omega() {
        // Conditional means with sample SD == sqrt(omega_jj) imply zero shrinkage.
        let cd = make(vec![
            ("S1", "ETA_CL", -1.0, 0.1, -1.0),
            ("S2", "ETA_CL",  1.0, 0.1,  1.0),
        ]);
        // sample SD of [-1, 1] is sqrt(2); omega_jj = 2 -> sqrt(omega_jj) = sqrt(2).
        let s = cd.shrinkage_for_eta("ETA_CL", 2.0);
        assert!(s.abs() < 1e-9, "expected ~0 shrinkage, got {s}");
    }

    #[test]
    fn shrinkage_full_when_means_collapse_to_zero() {
        let cd = make(vec![
            ("S1", "ETA_CL", 0.0, 0.1, 0.0),
            ("S2", "ETA_CL", 0.0, 0.1, 0.0),
        ]);
        let s = cd.shrinkage_for_eta("ETA_CL", 1.0);
        assert!((s - 1.0).abs() < 1e-9, "expected ~100% shrinkage, got {s}");
    }

    #[test]
    fn shrinkage_nan_on_bad_inputs() {
        let cd = make(vec![("S1", "ETA_CL", 0.1, 0.05, 0.08)]); // only 1 subject
        assert!(cd.shrinkage_for_eta("ETA_CL", 1.0).is_nan());

        let cd2 = make(vec![
            ("S1", "ETA_CL", 0.1, 0.05, 0.08),
            ("S2", "ETA_CL", -0.1, 0.05, -0.08),
        ]);
        assert!(cd2.shrinkage_for_eta("ETA_CL", 0.0).is_nan()); // omega_jj not positive
        assert!(cd2.shrinkage_for_eta("ETA_CL", f64::NAN).is_nan());
    }
}
