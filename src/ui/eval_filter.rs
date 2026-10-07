//! Filter / colour-by controls for the Evaluation → GOF plots, and the helpers the plots
//! use to turn a filtered, grouped set of rows into coloured point series.

use eframe::egui;
use egui_plot::MarkerShape;

use crate::domain::{
    DatasetStatus, EvalView, FilterClause, Grouping, VarColumn, VarKind, VarSource, VarTable,
    COLOR_BINS, NO_GROUP, fmt_num,
};
use crate::state::AppState;
use crate::app::theme;

/// Most levels a categorical variable may have and still be offered for colouring.
const MAX_COLOR_LEVELS: usize = 12;

/// One coloured set of points in a scatter plot.
pub struct Series {
    pub label: String,
    pub color: egui::Color32,
    pub shape: MarkerShape,
    pub points: Vec<[f64; 2]>,
}

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

/// Okabe-Ito colour-blind-safe qualitative palette.
const QUALITATIVE: [(u8, u8, u8); 8] = [
    (0x00, 0x72, 0xB2), (0xE6, 0x9F, 0x00), (0x00, 0x9E, 0x73), (0xD5, 0x5E, 0x00),
    (0x56, 0xB4, 0xE9), (0xCC, 0x79, 0xA7), (0xF0, 0xE4, 0x42), (0x99, 0x99, 0x99),
];
/// Sequential (viridis-like) ramp for binned continuous variables.
const SEQUENTIAL: [(u8, u8, u8); COLOR_BINS] = [
    (0x44, 0x01, 0x54), (0x41, 0x44, 0x87), (0x2A, 0x78, 0x8E),
    (0x22, 0xA8, 0x84), (0x7A, 0xD1, 0x51), (0xD8, 0xC9, 0x1E),
];
const MARKERS: [MarkerShape; 6] = [
    MarkerShape::Circle, MarkerShape::Diamond, MarkerShape::Square,
    MarkerShape::Up, MarkerShape::Cross, MarkerShape::Down,
];

pub fn group_color(i: usize, n: usize, continuous: bool, missing: bool, dark: bool) -> egui::Color32 {
    let alpha = if dark { 205 } else { 190 };
    let (r, g, b) = if missing {
        (0x80, 0x80, 0x80)
    } else if continuous {
        let k = if n <= 1 { 0 } else { i * (COLOR_BINS - 1) / (n - 1) };
        SEQUENTIAL[k.min(COLOR_BINS - 1)]
    } else {
        QUALITATIVE[i % QUALITATIVE.len()]
    };
    egui::Color32::from_rgba_unmultiplied(r, g, b, alpha)
}

pub fn group_marker(i: usize, continuous: bool, missing: bool) -> MarkerShape {
    // Continuous bins are ordered, so one shape; categories vary shape as a second cue.
    if continuous || missing { MarkerShape::Circle } else { MARKERS[i % MARKERS.len()] }
}

/// Everything a plot needs to know about the current filter / colour settings.
pub struct GofView {
    pub mask: Vec<bool>,
    pub grouping: Option<Grouping>,
    pub per_group_loess: bool,
    pub dark: bool,
}

impl GofView {
    /// Plain view: nothing filtered, one colour.
    pub fn plain(n_rows: usize, dark: bool) -> Self {
        Self { mask: vec![true; n_rows], grouping: None, per_group_loess: false, dark }
    }

    pub fn from_settings(view: &EvalView, table: Option<&VarTable>, n_rows: usize, dark: bool) -> Self {
        let Some(table) = table.filter(|t| t.n_rows == n_rows) else { return Self::plain(n_rows, dark) };
        let grouping = view.color_by.as_deref().and_then(|name| {
            let col = table.get(name)?;
            Some(Grouping::new(col, table.kind_of(view, name)?))
        });
        Self { mask: view.mask(table), grouping, per_group_loess: view.loess_per_group, dark }
    }

    pub fn is_kept(&self, row: usize) -> bool {
        self.mask.get(row).copied().unwrap_or(true)
    }

    pub fn n_kept(&self) -> usize {
        self.mask.iter().filter(|&&k| k).count()
    }

    /// Splits `(row index, [x, y])` points into coloured series. Rows removed by the
    /// filters are dropped. Without a colour variable everything is one series.
    pub fn series(&self, pts: impl Iterator<Item = (usize, [f64; 2])>, default_color: egui::Color32) -> Vec<Series> {
        let Some(g) = &self.grouping else {
            let points: Vec<[f64; 2]> = pts.filter(|(i, _)| self.is_kept(*i)).map(|(_, p)| p).collect();
            return vec![Series { label: String::new(), color: default_color, shape: MarkerShape::Circle, points }];
        };
        let n = g.labels.len();
        let mut buckets: Vec<Vec<[f64; 2]>> = vec![Vec::new(); n];
        for (i, p) in pts {
            if !self.is_kept(i) { continue; }
            let gi = g.group.get(i).copied().unwrap_or(NO_GROUP);
            if (gi as usize) < n { buckets[gi as usize].push(p); }
        }
        let n_real = if g.labels.last().is_some_and(|l| l == "(missing)") { n - 1 } else { n };
        buckets.into_iter().enumerate()
            .filter(|(_, pts)| !pts.is_empty())
            .map(|(k, points)| {
                let missing = g.is_missing_group(k);
                Series {
                    label: g.labels[k].clone(),
                    color: group_color(k, n_real, g.continuous, missing, self.dark),
                    shape: group_marker(k, g.continuous, missing),
                    points,
                }
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Controls
// ---------------------------------------------------------------------------

fn clause_label(clause: &FilterClause, col: Option<&VarColumn>) -> String {
    match (clause, col) {
        (FilterClause::Levels { var, allowed, keep_missing }, Some(c)) => {
            let total = c.levels.len();
            let mut names: Vec<String> = allowed.iter().map(|&v| c.level_label(v)).collect();
            if *keep_missing && c.n_missing() > 0 { names.push("NA".into()); }
            let body = if allowed.len() == total { "all".to_string() }
                       else if names.len() > 3 { format!("{} of {total}", allowed.len()) }
                       else { names.join(", ") };
            format!("{var}: {body}")
        }
        (FilterClause::Range { var, lo, hi }, _) => format!("{var}: {} – {}", fmt_num(*lo), fmt_num(*hi)),
        (FilterClause::Levels { var, .. }, None) => var.clone(),
    }
}

/// Variables offered as filter targets / colour variables, grouped by origin.
fn grouped_vars(table: &VarTable) -> Vec<(VarSource, Vec<&VarColumn>)> {
    [VarSource::Prediction, VarSource::Covariate, VarSource::Dataset].into_iter()
        .map(|src| (src, table.columns.iter().filter(|c| c.source == src).collect::<Vec<_>>()))
        .filter(|(_, v)| !v.is_empty())
        .collect()
}

fn colorable(table: &VarTable, view: &EvalView, c: &VarColumn) -> bool {
    match table.kind_of(view, &c.name) {
        Some(VarKind::Categorical) => (2..=MAX_COLOR_LEVELS).contains(&c.levels.len()),
        Some(VarKind::Numeric) => c.max > c.min,
        None => false,
    }
}

/// Editor shown inside a chip's popup. Returns true when the clause changed.
fn edit_clause(ui: &mut egui::Ui, table: &VarTable, view_kinds: &mut EvalView, idx: usize) -> bool {
    let Some(var) = view_kinds.filters.get(idx).map(|c| c.var().to_string()) else { return false };
    let Some(col) = table.get(&var) else { return false };
    let mut changed = false;
    let dark = ui.visuals().dark_mode;

    // Switch between category checklist and range, when the column allows both.
    let kind = table.kind_of(view_kinds, &var).unwrap_or(col.auto_kind);
    if col.can_be_categorical() && !col.is_text() {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Treat as:").color(theme::fg2(dark)).size(11.0));
            for (label, k) in [("Categories", VarKind::Categorical), ("Range", VarKind::Numeric)] {
                if ui.selectable_label(kind == k, label).clicked() && kind != k {
                    view_kinds.kind_override.insert(var.clone(), k);
                    if let Some(c) = view_kinds.default_clause(table, &var) {
                        view_kinds.filters[idx] = c;
                    }
                    changed = true;
                }
            }
        });
        ui.separator();
    }

    match &mut view_kinds.filters[idx] {
        FilterClause::Levels { allowed, keep_missing, .. } => {
            ui.horizontal(|ui| {
                if ui.small_button("All").clicked() { *allowed = col.levels.clone(); *keep_missing = true; changed = true; }
                if ui.small_button("None").clicked() { allowed.clear(); *keep_missing = false; changed = true; }
            });
            egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                for &lv in &col.levels {
                    let mut on = allowed.contains(&lv);
                    if ui.checkbox(&mut on, col.level_label(lv)).changed() {
                        if on { allowed.push(lv); } else { allowed.retain(|&a| a != lv); }
                        changed = true;
                    }
                }
                if col.n_missing() > 0 && ui.checkbox(keep_missing, "(missing)").changed() {
                    changed = true;
                }
            });
        }
        FilterClause::Range { lo, hi, .. } => {
            let speed = ((col.max - col.min) / 200.0).max(1e-9);
            ui.horizontal(|ui| {
                changed |= ui.add(egui::DragValue::new(lo).range(col.min..=col.max).speed(speed)).changed();
                ui.label("to");
                changed |= ui.add(egui::DragValue::new(hi).range(col.min..=col.max).speed(speed)).changed();
            });
            if *lo > *hi { std::mem::swap(lo, hi); }
            if ui.small_button("Full range").clicked() { *lo = col.min; *hi = col.max; changed = true; }
            if col.n_missing() > 0 {
                ui.label(egui::RichText::new(format!("{} rows with a missing value are excluded", col.n_missing()))
                    .color(theme::fg3(dark)).size(10.0));
            }
        }
    }
    changed
}

/// Second toolbar row for the GOF section: filters, colour-by, LOESS mode, counters and
/// the dataset badge.
pub fn show_gof_controls(ui: &mut egui::Ui, state: &mut AppState, dark: bool) {
    let Some(stem) = state.ui.eval_loaded_stem.clone() else { return };
    let n_rows = state.ui.eval_data.as_ref().map_or(0, |d| d.rows.len());
    if n_rows == 0 { return; }

    let table = state.ui.eval_vars.take();
    let mut view = state.ui.eval_views.remove(&stem).unwrap_or_default();
    let status = state.ui.eval_dataset_status.clone();
    let mut locate = false;
    let dim = theme::fg2(dark);

    if let Some(table) = &table {
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            // ── Add filter ──
            ui.menu_button(egui::RichText::new("+ Filter").size(11.0), |ui| {
                ui.set_min_width(170.0);
                if !matches!(status, DatasetStatus::Aligned { .. }) {
                    ui.label(egui::RichText::new("Dataset columns (CMT, MDV, …) are unavailable: see the note below the controls.")
                        .color(theme::ORANGE).size(10.0));
                    ui.add_space(4.0);
                }
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    for (src, cols) in grouped_vars(table) {
                        ui.label(egui::RichText::new(src.label()).color(theme::fg3(dark)).size(10.0));
                        for c in cols {
                            let used = view.filters.iter().any(|f| f.var() == c.name);
                            let constant = VarTable::constant_label(c);
                            let text = match &constant {
                                Some(l) => format!("{}  ({l})", c.name),
                                None => c.name.clone(),
                            };
                            let resp = ui.add_enabled(!used && constant.is_none(), egui::Button::new(text).frame(false));
                            let resp = if constant.is_some() {
                                resp.on_disabled_hover_text("Every observation has the same value, so there is nothing to filter")
                            } else { resp };
                            if resp.clicked() {
                                if let Some(clause) = view.default_clause(table, &c.name) {
                                    view.filters.push(clause);
                                }
                                ui.close_menu();
                            }
                        }
                        ui.add_space(4.0);
                    }
                });
            });

            // ── Active filter chips ──
            let mut remove: Option<usize> = None;
            for idx in 0..view.filters.len() {
                let label = clause_label(&view.filters[idx], table.get(view.filters[idx].var()));
                egui::Frame::new()
                    .fill(theme::card_fill(dark))
                    .corner_radius(egui::CornerRadius::same(10))
                    .inner_margin(egui::Margin::symmetric(6, 1))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.menu_button(egui::RichText::new(label).size(11.0).color(theme::fg(dark)), |ui| {
                            ui.set_min_width(190.0);
                            edit_clause(ui, table, &mut view, idx);
                        });
                        if ui.add(egui::Button::new(egui::RichText::new("✖").size(10.0).color(dim)).frame(false))
                            .on_hover_text("Remove this filter").clicked()
                        {
                            remove = Some(idx);
                        }
                    });
            }
            if let Some(i) = remove { view.filters.remove(i); }

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            // ── Colour by ──
            ui.label(egui::RichText::new("Color by:").color(dim).size(11.0));
            let cur = view.color_by.clone().unwrap_or_else(|| "None".into());
            egui::ComboBox::from_id_salt("gof_color_by")
                .selected_text(&cur)
                .width(90.0)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(view.color_by.is_none(), "None").clicked() {
                        view.color_by = None;
                    }
                    for (src, cols) in grouped_vars(table) {
                        ui.label(egui::RichText::new(src.label()).color(theme::fg3(dark)).size(10.0));
                        let (usable, other): (Vec<&VarColumn>, Vec<&VarColumn>) =
                            cols.into_iter().partition(|c| colorable(table, &view, c));
                        for c in usable {
                            if ui.selectable_label(view.color_by.as_deref() == Some(c.name.as_str()), &c.name).clicked() {
                                view.color_by = Some(c.name.clone());
                            }
                        }
                        for c in other {
                            let why = match VarTable::constant_label(c) {
                                Some(l) => l,
                                None => format!("{} levels, too many to colour", c.levels.len()),
                            };
                            ui.add_enabled(false, egui::Button::new(format!("{}  ({why})", c.name)).frame(false));
                        }
                    }
                });
            let categorical_color = view.color_by.as_deref()
                .and_then(|n| table.kind_of(&view, n)) == Some(VarKind::Categorical);
            if categorical_color {
                ui.checkbox(&mut view.loess_per_group, "LOESS per group")
                    .on_hover_text("One LOESS line per colour group (groups with fewer than 8 points are skipped)");
            }

            if (view.is_filtered() || view.color_by.is_some())
                && ui.add(egui::Button::new(egui::RichText::new("Reset").size(11.0)).frame(false)).clicked()
            {
                view.reset();
            }
        });
    }

    // ── Counter, dataset badge, colour legend ──
    let gv = GofView::from_settings(&view, table.as_ref(), n_rows, dark);
    ui.horizontal_wrapped(|ui| {
        let kept = gv.n_kept();
        let txt = if kept == n_rows { format!("n = {n_rows} obs") } else { format!("n = {kept} of {n_rows} obs") };
        ui.label(egui::RichText::new(txt).color(if kept == 0 { theme::RED } else { dim }).size(11.0));
        ui.add_space(8.0);
        match &status {
            DatasetStatus::Aligned { name, rows } => {
                ui.label(egui::RichText::new(format!("✔ Dataset {name} matched ({rows} rows)"))
                    .color(theme::fg3(dark)).size(10.5));
            }
            DatasetStatus::NoPath | DatasetStatus::NotFound(_) | DatasetStatus::Failed(_) => {
                let msg = match &status {
                    DatasetStatus::NoPath => "⚠ No dataset path recorded; dataset columns unavailable".to_string(),
                    DatasetStatus::NotFound(p) => format!("⚠ Dataset not found ({p}); dataset columns unavailable"),
                    DatasetStatus::Failed(e) => format!("⚠ Dataset not usable: {e}"),
                    _ => String::new(),
                };
                ui.label(egui::RichText::new(msg).color(theme::ORANGE).size(10.5));
                if ui.small_button("Locate dataset…").clicked() { locate = true; }
            }
            DatasetStatus::Unknown => {}
        }
    });

    if let (Some(g), Some(name), Some(table)) = (&gv.grouping, view.color_by.clone(), &table) {
        legend_row(ui, g, &name, table, &mut view, dark);
    }

    state.ui.eval_vars = table;
    state.ui.eval_views.insert(stem.clone(), view);

    if locate {
        if let Some(p) = rfd::FileDialog::new().add_filter("Dataset", &["csv", "txt", "dat"]).pick_file() {
            state.ui.eval_dataset_override.insert(stem, p);
            state.ui.eval_loaded_stem = None; // reload with the chosen dataset
        }
    }
}

/// Shared colour legend. Clicking a category hides or shows it by editing that variable's filter.
fn legend_row(ui: &mut egui::Ui, g: &Grouping, var: &str, table: &VarTable, view: &mut EvalView, dark: bool) {
    let n_real = if g.labels.last().is_some_and(|l| l == "(missing)") { g.labels.len() - 1 } else { g.labels.len() };
    let col = table.get(var);
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(format!("{var}:")).color(theme::fg2(dark)).size(11.0));
        for (k, label) in g.labels.iter().enumerate() {
            let missing = g.is_missing_group(k);
            let color = group_color(k, n_real, g.continuous, missing, dark);
            // For categories, "shown" means the level is allowed by this variable's own filter.
            let shown = match (view.filters.iter().find(|f| f.var() == var), col, g.continuous) {
                (Some(FilterClause::Levels { allowed, keep_missing, .. }), Some(c), false) => {
                    if missing { *keep_missing } else { c.levels.get(k).is_some_and(|l| allowed.contains(l)) }
                }
                _ => true,
            };
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::click());
            let fill = if shown { color } else { egui::Color32::TRANSPARENT };
            ui.painter().circle(rect.center(), 4.5, fill, egui::Stroke::new(1.2, color));
            let text = egui::RichText::new(label).size(11.0)
                .color(if shown { theme::fg(dark) } else { theme::fg3(dark) });
            let lresp = ui.add(egui::Label::new(text).sense(if g.continuous { egui::Sense::hover() } else { egui::Sense::click() }));
            if !g.continuous && (resp.clicked() || lresp.clicked()) {
                toggle_level(view, table, var, k, missing);
            }
            ui.add_space(6.0);
        }
    });
}

fn toggle_level(view: &mut EvalView, table: &VarTable, var: &str, k: usize, missing: bool) {
    if !view.filters.iter().any(|f| f.var() == var) {
        if let Some(c) = view.default_clause(table, var) { view.filters.push(c); }
    }
    let Some(col) = table.get(var) else { return };
    if let Some(FilterClause::Levels { allowed, keep_missing, .. }) = view.filters.iter_mut().find(|f| f.var() == var) {
        if missing {
            *keep_missing = !*keep_missing;
        } else if let Some(&lv) = col.levels.get(k) {
            if allowed.contains(&lv) { allowed.retain(|&a| a != lv); } else { allowed.push(lv); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{EvalData, PredRow, VarKind};

    fn pred(id: &str, t: f64) -> PredRow {
        PredRow { id: id.into(), time: t, dv: 1.0, pred: 1.0, ipred: 1.0, cwres: 0.0, iwres: 0.0, ebe_ofv: f64::NAN, tad: f64::NAN }
    }

    fn table() -> VarTable {
        let eval = EvalData::from_rows(vec![pred("1", 0.5), pred("1", 1.0), pred("2", 0.5), pred("2", 1.0)]);
        let ds = vec![("CMT".to_string(), vec!["1".to_string(), "2".into(), "1".into(), "2".into()])];
        VarTable::build(&eval, None, Some(&ds))
    }

    #[test]
    fn plain_view_keeps_everything_in_one_series() {
        let gv = GofView::plain(3, true);
        let s = gv.series((0..3).map(|i| (i, [i as f64, 1.0])), egui::Color32::WHITE);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].points.len(), 3);
    }

    #[test]
    fn filters_and_colour_split_points_by_group() {
        let t = table();
        let mut v = EvalView { color_by: Some("CMT".into()), ..Default::default() };
        let gv = GofView::from_settings(&v, Some(&t), 4, false);
        let s = gv.series((0..4).map(|i| (i, [i as f64, 0.0])), egui::Color32::WHITE);
        assert_eq!(s.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(), vec!["1", "2"]);
        assert_eq!(s[0].points, vec![[0.0, 0.0], [2.0, 0.0]]);
        assert_ne!(s[0].color, s[1].color);
        assert_ne!(format!("{:?}", s[0].shape), format!("{:?}", s[1].shape));
        // Hide CMT 2 through a filter: its series disappears, n_kept drops.
        v.filters.push(FilterClause::Levels { var: "CMT".into(), allowed: vec![1.0], keep_missing: false });
        let gv = GofView::from_settings(&v, Some(&t), 4, false);
        assert_eq!(gv.n_kept(), 2);
        let s = gv.series((0..4).map(|i| (i, [i as f64, 0.0])), egui::Color32::WHITE);
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn a_stale_table_is_ignored_instead_of_misaligning_rows() {
        let t = table();
        let gv = GofView::from_settings(&EvalView::default(), Some(&t), 99, false);
        assert_eq!(gv.mask.len(), 99);
        assert!(gv.grouping.is_none());
    }

    #[test]
    fn legend_toggle_creates_and_edits_the_variables_filter() {
        let t = table();
        let mut v = EvalView::default();
        toggle_level(&mut v, &t, "CMT", 1, false); // hide level index 1 (CMT = 2)
        assert_eq!(v.filters, vec![FilterClause::Levels { var: "CMT".into(), allowed: vec![1.0], keep_missing: true }]);
        toggle_level(&mut v, &t, "CMT", 1, false); // show it again
        assert_eq!(v.mask(&t), vec![true; 4]);
    }

    #[test]
    fn only_reasonable_variables_are_colourable() {
        let t = table();
        let v = EvalView::default();
        assert!(colorable(&t, &v, t.get("CMT").unwrap()));
        assert!(colorable(&t, &v, t.get("TIME").unwrap()));
        // ID has two levels here so it colours; a forced-categorical wide column would not.
        let many: Vec<String> = (0..40).map(|i| i.to_string()).collect();
        let eval = EvalData::from_rows((0..40).map(|i| pred("1", i as f64)).collect());
        let t2 = VarTable::build(&eval, None, Some(&[("N".to_string(), many)]));
        let mut v2 = EvalView::default();
        v2.kind_override.insert("N".into(), VarKind::Categorical);
        assert!(!colorable(&t2, &v2, t2.get("N").unwrap()));
    }

    #[test]
    fn sequential_ramp_spreads_over_fewer_bins() {
        let first = group_color(0, 3, true, false, true);
        let last = group_color(2, 3, true, false, true);
        assert_ne!(first, last);
        assert_eq!(group_color(0, 1, true, false, true), first);
    }
}

#[cfg(test)]
mod ui_tests {
    use super::*;
    use crate::domain::{EvalData, PredRow, VarTable};
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    fn pred(id: &str, t: f64) -> PredRow {
        PredRow { id: id.into(), time: t, dv: 1.0, pred: 1.0, ipred: 1.0, cwres: 0.0, iwres: 0.0, ebe_ofv: f64::NAN, tad: f64::NAN }
    }

    fn state_with(view: EvalView, status: DatasetStatus) -> AppState {
        let eval = EvalData::from_rows(vec![pred("1", 0.5), pred("1", 1.0), pred("2", 0.5)]);
        let ds = vec![("CMT".to_string(), vec!["1".to_string(), "2".into(), "1".into()])];
        let mut st = AppState::new();
        st.ui.eval_vars = Some(VarTable::build(&eval, None, Some(&ds)));
        st.ui.eval_data = Some(eval);
        st.ui.eval_loaded_stem = Some("m".into());
        st.ui.eval_dataset_status = status;
        st.ui.eval_views.insert("m".into(), view);
        st
    }

    #[test]
    fn controls_show_filter_chip_counter_legend_and_dataset_badge() {
        let view = EvalView {
            filters: vec![FilterClause::Levels { var: "CMT".into(), allowed: vec![1.0], keep_missing: false }],
            color_by: Some("CMT".into()),
            ..Default::default()
        };
        let mut st = state_with(view, DatasetStatus::Aligned { name: "d.csv".into(), rows: 3 });
        let mut h = Harness::new_ui(move |ui| show_gof_controls(ui, &mut st, true));
        h.run();
        assert!(h.query_by_label_contains("+ Filter").is_some());
        assert!(h.query_by_label_contains("CMT: 1").is_some(), "filter chip");
        assert!(h.query_by_label_contains("n = 2 of 3 obs").is_some(), "counter");
        assert!(h.query_by_label_contains("Dataset d.csv matched").is_some(), "badge");
        assert!(h.query_by_label_contains("Color by:").is_some());
        assert!(h.query_by_label_contains("Reset").is_some());
    }

    #[test]
    fn missing_dataset_offers_to_locate_it() {
        let mut st = state_with(EvalView::default(), DatasetStatus::NotFound("/gone/d.csv".into()));
        let mut h = Harness::new_ui(move |ui| show_gof_controls(ui, &mut st, false));
        h.run();
        assert!(h.query_by_label_contains("Dataset not found").is_some());
        assert!(h.query_by_label_contains("Locate dataset").is_some());
        assert!(h.query_by_label_contains("Reset").is_none(), "nothing to reset yet");
        assert!(h.query_by_label_contains("n = 3 obs").is_some());
    }
}
