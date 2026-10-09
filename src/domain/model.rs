use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use super::fit::FitSummary;

/// A `.ferx` file on disk, with extracted metadata.
#[derive(Debug, Clone)]
pub struct FerxModel {
    pub path: PathBuf,
    pub stem: String,
    /// Raw source text.
    pub source: String,
    /// Parameter names + initial values extracted from [parameters].
    pub params: ParsedParams,
    /// File creation/modification time as "YYYY-MM-DD HH:MM" for the audit trail.
    pub created_at: Option<String>,
    /// Dataset path declared in the model's own `[data]` block, if any
    /// (verbatim, relative to the model file's own directory) — `None` if
    /// the model has no `[data]` block.
    pub data_path: Option<String>,
}

/// Names and initial values parsed from a `.ferx` file.
#[derive(Debug, Clone, Default)]
pub struct ParsedParams {
    pub theta_names: Vec<String>,
    pub theta_init: Vec<f64>,
    pub theta_lower: Vec<f64>,
    pub theta_upper: Vec<f64>,
    pub omega_names: Vec<String>,
    pub omega_init: Vec<f64>,   // diagonal variances only
    pub sigma_names: Vec<String>,
    pub sigma_init: Vec<f64>,
    /// Names in `theta_names` declared as a level block (`theta NAME[COL, ...]`); the
    /// fit expands each into one theta per observed level (`NAME[STUDY=1,TIME=1]`).
    pub theta_level_blocks: Vec<String>,
    /// Priors declared inline with `prior(value, rse = X%)` on a theta / omega / sigma.
    pub priors: Vec<DeclaredPrior>,
    /// `from_fit = "path"` of a `[priors]` section, if present.
    pub priors_from_fit: Option<String>,
    /// First comment line or $PROBLEM-equivalent text (used as description).
    pub description: String,
}

/// A prior declared in the model file: `prior(value, rse = 25%)`.
#[derive(Debug, Clone, PartialEq)]
pub struct DeclaredPrior {
    pub name: String,
    pub value: f64,
    /// Relative standard error of the prior, in percent.
    pub rse_pct: f64,
}

impl ParsedParams {
    /// Initial value for a fitted theta name. A level-block theta
    /// (`PLACEBO[STUDY=1,TIME=1]`) reports the init of its block (`PLACEBO`).
    /// Falls back to position only when the model has no level blocks.
    pub fn theta_init_for(&self, fit_name: &str, idx: usize) -> f64 {
        let base = fit_name.split('[').next().unwrap_or(fit_name);
        if let Some(k) = self.theta_names.iter().position(|n| n == fit_name || n == base) {
            return self.theta_init.get(k).copied().unwrap_or(f64::NAN);
        }
        if self.theta_level_blocks.is_empty() {
            self.theta_init.get(idx).copied().unwrap_or(f64::NAN)
        } else {
            f64::NAN
        }
    }

    /// True if the model declares a level block (MBMA); ferx refuses SIR and the
    /// covariance recompute for these in 0.4.0.
    pub fn has_level_block(&self) -> bool {
        !self.theta_level_blocks.is_empty()
    }
}

/// Metadata stored in `model_meta.json`, keyed by model stem.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelMeta {
    #[serde(default)]
    pub starred: bool,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub status: ModelStatus,
    #[serde(default)]
    pub decision: ModelDecision,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: String,
    /// Stem of the model this was derived from.
    #[serde(default)]
    pub based_on: Option<String>,
    /// Whether this model is the current comparison reference (used for
    /// ΔOFV in the model list). At most one model should have this set at
    /// a time — enforced by the toggle logic, not by this type itself.
    #[serde(default)]
    pub is_reference: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModelStatus {
    #[default]
    Candidate,
    Base,
    Final,
}

impl ModelStatus {
    pub fn label(&self) -> &'static str {
        match self {
            ModelStatus::Base => "Base",
            ModelStatus::Candidate => "Candidate",
            ModelStatus::Final => "Final",
        }
    }
    pub fn all() -> &'static [ModelStatus] {
        &[ModelStatus::Base, ModelStatus::Candidate, ModelStatus::Final]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModelDecision {
    #[default]
    Include,
    Sensitivity,
    Exploratory,
    Rejected,
}

impl ModelDecision {
    pub fn label(&self) -> &'static str {
        match self {
            ModelDecision::Include => "Include",
            ModelDecision::Sensitivity => "Sensitivity",
            ModelDecision::Exploratory => "Exploratory",
            ModelDecision::Rejected => "Rejected",
        }
    }
    pub fn all() -> &'static [ModelDecision] {
        &[
            ModelDecision::Include,
            ModelDecision::Sensitivity,
            ModelDecision::Exploratory,
            ModelDecision::Rejected,
        ]
    }
}

/// A model entry in the model list — the `.ferx` file combined with its latest fit result.
#[derive(Debug, Clone)]
pub struct ModelEntry {
    pub model: FerxModel,
    /// Path to the `.fitrx` bundle alongside the model file (same stem).
    pub fitrx_path: Option<PathBuf>,
    /// Parsed fit summary from the `.fitrx` bundle.  None if no run yet.
    pub fit: Option<FitSummary>,
    /// Set when a `.fitrx` bundle exists but failed to parse (e.g. a ferx
    /// schema change this version of ferxgui doesn't yet handle) — distinct
    /// from `fit: None` meaning "never run", so the two aren't confused in
    /// the UI. `fit` is also `None` in this case.
    pub fit_parse_error: Option<String>,
    pub meta: ModelMeta,
    /// True when the `.ferx` mtime is newer than the `.fitrx` mtime.
    pub is_stale: bool,
}

#[allow(dead_code)]
impl ModelEntry {
    pub fn stem(&self) -> &str {
        &self.model.stem
    }

    pub fn description(&self) -> &str {
        if !self.meta.comment.is_empty() {
            &self.meta.comment
        } else {
            &self.model.params.description
        }
    }

    /// OFV from the fit, or NaN when not yet run.
    pub fn ofv(&self) -> f64 {
        self.fit.as_ref().map(|f| f.ofv).unwrap_or(f64::NAN)
    }

    /// ΔOFV relative to a reference model's OFV.
    /// Compared on the data half of the objective (see `FitSummary::ofv_cmp`).
    pub fn delta_ofv(&self, reference_ofv: f64) -> f64 {
        let ofv = self.fit.as_ref().map(|f| f.ofv_cmp()).unwrap_or(f64::NAN);
        if ofv.is_nan() || reference_ofv.is_nan() {
            f64::NAN
        } else {
            ofv - reference_ofv
        }
    }

    pub fn run_status(&self) -> RunStatus {
        if self.fit_parse_error.is_some() {
            return RunStatus::ParseError;
        }
        match &self.fit {
            None => RunStatus::NotRun,
            Some(f) if !f.converged => RunStatus::Failed,
            Some(f) if self.is_stale => RunStatus::Stale,
            _ => RunStatus::Converged,
        }
    }
}

/// Visual run status used for row colouring in the model list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunStatus {
    NotRun,
    Converged,
    /// A `.fitrx` bundle exists but ferxgui failed to parse it.
    ParseError,
    Failed,
    Stale,
}

// ---------------------------------------------------------------------------
// Model comparison
// ---------------------------------------------------------------------------

/// Outcome of a likelihood-ratio test between two fits.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// p below 0.05: the larger model fits significantly better.
    Better { p: f64 },
    NotBetter { p: f64 },
    /// An LRT does not apply, with the reason.
    NotApplicable(&'static str),
}

/// LRT verdict for `delta_ofv = OFV(model) - OFV(reference)`, where the reference is the simpler,
/// nested model. Valid only for nested models fitted to the same data, with df equal to the
/// difference in estimated parameters. The 0.05 cut-off therefore depends on df (3.84, 5.99,
/// 7.81, ...), not a fixed -3.84.
pub fn lrt_verdict(delta_ofv: f64, df: i64, nested: bool, same_data: bool) -> Verdict {
    if !nested { return Verdict::NotApplicable("nesting not declared"); }
    if !same_data { return Verdict::NotApplicable("data differ (or cannot be verified)"); }
    if df <= 0 { return Verdict::NotApplicable("the model has no additional parameters"); }
    if delta_ofv.is_nan() { return Verdict::NotApplicable("no OFV"); }
    let p = super::stats::chi2_sf(-delta_ofv, df as f64);
    if p < 0.05 { Verdict::Better { p } } else { Verdict::NotBetter { p } }
}

/// Differences of a fit against a reference fit.
#[derive(Debug, Clone)]
pub struct Comparison {
    pub delta_ofv: f64,
    pub delta_aic: f64,
    pub delta_bic: f64,
    /// Extra estimated parameters in the model relative to the reference.
    pub df: i64,
    pub same_data: bool,
}

impl Comparison {
    pub fn new(model: &FitSummary, reference: &FitSummary) -> Self {
        // Same data means the same file content and the same number of observations; a bundle
        // without hashes cannot prove it.
        let same_data = matches!((&model.data_hash, &reference.data_hash), (Some(a), Some(b)) if a == b)
            && model.n_obs == reference.n_obs;
        Self {
            delta_ofv: model.ofv_cmp() - reference.ofv_cmp(),
            delta_aic: model.aic - reference.aic,
            delta_bic: model.bic - reference.bic,
            df: model.n_parameters as i64 - reference.n_parameters as i64,
            same_data,
        }
    }

    pub fn verdict(&self, nested: bool) -> Verdict {
        lrt_verdict(self.delta_ofv, self.df, nested, self.same_data)
    }

    /// Hover text: always ΔAIC and ΔBIC; the LRT line only when nesting is declared.
    pub fn tooltip(&self, nested: bool) -> String {
        let mut t = format!("ΔOFV {:+.2}   ΔAIC {:+.2}   ΔBIC {:+.2}\nΔparameters {:+}", 
            self.delta_ofv, self.delta_aic, self.delta_bic, self.df);
        match self.verdict(nested) {
            Verdict::Better { p } => t.push_str(&format!("\nLRT: p = {p:.3} (χ², df {}): better", self.df)),
            Verdict::NotBetter { p } => t.push_str(&format!("\nLRT: p = {p:.3} (χ², df {}): not significantly better", self.df)),
            Verdict::NotApplicable(why) => t.push_str(&format!("\nNo LRT verdict: {why}")),
        }
        t
    }
}

#[cfg(test)]
mod comparison_tests {
    use super::*;

    #[test]
    fn lrt_requires_nested_and_same_data() {
        assert!(matches!(lrt_verdict(-10.0, 1, false, true), Verdict::NotApplicable(_)));
        assert!(matches!(lrt_verdict(-10.0, 1, true, false), Verdict::NotApplicable(_)));
        assert!(matches!(lrt_verdict(-10.0, 1, true, true), Verdict::Better { .. }));
    }

    #[test]
    fn lrt_df_is_parameter_difference() {
        // -4 is significant for one extra parameter (3.84) but not for three (7.81).
        assert!(matches!(lrt_verdict(-4.0, 1, true, true), Verdict::Better { .. }));
        assert!(matches!(lrt_verdict(-4.0, 3, true, true), Verdict::NotBetter { .. }));
        assert!(matches!(lrt_verdict(-9.0, 3, true, true), Verdict::Better { .. }));
        assert!(matches!(lrt_verdict(-9.0, 0, true, true), Verdict::NotApplicable(_)));
        if let Verdict::NotBetter { p } = lrt_verdict(-6.635, 3, true, true) {
            assert!((p - 0.0844878).abs() < 1e-5);
        } else { panic!() }
    }

    #[test]
    fn legacy_bundles_cannot_prove_same_data() {
        let a = FitSummary { n_obs: 10, ..Default::default() };
        assert!(!Comparison::new(&a, &a).same_data);
        let b = FitSummary { n_obs: 10, data_hash: Some("x".into()), ..Default::default() };
        assert!(Comparison::new(&b, &b).same_data);
    }
}
