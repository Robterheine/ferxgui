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
