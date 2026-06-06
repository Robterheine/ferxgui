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
    /// Parameter names + initial values extracted from [parameters] / [initial_values].
    pub params: ParsedParams,
    /// File creation/modification time as "YYYY-MM-DD HH:MM" for the audit trail.
    pub created_at: Option<String>,
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
    /// First comment line or $PROBLEM-equivalent text (used as description).
    pub description: String,
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
    pub fn delta_ofv(&self, reference_ofv: f64) -> f64 {
        let ofv = self.ofv();
        if ofv.is_nan() || reference_ofv.is_nan() {
            f64::NAN
        } else {
            ofv - reference_ofv
        }
    }

    pub fn run_status(&self) -> RunStatus {
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
    Failed,
    Stale,
}
