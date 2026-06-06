use std::path::PathBuf;
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// A record of a ferx invocation, persisted to `runs.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: String,
    pub model_stem: String,
    pub tool: String,          // always "ferx" for now
    #[serde(default)]
    pub method: Option<String>,
    pub status: JobStatus,
    /// ISO-8601 timestamp string.
    pub started: String,
    #[serde(default)]
    pub completed: Option<String>,
    #[serde(default)]
    pub duration_secs: Option<f64>,
    pub command: String,
    pub directory: PathBuf,
    /// Data file used for this run (stored so the VPC tab can reuse it).
    #[serde(default)]
    pub data_path: Option<PathBuf>,
    /// SHA-256 of key output files, keyed by filename.
    #[serde(default)]
    pub file_hashes: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn label(&self) -> &'static str {
        match self {
            JobStatus::Running    => "Running",
            JobStatus::Completed  => "OK",
            JobStatus::Failed     => "Failed",
            JobStatus::Cancelled  => "Cancelled",
        }
    }
}

/// A run waiting in the sequential execution queue.
#[derive(Debug, Clone)]
pub struct QueuedRun {
    pub stem: String,
    pub model_path: PathBuf,
    pub data_path: PathBuf,
    pub method: String,
    pub covariance: bool,
    /// Gradient method passed to ferx_fit(): "auto", "ad", or "fd".
    pub gradient: String,
    /// Optional JSON settings passthrough, e.g. `{"optimizer":"bobyqa"}`.
    pub settings: String,
    /// Thread count for per-subject parallelism (0 = auto / one per logical CPU).
    pub threads: u32,
    /// Whether to write an optimizer trace CSV alongside the .fitrx bundle.
    pub optimizer_trace: bool,
    /// Whether to extract sdtab / patab CSV files after the run completes.
    pub export_tables: bool,
    /// Whether to run SIR automatically after a successful fit.
    pub run_sir_after: bool,
}

/// State of an actively running subprocess, held in `RunState`.
#[derive(Debug)]
pub struct ActiveRun {
    pub record:        RunRecord,
    /// Wall-clock instant the run was launched (for elapsed-time display).
    pub started_at:    std::time::Instant,
    /// OS process ID — stored for potential future use (signals, reconnect).
    pub pid:           u32,
    /// Absolute path to the combined stdout/stderr log file on disk.
    pub log_path:      std::path::PathBuf,
    /// Path to the run manifest (`{app_dir}/running/{run_id}.runmfst`).
    pub manifest_path: std::path::PathBuf,
    /// Send a `CancelMode` to request graceful stop or hard kill.
    pub cancel_tx:     std::sync::mpsc::Sender<crate::workers::messages::CancelMode>,
    /// Whether to extract sdtab / patab CSV files after the run completes.
    pub export_tables: bool,
    /// Whether to run SIR automatically after the run completes.
    pub run_sir_after: bool,
}
