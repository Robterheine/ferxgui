use serde::{Deserialize, Serialize};

/// Summary of a completed FerX fit, parsed from `fit.json` inside a `.fitrx` bundle.
/// Fields are kept `Option` where the covariance step may not have run or the value
/// may be missing in older bundle versions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FitSummary {
    pub method: String,
    #[serde(default)]
    pub method_chain: Vec<String>,
    pub converged: bool,
    pub ofv: f64,
    pub aic: f64,
    pub bic: f64,
    pub n_obs: usize,
    pub n_subjects: usize,
    pub n_parameters: usize,
    pub n_iterations: usize,
    #[serde(default)]
    pub wall_time_secs: f64,

    // Parameter estimates (parallel vecs; same length as names below)
    #[serde(default)]
    pub theta: Vec<f64>,
    #[serde(default)]
    pub theta_names: Vec<String>,
    #[serde(default)]
    pub theta_lower: Vec<f64>,
    #[serde(default)]
    pub theta_upper: Vec<f64>,

    /// Flattened lower triangle of the OMEGA matrix (row-major).
    #[serde(default)]
    pub omega: Vec<f64>,
    /// Names of each ETA  (diagonal entries correspond to omega_names[i]).
    #[serde(default)]
    pub omega_names: Vec<String>,
    /// Dimension of the OMEGA matrix (n_eta).
    #[serde(default)]
    pub n_eta: usize,

    /// Flattened lower triangle of the KAPPA (IOV) matrix (row-major lower triangle).
    #[serde(default)]
    pub kappa: Vec<f64>,
    #[serde(default)]
    pub kappa_names: Vec<String>,
    /// Dimension of the KAPPA matrix (number of IOV random effects).
    #[serde(default)]
    pub n_kappa: usize,
    /// Standard errors for the diagonal kappa entries.
    #[serde(default)]
    pub se_kappa: Vec<f64>,
    /// Shrinkage per kappa, in percent (ferx writes a fraction; scaled when read).
    #[serde(default)]
    pub kappa_shrinkage: Vec<f64>,

    #[serde(default)]
    pub sigma: Vec<f64>,
    #[serde(default)]
    pub sigma_names: Vec<String>,

    // Standard errors (None when covariance step was skipped)
    #[serde(default)]
    pub se_theta: Vec<f64>,
    #[serde(default)]
    pub se_omega: Vec<f64>,
    #[serde(default)]
    pub se_sigma: Vec<f64>,

    /// Condition number of the covariance matrix (NaN when unavailable).
    #[serde(default = "nan")]
    pub cov_condition_number: f64,

    /// Whether the covariance step succeeded.
    #[serde(default)]
    pub covariance_ok: bool,

    // Shrinkage (% per ETA / per EPS)
    #[serde(default)]
    pub eta_shrinkage: Vec<f64>,
    #[serde(default)]
    pub eps_shrinkage: Vec<f64>,

    /// ETAbar: mean ETA per subject, one value per ETA.
    #[serde(default)]
    pub etabar: Vec<f64>,
    /// p-values for H₀: mean ETA = 0 (Wilcoxon/t-test), one per ETA.
    #[serde(default)]
    pub etabar_pvalue: Vec<f64>,

    /// ferx's own boundary verdict (`r_extras.estimate_near_boundary`). None when the
    /// bundle does not carry it (older ferx), which is not the same as "no".
    #[serde(default)]
    pub near_boundary: Option<bool>,
    /// `r_extras.stalled_at_init`: the optimiser never left the initial estimates.
    #[serde(default)]
    pub stalled_at_init: Option<bool>,
    /// `r_extras.max_abs_correlation` between estimated parameters.
    #[serde(default)]
    pub max_abs_corr: Option<f64>,

    /// SHA-256 of the model file the fit used (`model_hash` in fit.json).
    #[serde(default)]
    pub model_hash: Option<String>,
    /// SHA-256 of the dataset the fit used (`data_hash` in fit.json).
    #[serde(default)]
    pub data_hash: Option<String>,

    /// `theta.fixed` / `omega.fixed` from fit.json (empty when absent).
    #[serde(default)]
    pub theta_fixed: Vec<bool>,
    #[serde(default)]
    pub omega_fixed: Vec<bool>,

    /// Warnings emitted during the run (mirrors warnings.txt).
    #[serde(default)]
    pub warnings: Vec<String>,

    /// Path to the convergence trace CSV (may be absolute or relative to the .fitrx parent dir).
    #[serde(default)]
    pub trace_path: Option<String>,

    /// Parameter correlation matrix derived from the covariance matrix.
    /// Row-major N×N where N = cov_corr_n.  Empty when covariance step skipped.
    #[serde(default)]
    pub cov_corr_flat:  Vec<f64>,
    /// Dimension of the square correlation matrix.
    #[serde(default)]
    pub cov_corr_n:     usize,
    /// Parameter names in column order (theta → omega diagonal → sigma).
    #[serde(default)]
    pub cov_corr_names: Vec<String>,

    /// Pooled Durbin-Watson statistic for IWRES autocorrelation (ferx >= 0.1.5).
    /// Values < 1.5 or > 2.5 indicate autocorrelation worth investigating.
    #[serde(default)]
    pub dw_statistic: Option<f64>,

    /// Pooled lag-1 Pearson correlation of IWRES (ferx >= 0.1.5).
    #[serde(default)]
    pub iwres_lag1_r: Option<f64>,

    /// Structured warnings with severity, category, and source method (ferx >= 0.1.5).
    #[serde(default)]
    pub warnings_structured: Vec<crate::domain::StructuredWarning>,

    /// Per-ETA parameterisation type from `eta_param_info` in fit.json (ferx >= 0.1.5).
    /// Values: "log_normal", "additive", "normal", "logit", "custom".
    /// Empty when absent; callers should default to "log_normal".
    #[serde(default)]
    pub eta_param_types: Vec<String>,

    /// ferx version that produced the bundle (`ferx_version` in fit.json). None for
    /// bundles written before the key existed.
    #[serde(default)]
    pub ferx_version: Option<String>,

    /// `omega_is_diagonal` from fit.json (ferx >= 0.4.0). When false, `se_omega` is the
    /// packed lower triangle (column-major) instead of one SE per diagonal entry.
    #[serde(default)]
    pub omega_is_diagonal: Option<bool>,
    /// Same for `kappa_is_diagonal` / `se_kappa`.
    #[serde(default)]
    pub kappa_is_diagonal: Option<bool>,

    /// Data half of the objective (`ofv - ofv_prior`); equals `ofv` when no prior was
    /// declared. None when the bundle predates the split.
    #[serde(default)]
    pub ofv_data: Option<f64>,
    /// Prior penalty half of the objective; 0 when no prior was declared.
    #[serde(default)]
    pub ofv_prior: Option<f64>,
    /// One row per priored parameter (empty when no prior was declared).
    #[serde(default)]
    pub prior_summary: Vec<PriorRow>,

    /// Dataset path recorded in the bundle (`data_path` in fit.json).
    #[serde(default)]
    pub data_path: Option<String>,

    /// Estimated `block_sigma` correlations (ferx >= 0.4.0).
    #[serde(default)]
    pub residual_correlations: Vec<ResidualCorr>,
}

/// One priored parameter, as reported in `prior_summary` of fit.json.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PriorRow {
    pub name: String,
    pub prior_value: f64,
    pub estimate: f64,
    /// Signed distance between estimate and prior centre, in prior SDs.
    pub shift_in_prior_sds: f64,
    pub penalty: f64,
    pub family: String,
    pub prior_lower_95: f64,
    pub prior_upper_95: f64,
}

/// One estimated correlation between two sigma components (`block_sigma`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResidualCorr {
    /// 0-based sigma indices.
    pub sigma_i: usize,
    pub sigma_j: usize,
    pub rho: f64,
    pub fixed: bool,
    pub se: f64,
}

/// Index of lower-triangle entry (row >= col) in an n x n matrix packed column by
/// column: [(0,0), (1,0), ..., (n-1,0), (1,1), (2,1), ...].
pub fn packed_col_major_index(n: usize, row: usize, col: usize) -> usize {
    col * n - col * col.saturating_sub(1) / 2 + (row - col)
}

/// Parses the leading `major.minor.patch` of a ferx version string ("0.4.0.9000" -> (0,4,0)).
pub fn parse_version3(v: &str) -> Option<(u32, u32, u32)> {
    let mut it = v.trim().trim_start_matches('v').split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    let c = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    Some((a, b, c))
}

fn nan() -> f64 {
    f64::NAN
}

impl FitSummary {
    /// ferx's verdict that an estimate sits at a bound. `false` when the bundle has no verdict.
    pub fn has_boundary_hit(&self) -> bool {
        self.near_boundary == Some(true)
    }

    /// True when theta `i` cannot be negative by declaration (lower bound of zero or above; ferx packs
    /// these as log), so its
    /// confidence interval is built on the log scale.
    pub fn theta_is_positive(&self, i: usize) -> bool {
        self.theta_lower.get(i).is_some_and(|&l| l >= 0.0)
    }

    /// Number of within-subject consecutive residual pairs (n_obs - n_subjects): the n behind the
    /// lag-1 correlation. 0 when unknown.
    pub fn lag_pairs(&self) -> usize { self.n_obs.saturating_sub(self.n_subjects) }

    /// Approximate 95 % flag level for the lag-1 correlation: |r| > 2/√n. None when n is unknown.
    pub fn lag1_threshold(&self) -> Option<f64> {
        let n = self.lag_pairs();
        (n >= 4).then(|| 2.0 / (n as f64).sqrt())
    }

    /// True when the lag-1 correlation exceeds its sample-size-scaled threshold.
    pub fn lag1_flagged(&self) -> Option<bool> {
        Some(self.iwres_lag1_r?.abs() > self.lag1_threshold()?)
    }

    /// Whether theta `i` was declared FIX.
    pub fn is_theta_fixed(&self, i: usize) -> bool {
        self.theta_fixed.get(i).copied().unwrap_or(false)
    }

    /// True when the bundle records both hashes, so caches keyed on fit identity can be trusted.
    pub fn has_identity(&self) -> bool {
        self.model_hash.is_some() && self.data_hash.is_some()
    }

    /// Identity of this fit for cache keys: the model and data hashes plus a SHA-256 over the
    /// estimates (theta, omega, sigma, kappa). Standard errors are deliberately excluded:
    /// simulations do not depend on them, so a covariance recompute must not invalidate a cache.
    /// None for legacy bundles without hashes (caching is then disabled).
    pub fn estimates_fingerprint(&self) -> Option<String> {
        use sha2::{Digest, Sha256};
        let (m, d) = (self.model_hash.as_ref()?, self.data_hash.as_ref()?);
        let mut h = Sha256::new();
        h.update(m.as_bytes()); h.update(b"|"); h.update(d.as_bytes());
        for (tag, v) in [("t", &self.theta), ("o", &self.omega), ("s", &self.sigma), ("k", &self.kappa)] {
            h.update(tag.as_bytes());
            for x in v.iter() { h.update(x.to_bits().to_le_bytes()); }
        }
        h.update(self.ferx_version.as_deref().unwrap_or("").as_bytes());
        Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
    }

    /// RSE% for theta[i]:  |SE / estimate| × 100.
    #[allow(dead_code)]
    pub fn theta_rse(&self, i: usize) -> Option<f64> {
        let est = *self.theta.get(i)?;
        let se = *self.se_theta.get(i)?;
        if est == 0.0 {
            return None;
        }
        Some((se / est).abs() * 100.0)
    }

    /// RSE% for sigma[i].
    #[allow(dead_code)]
    pub fn sigma_rse(&self, i: usize) -> Option<f64> {
        let est = *self.sigma.get(i)?;
        let se = *self.se_sigma.get(i)?;
        if est == 0.0 {
            return None;
        }
        Some((se / est).abs() * 100.0)
    }

    /// Returns the (row, col) value from the omega lower-triangle vector.
    /// row >= col (lower triangle, 0-indexed).
    pub fn omega_value(&self, row: usize, col: usize) -> Option<f64> {
        if col > row {
            return self.omega_value(col, row); // symmetric
        }
        // index in flattened lower triangle: row*(row+1)/2 + col
        let idx = row * (row + 1) / 2 + col;
        self.omega.get(idx).copied()
    }

    /// Correlation between ETA row and ETA col derived from the omega matrix.
    pub fn omega_corr(&self, row: usize, col: usize) -> Option<f64> {
        if row == col {
            return Some(1.0);
        }
        let cov = self.omega_value(row, col)?;
        let var_r = self.omega_value(row, row)?;
        let var_c = self.omega_value(col, col)?;
        let denom = var_r.sqrt() * var_c.sqrt();
        if denom == 0.0 {
            None
        } else {
            Some(cov / denom)
        }
    }

    /// Returns the (row, col) value from the kappa lower-triangle vector.
    pub fn kappa_value(&self, row: usize, col: usize) -> Option<f64> {
        if col > row { return self.kappa_value(col, row); }
        let idx = row * (row + 1) / 2 + col;
        self.kappa.get(idx).copied()
    }

    /// Correlation between KAPPA row and KAPPA col derived from the kappa matrix.
    pub fn kappa_corr(&self, row: usize, col: usize) -> Option<f64> {
        if row == col { return Some(1.0); }
        let cov   = self.kappa_value(row, col)?;
        let var_r = self.kappa_value(row, row)?;
        let var_c = self.kappa_value(col, col)?;
        let denom = var_r.sqrt() * var_c.sqrt();
        if denom == 0.0 { None } else { Some(cov / denom) }
    }

    /// SE of the variance on the diagonal of a random-effect matrix of size `n`, given the
    /// stored SE vector. A diagonal fit stores one SE per diagonal entry; a block fit
    /// stores the packed lower triangle (column-major). Without the explicit flag the
    /// layout is inferred from the length.
    fn se_diag(se: &[f64], n: usize, is_diag: Option<bool>, i: usize) -> Option<f64> {
        let packed_len = n * (n + 1) / 2;
        let packed = match is_diag {
            Some(d) => !d && se.len() == packed_len,
            None => n > 1 && se.len() == packed_len,
        };
        if packed {
            se.get(packed_col_major_index(n, i, i)).copied()
        } else {
            se.get(i).copied()
        }
    }

    /// SE of omega diagonal entry `i` (layout-aware).
    pub fn se_omega_diag(&self, i: usize) -> Option<f64> {
        Self::se_diag(&self.se_omega, self.n_eta, self.omega_is_diagonal, i)
    }

    /// SEs of the omega diagonal, one per ETA (empty when no SEs were computed).
    pub fn se_omega_diag_vec(&self) -> Vec<f64> {
        if self.se_omega.is_empty() { return vec![]; }
        (0..self.n_eta).map(|i| self.se_omega_diag(i).unwrap_or(f64::NAN)).collect()
    }

    /// SE of an off-diagonal omega covariance (row > col).
    pub fn se_omega_offdiag(&self, row: usize, col: usize) -> Option<f64> {
        let (r, c) = if row >= col { (row, col) } else { (col, row) };
        let n = self.n_eta;
        if self.se_omega.len() != n * (n + 1) / 2 || self.omega_is_diagonal == Some(true) {
            return None;
        }
        self.se_omega.get(packed_col_major_index(n, r, c)).copied()
    }

    /// SE of kappa diagonal entry `i` (layout-aware).
    pub fn se_kappa_diag(&self, i: usize) -> Option<f64> {
        Self::se_diag(&self.se_kappa, self.n_kappa, self.kappa_is_diagonal, i)
    }

    /// SE of an off-diagonal kappa covariance (row > col).
    pub fn se_kappa_offdiag(&self, row: usize, col: usize) -> Option<f64> {
        let (r, c) = if row >= col { (row, col) } else { (col, row) };
        let n = self.n_kappa;
        if self.se_kappa.len() != n * (n + 1) / 2 || self.kappa_is_diagonal == Some(true) {
            return None;
        }
        self.se_kappa.get(packed_col_major_index(n, r, c)).copied()
    }

    /// OFV used for model-to-model deltas: the data half of the objective. A priored
    /// fit's `ofv` is the penalized total, which is not comparable with an unpenalized
    /// fit (AIC/BIC are computed from the data half for the same reason).
    pub fn ofv_cmp(&self) -> f64 {
        self.ofv_data.unwrap_or(self.ofv)
    }

    /// True when the fit carries an active prior penalty.
    pub fn has_prior(&self) -> bool {
        !self.prior_summary.is_empty() || self.ofv_prior.is_some_and(|p| p != 0.0)
    }

    /// True if the bundle was written by a ferx older than `min` (major, minor, patch).
    /// False when the version is unknown, so legacy bundles raise no notice.
    pub fn fitted_before(&self, min: (u32, u32, u32)) -> bool {
        self.ferx_version.as_deref().and_then(parse_version3).is_some_and(|v| v < min)
    }

    /// True if the condition number exceeds the conventional warning threshold of 1000.
    pub fn cn_high(&self) -> bool {
        self.cov_condition_number.is_finite() && self.cov_condition_number > 1000.0
    }
}

/// A single row in the parameter display table (covers THETA, diagonal OMEGA, SIGMA).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ParamRow {
    pub label: String,   // e.g. "TVCL", "ETA_CL", "EPS_PROP"
    pub kind: ParamKind,
    pub initial: f64,
    pub estimate: f64,
    pub se: f64,
    pub lower: f64,      // bound (NaN if none)
    pub upper: f64,      // bound (NaN if none)
    pub at_bound: bool,
    pub fixed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ParamKind {
    Theta,
    OmegaDiag,
    OmegaOffDiag,
    Kappa,
    Sigma,
}

#[allow(dead_code)]
impl ParamRow {
    /// Log₁₀ ratio of estimate to initial.  Used by the Init→Final track cell.
    pub fn log_ratio(&self) -> Option<f64> {
        if self.initial == 0.0 || self.initial.is_nan() || self.estimate.is_nan() {
            return None;
        }
        if (self.initial > 0.0) != (self.estimate > 0.0) {
            return None; // sign flip
        }
        Some((self.estimate / self.initial).abs().log10())
    }

    pub fn rse_pct(&self) -> Option<f64> {
        if self.estimate == 0.0 || self.se.is_nan() {
            return None;
        }
        Some((self.se / self.estimate).abs() * 100.0)
    }
}

#[cfg(test)]
mod lag_tests {
    use super::*;

    #[test]
    fn dw_lag_threshold_scales_with_n() {
        let f = FitSummary { n_obs: 110, n_subjects: 10, iwres_lag1_r: Some(0.19), ..Default::default() };
        assert_eq!(f.lag_pairs(), 100);
        assert!((f.lag1_threshold().unwrap() - 0.2).abs() < 1e-12);
        assert_eq!(f.lag1_flagged(), Some(false));
        let small = FitSummary { n_obs: 24, n_subjects: 4, iwres_lag1_r: Some(0.3), ..f.clone() };
        assert!((small.lag1_threshold().unwrap() - 2.0 / 20f64.sqrt()).abs() < 1e-12);
        assert_eq!(small.lag1_flagged(), Some(false)); // 0.3 < 0.447: not flagged with few pairs
        assert_eq!(FitSummary::default().lag1_flagged(), None);
    }
}

#[cfg(test)]
mod positive_theta_tests {
    use super::*;

    #[test]
    fn zero_lower_bound_is_log_packed_and_positive() {
        let f = FitSummary { theta_lower: vec![0.0, 0.001, -0.1, f64::NEG_INFINITY], ..Default::default() };
        assert!(f.theta_is_positive(0) && f.theta_is_positive(1));
        assert!(!f.theta_is_positive(2), "negative lower bound is identity-packed");
        assert!(!f.theta_is_positive(3), "unbounded theta may be negative");
        assert!(!f.theta_is_positive(9), "unknown bound stays on the natural scale");
    }
}
