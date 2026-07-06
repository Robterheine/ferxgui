/// SIR (Sampling Importance Resampling) tab.
///
/// Three sections:
///   0. CI Comparison  — point estimate vs asymptotic vs SIR 95% CI
///   1. Correlations   — SIR empirical correlation heatmap
///   2. Distributions  — per-parameter histogram with vertical markers
use std::f64::consts::TAU;

use eframe::egui;
use egui_plot::{Bar, BarChart, Line, Plot, PlotPoints, VLine};

use crate::app::theme;
use crate::domain::SirResult;
use crate::state::AppState;
use crate::workers::messages::WorkerMsg;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;

    let model_idx = match state.ui.selected_model {
        Some(i) => i,
        None => return hint(ui, dark, "Select a fitted model in the Models tab to run SIR."),
    };

    let fit = match &state.workspace.models[model_idx].fit {
        Some(f) if f.covariance_ok => f.clone(),
        Some(_) => return hint(ui, dark, "SIR requires a completed covariance step.\nRe-run with Covariance enabled."),
        None     => return hint(ui, dark, "No fit available. Run the model first."),
    };

    let fitrx_path = match &state.workspace.models[model_idx].fitrx_path {
        Some(p) => p.clone(),
        None    => return hint(ui, dark, "No .fitrx bundle found for this model."),
    };

    let stem    = state.workspace.models[model_idx].model.stem.clone();
    let running = state.workspace.sir_running.contains(&stem);
    let has_res = state.workspace.sir_results.contains_key(&stem);

    egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
        // Which model this SIR run belongs to — mirrors the Evaluation tab
        // so it's never ambiguous which model's fit is being visualised.
        ui.label(egui::RichText::new(&stem).size(12.0).strong().color(theme::fg(dark)));
        ui.add_space(6.0);

        // ── Settings card ─────────────────────────────────────────────────
        egui::Frame::new()
            .fill(theme::card_fill(dark))
            .inner_margin(egui::Margin::same(12))
            .corner_radius(egui::CornerRadius::same(6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let dim = theme::fg2(dark);
                ui.label(egui::RichText::new("SIR settings").color(dim).size(11.0));
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Samples:").color(dim).size(12.0));
                    ui.add(egui::DragValue::new(&mut state.ui.sir_n_samples).range(100..=10_000).speed(50.0));
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new("Resamples:").color(dim).size(12.0));
                    ui.add(egui::DragValue::new(&mut state.ui.sir_n_resamples)
                        .range(50..=state.ui.sir_n_samples).speed(10.0));
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new("Seed:").color(dim).size(12.0));
                    ui.add(egui::DragValue::new(&mut state.ui.sir_seed).range(0..=99_999));
                });

                ui.horizontal(|ui| {
                    ui.checkbox(&mut state.ui.sir_keep_samples, "Keep resamples")
                        .on_hover_text(
                            "Enables the Correlations and Distributions sections.\n\
                             Stores all resampled parameter vectors (negligible overhead).",
                        );
                });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.add_enabled(
                        !running,
                        egui::Button::new(egui::RichText::new("▶  Run SIR").size(13.0))
                            .fill(theme::ACCENT)
                            .min_size(egui::vec2(110.0, 30.0)),
                    ).clicked() {
                        state.workspace.sir_results.remove(&stem);
                        state.workspace.sir_running.insert(stem.clone());
                        state.ui.sir_selected_param.clear();

                        let tx           = state.worker_tx.clone();
                        let ctx          = ui.ctx().clone();
                        let stem_cl      = stem.clone();
                        let n_samples    = state.ui.sir_n_samples;
                        let n_resamples  = state.ui.sir_n_resamples;
                        let seed         = state.ui.sir_seed;
                        let keep         = state.ui.sir_keep_samples;
                        let fitrx_cl     = fitrx_path.clone();

                        std::thread::spawn(move || {
                            match crate::io::r_extract::compute_sir(
                                &fitrx_cl, n_samples, n_resamples, seed, keep)
                            {
                                Ok(result) => {
                                    let _ = tx.send(WorkerMsg::SirComplete {
                                        stem: stem_cl, result: Box::new(result),
                                    });
                                }
                                Err(e) => {
                                    let _ = tx.send(WorkerMsg::RTaskError {
                                        context: format!("sir:manual:{stem_cl}"),
                                        message: e,
                                    });
                                }
                            }
                            ctx.request_repaint();
                        });
                    }

                    if running {
                        ui.add_space(8.0);
                        ui.spinner();
                        ui.label(egui::RichText::new("Running SIR…").color(dim).size(11.0));
                    }
                    if has_res {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Clear").clicked() {
                                state.workspace.sir_results.remove(&stem);
                            }
                        });
                    }
                });
            });

        // Nothing more to show until results arrive.
        let sir = match state.workspace.sir_results.get(&stem).cloned() {
            Some(r) => r,
            None    => return,
        };

        ui.add_space(12.0);

        // ── ESS ───────────────────────────────────────────────────────────
        let ess_pct = if state.ui.sir_n_resamples > 0 {
            sir.sir_ess / state.ui.sir_n_resamples as f64 * 100.0
        } else { 0.0 };
        let ess_col = if ess_pct < 20.0 { theme::RED }
                      else if ess_pct < 40.0 { theme::ORANGE }
                      else { theme::GREEN };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Effective sample size:").color(theme::fg2(dark)).size(12.0));
            ui.label(egui::RichText::new(
                format!("{:.1}  /  {}  ({:.0}%)", sir.sir_ess, state.ui.sir_n_resamples, ess_pct))
                .color(ess_col).size(12.0).strong());
        });
        if ess_pct < 20.0 {
            ui.label(egui::RichText::new(
                "⚠  Low ESS — increase Samples, or the posterior departs strongly from normality.")
                .color(theme::ORANGE).size(11.0));
        }

        ui.add_space(10.0);

        // ── Section picker ────────────────────────────────────────────────
        let has_corr = !sir.corr_flat.is_empty();
        let _has_dist = !sir.param_samples.is_empty();
        let n_sections = if has_corr { 3 } else { 1 };
        // Clamp view index in case resamples weren't kept.
        if state.ui.sir_view_idx > 0 && !has_corr {
            state.ui.sir_view_idx = 0;
        }

        ui.horizontal(|ui| {
            let inactive_fill = if dark { theme::BG3 } else { egui::Color32::TRANSPARENT };
            let inactive_fg   = theme::fg2(dark);
            for (i, label) in ["CI Comparison", "Correlations", "Distributions"]
                .iter().enumerate().take(n_sections)
            {
                let active = state.ui.sir_view_idx == i;
                if ui.add(
                    egui::Button::new(egui::RichText::new(*label).size(11.0)
                        .color(if active { egui::Color32::WHITE } else { inactive_fg }))
                    .fill(if active { theme::ACCENT } else { inactive_fill })
                    .min_size(egui::vec2(0.0, 24.0)),
                ).clicked() {
                    state.ui.sir_view_idx = i;
                }
            }
        });
        ui.add_space(6.0);

        match state.ui.sir_view_idx {
            0 => show_ci_comparison(ui, &sir, &fit, dark),
            1 => show_correlation_heatmap(ui, &sir, dark),
            2 => show_distributions(ui, state, &sir, &fit, dark),
            _ => {}
        }
    });
}

// ---------------------------------------------------------------------------
// Section 0 — CI comparison
// ---------------------------------------------------------------------------

fn show_ci_comparison(
    ui:   &mut egui::Ui,
    sir:  &SirResult,
    fit:  &crate::domain::FitSummary,
    dark: bool,
) {
    for (section_title, cis, ests, ses) in [
        ("THETA",         sir.theta.as_slice(), fit.theta.as_slice(),  fit.se_theta.as_slice()),
        ("OMEGA (diag)",  sir.omega.as_slice(), omega_diag_vec(fit).leak(), fit.se_omega.as_slice()),
        ("SIGMA",         sir.sigma.as_slice(), fit.sigma.as_slice(),  fit.se_sigma.as_slice()),
    ] {
        if cis.is_empty() { continue; }
        sir_section_header(ui, section_title, dark);
        ui.add_space(2.0);

        egui::Grid::new(egui::Id::new(ui.next_auto_id()))
            .num_columns(7)
            .spacing([10.0, 4.0])
            .min_col_width(50.0)
            .show(ui, |ui| {
                for h in ["PARAM","ESTIMATE","ASYM 2.5%","ASYM 97.5%","SIR 2.5%","SIR 97.5%","ASYM vs SIR"] {
                    ui.label(egui::RichText::new(h).color(theme::fg2(dark)).size(10.0).strong());
                }
                ui.end_row();

                for (i, ci) in cis.iter().enumerate() {
                    let est     = ests.get(i).copied().unwrap_or(f64::NAN);
                    let se      = ses.get(i).copied().unwrap_or(f64::NAN);
                    let asym_lo = if se.is_finite() { est - 1.96 * se } else { f64::NAN };
                    let asym_hi = if se.is_finite() { est + 1.96 * se } else { f64::NAN };

                    ui.label(egui::RichText::new(&ci.name).color(theme::fg(dark)).size(12.0).monospace());
                    ui.label(egui::RichText::new(fmt4(est)).color(theme::fg(dark)).size(11.0));
                    ui.label(egui::RichText::new(fmt4(asym_lo)).color(theme::fg2(dark)).size(11.0));
                    ui.label(egui::RichText::new(fmt4(asym_hi)).color(theme::fg2(dark)).size(11.0));
                    ui.label(egui::RichText::new(fmt4(ci.lo)).color(theme::ACCENT).size(11.0));
                    ui.label(egui::RichText::new(fmt4(ci.hi)).color(theme::ACCENT).size(11.0));

                    let asym_w = asym_hi - asym_lo;
                    let sir_w  = ci.hi - ci.lo;
                    let (sym, col) = if asym_w > 0.0 && sir_w > 0.0 {
                        let ratio = sir_w / asym_w;
                        if ratio > 1.5        { ("▲ wider",    theme::ORANGE) }
                        else if ratio < 0.67  { ("▼ narrower", theme::YELLOW) }
                        else                  { ("≈ similar",  theme::fg3(dark)) }
                    } else {
                        ("—", theme::fg3(dark))
                    };
                    ui.label(egui::RichText::new(sym).color(col).size(10.0));
                    ui.end_row();
                }
            });
        ui.add_space(8.0);
    }
    ui.label(egui::RichText::new(
        "Asymptotic: estimate ± 1.96 × SE   |   SIR: non-parametric 2.5% – 97.5%")
        .color(theme::fg3(dark)).size(10.0));
}

// ---------------------------------------------------------------------------
// Section 1 — SIR correlation heatmap
// ---------------------------------------------------------------------------

/// Render a named N×N correlation heatmap.  `flat` is row-major, `n` is dimension.
pub(crate) fn correlation_heatmap(
    ui: &mut egui::Ui,
    names: &[String],
    flat: &[f64],
    n: usize,
    dark: bool,
) {
    if n == 0 || flat.len() != n * n {
        ui.label(egui::RichText::new("Correlation data not available.")
            .color(crate::app::theme::fg3(dark)).size(12.0));
        return;
    }

    let label_w = 70.0_f32;
    let avail   = (ui.available_width() - label_w).max(200.0);
    let cell    = (avail / n as f32).clamp(28.0, 70.0);
    let total_w = cell * n as f32;

    ui.horizontal(|ui| {
        ui.add_space(label_w);
        for name in names.iter().take(n) {
            let abbrev = name.chars().take(8).collect::<String>();
            ui.add_sized(egui::vec2(cell, 14.0),
                egui::Label::new(egui::RichText::new(abbrev)
                    .color(crate::app::theme::fg2(dark)).size(9.0).strong()));
        }
    });

    for row in 0..n {
        ui.horizontal(|ui| {
            let rname = names.get(row).map(|s| s.as_str()).unwrap_or("");
            ui.add_sized(egui::vec2(label_w, cell),
                egui::Label::new(egui::RichText::new(rname)
                    .color(crate::app::theme::fg2(dark)).size(10.0).monospace()));
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(total_w, cell), egui::Sense::hover());
            for col in 0..n {
                let r = flat.get(row * n + col).copied().unwrap_or(0.0);
                let cell_rect = egui::Rect::from_min_size(
                    rect.min + egui::vec2(col as f32 * cell, 0.0),
                    egui::vec2(cell - 1.0, cell - 1.0),
                );
                let fill = corr_color(r);
                ui.painter().rect_filled(cell_rect, 2.0, fill);
                if cell >= 36.0 {
                    ui.painter().text(cell_rect.center(),
                        egui::Align2::CENTER_CENTER, format!("{r:.2}"),
                        egui::FontId::proportional(9.0), contrast_text(fill));
                }
                let rn = names.get(row).map(|s| s.as_str()).unwrap_or("?");
                let cn = names.get(col).map(|s| s.as_str()).unwrap_or("?");
                ui.allocate_rect(cell_rect, egui::Sense::hover())
                    .on_hover_text(format!("{rn} ~ {cn}:  r = {r:.4}"));
            }
        });
    }

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        for (label, val) in [("-1.0",-1.0_f64),("-0.5",-0.5),("0",0.0),("+0.5",0.5),("+1.0",1.0)] {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0,10.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 2.0, corr_color(val));
            ui.label(egui::RichText::new(label).color(crate::app::theme::fg3(dark)).size(9.0));
        }
        ui.label(egui::RichText::new("← red = negative,  white = zero,  blue = positive")
            .color(crate::app::theme::fg3(dark)).size(9.0));
    });
}

fn show_correlation_heatmap(ui: &mut egui::Ui, sir: &SirResult, dark: bool) {
    if sir.corr_dim == 0 || sir.corr_flat.len() != sir.corr_dim * sir.corr_dim {
        ui.label(egui::RichText::new(
            "Correlation data not available.\nRun SIR with 'Keep resamples' enabled.")
            .color(theme::fg3(dark)).size(12.0));
        return;
    }
    correlation_heatmap(ui, &sir.corr_names, &sir.corr_flat, sir.corr_dim, dark);
}

/// Diverging navy–white–crimson colormap for r ∈ [-1, +1].
pub(crate) fn corr_color(r: f64) -> egui::Color32 {
    let t = r.clamp(-1.0, 1.0);
    if t >= 0.0 {
        let f = t as f32;
        // near-white (0) → dark navy (1):  rgb(8, 64, 150)
        egui::Color32::from_rgb(
            (255.0 * (1.0 - f * 0.969)) as u8,  // 255 → 8
            (255.0 * (1.0 - f * 0.749)) as u8,  // 255 → 64
            (255.0 * (1.0 - f * 0.412)) as u8,  // 255 → 150
        )
    } else {
        let f = (-t) as f32;
        // near-white (0) → dark crimson (-1):  rgb(160, 5, 15)
        egui::Color32::from_rgb(
            (255.0 * (1.0 - f * 0.373)) as u8,  // 255 → 160
            (255.0 * (1.0 - f * 0.980)) as u8,  // 255 → 5
            (255.0 * (1.0 - f * 0.941)) as u8,  // 255 → 15
        )
    }
}

/// Choose readable text colour for a given background — white on dark, near-black on light.
pub(crate) fn contrast_text(bg: egui::Color32) -> egui::Color32 {
    let lum = (0.299 * bg.r() as f32
             + 0.587 * bg.g() as f32
             + 0.114 * bg.b() as f32) / 255.0;
    // Threshold ≈ 0.35 puts the switch at ≈ |r| = 0.55 on both branches,
    // where white contrast ≥ 4.5:1 against the dark cell and dark text
    // ≥ 4.5:1 against the light cell.
    if lum < 0.35 {
        egui::Color32::WHITE
    } else {
        egui::Color32::from_rgb(12, 14, 24)  // near-black
    }
}

// ---------------------------------------------------------------------------
// Section 2 — Distribution histogram
// ---------------------------------------------------------------------------

fn show_distributions(
    ui:    &mut egui::Ui,
    state: &mut AppState,
    sir:   &SirResult,
    fit:   &crate::domain::FitSummary,
    dark:  bool,
) {
    if sir.param_samples.is_empty() {
        ui.label(egui::RichText::new(
            "Sample data not available.\nRun SIR with 'Keep resamples' enabled.")
            .color(theme::fg3(dark)).size(12.0));
        return;
    }

    // Build sorted param list.
    let param_list: Vec<String> = sir.corr_names.iter()
        .filter(|n| sir.param_samples.contains_key(*n))
        .cloned()
        .collect();
    if param_list.is_empty() { return; }

    // Initialise selection.
    if state.ui.sir_selected_param.is_empty()
        || !sir.param_samples.contains_key(&state.ui.sir_selected_param)
    {
        state.ui.sir_selected_param = param_list[0].clone();
    }

    let dim = theme::fg2(dark);

    // Parameter picker.
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Parameter:").color(dim).size(12.0));
        egui::ComboBox::from_id_salt("sir_param_picker")
            .selected_text(&state.ui.sir_selected_param)
            .width(140.0)
            .show_ui(ui, |ui| {
                for p in &param_list {
                    ui.selectable_value(&mut state.ui.sir_selected_param, p.clone(), p);
                }
            });
    });
    ui.add_space(8.0);

    let param = &state.ui.sir_selected_param;
    let samples = match sir.param_samples.get(param) {
        Some(s) if !s.is_empty() => s.clone(),
        _ => return,
    };

    // Look up estimate + SE + SIR CI for this parameter.
    let (est, se) = find_estimate_se(fit, param);
    let sir_ci    = find_sir_ci(sir, param);

    let n = samples.len();
    let n_bins = ((n as f64).sqrt() as usize).clamp(8, 40);

    let s_min = samples.iter().cloned().fold(f64::INFINITY, f64::min);
    let s_max = samples.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range  = (s_max - s_min).max(1e-10);

    // Expand range for the plot (add a little margin).
    let margin = range * 0.12;
    let x_lo = s_min - margin;
    let x_hi = s_max + margin;

    // Compute histogram bins (density scale).
    let bin_w = range / n_bins as f64;
    let mut counts = vec![0usize; n_bins];
    for &v in &samples {
        let idx = ((v - s_min) / bin_w).floor() as usize;
        counts[idx.min(n_bins - 1)] += 1;
    }
    let bars: Vec<Bar> = counts.iter().enumerate()
        .map(|(i, &c)| {
            let x = s_min + (i as f64 + 0.5) * bin_w;
            Bar::new(x, c as f64 / (n as f64 * bin_w))
                .width(bin_w * 0.9)
                .fill(egui::Color32::from_rgba_unmultiplied(0x4c, 0x8a, 0xff, 140))
        })
        .collect();

    // Asymptotic normal overlay (when SE available).
    let normal_pts: Option<Vec<[f64; 2]>> = if let (Some(mu), Some(se_v)) = (est, se) {
        if se_v.is_finite() && se_v > 0.0 {
            let pts: Vec<[f64; 2]> = (0..=120)
                .map(|i| {
                    let x = x_lo + (x_hi - x_lo) * i as f64 / 120.0;
                    let y = normal_pdf(x, mu, se_v);
                    [x, y]
                })
                .collect();
            Some(pts)
        } else { None }
    } else { None };

    let plot_h = (ui.available_height() - 60.0).clamp(200.0, 420.0);
    Plot::new("sir_dist_plot")
        .height(plot_h)
        .width(ui.available_width())
        .x_axis_label(param.as_str())
        .y_axis_label("Density")
        .show_grid(true)
        .allow_drag(false)
        .allow_zoom(false)
        .legend(egui_plot::Legend::default())
        .show(ui, |p| {
            // Histogram bars.
            p.bar_chart(BarChart::new(bars).name("SIR samples"));

            // Asymptotic normal curve.
            if let Some(pts) = normal_pts {
                p.line(Line::new(PlotPoints::new(pts))
                    .color(egui::Color32::from_rgba_unmultiplied(180, 180, 180, 220))
                    .width(1.5)
                    .style(egui_plot::LineStyle::Dashed { length: 8.0 })
                    .name("Asymptotic normal"));
            }

            // Point estimate (solid dark line).
            if let Some(mu) = est {
                p.vline(VLine::new(mu)
                    .color(theme::fg(dark))
                    .width(2.0)
                    .name("Estimate"));
            }

            // Asymptotic 95% CI (dashed gray).
            if let (Some(mu), Some(se_v)) = (est, se) {
                if se_v.is_finite() {
                    let gray = egui::Color32::from_rgba_unmultiplied(150, 150, 150, 200);
                    p.vline(VLine::new(mu - 1.96 * se_v)
                        .color(gray).width(1.2)
                        .style(egui_plot::LineStyle::Dashed { length: 6.0 })
                        .name("Asym 95% CI"));
                    p.vline(VLine::new(mu + 1.96 * se_v)
                        .color(gray).width(1.2)
                        .style(egui_plot::LineStyle::Dashed { length: 6.0 }));
                }
            }

            // SIR 95% CI (solid orange).
            if let Some((lo, hi)) = sir_ci {
                p.vline(VLine::new(lo)
                    .color(theme::ORANGE).width(1.8)
                    .name("SIR 95% CI"));
                p.vline(VLine::new(hi)
                    .color(theme::ORANGE).width(1.8));
            }
        });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn omega_diag_vec(fit: &crate::domain::FitSummary) -> Vec<f64> {
    (0..fit.n_eta)
        .map(|i| fit.omega_value(i, i).unwrap_or(f64::NAN))
        .collect()
}

fn find_estimate_se(fit: &crate::domain::FitSummary, name: &str) -> (Option<f64>, Option<f64>) {
    if let Some(i) = fit.theta_names.iter().position(|n| n == name) {
        return (fit.theta.get(i).copied(), fit.se_theta.get(i).copied());
    }
    if let Some(i) = fit.omega_names.iter().position(|n| n == name) {
        return (fit.omega_value(i, i), fit.se_omega.get(i).copied());
    }
    if let Some(i) = fit.sigma_names.iter().position(|n| n == name) {
        return (fit.sigma.get(i).copied(), fit.se_sigma.get(i).copied());
    }
    (None, None)
}

fn find_sir_ci(sir: &SirResult, name: &str) -> Option<(f64, f64)> {
    for group in [&sir.theta, &sir.omega, &sir.sigma] {
        if let Some(ci) = group.iter().find(|c| c.name == name) {
            return Some((ci.lo, ci.hi));
        }
    }
    None
}

fn normal_pdf(x: f64, mean: f64, std: f64) -> f64 {
    let z = (x - mean) / std;
    (1.0 / (std * (TAU / 2.0).sqrt() * std::f64::consts::SQRT_2)) * (-0.5 * z * z).exp()
}

fn sir_section_header(ui: &mut egui::Ui, title: &str, dark: bool) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 22.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::elevated_fill(dark));
    ui.painter().text(
        rect.left_center() + egui::vec2(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(11.0),
        theme::fg2(dark),
    );
    ui.add_space(4.0);
}

fn fmt4(v: f64) -> String {
    if v.is_nan() { "—".to_string() } else { format!("{v:.4}") }
}

fn hint(ui: &mut egui::Ui, dark: bool, msg: &str) {
    let color = if dark { theme::FG3 } else { egui::Color32::from_gray(150) };
    ui.centered_and_justified(|ui| {
        ui.label(egui::RichText::new(msg).color(color).size(13.0));
    });
}
