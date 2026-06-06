use std::collections::HashMap;
use std::sync::Arc;
use eframe::egui;

// ---------------------------------------------------------------------------
// Persistent band / filter / reference-line specs
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct BandSpec {
    pub lo_pct:  f32,
    pub hi_pct:  f32,
    pub color:   egui::Color32,
    pub alpha:   f32,
    pub visible: bool,
}

impl BandSpec {
    pub fn new(lo_pct: f32, hi_pct: f32, color: egui::Color32, alpha: f32) -> Self {
        Self { lo_pct, hi_pct, color, alpha, visible: true }
    }
}

/// A horizontal reference / threshold line (e.g. IC90, MEC, BLOQ).
#[derive(Clone)]
pub struct RefLine {
    /// Y-axis value.
    pub y:       f64,
    /// Legend / annotation label (e.g. "IC90").
    pub label:   String,
    pub color:   egui::Color32,
    pub visible: bool,
}

impl RefLine {
    pub fn new(y: f64, label: impl Into<String>, color: egui::Color32) -> Self {
        Self { y, label: label.into(), color, visible: true }
    }
}

#[derive(Clone, Default)]
pub struct FilterRow {
    pub col: String,
    pub op:  String,
    pub val: String,
}

impl FilterRow {
    pub fn new() -> Self {
        Self { col: String::new(), op: "==".to_string(), val: String::new() }
    }
}

// ---------------------------------------------------------------------------
// Loaded simulation data (column-oriented, shared with worker thread)
// ---------------------------------------------------------------------------

pub struct SimData {
    pub columns:  Vec<String>,
    pub col_data: HashMap<String, Vec<f64>>,
    pub n_rows:   usize,
}

// ---------------------------------------------------------------------------
// Computation result
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SimBandData {
    pub lo:  Vec<f64>,
    pub med: Vec<f64>,
    pub hi:  Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct SimPlotResult {
    pub times: Vec<f64>,
    pub bands: Vec<SimBandData>,
}

// ---------------------------------------------------------------------------
// Tab state
// ---------------------------------------------------------------------------

pub struct SimTabState {
    // Data
    pub file_path:  String,
    pub data:       Option<Arc<SimData>>,
    pub data_label: String,

    // Observed overlay
    pub obs_path:    String,
    pub obs_xy:      Option<(Vec<f64>, Vec<f64>)>,
    pub obs_columns: Vec<String>,
    pub obs_x_col:   String,
    pub obs_y_col:   String,
    pub obs_label:   String,

    // Column selection
    pub rep_col: String,
    pub x_col:   String,
    pub y_col:   String,

    // Bands
    pub bands:      Vec<BandSpec>,
    pub preset_idx: usize,

    // Appearance
    pub median_color: egui::Color32,
    pub median_lw:    f32,
    pub log_y:        bool,
    pub smooth:       bool,
    pub smooth_frac:  f32,

    // Filters
    pub mdv_filter: bool,
    pub filters:    Vec<FilterRow>,

    // Reference / threshold lines
    pub ref_lines: Vec<RefLine>,

    // Axis labels for publication export (editable, default = column name)
    pub x_label: String,
    pub y_label: String,

    // Computation state
    pub running: bool,
    pub result:  Option<SimPlotResult>,
    pub status:  String,

    // Export — `export_pending` triggers the plotters PNG render (replaces screenshot path)
    pub export_pending: bool,
}

impl Default for SimTabState {
    fn default() -> Self {
        let blue = egui::Color32::from_rgb(0x56, 0x9c, 0xd6);
        Self {
            file_path:  String::new(),
            data:       None,
            data_label: String::new(),

            obs_path:    String::new(),
            obs_xy:      None,
            obs_columns: Vec::new(),
            obs_x_col:   String::new(),
            obs_y_col:   String::new(),
            obs_label:   String::new(),

            rep_col: String::new(),
            x_col:   String::new(),
            y_col:   String::new(),

            bands: vec![
                BandSpec::new(5.0,  95.0, blue, 0.25),
                BandSpec::new(25.0, 75.0, blue, 0.40),
            ],
            preset_idx: 0,

            median_color: egui::Color32::from_rgb(0xdd, 0xe0, 0xee),
            median_lw:    2.0,
            log_y:        false,
            smooth:       false,
            smooth_frac:  0.30,

            mdv_filter: true,
            filters:    Vec::new(),

            ref_lines: Vec::new(),
            x_label:   String::new(),
            y_label:   String::new(),

            running: false,
            result:  None,
            status:  String::new(),

            export_pending: false,
        }
    }
}
