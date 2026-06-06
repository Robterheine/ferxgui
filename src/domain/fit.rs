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
    /// Shrinkage % per kappa.
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

    /// Which theta/sigma parameters are at their lower bound.
    #[serde(default)]
    pub at_lower_bound: Vec<bool>,

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
}

fn nan() -> f64 {
    f64::NAN
}

impl FitSummary {
    /// Whether any parameter is at its lower bound.
    pub fn has_boundary_hit(&self) -> bool {
        self.at_lower_bound.iter().any(|&b| b)
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
