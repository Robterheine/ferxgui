/// Simulate tab — runs `ferx_simulate()` and writes a merged CSV (original
/// input data + SIM + IPRED/DV_SIM) to disk. Paired with "Sim Plot": this tab
/// computes, Sim Plot displays whatever CSV it's pointed at (from here or
/// anywhere else) — see design/ferxgui-simulation-feature-plan.md.
use eframe::egui;

use crate::app::theme;
use crate::domain::SimRunConfig;
use crate::io::r_extract;
use crate::state::{AppState, SimBasis};
use crate::workers::messages::WorkerMsg;

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;

    let Some(idx) = state.ui.selected_model else {
        return hint(ui, "Select a model in the Models tab to run a simulation.");
    };

    let stem = state.workspace.models[idx].model.stem.clone();
    let has_fitrx = state.workspace.models[idx].fitrx_path.is_some();
    let has_covariance = state.workspace.models[idx].fit.as_ref()
        .map(|f| f.covariance_ok).unwrap_or(false);
    let has_sir_resamples = state.workspace.sir_results.get(&stem)
        .map(|r| r.sir_resamples_n > 0).unwrap_or(false);

    // A fit (and, for the uncertainty bases, a covariance matrix or kept SIR
    // resamples) is required for anything beyond "Initial estimates" — fall
    // back if the currently selected basis is no longer available (e.g.
    // after switching away from a fitted model).
    if state.ui.simrun_basis == SimBasis::Fitted && !has_fitrx {
        state.ui.simrun_basis = SimBasis::Initial;
    }
    if state.ui.simrun_basis == SimBasis::AsymptoticUncertainty && !has_covariance {
        state.ui.simrun_basis = SimBasis::Initial;
    }
    if state.ui.simrun_basis == SimBasis::SirUncertainty && !has_sir_resamples {
        state.ui.simrun_basis = SimBasis::Initial;
    }

    // Auto-populate the data path from the most recent run record for this model.
    if state.ui.simrun_data_path.is_none() {
        state.ui.simrun_data_path = state
            .run
            .run_history
            .iter()
            .rev()
            .find(|r| r.model_stem == stem && r.data_path.is_some())
            .and_then(|r| r.data_path.clone());
    }

    let left_w = 300.0_f32;
    ui.horizontal_top(|ui| {
        // ── Left panel ────────────────────────────────────────────────────
        ui.vertical(|ui| {
            ui.set_width(left_w);

            let avail_h  = ui.available_height();
            let btns_h   = 40.0;
            let scroll_h = (avail_h - btns_h).max(100.0);

            egui::ScrollArea::vertical()
                .id_salt("simulate_left_scroll")
                .max_height(scroll_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(left_w - 6.0);
                    show_options(ui, state, idx, has_fitrx, has_covariance, has_sir_resamples, dark);
                });

            let running    = state.workspace.simrun_computing.contains(&stem);
            let has_data   = state.ui.simrun_data_path.is_some();

            ui.add_space(4.0);
            let lbl = if running { "Simulating…" } else { "Run simulation" };
            if ui.add_enabled(
                has_data && !running,
                egui::Button::new(egui::RichText::new(lbl).size(14.0).strong())
                    .fill(theme::ACCENT)
                    .min_size(egui::vec2(ui.available_width(), 34.0)),
            ).clicked() {
                start_compute(ui, state, idx, &stem);
            }
        });

        ui.separator();

        // ── Right: status / result ──────────────────────────────────────
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(&stem).size(12.0).strong().color(theme::fg(dark)));
            ui.add_space(4.0);

            if state.workspace.simrun_computing.contains(&stem) {
                computing_spinner(ui, state.ui.simrun_basis, state.ui.simrun_n_sim, state.ui.simrun_n_draws, state.ui.simrun_n_sim_per_draw);
                ui.ctx().request_repaint();
            } else if let Some(result) = state.workspace.simrun_results.get(&stem).cloned() {
                show_result(ui, state, &result, dark);
            } else {
                hint(ui, "Set options on the left, then click Run simulation.");
            }
        });
    });
}

// ---------------------------------------------------------------------------
// Options panel
// ---------------------------------------------------------------------------

fn show_options(ui: &mut egui::Ui, state: &mut AppState, idx: usize, has_fitrx: bool, has_covariance: bool, has_sir_resamples: bool, dark: bool) {
    section(ui, "Data file", true, dark, |ui| {
        let path_str = state.ui.simrun_data_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "No data file selected".to_string());
        let color = if state.ui.simrun_data_path.is_some() {
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
                state.ui.simrun_data_path = Some(p);
            }
        }
    });

    section(ui, "Basis", true, dark, |ui| {
        ui.label(egui::RichText::new(
            "Which parameter set to simulate at.",
        ).size(10.5).color(theme::fg2(dark)));
        ui.add_space(4.0);
        if ui.selectable_label(state.ui.simrun_basis == SimBasis::Initial, "Initial estimates")
            .on_hover_text("Simulate at the model file's initial values (prior predictive).")
            .clicked()
        {
            state.ui.simrun_basis = SimBasis::Initial;
        }
        ui.add_enabled_ui(has_fitrx, |ui| {
            if ui.selectable_label(state.ui.simrun_basis == SimBasis::Fitted, "Fitted estimates")
                .on_hover_text(if has_fitrx {
                    "Simulate at the saved fit's theta/omega/sigma (posterior predictive)."
                } else {
                    "Run this model first — fitted estimates require a completed fit."
                })
                .clicked()
            {
                state.ui.simrun_basis = SimBasis::Fitted;
            }
        });
        ui.add_enabled_ui(has_covariance, |ui| {
            if ui.selectable_label(state.ui.simrun_basis == SimBasis::AsymptoticUncertainty, "With uncertainty (asymptotic)")
                .on_hover_text(if has_covariance {
                    "Draw parameter sets from a multivariate normal around the ML estimate, \
                     using the fit's covariance matrix — propagates parameter uncertainty \
                     into the simulation, not just individual variability."
                } else {
                    "Run this model with covariance = TRUE first — this basis needs the fit's covariance matrix."
                })
                .clicked()
            {
                state.ui.simrun_basis = SimBasis::AsymptoticUncertainty;
            }
        });
        ui.add_enabled_ui(has_sir_resamples, |ui| {
            if ui.selectable_label(state.ui.simrun_basis == SimBasis::SirUncertainty, "With uncertainty (SIR)")
                .on_hover_text(if has_sir_resamples {
                    "Draw parameter sets by resampling from the SIR tab's results — a more \
                     rigorous uncertainty estimate than the asymptotic approximation."
                } else {
                    "Run SIR in the SIR tab first, with \"Keep resamples\" enabled."
                })
                .clicked()
            {
                state.ui.simrun_basis = SimBasis::SirUncertainty;
            }
        });
    });

    let is_uncertainty = matches!(
        state.ui.simrun_basis,
        SimBasis::AsymptoticUncertainty | SimBasis::SirUncertainty
    );
    if is_uncertainty {
        section(ui, "Simulation", true, dark, |ui| {
            egui::Grid::new("simrun_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label(egui::RichText::new("Uncertainty draws").size(11.0));
                ui.add(egui::DragValue::new(&mut state.ui.simrun_n_draws).speed(10).range(1..=2000));
                ui.end_row();
                ui.label(egui::RichText::new("Replicates per draw").size(11.0));
                ui.add(egui::DragValue::new(&mut state.ui.simrun_n_sim_per_draw).speed(1).range(1..=100));
                ui.end_row();
                ui.label(egui::RichText::new("Seed").size(11.0));
                ui.add(egui::DragValue::new(&mut state.ui.simrun_seed).speed(1));
                ui.end_row();
            });
            ui.label(egui::RichText::new("Total rows = draws × replicates per draw × observations.")
                .size(9.5).color(theme::fg3(dark)).italics());
        });
    } else {
        section(ui, "Simulation", true, dark, |ui| {
            egui::Grid::new("simrun_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label(egui::RichText::new("Replicates").size(11.0));
                ui.add(egui::DragValue::new(&mut state.ui.simrun_n_sim).speed(10).range(1..=5000));
                ui.end_row();
                ui.label(egui::RichText::new("Seed").size(11.0));
                ui.add(egui::DragValue::new(&mut state.ui.simrun_seed).speed(1));
                ui.end_row();
            });
        });
    }

    section(ui, "Output", true, dark, |ui| {
        let default_preview = default_out_path(state, idx)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        ui.label(egui::RichText::new("Output CSV path (blank = auto)")
            .size(10.0).color(theme::fg2(dark)));
        ui.add(egui::TextEdit::singleline(&mut state.ui.simrun_out_path)
            .hint_text(&default_preview)
            .desired_width(ui.available_width()));
        if state.ui.simrun_out_path.trim().is_empty() {
            ui.label(egui::RichText::new(format!("→ {default_preview}"))
                .size(9.5).color(theme::fg3(dark)).italics());
        }
        ui.add_space(4.0);
        if ui.button("Choose…").clicked() {
            if let Some(p) = rfd::FileDialog::new()
                .add_filter("CSV", &["csv"])
                .set_file_name("sim_output.csv")
                .save_file()
            {
                state.ui.simrun_out_path = p.to_string_lossy().into_owned();
            }
        }
    });
}

fn default_out_path(state: &AppState, idx: usize) -> Option<std::path::PathBuf> {
    let model = &state.workspace.models.get(idx)?.model;
    let stem = &model.stem;
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs()).unwrap_or(0);
    Some(model.path
        .parent().unwrap_or_else(|| std::path::Path::new("."))
        .join(format!("{stem}_sim_{unix}.csv")))
}

// ---------------------------------------------------------------------------
// Compute
// ---------------------------------------------------------------------------

fn build_config(state: &AppState, idx: usize) -> Option<SimRunConfig> {
    let model_path = state.workspace.models[idx].model.path.clone();
    let fitrx_path = state.workspace.models[idx].fitrx_path.clone();
    let data_path  = state.ui.simrun_data_path.clone()?;

    let out_path = if state.ui.simrun_out_path.trim().is_empty() {
        default_out_path(state, idx)?
    } else {
        std::path::PathBuf::from(state.ui.simrun_out_path.trim())
    };

    let fitrx_arg = match state.ui.simrun_basis {
        SimBasis::Fitted | SimBasis::AsymptoticUncertainty | SimBasis::SirUncertainty
                          => fitrx_path.map(|p| p.to_string_lossy().into_owned()),
        SimBasis::Initial => None,
    };

    let (uncertainty_method, n_uncertainty_draws, n_sim_per_draw) = match state.ui.simrun_basis {
        SimBasis::AsymptoticUncertainty => (
            Some("asymptotic".to_string()),
            Some(state.ui.simrun_n_draws),
            Some(state.ui.simrun_n_sim_per_draw),
        ),
        SimBasis::SirUncertainty => (
            Some("sir".to_string()),
            Some(state.ui.simrun_n_draws),
            Some(state.ui.simrun_n_sim_per_draw),
        ),
        SimBasis::Initial | SimBasis::Fitted => (None, None, None),
    };

    let (sir_resamples_flat, sir_resamples_n, sir_resamples_dim) = if state.ui.simrun_basis == SimBasis::SirUncertainty {
        let stem = &state.workspace.models[idx].model.stem;
        let sir = state.workspace.sir_results.get(stem)?;
        (Some(sir.sir_resamples_flat.clone()), Some(sir.sir_resamples_n), Some(sir.sir_resamples_dim))
    } else {
        (None, None, None)
    };

    Some(SimRunConfig {
        model_path: model_path.to_string_lossy().into_owned(),
        data_path:  data_path.to_string_lossy().into_owned(),
        fitrx_path: fitrx_arg,
        n_sim: state.ui.simrun_n_sim,
        seed:  state.ui.simrun_seed,
        out_path: out_path.to_string_lossy().into_owned(),
        uncertainty_method,
        n_uncertainty_draws,
        n_sim_per_draw,
        sir_resamples_flat,
        sir_resamples_n,
        sir_resamples_dim,
    })
}

fn start_compute(ui: &egui::Ui, state: &mut AppState, idx: usize, stem: &str) {
    let Some(cfg) = build_config(state, idx) else { return; };
    state.workspace.simrun_computing.insert(stem.to_string());
    state.workspace.simrun_results.remove(stem);
    let tx = state.worker_tx.clone();
    let ctx = ui.ctx().clone();
    let stem_cl = stem.to_string();
    std::thread::spawn(move || {
        match r_extract::compute_simulation(&cfg) {
            Ok(result) => { let _ = tx.send(WorkerMsg::SimRunComplete { stem: stem_cl, result: Box::new(result) }); }
            Err(e)     => { let _ = tx.send(WorkerMsg::RTaskError { context: format!("simulate {stem_cl}"), message: e }); }
        }
        ctx.request_repaint();
    });
}

// ---------------------------------------------------------------------------
// Result panel
// ---------------------------------------------------------------------------

fn show_result(ui: &mut egui::Ui, state: &mut AppState, result: &crate::domain::SimRunResult, dark: bool) {
    egui::Frame::new()
        .fill(theme::card_fill(dark))
        .inner_margin(egui::Margin::same(12))
        .corner_radius(egui::CornerRadius::same(6))
        .show(ui, |ui| {
            ui.label(egui::RichText::new("✔ Simulation complete").color(theme::GREEN).size(13.0).strong());
            ui.add_space(6.0);
            ui.label(egui::RichText::new(format!("{} rows written", result.n_rows))
                .size(12.0).color(theme::fg2(dark)));
            ui.add(egui::Label::new(
                egui::RichText::new(&result.out_path).monospace().size(10.5).color(theme::fg3(dark)),
            ).truncate());
            ui.add_space(4.0);
            ui.label(egui::RichText::new(format!("Columns: {}", result.columns.join(", ")))
                .size(9.5).color(theme::fg3(dark)).italics());
            ui.add_space(10.0);
            if ui.add(
                egui::Button::new(egui::RichText::new("Open in Sim Plot").size(13.0).strong())
                    .fill(theme::ACCENT)
                    .min_size(egui::vec2(160.0, 30.0)),
            ).clicked() {
                state.sim.file_path = result.out_path.clone();
                crate::ui::sim_tab::load_sim_file(state);
                state.ui.active_tab = crate::state::Tab::SimPlot;
            }
        });
}

fn computing_spinner(ui: &mut egui::Ui, basis: SimBasis, n_sim: u32, n_draws: u32, n_sim_per_draw: u32) {
    let detail = match basis {
        SimBasis::AsymptoticUncertainty | SimBasis::SirUncertainty => format!(
            "ferx_simulate_with_uncertainty({n_draws} draws × {n_sim_per_draw} per draw), merged with the input dataset."
        ),
        SimBasis::Initial | SimBasis::Fitted => format!(
            "ferx_simulate({n_sim} replicates), merged with the input dataset."
        ),
    };
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.add(egui::Spinner::new().size(32.0));
            ui.add_space(12.0);
            ui.label(egui::RichText::new("Simulating…")
                .color(theme::FG2).size(13.0));
            ui.add_space(6.0);
            ui.label(egui::RichText::new(detail).color(theme::FG3).size(11.0));
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

fn hint(ui: &mut egui::Ui, msg: &str) {
    let color = if ui.visuals().dark_mode { theme::FG3 } else { egui::Color32::from_gray(160) };
    ui.centered_and_justified(|ui| {
        ui.label(egui::RichText::new(msg).color(color).size(13.0));
    });
}
