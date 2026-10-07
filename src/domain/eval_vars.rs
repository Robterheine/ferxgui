//! Per-observation variables for the Evaluation tab: everything a GOF plot can be
//! filtered or coloured by, aligned row for row with `EvalData.rows`.
//!
//! Variables come from three places (earlier wins on a name clash): the columns of
//! `predictions.csv`, the model's declared covariates (`covtab.csv`), and the columns of
//! the original dataset, joined back on by `io::dataset`.

use std::collections::HashMap;

use super::eval::{CovTabData, EvalData};

/// Most distinct values a numeric column may have and still default to "categorical".
const MAX_AUTO_LEVELS: usize = 20;
/// Most distinct values any column may be treated as categories (checklist) with.
const MAX_LEVELS: usize = 500;
/// Quantile bins used to colour a continuous variable.
pub const COLOR_BINS: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarSource {
    Prediction,
    Covariate,
    Dataset,
}

impl VarSource {
    pub fn label(self) -> &'static str {
        match self {
            VarSource::Prediction => "Prediction columns",
            VarSource::Covariate  => "Declared covariates",
            VarSource::Dataset    => "Dataset columns",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarKind {
    /// A handful of distinct values (CMT, MDV, OCC, SEX) or text: filtered by a checklist.
    Categorical,
    /// A continuous value (WT, AGE, TIME): filtered by a range.
    Numeric,
}

/// One variable, one value per `EvalData` row. Missing is `NaN`. Text columns store the
/// index into `text_levels` as the value.
#[derive(Debug, Clone)]
pub struct VarColumn {
    pub name: String,
    pub source: VarSource,
    pub values: Vec<f64>,
    /// The kind picked automatically from the data.
    pub auto_kind: VarKind,
    /// Sorted distinct finite values; empty when there are more than `MAX_LEVELS`.
    pub levels: Vec<f64>,
    /// Labels for a text column (`values` index into this).
    pub text_levels: Option<Vec<String>>,
    pub min: f64,
    pub max: f64,
}

fn is_missing_token(s: &str) -> bool {
    let t = s.trim();
    t.is_empty() || t == "." || t.eq_ignore_ascii_case("na") || t.eq_ignore_ascii_case("nan")
}

impl VarColumn {
    /// Builds a column from raw strings, deciding text vs numeric.
    pub fn from_strings(name: &str, source: VarSource, raw: &[String]) -> Self {
        let parsed: Vec<Option<f64>> = raw.iter()
            .map(|s| if is_missing_token(s) { Some(f64::NAN) } else { s.trim().parse::<f64>().ok() })
            .collect();
        if parsed.iter().all(Option::is_some) {
            let values = parsed.into_iter().map(|v| v.unwrap_or(f64::NAN)).collect();
            return Self::from_numeric(name, source, values);
        }
        // Text column: code each distinct string by sorted order.
        let mut uniq: Vec<String> = raw.iter()
            .filter(|s| !is_missing_token(s))
            .map(|s| s.trim().to_string())
            .collect();
        uniq.sort();
        uniq.dedup();
        let index: HashMap<&str, usize> = uniq.iter().enumerate().map(|(i, s)| (s.as_str(), i)).collect();
        let values: Vec<f64> = raw.iter()
            .map(|s| if is_missing_token(s) { f64::NAN } else { index[s.trim()] as f64 })
            .collect();
        let levels = if uniq.len() <= MAX_LEVELS { (0..uniq.len()).map(|i| i as f64).collect() } else { vec![] };
        let (min, max) = finite_range(&values);
        Self {
            name: name.to_string(), source, values,
            auto_kind: VarKind::Categorical, levels, text_levels: Some(uniq), min, max,
        }
    }

    pub fn from_numeric(name: &str, source: VarSource, values: Vec<f64>) -> Self {
        let mut uniq: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
        uniq.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        uniq.dedup();
        let all_int = uniq.iter().all(|v| v.fract() == 0.0);
        let auto_kind = if uniq.len() <= MAX_AUTO_LEVELS && all_int && !uniq.is_empty() {
            VarKind::Categorical
        } else {
            VarKind::Numeric
        };
        let (min, max) = finite_range(&values);
        let levels = if uniq.len() <= MAX_LEVELS { uniq } else { vec![] };
        Self { name: name.to_string(), source, values, auto_kind, levels, text_levels: None, min, max }
    }

    /// Label for one level value.
    pub fn level_label(&self, v: f64) -> String {
        match &self.text_levels {
            Some(t) => t.get(v as usize).cloned().unwrap_or_default(),
            None => fmt_num(v),
        }
    }

    /// Whether a checklist of levels is possible for this column.
    pub fn can_be_categorical(&self) -> bool {
        !self.levels.is_empty()
    }

    pub fn is_text(&self) -> bool {
        self.text_levels.is_some()
    }

    pub fn n_missing(&self) -> usize {
        self.values.iter().filter(|v| !v.is_finite()).count()
    }
}

fn finite_range(values: &[f64]) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &v in values.iter().filter(|v| v.is_finite()) {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    if lo.is_infinite() { (0.0, 0.0) } else { (lo, hi) }
}

/// Compact number formatting for level labels and bin edges.
pub fn fmt_num(v: f64) -> String {
    if !v.is_finite() { return "NA".into(); }
    if v.fract() == 0.0 && v.abs() < 1e9 { return format!("{}", v as i64); }
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Back-transformation applied to DV, PRED and IPRED for display, for models fitted to
/// log-transformed data. Residuals (CWRES, IWRES) are never transformed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum YTransform {
    #[default]
    AsFitted,
    /// exp(): data fitted on the natural-log scale.
    Exp,
    /// 10^x: data fitted on the log10 scale.
    Pow10,
}

impl YTransform {
    pub const ALL: [YTransform; 3] = [YTransform::AsFitted, YTransform::Exp, YTransform::Pow10];

    pub fn apply(self, v: f64) -> f64 {
        match self {
            YTransform::AsFitted => v,
            YTransform::Exp => v.exp(),
            YTransform::Pow10 => 10f64.powf(v),
        }
    }

    /// Menu label.
    pub fn label(self) -> &'static str {
        match self {
            YTransform::AsFitted => "As fitted",
            YTransform::Exp => "exp(ln)",
            YTransform::Pow10 => "10^(log10)",
        }
    }

    /// Axis label for a transformed quantity, e.g. `exp(DV)`.
    pub fn wrap(self, name: &str) -> String {
        match self {
            YTransform::AsFitted => name.to_string(),
            YTransform::Exp => format!("exp({name})"),
            YTransform::Pow10 => format!("10^{name}"),
        }
    }

    /// Token passed to the R export script.
    pub fn token(self) -> &'static str {
        match self {
            YTransform::AsFitted => "none",
            YTransform::Exp => "exp",
            YTransform::Pow10 => "pow10",
        }
    }
}

/// How the dataset join went; shown as a badge next to the filter controls.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum DatasetStatus {
    /// Not attempted yet / no fit loaded.
    #[default]
    Unknown,
    /// The bundle records no dataset path and none could be found.
    NoPath,
    /// A recorded path does not exist on disk.
    NotFound(String),
    /// The file was read but could not be matched to the prediction rows (or failed to parse).
    Failed(String),
    /// Joined: file name and number of aligned observation rows.
    Aligned { name: String, rows: usize },
}

/// All variables available for one fit.
#[derive(Debug, Clone, Default)]
pub struct VarTable {
    pub n_rows: usize,
    pub columns: Vec<VarColumn>,
}

impl VarTable {
    /// Builds the table from the prediction rows, any extra `predictions.csv` columns,
    /// the declared covariates, and (when the join succeeded) the dataset columns.
    pub fn build(
        eval: &EvalData,
        covtab: Option<&CovTabData>,
        dataset: Option<&[(String, Vec<String>)]>,
    ) -> Self {
        let n = eval.rows.len();
        let mut t = VarTable { n_rows: n, columns: Vec::new() };

        let ids: Vec<String> = eval.rows.iter().map(|r| r.id.clone()).collect();
        t.push(VarColumn::from_strings("ID", VarSource::Prediction, &ids));
        let num = |f: fn(&super::eval::PredRow) -> f64| -> Vec<f64> { eval.rows.iter().map(f).collect() };
        for (name, vals) in [
            ("TIME",    num(|r| r.time)),
            ("DV",      num(|r| r.dv)),
            ("PRED",    num(|r| r.pred)),
            ("IPRED",   num(|r| r.ipred)),
            ("CWRES",   num(|r| r.cwres)),
            ("IWRES",   num(|r| r.iwres)),
            ("TAD",     num(|r| r.tad)),
            ("EBE_OFV", num(|r| r.ebe_ofv)),
        ] {
            // Skip columns the bundle did not carry (all missing).
            if vals.iter().any(|v| v.is_finite()) {
                t.push(VarColumn::from_numeric(name, VarSource::Prediction, vals));
            }
        }
        for (name, raw) in &eval.extras {
            if raw.len() == n {
                t.push(VarColumn::from_strings(name, VarSource::Prediction, raw));
            }
        }
        if let Some(c) = covtab {
            for name in &c.covariate_names {
                let vals: Vec<f64> = eval.rows.iter()
                    .map(|r| c.lookup(&r.id, r.time, name).unwrap_or(f64::NAN))
                    .collect();
                if vals.iter().any(|v| v.is_finite()) {
                    t.push(VarColumn::from_numeric(name, VarSource::Covariate, vals));
                }
            }
        }
        if let Some(cols) = dataset {
            for (name, raw) in cols {
                if raw.len() == n {
                    t.push(VarColumn::from_strings(name, VarSource::Dataset, raw));
                }
            }
        }
        t
    }

    /// Adds a column unless one with the same name (case-insensitive) already exists.
    fn push(&mut self, col: VarColumn) {
        if !self.columns.iter().any(|c| c.name.eq_ignore_ascii_case(&col.name)) {
            self.columns.push(col);
        }
    }

    pub fn get(&self, name: &str) -> Option<&VarColumn> {
        self.columns.iter().find(|c| c.name == name)
    }

    /// Kind to use for `name`, honouring a user override when it is possible.
    pub fn kind_of(&self, view: &EvalView, name: &str) -> Option<VarKind> {
        let col = self.get(name)?;
        Some(match view.kind_override.get(name) {
            Some(VarKind::Categorical) if col.can_be_categorical() => VarKind::Categorical,
            Some(VarKind::Numeric) if !col.is_text() => VarKind::Numeric,
            _ => col.auto_kind,
        })
    }

    /// When every observation has the same (non-missing) value, that value as a label.
    pub fn constant_label(col: &VarColumn) -> Option<String> {
        if col.max > col.min { return None; }
        if col.values.iter().all(|v| !v.is_finite()) { return Some("all missing".into()); }
        Some(format!("all {}", col.level_label(col.min)))
    }
}

// ---------------------------------------------------------------------------
// Filters
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum FilterClause {
    /// Keep rows whose value is one of `allowed`.
    Levels { var: String, allowed: Vec<f64>, keep_missing: bool },
    /// Keep rows with `lo <= value <= hi`.
    Range { var: String, lo: f64, hi: f64 },
}

impl FilterClause {
    pub fn var(&self) -> &str {
        match self {
            FilterClause::Levels { var, .. } | FilterClause::Range { var, .. } => var,
        }
    }

    fn passes(&self, v: f64) -> bool {
        match self {
            FilterClause::Levels { allowed, keep_missing, .. } => {
                if v.is_finite() { allowed.contains(&v) } else { *keep_missing }
            }
            FilterClause::Range { lo, hi, .. } => v.is_finite() && v >= *lo && v <= *hi,
        }
    }
}

/// Filter + colour settings for one model's GOF plots.
#[derive(Debug, Clone, Default)]
pub struct EvalView {
    pub filters: Vec<FilterClause>,
    pub color_by: Option<String>,
    /// One LOESS line per colour group instead of a single overall line.
    pub loess_per_group: bool,
    pub kind_override: HashMap<String, VarKind>,
}

impl EvalView {
    /// Which rows survive every filter. Clauses on a variable the table no longer has
    /// are ignored.
    pub fn mask(&self, table: &VarTable) -> Vec<bool> {
        let mut keep = vec![true; table.n_rows];
        for clause in &self.filters {
            let Some(col) = table.get(clause.var()) else { continue };
            for (k, &v) in keep.iter_mut().zip(&col.values) {
                if *k && !clause.passes(v) { *k = false; }
            }
        }
        keep
    }

    pub fn is_filtered(&self) -> bool {
        !self.filters.is_empty()
    }

    pub fn reset(&mut self) {
        self.filters.clear();
        self.color_by = None;
        self.loess_per_group = false;
    }

    /// A fresh clause for `var` that keeps everything.
    pub fn default_clause(&self, table: &VarTable, var: &str) -> Option<FilterClause> {
        let col = table.get(var)?;
        Some(match table.kind_of(self, var)? {
            VarKind::Categorical => FilterClause::Levels {
                var: var.to_string(), allowed: col.levels.clone(), keep_missing: true,
            },
            VarKind::Numeric => FilterClause::Range { var: var.to_string(), lo: col.min, hi: col.max },
        })
    }
}

// ---------------------------------------------------------------------------
// Colour groups
// ---------------------------------------------------------------------------

pub const NO_GROUP: u16 = u16::MAX;

/// Assignment of rows to colour groups for one variable.
#[derive(Debug, Clone, Default)]
pub struct Grouping {
    pub labels: Vec<String>,
    /// Group index per row; the last group is "(missing)" when any value is missing.
    pub group: Vec<u16>,
    pub continuous: bool,
}

impl Grouping {
    pub fn new(col: &VarColumn, kind: VarKind) -> Self {
        match kind {
            VarKind::Categorical => Self::categorical(col),
            VarKind::Numeric => Self::binned(col, COLOR_BINS),
        }
    }

    fn categorical(col: &VarColumn) -> Self {
        let mut labels: Vec<String> = col.levels.iter().map(|&l| col.level_label(l)).collect();
        let mut group: Vec<u16> = col.values.iter()
            .map(|&v| col.levels.iter().position(|&l| l == v).map_or(NO_GROUP, |i| i as u16))
            .collect();
        Self::add_missing_group(&mut labels, &mut group, &col.values);
        Self { labels, group, continuous: false }
    }

    fn binned(col: &VarColumn, bins: usize) -> Self {
        let mut finite: Vec<f64> = col.values.iter().copied().filter(|v| v.is_finite()).collect();
        finite.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        if finite.is_empty() {
            return Self { labels: vec![], group: vec![NO_GROUP; col.values.len()], continuous: true };
        }
        // Quantile edges, de-duplicated so a heavily tied column gets fewer bins.
        let mut edges: Vec<f64> = (0..=bins)
            .map(|i| finite[((finite.len() - 1) as f64 * i as f64 / bins as f64).round() as usize])
            .collect();
        edges.dedup();
        let n_bins = (edges.len() - 1).max(1);
        let bin_of = |v: f64| -> u16 {
            let i = edges[1..].iter().position(|&e| v <= e).unwrap_or(n_bins - 1);
            i.min(n_bins - 1) as u16
        };
        let mut labels: Vec<String> = (0..n_bins)
            .map(|i| {
                let hi = edges.get(i + 1).copied().unwrap_or(edges[0]);
                format!("{} – {}", fmt_num(edges[i]), fmt_num(hi))
            })
            .collect();
        let mut group: Vec<u16> = col.values.iter()
            .map(|&v| if v.is_finite() { bin_of(v) } else { NO_GROUP })
            .collect();
        Self::add_missing_group(&mut labels, &mut group, &col.values);
        Self { labels, group, continuous: true }
    }

    fn add_missing_group(labels: &mut Vec<String>, group: &mut [u16], values: &[f64]) {
        if values.iter().any(|v| !v.is_finite()) {
            let idx = labels.len() as u16;
            labels.push("(missing)".to_string());
            for (g, v) in group.iter_mut().zip(values) {
                if !v.is_finite() { *g = idx; }
            }
        }
    }

    /// Whether group `i` is the "(missing)" bucket.
    pub fn is_missing_group(&self, i: usize) -> bool {
        self.labels.get(i).is_some_and(|l| l == "(missing)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::eval::{CovTabData, CovTabRow, PredRow};

    fn pred(id: &str, time: f64) -> PredRow {
        PredRow { id: id.into(), time, dv: 1.0, pred: 1.0, ipred: 1.0, cwres: 0.1, iwres: 0.1, ebe_ofv: f64::NAN, tad: f64::NAN }
    }

    fn eval3() -> EvalData {
        let mut e = EvalData::from_rows(vec![pred("1", 0.5), pred("1", 1.0), pred("2", 0.5)]);
        e.extras = vec![("OCC".into(), vec!["1".into(), "2".into(), "1".into()])];
        e
    }

    fn strs(v: &[&str]) -> Vec<String> { v.iter().map(|s| s.to_string()).collect() }

    #[test]
    fn small_integer_columns_are_categorical_and_continuous_ones_numeric() {
        let cmt = VarColumn::from_strings("CMT", VarSource::Dataset, &strs(&["1", "2", "1", "."]));
        assert_eq!(cmt.auto_kind, VarKind::Categorical);
        assert_eq!(cmt.levels, vec![1.0, 2.0]);
        assert_eq!(cmt.n_missing(), 1);
        let wt = VarColumn::from_strings("WT", VarSource::Dataset, &strs(&["70.6", "55.2", "81.0"]));
        assert_eq!(wt.auto_kind, VarKind::Numeric);
        assert_eq!((wt.min, wt.max), (55.2, 81.0));
    }

    #[test]
    fn text_columns_become_labelled_categories() {
        let c = VarColumn::from_strings("SEX", VarSource::Dataset, &strs(&["M", "F", "M", ""]));
        assert!(c.is_text());
        assert_eq!(c.auto_kind, VarKind::Categorical);
        assert_eq!(c.level_label(c.values[0]), "M");
        assert_eq!(c.level_label(c.values[1]), "F");
        assert!(c.values[3].is_nan());
    }

    #[test]
    fn many_integer_levels_stay_numeric_but_can_be_forced_categorical() {
        let raw: Vec<String> = (0..50).map(|i| i.to_string()).collect();
        let c = VarColumn::from_strings("N", VarSource::Dataset, &raw);
        assert_eq!(c.auto_kind, VarKind::Numeric);
        assert!(c.can_be_categorical());
    }

    #[test]
    fn table_merges_sources_and_prefers_earlier_names() {
        let eval = eval3();
        let cov = CovTabData::from_rows(
            vec![
                CovTabRow { id: "1".into(), time: 0.5, values: [("WT".to_string(), 70.0)].into() },
                CovTabRow { id: "1".into(), time: 1.0, values: [("WT".to_string(), 70.0)].into() },
                CovTabRow { id: "2".into(), time: 0.5, values: [("WT".to_string(), 55.0)].into() },
            ],
            vec!["WT".into()],
        );
        let ds = vec![
            ("CMT".to_string(), strs(&["1", "1", "1"])),
            ("OCC".to_string(), strs(&["9", "9", "9"])),
        ];
        let t = VarTable::build(&eval, Some(&cov), Some(&ds));
        assert!(t.get("TIME").is_some() && t.get("WT").is_some() && t.get("CMT").is_some());
        // OCC from predictions.csv wins over the dataset's OCC.
        assert_eq!(t.get("OCC").unwrap().source, VarSource::Prediction);
        assert_eq!(t.get("OCC").unwrap().values, vec![1.0, 2.0, 1.0]);
        // All-missing bundle columns (TAD, EBE_OFV) are left out.
        assert!(t.get("TAD").is_none());
        // CMT is constant: still listed, but described as constant so the UI can grey it out.
        assert_eq!(VarTable::constant_label(t.get("CMT").unwrap()).as_deref(), Some("all 1"));
        assert_eq!(VarTable::constant_label(t.get("TIME").unwrap()), None);
    }

    #[test]
    fn filters_combine_with_and_and_handle_missing() {
        let eval = eval3();
        let ds = vec![("WT".to_string(), strs(&["70", ".", "55"]))];
        let t = VarTable::build(&eval, None, Some(&ds));
        let mut v = EvalView::default();
        v.filters.push(FilterClause::Levels { var: "OCC".into(), allowed: vec![1.0], keep_missing: false });
        assert_eq!(v.mask(&t), vec![true, false, true]);
        v.filters.push(FilterClause::Range { var: "WT".into(), lo: 60.0, hi: 90.0 });
        assert_eq!(v.mask(&t), vec![true, false, false]); // WT 55 out, WT missing out
        v.filters.push(FilterClause::Range { var: "GONE".into(), lo: 0.0, hi: 1.0 });
        assert_eq!(v.mask(&t), vec![true, false, false]); // unknown variable ignored
        v.reset();
        assert_eq!(v.mask(&t), vec![true; 3]);
    }

    #[test]
    fn keep_missing_levels_filter() {
        let eval = EvalData::from_rows(vec![pred("1", 1.0), pred("1", 2.0)]);
        let ds = vec![("C".to_string(), strs(&["1", "."]))];
        let t = VarTable::build(&eval, None, Some(&ds));
        let clause = FilterClause::Levels { var: "C".into(), allowed: vec![1.0], keep_missing: true };
        let v = EvalView { filters: vec![clause], ..Default::default() };
        assert_eq!(v.mask(&t), vec![true, true]);
    }

    #[test]
    fn categorical_grouping_labels_levels_and_adds_a_missing_bucket() {
        let c = VarColumn::from_strings("CMT", VarSource::Dataset, &strs(&["2", "1", ".", "2"]));
        let g = Grouping::new(&c, VarKind::Categorical);
        assert_eq!(g.labels, vec!["1", "2", "(missing)"]);
        assert_eq!(g.group, vec![1, 0, 2, 1]);
        assert!(g.is_missing_group(2) && !g.is_missing_group(0));
    }

    #[test]
    fn continuous_grouping_uses_quantile_bins() {
        let raw: Vec<String> = (1..=60).map(|i| i.to_string()).collect();
        let c = VarColumn::from_strings("WT", VarSource::Dataset, &raw);
        let g = Grouping::new(&c, VarKind::Numeric);
        assert!(g.continuous);
        assert_eq!(g.labels.len(), COLOR_BINS);
        // Lowest value in the first bin, highest in the last, roughly equal counts.
        assert_eq!(g.group[0], 0);
        assert_eq!(*g.group.last().unwrap(), (COLOR_BINS - 1) as u16);
        for b in 0..COLOR_BINS {
            let n = g.group.iter().filter(|&&x| x == b as u16).count();
            assert!((8..=12).contains(&n), "bin {b} has {n}");
        }
    }

    #[test]
    fn tied_values_collapse_bins_instead_of_creating_empty_ones() {
        let c = VarColumn::from_numeric("X", VarSource::Dataset, vec![1.0; 30]);
        let g = Grouping::new(&c, VarKind::Numeric);
        assert_eq!(g.labels.len(), 1);
        assert!(g.group.iter().all(|&x| x == 0));
    }

    #[test]
    fn kind_override_is_honoured_only_when_possible() {
        let eval = eval3();
        let ds = vec![
            ("SEX".to_string(), strs(&["M", "F", "M"])),
            ("WT".to_string(), strs(&["70", "55.5", "80"])),
        ];
        let t = VarTable::build(&eval, None, Some(&ds));
        let mut v = EvalView::default();
        v.kind_override.insert("SEX".into(), VarKind::Numeric);      // impossible: text
        v.kind_override.insert("WT".into(), VarKind::Categorical);   // possible: 3 levels
        assert_eq!(t.kind_of(&v, "SEX"), Some(VarKind::Categorical));
        assert_eq!(t.kind_of(&v, "WT"), Some(VarKind::Categorical));
        assert_eq!(t.kind_of(&v, "NOPE"), None);
    }

    #[test]
    fn y_transform_back_transforms_and_labels() {
        assert_eq!(YTransform::AsFitted.apply(2.0), 2.0);
        assert!((YTransform::Exp.apply(1.0) - std::f64::consts::E).abs() < 1e-12);
        assert_eq!(YTransform::Pow10.apply(2.0), 100.0);
        assert!(YTransform::Exp.apply(1e4).is_infinite()); // overflow is non-finite, so plots drop it
        assert_eq!(YTransform::Exp.wrap("DV"), "exp(DV)");
        assert_eq!(YTransform::Pow10.wrap("IPRED"), "10^IPRED");
        assert_eq!(YTransform::AsFitted.wrap("DV"), "DV");
    }

    #[test]
    fn number_formatting_is_compact() {
        assert_eq!(fmt_num(2.0), "2");
        assert_eq!(fmt_num(2.5), "2.5");
        assert_eq!(fmt_num(70.600), "70.6");
        assert_eq!(fmt_num(f64::NAN), "NA");
    }
}
