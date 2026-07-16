use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use crate::domain::{ActiveRun, ModelEntry, QueuedRun, RunRecord};
use crate::io::persistence::{Bookmark, FerxBinarySource, Settings, Theme, app_dir,
                              load_bookmarks, load_model_meta, load_runs, load_settings};
use crate::workers::messages::WorkerMsg;

// ---------------------------------------------------------------------------
// Tab + pill enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Models,
    Files,
    Tree,
    Evaluation,
    Vpc,
    Uncertainty,
    Simulate,
    SimPlot,
    History,
}

#[allow(dead_code)]
impl Tab {
    pub const ALL: &'static [Tab] = &[
        Tab::Models,
        Tab::Files,
        Tab::Tree,
        Tab::Evaluation,
        Tab::Vpc,
        Tab::Uncertainty,
        Tab::Simulate,
        Tab::SimPlot,
        Tab::History,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Models => "Models",
            Tab::Files => "Files",
            Tab::Tree => "Tree",
            Tab::Evaluation => "Evaluation",
            Tab::Vpc => "VPC",
            Tab::Uncertainty => "SIR",
            Tab::Simulate => "Simulate",
            Tab::SimPlot => "Sim Plot",
            Tab::History => "History",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Tab::Models      => "Mdl",
            Tab::Files       => "Fil",
            Tab::Tree        => "Tre",
            Tab::Evaluation  => "Eval",
            Tab::Vpc         => "VPC",
            Tab::Uncertainty => "SIR",
            Tab::Simulate    => "Simu",
            Tab::SimPlot     => "Sim",
            Tab::History     => "Hist",
        }
    }

    pub fn shortcut_index(self) -> u8 {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0) as u8 + 1
    }
}

/// Sub-tabs in the Models tab right panel (workflow order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModelPill {
    #[default]
    Editor,
    Run,
    Output,
    Parameters,
    Info,
    Report,
}

impl ModelPill {
    pub const ALL: &'static [ModelPill] = &[
        ModelPill::Editor,
        ModelPill::Run,
        ModelPill::Output,
        ModelPill::Parameters,
        ModelPill::Info,
        ModelPill::Report,
    ];
    pub fn label(self) -> &'static str {
        match self {
            ModelPill::Editor     => "Editor",
            ModelPill::Run        => "Run",
            ModelPill::Output     => "Output",
            ModelPill::Parameters => "Parameters",
            ModelPill::Info       => "Info",
            ModelPill::Report     => "Report",
        }
    }
}

// ---------------------------------------------------------------------------
// Files tab types
// ---------------------------------------------------------------------------

/// One entry in the Files tab directory listing.
pub struct FilesEntry {
    pub name:     String,
    pub path:     std::path::PathBuf,
    pub is_dir:   bool,
    pub size:     u64,
    pub modified: Option<std::time::SystemTime>,
}

/// Which pane is active in the Files tab preview area.
#[derive(Default, PartialEq, Eq, Clone, Copy)]
pub enum FilesViewMode {
    #[default]
    Empty,
    Text,
    Table,
    Plot,
    Binary,
}

/// Sub-sections in the Evaluation tab (outer segmented control).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EvalSection {
    #[default]
    Gof,
    IndividualFits,
    OfvWaterfall,
    Convergence,
    EtaCov,
    ParamCorr,
    CondDist,
}

/// Which sub-view is active inside the Cond. Dist. (SAEM conditional
/// distribution) section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CondDistView {
    /// Histogram of the conditional mean across subjects, with a N(0, omega_jj)
    /// overlay and a distribution-based shrinkage annotation.
    #[default]
    Distributions,
    /// Per-subject conditional mean +/- SD, sorted by mean.
    Caterpillar,
    /// Scatter of conditional mode (EBE) vs. conditional mean, with the
    /// identity line — points pulled toward zero show EBE shrinkage bias.
    ModeVsMean,
}

/// Which of the two covariate-screening views is shown in the ETA-Cov
/// section. Both screen for candidate covariate effects, but from different
/// sources and with different rigor:
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EtaCovView {
    /// `fit$eta_cov` — an informal scan: Pearson r between each raw EBE and
    /// every numeric column in the original dataset. No covariate needs to
    /// be declared in the model.
    #[default]
    DatasetScan,
    /// `ferx_cov_screen(fit)` — a formal screen using the model's own
    /// declared `[covariates]` block, typed and aggregated exactly as the
    /// model would use them. Requires the model to declare covariates.
    DeclaredCovariates,
}

/// Which column the History table is sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HistorySortCol {
    #[default]
    Started,
    Model,
    Method,
    Ofv,
    Duration,
}

/// Which parameter set the Simulate tab's bridge script simulates at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SimBasis {
    /// The model file's initial estimates (`fit = NULL`) — prior predictive.
    #[default]
    Initial,
    /// The saved `.fitrx` bundle's fitted estimates — posterior predictive.
    Fitted,
    /// Parameter sets drawn from a multivariate normal around the ML
    /// estimate, using the fit's covariance matrix. Requires the fit to have
    /// been run with `covariance = TRUE`.
    AsymptoticUncertainty,
    /// Parameter sets resampled from the SIR tab's kept resamples. Requires a
    /// SIR run for this model with "Keep resamples" enabled.
    SirUncertainty,
}

/// Line style for VPC observed-percentile lines. Maps to an egui `LineStyle`
/// for the native render and to a ggplot `linetype` string for the R export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VpcLineStyle { Solid, Dashed, Dotted }

impl VpcLineStyle {
    pub const ALL: [VpcLineStyle; 3] = [Self::Solid, Self::Dashed, Self::Dotted];
    /// ggplot2 `linetype` name for the R export.
    pub fn r_name(self) -> &'static str {
        match self { Self::Solid => "solid", Self::Dashed => "dashed", Self::Dotted => "dotted" }
    }
    pub fn label(self) -> &'static str {
        match self { Self::Solid => "Solid", Self::Dashed => "Dashed", Self::Dotted => "Dotted" }
    }
}

/// Visual theme for the VPC plot. Mirrors the high-value elements of the `vpc`
/// package's `new_vpc_theme()`; applied live to the native egui render and
/// forwarded to the R ggplot export. Display-only — never triggers a re-simulate.
#[derive(Debug, Clone)]
pub struct VpcTheme {
    /// Transparency of the simulated prediction-interval band fill (0–1).
    pub sim_pi_alpha: f32,
    /// Transparency of the simulated median band fill (0–1).
    pub sim_median_alpha: f32,
    /// Observed median line width / style.
    pub obs_median_width: f32,
    pub obs_median_style: VpcLineStyle,
    /// Observed 5th/95th percentile line width / style.
    pub obs_ci_width: f32,
    pub obs_ci_style: VpcLineStyle,
    /// Radius of overlaid observed points.
    pub obs_point_size: f32,
    /// Draw vertical bin-edge separators, and their colour.
    pub bin_sep_show: bool,
    pub bin_sep_color: [f32; 3],
    /// Colour of the LLOQ/ULOQ reference line on censored VPCs.
    pub loq_color: [f32; 3],
}

impl Default for VpcTheme {
    fn default() -> Self {
        Self {
            sim_pi_alpha:     0.15,
            sim_median_alpha: 0.30,
            obs_median_width: 2.0,
            obs_median_style: VpcLineStyle::Solid,
            obs_ci_width:     1.0,
            obs_ci_style:     VpcLineStyle::Dashed,
            obs_point_size:   2.5,
            bin_sep_show:     true,
            bin_sep_color:    [0.53, 0.53, 0.53],
            loq_color:        [0.6, 0.0, 0.0],
        }
    }
}

/// User-tunable VPC options. Simulation fields (n_sim/seed) drive a re-simulate;
/// the rest are display options the `vpc` package recomputes cheaply from cache.
#[derive(Debug, Clone)]
pub struct VpcOpts {
    pub n_sim: u32,
    pub seed:  u32,
    pub pi_lo: f64,
    pub pi_hi: f64,
    pub ci_lo: f64,
    pub ci_hi: f64,
    /// Binning method passed to the vpc package.
    pub bins_type: String,
    pub n_bins: u32,
    /// Comma-separated bin separators; only used when `bins_type == "manual"`.
    pub manual_bins: String,
    pub log_y: bool,
    /// Smooth bands vs. rectangular per-bin boxes (vpc package `smooth`).
    pub smooth: bool,
    pub show_points: bool,
    /// Band fill color as linear RGB 0.0–1.0 (matches egui color picker).
    /// Default = #3388cc.
    pub band_color: [f32; 3],
    /// Observed percentile line color as linear RGB 0.0–1.0.
    /// Default = black.
    pub obs_color: [f32; 3],
    /// Show horizontal background grid lines.
    pub show_grid_h: bool,
    /// Show vertical background grid lines.
    pub show_grid_v: bool,
    /// "continuous" or "censored" (BLOQ).
    pub vpc_type: String,
    /// LLOQ text field (parsed to f64 when non-empty).
    pub lloq_str: String,
    /// ULOQ text field (parsed to f64 when non-empty).
    pub uloq_str: String,
    /// Prediction-corrected VPC (continuous only).
    pub pred_corr: bool,
    /// Lower bound for pcVPC normalisation.
    pub pred_corr_lower_bnd: f64,
    /// First stratification column name (empty = none).
    pub stratify1: String,
    /// Second stratification column name (empty = none).
    pub stratify2: String,
    /// Facet direction for the R ggplot export.
    pub facet: String,
    /// Visual theme (colours/alpha/linetypes/points). Display-only.
    pub theme: VpcTheme,
}

/// A computed VPC result paired with the (non-theme) options that produced
/// it. `VpcOpts`'s own contract is that every field but `.theme` requires a
/// fresh "Compute VPC" click to take effect (an R round-trip) — so the
/// native plot must always render from the options actually sent to R, not
/// whatever the panel has been changed to since, or axis labels/legend text
/// can describe data that was never computed. `.theme` is excluded from
/// this snapshot by design (documented `Display-only`) and is always
/// re-read live from the current options at render time.
#[derive(Debug, Clone)]
pub struct VpcRenderData {
    pub result: crate::domain::VpcResult,
    pub opts:   VpcOpts,
}

impl Default for VpcOpts {
    fn default() -> Self {
        Self {
            n_sim: 500,
            seed:  42,
            pi_lo: 0.05,
            pi_hi: 0.95,
            ci_lo: 0.05,
            ci_hi: 0.95,
            bins_type: "jenks".to_string(),
            n_bins: 8,
            manual_bins: String::new(),
            log_y: false,
            smooth: true,
            show_points: false,
            band_color:            [0.2, 0.533, 0.8],
            obs_color:             [0.0, 0.0, 0.0],
            show_grid_h:           true,
            show_grid_v:           true,
            vpc_type:              "continuous".to_string(),
            lloq_str:              String::new(),
            uloq_str:              String::new(),
            pred_corr:             false,
            pred_corr_lower_bnd:   0.0,
            stratify1:             String::new(),
            stratify2:             String::new(),
            facet:                 "wrap".to_string(),
            theme:                 VpcTheme::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// UI state
// ---------------------------------------------------------------------------

pub struct UiState {
    pub active_tab: Tab,
    pub active_model_pill: ModelPill,
    pub active_eval_section: EvalSection,
    /// Index into `WorkspaceState::models`.
    pub selected_model: Option<usize>,
    /// Index of the reference model for ΔOFV calculation.
    pub reference_model: Option<usize>,
    /// Model list filter text.
    pub model_filter: String,
    /// Model list status filter.
    pub model_status_filter: ModelStatusFilter,
    /// Sidebar collapsed to icon-only.
    pub sidebar_collapsed: bool,
    /// Toast / status bar message (cleared after a timeout).
    pub status_message: String,

    // ---- Quit guard ----
    /// True while the "Unsaved changes" quit-confirmation dialog is showing.
    pub quit_unsaved_dialog: bool,
    /// Set once the user has confirmed quitting despite unsaved changes, so
    /// the re-sent close command isn't intercepted a second time.
    pub quit_confirmed: bool,

    // ---- Editor pill ----
    /// Text currently in the editor.  Reloaded whenever selected_model changes.
    pub editor_buffer: String,
    /// True when `editor_buffer` differs from the file on disk.
    pub editor_dirty: bool,
    /// Which model stem is currently loaded in the editor (used to detect selection change).
    pub editor_loaded_stem: Option<String>,
    /// Cached syntax-highlighted layout job: (text_snapshot, dark_mode, job).
    /// Recomputed only when the buffer or theme changes.
    pub editor_layout_cache: Option<(String, bool, egui::text::LayoutJob)>,

    // ---- Run pill ----
    pub run_method: String,
    pub run_covariance: bool,
    pub run_threads: u32,
    pub run_extra_args: String,
    pub run_data_path: Option<std::path::PathBuf>,
    /// Gradient method for ferx_fit(): "auto", "ad", or "fd".
    pub run_gradient: String,
    /// (stem, message) from the most recent failed run *launch* (before any
    /// R process even started — e.g. the embedded run script couldn't be
    /// written, or the OS refused to spawn the process). Without this, such
    /// a failure was invisible: `active_run` never gets set, so the Run
    /// popup's "auto-open on new run" never fires, leaving no visible sign
    /// the click did anything at all beyond the tiny status bar. Keyed by
    /// stem (rather than shown unconditionally) so it only appears on the
    /// Run pill of the model it actually happened for.
    pub run_launch_error: Option<(String, String)>,
    /// (stem, message) from the last failed "Save output tables" export
    /// (runs as an opt-in background step after a run completes). Has zero
    /// in-flight indicator even on success, so a failure was completely
    /// invisible beyond the tiny status bar — shown as a line in the Run
    /// popup instead, since that's what the user is watching right after
    /// the run that triggered it finishes.
    pub export_tables_error: Option<(String, String)>,
    // ---- Tree tab ----
    /// Pan offset in logical (pre-zoom) canvas pixels.
    pub tree_pan: egui::Vec2,
    /// Zoom factor (1.0 = fit-to-window).
    pub tree_zoom: f32,
    /// Stem of the currently hovered node (drives the info panel).
    pub tree_hovered: Option<String>,

    // ---- SIR tab ----
    pub sir_n_samples:    u32,
    pub sir_n_resamples:  u32,
    pub sir_seed:         u32,
    /// Keep resamples to enable correlation heatmap + distribution histograms.
    pub sir_keep_samples: bool,
    /// Active section: 0 = CI comparison, 1 = Correlations, 2 = Distributions.
    pub sir_view_idx:     usize,
    /// Currently displayed parameter in the distribution histogram.
    pub sir_selected_param: String,
    /// Write optimizer trace CSV (enables convergence trace viewer).
    pub run_optimizer_trace: bool,
    /// Extract sdtab / patab CSV files next to the .fitrx after run completes.
    pub run_export_tables: bool,
    // ---- New Model dialog ----
    pub new_model_dialog: bool,
    pub new_model_template: String,
    pub new_model_stem: String,

    // ---- Evaluation tab ----
    /// Which model's predictions are currently cached.
    pub eval_loaded_stem: Option<String>,
    /// The `.fitrx` file's mtime at the time it was last loaded — re-fitting
    /// a model keeps the same stem (and `.fitrx` filename), so stem alone
    /// can't tell a fresh fit apart from the stale, previously-loaded one;
    /// without this, GOF/Individual-Fits/etc. kept showing an old (e.g.
    /// incorrectly coded) model's results after re-running it correctly.
    pub eval_loaded_fitrx_mtime: Option<std::time::SystemTime>,
    /// Lazily-loaded prediction rows from `predictions.csv`.
    pub eval_data: Option<crate::domain::EvalData>,
    /// Lazily-loaded per-subject EBE / iOFV data from `ebes.csv`.
    pub eval_ebes: Option<crate::domain::EbesData>,
    /// Index into `eval_data.subject_ids` for the Individual Fits view.
    pub eval_subject_idx: usize,
    /// Whether the DV/PRED axes use log scale.
    pub eval_log_scale: bool,
    /// Convergence tab: show the running-minimum OFV for FOCE/FOCEI rows
    /// instead of the raw per-evaluation trace (which includes rejected
    /// line-search trial steps). Mirrors ferx-r's own `plot(fit)` default.
    pub eval_monotonic_ofv: bool,
    /// Number of subjects shown per page in Individual Fits (1–9).
    pub eval_subjects_per_page: usize,
    /// Column name used as X-axis for the first CWRES scatter panel.
    pub eval_cwres_x_col: String,
    /// Column name used as X-axis for the second CWRES scatter panel.
    pub eval_cwres_x_col_2: String,
    // ---- GOF export ----
    pub eval_export_dialog: bool,
    /// True while a background GOF-export Rscript call is in flight; guards
    /// against a second export racing the first on the shared temp CSV.
    pub eval_exporting: bool,
    /// "pdf" | "png300" | "png600" | "svg"
    pub eval_export_format: String,
    /// Figure width in mm (84 = single column, 174 = double column).
    pub eval_export_width_mm: u32,
    pub eval_export_loess: bool,
    pub eval_export_ci_lines: bool,

    // ---- Evaluation tab (Cond. Dist. section) ----
    /// Lazily-loaded per-subject per-ETA conditional distribution data from
    /// `conddist.csv`. `None` when absent (older bundle, non-SAEM fit, or
    /// `conddist` not enabled) — the section shows a hint instead.
    pub eval_conddist: Option<crate::domain::CondDistData>,
    /// Which sub-view (Distributions / Caterpillar / Mode vs Mean) is shown.
    pub eval_conddist_view: CondDistView,
    /// Index into `eval_conddist.eta_names` for the currently displayed ETA.
    pub eval_conddist_eta_idx: usize,

    // ---- Evaluation tab (ETA-Cov section) ----
    /// Which of the two covariate-screening views (Dataset Scan / Declared
    /// Covariates) is shown.
    pub eval_eta_cov_view: EtaCovView,

    // ---- Run popup ----
    /// Whether the floating run-output window is visible.
    pub run_popup_open: bool,
    pub about_open: bool,
    /// Whether the floating Settings window is visible.
    pub settings_open: bool,
    /// ID of the run that last auto-opened the popup (prevents re-opening after dismiss for same run).
    pub run_popup_last_run_id: Option<String>,

    // ---- SIR popup ----
    /// Whether the floating SIR-progress window is visible.
    pub sir_popup_open: bool,
    /// Stem of the model that last triggered the SIR popup (prevents re-opening after dismiss).
    pub sir_popup_last_stem: Option<String>,

    // ---- Post-fit actions ----
    /// Run SIR automatically after a successful fit (uses the current SIR tab settings).
    pub run_sir_after_fit: bool,

    // ---- Tree PNG export ----
    /// True for one frame after "Export PNG" is clicked; triggers screenshot request.
    pub tree_export_pending: bool,
    /// Screenshot requested; waiting for the image to arrive next frame.
    pub tree_export_awaiting: bool,
    /// Canvas rect (logical pts) saved each frame for crop calculation.
    pub tree_canvas_rect: egui::Rect,

    // ---- Context-menu dialogs ----
    /// Directory awaiting a name-and-confirm bookmark dialog (path + draft label).
    pub pending_bookmark: Option<(std::path::PathBuf, String)>,
    /// True once the bookmark dialog's text field has been auto-focused for
    /// the current time it's open — prevents re-claiming focus every frame
    /// (which would otherwise block Tab-ing to the dialog's buttons).
    pub bookmark_dialog_focused: bool,
    /// Model awaiting a duplicate-rename dialog (index into workspace.models).
    pub pending_duplicate: Option<usize>,
    /// Same one-shot-focus purpose as `bookmark_dialog_focused`, for the
    /// duplicate dialog's text field.
    pub duplicate_dialog_focused: bool,
    /// Text buffer for the new stem name in the duplicate dialog.
    pub duplicate_stem_buf: String,
    /// Text buffer for the new model's description in the duplicate dialog —
    /// pre-filled with the source model's current description, editable.
    pub duplicate_description_buf: String,
    /// Whether "Set as child" checkbox is checked in the duplicate dialog.
    pub duplicate_set_as_child: bool,
    /// Model awaiting a delete-confirmation dialog.
    pub pending_delete: Option<usize>,
    // ---- Compare dialog ----
    /// Two model stems open for side-by-side parameter comparison.
    pub compare_a: Option<String>,
    pub compare_b: Option<String>,
    /// GOF prediction data for the compare dialog's GOF-plot section, cached
    /// per the `(compare_a, compare_b)` pair so each `.fitrx` is only read
    /// once per pairing rather than on every frame the dialog is open.
    pub compare_gof_a: Option<crate::domain::EvalData>,
    pub compare_gof_b: Option<crate::domain::EvalData>,
    pub compare_gof_loaded_for: Option<(String, String)>,
    /// "Compare Models…" picker — an explicit, discoverable entry point into
    /// the compare dialog, alongside the existing right-click "Compare
    /// with…" row action.
    pub compare_picker_open: bool,
    pub compare_picker_a: Option<String>,
    pub compare_picker_b: Option<String>,

    // ---- VPC tab ----
    /// Data CSV used for VPC simulation (persists across model selections).
    pub vpc_data_path: Option<PathBuf>,
    /// Current VPC display/simulation options.
    pub vpc_opts: VpcOpts,
    /// `vpc` package install check: None = not checked, Some(Ok(version)) /
    /// Some(Err(_)) once known. Drives the status banner.
    pub vpc_pkg_status: Option<Result<String, String>>,
    /// True while the package check is in flight (avoids respawning it).
    pub vpc_pkg_checking: bool,
    /// True while an R-ggplot PNG export is in flight.
    pub vpc_exporting: bool,
    /// Editable R script for the ggplot export (lazy-initialised from the
    /// embedded default). Empty string = not yet initialised.
    pub vpc_script: String,
    /// Whether the R-script editor popup is open.
    pub vpc_script_open: bool,
    /// Column names from the selected data CSV, for the stratification pickers.
    pub vpc_data_cols: Vec<String>,

    // ---- Simulate tab ----
    /// Data CSV used for the simulation (persists across model selections).
    pub simrun_data_path: Option<PathBuf>,
    /// "Initial estimates" (prior predictive) or "Fitted estimates" (posterior
    /// predictive, requires a `.fitrx` bundle).
    pub simrun_basis: SimBasis,
    pub simrun_n_sim: u32,
    pub simrun_seed:  u32,
    /// Output CSV path, editable. Empty = auto-derive alongside the model.
    pub simrun_out_path: String,
    /// Number of parameter draws — used only when `simrun_basis` is an
    /// uncertainty mode.
    pub simrun_n_draws: u32,
    /// Eta/epsilon replicates per parameter draw — used only when
    /// `simrun_basis` is an uncertainty mode.
    pub simrun_n_sim_per_draw: u32,

    // ---- History tab ----
    pub history_filter: String,
    /// Index into the *sorted+filtered* view (not into run_history directly).
    pub history_selected: Option<usize>,
    pub history_sort_col: HistorySortCol,
    pub history_sort_asc: bool,

    // ---- Files tab ----
    /// Currently displayed directory (may differ from working_directory if user drilled in).
    pub files_cwd:            Option<PathBuf>,
    /// Back-navigation stack (pushed on drill-down, popped on ←).
    pub files_back_stack:     Vec<PathBuf>,
    /// Cached directory listing; rebuilt whenever files_cwd changes.
    pub files_entries:        Vec<FilesEntry>,
    /// The directory path the current entries cache was built from.
    pub files_entries_dir:    Option<PathBuf>,
    /// Active extension filter pills (empty = All).
    pub files_active_exts:    std::collections::HashSet<String>,
    /// Free-text extension override field.
    pub files_ext_input:      String,
    /// Currently selected (previewed) file path.
    pub files_selected:       Option<PathBuf>,
    /// A file the user clicked while the current file has unsaved edits —
    /// deferred until they resolve the "Unsaved changes" dialog.
    pub files_pending_nav:    Option<PathBuf>,
    /// Which preview pane is visible.
    pub files_view_mode:      FilesViewMode,
    // Text view
    pub files_text:           String,
    pub files_text_dirty:     bool,
    pub files_text_is_ferx:   bool,
    // CSV / table view
    pub files_csv_headers:    Vec<String>,
    pub files_csv_rows:       Vec<Vec<String>>,
    pub files_csv_edit_mode:  bool,
    pub files_csv_dirty:      bool,
    /// Cell currently open for in-place editing (row, col).
    pub files_csv_editing:    Option<(usize, usize)>,
    /// Scratch buffer for the cell editor.
    pub files_csv_edit_buf:   String,
    // Plot view
    pub files_plot_x_col:     String,
    pub files_plot_y_col:     String,
    pub files_plot_color_col: String,
    pub files_plot_unity:     bool,
    pub files_plot_loess:     bool,
    pub files_plot_log_x:     bool,
    pub files_plot_log_y:     bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModelStatusFilter {
    #[default]
    All,
    Completed,
    Failed,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            active_tab: Tab::Models,
            active_model_pill: ModelPill::Editor,
            active_eval_section: EvalSection::Gof,
            selected_model: None,
            reference_model: None,
            model_filter: String::new(),
            model_status_filter: ModelStatusFilter::All,
            sidebar_collapsed: false,
            status_message: String::new(),
            quit_unsaved_dialog: false,
            quit_confirmed: false,
            editor_buffer: String::new(),
            editor_dirty: false,
            editor_loaded_stem: None,
            editor_layout_cache: None,
            run_method: "focei".to_string(),
            run_covariance: true,
            run_threads: 0,
            run_extra_args: String::new(),
            run_data_path: None,
            run_gradient: "auto".to_string(),
            run_launch_error: None,
            export_tables_error: None,
            run_optimizer_trace: true,
            tree_pan:     egui::Vec2::ZERO,
            tree_zoom:    1.0,
            tree_hovered: None,
            sir_n_samples:    1000,
            sir_n_resamples:  250,
            sir_seed:         42,
            sir_keep_samples: true,
            sir_view_idx:     0,
            sir_selected_param: String::new(),
            new_model_dialog: false,
            new_model_template: "1cpt_oral".to_string(),
            new_model_stem: String::new(),
            eval_loaded_stem: None,
            eval_loaded_fitrx_mtime: None,
            eval_data: None,
            eval_ebes: None,
            eval_subject_idx: 0,
            eval_log_scale: false,
            eval_monotonic_ofv: true,
            eval_subjects_per_page: 9,
            eval_cwres_x_col:    "TIME".to_string(),
            eval_cwres_x_col_2:  "PRED".to_string(),
            eval_export_dialog:  false,
            eval_exporting:      false,
            eval_export_format:  "pdf".to_string(),
            eval_export_width_mm: 174,
            eval_export_loess:   true,
            eval_export_ci_lines: true,
            run_export_tables:      false,
            eval_conddist: None,
            eval_conddist_view: CondDistView::default(),
            eval_conddist_eta_idx: 0,
            eval_eta_cov_view: EtaCovView::default(),
            run_popup_open:        false,
            about_open:            false,
            settings_open:         false,
            run_popup_last_run_id: None,
            sir_popup_open:        false,
            sir_popup_last_stem:   None,
            run_sir_after_fit:     false,
            tree_export_pending:  false,
            tree_export_awaiting: false,
            tree_canvas_rect:     egui::Rect::NOTHING,
            pending_bookmark: None,
            bookmark_dialog_focused: false,
            pending_duplicate: None,
            duplicate_dialog_focused: false,
            duplicate_stem_buf: String::new(),
            duplicate_description_buf: String::new(),
            duplicate_set_as_child: true,
            pending_delete: None,
            compare_a: None,
            compare_b: None,
            compare_gof_a: None,
            compare_gof_b: None,
            compare_gof_loaded_for: None,
            compare_picker_open: false,
            compare_picker_a: None,
            compare_picker_b: None,
            vpc_data_path: None,
            vpc_opts: VpcOpts::default(),
            vpc_pkg_status: None,
            vpc_pkg_checking: false,
            vpc_exporting: false,
            vpc_script: String::new(),
            vpc_script_open: false,
            vpc_data_cols: vec![],
            simrun_data_path: None,
            simrun_basis: SimBasis::default(),
            simrun_n_sim: 500,
            simrun_seed:  42,
            simrun_out_path: String::new(),
            simrun_n_draws: 100,
            simrun_n_sim_per_draw: 1,
            history_filter: String::new(),
            history_selected: None,
            history_sort_col: HistorySortCol::Started,
            history_sort_asc: false, // newest first by default
            files_cwd:            None,
            files_back_stack:     Vec::new(),
            files_entries:        Vec::new(),
            files_entries_dir:    None,
            files_active_exts:    std::collections::HashSet::new(),
            files_ext_input:      String::new(),
            files_selected:       None,
            files_pending_nav:    None,
            files_view_mode:      FilesViewMode::Empty,
            files_text:           String::new(),
            files_text_dirty:     false,
            files_text_is_ferx:   false,
            files_csv_headers:    Vec::new(),
            files_csv_rows:       Vec::new(),
            files_csv_edit_mode:  false,
            files_csv_dirty:      false,
            files_csv_editing:    None,
            files_csv_edit_buf:   String::new(),
            files_plot_x_col:     String::new(),
            files_plot_y_col:     String::new(),
            files_plot_color_col: String::new(),
            files_plot_unity:     false,
            files_plot_loess:     false,
            files_plot_log_x:     false,
            files_plot_log_y:     false,
        }
    }
}

// ---------------------------------------------------------------------------
// Workspace state
// ---------------------------------------------------------------------------

pub struct WorkspaceState {
    pub directory: Option<PathBuf>,
    pub models: Vec<ModelEntry>,
    pub bookmarks: Vec<Bookmark>,
    pub settings: Settings,
    /// App-level data directory (`~/.ferxgui/`).
    pub app_dir: Option<PathBuf>,
    /// Warnings collected during startup (e.g. corrupt settings file). Displayed once then cleared.
    pub startup_warnings: Vec<String>,
    /// Scanning in progress flag.
    pub scanning: bool,
    /// How the ferx binary was located this session (not persisted).
    pub ferx_binary_source: FerxBinarySource,
    /// Detected ferx package version (e.g. "0.1.3"), if available.
    pub ferx_version: Option<String>,
    pub r_version: Option<String>,
    /// Cached `ferx_model_inspect()` results keyed by model stem.
    pub r_model_infos: HashMap<String, crate::domain::RModelInfo>,
    /// Stems for which an R inspect is currently in flight.
    pub r_inspecting: HashSet<String>,
    /// Stems whose inspect failed — not auto-retried (avoids respawn storms).
    pub r_inspect_failed: HashSet<String>,
    /// VPC simulation results keyed by model stem, paired with the options
    /// that actually produced them (see `VpcRenderData`).
    pub vpc_data: HashMap<String, VpcRenderData>,
    /// Stems for which a VPC computation is currently in flight.
    pub vpc_computing: HashSet<String>,
    /// Options snapshot taken when a VPC compute was launched, held here
    /// until the matching `RVpcComplete`/`RTaskError` arrives and pairs it
    /// with the result (or discards it on failure).
    pub vpc_pending_opts: HashMap<String, VpcOpts>,
    /// Error message from the last failed VPC compute per model stem —
    /// without this, a failure looked identical to having never clicked
    /// "Compute VPC" at all: the right panel just falls back to the
    /// pre-compute hint, with no sign anything was attempted.
    pub vpc_error: HashMap<String, String>,
    /// Error message from the last failed VPC "Edit and execute R script"
    /// export per model stem — same silent-revert gap as `vpc_error`. Kept
    /// per-stem (even though only one export runs at a time, mirroring the
    /// global `vpc_exporting` flag) so a failure for one model doesn't get
    /// misattributed to a different model's script popup later.
    pub vpc_export_error: HashMap<String, String>,
    /// Simulate-tab results (status summary only — the CSV lives on disk) keyed by model stem.
    pub simrun_results: HashMap<String, crate::domain::SimRunResult>,
    /// Stems for which a Simulate-tab run is currently in flight.
    pub simrun_computing: HashSet<String>,
    /// Error message from the last failed Simulate-tab run per model stem —
    /// same silent-revert-to-pristine-hint gap as `vpc_error`.
    pub simrun_error: HashMap<String, String>,
    /// Cached `ferx_check_init()` results keyed by model stem.
    pub check_init_results: HashMap<String, crate::domain::CheckInitResult>,
    /// Stems for which a check_init is currently in flight.
    pub check_init_running: HashSet<String>,
    /// Error message from the last failed check_init per model stem — without
    /// this, a failed check_init was only visible as a tiny, easy-to-miss
    /// status-bar line (reported: "spinner appears then disappears, nothing
    /// else"). Rendered as a proper error card next to the "Check inits"
    /// button instead.
    pub check_init_error: HashMap<String, String>,
    /// Cached SIR results keyed by model stem.
    pub sir_results: HashMap<String, crate::domain::SirResult>,
    /// Stems for which a SIR run is currently in flight.
    pub sir_running: HashSet<String>,
    /// Wall-clock start time for each in-flight SIR run (drives elapsed display in popup).
    pub sir_started_at: HashMap<String, std::time::Instant>,
    /// Cancellation senders for in-flight SIR threads. Send `()` to request cancellation.
    pub sir_cancel_tx: HashMap<String, std::sync::mpsc::SyncSender<()>>,
    /// Error message from the last failed SIR run per model stem — without
    /// this, a failed SIR run was only visible as a tiny, easy-to-miss
    /// status-bar line (reported: "seems doing it, but no results or error
    /// messages appear"). Rendered as a proper error card in the SIR tab
    /// instead, mirroring `check_init_error`.
    pub sir_error: HashMap<String, String>,
    /// Cached ETA-covariate correlation results keyed by model stem.
    pub eta_cov_results: HashMap<String, crate::domain::EtaCovResult>,
    /// Stems for which an ETA-cov computation is currently in flight.
    pub eta_cov_running: HashSet<String>,
    /// Error message from the last failed ETA-cov computation per model
    /// stem. Without this, the view (which auto-triggers computation on
    /// open, with no button in between) would see "not running" and "no
    /// result" both stay true forever after a failure, and re-launch the
    /// computation again on the very next frame — an unbounded retry loop
    /// that silently re-spawns R every frame instead of just failing once.
    pub eta_cov_failed: HashMap<String, String>,
    /// `.fitrx` mtime each `eta_cov_results`/`eta_cov_failed` entry was
    /// computed against. Re-fitting a model keeps the same stem, so stem
    /// alone can't tell a fresh fit apart from the stale result already
    /// cached under that name — without this, ETA-covariate results from a
    /// since-corrected (re-fit) model kept showing indefinitely.
    pub eta_cov_loaded_mtime: HashMap<String, std::time::SystemTime>,
    /// Cached declared-covariate screen results (`ferx_cov_screen`) keyed by
    /// model stem. Separate from `eta_cov_*` so the two views in the ETA-Cov
    /// section can be computed independently and lazily.
    pub cov_screen_results: HashMap<String, crate::domain::CovScreenResult>,
    /// Stems for which a covariate-screen computation is currently in flight.
    pub cov_screen_running: HashSet<String>,
    /// Error message from the last failed covariate-screen computation per
    /// model stem — same unbounded-retry-loop guard as `eta_cov_failed`.
    pub cov_screen_failed: HashMap<String, String>,
    /// Same staleness guard as `eta_cov_loaded_mtime`, for `cov_screen_*`.
    pub cov_screen_loaded_mtime: HashMap<String, std::time::SystemTime>,
}

impl WorkspaceState {
    pub fn load() -> Self {
        let app_dir = app_dir();

        if app_dir.is_none() {
            eprintln!("ferxgui: home directory unavailable or its .ferxgui app dir was refused (e.g. a symlink) — persistent data will not be saved");
        }

        let (settings, settings_warn) = app_dir
            .as_deref()
            .map(load_settings)
            .unwrap_or_default();
        let bookmarks = app_dir
            .as_deref()
            .map(load_bookmarks)
            .unwrap_or_default();

        let mut startup_warnings = Vec::new();
        if app_dir.is_none() {
            startup_warnings.push(
                "Warning: home directory unavailable or inaccessible — settings and run history will not be saved.".to_string(),
            );
        }
        if let Some(w) = settings_warn {
            startup_warnings.push(w);
        }
        // If the user explicitly set a custom path, honour it; otherwise we
        // will probe for the binary in the background and update the source.
        let ferx_binary_source = if settings.ferx_binary_custom {
            FerxBinarySource::Custom
        } else {
            FerxBinarySource::Detecting
        };
        Self {
            directory: settings.working_directory.clone(),
            models: Vec::new(),
            bookmarks,
            settings,
            app_dir,
            startup_warnings,
            scanning: false,
            ferx_binary_source,
            ferx_version: None,
            r_version:    None,
            r_model_infos: HashMap::new(),
            r_inspecting:  HashSet::new(),
            r_inspect_failed: HashSet::new(),
            vpc_data:           HashMap::new(),
            vpc_computing:      HashSet::new(),
            vpc_pending_opts:   HashMap::new(),
            vpc_error:          HashMap::new(),
            vpc_export_error:   HashMap::new(),
            simrun_results:     HashMap::new(),
            simrun_computing:   HashSet::new(),
            simrun_error:       HashMap::new(),
            check_init_results: HashMap::new(),
            check_init_running: HashSet::new(),
            check_init_error:   HashMap::new(),
            sir_results:        HashMap::new(),
            sir_running:        HashSet::new(),
            sir_started_at:     HashMap::new(),
            sir_cancel_tx:      HashMap::new(),
            sir_error:          HashMap::new(),
            eta_cov_results:    HashMap::new(),
            eta_cov_running:    HashSet::new(),
            eta_cov_failed:     HashMap::new(),
            eta_cov_loaded_mtime: HashMap::new(),
            cov_screen_results: HashMap::new(),
            cov_screen_running: HashSet::new(),
            cov_screen_failed:  HashMap::new(),
            cov_screen_loaded_mtime: HashMap::new(),
        }
    }

    pub fn save_settings(&self) -> Option<String> {
        let dir = self.app_dir.as_ref()?;
        crate::io::persistence::save_settings(dir, &self.settings)
            .err()
            .map(|e| format!("Warning: could not save settings — {e}"))
    }

    pub fn theme(&self) -> &Theme {
        &self.settings.theme
    }
}

// ---------------------------------------------------------------------------
// Run state
// ---------------------------------------------------------------------------

pub struct RunState {
    pub active_run: Option<ActiveRun>,
    /// Ring buffer of stdout/stderr lines from the current / last run.
    pub log_buffer: VecDeque<String>,
    /// Pre-joined version of log_buffer — rebuilt only when a new line arrives,
    /// not every frame, avoiding a ~500 KB allocation at 60 fps.
    pub log_text: String,
    pub run_history: Vec<RunRecord>,
    /// Sequential run queue — items are started one by one as the previous run finishes.
    pub run_queue: VecDeque<QueuedRun>,
}

impl RunState {
    const LOG_CAPACITY: usize = 5_000;

    pub fn load(app_dir: Option<&PathBuf>) -> Self {
        let run_history = app_dir.map(|d| load_runs(d)).unwrap_or_default();
        Self {
            active_run: None,
            log_buffer: VecDeque::with_capacity(Self::LOG_CAPACITY),
            log_text: String::new(),
            run_history,
            run_queue: VecDeque::new(),
        }
    }

    pub fn push_log(&mut self, line: String) {
        if self.log_buffer.len() == Self::LOG_CAPACITY {
            self.log_buffer.pop_front();
            self.log_buffer.push_back(line.clone());
            // Eviction path: remove the first line from the cached text by
            // slicing off up to and including the first newline, then append.
            match self.log_text.find('\n') {
                Some(nl) => { self.log_text.drain(..=nl); }
                None     => { self.log_text.clear(); return; }
            }
            self.log_text.push('\n');
            self.log_text.push_str(&line);
        } else {
            // Fast path: append only.
            if !self.log_text.is_empty() {
                self.log_text.push('\n');
            }
            self.log_text.push_str(&line);
            self.log_buffer.push_back(line);
        }
    }

    pub fn save_history(&self, app_dir: Option<&PathBuf>) -> Option<String> {
        let dir = app_dir?;
        crate::io::persistence::save_runs(dir, &self.run_history)
            .err()
            .map(|e| format!("Warning: could not save run history — {e}"))
    }
}

#[cfg(test)]
mod reference_model_persistence_tests {
    use super::AppState;
    use crate::domain::{FerxModel, ModelEntry, ModelMeta};
    use crate::workers::messages::WorkerMsg;

    fn model_entry(stem: &str, is_reference: bool) -> ModelEntry {
        ModelEntry {
            model: FerxModel {
                path: std::path::PathBuf::from(format!("{stem}.ferx")),
                stem: stem.to_string(),
                source: String::new(),
                params: Default::default(),
                created_at: None,
                data_path: None,
            },
            fitrx_path: None,
            fit: None,
            fit_parse_error: None,
            meta: ModelMeta { is_reference, ..Default::default() },
            is_stale: false,
        }
    }

    /// Regression test for "the reference model is forgotten on restart":
    /// on a fresh `AppState` (as after launching the app), `reference_model`
    /// starts `None`, so there's nothing to re-anchor by stem — the only
    /// way to restore it is from the persisted `meta.is_reference` flag
    /// that scanning just loaded off disk.
    #[test]
    fn restores_reference_model_from_persisted_meta_on_first_scan() {
        let mut state = AppState::new();
        assert_eq!(state.ui.reference_model, None);

        state.apply(WorkerMsg::ScanComplete(vec![
            model_entry("model_a", false),
            model_entry("model_b", true),
            model_entry("model_c", false),
        ]));

        assert_eq!(state.ui.reference_model, Some(1));
    }
}

#[cfg(test)]
mod set_directory_tests {
    use super::AppState;

    /// Regression test for "switching projects in the Models tab doesn't
    /// switch the Files tab": `files_cwd` previously only auto-initialized
    /// from `workspace.directory` once (the first time it was `None`), so a
    /// later project switch left Files silently showing the old directory.
    #[test]
    fn switching_directory_follows_in_the_files_tab() {
        let mut state = AppState::new();
        let first = std::env::temp_dir().join("ferxgui_set_directory_test_a");
        let second = std::env::temp_dir().join("ferxgui_set_directory_test_b");

        state.set_directory(first.clone());
        assert_eq!(state.ui.files_cwd, Some(first));

        // Simulate having navigated deeper into the first project's Files tab.
        state.ui.files_cwd = Some(std::env::temp_dir().join("ferxgui_set_directory_test_a/sub"));
        state.ui.files_back_stack.push(std::env::temp_dir().join("ferxgui_set_directory_test_a"));

        state.set_directory(second.clone());
        assert_eq!(state.ui.files_cwd, Some(second));
        assert!(state.ui.files_back_stack.is_empty());
    }
}

#[cfg(test)]
mod run_state_log_tests {
    use super::RunState;

    /// Regression test for "starting a run shows the previous run's output":
    /// `log_text` is a separately-maintained pre-joined cache of
    /// `log_buffer` (see its doc comment above) — clearing only `log_buffer`
    /// at run-start left `log_text` holding the entire previous run's log
    /// until enough new lines arrived to incrementally push it out via
    /// `push_log`'s eviction path. Both must be cleared together.
    #[test]
    fn clearing_log_buffer_and_text_together_removes_previous_run_output() {
        let mut run = RunState::load(None);
        run.push_log("old run line 1".to_string());
        run.push_log("old run line 2".to_string());
        assert!(run.log_text.contains("old run"));

        run.log_buffer.clear();
        run.log_text.clear();
        assert!(
            run.log_text.is_empty(),
            "log_text must be empty immediately after a new run starts, not just log_buffer"
        );

        run.push_log("new run line 1".to_string());
        assert_eq!(run.log_text, "new run line 1");
        assert!(!run.log_text.contains("old run"));
    }
}

// ---------------------------------------------------------------------------
// Top-level AppState
// ---------------------------------------------------------------------------

pub struct AppState {
    pub ui: UiState,
    pub workspace: WorkspaceState,
    pub run: RunState,
    pub sim: crate::domain::SimTabState,
    /// Sender half — cloned and handed to worker threads.
    pub worker_tx: Sender<WorkerMsg>,
    /// Receiver half — drained in the egui update() loop via try_recv().
    pub worker_rx: Receiver<WorkerMsg>,
}

impl AppState {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<WorkerMsg>();
        let workspace = WorkspaceState::load();
        let run = RunState::load(workspace.app_dir.as_ref());
        let ui = UiState {
            sidebar_collapsed: workspace.settings.sidebar_collapsed,
            files_cwd: workspace.settings.working_directory.clone(),
            files_active_exts: workspace.settings.file_extensions.iter().cloned().collect(),
            ..Default::default()
        };

        Self {
            ui,
            workspace,
            run,
            sim: crate::domain::SimTabState::default(),
            worker_tx: tx,
            worker_rx: rx,
        }
    }

    /// Drain the worker channel and apply each message.  Called every frame.
    pub fn process_worker_messages(&mut self) {
        while let Ok(msg) = self.worker_rx.try_recv() {
            self.apply(msg);
        }
    }

    fn apply(&mut self, msg: WorkerMsg) {
        use WorkerMsg::*;
        match msg {
            ScanComplete(models) => {
                // Load any fresh SIR caches that appeared on disk.
                for m in &models {
                    if let Some(fitrx) = &m.fitrx_path {
                        let stem = m.model.stem.clone();
                        if let std::collections::hash_map::Entry::Vacant(e) = self.workspace.sir_results.entry(stem) {
                            if let Some(sir) = crate::domain::SirResult::load_if_fresh(fitrx) {
                                e.insert(sir);
                            }
                        }
                    }
                }

                // Re-anchor the current selection by stem so the index stays
                // valid even when the list grows/reorders between scans.
                let selected_stem = self.ui.selected_model
                    .and_then(|i| self.workspace.models.get(i))
                    .map(|m| m.model.stem.clone());
                let reference_stem = self.ui.reference_model
                    .and_then(|i| self.workspace.models.get(i))
                    .map(|m| m.model.stem.clone());

                self.workspace.models = models;
                self.workspace.scanning = false;
                self.ui.status_message = format!(
                    "Loaded {} model(s)",
                    self.workspace.models.len()
                );

                // Restore selection by stem.
                self.ui.selected_model = selected_stem.and_then(|s| {
                    self.workspace.models.iter().position(|m| m.model.stem == s)
                });
                // Falls back to the persisted `meta.is_reference` flag when
                // there's no in-session reference to re-anchor (e.g. right
                // after startup) — without this, the reference model was
                // forgotten on every restart despite being saved to
                // model_meta.json.
                self.ui.reference_model = reference_stem
                    .and_then(|s| self.workspace.models.iter().position(|m| m.model.stem == s))
                    .or_else(|| self.workspace.models.iter().position(|m| m.meta.is_reference));
            }
            RunLine(line) => {
                self.run.push_log(line);
            }
            RunFinished { exit_code, record } => {
                let record  = *record;
                let success = exit_code == 0;
                let stem    = record.model_stem.clone();
                let export_tables = self.run.active_run.as_ref()
                    .map(|r| r.export_tables)
                    .unwrap_or(false);
                let run_sir_after = self.run.active_run.as_ref()
                    .map(|r| r.run_sir_after)
                    .unwrap_or(false);
                // Clear unconditionally (not just when this run also exports
                // tables) — a stale error from an earlier run for this same
                // model would otherwise keep showing in the Run popup even
                // after a run that didn't touch export at all.
                if self.ui.export_tables_error.as_ref().is_some_and(|(s, _)| *s == stem) {
                    self.ui.export_tables_error = None;
                }
                self.ui.status_message = if success {
                    format!("Run completed: {stem}")
                } else {
                    format!("Run failed (exit {exit_code}): {stem}")
                };
                self.run.run_history.push(record.clone());
                self.run.active_run = None;
                if let Some(warn) = self.run.save_history(self.workspace.app_dir.as_ref()) {
                    self.ui.status_message = warn;
                }
                // Re-scan to pick up new .fitrx.  Brief delay so the OS has
                // time to flush the zip to disk before we try to open it.
                let tx = self.worker_tx.clone();
                let dir = self.workspace.directory.clone();
                let meta = match (self.workspace.app_dir.as_deref(), self.workspace.directory.as_deref()) {
                    (Some(app_dir), Some(ws)) => crate::io::persistence::load_model_meta(app_dir, ws),
                    _ => Default::default(),
                };
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    if let Some(d) = dir {
                        crate::workers::scan::scan_directory(d, meta, tx);
                    }
                });
                // Extract sdtab/patab when requested and run succeeded.
                if success && export_tables {
                    let fitrx = record.directory.join(format!("{stem}.fitrx"));
                    let stem2 = stem.clone();
                    let tx2   = self.worker_tx.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        match crate::io::fitrx::extract_output_tables(&fitrx) {
                            Ok(paths) => {
                                let _ = tx2.send(WorkerMsg::TablesExported { stem: stem2, paths });
                            }
                            Err(e) => {
                                let _ = tx2.send(WorkerMsg::RTaskError {
                                    context: format!("export_tables {stem2}"),
                                    message: e.to_string(),
                                });
                            }
                        }
                    });
                }
                // System notification — fires even when the app is in the background.
                crate::notify::send(&stem, success);

                // Auto-trigger SIR when requested, run succeeded, and SIR is not already running.
                if success && run_sir_after && !self.workspace.sir_running.contains(&stem) {
                    let fitrx       = record.directory.join(format!("{stem}.fitrx"));
                    let stem_sir    = stem.clone();
                    let tx_sir      = self.worker_tx.clone();
                    let n_samples   = self.ui.sir_n_samples;
                    let n_resamples = self.ui.sir_n_resamples;
                    let seed        = self.ui.sir_seed;
                    let keep        = self.ui.sir_keep_samples;
                    let (cancel_tx, cancel_rx) = std::sync::mpsc::sync_channel::<()>(1);
                    self.workspace.sir_running.insert(stem.clone());
                    self.workspace.sir_started_at.insert(stem.clone(), std::time::Instant::now());
                    self.workspace.sir_cancel_tx.insert(stem.clone(), cancel_tx);
                    self.workspace.sir_error.remove(&stem);
                    self.ui.sir_popup_open      = true;
                    self.ui.sir_popup_last_stem = Some(stem.clone());
                    std::thread::spawn(move || {
                        // Cancellable wait for the .fitrx to be fully flushed.
                        // 8 × 100 ms = 800 ms total, but interrupts on cancel.
                        for _ in 0..8 {
                            if cancel_rx.try_recv().is_ok() { return; }
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                        match crate::io::r_extract::compute_sir(&fitrx, n_samples, n_resamples, seed, keep) {
                            Ok(result) => {
                                let _ = tx_sir.send(WorkerMsg::SirComplete {
                                    stem: stem_sir, result: Box::new(result),
                                });
                            }
                            Err(e) => {
                                let _ = tx_sir.send(WorkerMsg::RTaskError {
                                    context: format!("sir:auto:{stem_sir}"),
                                    message: e,
                                });
                            }
                        }
                    });
                }
            }
            RunError(msg) => {
                self.run.push_log(format!("[error] {}", msg));
                self.run.active_run = None;
                self.ui.status_message = format!("Run error: {}", msg);
            }
            RInspectComplete { stem, info } => {
                self.workspace.r_inspecting.remove(&stem);
                self.workspace.r_model_infos.insert(stem, *info);
            }
            RVpcComplete { stem, data } => {
                self.workspace.vpc_computing.remove(&stem);
                self.workspace.vpc_error.remove(&stem);
                let opts = self.workspace.vpc_pending_opts.remove(&stem).unwrap_or_default();
                if data.warnings.is_empty() {
                    self.ui.status_message = format!("VPC ready: {stem}");
                } else {
                    self.ui.status_message = format!(
                        "VPC ready: {stem} (warning: {})",
                        data.warnings.join("; ")
                    );
                }
                self.workspace.vpc_data.insert(stem, VpcRenderData { result: *data, opts });
            }
            VpcPkgStatus(status) => {
                self.ui.vpc_pkg_checking = false;
                self.ui.vpc_pkg_status = Some(status);
            }
            VpcPlotExported { path } => {
                self.ui.vpc_exporting = false;
                // Only one export runs at a time app-wide, so on success
                // there's exactly one entry that could possibly be stale;
                // clearing the whole map is simpler than threading a stem
                // through this message just to remove one key.
                self.workspace.vpc_export_error.clear();
                if let Err(e) = open::that(&path) {
                    self.ui.status_message = format!("Exported R ggplot → {path} (could not open: {e})");
                } else {
                    self.ui.status_message = format!("Opened R ggplot → {path}");
                }
            }
            GofExportComplete { path } => {
                self.ui.eval_exporting = false;
                self.ui.status_message = format!("Exported → {path}");
            }
            GofExportError { message } => {
                self.ui.eval_exporting = false;
                self.ui.status_message = format!("Export failed: {message}");
            }
            SimComplete { generation, result } => {
                if generation == self.sim.generation {
                    self.sim.running = false;
                    let n = result.times.len();
                    self.sim.status = format!("Plot ready — {n} unique X values");
                    self.sim.result = Some(*result);
                }
                // else: stale result from a since-superseded file — discard.
            }
            SimError { generation, message } => {
                if generation == self.sim.generation {
                    self.sim.running = false;
                    self.sim.status = format!("Error: {message}");
                }
            }
            SimRunComplete { stem, result } => {
                self.workspace.simrun_computing.remove(&stem);
                self.workspace.simrun_error.remove(&stem);
                self.ui.status_message = format!("Simulation ready: {stem} ({} rows)", result.n_rows);
                self.workspace.simrun_results.insert(stem, *result);
            }
            ModelCreated(stem) => {
                self.ui.status_message = format!("Created {stem}.ferx");
                self.trigger_scan();
            }
            RCheckInitComplete { stem, result } => {
                self.workspace.check_init_running.remove(&stem);
                self.workspace.check_init_error.remove(&stem);
                self.workspace.check_init_results.insert(stem, *result);
            }
            SirComplete { stem, result } => {
                self.workspace.sir_running.remove(&stem);
                self.workspace.sir_started_at.remove(&stem);
                self.workspace.sir_cancel_tx.remove(&stem);
                self.workspace.sir_error.remove(&stem);
                // Auto-select first parameter for distribution histogram.
                if self.ui.sir_selected_param.is_empty() {
                    if let Some(first) = result.corr_names.first() {
                        self.ui.sir_selected_param = first.clone();
                    }
                }
                // Persist alongside the .fitrx so results survive restarts.
                if let Some(fitrx) = self.workspace.models.iter()
                    .find(|m| m.model.stem == stem)
                    .and_then(|m| m.fitrx_path.as_ref())
                {
                    let _ = result.save(fitrx);
                }
                self.workspace.sir_results.insert(stem, *result);
            }
            TablesExported { stem, paths } => {
                self.ui.export_tables_error = None;
                let names: Vec<String> = paths.iter()
                    .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(str::to_owned))
                    .collect();
                self.ui.status_message = format!(
                    "Tables written for {stem}: {}",
                    names.join(", ")
                );
            }
            EtaCovComplete { stem, result } => {
                self.workspace.eta_cov_running.remove(&stem);
                self.workspace.eta_cov_failed.remove(&stem);
                self.ui.status_message = format!("ETA-cov ready: {stem}");
                self.workspace.eta_cov_results.insert(stem, *result);
            }
            CovScreenComplete { stem, result } => {
                self.workspace.cov_screen_running.remove(&stem);
                self.workspace.cov_screen_failed.remove(&stem);
                self.ui.status_message = format!("Covariate screen ready: {stem}");
                self.workspace.cov_screen_results.insert(stem, *result);
            }
            RTaskError { context, message } => {
                // Clean up in-flight tracking if stem is encoded in the context.
                // For inspect, also mark as failed so we don't auto-retry every frame.
                if let Some(stem) = context.strip_prefix("inspect ") {
                    self.workspace.r_inspecting.remove(stem);
                    self.workspace.r_inspect_failed.insert(stem.to_string());
                }
                if let Some(stem) = context.strip_prefix("vpc ") {
                    self.workspace.vpc_computing.remove(stem);
                    self.workspace.vpc_pending_opts.remove(stem);
                    self.workspace.vpc_error.insert(stem.to_string(), message.clone());
                }
                if let Some(stem) = context.strip_prefix("vpc_export ") {
                    self.ui.vpc_exporting = false;
                    self.workspace.vpc_export_error.insert(stem.to_string(), message.clone());
                }
                if let Some(stem) = context.strip_prefix("simulate ") {
                    self.workspace.simrun_computing.remove(stem);
                    self.workspace.simrun_error.insert(stem.to_string(), message.clone());
                }
                if let Some(stem) = context.strip_prefix("check_init ") {
                    self.workspace.check_init_running.remove(stem);
                    self.workspace.check_init_error.insert(stem.to_string(), message.clone());
                }
                if let Some(stem) = context.strip_prefix("sir:manual:") {
                    self.workspace.sir_running.remove(stem.trim());
                    self.workspace.sir_started_at.remove(stem.trim());
                    self.workspace.sir_cancel_tx.remove(stem.trim());
                    self.workspace.sir_error.insert(stem.trim().to_string(), message.clone());
                }
                if let Some(stem) = context.strip_prefix("sir:auto:") {
                    self.workspace.sir_running.remove(stem.trim());
                    self.workspace.sir_started_at.remove(stem.trim());
                    self.workspace.sir_cancel_tx.remove(stem.trim());
                    self.workspace.sir_error.insert(stem.trim().to_string(), message.clone());
                }
                if let Some(stem) = context.strip_prefix("eta_cov ") {
                    self.workspace.eta_cov_running.remove(stem);
                    self.workspace.eta_cov_failed.insert(stem.to_string(), message.clone());
                }
                if let Some(stem) = context.strip_prefix("cov_screen ") {
                    self.workspace.cov_screen_running.remove(stem);
                    self.workspace.cov_screen_failed.insert(stem.to_string(), message.clone());
                }
                if let Some(stem) = context.strip_prefix("export_tables ") {
                    self.ui.export_tables_error = Some((stem.to_string(), message.clone()));
                }
                self.ui.status_message = format!("R error ({context}): {message}");
            }
            FerxBinaryDetected(result) => {
                // Never override a path the user set explicitly.
                if self.workspace.ferx_binary_source != FerxBinarySource::Custom {
                    match result {
                        Some((path, version, r_ver)) => {
                            self.workspace.settings.ferx_binary = Some(path);
                            self.workspace.ferx_version = Some(version);
                            self.workspace.r_version = Some(r_ver);
                            self.workspace.ferx_binary_source = FerxBinarySource::RPackage;
                        }
                        None => {
                            self.workspace.ferx_version = None;
                            self.workspace.ferx_binary_source =
                                if self.workspace.settings.ferx_binary.is_some() {
                                    FerxBinarySource::SystemPath
                                } else {
                                    FerxBinarySource::NotFound
                                };
                        }
                    }
                }
            }
        }
    }

    /// Kick off an asynchronous directory scan on a background thread.
    pub fn trigger_scan(&mut self) {
        let Some(dir) = self.workspace.directory.clone() else { return };
        let meta_map = match (self.workspace.app_dir.as_deref(), self.workspace.directory.as_deref()) {
            (Some(app_dir), Some(ws)) => load_model_meta(app_dir, ws),
            _ => Default::default(),
        };
        let tx = self.worker_tx.clone();
        self.workspace.scanning = true;
        std::thread::spawn(move || {
            crate::workers::scan::scan_directory(dir, meta_map, tx);
        });
    }

    /// Set the working directory and immediately trigger a scan.
    pub fn set_directory(&mut self, dir: PathBuf) {
        self.workspace.directory = Some(dir.clone());
        self.workspace.settings.working_directory = Some(dir.clone());
        if let Some(warn) = self.workspace.save_settings() {
            self.ui.status_message = warn;
        }
        self.trigger_scan();
        // Close any in-progress bookmark dialog — it holds a path for the old directory.
        self.ui.pending_bookmark = None;
        // Follow the new workspace directory in the Files tab too — otherwise
        // switching projects here left Files silently showing the *previous*
        // project's directory, since `files_cwd` only ever auto-initializes
        // once (the first time it's `None`), never re-synced afterward.
        self.ui.files_cwd = Some(dir);
        self.ui.files_selected = None;
        self.ui.files_view_mode = crate::state::FilesViewMode::Empty;
        self.ui.files_back_stack.clear();
    }

    /// Reference model OFV for ΔOFV column.
    pub fn reference_ofv(&self) -> f64 {
        self.ui
            .reference_model
            .and_then(|i| self.workspace.models.get(i))
            .and_then(|m| m.fit.as_ref())
            .map(|f| f.ofv)
            .unwrap_or(f64::NAN)
    }
}

#[cfg(test)]
mod vpc_theme_tests {
    use super::{VpcLineStyle, VpcTheme};

    #[test]
    fn line_style_r_names_match_ggplot() {
        // These strings are passed verbatim to ggplot2 `linetype`; a typo would
        // silently break the exported plot theme.
        assert_eq!(VpcLineStyle::Solid.r_name(),  "solid");
        assert_eq!(VpcLineStyle::Dashed.r_name(), "dashed");
        assert_eq!(VpcLineStyle::Dotted.r_name(), "dotted");
    }

    #[test]
    fn theme_defaults_match_vpc_package() {
        // Mirrors vpc::new_vpc_theme() defaults so the native render and the R
        // export start from the same look.
        let t = VpcTheme::default();
        assert!((t.sim_pi_alpha - 0.15).abs() < 1e-9);
        assert!((t.sim_median_alpha - 0.30).abs() < 1e-9);
        assert_eq!(t.obs_median_style, VpcLineStyle::Solid);
        assert_eq!(t.obs_ci_style, VpcLineStyle::Dashed);
        assert!(t.bin_sep_show);
    }
}
