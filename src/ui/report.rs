/// Run Report pill — a single scrollable document summarising a completed fit.
///
/// Shows all information a pharmacometrician needs to assess a model:
/// metadata, parameter estimates (as estimated), diagnostics, parameter
/// correlation matrix, and warnings.
///
/// An "Export HTML" button writes a self-contained, timestamped HTML file
/// to the model directory — suitable for inclusion in a modelling report.

use eframe::egui;

use crate::app::theme;
use crate::domain::{FitSummary, ModelEntry};
use crate::domain::run_record::RunRecord;
use crate::state::AppState;
use crate::workers::run::{now_iso, now_unix};

// ── Public entry point ────────────────────────────────────────────────────────

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;
    let Some(idx) = state.ui.selected_model else { return };
    let entry = &state.workspace.models[idx];

    let fit = match &entry.fit {
        Some(f) => f.clone(),
        None => {
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new("No run output yet.\nRun the model to generate a report.")
                    .color(theme::fg3(dark)).size(13.0));
            });
            return;
        }
    };

    // Find the most recent RunRecord for this model.
    let run_record = state.run.run_history.iter().rev()
        .find(|r| r.model_stem == entry.model.stem)
        .cloned();
    let stem = entry.model.stem.clone();

    // Header row: title + export button.
    let mut do_export = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("Run Report — {stem}"))
            .size(13.0).strong().color(theme::fg(dark)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(
                egui::Button::new(egui::RichText::new("Export HTML").color(egui::Color32::WHITE))
                    .fill(theme::ACCENT)
                    .min_size(egui::vec2(0.0, 24.0)),
            ).clicked() {
                do_export = true;
            }
        });
    });
    if do_export { export_html(state, idx); return; }
    ui.separator();

    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            show_run_summary(ui, &fit, entry, run_record.as_ref(), dark);
            ui.add_space(10.0);
            show_files_section(ui, entry, run_record.as_ref(), dark);
            ui.add_space(10.0);
            show_theta_section(ui, &fit, dark);
            ui.add_space(6.0);
            show_omega_section(ui, &fit, dark);
            if fit.n_kappa > 0 { ui.add_space(6.0); show_kappa_section(ui, &fit, dark); }
            ui.add_space(6.0);
            show_sigma_section(ui, &fit, dark);
            ui.add_space(10.0);
            show_diagnostics_section(ui, &fit, dark);
            ui.add_space(10.0);
            show_param_corr_section(ui, &fit, dark);
            ui.add_space(10.0);
            show_warnings_section(ui, &fit, dark);
        });
}

// ── Section renderers ─────────────────────────────────────────────────────────

fn show_run_summary(
    ui: &mut egui::Ui,
    fit: &FitSummary,
    _entry: &ModelEntry,
    rr: Option<&RunRecord>,
    dark: bool,
) {
    section_header(ui, "Run Summary", dark);
    egui::Grid::new("rpt_summary")
        .num_columns(4)
        .spacing([20.0, 4.0])
        .show(ui, |ui| {
            kv(ui, "OFV",   &fmt_f64_3dp(fit.ofv),   dark);
            kv(ui, "AIC",   &fmt_f64_1dp(fit.aic),   dark);
            kv(ui, "BIC",   &fmt_f64_1dp(fit.bic),   dark);
            kv(ui, "Observations", &fit.n_obs.to_string(), dark);
            ui.end_row();
            let cov_str = if fit.covariance_ok { "✓ computed" } else { "✗ not computed" };
            let cov_col = if fit.covariance_ok { theme::GREEN } else { theme::RED };
            kv_col(ui, "Covariance", cov_str, cov_col, dark);
            let cn_str = if fit.cov_condition_number.is_finite() {
                if fit.cn_high() {
                    format!("{:.1}  ⚠ High — potential overparameterisation",
                            fit.cov_condition_number)
                } else {
                    format!("{:.1}", fit.cov_condition_number)
                }
            } else { "—".to_string() };
            let cn_col = if fit.cn_high() { theme::ORANGE } else { theme::fg(dark) };
            kv_col(ui, "Condition Number", &cn_str, cn_col, dark);
            let conv_str = if fit.converged { "✓ converged" } else { "✗ not converged" };
            let conv_col = if fit.converged { theme::GREEN } else { theme::RED };
            kv_col(ui, "Convergence", conv_str, conv_col, dark);
            kv(ui, "Subjects", &fit.n_subjects.to_string(), dark);
            ui.end_row();
            let method = if fit.method_chain.is_empty() {
                fit.method.clone()
            } else {
                fit.method_chain.join(" + ")
            };
            kv(ui, "Method", &method, dark);
            kv(ui, "Parameters", &fit.n_parameters.to_string(), dark);
            kv(ui, "Iterations", &fit.n_iterations.to_string(), dark);
            let wt = if fit.wall_time_secs > 0.0 { fmt_duration(fit.wall_time_secs) } else { "—".into() };
            kv(ui, "Wall time", &wt, dark);
            ui.end_row();
            if let Some(rr) = rr {
                kv(ui, "Started",   &rr.started, dark);
                kv(ui, "Completed", rr.completed.as_deref().unwrap_or("—"), dark);
            }
        });
}

fn show_files_section(
    ui: &mut egui::Ui,
    entry: &ModelEntry,
    rr: Option<&RunRecord>,
    dark: bool,
) {
    section_header(ui, "Files", dark);
    let path_col = theme::fg2(dark);
    egui::Grid::new("rpt_files").num_columns(2).spacing([12.0, 3.0]).show(ui, |ui| {
        ui.label(egui::RichText::new("Model").color(theme::fg2(dark)).size(11.0).strong());
        ui.label(egui::RichText::new(entry.model.path.to_string_lossy().as_ref())
            .color(path_col).size(11.0).monospace());
        ui.end_row();
        if let Some(rr) = rr {
            if let Some(dp) = &rr.data_path {
                ui.label(egui::RichText::new("Data").color(theme::fg2(dark)).size(11.0).strong());
                ui.label(egui::RichText::new(dp.to_string_lossy().as_ref())
                    .color(path_col).size(11.0).monospace());
                ui.end_row();
            }
        }
        if let Some(fp) = &entry.fitrx_path {
            ui.label(egui::RichText::new("Output (.fitrx)").color(theme::fg2(dark)).size(11.0).strong());
            ui.label(egui::RichText::new(fp.to_string_lossy().as_ref())
                .color(path_col).size(11.0).monospace());
            ui.end_row();
        }
    });
}

fn show_theta_section(ui: &mut egui::Ui, fit: &FitSummary, dark: bool) {
    section_header(ui, &format!("Fixed Effects — THETA  ({})", fit.theta.len()), dark);
    if fit.theta.is_empty() { return; }
    param_table(ui, "rpt_theta", 6, dark, |ui| {
        for h in ["PARAM", "ESTIMATE", "SE", "RSE%", "95% CI LO", "95% CI HI"] {
            ui.label(egui::RichText::new(h).size(10.0).color(theme::fg3(dark)).strong());
        }
        ui.end_row();
        for i in 0..fit.theta.len() {
            let name = fit.theta_names.get(i).cloned()
                .unwrap_or_else(|| format!("THETA{}", i + 1));
            let est = fit.theta.get(i).copied().unwrap_or(f64::NAN);
            let se  = fit.se_theta.get(i).copied().unwrap_or(f64::NAN);
            let at_b = fit.at_lower_bound.get(i).copied().unwrap_or(false);
            param_row_fixed(ui, &name, est, se, at_b, dark);
        }
    });
}

fn show_omega_section(ui: &mut egui::Ui, fit: &FitSummary, dark: bool) {
    section_header(ui, &format!("Between-Subject Variability — OMEGA  ({} ETA)", fit.n_eta), dark);
    if fit.n_eta == 0 { return; }
    param_table(ui, "rpt_omega", 7, dark, |ui| {
        for h in ["PARAM", "ESTIMATE", "SE", "RSE%", "95% CI LO", "95% CI HI", "SHRINKAGE%"] {
            ui.label(egui::RichText::new(h).size(10.0).color(theme::fg3(dark)).strong());
        }
        ui.end_row();
        for i in 0..fit.n_eta {
            let name = fit.omega_names.get(i).cloned()
                .unwrap_or_else(|| format!("OMEGA({},{})", i + 1, i + 1));
            let est  = fit.omega_value(i, i).unwrap_or(f64::NAN);
            let se   = fit.se_omega.get(i).copied().unwrap_or(f64::NAN);
            let shrink = fit.eta_shrinkage.get(i).copied();
            param_row_omega(ui, &name, est, se, shrink, dark);
        }
    });
}

fn show_kappa_section(ui: &mut egui::Ui, fit: &FitSummary, dark: bool) {
    section_header(ui, &format!("Inter-Occasion Variability — KAPPA  ({})", fit.n_kappa), dark);
    param_table(ui, "rpt_kappa", 6, dark, |ui| {
        for h in ["PARAM", "ESTIMATE", "SE", "RSE%", "95% CI LO", "95% CI HI"] {
            ui.label(egui::RichText::new(h).size(10.0).color(theme::fg3(dark)).strong());
        }
        ui.end_row();
        for i in 0..fit.n_kappa {
            let name = fit.kappa_names.get(i).cloned()
                .unwrap_or_else(|| format!("KAPPA{}", i + 1));
            let est = fit.kappa_value(i, i).unwrap_or(f64::NAN);
            let se  = fit.se_kappa.get(i).copied().unwrap_or(f64::NAN);
            param_row_fixed(ui, &name, est, se, false, dark);
        }
    });
}

fn show_sigma_section(ui: &mut egui::Ui, fit: &FitSummary, dark: bool) {
    section_header(ui, &format!("Residual Error — SIGMA  ({})", fit.sigma.len()), dark);
    if fit.sigma.is_empty() { return; }
    param_table(ui, "rpt_sigma", 7, dark, |ui| {
        for h in ["PARAM", "ESTIMATE", "SE", "RSE%", "95% CI LO", "95% CI HI", "SHRINKAGE%"] {
            ui.label(egui::RichText::new(h).size(10.0).color(theme::fg3(dark)).strong());
        }
        ui.end_row();
        for i in 0..fit.sigma.len() {
            let name = fit.sigma_names.get(i).cloned()
                .unwrap_or_else(|| format!("SIGMA{}", i + 1));
            let est    = fit.sigma.get(i).copied().unwrap_or(f64::NAN);
            let se     = fit.se_sigma.get(i).copied().unwrap_or(f64::NAN);
            let shrink = fit.eps_shrinkage.get(i).copied();
            param_row_omega(ui, &name, est, se, shrink, dark);
        }
    });
}

fn show_diagnostics_section(ui: &mut egui::Ui, fit: &FitSummary, dark: bool) {
    section_header(ui, "Diagnostics", dark);
    let dim = theme::fg3(dark);

    if let Some(dw) = fit.dw_statistic {
        let ok  = dw >= 1.5 && dw <= 2.5;
        let col = if ok { theme::GREEN } else { theme::ORANGE };
        let verdict = if dw < 1.5 {
            "positive autocorrelation"
        } else if dw > 2.5 {
            "negative autocorrelation"
        } else {
            "acceptable — no autocorrelation"
        };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Durbin-Watson").color(theme::fg2(dark)).size(11.0));
            ui.label(egui::RichText::new(format!("{dw:.3}")).color(col).size(11.0).strong());
            ui.label(egui::RichText::new(format!("({verdict})")).color(col).size(11.0));
        });
        ui.label(egui::RichText::new(
            "Tests for temporal autocorrelation in IWRES. \
             Computed as a pooled statistic across all subjects. \
             Acceptable range: 1.5 – 2.5 (near 2.0 = independent residuals). \
             Values outside this range suggest model misspecification — \
             check the structural model or residual error model.")
            .color(dim).size(10.0).italics());
        ui.add_space(6.0);
    }

    if let Some(r) = fit.iwres_lag1_r {
        let flagged = r.abs() > 0.2;
        let col     = if flagged { theme::ORANGE } else { theme::GREEN };
        let verdict = if flagged { "autocorrelation detected" } else { "acceptable" };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("IWRES lag-1 r").color(theme::fg2(dark)).size(11.0));
            ui.label(egui::RichText::new(format!("{r:.3}")).color(col).size(11.0).strong());
            ui.label(egui::RichText::new(format!("({verdict})")).color(col).size(11.0));
        });
        ui.label(egui::RichText::new(
            "Pearson correlation between consecutive IWRES within each subject (lag 1). \
             Complements the Durbin-Watson statistic. \
             Near zero = residuals are independent. \
             |r| > 0.2 flags autocorrelation worth investigating.")
            .color(dim).size(10.0).italics());
        ui.add_space(6.0);
    }

    // Shrinkage table.
    if !fit.eta_shrinkage.is_empty() || !fit.eps_shrinkage.is_empty() {
        ui.add_space(6.0);
        ui.label(egui::RichText::new("Shrinkage").color(theme::fg2(dark)).size(11.0).strong());
        egui::Grid::new("rpt_shrink").num_columns(3).spacing([16.0, 3.0]).show(ui, |ui| {
            for h in ["PARAMETER", "TYPE", "SHRINKAGE%"] {
                ui.label(egui::RichText::new(h).size(10.0).color(theme::fg3(dark)).strong());
            }
            ui.end_row();
            for i in 0..fit.n_eta {
                let name = fit.omega_names.get(i).cloned()
                    .unwrap_or_else(|| format!("ETA{}", i + 1));
                let s = fit.eta_shrinkage.get(i).copied().unwrap_or(f64::NAN);
                shrinkage_row(ui, &name, "ETA", s, dark);
            }
            for i in 0..fit.eps_shrinkage.len() {
                let name = fit.sigma_names.get(i).cloned()
                    .unwrap_or_else(|| format!("EPS{}", i + 1));
                let s = fit.eps_shrinkage[i];
                shrinkage_row(ui, &name, "EPS", s, dark);
            }
        });
    }
}

fn show_param_corr_section(ui: &mut egui::Ui, fit: &FitSummary, dark: bool) {
    section_header(ui, "Parameter Correlations", dark);
    if fit.cov_corr_n == 0 {
        ui.label(egui::RichText::new(
            "Not available — covariance step was not computed or did not converge.")
            .color(theme::fg3(dark)).size(12.0));
        return;
    }
    ui.label(egui::RichText::new(
        "Correlations close to ±1 indicate a structural identifiability problem.")
        .color(theme::fg2(dark)).size(11.0));
    ui.add_space(4.0);
    crate::ui::sir_tab::correlation_heatmap(
        ui, &fit.cov_corr_names, &fit.cov_corr_flat, fit.cov_corr_n, dark);
}

fn show_warnings_section(ui: &mut egui::Ui, fit: &FitSummary, dark: bool) {
    let n_crit = fit.warnings_structured.iter().filter(|w| w.severity == "critical").count();
    let n_warn = fit.warnings_structured.iter().filter(|w| w.severity == "warning").count();
    let n_info = fit.warnings_structured.iter().filter(|w| w.severity == "info").count();
    let summary = format!("Warnings  ({n_crit} critical · {n_warn} warning · {n_info} info)");
    section_header(ui, &summary, dark);

    if fit.warnings_structured.is_empty() {
        ui.label(egui::RichText::new("No warnings.").color(theme::GREEN).size(12.0));
        return;
    }
    for sev in ["critical", "warning", "info"] {
        let group: Vec<_> = fit.warnings_structured.iter()
            .filter(|w| w.severity == sev).collect();
        if group.is_empty() { continue; }
        let col = match sev {
            "critical" => theme::RED,
            "warning"  => theme::ORANGE,
            _          => theme::fg2(dark),
        };
        ui.add_space(4.0);
        ui.label(egui::RichText::new(sev.to_uppercase()).color(col).size(11.0).strong());
        for w in &group {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(egui::RichText::new(format!("[{}] {}", w.category, w.message))
                    .color(theme::fg(dark)).size(11.0));
            });
        }
    }
}

// ── Widget helpers ────────────────────────────────────────────────────────────

fn section_header(ui: &mut egui::Ui, title: &str, dark: bool) {
    ui.separator();
    ui.label(egui::RichText::new(title).size(12.0).strong().color(theme::fg(dark)));
    ui.add_space(2.0);
}

fn param_table(
    ui: &mut egui::Ui,
    id: &str,
    n_cols: usize,
    _dark: bool,
    add_rows: impl FnOnce(&mut egui::Ui),
) {
    egui::Grid::new(id)
        .num_columns(n_cols)
        .spacing([14.0, 3.0])
        .striped(true)
        .show(ui, add_rows);
}

fn param_row_fixed(
    ui: &mut egui::Ui, name: &str, est: f64, se: f64, at_bound: bool, dark: bool,
) {
    let nc = if at_bound { theme::ORANGE } else { theme::fg(dark) };
    ui.label(egui::RichText::new(name).color(nc).size(12.0).monospace());
    ui.label(egui::RichText::new(fmt_sig4(est)).color(theme::fg(dark)).size(12.0));
    ui.label(egui::RichText::new(fmt_sig4(se)).color(theme::fg2(dark)).size(12.0));
    let rse = rse_pct(est, se);
    ui.label(egui::RichText::new(fmt_rse(rse)).color(rse_color(rse)).size(12.0));
    ci_cells(ui, est, se, dark);
    ui.end_row();
}

fn param_row_omega(
    ui: &mut egui::Ui, name: &str, est: f64, se: f64,
    shrinkage: Option<f64>, dark: bool,
) {
    ui.label(egui::RichText::new(name).color(theme::fg(dark)).size(12.0).monospace());
    ui.label(egui::RichText::new(fmt_sig4(est)).color(theme::fg(dark)).size(12.0));
    ui.label(egui::RichText::new(fmt_sig4(se)).color(theme::fg2(dark)).size(12.0));
    let rse = rse_pct(est, se);
    ui.label(egui::RichText::new(fmt_rse(rse)).color(rse_color(rse)).size(12.0));
    ci_cells(ui, est, se, dark);
    // Shrinkage column.
    if let Some(s) = shrinkage {
        let col = shrink_color(s);
        ui.label(egui::RichText::new(format!("{s:.1}%")).color(col).size(12.0));
    } else {
        ui.label(egui::RichText::new("—").color(theme::fg3(dark)).size(12.0));
    }
    ui.end_row();
}

fn shrinkage_row(ui: &mut egui::Ui, name: &str, kind: &str, s: f64, dark: bool) {
    ui.label(egui::RichText::new(name).color(theme::fg(dark)).size(11.0).monospace());
    ui.label(egui::RichText::new(kind).color(theme::fg2(dark)).size(11.0));
    let col = shrink_color(s);
    ui.label(egui::RichText::new(if s.is_nan() { "—".into() } else { format!("{s:.1}%") })
        .color(col).size(11.0));
    ui.end_row();
}

fn kv(ui: &mut egui::Ui, label: &str, value: &str, dark: bool) {
    ui.label(egui::RichText::new(label).color(theme::fg2(dark)).size(11.0));
    ui.label(egui::RichText::new(value).color(theme::fg(dark)).size(11.0));
}

fn kv_col(ui: &mut egui::Ui, label: &str, value: &str, col: egui::Color32, dark: bool) {
    ui.label(egui::RichText::new(label).color(theme::fg2(dark)).size(11.0));
    ui.label(egui::RichText::new(value).color(col).size(11.0));
}

fn ci_cells(ui: &mut egui::Ui, est: f64, se: f64, dark: bool) {
    if se.is_finite() && est.is_finite() {
        let c = theme::fg2(dark);
        ui.label(egui::RichText::new(fmt_sig4(est - 1.96 * se)).color(c).size(11.0));
        ui.label(egui::RichText::new(fmt_sig4(est + 1.96 * se)).color(c).size(11.0));
    } else {
        let c = theme::fg3(dark);
        ui.label(egui::RichText::new("—").color(c).size(11.0));
        ui.label(egui::RichText::new("—").color(c).size(11.0));
    }
}

// ── Numeric helpers ───────────────────────────────────────────────────────────

fn rse_pct(est: f64, se: f64) -> f64 {
    if est != 0.0 && se.is_finite() { (se / est).abs() * 100.0 } else { f64::NAN }
}

fn rse_color(rse: f64) -> egui::Color32 {
    if rse.is_nan()   { theme::FG3 }
    else if rse < 20.0 { theme::GREEN }
    else if rse < 50.0 { theme::ORANGE }
    else               { theme::RED }
}

fn shrink_color(s: f64) -> egui::Color32 {
    if s.is_nan()   { theme::FG3 }
    else if s < 20.0 { theme::GREEN }
    else if s < 40.0 { theme::ORANGE }
    else               { theme::RED }
}

fn fmt_sig4(v: f64) -> String { if v.is_nan() { "—".into() } else { format!("{v:.4}") } }
fn fmt_f64_1dp(v: f64) -> String { if v.is_nan() { "—".into() } else { format!("{v:.1}") } }
fn fmt_f64_3dp(v: f64) -> String { if v.is_nan() { "—".into() } else { format!("{v:.3}") } }
fn fmt_rse(rse: f64) -> String { if rse.is_nan() { "—".into() } else { format!("{rse:.1}%") } }
fn fmt_duration(secs: f64) -> String {
    if secs < 60.0 { format!("{secs:.1} s") }
    else { format!("{:.1} min", secs / 60.0) }
}

// ── HTML export ───────────────────────────────────────────────────────────────

pub fn export_html(state: &mut AppState, idx: usize) {
    let entry = &state.workspace.models[idx];
    let fit = match &entry.fit { Some(f) => f.clone(), None => return };
    let rr = state.run.run_history.iter().rev()
        .find(|r| r.model_stem == entry.model.stem)
        .cloned();
    let gen_ts = now_iso();
    let html = generate_html(&fit, entry, rr.as_ref(), &gen_ts);

    // Write to {model_dir}/{stem}_report_{unix}.html
    let dir = entry.model.path.parent()
        .unwrap_or(std::path::Path::new("."));
    let fname = format!("{}_report_{}.html", entry.model.stem, now_unix());
    let out_path = dir.join(&fname);

    match std::fs::write(&out_path, html.as_bytes()) {
        Ok(()) => {
            state.ui.status_message = format!("Report exported: {}", out_path.display());
            // Open in the default browser.
            let _ = std::process::Command::new("open").arg(&out_path).spawn();
        }
        Err(e) => {
            state.ui.status_message = format!("Export failed: {e}");
        }
    }
}

fn generate_html(
    fit:    &FitSummary,
    entry:  &ModelEntry,
    rr:     Option<&RunRecord>,
    gen_ts: &str,
) -> String {
    let stem  = &entry.model.stem;
    let style = html_css();
    let mut b = String::with_capacity(32_768);

    b.push_str(&format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Run Report – {stem}</title>
<style>{style}</style>
</head>
<body>
<header>
  <h1>Run Report – {stem}</h1>
  <p class="meta">Generated: {gen_ts}</p>
</header>
"#
    ));

    // ── Files ──
    b.push_str("<section><h2>Files</h2><table class=\"kv\">\n");
    b.push_str(&html_kv("Model", &entry.model.path.to_string_lossy()));
    if let Some(rr) = rr {
        if let Some(dp) = &rr.data_path {
            b.push_str(&html_kv("Data", &dp.to_string_lossy()));
        }
    }
    if let Some(fp) = &entry.fitrx_path {
        b.push_str(&html_kv("Output (.fitrx)", &fp.to_string_lossy()));
    }
    b.push_str("</table></section>\n");

    // ── Run Summary ──
    let method = if fit.method_chain.is_empty() { fit.method.clone() }
                 else { fit.method_chain.join(" + ") };
    let cov_str = if fit.covariance_ok { "✓ computed" } else { "✗ not computed" };
    let cn_str = if fit.cov_condition_number.is_finite() {
        if fit.cn_high() {
            format!("{:.1} &mdash; <span class=\"warn\">⚠ High (&gt;1000) — potential overparameterisation or strong parameter correlations</span>",
                    fit.cov_condition_number)
        } else {
            format!("{:.1}", fit.cov_condition_number)
        }
    } else { "—".into() };
    let conv_str = if fit.converged { "✓ converged" } else { "✗ not converged" };
    let wt = if fit.wall_time_secs > 0.0 { fmt_duration(fit.wall_time_secs) } else { "—".into() };

    b.push_str("<section><h2>Run Summary</h2><table class=\"kv\">\n");
    b.push_str(&html_kv("OFV",          &fmt_f64_3dp(fit.ofv)));
    b.push_str(&html_kv("AIC",          &fmt_f64_1dp(fit.aic)));
    b.push_str(&html_kv("BIC",          &fmt_f64_1dp(fit.bic)));
    b.push_str(&html_kv("Method",       &method));
    b.push_str(&html_kv("Convergence",  conv_str));
    b.push_str(&html_kv("Covariance",   cov_str));
    b.push_str(&html_kv_raw("Condition Number", &cn_str));
    b.push_str(&html_kv("Observations", &fit.n_obs.to_string()));
    b.push_str(&html_kv("Subjects",     &fit.n_subjects.to_string()));
    b.push_str(&html_kv("Parameters",   &fit.n_parameters.to_string()));
    b.push_str(&html_kv("Iterations",   &fit.n_iterations.to_string()));
    b.push_str(&html_kv("Wall time",    &wt));
    if let Some(rr) = rr {
        b.push_str(&html_kv("Run started",   &rr.started));
        b.push_str(&html_kv("Run completed", rr.completed.as_deref().unwrap_or("—")));
    }
    b.push_str("</table></section>\n");

    // ── THETA ──
    if !fit.theta.is_empty() {
        b.push_str(&format!(
            "<section><h2>Fixed Effects — THETA  ({})</h2>\n", fit.theta.len()));
        b.push_str("<table class=\"params\"><tr>\
            <th>PARAM</th><th>ESTIMATE</th><th>SE</th>\
            <th>RSE%</th><th>95% CI LO</th><th>95% CI HI</th></tr>\n");
        for i in 0..fit.theta.len() {
            let name = fit.theta_names.get(i).cloned()
                .unwrap_or_else(|| format!("THETA{}", i + 1));
            let est = fit.theta.get(i).copied().unwrap_or(f64::NAN);
            let se  = fit.se_theta.get(i).copied().unwrap_or(f64::NAN);
            let at_b = fit.at_lower_bound.get(i).copied().unwrap_or(false);
            b.push_str(&html_param_row_fixed(&name, est, se, at_b));
        }
        b.push_str("</table></section>\n");
    }

    // ── OMEGA ──
    if fit.n_eta > 0 {
        b.push_str(&format!(
            "<section><h2>Between-Subject Variability — OMEGA  ({} ETA)</h2>\n", fit.n_eta));
        b.push_str("<table class=\"params\"><tr>\
            <th>PARAM</th><th>ESTIMATE</th><th>SE</th>\
            <th>RSE%</th><th>95% CI LO</th><th>95% CI HI</th><th>SHRINKAGE%</th></tr>\n");
        for i in 0..fit.n_eta {
            let name = fit.omega_names.get(i).cloned()
                .unwrap_or_else(|| format!("OMEGA({},{})", i + 1, i + 1));
            let est    = fit.omega_value(i, i).unwrap_or(f64::NAN);
            let se     = fit.se_omega.get(i).copied().unwrap_or(f64::NAN);
            let shrink = fit.eta_shrinkage.get(i).copied();
            b.push_str(&html_param_row_omega(&name, est, se, shrink));
        }
        b.push_str("</table></section>\n");
    }

    // ── KAPPA ──
    if fit.n_kappa > 0 {
        b.push_str(&format!(
            "<section><h2>Inter-Occasion Variability — KAPPA  ({})</h2>\n", fit.n_kappa));
        b.push_str("<table class=\"params\"><tr>\
            <th>PARAM</th><th>ESTIMATE</th><th>SE</th>\
            <th>RSE%</th><th>95% CI LO</th><th>95% CI HI</th></tr>\n");
        for i in 0..fit.n_kappa {
            let name = fit.kappa_names.get(i).cloned()
                .unwrap_or_else(|| format!("KAPPA{}", i + 1));
            let est = fit.kappa_value(i, i).unwrap_or(f64::NAN);
            let se  = fit.se_kappa.get(i).copied().unwrap_or(f64::NAN);
            b.push_str(&html_param_row_fixed(&name, est, se, false));
        }
        b.push_str("</table></section>\n");
    }

    // ── SIGMA ──
    if !fit.sigma.is_empty() {
        b.push_str(&format!(
            "<section><h2>Residual Error — SIGMA  ({})</h2>\n", fit.sigma.len()));
        b.push_str("<table class=\"params\"><tr>\
            <th>PARAM</th><th>ESTIMATE</th><th>SE</th>\
            <th>RSE%</th><th>95% CI LO</th><th>95% CI HI</th><th>SHRINKAGE%</th></tr>\n");
        for i in 0..fit.sigma.len() {
            let name = fit.sigma_names.get(i).cloned()
                .unwrap_or_else(|| format!("SIGMA{}", i + 1));
            let est    = fit.sigma.get(i).copied().unwrap_or(f64::NAN);
            let se     = fit.se_sigma.get(i).copied().unwrap_or(f64::NAN);
            let shrink = fit.eps_shrinkage.get(i).copied();
            b.push_str(&html_param_row_omega(&name, est, se, shrink));
        }
        b.push_str("</table></section>\n");
    }

    // ── Diagnostics ──
    b.push_str("<section><h2>Diagnostics</h2><table class=\"kv\">\n");
    if let Some(dw) = fit.dw_statistic {
        let ok  = dw >= 1.5 && dw <= 2.5;
        let cls = if ok { "ok" } else { "warn" };
        let verdict = if dw < 1.5 { "positive autocorrelation" }
                      else if dw > 2.5 { "negative autocorrelation" }
                      else { "acceptable — no autocorrelation" };
        b.push_str(&format!(
            "<tr>\
               <td>Durbin-Watson\
                 <div class=\"explain\">Tests for temporal autocorrelation in IWRES \
                   (pooled across subjects). Acceptable range: 1.5–2.5. \
                   Values outside this range suggest model misspecification — \
                   review the structural model or residual error model.</div></td>\
               <td class=\"{cls}\">{:.3} ({verdict})</td>\
             </tr>\n",
            dw,
        ));
    }
    if let Some(r) = fit.iwres_lag1_r {
        let flagged = r.abs() > 0.2;
        let cls = if flagged { "warn" } else { "ok" };
        let verdict = if flagged { "autocorrelation detected" } else { "acceptable" };
        b.push_str(&format!(
            "<tr>\
               <td>IWRES lag-1 r\
                 <div class=\"explain\">Pearson correlation of consecutive IWRES within each \
                   subject (lag 1). Complements the Durbin-Watson statistic. \
                   Near zero = independent residuals. |r| &gt; 0.2 flags autocorrelation \
                   worth investigating.</div></td>\
               <td class=\"{cls}\">{:.3} ({verdict})</td>\
             </tr>\n",
            r,
        ));
    }
    b.push_str("</table>\n");

    // Shrinkage sub-table.
    if !fit.eta_shrinkage.is_empty() || !fit.eps_shrinkage.is_empty() {
        b.push_str("<h3>Shrinkage</h3>\
            <table class=\"params\"><tr><th>PARAMETER</th><th>TYPE</th><th>SHRINKAGE%</th></tr>\n");
        for i in 0..fit.n_eta {
            let name = fit.omega_names.get(i).cloned()
                .unwrap_or_else(|| format!("ETA{}", i + 1));
            let s = fit.eta_shrinkage.get(i).copied().unwrap_or(f64::NAN);
            b.push_str(&html_shrinkage_row(&name, "ETA", s));
        }
        for i in 0..fit.eps_shrinkage.len() {
            let name = fit.sigma_names.get(i).cloned()
                .unwrap_or_else(|| format!("EPS{}", i + 1));
            b.push_str(&html_shrinkage_row(&name, "EPS", fit.eps_shrinkage[i]));
        }
        b.push_str("</table>\n");
    }
    b.push_str("</section>\n");

    // ── Parameter correlations ──
    if fit.cov_corr_n > 0 {
        b.push_str("<section><h2>Parameter Correlations</h2>\n");
        b.push_str("<p>Correlations close to ±1 indicate a structural identifiability problem.</p>\n");
        b.push_str(&html_corr_matrix(&fit.cov_corr_names, &fit.cov_corr_flat, fit.cov_corr_n));
        b.push_str("</section>\n");
    }

    // ── Warnings ──
    if !fit.warnings_structured.is_empty() {
        b.push_str("<section><h2>Warnings</h2>\n");
        for sev in ["critical", "warning", "info"] {
            let group: Vec<_> = fit.warnings_structured.iter()
                .filter(|w| w.severity == sev).collect();
            if group.is_empty() { continue; }
            let cls = match sev { "critical" => "crit", "warning" => "warn", _ => "info" };
            b.push_str(&format!("<h3 class=\"{cls}\">{}</h3><ul>\n", sev.to_uppercase()));
            for w in &group {
                b.push_str(&format!(
                    "<li><strong>[{}]</strong> {}</li>\n", w.category, w.message));
            }
            b.push_str("</ul>\n");
        }
        b.push_str("</section>\n");
    }

    b.push_str("<footer><p>Generated by FerxGUI</p></footer>\n</body>\n</html>\n");
    b
}

// ── HTML helpers ──────────────────────────────────────────────────────────────

fn html_kv(label: &str, value: &str) -> String {
    format!("<tr><td>{label}</td><td class=\"mono\">{}</td></tr>\n", html_escape(value))
}

/// Like html_kv but the value is already HTML (may contain tags/entities).
fn html_kv_raw(label: &str, value: &str) -> String {
    format!("<tr><td>{label}</td><td>{value}</td></tr>\n")
}

fn html_param_row_fixed(name: &str, est: f64, se: f64, at_bound: bool) -> String {
    let rse = rse_pct(est, se);
    let cls = if at_bound { " class=\"warn\"" } else { "" };
    let (lo, hi) = ci_pair(est, se);
    format!(
        "<tr{cls}><td class=\"mono\">{name}</td><td>{}</td><td>{}</td>\
         <td class=\"{}\">{}</td><td>{lo}</td><td>{hi}</td></tr>\n",
        fmt_sig4(est), fmt_sig4(se),
        rse_html_class(rse), fmt_rse(rse),
    )
}

fn html_param_row_omega(name: &str, est: f64, se: f64, shrink: Option<f64>) -> String {
    let rse = rse_pct(est, se);
    let (lo, hi) = ci_pair(est, se);
    let shrink_str = match shrink {
        Some(s) => format!("<td class=\"{}\">{:.1}%</td>", shrink_html_class(s), s),
        None    => "<td>—</td>".to_string(),
    };
    format!(
        "<tr><td class=\"mono\">{name}</td><td>{}</td><td>{}</td>\
         <td class=\"{}\">{}</td><td>{lo}</td><td>{hi}</td>{shrink_str}</tr>\n",
        fmt_sig4(est), fmt_sig4(se),
        rse_html_class(rse), fmt_rse(rse),
    )
}

fn html_shrinkage_row(name: &str, kind: &str, s: f64) -> String {
    let cls = shrink_html_class(s);
    let val = if s.is_nan() { "—".into() } else { format!("{s:.1}%") };
    format!("<tr><td class=\"mono\">{name}</td><td>{kind}</td>\
             <td class=\"{cls}\">{val}</td></tr>\n")
}

fn html_corr_matrix(names: &[String], flat: &[f64], n: usize) -> String {
    let mut out = String::from("<table class=\"corr\">\n<tr><th></th>");
    for name in names.iter().take(n) {
        out.push_str(&format!("<th>{}</th>", html_escape(name)));
    }
    out.push_str("</tr>\n");
    for row in 0..n {
        let rname = names.get(row).map(|s| s.as_str()).unwrap_or("");
        out.push_str(&format!("<tr><th class=\"mono\">{}</th>", html_escape(rname)));
        for col in 0..n {
            let r = flat.get(row * n + col).copied().unwrap_or(0.0);
            let (bg_r, bg_g, bg_b) = corr_rgb(r);
            let lum = (0.299 * bg_r as f32 + 0.587 * bg_g as f32 + 0.114 * bg_b as f32) / 255.0;
            let text_col = if lum < 0.35 { "#fff" } else { "#111" };
            out.push_str(&format!(
                "<td style=\"background:rgb({bg_r},{bg_g},{bg_b});color:{text_col}\">{:.2}</td>",
                r,
            ));
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</table>\n");
    out
}

/// Diverging navy–white–crimson RGB for HTML cells. Same formula as `corr_color`.
fn corr_rgb(r: f64) -> (u8, u8, u8) {
    let t = r.clamp(-1.0, 1.0);
    if t >= 0.0 {
        let f = t as f32;
        ((255.0 * (1.0 - f * 0.969)) as u8,
         (255.0 * (1.0 - f * 0.749)) as u8,
         (255.0 * (1.0 - f * 0.412)) as u8)
    } else {
        let f = (-t) as f32;
        ((255.0 * (1.0 - f * 0.373)) as u8,
         (255.0 * (1.0 - f * 0.980)) as u8,
         (255.0 * (1.0 - f * 0.941)) as u8)
    }
}

fn ci_pair(est: f64, se: f64) -> (String, String) {
    if se.is_finite() && est.is_finite() {
        (fmt_sig4(est - 1.96 * se), fmt_sig4(est + 1.96 * se))
    } else {
        ("—".into(), "—".into())
    }
}

fn rse_html_class(rse: f64) -> &'static str {
    if rse.is_nan()    { "" }
    else if rse < 20.0 { "ok" }
    else if rse < 50.0 { "warn" }
    else               { "crit" }
}

fn shrink_html_class(s: f64) -> &'static str {
    if s.is_nan()    { "" }
    else if s < 20.0 { "ok" }
    else if s < 40.0 { "warn" }
    else             { "crit" }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn html_css() -> &'static str {
    r#"
body{font-family:system-ui,-apple-system,sans-serif;font-size:13px;
     max-width:960px;margin:0 auto;padding:24px;color:#1a1a1a;background:#fff}
h1{font-size:1.4em;margin:0 0 4px}
h2{font-size:1.1em;border-bottom:1px solid #ddd;padding-bottom:4px;margin:20px 0 8px}
h3{font-size:1.0em;margin:10px 0 4px}
p.meta{color:#666;font-size:11px;margin:0 0 20px}
section{margin-bottom:20px}
table{border-collapse:collapse;width:100%}
table.kv td{padding:3px 8px;vertical-align:top}
table.kv td:first-child{color:#555;width:140px;font-weight:600}
table.params th{background:#f4f4f4;padding:4px 8px;text-align:left;
                font-size:11px;border:1px solid #e0e0e0}
table.params td{padding:3px 8px;border:1px solid #eee;font-size:12px}
table.params tr:nth-child(even) td{background:#fafafa}
table.corr th,table.corr td{padding:4px 6px;text-align:center;
                              font-size:11px;border:1px solid #ddd}
table.corr th{background:#f4f4f4;font-weight:600}
.mono{font-family:ui-monospace,monospace}
.ok{color:#1a7a40;font-weight:600}
.warn{color:#b06000;font-weight:600}
.crit{color:#c0392b;font-weight:600}
.info{color:#555}
footer{margin-top:32px;padding-top:8px;border-top:1px solid #ddd;
       font-size:11px;color:#888}
.explain{font-size:10px;color:#777;font-style:italic;margin-top:2px;font-weight:normal}
"#
}
