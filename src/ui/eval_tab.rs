use eframe::egui;
use egui_plot::{Bar, BarChart, HLine, Line, Plot, PlotPoints, Points};

use crate::app::theme;
use crate::domain::TraceRow;
use crate::state::{AppState, EvalSection};

// ── Public entry point ────────────────────────────────────────────────────────

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;

    let model_idx = match state.ui.selected_model {
        Some(i) => i,
        None => {
            show_no_model(ui, state, dark);
            return;
        }
    };

    let has_fit = state.workspace.models[model_idx].fit.is_some();

    // ── Section segmented control ─────────────────────────────────────────
    ui.horizontal(|ui| {
        // Model name as a non-interactive label — selection is driven by the Models tab.
        let stem = state.workspace.models[model_idx].model.stem.as_str();
        ui.label(egui::RichText::new(stem).size(12.0).strong().color(theme::fg(dark)));

        ui.add_space(8.0);

        let inactive_fill = if dark { theme::BG3 } else { egui::Color32::TRANSPARENT };
        let inactive_fg   = theme::fg2(dark);
        for (label, section) in [
            ("GOF",             EvalSection::Gof),
            ("Individual Fits", EvalSection::IndividualFits),
            ("iOFV Waterfall",  EvalSection::OfvWaterfall),
            ("Convergence",     EvalSection::Convergence),
            ("ETA-Cov",         EvalSection::EtaCov),
            ("Param Corr",      EvalSection::ParamCorr),
        ] {
            let active = state.ui.active_eval_section == section;
            if ui.add(
                egui::Button::new(egui::RichText::new(label).size(11.0)
                    .color(if active { egui::Color32::WHITE } else { inactive_fg }))
                .fill(if active { theme::ACCENT } else { inactive_fill })
                .min_size(egui::vec2(0.0, 22.0)),
            ).clicked() {
                state.ui.active_eval_section = section;
            }
        }

        if state.ui.active_eval_section == EvalSection::Gof {
            // Log-scale toggle.
            ui.add_space(12.0);
            ui.checkbox(&mut state.ui.eval_log_scale, "Log scale");

            // Independent CWRES x-axis pickers.
            let dim = theme::fg2(dark);
            ui.add_space(10.0);
            ui.label(egui::RichText::new("CWRES₁ x:").color(dim).size(11.0));
            egui::ComboBox::from_id_salt("cwres_x1_combo")
                .selected_text(&state.ui.eval_cwres_x_col)
                .width(70.0)
                .show_ui(ui, |ui| {
                    for opt in ["TIME", "PRED", "IPRED"] {
                        ui.selectable_value(&mut state.ui.eval_cwres_x_col, opt.to_string(), opt);
                    }
                });
            ui.add_space(6.0);
            ui.label(egui::RichText::new("CWRES₂ x:").color(dim).size(11.0));
            egui::ComboBox::from_id_salt("cwres_x2_combo")
                .selected_text(&state.ui.eval_cwres_x_col_2)
                .width(70.0)
                .show_ui(ui, |ui| {
                    for opt in ["PRED", "TIME", "IPRED"] {
                        ui.selectable_value(&mut state.ui.eval_cwres_x_col_2, opt.to_string(), opt);
                    }
                });

            // Export button (right-aligned).
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(
                    egui::Button::new(egui::RichText::new("⬇ Export figure…").size(11.0))
                        .fill(theme::card_fill(dark))
                        .min_size(egui::vec2(0.0, 22.0)),
                ).clicked() {
                    state.ui.eval_export_dialog = true;
                }
            });
        }

        // Subjects per page selector (Individual Fits only).
        if state.ui.active_eval_section == EvalSection::IndividualFits {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Per page:").color(theme::fg2(dark)).size(11.0));
            egui::ComboBox::from_id_salt("spp_combo")
                .selected_text(state.ui.eval_subjects_per_page.to_string())
                .width(42.0)
                .show_ui(ui, |ui| {
                    for n in 1usize..=6 {
                        if ui.selectable_label(state.ui.eval_subjects_per_page == n,
                            n.to_string()).clicked() {
                            state.ui.eval_subjects_per_page = n;
                            state.ui.eval_subject_idx = 0;
                        }
                    }
                });
        }
    });
    ui.separator();

    // ── Lazy-load predictions + ebes ─────────────────────────────────────
    if has_fit {
        let stem = state.workspace.models[model_idx].model.stem.clone();
        if state.ui.eval_loaded_stem.as_deref() != Some(&stem) {
            let fitrx = state.workspace.models[model_idx].fitrx_path.clone();
            state.ui.eval_data = fitrx.as_deref()
                .and_then(|p| crate::io::fitrx::read_predictions(p).ok().flatten());
            state.ui.eval_ebes = fitrx.as_deref()
                .and_then(|p| crate::io::fitrx::read_ebes(p).ok().flatten());
            state.ui.eval_loaded_stem = Some(stem);
            state.ui.eval_subject_idx = 0;
        }
    }

    // ── Export dialog (floats above everything) ───────────────────────────
    show_export_dialog(ui.ctx(), state, model_idx);

    // ── Route to section ──────────────────────────────────────────────────
    match state.ui.active_eval_section {
        EvalSection::Gof            => show_gof(ui, state, model_idx, dark),
        EvalSection::IndividualFits => show_individual_fits(ui, state, model_idx, dark),
        EvalSection::OfvWaterfall   => show_iofv_waterfall(ui, state, dark),
        EvalSection::Convergence    => show_convergence(ui, state, model_idx, dark),
        EvalSection::EtaCov         => show_eta_cov(ui, state, model_idx, dark),
        EvalSection::ParamCorr      => show_param_corr(ui, state, model_idx, dark),
    }
}

// ── LOESS helper ─────────────────────────────────────────────────────────────

/// Gaussian-kernel locally-weighted smoother.  Returns ~60 (x, y) points
/// spanning the data range.  `bandwidth_frac` controls the kernel width
/// as a fraction of the x range (0.3 = 30% is a reasonable default).
pub(crate) fn loess(points: &[[f64; 2]], bandwidth_frac: f64) -> Vec<[f64; 2]> {
    if points.len() < 4 { return vec![]; }
    let xs: Vec<f64> = points.iter().map(|p| p[0]).collect();
    let ys: Vec<f64> = points.iter().map(|p| p[1]).collect();
    let x_min = xs.iter().cloned().fold(f64::INFINITY,     f64::min);
    let x_max = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = x_max - x_min;
    if range < 1e-10 { return vec![]; }
    let h = range * bandwidth_frac;

    (0..=60).filter_map(|i| {
        let x0 = x_min + range * i as f64 / 60.0;
        // Weighted sums for local linear regression.
        let (mut sw, mut swx, mut swy, mut swxx, mut swxy) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for (&x, &y) in xs.iter().zip(ys.iter()) {
            if y.is_finite() {
                let d = (x - x0) / h;
                let w = (-0.5 * d * d).exp();
                sw   += w;
                swx  += w * x;
                swy  += w * y;
                swxx += w * x * x;
                swxy += w * x * y;
            }
        }
        if sw < 1e-10 { return None; }
        let denom = sw * swxx - swx * swx;
        let y_fit = if denom.abs() < 1e-10 {
            swy / sw
        } else {
            let b0 = (swxx * swy - swx * swxy) / denom;
            let b1 = (sw  * swxy - swx * swy)  / denom;
            b0 + b1 * x0
        };
        if y_fit.is_finite() { Some([x0, y_fit]) } else { None }
    }).collect()
}

// ── GOF 2×2 ──────────────────────────────────────────────────────────────────

fn show_gof(ui: &mut egui::Ui, state: &AppState, _idx: usize, dark: bool) {
    let data = match &state.ui.eval_data {
        Some(d) if !d.rows.is_empty() => d,
        _ => { no_predictions(ui, dark); return; }
    };

    let log = state.ui.eval_log_scale;
    let avail = ui.available_size();
    let half_w = (avail.x / 2.0 - 6.0).max(150.0);
    let half_h = (avail.y / 2.0 - 6.0).max(150.0);

    let pt_col  = if dark { egui::Color32::from_rgba_unmultiplied(76,138,255,200) }
                  else    { egui::Color32::from_rgba_unmultiplied(30, 90,210,180) };
    let ref_col = if dark { egui::Color32::from_gray(120) } else { egui::Color32::from_gray(160) };
    let loess_col = theme::ORANGE;

    let [dv_lo, dv_hi] = data.dv_pred_range();
    let pad   = (dv_hi - dv_lo) * 0.05;
    let ax_lo = if log { (dv_lo - pad).max(1e-6) } else { dv_lo - pad };
    let ax_hi = dv_hi + pad;

    // Collect scatter data.
    let pts_dv_pred:  Vec<[f64;2]> = data.rows.iter()
        .filter(|r| r.pred.is_finite() && r.dv.is_finite())
        .map(|r| [r.pred, r.dv]).collect();
    let pts_dv_ipred: Vec<[f64;2]> = data.rows.iter()
        .filter(|r| r.ipred.is_finite() && r.dv.is_finite())
        .map(|r| [r.ipred, r.dv]).collect();

    // CWRES X column.
    let cwres_x: Vec<[f64;2]> = data.rows.iter()
        .filter(|r| r.cwres.is_finite())
        .map(|r| {
            let x = match state.ui.eval_cwres_x_col.as_str() {
                "PRED"    => r.pred,
                "IPRED"   => r.ipred,
                "EBE_OFV" => r.ebe_ofv,
                _          => r.time,  // default TIME
            };
            [x, r.cwres]
        })
        .filter(|p| p[0].is_finite())
        .collect();

    let cwres_abs = data.rows.iter()
        .filter_map(|r| r.cwres.is_finite().then_some(r.cwres.abs()))
        .fold(0.0f64, f64::max);
    let cw_pad = (cwres_abs * 1.15).max(3.0);

    let x_lo_cw = cwres_x.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let x_hi_cw = cwres_x.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);

    // Row 1.
    ui.horizontal(|ui| {
        scatter_with_loess(ui, "gof_dv_pred",  "DV vs PRED",
            state.ui.eval_cwres_x_col.as_str(), "DV",
            half_w, half_h, &pts_dv_pred,
            pt_col, ref_col, loess_col, log,
            PlotKind::Identity { lo: ax_lo, hi: ax_hi });
        ui.add_space(4.0);
        scatter_with_loess(ui, "gof_dv_ipred", "DV vs IPRED",
            state.ui.eval_cwres_x_col.as_str(), "DV",
            half_w, half_h, &pts_dv_ipred,
            pt_col, ref_col, loess_col, log,
            PlotKind::Identity { lo: ax_lo, hi: ax_hi });
    });
    ui.add_space(4.0);
    // Row 2 — CWRES (both axes independently configured).
    let col1 = state.ui.eval_cwres_x_col.clone();
    let col2 = state.ui.eval_cwres_x_col_2.clone();

    let pick_x = |r: &crate::domain::PredRow, col: &str| match col {
        "PRED"  => r.pred,
        "IPRED" => r.ipred,
        _       => r.time,
    };

    let cwres_2: Vec<[f64;2]> = data.rows.iter()
        .map(|r| [pick_x(r, &col2), r.cwres])
        .filter(|p| p[0].is_finite() && p[1].is_finite())
        .collect();
    let x_lo_cw2 = cwres_2.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let x_hi_cw2 = cwres_2.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);

    ui.horizontal(|ui| {
        scatter_with_loess(ui, "gof_cwres_x1",
            &format!("CWRES vs {col1}"), &col1, "CWRES",
            half_w, half_h, &cwres_x,
            pt_col, ref_col, loess_col, false,
            PlotKind::Residual { x_lo: x_lo_cw, x_hi: x_hi_cw, y_pad: cw_pad });
        ui.add_space(4.0);
        scatter_with_loess(ui, "gof_cwres_x2",
            &format!("CWRES vs {col2}"), &col2, "CWRES",
            half_w, half_h, &cwres_2,
            pt_col, ref_col, loess_col, false,
            PlotKind::Residual { x_lo: x_lo_cw2, x_hi: x_hi_cw2, y_pad: cw_pad });
    });
}

enum PlotKind {
    Identity { lo: f64, hi: f64 },
    #[allow(dead_code)]
    Residual { x_lo: f64, x_hi: f64, y_pad: f64 },
}

fn scatter_with_loess(
    ui:        &mut egui::Ui,
    id:        &str,
    title:     &str,
    _x_label:  &str,
    _y_label:  &str,
    w: f32, h: f32,
    points:    &[[f64; 2]],
    pt_color:  egui::Color32,
    ref_color: egui::Color32,
    loess_col: egui::Color32,
    _log:      bool,
    kind:      PlotKind,
) {
    let dark = ui.visuals().dark_mode;
    let title_col = theme::fg2(dark);

    // Compute LOESS before entering the Plot closure (avoids borrow issues).
    let loess_pts = loess(points, 0.35);

    ui.vertical(|ui| {
        ui.label(egui::RichText::new(title).size(11.0).color(title_col).strong());
        Plot::new(id)
            .width(w).height(h - 18.0)
            .data_aspect(match &kind { PlotKind::Identity { .. } => 1.0, _ => 0.0 })
            .show_grid(true)
            .label_formatter(|_, v| format!("x={:.3}  y={:.3}", v.x, v.y))
            .show(ui, |p| {
                if !points.is_empty() {
                    p.points(Points::new(PlotPoints::new(points.to_vec()))
                        .radius(2.2).color(pt_color));
                }
                // LOESS trendline.
                if loess_pts.len() > 1 {
                    p.line(Line::new(PlotPoints::new(loess_pts))
                        .color(loess_col).width(2.0).name("LOESS"));
                }
                // Reference lines.
                match kind {
                    PlotKind::Identity { lo, hi } => {
                        p.line(Line::new(PlotPoints::new(vec![[lo,lo],[hi,hi]]))
                            .color(ref_color).width(1.2));
                    }
                    PlotKind::Residual { x_lo: _, x_hi: _, y_pad: _ } => {
                        p.hline(HLine::new(0.0).color(ref_color).width(1.2));
                        for &lvl in &[2.0_f64, -2.0] {
                            p.hline(HLine::new(lvl)
                                .color(egui::Color32::from_rgba_unmultiplied(232,149,64,160))
                                .width(1.0));
                        }
                    }
                }
            });
    });
}

// ── Individual Fits ───────────────────────────────────────────────────────────

fn show_individual_fits(ui: &mut egui::Ui, state: &mut AppState, _idx: usize, dark: bool) {
    let data = match &state.ui.eval_data {
        Some(d) if !d.rows.is_empty() => d,
        _ => { no_predictions(ui, dark); return; }
    };

    let n_subj = data.subject_ids.len();
    if n_subj == 0 { no_predictions(ui, dark); return; }

    let spp   = state.ui.eval_subjects_per_page.max(1).min(6);
    let pages = (n_subj + spp - 1) / spp;
    let page  = (state.ui.eval_subject_idx / spp).min(pages.saturating_sub(1));
    let start = page * spp;
    let end   = (start + spp).min(n_subj);

    // Navigation bar.
    ui.horizontal(|ui| {
        if ui.add_enabled(page > 0,
            egui::Button::new("◀").min_size(egui::vec2(28.0, 24.0))).clicked() {
            state.ui.eval_subject_idx = start.saturating_sub(spp);
        }
        let dim = theme::fg2(dark);
        ui.label(egui::RichText::new(
            format!("Subjects {}-{} of {}  (page {} / {})",
                start+1, end, n_subj, page+1, pages))
            .size(12.0).color(dim));
        if ui.add_enabled(end < n_subj,
            egui::Button::new("▶").min_size(egui::vec2(28.0, 24.0))).clicked() {
            state.ui.eval_subject_idx = end;
        }
        // Subject dropdown for quick-jump.
        egui::ComboBox::from_id_salt("indfit_subj")
            .selected_text(data.subject_ids.get(start).map(|s| s.as_str()).unwrap_or("-"))
            .width(80.0)
            .show_ui(ui, |ui| {
                for (i, id) in data.subject_ids.iter().enumerate() {
                    if ui.selectable_label(i == start, id).clicked() {
                        // align to page boundary
                        state.ui.eval_subject_idx = (i / spp) * spp;
                    }
                }
            });
    });
    ui.add_space(4.0);

    let avail = ui.available_size();
    let cols  = if spp <= 1 { 1usize } else { 2 };
    let rows  = (spp + cols - 1) / cols;
    let pw    = avail.x / cols as f32 - 6.0;
    let ph    = (avail.y / rows as f32 - 28.0).max(80.0);

    let pt_col   = if dark { egui::Color32::from_rgb(221,224,238) } else { egui::Color32::from_gray(30) };
    let ipred_col = egui::Color32::from_rgb(76, 138, 255);
    let pred_col  = egui::Color32::from_rgba_unmultiplied(62,201,122,160);

    // Re-borrow data cleanly.
    let subject_ids = match &state.ui.eval_data {
        Some(d) => d.subject_ids.clone(),
        None    => return,
    };

    let mut row_idx = 0;
    for chunk_start in (start..end).step_by(cols) {
        ui.horizontal(|ui| {
            for si in chunk_start..(chunk_start + cols).min(end) {
                let subj_id = &subject_ids[si];
                let rows_for = match &state.ui.eval_data {
                    Some(d) => d.rows.iter().filter(|r| &r.id == subj_id).cloned().collect::<Vec<_>>(),
                    None    => vec![],
                };

                let obs_pts: Vec<[f64;2]> = rows_for.iter()
                    .filter(|r| r.dv.is_finite()).map(|r| [r.time, r.dv]).collect();
                let mut ipred_s = rows_for.iter()
                    .filter(|r| r.ipred.is_finite() && r.time.is_finite())
                    .map(|r| [r.time, r.ipred]).collect::<Vec<_>>();
                let mut pred_s  = rows_for.iter()
                    .filter(|r| r.pred.is_finite() && r.time.is_finite())
                    .map(|r| [r.time, r.pred]).collect::<Vec<_>>();
                ipred_s.sort_by(|a,b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
                pred_s.sort_by( |a,b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));

                let dim = theme::fg2(dark);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(format!("Subject {subj_id}"))
                        .size(11.0).color(dim));
                    Plot::new(format!("indfit_{si}"))
                        .width(pw).height(ph)
                        .x_axis_label("TIME").y_axis_label("DV")
                        .show_grid(true)
                        .legend(egui_plot::Legend::default())
                        .show(ui, |p| {
                            if !obs_pts.is_empty() {
                                p.points(Points::new(PlotPoints::new(obs_pts))
                                    .radius(4.0).color(pt_col).name("DV (obs)"));
                            }
                            if !ipred_s.is_empty() {
                                p.line(Line::new(PlotPoints::new(ipred_s))
                                    .color(ipred_col).width(2.0).name("IPRED"));
                            }
                            if !pred_s.is_empty() {
                                p.line(Line::new(PlotPoints::new(pred_s))
                                    .color(pred_col).width(1.5).name("PRED"));
                            }
                        });
                });
                if si + 1 < (chunk_start + cols).min(end) {
                    ui.add_space(4.0);
                }
            }
        });
        row_idx += 1;
        if row_idx < rows { ui.add_space(4.0); }
    }
}

// ── iOFV Waterfall ────────────────────────────────────────────────────────────

fn show_iofv_waterfall(ui: &mut egui::Ui, state: &AppState, dark: bool) {
    let ebes = match &state.ui.eval_ebes {
        Some(e) if !e.rows.is_empty() => e,
        _ => {
            let dim = theme::fg3(dark);
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("No per-subject iOFV data").size(15.0).strong());
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(
                        "ebes.csv not found in this bundle.\n\
                         Try re-running the model.")
                        .color(dim).size(12.0));
                });
            });
            return;
        }
    };

    let sorted = ebes.sorted_by_iofv();
    let n = sorted.len();
    let mean_iofv = ebes.total_ofv / n as f64;

    // Title + stats.
    let dim = theme::fg2(dark);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("Individual OFV contributions  ({n} subjects)"))
            .size(13.0).strong());
        ui.add_space(12.0);
        ui.label(egui::RichText::new(
            format!("Total: {:.2}   Mean per subject: {:.2}", ebes.total_ofv, mean_iofv))
            .color(dim).size(11.0));
    });
    ui.label(egui::RichText::new(
        "Bars sorted descending — subjects at left contribute most to OFV (worst fit).")
        .color(theme::fg3(dark)).size(10.0));
    ui.add_space(4.0);

    let ids: Vec<String> = sorted.iter().map(|r| r.id.clone()).collect();

    let bars: Vec<Bar> = sorted.iter().enumerate().map(|(i, r)| {
        let above_mean = r.ofv_contribution > mean_iofv;
        let color = if above_mean { theme::ORANGE } else { theme::GREEN };
        Bar::new(i as f64, r.ofv_contribution)
            .fill(color)
            .stroke(egui::Stroke::new(0.5, egui::Color32::from_rgba_unmultiplied(0,0,0,40)))
            .name(&r.id)
    }).collect();

    Plot::new("iofv_waterfall")
        .width(ui.available_width())
        .height(ui.available_height() - 40.0)
        .y_axis_label("iOFV contribution")
        .show_grid(true)
        .x_axis_formatter(move |mark, _| {
            let i = mark.value.round() as usize;
            ids.get(i).cloned().unwrap_or_default()
        })
        .label_formatter(|name, v| {
            if name.is_empty() { format!("iOFV = {:.2}", v.y) }
            else { format!("Subject {}\niOFV = {:.2}", name, v.y) }
        })
        .show(ui, |p| {
            p.bar_chart(BarChart::new(bars));
            // Mean iOFV reference line.
            p.hline(HLine::new(mean_iofv)
                .color(egui::Color32::from_rgba_unmultiplied(200,200,200,180))
                .width(1.2)
                .name(format!("Mean ({mean_iofv:.2})")));
        });

    // Legend note.
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        legend_dot(ui, theme::ORANGE, "Above mean (poor fit)");
        legend_dot(ui, theme::GREEN,  "Below mean");
        legend_dot(ui, egui::Color32::from_gray(160), "Mean iOFV");
    });
}

// ── Export dialog ─────────────────────────────────────────────────────────────

fn show_export_dialog(ctx: &egui::Context, state: &mut AppState, model_idx: usize) {
    if !state.ui.eval_export_dialog { return; }

    let mut close   = false;
    let mut do_export = false;

    egui::Window::new("Export GOF figure")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            let dark = ui.visuals().dark_mode;
            let dim  = theme::fg2(dark);
            ui.set_min_width(360.0);

            egui::Grid::new("export_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    // Format
                    ui.label(egui::RichText::new("Format:").color(dim).size(12.0));
                    ui.horizontal(|ui| {
                        for (label, val) in [
                            ("PDF",      "pdf"),
                            ("PNG 300",  "png300"),
                            ("PNG 600",  "png600"),
                            ("SVG",      "svg"),
                        ] {
                            let sel = &state.ui.eval_export_format == val;
                            if ui.add(
                                egui::Button::new(egui::RichText::new(label).size(11.0)
                                    .color(if sel { egui::Color32::WHITE } else { dim }))
                                .fill(if sel { theme::ACCENT } else { theme::card_fill(dark) })
                                .min_size(egui::vec2(52.0, 22.0)),
                            ).clicked() {
                                state.ui.eval_export_format = val.to_string();
                            }
                        }
                    });
                    ui.end_row();

                    // Width
                    ui.label(egui::RichText::new("Width:").color(dim).size(12.0));
                    ui.horizontal(|ui| {
                        for (label, mm) in [("84 mm (1-col)", 84u32), ("174 mm (2-col)", 174)] {
                            let sel = state.ui.eval_export_width_mm == mm;
                            if ui.add(
                                egui::Button::new(egui::RichText::new(label).size(11.0)
                                    .color(if sel { egui::Color32::WHITE } else { dim }))
                                .fill(if sel { theme::ACCENT } else { theme::card_fill(dark) })
                                .min_size(egui::vec2(0.0, 22.0)),
                            ).clicked() {
                                state.ui.eval_export_width_mm = mm;
                            }
                        }
                    });
                    ui.end_row();

                    // Options
                    ui.label(egui::RichText::new("Include:").color(dim).size(12.0));
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut state.ui.eval_export_loess,    "LOESS trend");
                        ui.add_space(8.0);
                        ui.checkbox(&mut state.ui.eval_export_ci_lines, "±2 lines");
                    });
                    ui.end_row();
                });

            ui.add_space(4.0);
            ui.label(egui::RichText::new(
                "Uses ggplot2 + patchwork if available; falls back to base R graphics.")
                .color(theme::fg3(dark)).size(10.0));
            ui.add_space(14.0);

            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() { close = true; }
                ui.add_space(8.0);
                if ui.add(
                    egui::Button::new(egui::RichText::new("Export…").color(egui::Color32::WHITE))
                        .fill(theme::ACCENT),
                ).clicked() {
                    do_export = true;
                }
            });

            if ui.input(|i| i.key_pressed(egui::Key::Escape)) { close = true; }
        });

    if close { state.ui.eval_export_dialog = false; }

    if do_export {
        state.ui.eval_export_dialog = false;

        // Ask user to pick the output path.
        let ext = match state.ui.eval_export_format.as_str() {
            "pdf"  => "pdf",
            "svg"  => "svg",
            _      => "png",
        };
        let stem = state.workspace.models[model_idx].model.stem.clone();
        let default_name = format!("{stem}_gof.{ext}");

        let save_path = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("Figure", &[ext])
            .save_file();

        if let Some(out_path) = save_path {
            // Write predictions CSV to a temp file.
            let pred_rows = match &state.ui.eval_data {
                Some(d) => d.rows.clone(),
                None    => return,
            };
            let tmp_csv = std::env::temp_dir().join("ferxgui_gof_export.csv");
            if let Ok(mut wtr) = csv::Writer::from_path(&tmp_csv) {
                let _ = wtr.write_record(["ID","TIME","DV","PRED","IPRED","CWRES","IWRES"]);
                for r in &pred_rows {
                    let _ = wtr.write_record(&[
                        &r.id,
                        &r.time.to_string(),  &r.dv.to_string(),
                        &r.pred.to_string(),  &r.ipred.to_string(),
                        &r.cwres.to_string(), &r.iwres.to_string(),
                    ]);
                }
                let _ = wtr.flush();
            }

            let tx          = state.worker_tx.clone();
            let ctx_cl      = ctx.clone();
            let format      = state.ui.eval_export_format.clone();
            let width       = state.ui.eval_export_width_mm;
            let col1        = state.ui.eval_cwres_x_col.clone();
            let col2        = state.ui.eval_cwres_x_col_2.clone();
            let loess       = state.ui.eval_export_loess;
            let ci          = state.ui.eval_export_ci_lines;

            state.ui.status_message = "Exporting figure…".to_string();

            std::thread::spawn(move || {
                match crate::io::r_extract::export_gof(
                    &tmp_csv, &out_path, &format, width,
                    &col1, &col2, loess, ci)
                {
                    Ok(path) => {
                        let _ = tx.send(crate::workers::messages::WorkerMsg::GofExportComplete { path });
                    }
                    Err(e) => {
                        let _ = tx.send(crate::workers::messages::WorkerMsg::GofExportError { message: e });
                    }
                }
                ctx_cl.request_repaint();
            });
        }
    }
}

fn legend_dot(ui: &mut egui::Ui, color: egui::Color32, label: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
    ui.add_space(3.0);
    ui.label(egui::RichText::new(label).size(11.0));
    ui.add_space(12.0);
}

// ── Convergence ───────────────────────────────────────────────────────────────

fn show_convergence(ui: &mut egui::Ui, state: &AppState, idx: usize, dark: bool) {
    let fitrx_path = match state.workspace.models[idx].fitrx_path.as_ref() {
        Some(p) => p.clone(),
        None => { no_trace(ui, dark, "No fit data for this model."); return; }
    };
    let fit = match state.workspace.models[idx].fit.as_ref() {
        Some(f) => f,
        None => { no_trace(ui, dark, "No fit data for this model."); return; }
    };
    let trace_path = match crate::io::fitrx::resolve_trace_path(&fitrx_path, fit) {
        Some(p) => p,
        None => {
            no_trace(ui, dark,
                "No convergence trace in this bundle.\n\n\
                 To generate it: go to the Run tab for this model, enable\n\
                 the 'Optimizer trace' checkbox, then re-run the model.");
            return;
        }
    };
    let rows: Vec<TraceRow> = match crate::io::fitrx::read_trace_csv(&trace_path) {
        Ok(r) if !r.is_empty() => r,
        _ => {
            no_trace(ui, dark, &format!("Trace file not found or empty:\n{}", trace_path.display()));
            return;
        }
    };

    // Determine which method-specific metric to show in the bottom panel.
    let has_mh  = rows.iter().any(|r| r.mh_accept_rate.is_finite());
    let has_lm  = rows.iter().any(|r| r.lm_lambda.is_finite());
    let has_grd = rows.iter().any(|r| r.grad_norm.is_finite());
    let (metric_pts, metric_label, metric_color): (Vec<[f64;2]>, &str, egui::Color32) =
        if has_mh {
            (rows.iter().filter(|r| r.iteration.is_finite() && r.mh_accept_rate.is_finite())
                .map(|r| [r.iteration, r.mh_accept_rate]).collect(),
             "MH Accept Rate",
             egui::Color32::from_rgb(245, 166, 35))
        } else if has_lm {
            (rows.iter().filter(|r| r.iteration.is_finite() && r.lm_lambda.is_finite())
                .map(|r| [r.iteration, r.lm_lambda]).collect(),
             "LM Lambda",
             egui::Color32::from_rgb(43, 173, 110))
        } else if has_grd {
            (rows.iter().filter(|r| r.iteration.is_finite() && r.grad_norm.is_finite())
                .map(|r| [r.iteration, r.grad_norm]).collect(),
             "Gradient Norm",
             egui::Color32::from_rgb(224, 90, 82))
        } else {
            (vec![], "", egui::Color32::TRANSPARENT)
        };

    // Phase boundary x-positions (iteration where phase label changes).
    let phase_lines: Vec<f64> = rows.windows(2)
        .filter(|w| !w[0].phase.is_empty() && w[0].phase != w[1].phase)
        .map(|w| w[1].iteration)
        .collect();

    let ofv_pts: Vec<[f64;2]> = rows.iter()
        .filter(|r| r.iteration.is_finite() && r.ofv.is_finite())
        .map(|r| [r.iteration, r.ofv]).collect();
    let final_ofv = rows.last().map(|r| r.ofv).unwrap_or(f64::NAN);
    let n_iters   = rows.len();

    // Summary line.
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("{n_iters} iterations")).size(12.0));
        ui.add_space(8.0);
        ui.label(egui::RichText::new(format!("Final OFV: {final_ofv:.4}"))
            .size(12.0).color(theme::fg2(dark)));
        let methods: Vec<String> = {
            let mut seen = std::collections::HashSet::new();
            rows.iter().filter(|r| !r.method.is_empty())
                .filter_map(|r| if seen.insert(r.method.clone()) { Some(r.method.clone()) } else { None })
                .collect()
        };
        if !methods.is_empty() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(methods.join(" → "))
                .size(11.0).color(theme::fg3(dark)));
        }
    });
    ui.add_space(4.0);

    let avail      = ui.available_height() - 8.0;
    let has_metric = !metric_pts.is_empty();
    let ofv_h      = if has_metric { avail * 0.60 } else { avail };
    let metric_h   = avail - ofv_h;
    let blue       = egui::Color32::from_rgb(76, 138, 255);
    let phase_col  = if dark { egui::Color32::from_gray(90) } else { egui::Color32::from_gray(180) };

    // ── Top panel: OFV ───────────────────────────────────────────────────────
    Plot::new("conv_ofv")
        .width(ui.available_width())
        .height(ofv_h)
        .y_axis_label("OFV")
        .show_x(has_metric)  // hide x-axis labels when metric panel follows
        .show_grid(true)
        .label_formatter(|_, v| format!("iter={:.0}  OFV={:.4}", v.x, v.y))
        .show(ui, |p| {
            p.line(Line::new(PlotPoints::new(ofv_pts)).color(blue).width(2.0).name("OFV"));
            for &x in &phase_lines {
                p.vline(egui_plot::VLine::new(x).color(phase_col).width(1.0).style(egui_plot::LineStyle::Dashed { length: 8.0 }));
            }
        });

    // ── Bottom panel: method-specific metric ─────────────────────────────────
    if has_metric {
        Plot::new("conv_metric")
            .width(ui.available_width())
            .height(metric_h)
            .x_axis_label("Iteration")
            .y_axis_label(metric_label)
            .show_grid(true)
            .label_formatter(move |_, v| format!("iter={:.0}  {metric_label}={:.4}", v.x, v.y))
            .show(ui, |p| {
                p.line(Line::new(PlotPoints::new(metric_pts.clone())).color(metric_color).width(2.0).name(metric_label));
                for &x in &phase_lines {
                    p.vline(egui_plot::VLine::new(x).color(phase_col).width(1.0).style(egui_plot::LineStyle::Dashed { length: 8.0 }));
                }
            });
    }
}

fn no_trace(ui: &mut egui::Ui, dark: bool, msg: &str) {
    let dim = theme::fg3(dark);
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new(msg).color(dim).size(12.0));
        });
    });
}

// ── Empty / error states ──────────────────────────────────────────────────────

// ── ETA-Covariate Correlation ─────────────────────────────────────────────────

fn show_eta_cov(ui: &mut egui::Ui, state: &mut AppState, idx: usize, dark: bool) {
    let entry = &state.workspace.models[idx];
    let stem  = entry.model.stem.clone();

    let Some(fitrx_path) = entry.fitrx_path.clone() else {
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new("Run the model first — ETA-cov requires a completed fit.")
                    .color(theme::fg3(dark)).size(13.0),
            );
        });
        return;
    };

    // Auto-populate data path from run history (same as Run pill).
    let needs_path = state.ui.eval_eta_cov_data_path.as_deref()
        .map_or(true, |p| !p.exists());
    if needs_path {
        state.ui.eval_eta_cov_data_path = state.run.run_history
            .iter().rev()
            .find(|r| r.model_stem == stem && r.data_path.as_ref().map_or(false, |p| p.exists()))
            .and_then(|r| r.data_path.clone());
    }

    // Auto-trigger: fire immediately when the section is entered if we have a
    // data path, no cached result, and the computation isn't already running.
    if !state.workspace.eta_cov_running.contains(&stem)
        && !state.workspace.eta_cov_results.contains_key(&stem)
    {
        if let Some(data_path) = state.ui.eval_eta_cov_data_path.clone() {
            let fitrx_c = fitrx_path.clone();
            let stem_c  = stem.clone();
            let tx      = state.worker_tx.clone();
            state.workspace.eta_cov_running.insert(stem_c.clone());
            std::thread::spawn(move || {
                match crate::io::r_extract::compute_eta_cov(&fitrx_c, &data_path) {
                    Ok(result) => {
                        let _ = tx.send(crate::workers::messages::WorkerMsg::EtaCovComplete {
                            stem: stem_c, result: Box::new(result),
                        });
                    }
                    Err(msg) => {
                        let _ = tx.send(crate::workers::messages::WorkerMsg::RTaskError {
                            context: format!("eta_cov {stem_c}"),
                            message: msg,
                        });
                    }
                }
            });
        }
    }

    // ── Computing spinner ──────────────────────────────────────────────────
    if state.workspace.eta_cov_running.contains(&stem) {
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                ui.add(egui::Spinner::new().size(32.0));
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new("Computing ETA-covariate correlations via R…")
                        .color(theme::fg2(dark)).size(13.0),
                );
            });
        });
        ui.ctx().request_repaint();
        return;
    }

    // ── Results ────────────────────────────────────────────────────────────
    if let Some(result) = state.workspace.eta_cov_results.get(&stem).cloned() {
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                if result.rows.is_empty() {
                    ui.add_space(20.0);
                    ui.label(
                        egui::RichText::new("No ETA-covariate pairs found (need ≥3 subjects with finite values).")
                            .color(theme::fg2(dark)).size(12.0),
                    );
                } else {
                    let flagged = result.rows.iter().filter(|r| r.flag).count();
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "{} pairs screened  ·  {} candidate(s) flagged (|r| ≥ 0.3)",
                            result.rows.len(), flagged
                        ))
                        .color(theme::fg2(dark)).size(11.0),
                    );
                    ui.add_space(6.0);

                    egui::Grid::new("eta_cov_table")
                        .num_columns(4)
                        .striped(true)
                        .spacing([16.0, 3.0])
                        .min_col_width(60.0)
                        .show(ui, |ui| {
                            for h in ["ETA", "COVARIATE", "r", "p-value"] {
                                ui.label(
                                    egui::RichText::new(h).strong()
                                        .color(theme::fg2(dark)).size(11.0),
                                );
                            }
                            ui.end_row();

                            for row in &result.rows {
                                let name_col = if row.flag { theme::ORANGE } else { theme::fg(dark) };
                                ui.label(
                                    egui::RichText::new(&row.eta)
                                        .color(name_col).size(12.0).monospace(),
                                );
                                ui.label(
                                    egui::RichText::new(&row.covariate)
                                        .color(name_col).size(12.0).monospace(),
                                );
                                let r_str = if row.r.is_finite() {
                                    format!("{:+.3}", row.r)
                                } else {
                                    "—".to_string()
                                };
                                let r_col = if row.flag { theme::ORANGE } else { theme::fg(dark) };
                                ui.label(
                                    egui::RichText::new(r_str).color(r_col).size(12.0),
                                );
                                let p_str = if row.p_val.is_finite() {
                                    if row.p_val < 0.001 {
                                        "<0.001".to_string()
                                    } else {
                                        format!("{:.3}", row.p_val)
                                    }
                                } else {
                                    "—".to_string()
                                };
                                let p_col = if row.p_val.is_finite() && row.p_val < 0.05 {
                                    theme::ORANGE
                                } else {
                                    theme::fg2(dark)
                                };
                                ui.label(egui::RichText::new(p_str).color(p_col).size(12.0));
                                ui.end_row();
                            }
                        });
                }

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);
                show_eta_cov_launcher(ui, state, &stem, &fitrx_path, true, dark);
            });
        return;
    }

    // ── Setup panel ────────────────────────────────────────────────────────
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.set_max_width(460.0);
                ui.label(
                    egui::RichText::new("ETA-Covariate Correlation Screen")
                        .size(16.0).strong().color(theme::fg(dark)),
                );
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(
                        "Computes Pearson r between each ETA (EBE) and numeric dataset \
                         covariates. Pairs with |r| ≥ 0.3 are flagged as covariate \
                         model building candidates.",
                    )
                    .color(theme::fg2(dark))
                    .size(12.0),
                );
                ui.add_space(24.0);
                show_eta_cov_launcher(ui, state, &stem, &fitrx_path, false, dark);
            });
        });
}

fn show_eta_cov_launcher(
    ui:         &mut egui::Ui,
    state:      &mut AppState,
    stem:       &str,
    fitrx_path: &std::path::Path,
    compact:    bool,
    dark:       bool,
) {
    // Auto-populate from the most recent run for this model — same pattern
    // as the Run pill.  Only triggers when the path is not already set or
    // the currently stored path no longer exists on disk.
    let needs_path = state.ui.eval_eta_cov_data_path.as_deref()
        .map_or(true, |p| !p.exists());
    if needs_path {
        state.ui.eval_eta_cov_data_path = state.run.run_history
            .iter()
            .rev()
            .find(|r| r.model_stem == stem && r.data_path.as_ref().map_or(false, |p| p.exists()))
            .and_then(|r| r.data_path.clone());
    }

    if !compact {
        ui.label(
            egui::RichText::new("Dataset CSV (the same file used for fitting):")
                .color(theme::fg2(dark)).size(12.0),
        );
        ui.add_space(4.0);
    }

    ui.horizontal(|ui| {
        let path_str = state.ui.eval_eta_cov_data_path.as_deref()
            .and_then(|p| p.to_str())
            .unwrap_or("(no file selected)");
        ui.label(
            egui::RichText::new(path_str)
                .color(theme::fg2(dark)).size(11.0).monospace(),
        );
        if ui.button("Browse…").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("CSV", &["csv"])
                .pick_file()
            {
                state.ui.eval_eta_cov_data_path = Some(path);
            }
        }
    });

    ui.add_space(8.0);
    let can_run = state.ui.eval_eta_cov_data_path.is_some();
    ui.add_enabled_ui(can_run, |ui| {
        if ui.button(if compact { "Re-run" } else { "Run ETA-Cov Screen" }).clicked() {
            if let Some(data_path) = state.ui.eval_eta_cov_data_path.clone() {
                let fitrx   = fitrx_path.to_path_buf();
                let stem_s  = stem.to_string();
                let tx      = state.worker_tx.clone();
                state.workspace.eta_cov_running.insert(stem_s.clone());
                std::thread::spawn(move || {
                    match crate::io::r_extract::compute_eta_cov(&fitrx, &data_path) {
                        Ok(result) => {
                            let _ = tx.send(crate::workers::messages::WorkerMsg::EtaCovComplete {
                                stem: stem_s,
                                result: Box::new(result),
                            });
                        }
                        Err(msg) => {
                            let _ = tx.send(crate::workers::messages::WorkerMsg::RTaskError {
                                context: format!("eta_cov {stem_s}"),
                                message: msg,
                            });
                        }
                    }
                });
            }
        }
    });
    if !can_run {
        ui.label(
            egui::RichText::new("Select a dataset CSV to run.")
                .color(theme::fg3(dark)).size(11.0),
        );
    }
}

fn show_no_model(ui: &mut egui::Ui, state: &mut AppState, dark: bool) {
    let dim  = theme::fg3(dark);
    let dim2 = theme::fg2(dark);

    if state.workspace.models.iter().any(|m| m.fit.is_some()) {
        ui.horizontal(|ui| {
            let inactive_fill = if dark { theme::BG3 } else { egui::Color32::TRANSPARENT };
            let inactive_fg   = theme::fg2(dark);
            for (label, section) in [
                ("GOF",            EvalSection::Gof),
                ("Individual Fits",EvalSection::IndividualFits),
                ("iOFV Waterfall", EvalSection::OfvWaterfall),
                ("Convergence",    EvalSection::Convergence),
                ("ETA-Cov",        EvalSection::EtaCov),
                ("Param Corr",     EvalSection::ParamCorr),
            ] {
                let active = state.ui.active_eval_section == section;
                if ui.add(
                    egui::Button::new(egui::RichText::new(label).size(11.0)
                        .color(if active { egui::Color32::WHITE } else { inactive_fg }))
                    .fill(if active { theme::ACCENT } else { inactive_fill })
                    .min_size(egui::vec2(0.0, 22.0)),
                ).clicked() {
                    state.ui.active_eval_section = section;
                }
            }
        });
        ui.separator();
        if state.ui.active_eval_section == EvalSection::OfvWaterfall {
            show_iofv_waterfall(ui, state, dark);
            return;
        }
    }

    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("No model selected").size(16.0).strong());
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Select a model in the Models tab to view diagnostics")
                .color(dim2).size(13.0));
            if state.workspace.models.iter().any(|m| m.fit.is_some()) {
                ui.add_space(16.0);
                ui.label(egui::RichText::new("← iOFV Waterfall is available without a selection")
                    .color(dim).size(11.0));
            }
        });
    });
}

// ── Parameter Correlation section ────────────────────────────────────────────

fn show_param_corr(ui: &mut egui::Ui, state: &AppState, idx: usize, dark: bool) {
    let fit = match state.workspace.models[idx].fit.as_ref() {
        Some(f) => f,
        None => {
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new("No run output for this model.")
                    .color(theme::fg3(dark)).size(13.0));
            });
            return;
        }
    };

    if fit.cov_corr_n == 0 {
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("Parameter correlation matrix not available.")
                    .size(14.0).color(theme::fg2(dark)));
                ui.add_space(6.0);
                ui.label(egui::RichText::new(
                    "The covariance step did not run or did not converge.\n\
                     Re-run the model with 'Covariance step' enabled.")
                    .color(theme::fg3(dark)).size(12.0));
            });
        });
        return;
    }

    ui.label(egui::RichText::new(
        "Correlation matrix of estimated parameters derived from the covariance matrix. \
         Correlations near ±1 indicate structural identifiability problems — \
         consider reparameterisation or removing a parameter.")
        .color(theme::fg2(dark)).size(11.0).italics());
    ui.add_space(8.0);

    crate::ui::sir_tab::correlation_heatmap(
        ui, &fit.cov_corr_names, &fit.cov_corr_flat, fit.cov_corr_n, dark);
}

fn no_predictions(ui: &mut egui::Ui, dark: bool) {
    let dim = theme::fg3(dark);
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("No predictions data").size(15.0).strong());
            ui.add_space(6.0);
            ui.label(egui::RichText::new(
                "predictions.csv was not found in the .fitrx bundle.\n\
                 This may be an older FerX version. Re-run the model to generate it.")
                .color(dim).size(12.0));
        });
    });
}
