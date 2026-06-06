/// VPC tab — Visual Predictive Check.
///
/// All statistics are computed by the `vpc` R package (`vpcdb = TRUE`);
/// this tab collects options, drives the bridge, and renders natively.
///
/// # Rendering note
/// egui_plot's `Polygon` uses `Shape::convex_polygon` internally, which only
/// handles convex shapes correctly. A VPC ribbon (forward along the low
/// percentile, backward along the high percentile) is non-convex for any curved
/// PK profile. We therefore render smooth ribbons as N-1 convex quadrilateral
/// strips (one per adjacent bin pair) instead of one large polygon. Each strip
/// IS always convex as long as time increases and lo < hi at every bin.
use eframe::egui;
use egui_plot::{HLine, Legend, Line, Plot, PlotPoints, Points, Polygon, VLine};

use crate::app::theme;
use crate::domain::{VpcConfig, VpcResult};
use crate::io::r_extract;
use crate::state::{AppState, VpcOpts};
use crate::workers::messages::WorkerMsg;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;

    // Lazy-initialise the editable R script from the embedded default.
    if state.ui.vpc_script.is_empty() {
        state.ui.vpc_script = r_extract::VPC_PLOT_R_DEFAULT.to_string();
    }

    ensure_pkg_check(ui, state);

    let Some(idx) = state.ui.selected_model else {
        return hint(ui, "Select a model in the Models tab to compute a VPC.");
    };

    let stem = state.workspace.models[idx].model.stem.clone();

    if state.workspace.models[idx].fit.is_none() {
        return hint(ui, "Run the model first — VPC requires a completed fit.");
    }

    // Auto-populate the data path from the most recent run record for this model.
    if state.ui.vpc_data_path.is_none() {
        state.ui.vpc_data_path = state
            .run
            .run_history
            .iter()
            .rev()
            .find(|r| r.model_stem == stem && r.data_path.is_some())
            .and_then(|r| r.data_path.clone());
    }

    // R-script editor OS viewport (independent of left/right layout).
    show_script_popup(ui, state, idx, &stem);

    let left_w = 300.0_f32;
    ui.horizontal_top(|ui| {
        // ── Left panel ────────────────────────────────────────────────────────
        ui.vertical(|ui| {
            ui.set_width(left_w);

            let avail_h  = ui.available_height();
            let btns_h   = 70.0;   // Compute + Script buttons
            let scroll_h = (avail_h - btns_h).max(100.0);

            egui::ScrollArea::vertical()
                .id_salt("vpc_left_scroll")
                .max_height(scroll_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(left_w - 6.0);
                    show_options(ui, state, dark);
                });

            let computing  = state.workspace.vpc_computing.contains(&stem);
            let pkg_ok     = matches!(state.ui.vpc_pkg_status, Some(Ok(_)));
            let has_data   = state.ui.vpc_data_path.is_some();
            let valid_bins = manual_bins_valid(&state.ui.vpc_opts);
            let valid_lloq = lloq_valid(&state.ui.vpc_opts);

            ui.add_space(2.0);

            // Primary: Compute VPC
            let lbl = if computing { "Computing…" } else { "Compute VPC" };
            if ui.add_enabled(
                has_data && pkg_ok && !computing && valid_bins && valid_lloq,
                egui::Button::new(egui::RichText::new(lbl).size(14.0).strong())
                    .fill(theme::ACCENT)
                    .min_size(egui::vec2(ui.available_width(), 34.0)),
            ).clicked() {
                start_compute(ui, state, idx, &stem);
            }

            ui.add_space(3.0);

            // Secondary: Open the script editor / runner (OS-native window).
            let script_fill = if dark {
                egui::Color32::from_rgb(55, 55, 80)
            } else {
                egui::Color32::from_rgb(220, 220, 240)
            };
            if ui.add(
                egui::Button::new(egui::RichText::new("Edit and execute R script").size(11.5))
                    .fill(script_fill)
                    .min_size(egui::vec2(ui.available_width(), 28.0)),
            ).on_hover_text(
                "Open the R script in an editor window. Customise and run it to \
                 generate a vpc ggplot PNG."
            ).clicked() {
                state.ui.vpc_script_open = true;
            }
        });

        ui.separator();

        // ── Right: plot / spinner / hint ─────────────────────────────────────
        ui.vertical(|ui| {
            if state.workspace.vpc_computing.contains(&stem) {
                computing_spinner(ui, &state.ui.vpc_opts);
                ui.ctx().request_repaint();
            } else if let Some(vpc) = state.workspace.vpc_data.get(&stem).cloned() {
                let opts = state.ui.vpc_opts.clone();
                show_vpc_plot(ui, &vpc, &opts, dark);
            } else {
                hint(ui, "Set options on the left, then click Compute VPC.");
            }
        });
    });
}

// ---------------------------------------------------------------------------
// R-script editor popup
// ---------------------------------------------------------------------------

fn show_script_popup(ui: &egui::Ui, state: &mut AppState, idx: usize, stem: &str) {
    if !state.ui.vpc_script_open { return; }

    let dark       = ui.visuals().dark_mode;
    let exporting  = state.ui.vpc_exporting;
    let has_data   = state.ui.vpc_data_path.is_some();
    let pkg_ok     = matches!(state.ui.vpc_pkg_status, Some(Ok(_)));
    let has_result = state.workspace.vpc_data.contains_key(stem);

    // Derive the PNG output path so the user can see where it will land.
    let png_preview = state.workspace.models.get(idx)
        .map(|m| m.model.path.parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(format!("{stem}_vpc_<timestamp>.png"))
            .to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut do_close = false;

    ui.ctx().show_viewport_immediate(
        egui::ViewportId::from_hash_of("vpc_script_editor"),
        egui::ViewportBuilder::default()
            .with_title("VPC R Script — edit and execute")
            .with_inner_size(egui::vec2(780.0, 620.0))
            .with_min_inner_size(egui::vec2(400.0, 300.0)),
        |ctx, _class| {
            if dark { crate::app::theme::apply_dark(ctx); } else { crate::app::theme::apply_light(ctx); }

            if ctx.input(|i| i.viewport().close_requested()) {
                do_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }

            egui::CentralPanel::default().show(ctx, |ui| {
                // ── Instructions ──────────────────────────────────────────
                ui.label(egui::RichText::new(
                    "The script is called as: Rscript script.R <config.json> <output.png>\n\
                     cfg <- jsonlite::fromJSON(args[1]) holds all VPC settings (pi, ci, binning, \
                     colours, stratification, …). Customise the ggplot appearance freely.",
                ).size(11.0).color(theme::FG2));
                ui.add_space(6.0);

                // ── Script editor ─────────────────────────────────────────
                let avail_h = ui.available_height();
                let footer_h = 72.0;
                egui::ScrollArea::vertical()
                    .id_salt("vpc_script_scroll")
                    .max_height((avail_h - footer_h).max(120.0))
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut state.ui.vpc_script)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY)
                                .desired_rows(24),
                        );
                    });

                ui.separator();

                // ── Output path preview ───────────────────────────────────
                ui.label(egui::RichText::new(format!("Output: {png_preview}"))
                    .size(10.0).color(theme::FG3).monospace());

                ui.add_space(4.0);

                // ── Action row ────────────────────────────────────────────
                ui.horizontal(|ui| {
                    if ui.button("Reset to default").clicked() {
                        state.ui.vpc_script = r_extract::VPC_PLOT_R_DEFAULT.to_string();
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let run_label = if exporting { "Running…" } else { "Run Script" };
                        let can_run = has_data && pkg_ok && has_result && !exporting;
                        if ui.add_enabled(
                            can_run,
                            egui::Button::new(
                                egui::RichText::new(run_label).size(13.0).strong(),
                            )
                            .fill(theme::ACCENT)
                            .min_size(egui::vec2(120.0, 30.0)),
                        ).on_hover_text(
                            "Render the ggplot using the script above, \
                             save the PNG next to the model, and open it."
                        ).clicked() {
                            start_export_from_ctx(ctx, state, idx, stem);
                        }

                        if exporting {
                            ui.add(egui::Spinner::new().size(18.0));
                        }
                    });
                });
            });
        },
    );

    if do_close { state.ui.vpc_script_open = false; }
}

// ---------------------------------------------------------------------------
// Options panel
// ---------------------------------------------------------------------------

fn show_options(ui: &mut egui::Ui, state: &mut AppState, dark: bool) {
    let is_censored = state.ui.vpc_opts.vpc_type == "censored";
    pkg_banner(ui, state, dark);

    section(ui, "Data file", true, dark, |ui| {
        let path_str = state.ui.vpc_data_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "No data file selected".to_string());
        let color = if state.ui.vpc_data_path.is_some() {
            theme::fg2(dark)
        } else {
            theme::ORANGE
        };
        ui.add(egui::Label::new(
            egui::RichText::new(&path_str).monospace().size(10.5).color(color),
        ).truncate());
        ui.add_space(4.0);
        if ui.button("Choose data file…").clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).pick_file() {
                state.ui.vpc_data_cols = read_csv_header(&p);
                state.ui.vpc_data_path = Some(p);
            }
        }
        // Lazy-load column list if not yet populated.
        if state.ui.vpc_data_cols.is_empty() {
            if let Some(p) = &state.ui.vpc_data_path.clone() {
                state.ui.vpc_data_cols = read_csv_header(p);
            }
        }
    });

    section(ui, "Simulation", true, dark, |ui| {
        let o = &mut state.ui.vpc_opts;
        egui::Grid::new("vpc_sim_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label(egui::RichText::new("Replicates").size(11.0));
            ui.add(egui::DragValue::new(&mut o.n_sim).speed(10).range(50..=5000));
            ui.end_row();
            ui.label(egui::RichText::new("Seed").size(11.0));
            ui.add(egui::DragValue::new(&mut o.seed).speed(1));
            ui.end_row();
        });
        ui.label(egui::RichText::new("Changing these re-simulates (slower).")
            .size(9.5).color(theme::fg3(dark)).italics());
    });

    if !is_censored { section(ui, "Prediction interval", true, dark, |ui| {
        let o = &mut state.ui.vpc_opts;
        let presets: [(&str, f64, f64); 3] = [
            ("90%  (5th–95th)",  0.05, 0.95),
            ("80%  (10th–90th)", 0.10, 0.90),
            ("50%  (25th–75th)", 0.25, 0.75),
        ];
        let cur = presets.iter()
            .find(|(_, lo, hi)| (o.pi_lo - lo).abs() < 1e-9 && (o.pi_hi - hi).abs() < 1e-9)
            .map(|(l, _, _)| *l).unwrap_or("Custom");
        egui::ComboBox::from_id_salt("vpc_pi_combo")
            .selected_text(cur)
            .width(ui.available_width() - 8.0)
            .show_ui(ui, |ui| {
                for (label, lo, hi) in presets {
                    if ui.selectable_label(cur == label, label).clicked() {
                        o.pi_lo = lo; o.pi_hi = hi;
                    }
                }
            });
        if cur == "Custom" {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut o.pi_lo).speed(0.005).range(0.0..=0.49).fixed_decimals(3));
                ui.label("–");
                ui.add(egui::DragValue::new(&mut o.pi_hi).speed(0.005).range(0.51..=1.0).fixed_decimals(3));
            });
        }
    }); } // end PI section (continuous only)

    section(ui, "Confidence interval", false, dark, |ui| {
        let o = &mut state.ui.vpc_opts;
        let presets: [(&str, f64, f64); 3] = [
            ("90%  (0.05–0.95)",   0.05,  0.95),
            ("95%  (0.025–0.975)", 0.025, 0.975),
            ("80%  (0.1–0.9)",     0.10,  0.90),
        ];
        let cur = presets.iter()
            .find(|(_, lo, hi)| (o.ci_lo - lo).abs() < 1e-9 && (o.ci_hi - hi).abs() < 1e-9)
            .map(|(l, _, _)| *l).unwrap_or("90%  (0.05–0.95)");
        egui::ComboBox::from_id_salt("vpc_ci_combo")
            .selected_text(cur)
            .width(ui.available_width() - 8.0)
            .show_ui(ui, |ui| {
                for (label, lo, hi) in presets {
                    if ui.selectable_label(cur == label, label).clicked() {
                        o.ci_lo = lo; o.ci_hi = hi;
                    }
                }
            });
    });

    section(ui, "Binning", true, dark, |ui| {
        let o = &mut state.ui.vpc_opts;
        const METHODS: [(&str, &str); 8] = [
            ("jenks",    "Fisher–Jenks natural breaks (recommended)"),
            ("kmeans",   "K-means clustering"),
            ("pretty",   "R pretty() round-number breaks"),
            ("quantile", "Equal-count quantile breaks"),
            ("density",  "Density-weighted breaks"),
            ("time",     "Evenly spaced in time"),
            ("data",     "Break at each observed time"),
            ("manual",   "Specify exact bin boundaries below"),
        ];
        egui::ComboBox::from_id_salt("vpc_bins_combo")
            .selected_text(&o.bins_type)
            .width(ui.available_width() - 8.0)
            .show_ui(ui, |ui| {
                for (m, tip) in METHODS {
                    if ui.selectable_label(o.bins_type == m, m).on_hover_text(tip).clicked() {
                        o.bins_type = m.to_string();
                    }
                }
            });
        ui.add_space(4.0);
        if o.bins_type == "manual" {
            ui.label(egui::RichText::new("Bin separators (comma-separated)").size(10.0).color(theme::fg2(dark)));
            ui.add(egui::TextEdit::singleline(&mut o.manual_bins)
                .hint_text("0, 4, 8, 12, 24, 48, 96, 120")
                .desired_width(ui.available_width()));
            let n_valid = o.manual_bins.split(',')
                .filter(|s| s.trim().parse::<f64>().is_ok()).count();
            if !o.manual_bins.trim().is_empty() && n_valid < 2 {
                ui.label(egui::RichText::new("Enter ≥ 2 comma-separated numbers.")
                    .size(10.0).color(theme::ORANGE));
            }
        } else {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Number of bins").size(11.0));
                ui.add(egui::DragValue::new(&mut o.n_bins).speed(0.2).range(2..=30));
            });
        }
    });

    // ── VPC type (continuous / censored / pcVPC) ──────────────────────────────
    section(ui, "VPC type", true, dark, |ui| {
        let o = &mut state.ui.vpc_opts;
        ui.horizontal(|ui| {
            if ui.selectable_label(o.vpc_type == "continuous", "Continuous").clicked() {
                o.vpc_type = "continuous".to_string();
            }
            if ui.selectable_label(o.vpc_type == "censored", "Censored (BLOQ)").clicked() {
                o.vpc_type = "censored".to_string();
            }
        });
        if o.vpc_type == "censored" {
            ui.add_space(4.0);
            egui::Grid::new("vpc_loq_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label(egui::RichText::new("LLOQ").size(11.0));
                ui.add(egui::TextEdit::singleline(&mut o.lloq_str)
                    .hint_text("e.g. 0.5").desired_width(80.0));
                ui.end_row();
                ui.label(egui::RichText::new("ULOQ").size(11.0));
                ui.add(egui::TextEdit::singleline(&mut o.uloq_str)
                    .hint_text("optional").desired_width(80.0));
                ui.end_row();
            });
            let lloq_s = o.lloq_str.trim().to_string();
            if lloq_s.is_empty() {
                ui.label(egui::RichText::new("LLOQ is required for censored VPC.")
                    .size(10.0).color(theme::ORANGE));
            } else if lloq_s.parse::<f64>().is_err() {
                ui.label(egui::RichText::new("LLOQ must be a number.")
                    .size(10.0).color(theme::RED));
            }
        }
        if o.vpc_type == "continuous" {
            ui.add_space(4.0);
            ui.add_enabled_ui(!is_censored, |ui| {
                if ui.checkbox(&mut o.pred_corr, "Prediction-corrected VPC (pcVPC)")
                    .on_hover_text("Normalises DV and simulations by PRED from sdtab.").changed() {}
                if o.pred_corr {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Lower bound").size(11.0));
                        ui.add(egui::DragValue::new(&mut o.pred_corr_lower_bnd)
                            .speed(0.01).range(0.0..=1e9).fixed_decimals(3));
                    });
                }
            });
        }
    });

    // ── Stratification ────────────────────────────────────────────────────────
    let cols = state.ui.vpc_data_cols.clone();
    section(ui, "Stratification", false, dark, |ui| {
        let o = &mut state.ui.vpc_opts;
        ui.label(egui::RichText::new(
            "Split the VPC into panels by a dataset column (e.g. CMT, ANALYTE, dose group).",
        ).size(10.5).color(theme::fg2(dark)));
        ui.add_space(4.0);

        for (idx_s, (label, field)) in [
            ("Variable 1", &mut o.stratify1),
            ("Variable 2", &mut o.stratify2),
        ].iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(*label).size(11.0));
                let none_label = if idx_s == 0 { "None" } else { "—" };
                egui::ComboBox::from_id_salt(format!("strat_combo_{idx_s}"))
                    .selected_text(if field.is_empty() { none_label } else { field.as_str() })
                    .width(ui.available_width() - 8.0)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(field.is_empty(), none_label).clicked() {
                            **field = String::new();
                        }
                        for col in &cols {
                            if ui.selectable_label(*field == col, col).clicked() {
                                **field = col.clone();
                            }
                        }
                    });
            });
        }
        if !o.stratify1.is_empty() || !o.stratify2.is_empty() {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Facet (ggplot)").size(11.0));
                for f in ["wrap", "rows", "columns"] {
                    if ui.selectable_label(o.facet == f, f).clicked() {
                        o.facet = f.to_string();
                    }
                }
            });
        }
    });

    section(ui, "Appearance", false, dark, |ui| {
        let o = &mut state.ui.vpc_opts;
        ui.checkbox(&mut o.smooth, "Smooth bands (vs. per-bin boxes)");
        ui.checkbox(&mut o.log_y, "Log y-axis");
        if !is_censored {
            ui.checkbox(&mut o.show_points, "Show observed points");
        }
        ui.checkbox(&mut o.show_grid_h, "Horizontal grid lines");
        ui.checkbox(&mut o.show_grid_v, "Vertical grid lines");
        ui.add_space(6.0);
        egui::Grid::new("vpc_color_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label(egui::RichText::new("Band colour").size(11.0));
            ui.color_edit_button_rgb(&mut o.band_color);
            ui.end_row();
            ui.label(egui::RichText::new("Observed lines").size(11.0));
            ui.color_edit_button_rgb(&mut o.obs_color);
            ui.end_row();
        });
    });
}

fn pkg_banner(ui: &mut egui::Ui, state: &AppState, dark: bool) {
    let (text, color) = match &state.ui.vpc_pkg_status {
        None       => ("Checking vpc package…".to_string(), theme::fg3(dark)),
        Some(Ok(v)) => (format!("vpc package v{v} ✓"), theme::GREEN),
        Some(Err(_)) => (
            "vpc package not installed — run install.packages(\"vpc\") in R".to_string(),
            theme::RED,
        ),
    };
    ui.label(egui::RichText::new(text).size(10.5).color(color));
    ui.add_space(4.0);
}

// ---------------------------------------------------------------------------
// Compute / export / pkg-check triggers
// ---------------------------------------------------------------------------

fn build_config(state: &AppState, idx: usize) -> Option<VpcConfig> {
    let model_path = state.workspace.models[idx].model.path.clone();
    let fitrx_path = state.workspace.models[idx].fitrx_path.clone();
    let data_path  = state.ui.vpc_data_path.clone()?;
    let o = &state.ui.vpc_opts;

    let cache_path = r_extract::vpc_cache_path(
        &model_path, &data_path, fitrx_path.as_deref(), o.n_sim, o.seed,
    );
    let manual_bins = if o.bins_type == "manual" {
        let parsed: Vec<f64> = o.manual_bins.split(',')
            .filter_map(|s| s.trim().parse::<f64>().ok()).collect();
        (parsed.len() >= 2).then_some(parsed)
    } else { None };

    let [r, g, b] = o.band_color;
    let band_color = format!("#{:02x}{:02x}{:02x}", (r*255.) as u8, (g*255.) as u8, (b*255.) as u8);
    let lloq = o.lloq_str.trim().parse::<f64>().ok();
    let uloq = o.uloq_str.trim().parse::<f64>().ok();
    let stratify: Vec<String> = [o.stratify1.trim(), o.stratify2.trim()]
        .iter().filter(|s| !s.is_empty()).map(|s| s.to_string()).collect();

    Some(VpcConfig {
        model_path: model_path.to_string_lossy().into_owned(),
        data_path:  data_path.to_string_lossy().into_owned(),
        fitrx_path: fitrx_path.map(|p| p.to_string_lossy().into_owned()),
        cache_path: cache_path.to_string_lossy().into_owned(),
        n_sim: o.n_sim, seed: o.seed,
        pi_lo: o.pi_lo, pi_hi: o.pi_hi,
        ci_lo: o.ci_lo, ci_hi: o.ci_hi,
        bins_type: o.bins_type.clone(), n_bins: o.n_bins,
        manual_bins, log_y: o.log_y, smooth: o.smooth,
        show_points: o.show_points, band_color,
        vpc_type: o.vpc_type.clone(), lloq, uloq,
        pred_corr: o.pred_corr, pred_corr_lower_bnd: o.pred_corr_lower_bnd,
        stratify, facet: o.facet.clone(),
    })
}

fn start_compute(ui: &egui::Ui, state: &mut AppState, idx: usize, stem: &str) {
    let Some(cfg) = build_config(state, idx) else { return; };
    state.workspace.vpc_computing.insert(stem.to_string());
    let tx = state.worker_tx.clone();
    let ctx = ui.ctx().clone();
    let stem_cl = stem.to_string();
    std::thread::spawn(move || {
        match r_extract::compute_vpc(&cfg) {
            Ok(data) => { let _ = tx.send(WorkerMsg::RVpcComplete { stem: stem_cl, data: Box::new(data) }); }
            Err(e)   => { let _ = tx.send(WorkerMsg::RTaskError { context: format!("vpc {stem_cl}"), message: e }); }
        }
        ctx.request_repaint();
    });
}

/// Called from the OS-native script viewport (has `egui::Context`, not `egui::Ui`).
fn start_export_from_ctx(ctx: &egui::Context, state: &mut AppState, idx: usize, stem: &str) {
    let Some(cfg) = build_config(state, idx) else { return; };
    let model_path = state.workspace.models[idx].model.path.clone();
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs()).unwrap_or(0);
    let png_path = model_path
        .parent().unwrap_or_else(|| std::path::Path::new("."))
        .join(format!("{stem}_vpc_{unix}.png"));

    state.ui.vpc_exporting = true;
    let script  = state.ui.vpc_script.clone();
    let tx      = state.worker_tx.clone();
    let ctx_cl  = ctx.clone();
    let stem_cl = stem.to_string();
    std::thread::spawn(move || {
        match r_extract::export_vpc_plot(&cfg, &png_path, &script) {
            Ok(()) => {
                let _ = tx.send(WorkerMsg::VpcPlotExported { path: png_path.to_string_lossy().into_owned() });
            }
            Err(e) => {
                let _ = tx.send(WorkerMsg::RTaskError { context: format!("vpc_export {stem_cl}"), message: e });
            }
        }
        ctx_cl.request_repaint();
    });
}

fn ensure_pkg_check(ui: &egui::Ui, state: &mut AppState) {
    if state.ui.vpc_pkg_status.is_some() || state.ui.vpc_pkg_checking { return; }
    state.ui.vpc_pkg_checking = true;
    let tx = state.worker_tx.clone();
    let ctx = ui.ctx().clone();
    std::thread::spawn(move || {
        let res = r_extract::vpc_package_version();
        let _ = tx.send(WorkerMsg::VpcPkgStatus(res));
        ctx.request_repaint();
    });
}

// ---------------------------------------------------------------------------
// Plot
// ---------------------------------------------------------------------------

fn show_vpc_plot(ui: &mut egui::Ui, vpc: &VpcResult, opts: &VpcOpts, dark: bool) {
    if vpc.vpc_dat.is_empty() && vpc.aggr_obs.is_empty() {
        return hint(ui, "VPC returned no data — check the dataset and binning.");
    }

    // Collect unique strata (unstratified data has strat = "1").
    let strata: Vec<String> = {
        let mut s: Vec<String> = vpc.vpc_dat.iter().map(|b| b.strat.clone())
            .chain(vpc.aggr_obs.iter().map(|b| b.strat.clone()))
            .collect::<std::collections::BTreeSet<_>>().into_iter().collect();
        if s.is_empty() { s.push("1".to_string()); }
        s
    };
    let n_strata = strata.len();

    // Available height split equally across panels, minus a small label row each.
    let avail_h  = ui.available_height();
    let panel_h  = (avail_h / n_strata as f32).max(80.0);
    let label_h  = if n_strata > 1 { 18.0 } else { 0.0 };
    let plot_h   = (panel_h - label_h).max(60.0);

    // Colors
    let [br, bg, bb] = opts.band_color;
    let (ir, ig, ib) = ((br*255.) as u8, (bg*255.) as u8, (bb*255.) as u8);
    let pi_fill    = egui::Color32::from_rgba_unmultiplied(ir, ig, ib, 55);
    let med_fill   = egui::Color32::from_rgba_unmultiplied(ir, ig, ib, 100);
    let pi_stroke  = egui::Stroke::new(0.5, egui::Color32::from_rgba_unmultiplied(ir, ig, ib, 140));
    let med_stroke = egui::Stroke::new(0.5, egui::Color32::from_rgba_unmultiplied(ir, ig, ib, 200));
    let [or_, og, ob] = opts.obs_color;
    let obs_col = egui::Color32::from_rgb((or_*255.) as u8, (og*255.) as u8, (ob*255.) as u8);

    let is_censored  = vpc.vpc_mode == "censored";
    let is_pred_corr = opts.pred_corr && !is_censored;
    // Log-y is meaningful only for continuous concentration VPC.
    let log_y = opts.log_y && !is_censored;
    let ly = |v: f64| -> f64 { if log_y && v > 0.0 { v.log10() } else { v } };

    let pi_pct    = format!("{:.0}th", opts.pi_lo * 100.0);
    let pi_hi_pct = format!("{:.0}th", opts.pi_hi * 100.0);
    let y_label = match (is_censored, is_pred_corr, log_y) {
        (true,  _,     _    ) => "Fraction below LOQ",
        (false, true,  true ) => "log₁₀(Pred-corr. DV)",
        (false, true,  false) => "Pred-corrected DV",
        (false, false, true ) => "log₁₀(Concentration)",
        (false, false, false) => "Concentration",
    };

    // Build quads for one band within a given stratum.
    // y-values are run through `ly` for optional log transform.
    let make_quads = |bands: &[&crate::domain::VpcBandRow],
                      lo_fn: &dyn Fn(&crate::domain::VpcBandRow) -> Option<f64>,
                      hi_fn: &dyn Fn(&crate::domain::VpcBandRow) -> Option<f64>| -> Vec<Vec<[f64; 2]>> {
        if opts.smooth {
            bands.windows(2).filter_map(|w| {
                let (b0, b1) = (w[0], w[1]);
                let (lo0, hi0) = (lo_fn(b0)?, hi_fn(b0)?);
                let (lo1, hi1) = (lo_fn(b1)?, hi_fn(b1)?);
                let (x0, x1) = (b0.bin_mid?, b1.bin_mid?);
                if [lo0, hi0, lo1, hi1, x0, x1].iter().any(|v| !v.is_finite()) { return None; }
                Some(vec![[x0, ly(lo0)], [x1, ly(lo1)], [x1, ly(hi1)], [x0, ly(hi0)]])
            }).collect()
        } else {
            bands.iter().filter_map(|b| {
                let (lo_v, hi_v) = (lo_fn(b)?, hi_fn(b)?);
                let (x0, x1) = (b.bin_min?, b.bin_max?);
                if [lo_v, hi_v, x0, x1].iter().any(|v| !v.is_finite()) { return None; }
                Some(vec![[x0, ly(lo_v)], [x1, ly(lo_v)], [x1, ly(hi_v)], [x0, ly(hi_v)]])
            }).collect()
        }
    };

    let draw_band = |plot_ui: &mut egui_plot::PlotUi,
                     polys: &Vec<Vec<[f64; 2]>>,
                     fill: egui::Color32,
                     stroke: egui::Stroke,
                     name: &str| {
        for (i, poly) in polys.iter().enumerate() {
            if poly.len() < 3 { continue; }
            let p = Polygon::new(PlotPoints::from(poly.clone())).fill_color(fill).stroke(stroke);
            let p = if i == 0 { p.name(name) } else { p };
            plot_ui.polygon(p);
        }
    };

    egui::ScrollArea::vertical()
        .id_salt("vpc_plot_scroll")
        .show(ui, |ui| {
        for (si, strat) in strata.iter().enumerate() {
            // Panel label for stratified data.
            if n_strata > 1 {
                let prefix = if vpc.strat_names.is_empty() {
                    "Stratum".to_string()
                } else {
                    vpc.strat_names.join(", ")
                };
                ui.label(egui::RichText::new(format!("{prefix} = {strat}"))
                    .strong().size(11.0).color(theme::fg2(dark)));
            }

            // Filter bands and obs rows for this stratum.
            let mut bands: Vec<&crate::domain::VpcBandRow> = vpc.vpc_dat.iter()
                .filter(|b| b.strat == *strat).collect();
            bands.sort_by(|a, b| a.bin_mid.unwrap_or(f64::NAN)
                .partial_cmp(&b.bin_mid.unwrap_or(f64::NAN))
                .unwrap_or(std::cmp::Ordering::Equal));
            let mut obs_rows: Vec<&crate::domain::VpcObsRow> = vpc.aggr_obs.iter()
                .filter(|b| b.strat == *strat).collect();
            obs_rows.sort_by(|a, b| a.bin_mid.unwrap_or(f64::NAN)
                .partial_cmp(&b.bin_mid.unwrap_or(f64::NAN))
                .unwrap_or(std::cmp::Ordering::Equal));

            // Observed percentile lines (y-transformed for log).
            let obs_line = |sel: &dyn Fn(&crate::domain::VpcObsRow) -> Option<f64>| -> Vec<[f64; 2]> {
                obs_rows.iter()
                    .filter_map(|b| Some([b.bin_mid?, ly(sel(b)?)]))
                    .filter(|p| p[0].is_finite() && p[1].is_finite()).collect()
            };

            // obs_points carry no strat info so can't be split per panel;
            // only show them on unstratified plots.
            let obs_pts: Vec<[f64; 2]> = if opts.show_points && !is_censored && n_strata == 1 {
                vpc.obs_points.iter()
                    .filter_map(|p| p.dv.map(|dv| [p.time, ly(dv)]))
                    .filter(|p| p[0].is_finite() && p[1].is_finite()).collect()
            } else { vec![] };

            // Continuous: q5/q50/q95 bands + obs 5th/median/95th lines.
            // Censored:   q50 band only + obs median (fraction below LOQ).
            let pi_lo_polys = if !is_censored { make_quads(&bands, &|b| b.q5_low,  &|b| b.q5_up)  } else { vec![] };
            let med_polys   = make_quads(&bands, &|b| b.q50_low, &|b| b.q50_up);
            let pi_hi_polys = if !is_censored { make_quads(&bands, &|b| b.q95_low, &|b| b.q95_up) } else { vec![] };

            let obs_lo  = if !is_censored { obs_line(&|b| b.obs5)  } else { vec![] };
            let obs_med = obs_line(&|b| b.obs50);
            let obs_hi  = if !is_censored { obs_line(&|b| b.obs95) } else { vec![] };

            let mut pl = Plot::new(format!("vpc_plot_{si}"))
                .height(plot_h)
                .x_axis_label("Time")
                .y_axis_label(y_label)
                .legend(Legend::default())
                .show_grid(egui::Vec2b::new(opts.show_grid_v, opts.show_grid_h));
            if log_y {
                pl = pl.y_grid_spacer(egui_plot::log_grid_spacer(10));
            }
            pl.show(ui, |plot_ui| {
                    // Bin-edge separators.
                    if opts.show_grid_v {
                        let ec = egui::Color32::from_rgba_unmultiplied(0x88, 0x88, 0x88, 40);
                        for edge in vpc.bins.iter().filter(|e| e.is_finite()) {
                            plot_ui.vline(VLine::new(*edge).color(ec).width(0.5));
                        }
                    }
                    // LOQ reference line(s) for censored VPC.
                    if is_censored {
                        if let Some(lloq) = vpc.lloq {
                            plot_ui.hline(HLine::new(lloq)
                                .color(egui::Color32::from_rgb(0x99, 0x00, 0x00)).width(1.0).name("LLOQ"));
                        }
                        if let Some(uloq) = vpc.uloq {
                            plot_ui.hline(HLine::new(uloq)
                                .color(egui::Color32::from_rgb(0x99, 0x00, 0x00)).width(1.0).name("ULOQ"));
                        }
                    }
                    // Simulated bands.
                    draw_band(plot_ui, &pi_lo_polys, pi_fill,  pi_stroke,  &format!("Sim {pi_pct} CI"));
                    draw_band(plot_ui, &med_polys,   med_fill, med_stroke, "Sim median CI");
                    draw_band(plot_ui, &pi_hi_polys, pi_fill,  pi_stroke,  &format!("Sim {pi_hi_pct} CI"));
                    // Observed lines.
                    if obs_lo.len() >= 2 {
                        plot_ui.line(Line::new(PlotPoints::from(obs_lo))
                            .name("Obs 5th").color(obs_col).width(1.0)
                            .style(egui_plot::LineStyle::Dashed { length: 8.0 }));
                    }
                    if obs_med.len() >= 2 {
                        let lbl = if is_censored { "Obs fraction below LOQ" } else { "Obs median" };
                        plot_ui.line(Line::new(PlotPoints::from(obs_med))
                            .name(lbl).color(obs_col).width(2.0));
                    }
                    if obs_hi.len() >= 2 {
                        plot_ui.line(Line::new(PlotPoints::from(obs_hi))
                            .name("Obs 95th").color(obs_col).width(1.0)
                            .style(egui_plot::LineStyle::Dashed { length: 8.0 }));
                    }
                    if !obs_pts.is_empty() {
                        plot_ui.points(Points::new(PlotPoints::from(obs_pts))
                            .name("Observed").radius(2.5)
                            .shape(egui_plot::MarkerShape::Circle).filled(false).color(obs_col));
                    }
                });
        }
    });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn computing_spinner(ui: &mut egui::Ui, opts: &VpcOpts) {
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.add(egui::Spinner::new().size(32.0));
            ui.add_space(12.0);
            ui.label(egui::RichText::new("Computing VPC via the vpc package…")
                .color(theme::FG2).size(13.0));
            ui.add_space(6.0);
            ui.label(egui::RichText::new(format!(
                "ferx_simulate({} replicates) + vpc(). \
                 First run for these simulation settings may take a few minutes; \
                 later option changes reuse the cached simulation.",
                opts.n_sim
            )).color(theme::FG3).size(11.0));
        });
    });
}

fn section(
    ui: &mut egui::Ui, title: &str, expanded: bool, dark: bool,
    content: impl FnOnce(&mut egui::Ui),
) {
    let border = if dark { egui::Color32::from_gray(55) } else { egui::Color32::from_gray(210) };
    egui::Frame::new()
        .fill(theme::card_fill(dark))
        .inner_margin(egui::Margin::same(0))
        .stroke(egui::Stroke::new(1.0, border))
        .corner_radius(egui::CornerRadius::same(5))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::CollapsingHeader::new(
                egui::RichText::new(title).size(11.0).strong().color(theme::fg2(dark)),
            )
            .default_open(expanded)
            .show(ui, |ui| {
                ui.add_space(2.0);
                egui::Frame::new()
                    .inner_margin(egui::Margin { left: 8, right: 8, top: 0, bottom: 8 })
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        content(ui);
                    });
            });
        });
    ui.add_space(4.0);
}

/// Read column names from the first line of a CSV file (fast — reads one line only).
fn read_csv_header(path: &std::path::Path) -> Vec<String> {
    use std::io::{BufRead, BufReader};
    let Ok(f) = std::fs::File::open(path) else { return vec![]; };
    let mut line = String::new();
    let _ = BufReader::new(f).read_line(&mut line);
    line.trim()
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn manual_bins_valid(o: &VpcOpts) -> bool {
    if o.bins_type != "manual" { return true; }
    o.manual_bins.split(',').filter(|s| s.trim().parse::<f64>().is_ok()).count() >= 2
}

fn lloq_valid(o: &VpcOpts) -> bool {
    if o.vpc_type != "censored" { return true; }
    !o.lloq_str.trim().is_empty() && o.lloq_str.trim().parse::<f64>().is_ok()
}

fn hint(ui: &mut egui::Ui, msg: &str) {
    let color = if ui.visuals().dark_mode { theme::FG3 } else { egui::Color32::from_gray(160) };
    ui.centered_and_justified(|ui| {
        ui.label(egui::RichText::new(msg).color(color).size(13.0));
    });
}
