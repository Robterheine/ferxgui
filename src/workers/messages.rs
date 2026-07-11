use std::path::PathBuf;
use crate::domain::{ModelEntry, RunRecord};

/// How to cancel a running ferx process.
#[derive(Debug)]
pub enum CancelMode {
    /// Ask the process to stop cleanly (SIGTERM on Unix — graceful).
    Graceful,
    /// Kill immediately (SIGKILL / TerminateProcess).
    Kill,
}

/// Every worker sends one of these variants back to the main thread.
/// The egui update() loop drains the channel with try_recv() — never blocks.
#[derive(Debug)]
pub enum WorkerMsg {
    /// Directory scan completed; full refreshed model list.
    ScanComplete(Vec<ModelEntry>),

    /// A line of stdout / stderr from the active ferx subprocess.
    RunLine(String),

    /// The ferx subprocess exited.
    RunFinished {
        exit_code: i32,
        record: Box<RunRecord>,
    },

    /// The ferx subprocess could not be spawned or was killed with an error.
    RunError(String),

    /// Result of the background ferx detection via the R package.
    /// `Some((rscript_path, ferx_version, r_version))` on success.
    FerxBinaryDetected(Option<(PathBuf, String, String)>),

    /// `ferx_model_inspect()` completed for the given model stem.
    RInspectComplete {
        stem: String,
        info: Box<crate::domain::RModelInfo>,
    },

    /// VPC computation completed (vpc package, vpcdb) — bands/lines ready.
    RVpcComplete {
        stem: String,
        data: Box<crate::domain::VpcResult>,
    },

    /// Result of the `vpc` package installation check for the status banner.
    /// `Ok(version)` when installed, `Err(_)` otherwise.
    VpcPkgStatus(Result<String, String>),

    /// An R-ggplot VPC PNG was rendered; carries the saved file path to open.
    VpcPlotExported { path: String },

    /// A new model file was created from a template; triggers a directory rescan.
    ModelCreated(String),

    /// GOF figure export completed successfully.
    GofExportComplete { path: String },
    /// GOF figure export failed.
    GofExportError { message: String },

    /// `ferx_sir()` completed for the given model stem.
    SirComplete {
        stem:   String,
        result: Box<crate::domain::SirResult>,
    },

    /// `ferx_check_init()` completed for the given model stem.
    RCheckInitComplete {
        stem: String,
        result: Box<crate::domain::CheckInitResult>,
    },

    /// Output tables (sdtab / patab) were extracted from a `.fitrx` bundle.
    TablesExported {
        stem:  String,
        paths: Vec<std::path::PathBuf>,
    },

    /// `ferx_eta_cov()` completed for the given model stem.
    EtaCovComplete {
        stem:   String,
        result: Box<crate::domain::EtaCovResult>,
    },

    /// `ferx_cov_screen()` completed for the given model stem.
    CovScreenComplete {
        stem:   String,
        result: Box<crate::domain::CovScreenResult>,
    },

    /// An R background task failed.
    RTaskError {
        /// Short context string, e.g. "inspect run001", "vpc run001", or "check_init run001".
        context: String,
        message: String,
    },

    /// Sim plot quantile computation completed.
    SimComplete { generation: u64, result: Box<crate::domain::SimPlotResult> },
    /// Sim plot computation failed.
    SimError { generation: u64, message: String },

    /// Simulate-tab `ferx_simulate()` run completed — the CSV is already
    /// written to disk; this only carries the status summary.
    SimRunComplete {
        stem:   String,
        result: Box<crate::domain::SimRunResult>,
    },
}
