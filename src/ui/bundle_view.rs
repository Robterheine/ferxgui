//! `.fitrx` bundle inspector for the Files tab: provenance and reproducibility checks, run
//! and fit statistics, parameter tables, the bundled model, every entry in the archive with
//! previews, and the raw `fit.json` as a tree.

use eframe::egui;
use serde_json::Value;

use crate::app::theme;
use crate::io::bundle::{self, BundleInfo, EntryPreview, FileCheck};
use crate::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BundleSection {
    #[default]
    Overview,
    Parameters,
    Model,
    Contents,
    FitJson,
}

/// Everything the Files tab keeps for the bundle that is currently open.
pub struct BundleViewState {
    pub info: BundleInfo,
    pub section: BundleSection,
    pub data_check: FileCheck,
    pub selected_entry: Option<String>,
    pub preview: Option<Result<EntryPreview, String>>,
    pub json_filter: String,
    /// One-line result of the last action (save entry, copy ...).
    pub status: String,
}

impl BundleViewState {
    pub fn new(info: BundleInfo) -> Self {
        Self {
            info, section: BundleSection::Overview, data_check: FileCheck::NotChecked,
            selected_entry: None, preview: None, json_filter: String::new(), status: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn show(ui: &mut egui::Ui, state: &mut AppState, dark: bool) {
    let Some(mut bv) = state.ui.files_bundle.take() else {
        let msg = state.ui.files_bundle_error.clone()
            .unwrap_or_else(|| "No bundle loaded.".to_string());
        ui.add_space(20.0);
        ui.label(egui::RichText::new("Could not read this .fitrx bundle").color(theme::RED).size(13.0));
        ui.label(egui::RichText::new(msg).color(theme::fg3(dark)).size(11.0));
        return;
    };

    ui.horizontal(|ui| {
        for (label, sec) in [
            ("Overview", BundleSection::Overview),
            ("Parameters", BundleSection::Parameters),
            ("Model", BundleSection::Model),
            ("Contents", BundleSection::Contents),
            ("fit.json", BundleSection::FitJson),
        ] {
            let active = bv.section == sec;
            if ui.add(
                egui::Button::new(egui::RichText::new(label).size(11.0)
                    .color(if active { egui::Color32::WHITE } else { theme::fg2(dark) }))
                .fill(if active { theme::ACCENT } else { theme::card_fill(dark) })
                .min_size(egui::vec2(0.0, 22.0)),
            ).clicked() {
                bv.section = sec;
            }
        }
        if !bv.status.is_empty() {
            ui.add_space(10.0);
            ui.label(egui::RichText::new(&bv.status).color(theme::fg3(dark)).size(10.5));
        }
    });
    ui.add_space(4.0);

    egui::ScrollArea::both().auto_shrink([false; 2]).show(ui, |ui| {
        match bv.section {
            BundleSection::Overview   => show_overview(ui, &mut bv, dark),
            BundleSection::Parameters => show_parameters(ui, &bv, dark),
            BundleSection::Model      => show_model(ui, &mut bv, dark),
            BundleSection::Contents   => show_contents(ui, &mut bv, dark),
            BundleSection::FitJson    => show_fit_json(ui, &mut bv, dark),
        }
    });

    state.ui.files_bundle = Some(bv);
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn card(ui: &mut egui::Ui, dark: bool, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(6.0);
    egui::Frame::new()
        .fill(theme::card_fill(dark))
        .inner_margin(egui::Margin::same(10))
        .corner_radius(egui::CornerRadius::same(6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(title).color(theme::fg2(dark)).size(11.0).strong());
            ui.add_space(4.0);
            add(ui);
        });
}

fn kv(ui: &mut egui::Ui, dark: bool, key: &str, value: &str) {
    kv_color(ui, dark, key, value, theme::fg(dark));
}

fn kv_color(ui: &mut egui::Ui, dark: bool, key: &str, value: &str, color: egui::Color32) {
    ui.label(egui::RichText::new(key).color(theme::fg3(dark)).size(11.0));
    ui.label(egui::RichText::new(value).color(color).size(11.5));
    ui.end_row();
}

fn grid(id: &str, cols: usize) -> egui::Grid {
    egui::Grid::new(id).num_columns(cols).spacing([14.0, 4.0])
}

fn num(v: Option<f64>, digits: usize) -> String {
    match v {
        Some(x) if x.is_finite() => format!("{x:.digits$}"),
        _ => "—".to_string(),
    }
}

fn sig(x: f64) -> String {
    if !x.is_finite() { return "—".into(); }
    if x == 0.0 { return "0".into(); }
    let a = x.abs();
    if (1e-3..1e5).contains(&a) { format!("{x:.5}").trim_end_matches('0').trim_end_matches('.').to_string() }
    else { format!("{x:.4e}") }
}

pub fn fmt_duration(secs: f64) -> String {
    if !secs.is_finite() || secs <= 0.0 { return "—".into(); }
    let s = secs.round() as u64;
    match (s / 3600, (s % 3600) / 60, s % 60) {
        (0, 0, s) => format!("{s}s"),
        (0, m, s) => format!("{m}m {s:02}s"),
        (h, m, s) => format!("{h}h {m:02}m {s:02}s"),
    }
}

pub fn human_bytes(n: u64) -> String {
    const U: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 { v /= 1024.0; i += 1; }
    if i == 0 { format!("{n} B") } else { format!("{v:.1} {}", U[i]) }
}

fn short_hash(h: Option<&str>) -> String {
    match h {
        Some(h) if h.len() > 16 => format!("{}…", &h[..16]),
        Some(h) => h.to_string(),
        None => "—".into(),
    }
}

fn badge(ui: &mut egui::Ui, dark: bool, check: &FileCheck, what: &str) {
    let (txt, col) = match check {
        FileCheck::NotChecked => (String::new(), theme::fg3(dark)),
        FileCheck::NoRecord   => (format!("no {what} recorded to compare"), theme::fg3(dark)),
        FileCheck::Missing(p) => (format!("⚠ file not found ({p})"), theme::ORANGE),
        FileCheck::Match(p)   => (format!("✔ identical to {p}"), theme::GREEN),
        FileCheck::Differs(p) => (format!("⚠ {p} has changed since the fit"), theme::ORANGE),
    };
    if !txt.is_empty() {
        ui.label(egui::RichText::new(txt).color(col).size(10.5));
    }
}

fn jstr<'a>(v: &'a Value, k: &str) -> Option<&'a str> { v.get(k).and_then(Value::as_str) }
fn jf(v: &Value, k: &str) -> Option<f64> { v.get(k).and_then(Value::as_f64) }
fn ji(v: &Value, k: &str) -> Option<i64> { v.get(k).and_then(Value::as_i64) }
fn jb(v: &Value, k: &str) -> Option<bool> { v.get(k).and_then(Value::as_bool) }

fn str_list(v: &Value, k: &str) -> Vec<String> {
    match v.get(k) {
        Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => vec![],
    }
}

fn yes_no(b: Option<bool>) -> &'static str {
    match b { Some(true) => "yes", Some(false) => "no", None => "—" }
}

// ---------------------------------------------------------------------------
// Overview
// ---------------------------------------------------------------------------

fn show_overview(ui: &mut egui::Ui, bv: &mut BundleViewState, dark: bool) {
    let fit = bv.info.fit.clone();
    let man = bv.info.manifest.clone();

    // ── Provenance & reproducibility ──
    card(ui, dark, "PROVENANCE", |ui| {
        grid("bv_prov", 2).show(ui, |ui| {
            kv(ui, dark, "Model", jstr(&man, "model_name").or(jstr(&fit, "model_name")).unwrap_or("—"));
            kv(ui, dark, "ferx version", &format!("{}   (bundle format {})",
                jstr(&man, "ferx_version").or(jstr(&fit, "ferx_version")).unwrap_or("—"),
                jstr(&man, "format_version").unwrap_or("—")));
            kv(ui, dark, "Created", jstr(&man, "created_at").unwrap_or("—"));
            kv(ui, dark, "Bundle size", &format!("{}  ({} entries)",
                human_bytes(bv.info.file_size), bv.info.entries.len()));
        });
        ui.add_space(6.0);
        grid("bv_repro", 3).show(ui, |ui| {
            ui.label(egui::RichText::new("Model file").color(theme::fg3(dark)).size(11.0));
            ui.label(egui::RichText::new(jstr(&fit, "model_path").unwrap_or("—")).monospace().size(10.5));
            ui.end_row();
            ui.label("");
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("sha256 {}", short_hash(bv.info.recorded_model_hash())))
                    .color(theme::fg3(dark)).size(10.0).monospace());
                badge(ui, dark, &bv.info.model_check, "model");
            });
            ui.end_row();

            ui.label(egui::RichText::new("Dataset").color(theme::fg3(dark)).size(11.0));
            ui.label(egui::RichText::new(jstr(&fit, "data_path").unwrap_or("—")).monospace().size(10.5));
            ui.end_row();
            ui.label("");
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("sha256 {}", short_hash(bv.info.recorded_data_hash())))
                    .color(theme::fg3(dark)).size(10.0).monospace());
                if matches!(bv.data_check, FileCheck::NotChecked)
                    && bv.info.recorded_data_hash().is_some()
                    && ui.small_button("Verify data file").on_hover_text(
                        "Hash the dataset on disk and compare it with the hash recorded at fit time").clicked()
                {
                    bv.data_check = bundle::verify_data(&bv.info);
                }
                badge(ui, dark, &bv.data_check, "dataset");
            });
            ui.end_row();
        });
    });

    // ── Run ──
    card(ui, dark, "RUN", |ui| {
        grid("bv_run", 2).show(ui, |ui| {
            let chain = str_list(&fit, "method_chain");
            let method = if chain.is_empty() { jstr(&fit, "method").unwrap_or("—").to_string() } else { chain.join("  →  ") };
            kv(ui, dark, "Method", &method);
            let conv = jb(&fit, "converged");
            kv_color(ui, dark, "Converged", match conv { Some(true) => "✔ yes", Some(false) => "✖ no", None => "—" },
                match conv { Some(true) => theme::GREEN, Some(false) => theme::RED, None => theme::fg3(dark) });
            kv(ui, dark, "Iterations", &ji(&fit, "n_iterations").map_or("—".into(), |v| v.to_string()));
            kv(ui, dark, "Wall time", &fmt_duration(jf(&fit, "wall_time_secs").unwrap_or(f64::NAN)));
            kv(ui, dark, "Threads", &ji(&fit, "n_threads_used").map_or("—".into(), |v| v.to_string()));
            kv(ui, dark, "Gradient (inner / outer)", &format!("{}  /  {}",
                jstr(&fit, "gradient_method_inner").unwrap_or("—"), jstr(&fit, "gradient_method_outer").unwrap_or("—")));
            kv(ui, dark, "Interaction", yes_no(jb(&fit, "interaction")));
            kv(ui, dark, "ODE solver / SDE", &format!("{} / {}", yes_no(jb(&fit, "uses_ode_solver")), yes_no(jb(&fit, "uses_sde"))));
            kv(ui, dark, "Error model", jstr(&fit, "error_model").unwrap_or("—"));
        });
    });

    // ── Fit statistics ──
    card(ui, dark, "FIT", |ui| {
        grid("bv_fit", 2).show(ui, |ui| {
            kv(ui, dark, "OFV", &num(jf(&fit, "ofv"), 4));
            kv(ui, dark, "AIC", &num(jf(&fit, "aic"), 3));
            kv(ui, dark, "BIC", &num(jf(&fit, "bic"), 3));
            kv(ui, dark, "Observations", &ji(&fit, "n_obs").map_or("—".into(), |v| v.to_string()));
            kv(ui, dark, "Subjects", &ji(&fit, "n_subjects").map_or("—".into(), |v| v.to_string()));
            let npar = ji(&fit, "n_parameters").map_or("—".into(), |v| v.to_string());
            let bi = fit.get("bic_inputs").cloned().unwrap_or(Value::Null);
            let breakdown = if bi.is_null() { String::new() } else {
                format!("   (θ {} free + {} fixed, ω {}, κ {}, σ {})",
                    ji(&bi, "theta_random").unwrap_or(0), ji(&bi, "theta_fixed").unwrap_or(0),
                    ji(&bi, "omega").unwrap_or(0), ji(&bi, "kappa").unwrap_or(0), ji(&bi, "sigma").unwrap_or(0))
            };
            kv(ui, dark, "Parameters", &format!("{npar}{breakdown}"));
            if let Some(p) = jf(&fit, "ofv_prior").filter(|p| *p != 0.0) {
                kv(ui, dark, "OFV (data / prior)", &format!("{} / {}", num(jf(&fit, "ofv_data"), 4), num(Some(p), 4)));
            }
        });
    });

    // ── Covariance ──
    card(ui, dark, "COVARIANCE", |ui| {
        grid("bv_cov", 2).show(ui, |ui| {
            let status = jstr(&fit, "covariance_status").unwrap_or("—");
            kv_color(ui, dark, "Status", status,
                if status == "computed" { theme::GREEN } else { theme::fg2(dark) });
            let cn = jf(&fit, "cov_condition_number");
            kv_color(ui, dark, "Condition number", &num(cn, 1),
                if cn.is_some_and(|c| c > 1000.0) { theme::ORANGE } else { theme::fg(dark) });
            if let Some(Value::Array(ev)) = fit.get("cov_eigenvalues") {
                let v: Vec<f64> = ev.iter().filter_map(Value::as_f64).collect();
                if let (Some(lo), Some(hi)) = (v.iter().cloned().reduce(f64::min), v.iter().cloned().reduce(f64::max)) {
                    kv(ui, dark, "Eigenvalues (min / max)", &format!("{} / {}", sig(lo), sig(hi)));
                }
            }
            if let Some(m) = fit.pointer("/r_extras/max_abs_correlation").and_then(Value::as_f64) {
                kv_color(ui, dark, "Max |correlation|", &format!("{m:.3}"),
                    if m > 0.95 { theme::ORANGE } else { theme::fg(dark) });
            }
            if let Some(cm) = fit.get("covariance_matrix") {
                kv(ui, dark, "Matrix", &format!("{} × {}", ji(cm, "rows").unwrap_or(0), ji(cm, "cols").unwrap_or(0)));
            }
        });
    });

    // ── Diagnostics ──
    card(ui, dark, "DIAGNOSTICS", |ui| {
        grid("bv_diag", 2).show(ui, |ui| {
            kv(ui, dark, "Durbin-Watson", &num(jf(&fit, "dw_statistic"), 3));
            kv(ui, dark, "IWRES lag-1 r", &num(jf(&fit, "iwres_lag1_r"), 3));
            if let Some(s) = &bv.info.summary {
                if let Some(e) = s.eps_shrinkage.first() {
                    kv(ui, dark, "ε-shrinkage", &format!("{e:.1}%"));
                }
                if !s.eta_shrinkage.is_empty() {
                    let txt = s.eta_shrinkage.iter().enumerate()
                        .map(|(i, v)| format!("{} {:.1}%", s.omega_names.get(i).map(String::as_str).unwrap_or("ETA"), v))
                        .collect::<Vec<_>>().join(",  ");
                    kv(ui, dark, "η-shrinkage", &txt);
                }
            }
            kv(ui, dark, "EBE not converged", &format!("{} warnings, max {} subjects, {} fallbacks",
                ji(&fit, "ebe_convergence_warnings").unwrap_or(0),
                ji(&fit, "max_unconverged_subjects").unwrap_or(0),
                ji(&fit, "total_ebe_fallbacks").unwrap_or(0)));
            if let Some(b) = fit.pointer("/r_extras/stalled_at_init").and_then(Value::as_bool) {
                kv_color(ui, dark, "Stalled at initial values", yes_no(Some(b)), if b { theme::ORANGE } else { theme::fg(dark) });
            }
            if let Some(b) = fit.pointer("/r_extras/estimate_near_boundary").and_then(Value::as_bool) {
                kv_color(ui, dark, "Estimate near a boundary", yes_no(Some(b)), if b { theme::ORANGE } else { theme::fg(dark) });
            }
        });
    });

    // ── Data ──
    card(ui, dark, "DATA", |ui| {
        grid("bv_data", 2).show(ui, |ui| {
            let cols = str_list(&fit, "input_columns");
            if !cols.is_empty() { kv(ui, dark, "Columns", &cols.join(", ")); }
            let cov = str_list(&fit, "covariate_names");
            if !cov.is_empty() { kv(ui, dark, "Covariates", &cov.join(", ")); }
            if let Some(ex) = fit.get("exclusions").filter(|e| !e.is_null()) {
                kv(ui, dark, "Records", &format!("{} total, {} observations / {} doses / {} other excluded",
                    ji(ex, "n_records_total").unwrap_or(0), ji(ex, "n_obs_excluded").unwrap_or(0),
                    ji(ex, "n_dose_excluded").unwrap_or(0), ji(ex, "n_other_excluded").unwrap_or(0)));
                let subj = str_list(ex, "excluded_subject_ids");
                if !subj.is_empty() { kv(ui, dark, "Excluded subjects", &subj.join(", ")); }
            }
        });
    });

    // ── Warnings ──
    if !bv.info.warnings.is_empty() {
        card(ui, dark, &format!("WARNINGS  ({})", bv.info.warnings.len()), |ui| {
            for w in &bv.info.warnings {
                ui.add(egui::Label::new(egui::RichText::new(format!("•  {w}")).color(theme::ORANGE).size(11.0)).wrap());
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Parameters
// ---------------------------------------------------------------------------

fn show_parameters(ui: &mut egui::Ui, bv: &BundleViewState, dark: bool) {
    let Some(s) = &bv.info.summary else {
        ui.label(egui::RichText::new("The bundle's fit.json could not be read, so there are no parameters to show.")
            .color(theme::fg3(dark)).size(12.0));
        return;
    };
    let fit = &bv.info.fit;
    let hdr = |ui: &mut egui::Ui, cols: &[&str]| {
        for c in cols { ui.label(egui::RichText::new(*c).color(theme::fg3(dark)).size(10.0).strong()); }
        ui.end_row();
    };
    let cell = |ui: &mut egui::Ui, t: String| { ui.label(egui::RichText::new(t).size(11.0).monospace()); };
    let rse = |est: f64, se: f64| if est != 0.0 && se.is_finite() { format!("{:.1}", (se / est).abs() * 100.0) } else { "—".into() };

    let init: Vec<f64> = fit.get("theta_init").and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    let fixed: Vec<bool> = fit.pointer("/theta/fixed").and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_bool).collect()).unwrap_or_default();

    card(ui, dark, &format!("THETA  ({})", s.theta.len()), |ui| {
        grid("bv_theta", 6).striped(true).show(ui, |ui| {
            hdr(ui, &["NAME", "INIT", "ESTIMATE", "SE", "RSE %", "FIXED"]);
            for (i, est) in s.theta.iter().enumerate() {
                let se = s.se_theta.get(i).copied().unwrap_or(f64::NAN);
                cell(ui, s.theta_names.get(i).cloned().unwrap_or_else(|| format!("THETA{}", i + 1)));
                cell(ui, init.get(i).map_or("—".into(), |v| sig(*v)));
                cell(ui, sig(*est));
                cell(ui, sig(se));
                cell(ui, rse(*est, se));
                cell(ui, if fixed.get(i).copied().unwrap_or(false) { "fixed".into() } else { String::new() });
            }
        });
    });

    if s.n_eta > 0 {
        card(ui, dark, &format!("OMEGA  ({} ETA{})", s.n_eta,
            if s.omega_is_diagonal == Some(false) { ", block" } else { "" }), |ui| {
            grid("bv_omega", 6).striped(true).show(ui, |ui| {
                hdr(ui, &["NAME", "VARIANCE", "SD", "SE", "RSE %", "SHRINKAGE %"]);
                for i in 0..s.n_eta {
                    let var = s.omega_value(i, i).unwrap_or(f64::NAN);
                    let se = s.se_omega_diag(i).unwrap_or(f64::NAN);
                    cell(ui, s.omega_names.get(i).cloned().unwrap_or_else(|| format!("ETA{}", i + 1)));
                    cell(ui, sig(var));
                    cell(ui, sig(var.max(0.0).sqrt()));
                    cell(ui, sig(se));
                    cell(ui, rse(var, se));
                    cell(ui, s.eta_shrinkage.get(i).map_or("—".into(), |v| format!("{v:.1}")));
                }
            });
            let off: Vec<(usize, usize)> = (1..s.n_eta).flat_map(|r| (0..r).map(move |c| (r, c)))
                .filter(|&(r, c)| s.omega_value(r, c).is_some_and(|v| v != 0.0)).collect();
            if !off.is_empty() {
                ui.add_space(6.0);
                grid("bv_omega_off", 4).striped(true).show(ui, |ui| {
                    hdr(ui, &["PAIR", "COVARIANCE", "CORRELATION", "SE"]);
                    for (r, c) in off {
                        let nm = |i: usize| s.omega_names.get(i).cloned().unwrap_or_else(|| format!("ETA{}", i + 1));
                        cell(ui, format!("{} ~ {}", nm(r), nm(c)));
                        cell(ui, sig(s.omega_value(r, c).unwrap_or(f64::NAN)));
                        cell(ui, s.omega_corr(r, c).map_or("—".into(), |v| format!("{v:.3}")));
                        cell(ui, s.se_omega_offdiag(r, c).map_or("—".into(), sig));
                    }
                });
            }
        });
    }

    if s.n_kappa > 0 {
        card(ui, dark, &format!("KAPPA  ({} IOV)", s.n_kappa), |ui| {
            grid("bv_kappa", 4).striped(true).show(ui, |ui| {
                hdr(ui, &["NAME", "VARIANCE", "SD", "SE"]);
                for i in 0..s.n_kappa {
                    let var = s.kappa_value(i, i).unwrap_or(f64::NAN);
                    cell(ui, s.kappa_names.get(i).cloned().unwrap_or_else(|| format!("KAPPA{}", i + 1)));
                    cell(ui, sig(var));
                    cell(ui, sig(var.max(0.0).sqrt()));
                    cell(ui, s.se_kappa_diag(i).map_or("—".into(), sig));
                }
            });
        });
    }

    card(ui, dark, &format!("SIGMA  ({})", s.sigma.len()), |ui| {
        grid("bv_sigma", 5).striped(true).show(ui, |ui| {
            hdr(ui, &["NAME", "ESTIMATE", "SE", "RSE %", "SHRINKAGE %"]);
            for (i, est) in s.sigma.iter().enumerate() {
                let se = s.se_sigma.get(i).copied().unwrap_or(f64::NAN);
                cell(ui, s.sigma_names.get(i).cloned().unwrap_or_else(|| format!("SIGMA{}", i + 1)));
                cell(ui, sig(*est));
                cell(ui, sig(se));
                cell(ui, rse(*est, se));
                cell(ui, s.eps_shrinkage.get(i).map_or("—".into(), |v| format!("{v:.1}")));
            }
        });
        for rc in &s.residual_correlations {
            let nm = |i: usize| s.sigma_names.get(i).cloned().unwrap_or_else(|| format!("SIGMA{}", i + 1));
            ui.label(egui::RichText::new(format!("ρ({}, {}) = {:.3}  (SE {})", nm(rc.sigma_i), nm(rc.sigma_j), rc.rho,
                if rc.se.is_finite() { sig(rc.se) } else { "—".into() })).size(11.0).monospace());
        }
    });

    if s.has_prior() {
        card(ui, dark, "PRIORS", |ui| {
            grid("bv_priors", 6).striped(true).show(ui, |ui| {
                hdr(ui, &["PARAM", "PRIOR", "ESTIMATE", "SHIFT (SD)", "PENALTY", "FAMILY"]);
                for r in &s.prior_summary {
                    cell(ui, r.name.clone()); cell(ui, sig(r.prior_value)); cell(ui, sig(r.estimate));
                    cell(ui, format!("{:+.2}", r.shift_in_prior_sds)); cell(ui, sig(r.penalty)); cell(ui, r.family.clone());
                }
            });
        });
    }
}

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

fn show_model(ui: &mut egui::Ui, bv: &mut BundleViewState, dark: bool) {
    let Some(src) = bv.info.model_source.clone() else {
        ui.label(egui::RichText::new("This bundle has no model.ferx entry.").color(theme::fg3(dark)).size(12.0));
        return;
    };
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("model.ferx as fitted").color(theme::fg2(dark)).size(11.0).strong());
        badge(ui, dark, &bv.info.model_check, "model");
        if ui.small_button("Copy").clicked() {
            ui.ctx().copy_text(src.clone());
            bv.status = "Model source copied".to_string();
        }
    });
    ui.add_space(4.0);
    let job = crate::ui::models_tab::highlight_ferx(&src, dark);
    ui.add(egui::Label::new(job).selectable(true));
}

// ---------------------------------------------------------------------------
// Contents
// ---------------------------------------------------------------------------

fn entry_description(name: &str) -> &'static str {
    match name {
        "manifest.json"   => "format version, ferx version, timestamp, entry index",
        "fit.json"        => "scalars, vectors and matrices of the fit result",
        "ebes.csv"        => "per-subject empirical Bayes estimates and iOFV",
        "ebes_kappa.csv"  => "per-subject, per-occasion IOV (kappa) estimates",
        "predictions.csv" => "per-observation DV, PRED, IPRED and residuals",
        "trace.csv"       => "optimizer trace: OFV, parameters and gradients per iteration",
        "model.ferx"      => "the model source exactly as fitted",
        "warnings.txt"    => "warnings raised during the fit",
        "covtab.csv"      => "declared covariates per observation",
        "conddist.csv"    => "conditional distributions of the random effects (SAEM)",
        "data.csv"        => "the embedded input dataset",
        _                 => "",
    }
}

fn show_contents(ui: &mut egui::Ui, bv: &mut BundleViewState, dark: bool) {
    let entries = bv.info.entries.clone();
    card(ui, dark, &format!("ARCHIVE  ({} entries, {} on disk)", entries.len(), human_bytes(bv.info.file_size)), |ui| {
        let mut clicked: Option<String> = None;
        grid("bv_entries", 4).striped(true).show(ui, |ui| {
            for h in ["ENTRY", "SIZE", "COMPRESSED", "CONTENT"] {
                ui.label(egui::RichText::new(h).color(theme::fg3(dark)).size(10.0).strong());
            }
            ui.end_row();
            for e in &entries {
                let sel = bv.selected_entry.as_deref() == Some(e.name.as_str());
                if ui.selectable_label(sel, egui::RichText::new(&e.name).monospace().size(11.0)).clicked() {
                    clicked = Some(e.name.clone());
                }
                ui.label(egui::RichText::new(human_bytes(e.size)).size(11.0).monospace());
                ui.label(egui::RichText::new(human_bytes(e.compressed)).size(11.0).monospace());
                ui.label(egui::RichText::new(entry_description(&e.name)).color(theme::fg3(dark)).size(10.5));
                ui.end_row();
            }
        });
        if let Some(name) = clicked {
            bv.preview = Some(bundle::read_entry_preview(&bv.info.path, &name));
            bv.selected_entry = Some(name);
        }
    });

    let Some(name) = bv.selected_entry.clone() else {
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Select an entry to preview it.").color(theme::fg3(dark)).size(11.5));
        return;
    };
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(&name).color(theme::fg(dark)).size(12.0).strong());
        if ui.small_button("Save entry as…").clicked() {
            if let Some(dest) = rfd::FileDialog::new().set_file_name(&name).save_file() {
                bv.status = match bundle::extract_entry(&bv.info.path, &name, &dest) {
                    Ok(()) => format!("Saved {name} to {}", dest.display()),
                    Err(e) => format!("Could not save {name}: {e}"),
                };
            }
        }
    });
    match &bv.preview {
        Some(Ok(EntryPreview::Table { headers, rows, total_rows })) => {
            ui.label(egui::RichText::new(format!(
                "{} columns, {} rows{}", headers.len(), total_rows,
                if *total_rows > rows.len() { format!("  (first {} shown)", rows.len()) } else { String::new() }))
                .color(theme::fg3(dark)).size(10.5));
            ui.add_space(4.0);
            egui::ScrollArea::horizontal().id_salt("bv_preview_table").show(ui, |ui| {
                grid("bv_preview_grid", headers.len().max(1)).striped(true).show(ui, |ui| {
                    for h in headers {
                        ui.label(egui::RichText::new(h).color(theme::fg2(dark)).size(10.0).strong().monospace());
                    }
                    ui.end_row();
                    for r in rows {
                        for c in r {
                            let shown: String = c.chars().take(14).collect();
                            ui.label(egui::RichText::new(shown).size(10.5).monospace());
                        }
                        ui.end_row();
                    }
                });
            });
        }
        Some(Ok(EntryPreview::Text { text, truncated })) => {
            if *truncated {
                ui.label(egui::RichText::new("(truncated)").color(theme::fg3(dark)).size(10.5));
            }
            ui.add(egui::Label::new(egui::RichText::new(text).monospace().size(11.0)).selectable(true));
        }
        Some(Err(e)) => {
            ui.label(egui::RichText::new(format!("Could not read {name}: {e}")).color(theme::RED).size(11.0));
        }
        None => {}
    }
}

// ---------------------------------------------------------------------------
// fit.json tree
// ---------------------------------------------------------------------------

/// Most array items expanded in the tree.
const TREE_MAX_ITEMS: usize = 100;

fn scalar_text(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.as_f64().map_or(n.to_string(), |f| if f.fract() == 0.0 && f.abs() < 1e15 { format!("{f:.0}") } else { sig(f) }),
        Value::String(s) => format!("\"{s}\""),
        _ => String::new(),
    }
}

fn is_scalar(v: &Value) -> bool { !matches!(v, Value::Array(_) | Value::Object(_)) }

/// One-line form of a short scalar array (e.g. `[0.13, 7.7, 0.72]`), or `None`.
fn inline_array(a: &[Value]) -> Option<String> {
    if a.len() <= 8 && a.iter().all(is_scalar) {
        Some(format!("[{}]", a.iter().map(scalar_text).collect::<Vec<_>>().join(", ")))
    } else {
        None
    }
}

fn json_tree(ui: &mut egui::Ui, key: &str, v: &Value, dark: bool) {
    let mono = |t: String, col: egui::Color32| egui::RichText::new(t).monospace().size(11.0).color(col);
    match v {
        Value::Object(m) => {
            egui::CollapsingHeader::new(mono(format!("{key}  {{{}}}", m.len()), theme::fg(dark)))
                .id_salt(key)
                .show(ui, |ui| {
                    for (k, child) in m { json_tree(ui, k, child, dark); }
                });
        }
        Value::Array(a) => {
            if let Some(line) = inline_array(a) {
                ui.label(mono(format!("{key}: {line}"), theme::fg(dark)));
            } else {
                egui::CollapsingHeader::new(mono(format!("{key}  [{}]", a.len()), theme::fg(dark)))
                    .id_salt(key)
                    .show(ui, |ui| {
                        for (i, child) in a.iter().take(TREE_MAX_ITEMS).enumerate() {
                            json_tree(ui, &i.to_string(), child, dark);
                        }
                        if a.len() > TREE_MAX_ITEMS {
                            ui.label(mono(format!("… {} more", a.len() - TREE_MAX_ITEMS), theme::fg3(dark)));
                        }
                    });
            }
        }
        scalar => {
            ui.label(mono(format!("{key}: {}", scalar_text(scalar)), theme::fg(dark)));
        }
    }
}

fn show_fit_json(ui: &mut egui::Ui, bv: &mut BundleViewState, dark: bool) {
    if bv.info.fit.is_null() {
        ui.label(egui::RichText::new("fit.json is missing or is not valid JSON.").color(theme::RED).size(12.0));
        return;
    }
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Filter keys:").color(theme::fg2(dark)).size(11.0));
        ui.add(egui::TextEdit::singleline(&mut bv.json_filter).desired_width(160.0).hint_text("e.g. theta"));
        if ui.small_button("Copy JSON").clicked() {
            if let Ok(t) = serde_json::to_string_pretty(&bv.info.fit) {
                ui.ctx().copy_text(t);
                bv.status = "fit.json copied".to_string();
            }
        }
    });
    ui.add_space(4.0);
    let needle = bv.json_filter.trim().to_lowercase();
    if let Value::Object(m) = &bv.info.fit {
        for (k, v) in m {
            if needle.is_empty() || k.to_lowercase().contains(&needle) {
                json_tree(ui, k, v, dark);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;

    #[test]
    fn formats_durations_and_sizes() {
        assert_eq!(fmt_duration(10200.22), "2h 50m 00s");
        assert_eq!(fmt_duration(75.0), "1m 15s");
        assert_eq!(fmt_duration(9.4), "9s");
        assert_eq!(fmt_duration(f64::NAN), "—");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(163182), "159.4 KB");
        assert_eq!(human_bytes(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn json_helpers_describe_values() {
        let v: Value = serde_json::from_str(r#"{"a":[1,2.5,"x"],"b":[[1],[2]],"c":null}"#).unwrap();
        assert_eq!(inline_array(v["a"].as_array().unwrap()).as_deref(), Some("[1, 2.5, \"x\"]"));
        assert_eq!(inline_array(v["b"].as_array().unwrap()), None);
        assert_eq!(scalar_text(&v["c"]), "null");
        let long: Vec<Value> = (0..20).map(Value::from).collect();
        assert_eq!(inline_array(&long), None);
        assert_eq!(short_hash(Some("0123456789abcdef0123")), "0123456789abcdef…");
        assert_eq!(short_hash(None), "—");
    }

    #[test]
    fn sig_picks_a_readable_form() {
        assert_eq!(sig(0.13), "0.13");
        assert_eq!(sig(0.0), "0");
        assert!(sig(1.2e-7).contains('e'));
        assert_eq!(sig(f64::NAN), "—");
    }

    // ── Headless rendering of every section ──

    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;
    use std::io::Write;

    fn write_bundle(path: &std::path::Path) {
        let fit = r#"{"method":"foce","method_chain":["foce"],"converged":true,"ofv":-280.4,"aic":-264.4,"bic":-242.8,
            "n_obs":110,"n_subjects":10,"n_parameters":8,"n_iterations":45,"wall_time_secs":3725.0,"n_threads_used":8,
            "covariance_status":"computed","cov_condition_number":2.7,"dw_statistic":2.6,"shrinkage_eps":0.16,
            "theta":{"names":["TVCL","TVV"],"estimates":[0.133,7.73],"se":[0.0067,0.236],"fixed":[false,true]},
            "theta_init":[0.134,8.1],
            "omega":{"names":["ETA_CL","ETA_V"],"matrix":{"rows":2,"cols":2,"data":[0.0286,0.0018,0.0018,0.0096]},
                     "se":[0.0128,0.0053,0.0043],"shrinkage":[0.31,0.02]},
            "sigma":{"names":["PROP"],"estimates":[0.0107],"se":[0.00095]},
            "omega_is_diagonal":false,"input_columns":["ID","TIME","DV"],"warnings":["w1"],
            "model_hash":"abcdef0123456789abcdef","data_hash":"0123456789abcdef0123","data_path":"/nope/d.csv",
            "model_path":"/nope/m.ferx"}"#;
        let f = std::fs::File::create(path).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let o = zip::write::SimpleFileOptions::default();
        for (n, b) in [("manifest.json", r#"{"format_version":"1","ferx_version":"0.4.0","model_name":"m","created_at":"2026-10-07T11:21:43Z"}"#),
                       ("fit.json", fit), ("model.ferx", "[parameters]\n  theta TVCL(0.134, 0.001, 10)\n"),
                       ("warnings.txt", "w1\n"), ("predictions.csv", "ID,TIME,DV\n1,0.5,3.2\n")] {
            z.start_file(n, o).unwrap();
            z.write_all(b.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }

    fn render(section: BundleSection, bundle: &std::path::Path) -> Harness<'static> {
        let mut st = AppState::new();
        let mut bv = BundleViewState::new(crate::io::bundle::inspect(bundle).unwrap());
        bv.section = section;
        if section == BundleSection::Contents {
            bv.selected_entry = Some("predictions.csv".into());
            bv.preview = Some(crate::io::bundle::read_entry_preview(bundle, "predictions.csv"));
        }
        st.ui.files_bundle = Some(bv);
        let mut h = Harness::builder().with_size(egui::vec2(900.0, 1400.0))
            .build_ui(move |ui| show(ui, &mut st, true));
        h.run();
        h
    }

    #[test]
    fn every_section_renders_its_key_content() {
        let d = std::env::temp_dir().join("ferxgui_bundle_view_test");
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let b = d.join("m.fitrx");
        write_bundle(&b);

        let h = render(BundleSection::Overview, &b);
        for needle in ["PROVENANCE", "RUN", "FIT", "COVARIANCE", "DIAGNOSTICS", "DATA", "WARNINGS",
                       "Verify data file", "1h 02m 05s", "foce", "ferx version"] {
            assert!(h.query_by_label_contains(needle).is_some(), "Overview is missing {needle}");
        }
        let h = render(BundleSection::Parameters, &b);
        for needle in ["THETA  (2)", "OMEGA  (2 ETA, block)", "SIGMA  (1)", "TVCL", "fixed", "ETA_V ~ ETA_CL"] {
            assert!(h.query_by_label_contains(needle).is_some(), "Parameters is missing {needle}");
        }
        let h = render(BundleSection::Model, &b);
        assert!(h.query_by_label_contains("model.ferx as fitted").is_some());
        let h = render(BundleSection::Contents, &b);
        for needle in ["ARCHIVE", "predictions.csv", "Save entry as", "1 rows"] {
            assert!(h.query_all_by_label_contains(needle).next().is_some(), "Contents is missing {needle}");
        }
        let h = render(BundleSection::FitJson, &b);
        assert!(h.query_by_label_contains("Filter keys").is_some());
        assert!(h.query_by_label_contains("method: \"foce\"").is_some());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_unreadable_bundle_shows_the_reason() {
        let mut st = AppState::new();
        st.ui.files_bundle_error = Some("not a readable zip archive: invalid Zip archive".into());
        let mut h = Harness::new_ui(move |ui| show(ui, &mut st, true));
        h.run();
        assert!(h.query_by_label_contains("Could not read this .fitrx bundle").is_some());
        assert!(h.query_by_label_contains("not a readable zip").is_some());
    }

    /// With `FERX_TEST_FLEXPROVE` set: the real bundle renders every section.
    #[test]
    fn real_bundle_renders() {
        let Ok(p) = std::env::var("FERX_TEST_FLEXPROVE") else { return };
        let p = std::path::PathBuf::from(p);
        for sec in [BundleSection::Overview, BundleSection::Parameters, BundleSection::Model,
                    BundleSection::Contents, BundleSection::FitJson] {
            let h = render(sec, &p);
            assert!(h.query_by_label_contains("Overview").is_some());
        }
        let h = render(BundleSection::Overview, &p);
        assert!(h.query_by_label_contains("2h 50m 00s").is_some(), "wall time of the real fit");
        assert!(h.query_by_label_contains("ETA_EMAX 31.6%").is_some(), "shrinkage shown as percent");
    }
}
