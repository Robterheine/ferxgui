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
