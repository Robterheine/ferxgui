/// Domain types for data extracted via the ferx R package.
use serde::{Deserialize, Serialize};
use std::path::Path;

// ---------------------------------------------------------------------------
// Model inspection (ferx_model_inspect)
// ---------------------------------------------------------------------------

/// Parsed output of `ferx_model_inspect()` — model structure without fitting.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RModelInfo {
    #[serde(default)]
    pub model_type: String,
    #[serde(default)]
    pub theta_names: Vec<String>,
    #[serde(default)]
    pub iiv: Vec<String>,
    #[serde(default)]
    pub residual: String,
}

// ---------------------------------------------------------------------------
// VPC — all statistics are computed by the `vpc` R package (vpcdb = TRUE);
// these types mirror the tables it returns so Rust only has to render them.
// ---------------------------------------------------------------------------

/// Options the GUI sends to the VPC bridge script (serialized to a JSON file).
#[derive(Debug, Clone, Serialize)]
pub struct VpcConfig {
    pub model_path: String,
    pub data_path:  String,
    /// Existing `.fitrx` bundle to load (skips a refit). None → fit from scratch.
    pub fitrx_path: Option<String>,
    /// RDS cache for the simulated dataset; reused when only display options change.
    pub cache_path: String,
    pub n_sim: u32,
    pub seed:  u32,
    /// Prediction-interval bounds (outer percentiles), e.g. 0.05 / 0.95.
    pub pi_lo: f64,
    pub pi_hi: f64,
    /// Confidence-interval bounds for the bands, e.g. 0.05 / 0.95.
    pub ci_lo: f64,
    pub ci_hi: f64,
    /// Binning method: "jenks", "kmeans", "pretty", "quantile", "density",
    /// "time", "data", or "manual".
    pub bins_type: String,
    pub n_bins: u32,
    /// Explicit bin separators when `bins_type == "manual"`.
    pub manual_bins: Option<Vec<f64>>,
    pub log_y: bool,
    /// Smooth bands (connect bin midpoints) vs. rectangular per-bin boxes.
    /// Matches the `vpc` package `smooth` argument. Ignored by the stats step.
    pub smooth: bool,
    /// Overlay raw observed points (the package `show$obs_dv`). Display-only.
    pub show_points: bool,
    /// Band fill colour as a CSS hex string (e.g. `"#3388cc"`).
    /// Injected into the R ggplot theme; display-only for the native render.
    pub band_color: String,
    /// "continuous" (default) or "censored" — routes to `vpc()` / `vpc_cens()`.
    pub vpc_type: String,
    /// Lower limit of quantification for censored VPC.
    pub lloq: Option<f64>,
    /// Upper limit of quantification for censored VPC.
    pub uloq: Option<f64>,
    /// Perform prediction-corrected VPC (continuous only).
    pub pred_corr: bool,
    /// Lower bound for pcVPC normalisation; values below this are excluded.
    pub pred_corr_lower_bnd: f64,
    /// Stratification columns (empty = no stratification).
    pub stratify: Vec<String>,
    /// Facet direction for the R ggplot: "wrap" | "rows" | "columns".
    pub facet: String,

    // ---- Appearance theme (display-only; forwarded to new_vpc_theme) --------
    /// Observed-line colour as a CSS hex string.
    pub obs_color: String,
    /// Simulated prediction-interval band fill transparency (0–1).
    pub sim_pi_alpha: f64,
    /// Simulated median band fill transparency (0–1).
    pub sim_median_alpha: f64,
    /// Observed median line: ggplot linetype name + width.
    pub obs_median_linetype: String,
    pub obs_median_linewidth: f64,
    /// Observed 5th/95th line: ggplot linetype name + width.
    pub obs_ci_linetype: String,
    pub obs_ci_linewidth: f64,
    /// Bin-separator colour hex, or "" to hide them.
    pub bin_separators_color: String,
    /// LLOQ/ULOQ reference-line colour hex.
    pub loq_color: String,
}

/// One simulated-percentile band row from `vpcdb$vpc_dat`.
/// `qN.low/.med/.up` are the CI on the Nth simulated percentile across replicates.
/// Full row shape is retained; `strat`/`bin`/`bin_min`/`bin_max` feed later
/// stratification and hover phases.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct VpcBandRow {
    #[serde(default)] pub strat: String,
    #[serde(default)] pub bin:   i64,
    #[serde(rename = "q5.low")]  pub q5_low:  Option<f64>,
    #[serde(rename = "q5.med")]  pub q5_med:  Option<f64>,
    #[serde(rename = "q5.up")]   pub q5_up:   Option<f64>,
    #[serde(rename = "q50.low")] pub q50_low: Option<f64>,
    #[serde(rename = "q50.med")] pub q50_med: Option<f64>,
    #[serde(rename = "q50.up")]  pub q50_up:  Option<f64>,
    #[serde(rename = "q95.low")] pub q95_low: Option<f64>,
    #[serde(rename = "q95.med")] pub q95_med: Option<f64>,
    #[serde(rename = "q95.up")]  pub q95_up:  Option<f64>,
    #[serde(default)] pub bin_mid: Option<f64>,
    #[serde(default)] pub bin_min: Option<f64>,
    #[serde(default)] pub bin_max: Option<f64>,
}

/// One observed-percentile row from `vpcdb$aggr_obs`.
/// Full row shape retained; `strat`/`bin`/`bin_min`/`bin_max` feed later phases.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct VpcObsRow {
    #[serde(default)] pub strat:   String,
    #[serde(default)] pub bin:     i64,
    #[serde(default)] pub obs5:    Option<f64>,
    #[serde(default)] pub obs50:   Option<f64>,
    #[serde(default)] pub obs95:   Option<f64>,
    #[serde(default)] pub bin_mid: Option<f64>,
    #[serde(default)] pub bin_min: Option<f64>,
    #[serde(default)] pub bin_max: Option<f64>,
}

/// A raw observed data point, for the scatter overlay.
#[derive(Debug, Clone, Deserialize)]
pub struct VpcObsPoint {
    #[serde(default)] pub time: f64,
    pub dv: Option<f64>,
}

/// Full result from the VPC bridge: the package's computed tables plus the
/// raw observed points. All statistics come from the `vpc` R package.
#[derive(Debug, Clone, Deserialize)]
pub struct VpcResult {
    #[serde(default)] pub vpc_dat:     Vec<VpcBandRow>,
    #[serde(default)] pub aggr_obs:    Vec<VpcObsRow>,
    #[serde(default)] pub bins:        Vec<f64>,
    #[serde(default)] pub obs_points:  Vec<VpcObsPoint>,
    /// "continuous" or "censored" — tells the renderer which plot type to draw.
    #[serde(default)] pub vpc_mode:    String,
    /// Echoed LLOQ for the horizontal reference line on censored plots.
    #[serde(default)] pub lloq:        Option<f64>,
    /// Echoed ULOQ for the horizontal reference line on censored plots.
    #[serde(default)] pub uloq:        Option<f64>,
    /// Stratification column names echoed for panel labels.
    #[serde(default)] pub strat_names: Vec<String>,
    #[allow(dead_code)] #[serde(default)] pub pi_lo: f64,
    #[allow(dead_code)] #[serde(default)] pub pi_hi: f64,
    /// Non-fatal warnings from the R bridge (e.g. strat column not found).
    #[serde(default)] pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Init check (ferx_check_init)
// ---------------------------------------------------------------------------

/// Result of a `ferx_check_init()` pilot fit (5 iterations).
/// `ofv_start` being non-finite is the primary failure signal.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CheckInitResult {
    #[serde(default)]
    pub n_iter: usize,
    #[serde(default)]
    pub ofv_start: Option<f64>,
    #[serde(default)]
    pub ofv_end: Option<f64>,
    #[serde(default)]
    pub ofv_drop: Option<f64>,
    #[serde(default)]
    #[allow(dead_code)]
    pub converged: bool,
}

// ---------------------------------------------------------------------------
// Structured warnings (ferx >= 0.1.5)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// SIR results (ferx_sir)
// ---------------------------------------------------------------------------

/// One parameter's SIR 95% confidence interval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SirCi {
    pub name: String,
    pub lo:   f64,
    pub hi:   f64,
}

/// Full result returned by the `sir.R` background script.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SirResult {
    /// Effective sample size.  Closer to `n_resamples` = good coverage.
    pub sir_ess: f64,
    pub theta:   Vec<SirCi>,
    pub omega:   Vec<SirCi>,
    pub sigma:   Vec<SirCi>,

    // ── Resamples-derived fields (only present when keep_samples = true) ──

    /// Parameter names in correlation-matrix column order: theta, omega diagonal, sigma.
    pub corr_names:    Vec<String>,
    /// Dimension of the square correlation matrix.
    pub corr_dim:      usize,
    /// Row-major flattened N×N SIR empirical correlation matrix.
    pub corr_flat:     Vec<f64>,
    /// Per-parameter marginal SIR samples (for distribution histograms).
    pub param_samples: std::collections::HashMap<String, Vec<f64>>,
}

impl SirResult {
    /// JSON cache path: `{stem}.sir.json` alongside the `.fitrx` bundle.
    pub fn cache_path(fitrx_path: &Path) -> std::path::PathBuf {
        fitrx_path.with_extension("sir.json")
    }

    /// Persist to disk next to the `.fitrx` bundle.
    pub fn save(&self, fitrx_path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string(self)
            .map_err(std::io::Error::other)?;
        std::fs::write(Self::cache_path(fitrx_path), json)
    }

    /// Load from disk if the cache exists and is not older than the `.fitrx`
    /// (older = stale from a previous estimation run → discard).
    pub fn load_if_fresh(fitrx_path: &Path) -> Option<Self> {
        let cache = Self::cache_path(fitrx_path);
        if !cache.exists() { return None; }
        // Discard if the .fitrx was written after the SIR cache.
        let fitrx_mt = std::fs::metadata(fitrx_path).and_then(|m| m.modified()).ok();
        let cache_mt = std::fs::metadata(&cache).and_then(|m| m.modified()).ok();
        if let (Some(ft), Some(ct)) = (fitrx_mt, cache_mt) {
            if ft > ct { return None; }
        }
        let json = std::fs::read_to_string(&cache).ok()?;
        serde_json::from_str(&json).ok()
    }
}

// ---------------------------------------------------------------------------
// ETA-covariate correlation (fit$eta_cov, ferx-r >= 0.2.0)
// ---------------------------------------------------------------------------

/// One row from `ferx_eta_cov()` — Pearson r between an ETA and a dataset covariate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EtaCovRow {
    pub eta:       String,
    pub covariate: String,
    /// Pearson r; NaN when fewer than 3 finite pairs.
    #[serde(default = "nan")]
    pub r:         f64,
    /// Two-sided p-value; NaN when not computable.
    #[serde(default = "nan")]
    pub p_val:     f64,
    /// True when |r| ≥ 0.3.
    #[serde(default)]
    pub flag:      bool,
}

fn nan() -> f64 { f64::NAN }

/// Full result from the `eta_cov.R` background script.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EtaCovResult {
    #[serde(default)]
    pub rows: Vec<EtaCovRow>,
    /// True when `rows` is empty because the original dataset could no
    /// longer be read from its recorded path — distinct from a legitimate
    /// "too few subjects / no numeric covariates" empty result.
    #[serde(default)]
    pub data_unavailable: bool,
}

// ---------------------------------------------------------------------------
// Declared-covariate screen (ferx_cov_screen(fit), ferx-r >= 0.2.0)
// ---------------------------------------------------------------------------

/// One row from `ferx_cov_screen()` — association between an individual
/// parameter and a covariate declared in the model's `[covariates]` block,
/// measured two ways: against the raw individual parameter estimate (`ebe`)
/// and against its random effect (`eta`, a correlation *ratio* for
/// categorical covariates — not the random effect itself; the R function's
/// own naming collides with "ETA" elsewhere, so the GUI must not reuse the
/// bare column name in a label).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CovScreenRow {
    pub parameter: String,
    pub covariate: String,
    /// "continuous" or "categorical" (from `fit$covariate_types`).
    #[serde(default)]
    pub cov_type:  String,
    /// Association with the raw individual parameter estimate; NaN when not computable.
    #[serde(default = "nan")]
    pub ebe:       f64,
    /// Association with the parameter's random effect (Pearson r for
    /// continuous covariates, correlation ratio η ∈ [0,1] for categorical);
    /// NaN when not computable.
    #[serde(default = "nan")]
    pub eta:       f64,
}

/// Full result from the `cov_screen.R` background script.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CovScreenResult {
    #[serde(default)]
    pub rows: Vec<CovScreenRow>,
    /// True when the model declares no `[covariates]` block (`fit$covtab` is
    /// NULL) — the expected, common case for most models, not an error.
    #[serde(default)]
    pub no_covariates: bool,
    /// True when the model has no random effects at all (`fit$ebe_etas` is
    /// NULL) — the screen needs at least one ETA to associate against.
    #[serde(default)]
    pub no_etas: bool,
}

// ---------------------------------------------------------------------------
// Structured warnings (ferx >= 0.1.5)
// ---------------------------------------------------------------------------

/// One entry from `fit$warnings_structured` — severity-tagged warning with
/// category and remediation context.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct StructuredWarning {
    #[serde(default)]
    pub severity: String,       // "critical" | "warning" | "info"
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub source_method: String,
}

impl CheckInitResult {
    /// True when the start OFV is finite — the minimum bar for a healthy model.
    pub fn start_finite(&self) -> bool {
        self.ofv_start.map(|v| v.is_finite()).unwrap_or(false)
    }

    /// True when the OFV dropped over the pilot iterations (gradient pointing down).
    pub fn dropping(&self) -> bool {
        self.ofv_drop.map(|d| d > 0.0).unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed but faithful sample of the JSON the VPC bridge emits
    /// (captured from `vpc::vpc(vpcdb = TRUE)` on the warfarin model).
    const VPC_FIXTURE: &str = r#"{
      "vpc_dat": [
        {"strat":"1","bin":1,"q5.low":1.863602,"q5.med":2.651113,"q5.up":3.737594,
         "q50.low":5.65219,"q50.med":6.888663,"q50.up":8.447812,
         "q95.low":10.431911,"q95.med":11.884128,"q95.up":13.546657,
         "bin_mid":1.166667,"bin_min":0.5,"bin_max":4},
        {"strat":"1","bin":2,"q5.low":7.175848,"q5.med":9.071249,"q5.up":10.5999,
         "q50.low":10.418216,"q50.med":11.426507,"q50.up":12.211377,
         "q95.low":12.237225,"q95.med":13.11112,"q95.up":14.381597,
         "bin_mid":4,"bin_min":4,"bin_max":8}
      ],
      "aggr_obs": [
        {"strat":"1","bin":1,"obs5":2.753015,"obs50":7.0433,"obs95":11.46328,
         "bin_mid":1.166667,"bin_min":0.5,"bin_max":4}
      ],
      "bins": [0.5, 4, 8, 12, 24, 48, 72, 96, 120],
      "obs_points": [{"time":0.5,"dv":5.3653},{"time":1.0,"dv":null}],
      "pi_lo": 0.05,
      "pi_hi": 0.95
    }"#;

    #[test]
    fn vpc_result_parses_bridge_json() {
        let r: VpcResult = serde_json::from_str(VPC_FIXTURE).expect("parse VpcResult");

        assert_eq!(r.vpc_dat.len(), 2);
        assert_eq!(r.aggr_obs.len(), 1);
        assert_eq!(r.bins, vec![0.5, 4.0, 8.0, 12.0, 24.0, 48.0, 72.0, 96.0, 120.0]);
        assert_eq!(r.obs_points.len(), 2);
        assert_eq!(r.pi_lo, 0.05);
        assert_eq!(r.pi_hi, 0.95);

        // Dotted-key columns must map through the serde renames.
        let b0 = &r.vpc_dat[0];
        assert_eq!(b0.bin, 1);
        assert_eq!(b0.q50_med, Some(6.888663));
        assert_eq!(b0.q5_low,  Some(1.863602));
        assert_eq!(b0.q95_up,  Some(13.546657));
        assert_eq!(b0.bin_mid, Some(1.166667));

        // Observed percentiles.
        assert_eq!(r.aggr_obs[0].obs50, Some(7.0433));

        // R `na = "null"` must become None, not an error.
        assert_eq!(r.obs_points[1].dv, None);
        assert_eq!(r.obs_points[0].dv, Some(5.3653));
    }

    #[test]
    fn cov_screen_result_parses_bridge_json() {
        // A trimmed but faithful sample of what cov_screen.R emits when the
        // model declares [covariates] and ferx_cov_screen() finds candidates.
        const FIXTURE: &str = r#"{
          "rows": [
            {"parameter":"CL","covariate":"WT","cov_type":"continuous","ebe":0.412,"eta":0.398},
            {"parameter":"V","covariate":"SEX","cov_type":"categorical","ebe":0.255,"eta":0.221}
          ],
          "no_covariates": false,
          "no_etas": false
        }"#;
        let r: CovScreenResult = serde_json::from_str(FIXTURE).expect("parse CovScreenResult");
        assert_eq!(r.rows.len(), 2);
        assert!(!r.no_covariates);
        assert!(!r.no_etas);
        assert_eq!(r.rows[0].cov_type, "continuous");
        assert_eq!(r.rows[1].cov_type, "categorical");
        assert_eq!(r.rows[0].eta, 0.398);
    }

    #[test]
    fn cov_screen_result_distinguishes_empty_reasons() {
        let no_cov: CovScreenResult =
            serde_json::from_str(r#"{"rows":[],"no_covariates":true,"no_etas":false}"#)
                .expect("parse");
        assert!(no_cov.no_covariates);
        assert!(no_cov.rows.is_empty());

        let no_eta: CovScreenResult =
            serde_json::from_str(r#"{"rows":[],"no_covariates":false,"no_etas":true}"#)
                .expect("parse");
        assert!(no_eta.no_etas);

        // Legitimate "nothing cleared the threshold" case: both false, empty rows.
        let below_threshold: CovScreenResult =
            serde_json::from_str(r#"{"rows":[],"no_covariates":false,"no_etas":false}"#)
                .expect("parse");
        assert!(!below_threshold.no_covariates && !below_threshold.no_etas);
        assert!(below_threshold.rows.is_empty());
    }
}
