use std::collections::HashMap;

use eframe::egui;
use egui_extras::{Column, TableBuilder};

use crate::app::theme;
use crate::domain::{JobStatus, ModelDecision, ModelStatus, RunRecord, RunStatus};
use crate::io::ferx_file::{tokenise_line, TokenKind};
use crate::io::persistence::save_model_meta;
use crate::state::{AppState, ModelPill, ModelStatusFilter};
use crate::workers::{
    messages::CancelMode,
    run::spawn_detached_run,
    run_manifest::{manifest_path, running_dir},
};

// ── Column widths ────────────────────────────────────────────────────────────

const W_STAR:   f32 = 22.0;
const W_NAME:   f32 = 140.0;
const W_DESC:   f32 = 190.0;
const W_DATA:   f32 = 110.0;
const W_OFV:    f32 = 78.0;
const W_DOFV:   f32 = 70.0;
const W_COV:    f32 = 36.0;
const W_AIC:    f32 = 75.0;
const W_CN:     f32 = 55.0;
const W_METH:   f32 = 70.0;
const W_INDOBS: f32 = 75.0;
const W_ETA:    f32 = 72.0;
const W_EPS:    f32 = 65.0;
const W_NPAR:   f32 = 42.0;
const W_TIME:   f32 = 60.0;
const W_FLAG:   f32 = 20.0;

// ── Public entry point ───────────────────────────────────────────────────────

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    // If selected model changed, reload editor buffer.
    sync_editor_buffer(state);

    show_top_bar(ui, state);
    ui.separator();

    egui::SidePanel::left("model_list_panel")
        .default_width(650.0)
        .width_range(280.0..=1100.0)
        .resizable(true)
        .show_inside(ui, |ui| {
            show_model_list(ui, state);
        });

    egui::CentralPanel::default().show_inside(ui, |ui| {
        show_detail_panel(ui, state);
    });

    // Modal dialogs float over all panels.
    show_bookmark_dialog(ui.ctx(), state);
    show_duplicate_dialog(ui.ctx(), state);
    show_delete_dialog(ui.ctx(), state);
    show_new_model_dialog(ui.ctx(), state);
    show_compare_picker(ui.ctx(), state);
    show_compare_dialog(ui.ctx(), state);
}

// ── Top bar ──────────────────────────────────────────────────────────────────

fn show_top_bar(ui: &mut egui::Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;
    let label_fg = if dark { theme::FG2 } else { egui::Color32::from_gray(100) };
    let path_fg  = if dark { theme::FG  } else { egui::Color32::from_gray(30)  };

    // Row 1: directory breadcrumb + bookmark star + bookmarks dropdown.
    ui.horizontal(|ui| {
        if let Some(dir) = state.workspace.directory.as_ref() {
            let name = dir.file_name().unwrap_or_default().to_string_lossy().to_string();
            ui.add(egui::Label::new(
                egui::RichText::new(name).color(path_fg).strong().size(12.0),
            ).truncate());
        } else {
            ui.label(egui::RichText::new("No directory").color(label_fg).size(12.0));
        }
        if ui.small_button("Change…").clicked() {
            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                state.set_directory(dir);
            }
        }
        if state.workspace.directory.is_some() && ui.small_button("Rescan").clicked() {
            state.trigger_scan();
        }

        // "Bookmark project" pill — replaces the ambiguous ☆ star.
        // Uses a labeled button so it cannot be confused with model-row stars.
        if let Some(dir) = state.workspace.directory.clone() {
            let already_bookmarked = state.workspace.bookmarks
                .iter()
                .any(|b| b.path == dir);
            let (btn_label, btn_fill, btn_fg, tip) = if already_bookmarked {
                (
                    "✔ Bookmark project",
                    theme::ACCENT,
                    egui::Color32::WHITE,
                    "Remove this directory from project bookmarks",
                )
            } else {
                let fill = if dark { theme::BG3 } else { egui::Color32::TRANSPARENT };
                let fg   = if dark { theme::FG3 } else { egui::Color32::from_gray(120) };
                ("+ Bookmark project", fill, fg, "Save this directory as a project bookmark")
            };
            if ui.add(
                egui::Button::new(
                    egui::RichText::new(btn_label).size(11.0).color(btn_fg),
                )
                .fill(btn_fill),
            ).on_hover_text(tip).clicked() {
                if already_bookmarked {
                    state.workspace.bookmarks.retain(|b| b.path != dir);
                    if let Some(app_dir) = &state.workspace.app_dir.clone() {
                        if let Err(e) = crate::io::persistence::save_bookmarks(app_dir, &state.workspace.bookmarks) {
                            state.ui.status_message = format!("Could not save bookmarks: {e}");
                        }
                    }
                } else {
                    // Open the name dialog; default label = directory name.
                    let default_label = dir.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    state.ui.pending_bookmark = Some((dir, default_label));
                }
            }
        }

        // Projects dropdown — lists all bookmarked directories.
        let bm_label = if state.workspace.bookmarks.is_empty() {
            "Projects"
        } else {
            "Projects ▾"
        };
        egui::ComboBox::from_id_salt("bookmarks_combo")
            .selected_text(egui::RichText::new(bm_label).size(12.0).color(label_fg))
            .width(110.0)
            .show_ui(ui, |ui| {
                if state.workspace.bookmarks.is_empty() {
                    ui.label(
                        egui::RichText::new("No bookmarks yet — use '+ Bookmark project' to add one")
                            .color(label_fg)
                            .size(11.0),
                    );
                } else {
                    let bookmarks = state.workspace.bookmarks.clone();
                    let mut remove_idx: Option<usize> = None;
                    for (i, bm) in bookmarks.iter().enumerate() {
                        ui.horizontal(|ui| {
                            if ui.selectable_label(
                                state.workspace.directory.as_deref() == Some(&bm.path),
                                egui::RichText::new(&bm.label).size(12.0),
                            ).clicked() {
                                state.set_directory(bm.path.clone());
                            }
                            let x_color = if dark { theme::FG3 } else { egui::Color32::from_gray(160) };
                            if ui.add(
                                egui::Button::new(egui::RichText::new("✕").size(10.0).color(x_color))
                                    .frame(false),
                            ).on_hover_text("Remove bookmark").clicked() {
                                remove_idx = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove_idx {
                        state.workspace.bookmarks.remove(i);
                        if let Some(app_dir) = &state.workspace.app_dir.clone() {
                            if let Err(e) = crate::io::persistence::save_bookmarks(app_dir, &state.workspace.bookmarks) {
                                state.ui.status_message = format!("Could not save bookmarks: {e}");
                            }
                        }
                    }
                }
            });
    });

    // Row 2: search + status filter + new model.
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Filter:").color(label_fg).size(12.0));
        ui.add(
            egui::TextEdit::singleline(&mut state.ui.model_filter)
                .desired_width(180.0)
                .hint_text("model name…"),
        );

        // Status pills — theme-aware fills so they don't appear black in light mode.
        let inactive_fill = if dark { theme::BG3 } else { egui::Color32::TRANSPARENT };
        let inactive_fg   = if dark { theme::FG2 } else { ui.visuals().text_color() };
        for (label, val) in [
            ("All", ModelStatusFilter::All),
            ("Completed", ModelStatusFilter::Completed),
            ("Failed", ModelStatusFilter::Failed),
        ] {
            let active = state.ui.model_status_filter == val;
            let btn = egui::Button::new(
                egui::RichText::new(label)
                    .size(11.0)
                    .color(if active { egui::Color32::WHITE } else { inactive_fg }),
            )
            .fill(if active { theme::ACCENT } else { inactive_fill })
            .min_size(egui::vec2(70.0, 22.0));
            if ui.add(btn).clicked() {
                state.ui.model_status_filter = val;
            }
        }

        // "New Model…" only appears once a directory is set (it's inert until then).
        if state.workspace.directory.is_some() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new("New Model…").size(12.0))
                            .fill(theme::ACCENT)
                            .min_size(egui::vec2(90.0, 22.0)),
                    )
                    .clicked()
                {
                    state.ui.new_model_stem.clear();
                    state.ui.new_model_dialog = true;
                }

                // Explicit, discoverable entry point into the compare dialog
                // (previously only reachable via right-click "Compare with…"
                // on a model row, which a first-time user has no reason to find).
                let n_fitted = state.workspace.models.iter().filter(|m| m.fit.is_some()).count();
                let btn = egui::Button::new(egui::RichText::new("Compare Models…").size(12.0))
                    .min_size(egui::vec2(115.0, 22.0));
                let resp = ui.add_enabled(n_fitted >= 2, btn);
                let resp = if n_fitted < 2 {
                    resp.on_disabled_hover_text("Need at least two models with a completed fit to compare")
                } else {
                    resp
                };
                if resp.clicked() {
                    state.ui.compare_picker_open = true;
                    state.ui.compare_picker_a = None;
                    state.ui.compare_picker_b = None;
                }
            });
        }
    });
}

// ── Model list table ─────────────────────────────────────────────────────────

/// Flat data extracted from ModelEntry for rendering without borrow issues.
struct ModelRow {
    idx: usize,
    stem: String,
    description: String,
    /// Dataset filename declared in the model's own `[data]` block, if any.
    data_file: Option<String>,
    starred: bool,
    is_reference: bool,
    run_status: RunStatus,
    ofv: f64,
    delta_ofv: f64,
    cov_ok: Option<bool>,
    aic: f64,
    cn: f64,
    method: String,
    n_subjects: usize,
    n_obs: usize,
    max_eta_shrink: f64,
    eps_shrink: f64,
    n_parameters: usize,
    wall_time_secs: f64,
    has_boundary: bool,
    /// Set when a `.fitrx` exists but ferxgui failed to parse it.
    parse_error: Option<String>,
}

fn build_rows(state: &AppState) -> Vec<ModelRow> {
    let ref_ofv = state.reference_ofv();
    let filter = state.ui.model_filter.to_lowercase();
    state
        .workspace
        .models
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            // text filter
            (filter.is_empty() || e.model.stem.to_lowercase().contains(&filter))
            // status filter
            && match state.ui.model_status_filter {
                ModelStatusFilter::All => true,
                ModelStatusFilter::Completed => e.run_status() == RunStatus::Converged,
                ModelStatusFilter::Failed => e.run_status() == RunStatus::Failed,
            }
        })
        .map(|(idx, e)| {
            let fit = e.fit.as_ref();
            ModelRow {
                idx,
                stem: e.model.stem.clone(),
                description: e.description().to_string(),
                data_file: e.model.data_path.clone(),
                starred: e.meta.starred,
                is_reference: state.ui.reference_model == Some(idx),
                run_status: e.run_status(),
                ofv: fit.map(|f| f.ofv).unwrap_or(f64::NAN),
                delta_ofv: e.delta_ofv(ref_ofv),
                cov_ok: fit.map(|f| f.covariance_ok),
                aic: fit.map(|f| f.aic).unwrap_or(f64::NAN),
                cn: fit.map(|f| f.cov_condition_number).unwrap_or(f64::NAN),
                method: fit.map(|f| f.method.clone()).unwrap_or_default(),
                n_subjects: fit.map(|f| f.n_subjects).unwrap_or(0),
                n_obs: fit.map(|f| f.n_obs).unwrap_or(0),
                max_eta_shrink: fit
                    .map(|f| {
                        f.eta_shrinkage
                            .iter()
                            .cloned()
                            .fold(f64::NAN, f64::max)
                    })
                    .unwrap_or(f64::NAN),
                eps_shrink: fit
                    .map(|f| f.eps_shrinkage.first().copied().unwrap_or(f64::NAN))
                    .unwrap_or(f64::NAN),
                n_parameters: fit.map(|f| f.n_parameters).unwrap_or(0),
                wall_time_secs: fit.map(|f| f.wall_time_secs).unwrap_or(0.0),
                has_boundary: fit.map(|f| f.has_boundary_hit()).unwrap_or(false),
                parse_error: e.fit_parse_error.clone(),
            }
        })
        .collect()
}

fn show_empty_state_no_directory(ui: &mut egui::Ui, state: &mut AppState) {
    let dim = if ui.visuals().dark_mode { theme::FG2 } else { egui::Color32::from_gray(120) };
    let top = (ui.available_height() - 130.0) * 0.38;
    ui.add_space(top.max(0.0));
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new("No working directory").size(18.0).strong().color(theme::fg(ui.visuals().dark_mode)));
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("Choose a folder that contains .ferx model files.")
                .color(dim)
                .size(13.0),
        );
        ui.add_space(20.0);
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new("Choose Directory…")
                        .size(13.0)
                        .color(egui::Color32::WHITE),
                )
                .fill(theme::ACCENT)
                .min_size(egui::vec2(160.0, 30.0)),
            )
            .clicked()
        {
            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                state.set_directory(dir);
            }
        }
    });
}

fn show_empty_state_no_models(ui: &mut egui::Ui, state: &AppState) {
    let dim = if ui.visuals().dark_mode { theme::FG2 } else { egui::Color32::from_gray(120) };
    let dir_name = state
        .workspace
        .directory
        .as_ref()
        .and_then(|d| d.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let top = (ui.available_height() - 100.0) * 0.38;
    ui.add_space(top.max(0.0));
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new("No models found").size(18.0).strong().color(theme::fg(ui.visuals().dark_mode)));
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(format!("No .ferx files found in '{}'.", dir_name))
                .color(dim)
                .size(13.0),
        );
    });
}

fn show_model_list(ui: &mut egui::Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;
    // ── Zero states ───────────────────────────────────────────────────────────
    if state.workspace.directory.is_none() {
        show_empty_state_no_directory(ui, state);
        return;
    }
    if state.workspace.models.is_empty() {
        if state.workspace.scanning {
            ui.centered_and_justified(|ui| { ui.spinner(); });
        } else {
            show_empty_state_no_models(ui, state);
        }
        return;
    }

    let rows = build_rows(state);

    if rows.is_empty() {
        let dim = if ui.visuals().dark_mode { theme::FG3 } else { egui::Color32::from_gray(140) };
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new("No models match the current filter.").color(dim).size(13.0));
        });
        return;
    }

    // `new_selection` is written during the TableBuilder pass and read by
    // `tr.set_selected()` in the *same* pass — so we must apply it to the
    // real state immediately (not deferred) so the highlight appears the same
    // frame the click is registered.
    let mut new_selection = state.ui.selected_model;
    let mut toggle_star: Option<usize> = None;
    let mut switch_to_output: Option<usize> = None;
    let mut ctx_action: Option<(usize, CtxAction)> = None;

    egui::ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| {
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(W_STAR))
            .column(Column::exact(W_NAME).resizable(true))
            .column(Column::exact(W_DESC).resizable(true))
            .column(Column::exact(W_DATA).resizable(true))
            .column(Column::exact(W_OFV))
            .column(Column::exact(W_DOFV))
            .column(Column::exact(W_COV))
            .column(Column::exact(W_AIC))
            .column(Column::exact(W_CN))
            .column(Column::exact(W_METH))
            .column(Column::exact(W_INDOBS))
            .column(Column::exact(W_ETA))
            .column(Column::exact(W_EPS))
            .column(Column::exact(W_NPAR))
            .column(Column::exact(W_TIME))
            .column(Column::exact(W_FLAG))
            .header(22.0, |mut h| {
                for label in ["★","NAME","DESCRIPTION","DATA","OFV","ΔOFV","COV","AIC","CN",
                              "METHOD","IND/OBS","ETA%","EPS%","nPAR","TIME","⚠"] {
                    h.col(|ui| {
                        ui.label(egui::RichText::new(label).color(theme::fg2(dark)).size(11.0).strong());
                    });
                }
            })
            .body(|mut body| {
                for row in &rows {
                    let selected = new_selection == Some(row.idx);
                    body.row(24.0, |mut tr| {
                        tr.set_selected(selected);

                        // ★
                        tr.col(|ui| {
                            if row.starred {
                                if ui
                                    .add(
                                        egui::Label::new(
                                            egui::RichText::new("★").color(theme::STAR).size(14.0),
                                        )
                                        .sense(egui::Sense::click()),
                                    )
                                    .clicked()
                                {
                                    toggle_star = Some(row.idx);
                                }
                            } else if ui
                                .add(
                                    egui::Label::new(
                                        egui::RichText::new("☆").color(theme::fg3(dark)).size(14.0),
                                    )
                                    .sense(egui::Sense::click()),
                                )
                                .clicked()
                            {
                                toggle_star = Some(row.idx);
                            }
                        });

                        // NAME (coloured by run status, "(ref)" badge, context menu)
                        tr.col(|ui| {
                            let color = match row.run_status {
                                RunStatus::Converged  => theme::GREEN,
                                RunStatus::Failed     => theme::RED,
                                RunStatus::Stale      => theme::ORANGE,
                                RunStatus::ParseError => theme::RED,
                                // Not-run models use the secondary text colour so they
                                // don't compete visually with converged models.
                                RunStatus::NotRun     => theme::fg2(dark),
                            };
                            // Build label text, appending a "(ref)" marker when relevant.
                            let label_text = if row.is_reference {
                                format!("{} ◆", row.stem)
                            } else {
                                row.stem.clone()
                            };
                            let name_label = ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&label_text).color(color).size(12.0),
                                )
                                .sense(egui::Sense::click())
                                .truncate(),
                            );
                            let resp = if let Some(err) = &row.parse_error {
                                name_label.on_hover_text(format!(
                                    "Could not read this model's fit results — a .fitrx bundle \
                                     exists but ferxgui failed to parse it (likely an \
                                     incompatibility with the ferx version that produced it):\n\n{err}"
                                ))
                            } else {
                                name_label
                            };
                            if resp.clicked() {
                                new_selection = Some(row.idx);
                            }
                            if resp.double_clicked() {
                                new_selection = Some(row.idx);
                                switch_to_output = Some(row.idx);
                            }
                            // ── Right-click context menu ─────────────────────
                            let row_idx = row.idx;
                            let is_ref  = row.is_reference;
                            resp.context_menu(|ui| {
                                ui.set_min_width(190.0);

                                // ── Run ────────────────────────────────────
                                // A submenu, not a single click — lets
                                // per-run options (covariance, trace) be
                                // checked/flipped right here instead of
                                // silently firing off whatever the Run pill
                                // last had configured. These mutate the same
                                // global `state.ui.run_*` fields the Run
                                // pill itself edits, so changes here show up
                                // there too.
                                //
                                // Estimation method is deliberately NOT
                                // editable here: the model file's own
                                // `[fit_options]` is authoritative for this
                                // row (unlike covariance/trace, which aren't
                                // tied to a specific model the way method
                                // is). If the file doesn't declare one,
                                // "Run now" is disabled — nothing to run
                                // with otherwise.
                                //
                                // Plain "Run" — no manual "▶" — `menu_button`
                                // already appends its own submenu arrow.
                                ui.menu_button("Run", |ui| {
                                    ui.set_min_width(170.0);
                                    let declared_method = crate::io::ferx_file::parse_fit_options(
                                        &state.workspace.models[row_idx].model.source
                                    ).method;
                                    match &declared_method {
                                        Some(m) => {
                                            ui.label(egui::RichText::new(format!("Method: {m}"))
                                                .color(theme::fg2(dark)).size(11.0))
                                                .on_hover_text("Declared in this model's [fit_options] block");
                                        }
                                        None => {
                                            ui.label(egui::RichText::new(
                                                "No estimation method declared in [fit_options]")
                                                .color(theme::ORANGE).size(11.0));
                                        }
                                    }
                                    ui.checkbox(&mut state.ui.run_covariance, "Covariance step");
                                    ui.checkbox(&mut state.ui.run_optimizer_trace, "Optimizer trace");

                                    ui.separator();

                                    // `resolve_data_path_for_run`, not the raw
                                    // `run_data_path` — this row may never
                                    // have been the *selected* model this
                                    // session, so that global field may never
                                    // have been auto-populated for it even
                                    // though it has run (and has a data path)
                                    // before.
                                    let can_run = can_launch_run(
                                        state.run.active_run.is_some(),
                                        state.workspace.settings.ferx_binary.is_some(),
                                        resolve_data_path_for_run(state, &row.stem).is_some(),
                                    ) && declared_method.is_some();
                                    let run_resp = ui.add_enabled(can_run, egui::Button::new("Run now"));
                                    let run_resp = if !can_run {
                                        run_resp.on_disabled_hover_text(
                                            "A run is already active, ferx isn't configured, no \
                                             data file is selected, or this model has no \
                                             [fit_options] method declared"
                                        )
                                    } else {
                                        run_resp
                                    };
                                    if run_resp.clicked() {
                                        // Use the row's own declared method, not whatever
                                        // `state.ui.run_method` currently holds (which may
                                        // reflect a different model's last-edited value via
                                        // the Run pill).
                                        if let Some(m) = declared_method.clone() {
                                            state.ui.run_method = m;
                                        }
                                        ctx_action = Some((row_idx, CtxAction::Run));
                                        ui.close_menu();
                                    }
                                });

                                ui.separator();

                                // ── Model actions ─────────────────────────
                                if ui.button("Duplicate as child…").clicked() {
                                    ctx_action = Some((row_idx, CtxAction::Duplicate));
                                    ui.close_menu();
                                }
                                let ref_label = if is_ref { "Clear Reference" } else { "Set as Reference" };
                                if ui.button(ref_label).clicked() {
                                    ctx_action = Some((row_idx, CtxAction::ToggleReference));
                                    ui.close_menu();
                                }

                                // ── Compare with… (submenu) ───────────────
                                let other_models: Vec<(usize, String)> = state.workspace.models
                                    .iter().enumerate()
                                    .filter(|(i, m)| *i != row_idx && m.fit.is_some())
                                    .map(|(i, m)| (i, m.model.stem.clone()))
                                    .collect();
                                if !other_models.is_empty() {
                                    ui.menu_button("Compare with…  ▶", |ui| {
                                        ui.set_min_width(160.0);
                                        for (_i, stem) in &other_models {
                                            if ui.button(stem).clicked() {
                                                ctx_action = Some((row_idx, CtxAction::CompareWith(stem.clone())));
                                                ui.close_menu();
                                            }
                                        }
                                    });
                                }

                                ui.separator();

                                // Neither of these apply to a model that has never been
                                // run — greyed out rather than silently jumping to an
                                // empty History tab or a nonexistent log file.
                                let has_run = row.run_status != crate::domain::RunStatus::NotRun;

                                // ── File / folder ─────────────────────────
                                let log_resp = ui.add_enabled(has_run, egui::Button::new("View run log"));
                                let log_resp = if !has_run {
                                    log_resp.on_disabled_hover_text("This model has not run yet")
                                } else {
                                    log_resp
                                };
                                if log_resp.clicked() {
                                    ctx_action = Some((row_idx, CtxAction::ViewRunLog));
                                    ui.close_menu();
                                }

                                ui.separator();

                                // ── History ───────────────────────────────
                                let record_resp = ui.add_enabled(has_run, egui::Button::new("View run record…"));
                                let record_resp = if !has_run {
                                    record_resp.on_disabled_hover_text("This model has not run yet")
                                } else {
                                    record_resp
                                };
                                if record_resp.clicked() {
                                    ctx_action = Some((row_idx, CtxAction::ViewRunRecord));
                                    ui.close_menu();
                                }

                                ui.separator();

                                // ── Destructive ───────────────────────────
                                if ui.add(
                                    egui::Button::new(
                                        egui::RichText::new("Delete…").color(theme::RED),
                                    )
                                    .fill(egui::Color32::TRANSPARENT),
                                ).clicked() {
                                    ctx_action = Some((row_idx, CtxAction::Delete));
                                    ui.close_menu();
                                }
                            });
                        });

                        // DESCRIPTION
                        tr.col(|ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&row.description)
                                        .color(theme::fg2(dark))
                                        .size(12.0),
                                )
                                .truncate(),
                            );
                        });

                        // DATA — the model's own declared [data] path, blank if none.
                        tr.col(|ui| {
                            if let Some(data_file) = &row.data_file {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(data_file)
                                            .color(theme::fg2(dark))
                                            .size(12.0),
                                    )
                                    .truncate(),
                                );
                            }
                        });

                        // OFV
                        tr.col(|ui| {
                            ui.label(fmt_f64_4dp(row.ofv));
                        });

                        // ΔOFV
                        tr.col(|ui| {
                            let txt = fmt_f64_2dp(row.delta_ofv);
                            let color = if row.delta_ofv.is_nan() {
                                theme::fg3(dark)
                            } else if row.delta_ofv <= -3.84 {
                                theme::GREEN
                            } else if row.delta_ofv > 0.0 {
                                theme::RED
                            } else {
                                theme::fg(dark)
                            };
                            ui.label(egui::RichText::new(txt).color(color).size(12.0));
                        });

                        // COV
                        tr.col(|ui| {
                            match row.cov_ok {
                                Some(true) => {
                                    ui.label(
                                        egui::RichText::new("✔").color(theme::GREEN).size(13.0),
                                    );
                                }
                                Some(false) => {
                                    ui.label(
                                        egui::RichText::new("✖").color(theme::RED).size(13.0),
                                    );
                                }
                                None => {
                                    ui.label(egui::RichText::new("—").color(theme::fg3(dark)).size(12.0));
                                }
                            }
                        });

                        // AIC
                        tr.col(|ui| {
                            ui.label(fmt_f64_1dp(row.aic));
                        });

                        // CN (orange when > 1000)
                        tr.col(|ui| {
                            if row.cn.is_finite() && row.cn > 1000.0 {
                                ui.label(
                                    egui::RichText::new(format!("! {:.0}", row.cn))
                                        .color(theme::ORANGE)
                                        .size(12.0),
                                );
                            } else {
                                ui.label(fmt_f64_0dp(row.cn));
                            }
                        });

                        // METHOD
                        tr.col(|ui| {
                            ui.label(
                                egui::RichText::new(&row.method).color(theme::fg2(dark)).size(12.0),
                            );
                        });

                        // IND/OBS
                        tr.col(|ui| {
                            if row.n_subjects > 0 {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} / {}",
                                        row.n_subjects, row.n_obs
                                    ))
                                    .size(12.0),
                                );
                            }
                        });

                        // ETA shrinkage
                        tr.col(|ui| {
                            shrink_label(ui, row.max_eta_shrink);
                        });

                        // EPS shrinkage
                        tr.col(|ui| {
                            shrink_label(ui, row.eps_shrink);
                        });

                        // nPAR
                        tr.col(|ui| {
                            if row.n_parameters > 0 {
                                ui.label(
                                    egui::RichText::new(row.n_parameters.to_string()).size(12.0),
                                );
                            }
                        });

                        // TIME
                        tr.col(|ui| {
                            if row.wall_time_secs > 0.0 {
                                ui.label(
                                    egui::RichText::new(fmt_duration(row.wall_time_secs))
                                        .color(theme::fg2(dark))
                                        .size(11.0),
                                );
                            }
                        });

                        // ⚠ boundary flag
                        tr.col(|ui| {
                            if row.has_boundary {
                                ui.label(
                                    egui::RichText::new("⚠").color(theme::ORANGE).size(13.0),
                                )
                                .on_hover_text("Parameter(s) at lower bound");
                            }
                        });

                        // Row-level click — selects the model when the user clicks
                        // anywhere in the row that isn't captured by a child widget
                        // (i.e. every column except NAME and ★).
                        if tr.response().clicked() {
                            new_selection = Some(row.idx);
                        }
                    });
                }
            });
    });

    // Apply deferred mutations.
    if let Some(idx) = new_selection {
        state.ui.selected_model = Some(idx);
    }
    if let Some(idx) = switch_to_output {
        state.ui.selected_model = Some(idx);
        state.ui.active_model_pill = ModelPill::Output;
    }
    if let Some(idx) = toggle_star {
        state.workspace.models[idx].meta.starred ^= true;
        save_meta_for(state, idx);
    }
    if let Some((idx, action)) = ctx_action {
        apply_ctx_action(state, idx, action);
    }

    // Keyboard: Space = toggle star, Enter = switch to Output pill.
    // Guarded by focus: these are list-row shortcuts and must not fire while
    // another widget (e.g. the code editor's TextEdit) has keyboard focus —
    // otherwise typing Enter/Space into the editor would also yank focus away
    // from it via this global listener.
    if let Some(sel) = state.ui.selected_model {
        let space = ui.input(|i| i.key_pressed(egui::Key::Space));
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        let other_widget_focused = ui.ctx().memory(|m| m.focused().is_some());
        if space && should_apply_list_shortcut(other_widget_focused) {
            state.workspace.models[sel].meta.starred ^= true;
            save_meta_for(state, sel);
        }
        if enter && should_apply_list_shortcut(other_widget_focused) {
            state.ui.active_model_pill = ModelPill::Output;
        }
    }
}

/// Whether a model-list keyboard shortcut (Space/Enter) should act, given
/// whether some other widget currently holds keyboard focus.
fn should_apply_list_shortcut(other_widget_focused: bool) -> bool {
    !other_widget_focused
}

#[cfg(test)]
mod list_shortcut_tests {
    use super::{egui, should_apply_list_shortcut};
    use egui_kittest::Harness;

    #[test]
    fn fires_when_nothing_else_is_focused() {
        assert!(should_apply_list_shortcut(false));
    }

    #[test]
    fn suppressed_when_another_widget_is_focused() {
        assert!(!should_apply_list_shortcut(true));
    }

    /// End-to-end: typing into a real egui TextEdit gives it focus, and the
    /// production focus check (`ctx.memory(|m| m.focused().is_some())`) must
    /// report that focus is held — this is the actual mechanism the fix
    /// depends on, exercised against real egui rather than a mock.
    #[test]
    fn textedit_holds_focus_while_editing() {
        let mut buf = String::new();
        let mut harness = Harness::new_ui(move |ui| {
            let resp = ui.add(egui::TextEdit::multiline(&mut buf));
            resp.request_focus();
        });
        harness.run();

        let focused = harness.ctx.memory(|m| m.focused().is_some());
        assert!(focused, "TextEdit should hold keyboard focus after request_focus()");
        assert!(!should_apply_list_shortcut(focused));
    }
}

// ── Detail panel ─────────────────────────────────────────────────────────────

fn show_detail_panel(ui: &mut egui::Ui, state: &mut AppState) {
    if state.ui.selected_model.is_none() {
        let dim = if ui.visuals().dark_mode { theme::FG3 } else { egui::Color32::from_gray(160) };
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new("Select a model from the list").color(dim).size(14.0));
        });
        return;
    }

    // Tab bar. Deliberately not filled buttons: a tab strip should read as
    // part of the content pane it switches, not a row of separate buttons —
    // the active tab is marked by accent-colored text plus an underline
    // (flush with the separator below) rather than a solid background.
    let dark = ui.visuals().dark_mode;
    let inactive_fg = if dark { theme::FG2 } else { ui.visuals().text_color() };
    let hover_bg = if dark { theme::BG3 } else { egui::Color32::from_gray(230) };
    ui.horizontal(|ui| {
        for pill in ModelPill::ALL {
            let active = state.ui.active_model_pill == *pill;
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(70.0, 26.0),
                egui::Sense::click(),
            );
            // `theme::accent(dark)`, not the raw dark-only `theme::ACCENT` —
            // as text on the light theme's near-white panel, `ACCENT` itself
            // only measures ~3.1 contrast (below the 4.5 AA floor).
            let fg = if active {
                theme::accent(dark)
            } else if response.hovered() {
                theme::fg(dark)
            } else {
                inactive_fg
            };
            if response.hovered() && !active {
                ui.painter().rect_filled(rect, 4.0, hover_bg);
            }
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                pill.label(),
                egui::FontId::proportional(12.0),
                fg,
            );
            if active {
                let underline_y = rect.bottom() - 1.0;
                ui.painter().line_segment(
                    [egui::pos2(rect.left() + 4.0, underline_y), egui::pos2(rect.right() - 4.0, underline_y)],
                    egui::Stroke::new(2.0, theme::accent(dark)),
                );
            }
            if response.clicked() {
                state.ui.active_model_pill = *pill;
            }
        }
    });
    ui.separator();

    match state.ui.active_model_pill {
        ModelPill::Editor => show_editor_pill(ui, state),
        ModelPill::Run => show_run_pill(ui, state),
        ModelPill::Output => show_output_pill(ui, state),
        ModelPill::Parameters => show_params_pill(ui, state),
        ModelPill::Info   => show_info_pill(ui, state),
        ModelPill::Report => crate::ui::report::show(ui, state),
    }
}

// ── Editor pill ───────────────────────────────────────────────────────────────

fn show_editor_pill(ui: &mut egui::Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;
    let Some(idx) = state.ui.selected_model else { return };

    // Toolbar.
    ui.horizontal(|ui| {
        let stem = &state.workspace.models[idx].model.stem;
        ui.label(egui::RichText::new(stem).color(theme::fg2(dark)).size(12.0).monospace());
        if state.ui.editor_dirty {
            ui.label(egui::RichText::new("●").color(theme::ORANGE).size(12.0))
                .on_hover_text("Unsaved changes");
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if state.ui.editor_dirty {
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new("Discard").size(12.0))
                            .fill(theme::elevated_fill(dark)),
                    )
                    .clicked()
                {
                    // Reload from disk.
                    let source = state.workspace.models[idx].model.source.clone();
                    state.ui.editor_buffer = source;
                    state.ui.editor_dirty = false;
                }
                if ui
                    .add(
                        // Explicit BLACK text: the button's fill overrides the
                        // background to a fixed `theme::GREEN` regardless of
                        // theme, but text color was left to the theme default
                        // (near-white in dark mode) — near-white on this green
                        // measures ~1.6 contrast, reported as hard to read.
                        // BLACK-on-GREEN measures ~9.8, and stays correct in
                        // both themes since the button's own fill doesn't vary.
                        egui::Button::new(
                            egui::RichText::new("Save").size(12.0).color(egui::Color32::BLACK),
                        )
                        .fill(theme::GREEN),
                    )
                    .clicked()
                {
                    save_editor(state, idx);
                }
            }
        });
    });

    // Ctrl+S.
    if ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::S)) && state.ui.editor_dirty {
        save_editor(state, idx);
    }

    // Editor body: line numbers + text area.
    egui::ScrollArea::both()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                // Line number gutter.
                let line_count = state.ui.editor_buffer.lines().count().max(1);
                let gutter: String = (1..=line_count)
                    .map(|n| format!("{:>4}\n", n))
                    .collect();
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(gutter)
                            .font(egui::FontId::monospace(12.0))
                            .color(theme::fg3(dark)),
                    )
                    .wrap(),
                );

                // Thin separator.
                let sep_rect = ui.available_rect_before_wrap();
                ui.painter().line_segment(
                    [
                        egui::pos2(sep_rect.left(), sep_rect.top()),
                        egui::pos2(sep_rect.left(), sep_rect.bottom()),
                    ],
                    egui::Stroke::new(1.0, theme::BORDER),
                );
                ui.add_space(4.0);

                // Text editor with syntax highlighting.
                let mut layouter = editor_layouter(&mut state.ui.editor_layout_cache, dark);
                let buf = &mut state.ui.editor_buffer;
                let resp = ui.add(
                    egui::TextEdit::multiline(buf)
                        .font(egui::FontId::monospace(12.0))
                        .desired_rows(40)
                        .desired_width(f32::INFINITY)
                        .layouter(&mut layouter),
                );
                if resp.changed() {
                    state.ui.editor_dirty = true;
                }
            });
        });
}

/// Builds a `TextEdit` layouter that re-highlights only when the queried
/// text or theme differs from what's cached.
///
/// The cache key must be the `text` the layouter is *asked* about, not some
/// external buffer (e.g. `editor_buffer`): `TextEdit` calls the layouter
/// with text as of *this* frame's edit already applied, before any external
/// copy is guaranteed to match it. The returned galley must correspond
/// exactly to that `text` argument, or `TextEdit`'s cursor-placement math
/// (row/column derived from the galley) breaks — previously manifesting as
/// the cursor jumping to the wrong line while typing.
fn editor_layouter(
    cache: &mut Option<(String, bool, egui::text::LayoutJob)>,
    dark: bool,
) -> impl FnMut(&egui::Ui, &str, f32) -> std::sync::Arc<egui::Galley> + '_ {
    move |ui: &egui::Ui, text: &str, _wrap: f32| {
        let cache_valid = cache.as_ref().is_some_and(|(t, d, _)| t == text && *d == dark);
        if !cache_valid {
            let job = highlight_ferx(text, dark);
            *cache = Some((text.to_string(), dark, job));
        }
        let job = cache.as_ref().expect("populated above").2.clone();
        ui.fonts(|f| f.layout_job(job))
    }
}

#[cfg(test)]
mod editor_layouter_tests {
    use super::editor_layouter;
    use egui_kittest::Harness;

    /// Regression test for the root cause: `egui::TextEdit` calls its
    /// `layouter` twice per frame that changes the text — once with the
    /// pre-edit text, and again (after applying the keystroke internally)
    /// with the post-edit text — and trusts that the returned galley always
    /// matches whatever `text` argument it was just given, since it uses
    /// that galley to map cursor position to row/column. The original bug
    /// ignored the `text` argument and always served the last cached job,
    /// so the second call within an edit frame silently returned a galley
    /// for the *previous* text — which is exactly what broke cursor
    /// placement (symptom: cursor jumps to the wrong line while typing).
    ///
    /// This asserts the invariant directly: querying the layouter with two
    /// different strings back to back must yield two galleys whose text
    /// content actually matches what was asked, not a stale reuse.
    #[test]
    fn layouter_output_always_matches_the_queried_text() {
        let mut cache = None;
        let mut job_a_text = None;
        let mut job_b_text = None;
        let mut harness = Harness::new_ui(|ui| {
            let mut layouter = editor_layouter(&mut cache, false);
            job_a_text = Some(layouter(ui, "line1", 200.0).job.text.clone());
            // Different text, queried immediately after — the exact
            // situation `TextEdit` creates within a single edit frame.
            job_b_text = Some(layouter(ui, "line1\nline2", 200.0).job.text.clone());
        });
        harness.run();
        drop(harness); // release the closure's borrows of job_a_text/job_b_text/cache

        // `highlight_ferx` appends a trailing "\n" to its job text (pre-existing
        // behaviour, unrelated to this fix) — compare against its actual output
        // rather than the raw query string.
        assert_eq!(job_a_text, Some(super::highlight_ferx("line1", false).text.clone()));
        assert_eq!(
            job_b_text,
            Some(super::highlight_ferx("line1\nline2", false).text.clone()),
            "layouter must return a galley matching the text it was just queried with, \
             not a stale cached job left over from a different (earlier) query"
        );
        assert_ne!(
            job_a_text, job_b_text,
            "sanity check: the two queries must actually produce different job text"
        );
    }

    /// Sanity check on the caching behaviour: querying the *same* text twice
    /// should reuse the cached job rather than recomputing (the whole point
    /// of caching), which we can observe via the cache's stored text staying
    /// equal to what was asked, across repeated identical queries.
    #[test]
    fn identical_repeated_queries_stay_cached() {
        let mut cache = None;
        let mut harness = Harness::new_ui(|ui| {
            let mut layouter = editor_layouter(&mut cache, false);
            let _ = layouter(ui, "same text", 200.0);
            let _ = layouter(ui, "same text", 200.0);
        });
        harness.run();
        drop(harness); // release the closure's borrow of cache

        let (cached_text, _dark, _job) = cache.expect("cache populated after queries");
        assert_eq!(cached_text, "same text");
    }
}

#[cfg(test)]
mod contrast_tests {
    use crate::app::theme;
    use eframe::egui::Color32;

    // WCAG 2.x relative luminance + contrast ratio (see also app.rs's
    // theme_contrast_tests, which duplicates this small pure-math helper
    // rather than sharing it across a two-test-module boundary).
    fn relative_luminance(c: Color32) -> f64 {
        let to_linear = |ch: u8| {
            let c = ch as f64 / 255.0;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * to_linear(c.r()) + 0.7152 * to_linear(c.g()) + 0.0722 * to_linear(c.b())
    }

    fn contrast_ratio(a: Color32, b: Color32) -> f64 {
        let (la, lb) = (relative_luminance(a), relative_luminance(b));
        let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// Regression test for the model-pill tab-strip restyle: the active tab's
    /// text/underline color (`theme::accent(dark)`) is painted directly on
    /// the surrounding `CentralPanel` background (no button fill anymore —
    /// see `show_detail_panel`), so it must independently clear AA contrast
    /// against that panel background in both themes. Using the dark-only
    /// `theme::ACCENT` for this in light mode was the bug (measured ~3.1).
    #[test]
    fn active_tab_accent_text_meets_aa_contrast_on_panel_bg_in_both_themes() {
        let panel_dark  = Color32::from_rgb(0x1a, 0x1a, 0x20); // theme::BG
        let panel_light = Color32::from_gray(248);             // CentralPanel fill, light theme

        let dark_ratio = contrast_ratio(theme::accent(true), panel_dark);
        assert!(
            dark_ratio >= 4.5,
            "dark-theme active-tab text/panel contrast is {dark_ratio:.2}, below AA floor of 4.5"
        );

        let light_ratio = contrast_ratio(theme::accent(false), panel_light);
        assert!(
            light_ratio >= 4.5,
            "light-theme active-tab text/panel contrast is {light_ratio:.2}, below AA floor of 4.5"
        );
    }

    /// Regression test for "warnings in output are hard to read, make orange
    /// on black": the warning card's fill is fixed `Color32::BLACK` in both
    /// themes (see `show_output_pill`'s "Warnings" section), replacing a
    /// translucent orange-over-panel wash that measured ~2.1 contrast in
    /// light mode.
    #[test]
    fn warning_card_text_meets_aa_contrast_against_its_fixed_black_fill() {
        let ratio = contrast_ratio(theme::ORANGE, Color32::BLACK);
        assert!(
            ratio >= 4.5,
            "warning card text/fill contrast is {ratio:.2}, below AA floor of 4.5"
        );
    }

    /// Regression test for "Save button is hard to read (green on white)":
    /// the button's fill is the fixed `theme::GREEN` in both themes (see
    /// `show_editor_pill`), so its text color must be chosen to contrast
    /// against that fixed green regardless of the active theme — not left
    /// to the theme's default button text color (which is near-white in
    /// dark mode, measuring ~1.6 contrast against this green).
    #[test]
    fn save_button_text_meets_aa_contrast_against_its_fixed_green_fill() {
        let text_color = Color32::BLACK; // must match the `.color(...)` used in show_editor_pill
        let ratio = contrast_ratio(text_color, theme::GREEN);
        assert!(
            ratio >= 4.5,
            "Save button text/fill contrast is {ratio:.2}, below AA floor of 4.5"
        );
    }
}

fn save_editor(state: &mut AppState, idx: usize) {
    let path = state.workspace.models[idx].model.path.clone();
    let buf = state.ui.editor_buffer.clone();
    if std::fs::write(&path, &buf).is_ok() {
        state.workspace.models[idx].model.source = buf;
        state.ui.editor_dirty = false;
        state.ui.status_message = format!("Saved {}", path.display());
    } else {
        state.ui.status_message = format!("Save failed: {}", path.display());
    }
}

// ── Run pill ──────────────────────────────────────────────────────────────────

fn show_run_pill(ui: &mut egui::Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;
    let Some(idx) = state.ui.selected_model else { return };
    let stem = state.workspace.models[idx].model.stem.clone();

    // Auto-populate the data path from the most recent run for this model
    // so the Run button is ready immediately after restart.
    if state.ui.run_data_path.is_none() {
        state.ui.run_data_path = state.run.run_history
            .iter()
            .rev()
            .find(|r| {
                r.model_stem == stem
                    && r.data_path.as_ref().is_some_and(|p| p.exists())
            })
            .and_then(|r| r.data_path.clone());
    }

    // Estimation method is not user-editable here — the model's own
    // [fit_options] is authoritative. Parsed fresh from the current source
    // (not just on model switch), so editing the file and coming back to
    // this tab always reflects the latest declared method rather than a
    // stale value left over from before an edit.
    let declared_method = crate::io::ferx_file::parse_fit_options(
        &state.workspace.models[idx].model.source
    ).method;
    if let Some(m) = &declared_method {
        state.ui.run_method = m.clone();
    }

    let running = state.run.active_run.is_some();

    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            // ── Configuration ──
            egui::Frame::new()
                .fill(theme::card_fill(dark))
                .inner_margin(egui::Margin::same(10))
                .corner_radius(egui::CornerRadius::same(6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());

                    ui.label(
                        egui::RichText::new("Run configuration").color(theme::fg2(dark)).size(11.0),
                    );
                    ui.add_space(6.0);

                    // Data file.
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Data file:").color(theme::fg2(dark)).size(12.0));
                        let data_str = state
                            .ui
                            .run_data_path
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default();
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(if data_str.is_empty() {
                                    "— not set —"
                                } else {
                                    &data_str
                                })
                                .color(if data_str.is_empty() { theme::fg3(dark) } else { theme::fg(dark) })
                                .size(12.0)
                                .monospace(),
                            )
                            .truncate(),
                        );
                        if ui.small_button("Browse…").clicked() {
                            if let Some(p) = rfd::FileDialog::new()
                                .add_filter("CSV / NONMEM", &["csv", "txt"])
                                .pick_file()
                            {
                                state.ui.run_data_path = Some(p);
                            }
                        }
                    });

                    // Method: read-only — declared in the model's own
                    // [fit_options], not editable here (see `declared_method`
                    // above). A warning if the file doesn't declare one.
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Method:").color(theme::fg2(dark)).size(12.0));
                        match &declared_method {
                            Some(m) => {
                                ui.label(egui::RichText::new(m).color(theme::fg(dark)).size(12.0).monospace())
                                    .on_hover_text("Declared in this model's [fit_options] block");
                            }
                            None => {
                                ui.label(egui::RichText::new("No method declared in [fit_options]")
                                    .color(theme::ORANGE).size(12.0));
                            }
                        }
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("Gradient:").color(theme::fg2(dark)).size(12.0));
                        egui::ComboBox::from_id_salt("run_gradient_combo")
                            .selected_text(&state.ui.run_gradient)
                            .width(70.0)
                            .show_ui(ui, |ui| {
                                for g in ["auto", "ad", "fd"] {
                                    ui.selectable_value(
                                        &mut state.ui.run_gradient, g.to_string(), g);
                                }
                            });
                    });

                    // Covariance + threads + optimizer trace.
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut state.ui.run_covariance, "Covariance step");
                        ui.add_space(12.0);
                        ui.label(egui::RichText::new("Threads:").color(theme::fg2(dark)).size(12.0));
                        let suffix = if state.ui.run_threads == 0 { " (auto)" } else { "" };
                        ui.add(
                            egui::DragValue::new(&mut state.ui.run_threads)
                                .range(0..=64)
                                .suffix(suffix),
                        );
                        ui.add_space(12.0);
                        ui.checkbox(&mut state.ui.run_optimizer_trace, "Optimizer trace")
                            .on_hover_text(
                                "Write convergence trace CSV — enables the Convergence tab \
                                 in the Evaluation view",
                            );
                    });
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut state.ui.run_export_tables, "Save output tables")
                            .on_hover_text(
                                "After run: write {stem}_sdtab.csv (predictions) and \
                                 {stem}_patab.csv (EBEs) next to the model — \
                                 equivalent to NONMEM's sdtab/patab",
                            );
                    });

                    // ── Post-fit actions ──────────────────────────────────
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new("Post-fit actions")
                            .color(theme::fg2(dark))
                            .size(11.0),
                    );
                    ui.add_space(4.0);
                    let cov_on = state.ui.run_covariance;
                    ui.add_enabled_ui(cov_on, |ui| {
                        ui.checkbox(&mut state.ui.run_sir_after_fit, "Run SIR after fit")
                            .on_hover_text(if cov_on {
                                "Sampling Importance Resampling — non-parametric uncertainty \
                                 intervals after fitting. Uses the settings on the SIR tab. \
                                 A progress popup appears while SIR is running."
                            } else {
                                "Requires the covariance step — tick Covariance step above."
                            });
                    });
                    if state.ui.run_sir_after_fit && cov_on {
                        ui.label(
                            egui::RichText::new(format!(
                                "  {} samples · {} resamples · seed {}",
                                state.ui.sir_n_samples,
                                state.ui.sir_n_resamples,
                                state.ui.sir_seed,
                            ))
                            .color(theme::fg3(dark))
                            .size(10.0),
                        );
                        if ui.add(
                            egui::Label::new(
                                egui::RichText::new("  Edit SIR settings →")
                                    .color(theme::ACCENT)
                                    .size(10.0),
                            )
                            .sense(egui::Sense::click()),
                        ).clicked() {
                            state.ui.active_tab = crate::state::Tab::Uncertainty;
                        }
                    }

                    // Settings passthrough.
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Settings (JSON):").color(theme::fg2(dark)).size(12.0));
                        ui.add(
                            egui::TextEdit::singleline(&mut state.ui.run_extra_args)
                                .desired_width(f32::INFINITY)
                                .hint_text(r#"e.g. {"optimizer":"bobyqa"}"#),
                        );
                    });
                });

            ui.add_space(8.0);

            // ── Action buttons ──
            ui.horizontal(|ui| {
                let can_run = can_launch_run(
                    running,
                    state.workspace.settings.ferx_binary.is_some(),
                    state.ui.run_data_path.is_some(),
                ) && declared_method.is_some();
                let can_queue = state.workspace.settings.ferx_binary.is_some()
                    && state.ui.run_data_path.is_some()
                    && declared_method.is_some();

                if ui
                    .add_enabled(
                        can_run,
                        egui::Button::new(egui::RichText::new("▶  Run").size(13.0))
                            .fill(theme::ACCENT)
                            .min_size(egui::vec2(90.0, 28.0)),
                    )
                    .clicked()
                {
                    launch_run(state, idx, &stem);
                }

                if ui
                    .add_enabled(
                        can_queue,
                        egui::Button::new(egui::RichText::new("+ Queue").size(13.0))
                            .fill(theme::elevated_fill(dark))
                            .min_size(egui::vec2(80.0, 28.0)),
                    )
                    .on_hover_text("Add to the sequential run queue — starts automatically when the current run finishes")
                    .clicked()
                {
                    let already_queued = state.run.run_queue.iter().any(|q| q.stem == stem);
                    if already_queued {
                        state.ui.status_message = format!("{stem} is already in the queue");
                    } else if let Some(data_path) = state.ui.run_data_path.clone() {
                        let model_path = state.workspace.models[idx].model.path.clone();
                        state.run.run_queue.push_back(crate::domain::QueuedRun {
                            stem: stem.clone(),
                            model_path,
                            data_path,
                            method: state.ui.run_method.clone(),
                            covariance: state.ui.run_covariance,
                            gradient: state.ui.run_gradient.clone(),
                            settings: state.ui.run_extra_args.clone(),
                            threads: state.ui.run_threads,
                            optimizer_trace: state.ui.run_optimizer_trace,
                            export_tables: state.ui.run_export_tables,
                            run_sir_after: state.ui.run_sir_after_fit,
                        });
                        let n = state.run.run_queue.len();
                        state.ui.status_message = format!("Queued {stem} ({n} in queue)");
                    }
                }

                if running {
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new("■  Stop").size(13.0))
                                .fill(theme::card_fill(dark))
                                .min_size(egui::vec2(80.0, 28.0)),
                        )
                        .on_hover_text(if cfg!(unix) { "Graceful stop (SIGTERM → kill after 5 s)" } else { "Graceful stop (CTRL_BREAK → kill after 5 s)" })
                        .clicked()
                    {
                        if let Some(run) = &state.run.active_run {
                            let _ = run.cancel_tx.send(CancelMode::Graceful);
                        }
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("✕  Kill").size(13.0).color(theme::RED),
                            )
                            .stroke(egui::Stroke::new(1.0, theme::RED))
                            .fill(theme::card_fill(dark))
                            .min_size(egui::vec2(76.0, 28.0)),
                        )
                        .on_hover_text("Kill immediately (SIGKILL)")
                        .clicked()
                    {
                        if let Some(run) = &state.run.active_run {
                            let _ = run.cancel_tx.send(CancelMode::Kill);
                        }
                    }
                }

                // Hint when prerequisites are missing.
                if state.workspace.settings.ferx_binary.is_none() {
                    ui.label(
                        egui::RichText::new("ferx binary not found — set path in Settings")
                            .color(theme::ORANGE)
                            .size(11.0),
                    );
                } else if state.ui.run_data_path.is_none() {
                    ui.label(
                        egui::RichText::new("Select a data file above")
                            .color(theme::fg3(dark))
                            .size(11.0),
                    );
                }
            });

            // ── Init check ──
            {
                let checking = state.workspace.check_init_running.contains(&stem);
                let can_check = state.workspace.settings.ferx_binary.is_some()
                    && state.ui.run_data_path.is_some()
                    && !checking;
                let model_path  = state.workspace.models[idx].model.path.clone();
                let data_path   = state.ui.run_data_path.clone();
                let dark        = ui.visuals().dark_mode;
                let dim         = if dark { theme::FG2 } else { egui::Color32::from_gray(100) };

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            can_check,
                            egui::Button::new(
                                egui::RichText::new("⚡ Check inits").size(12.0),
                            )
                            .fill(theme::elevated_fill(dark))
                            .min_size(egui::vec2(110.0, 24.0)),
                        )
                        .on_hover_text(
                            "Run 5 FOCEI iterations to verify initial estimates are healthy \
                             before committing to a full fit",
                        )
                        .clicked()
                    {
                        if let Some(dp) = data_path {
                            // Clear any previous result/error for this stem.
                            state.workspace.check_init_results.remove(&stem);
                            state.workspace.check_init_error.remove(&stem);
                            state.workspace.check_init_running.insert(stem.clone());
                            let tx      = state.worker_tx.clone();
                            let ctx     = ui.ctx().clone();
                            let stem_cl = stem.clone();
                            std::thread::spawn(move || {
                                match crate::io::r_extract::compute_check_init(&model_path, &dp) {
                                    Ok(result) => {
                                        let _ = tx.send(crate::workers::messages::WorkerMsg::RCheckInitComplete {
                                            stem: stem_cl,
                                            result: Box::new(result),
                                        });
                                    }
                                    Err(e) => {
                                        let _ = tx.send(crate::workers::messages::WorkerMsg::RTaskError {
                                            context: format!("check_init {stem_cl}"),
                                            message: e,
                                        });
                                    }
                                }
                                ctx.request_repaint();
                            });
                        }
                    }

                    if checking {
                        ui.add_space(8.0);
                        ui.spinner();
                        ui.label(
                            egui::RichText::new("Running 5 iterations…")
                                .color(dim)
                                .size(11.0),
                        );
                    }
                });

                // Error card — a failed check_init previously only surfaced
                // via the tiny status-bar line (reported: "spinner appears
                // then disappears, nothing else"). Shown with the same
                // visual weight as the result card below so it can't be missed.
                if let Some(err) = state.workspace.check_init_error.get(&stem).cloned() {
                    ui.add_space(6.0);
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgba_unmultiplied(0xe8, 0x55, 0x55, 20))
                        .inner_margin(egui::Margin::same(8))
                        .corner_radius(egui::CornerRadius::same(5))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                egui::RichText::new("Check inits failed")
                                    .color(theme::RED)
                                    .size(13.0)
                                    .strong(),
                            );
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(&err).color(theme::RED).size(11.0));
                        });
                }

                // Result card.
                if let Some(ci) = state.workspace.check_init_results.get(&stem).cloned() {
                    ui.add_space(6.0);
                    let (card_fill, status_color, status_icon, status_msg) =
                        if !ci.start_finite() {
                            (
                                egui::Color32::from_rgba_unmultiplied(0xe8, 0x55, 0x55, 20),
                                theme::RED,
                                "✖",
                                "Non-finite start OFV — check model structure or data",
                            )
                        } else if ci.dropping() {
                            (
                                egui::Color32::from_rgba_unmultiplied(0x3e, 0xc9, 0x7a, 20),
                                theme::GREEN,
                                "✔",
                                "Gradient pointing down — inits look healthy",
                            )
                        } else {
                            (
                                egui::Color32::from_rgba_unmultiplied(0xe8, 0x95, 0x40, 20),
                                theme::ORANGE,
                                "⚠",
                                "OFV not decreasing — consider tightening bounds or rescaling",
                            )
                        };

                    egui::Frame::new()
                        .fill(card_fill)
                        .inner_margin(egui::Margin::same(8))
                        .corner_radius(egui::CornerRadius::same(5))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(status_icon)
                                        .color(status_color)
                                        .size(13.0)
                                        .strong(),
                                );
                                ui.label(
                                    egui::RichText::new(status_msg)
                                        .color(status_color)
                                        .size(11.0),
                                );
                            });
                            ui.add_space(4.0);
                            egui::Grid::new("check_init_grid")
                                .num_columns(4)
                                .spacing([16.0, 2.0])
                                .show(ui, |ui| {
                                    let fmt = |v: Option<f64>| {
                                        v.map(|x| format!("{x:.2}"))
                                            .unwrap_or_else(|| "—".to_string())
                                    };
                                    ui.label(egui::RichText::new("Start OFV").color(dim).size(11.0));
                                    ui.label(egui::RichText::new(fmt(ci.ofv_start)).size(11.0).monospace());
                                    ui.label(egui::RichText::new("End OFV").color(dim).size(11.0));
                                    ui.label(egui::RichText::new(fmt(ci.ofv_end)).size(11.0).monospace());
                                    ui.end_row();

                                    let drop_str = ci.ofv_drop
                                        .map(|d| format!("{:+.2}", d))
                                        .unwrap_or_else(|| "—".to_string());
                                    let drop_color = if ci.dropping() { theme::GREEN } else { theme::ORANGE };
                                    ui.label(egui::RichText::new("OFV drop").color(dim).size(11.0));
                                    ui.label(
                                        egui::RichText::new(drop_str)
                                            .size(11.0)
                                            .color(drop_color)
                                            .monospace(),
                                    );
                                    ui.label(egui::RichText::new("Iterations").color(dim).size(11.0));
                                    ui.label(
                                        egui::RichText::new(ci.n_iter.to_string())
                                            .size(11.0)
                                            .monospace(),
                                    );
                                    ui.end_row();
                                });
                        });
                }
            }

            // ── Queue list ──
            if !state.run.run_queue.is_empty() {
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                let dark = ui.visuals().dark_mode;
                let dim = if dark { theme::FG2 } else { egui::Color32::from_gray(100) };
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(
                            format!("Queue  ({} pending)", state.run.run_queue.len()),
                        )
                        .color(dim)
                        .size(11.0)
                        .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new("Clear all").size(11.0))
                                    .fill(theme::elevated_fill(dark))
                                    .min_size(egui::vec2(60.0, 18.0)),
                            )
                            .clicked()
                        {
                            state.run.run_queue.clear();
                        }
                    });
                });
                let mut remove_idx: Option<usize> = None;
                let queue_snapshot: Vec<(usize, String, String)> = state.run.run_queue
                    .iter()
                    .enumerate()
                    .map(|(i, q)| (i, q.stem.clone(), q.method.clone()))
                    .collect();
                for (i, stem_q, method_q) in &queue_snapshot {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("{}.", i + 1))
                                .color(dim)
                                .size(11.0)
                                .monospace(),
                        );
                        ui.label(
                            egui::RichText::new(stem_q)
                                .color(if dark { theme::FG } else { egui::Color32::from_gray(30) })
                                .size(12.0),
                        );
                        ui.label(
                            egui::RichText::new(format!("— {method_q}"))
                                .color(dim)
                                .size(11.0),
                        );
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("✕").size(10.0).color(theme::fg3(dark)),
                                )
                                .frame(false),
                            )
                            .on_hover_text("Remove from queue")
                            .clicked()
                        {
                            remove_idx = Some(*i);
                        }
                    });
                }
                if let Some(i) = remove_idx {
                    state.run.run_queue.remove(i);
                }
            }

            ui.add_space(8.0);

            // Output lives in the bottom run panel (visible across all tabs).
            let note_fg = if ui.visuals().dark_mode { theme::FG3 } else { egui::Color32::from_gray(150) };
            ui.label(
                egui::RichText::new("▼  Live output appears in the run panel at the bottom of the window")
                    .color(note_fg)
                    .size(11.0),
            );
        });
}

/// Whether a run can be launched right now — shared by the Run pill's button
/// and the model list's right-click "Run" item, so the two can't drift apart
/// (e.g. one allowing a second concurrent run that the other would block).
fn can_launch_run(running: bool, has_ferx_binary: bool, has_data_path: bool) -> bool {
    !running && has_ferx_binary && has_data_path
}

#[cfg(test)]
mod can_launch_run_tests {
    use super::can_launch_run;

    #[test]
    fn requires_not_already_running_and_binary_and_data_path() {
        assert!(can_launch_run(false, true, true));
        assert!(!can_launch_run(true, true, true), "must not allow launching while a run is active");
        assert!(!can_launch_run(false, false, true), "must not allow launching without ferx configured");
        assert!(!can_launch_run(false, true, false), "must not allow launching without a data path");
    }
}

/// Resolves the data path that `stem` would run with right now, without
/// mutating any state — either the already-configured global
/// `run_data_path` (used regardless of which model is selected, matching
/// `launch_run`'s own read of it), or that model's own most recent run
/// history entry as a fallback.
///
/// Needed because `show_run_pill`'s auto-populate ([models_tab.rs:1205])
/// only runs for whichever model is currently *selected* — a context-menu
/// "Run" targeting a different, never-selected row would otherwise see
/// `run_data_path` as unset and show as disabled even though that row has
/// run (and has a data path) before.
fn resolve_data_path_for_run(state: &AppState, stem: &str) -> Option<std::path::PathBuf> {
    state.ui.run_data_path.clone().or_else(|| {
        state.run.run_history
            .iter()
            .rev()
            .find(|r| r.model_stem == stem && r.data_path.as_ref().is_some_and(|p| p.exists()))
            .and_then(|r| r.data_path.clone())
    })
}

#[cfg(test)]
mod resolve_data_path_for_run_tests {
    use super::resolve_data_path_for_run;
    use crate::domain::{JobStatus, RunRecord};
    use crate::state::AppState;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn run_record(stem: &str, data_path: Option<PathBuf>) -> RunRecord {
        RunRecord {
            id: format!("{stem}-1"),
            model_stem: stem.to_string(),
            tool: "ferx".to_string(),
            method: None,
            status: JobStatus::Completed,
            started: String::new(),
            completed: None,
            duration_secs: None,
            command: String::new(),
            directory: PathBuf::new(),
            data_path,
            file_hashes: HashMap::new(),
        }
    }

    #[test]
    fn prefers_the_already_configured_global_path() {
        let mut state = AppState::new();
        state.ui.run_data_path = Some(PathBuf::from("/tmp/current.csv"));
        state.run.run_history.push(run_record("model_a", Some(PathBuf::from("/tmp/other.csv"))));
        assert_eq!(
            resolve_data_path_for_run(&state, "model_a"),
            Some(PathBuf::from("/tmp/current.csv"))
        );
    }

    #[test]
    fn falls_back_to_the_stems_own_run_history_when_global_path_unset() {
        // This is the reported bug: right-clicking a model that was never the
        // *selected* one this session (so run_data_path was never
        // auto-populated for it) must still resolve its own past data path,
        // not show as permanently disabled.
        let mut state = AppState::new();
        state.ui.run_data_path = None;
        state.run.run_history.push(run_record("model_a", Some(std::env::temp_dir())));
        assert_eq!(
            resolve_data_path_for_run(&state, "model_a"),
            Some(std::env::temp_dir())
        );
    }

    #[test]
    fn none_when_neither_global_nor_history_has_a_path() {
        let state = AppState::new();
        assert_eq!(resolve_data_path_for_run(&state, "model_a"), None);
    }
}

fn launch_run(state: &mut AppState, idx: usize, stem: &str) {
    let model_path = state.workspace.models[idx].model.path.clone();
    let data = match state.ui.run_data_path.clone() {
        Some(p) => p,
        None => {
            state.ui.status_message = "Select a data file before running".to_string();
            return;
        }
    };
    do_launch_queued(state, crate::domain::QueuedRun {
        stem: stem.to_string(),
        model_path,
        data_path: data,
        method: state.ui.run_method.clone(),
        covariance: state.ui.run_covariance,
        gradient: state.ui.run_gradient.clone(),
        settings: state.ui.run_extra_args.clone(),
        threads: state.ui.run_threads,
        optimizer_trace: state.ui.run_optimizer_trace,
        export_tables: state.ui.run_export_tables,
        run_sir_after: state.ui.run_sir_after_fit,
    });
}

/// Pop the next item from the run queue and start it.  No-op when idle queue is empty
/// or another run is already active.  Called every frame by the app update loop.
pub fn advance_queue(state: &mut AppState) {
    if state.run.active_run.is_some() { return; }
    let Some(queued) = state.run.run_queue.pop_front() else { return };
    do_launch_queued(state, queued);
}

/// Core launch logic: start a run described by `queued`.  Sets `active_run` on success;
/// writes an error to `status_message` on failure.
pub fn do_launch_queued(state: &mut AppState, queued: crate::domain::QueuedRun) {
    // ferx runs through R: the stored "binary" is actually the Rscript path.
    let rscript = match state.workspace.settings.ferx_binary.clone() {
        Some(p) => p,
        None => {
            state.ui.status_message =
                "ferx is not available — check R + the ferx package in Settings".to_string();
            return;
        }
    };

    let cwd = queued.model_path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .to_path_buf();

    // Ensure the embedded R run script exists on disk (in ~/.ferxgui/).
    let run_script = match state.workspace.app_dir.as_deref() {
        Some(d) => match crate::io::r_extract::ensure_run_script(d) {
            Ok(p) => p,
            Err(e) => {
                state.ui.status_message = format!("Could not write run script: {e}");
                return;
            }
        },
        None => {
            state.ui.status_message = "No app data directory available".to_string();
            return;
        }
    };

    // Output bundle next to the model so the scanner pairs it with the model.
    let out_fitrx = cwd.join(format!("{}.fitrx", queued.stem));

    // Rscript --vanilla run_ferx.R <model> <data> <method> <covariance> <out.fitrx>
    //   [gradient] [settings_json] [threads] [optimizer_trace]
    let threads_str = if queued.threads == 0 { String::new() } else { queued.threads.to_string() };
    let args = vec![
        "--vanilla".to_string(),
        run_script.to_string_lossy().to_string(),
        queued.model_path.to_string_lossy().to_string(),
        queued.data_path.to_string_lossy().to_string(),
        queued.method.clone(),
        queued.covariance.to_string(),
        out_fitrx.to_string_lossy().to_string(),
        queued.gradient.clone(),
        queued.settings.clone(),
        threads_str,
        queued.optimizer_trace.to_string(),
    ];

    let run_id = format!("{}-{}", queued.stem, now_unix());

    // Log file: next to the model so it's easy to find in the file browser.
    let log_path = cwd.join(format!("{}_run.log", queued.stem));

    // Manifest: in ~/.ferxgui/running/ (centralised for startup reconnect scan).
    let mfst_path = state.workspace.app_dir
        .as_deref()
        .and_then(|d| manifest_path(d, &run_id))
        .unwrap_or_else(|| cwd.join(format!("{run_id}.runmfst")));

    // Ensure the running/ dir exists (manifest_path() does this, but fallback may not).
    if let Some(app_dir) = &state.workspace.app_dir {
        let _ = running_dir(app_dir);
    }

    let record = RunRecord {
        id: run_id,
        model_stem: queued.stem.clone(),
        tool: "ferx".to_string(),
        method: Some(queued.method.clone()),
        status: JobStatus::Running,
        started: now_iso(),
        completed: None,
        duration_secs: None,
        command: format!("\"{}\" {}", rscript.display(), args.join(" ")),
        directory: cwd.clone(),
        data_path: Some(queued.data_path),
        file_hashes: HashMap::new(),
    };

    let (cancel_tx, cancel_rx) = std::sync::mpsc::channel::<CancelMode>();
    let tx = state.worker_tx.clone();

    match spawn_detached_run(
        record.clone(),
        rscript,
        args,
        cwd,
        log_path.clone(),
        mfst_path.clone(),
        tx,
        cancel_rx,
    ) {
        Ok(spawned) => {
            state.run.active_run = Some(crate::domain::ActiveRun {
                record,
                started_at: std::time::Instant::now(),
                log_path: spawned.log_path,
                cancel_tx,
                export_tables: queued.export_tables,
                run_sir_after: queued.run_sir_after,
            });
            // `log_text` is a separately-maintained pre-joined cache of
            // `log_buffer` (see its doc comment in state.rs) — it must be
            // cleared alongside the buffer, or the Run popup keeps showing
            // the previous run's full log until enough new lines arrive to
            // incrementally push it out (reported: "popup shows history of
            // previous runs" on a fresh run).
            state.run.log_buffer.clear();
            state.run.log_text.clear();
            state.ui.status_message = format!("Running {}", queued.stem);
        }
        Err(e) => {
            state.ui.status_message = format!("Failed to start run: {e}");
        }
    }
}

// ── Output pill ───────────────────────────────────────────────────────────────

fn show_output_pill(ui: &mut egui::Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;
    let Some(idx) = state.ui.selected_model else { return };

    let fit = match &state.workspace.models[idx].fit {
        Some(f) => f.clone(),
        None => {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    if let Some(err) = &state.workspace.models[idx].fit_parse_error {
                        ui.label(egui::RichText::new("Could not read fit results")
                            .strong().color(theme::fg2(dark)).size(13.0));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(format!(
                            "A .fitrx bundle exists but ferxgui failed to parse it — likely an \
                             incompatibility with the ferx version that produced it.\n\n{err}"
                        )).color(theme::fg3(dark)).size(11.0));
                    } else {
                        ui.label(
                            egui::RichText::new("No run output yet\nRun the model from the Run tab")
                                .color(theme::fg3(dark))
                                .size(13.0),
                        );
                    }
                });
            });
            return;
        }
    };

    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            // ── Summary grid ──
            egui::Frame::new()
                .fill(theme::card_fill(dark))
                .inner_margin(egui::Margin::same(12))
                .corner_radius(egui::CornerRadius::same(6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::Grid::new("output_summary")
                        .num_columns(4)
                        .spacing([24.0, 6.0])
                        .show(ui, |ui| {
                            // Row 1
                            kv(ui, "Status", if fit.converged { "✔ Converged" } else { "✖ Not converged" },
                               if fit.converged { theme::GREEN } else { theme::RED });
                            kv(ui, "Method", &fit.method.to_uppercase(), theme::fg(dark));
                            ui.end_row();

                            // Row 2
                            kv(ui, "OFV", &fmt_f64_4dp(fit.ofv), theme::fg(dark));
                            kv(ui, "AIC", &fmt_f64_2dp(fit.aic), theme::fg(dark));
                            kv(ui, "BIC", &fmt_f64_2dp(fit.bic), theme::fg(dark));
                            kv(ui, "nPar", &fit.n_parameters.to_string(), theme::fg(dark));
                            ui.end_row();

                            // Row 3
                            kv(ui, "Subjects", &fit.n_subjects.to_string(), theme::fg(dark));
                            kv(ui, "Obs", &fit.n_obs.to_string(), theme::fg(dark));
                            kv(ui, "Iterations", &fit.n_iterations.to_string(), theme::fg(dark));
                            kv(ui, "Time", &fmt_duration(fit.wall_time_secs), theme::fg2(dark));
                            ui.end_row();

                            // Row 4: covariance
                            kv(
                                ui,
                                "Covariance",
                                if fit.covariance_ok { "✔ OK" } else { "✖ Failed" },
                                if fit.covariance_ok { theme::GREEN } else { theme::RED },
                            );
                            if fit.cov_condition_number.is_finite() {
                                let cn_color =
                                    if fit.cn_high() { theme::ORANGE } else { theme::FG };
                                kv(ui, "Cond. number", &fmt_f64_0dp(fit.cov_condition_number), cn_color);
                            }
                            ui.end_row();

                            // Row 5: IWRES diagnostics (ferx >= 0.1.5 — only shown when available)
                            if let Some(dw) = fit.dw_statistic {
                                let dw_color = if !(1.5..=2.5).contains(&dw) {
                                    theme::ORANGE
                                } else {
                                    theme::GREEN
                                };
                                kv(ui, "Durbin-Watson", &format!("{dw:.3}"), dw_color);
                                if let Some(r) = fit.iwres_lag1_r {
                                    let r_color = if r.abs() > 0.2 { theme::ORANGE } else { theme::FG };
                                    kv(ui, "IWRES lag-1 r", &format!("{r:.3}"), r_color);
                                }
                                ui.end_row();
                            }
                        });
                });

            // ── Warnings ──
            if !fit.warnings_structured.is_empty() {
                // Severity-grouped display (ferx >= 0.1.5).
                for (severity, icon, color, bg) in [
                    ("critical", "✖", theme::RED,    egui::Color32::from_rgba_unmultiplied(0xe8, 0x55, 0x55, 25)),
                    // Solid black (not theme-tinted translucent): the previous
                    // translucent orange-over-panel washed out to a pale peach
                    // in light mode (measured contrast ~2.1, below AA's 4.5
                    // floor) — reported as hard to read. Orange-on-black
                    // measures ~8.8 and is the same in both themes.
                    ("warning",  "⚠", theme::ORANGE, egui::Color32::BLACK),
                    ("info",     "ℹ", theme::fg2(dark),    egui::Color32::from_rgba_unmultiplied(0x4c, 0x8a, 0xff, 15)),
                ] {
                    let group: Vec<_> = fit.warnings_structured.iter()
                        .filter(|w| w.severity == severity)
                        .collect();
                    if group.is_empty() { continue; }
                    ui.add_space(8.0);
                    egui::Frame::new()
                        .fill(bg)
                        .inner_margin(egui::Margin::same(10))
                        .corner_radius(egui::CornerRadius::same(6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(egui::RichText::new(
                                format!("{icon}  {} ({})", severity.to_uppercase(), group.len()))
                                .color(color).size(11.0).strong());
                            ui.add_space(4.0);
                            for w in &group {
                                let cat = if w.category.is_empty() { String::new() }
                                          else { format!("[{}] ", w.category) };
                                ui.label(egui::RichText::new(format!("• {}{}", cat, w.message))
                                    .color(color).size(11.0));
                            }
                        });
                }
            } else if !fit.warnings.is_empty() {
                // Flat fallback for older .fitrx bundles. Same solid-black
                // card as the structured warning block above, for the same
                // contrast reason.
                ui.add_space(10.0);
                egui::Frame::new()
                    .fill(egui::Color32::BLACK)
                    .inner_margin(egui::Margin::same(10))
                    .corner_radius(egui::CornerRadius::same(6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(egui::RichText::new("⚠  Warnings")
                            .color(theme::ORANGE).size(12.0).strong());
                        for w in &fit.warnings {
                            ui.label(egui::RichText::new(format!("• {}", w))
                                .color(theme::ORANGE).size(12.0));
                        }
                    });
            }

            // ── Shrinkage ──
            if !fit.eta_shrinkage.is_empty() || !fit.eps_shrinkage.is_empty() {
                ui.add_space(10.0);
                ui.label(egui::RichText::new("Shrinkage").color(theme::fg2(dark)).size(11.0).strong());
                egui::Grid::new("shrinkage_grid")
                    .num_columns(2)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        for (i, &s) in fit.eta_shrinkage.iter().enumerate() {
                            let name = fit.omega_names.get(i).map(|n| n.as_str()).unwrap_or("ETA");
                            ui.label(egui::RichText::new(format!("{} shrinkage:", name)).color(theme::fg2(dark)).size(12.0));
                            shrink_label(ui, s);
                            ui.end_row();
                        }
                        for (i, &s) in fit.eps_shrinkage.iter().enumerate() {
                            let name = fit.sigma_names.get(i).map(|n| n.as_str()).unwrap_or("EPS");
                            ui.label(egui::RichText::new(format!("{} shrinkage:", name)).color(theme::fg2(dark)).size(12.0));
                            shrink_label(ui, s);
                            ui.end_row();
                        }
                    });
            }
        });
}

// ── Parameters pill ───────────────────────────────────────────────────────────

fn show_params_pill(ui: &mut egui::Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;
    let Some(idx) = state.ui.selected_model else { return };
    let entry = &state.workspace.models[idx];

    let fit = match &entry.fit {
        Some(f) => f.clone(),
        None => {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    if let Some(err) = &entry.fit_parse_error {
                        ui.label(egui::RichText::new("Could not read fit results")
                            .strong().color(theme::fg2(dark)).size(13.0));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(format!(
                            "A .fitrx bundle exists but ferxgui failed to parse it — likely an \
                             incompatibility with the ferx version that produced it.\n\n{err}"
                        )).color(theme::fg3(dark)).size(11.0));
                    } else {
                        ui.label(egui::RichText::new("No run output yet").color(theme::fg3(dark)).size(13.0));
                    }
                });
            });
            return;
        }
    };
    let params = entry.model.params.clone();

    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            // ── THETA ──
            section_header(ui, &format!("THETA  ({})", fit.theta.len()));
            params_table(ui, 7, |ui| {
                theta_table_header(ui);
                for i in 0..fit.theta.len() {
                    let name = fit.theta_names.get(i).cloned()
                        .unwrap_or_else(|| format!("THETA{}", i + 1));
                    let init = params.theta_init.get(i).copied().unwrap_or(f64::NAN);
                    let est  = fit.theta.get(i).copied().unwrap_or(f64::NAN);
                    let se   = fit.se_theta.get(i).copied().unwrap_or(f64::NAN);
                    let at_b = fit.at_lower_bound.get(i).copied().unwrap_or(false);
                    theta_param_row(ui, &name, init, est, se, at_b, false);
                }
            });

            ui.add_space(8.0);

            // ── OMEGA ──
            section_header(ui, &format!("OMEGA  ({} ETA)", fit.n_eta));
            params_table(ui, 8, |ui| {
                omega_table_header(ui);
                for i in 0..fit.n_eta {
                    let name = fit.omega_names.get(i).cloned()
                        .unwrap_or_else(|| format!("OMEGA({},{})", i + 1, i + 1));
                    let init = params.omega_init.get(i).copied().unwrap_or(f64::NAN);
                    let est  = fit.omega_value(i, i).unwrap_or(f64::NAN);
                    // se_omega from ferx has one entry per diagonal parameter.
                    let se   = fit.se_omega.get(i).copied().unwrap_or(f64::NAN);
                    let param_type = fit.eta_param_types.get(i).map(|s| s.as_str()).unwrap_or("log_normal");
                    omega_param_row(ui, &name, init, est, se, param_type, false, false);
                }
            });
            // Off-diagonal covariances (block omega) — rendered outside the grid.
            // Note: se_omega in ferx 0.1.5 only carries diagonal SEs, so SE and
            // 95% CI are not available for off-diagonal entries.
            if fit.n_eta > 1 {
                // Collect all off-diagonal entries that have a defined covariance.
                let off_diag: Vec<(String, String, f64, f64)> = (1..fit.n_eta)
                    .flat_map(|r| (0..r).map(move |c| (r, c)))
                    .filter_map(|(r, c)| {
                        let cov  = fit.omega_value(r, c)?;
                        let corr = fit.omega_corr(r, c)?;
                        let rn = fit.omega_names.get(r).cloned()
                            .unwrap_or_else(|| format!("ETA{}", r + 1));
                        let cn = fit.omega_names.get(c).cloned()
                            .unwrap_or_else(|| format!("ETA{}", c + 1));
                        Some((rn, cn, cov, corr))
                    })
                    .collect();

                if !off_diag.is_empty() {
                    let dark = ui.visuals().dark_mode;
                    let dim  = if dark { theme::FG2 } else { egui::Color32::from_gray(100) };
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("Covariances / correlations  (SE not available for off-diagonal)")
                            .color(dim).size(10.0),
                    );
                    ui.add_space(3.0);
                    egui::Grid::new("omega_offdiag")
                        .num_columns(3)
                        .spacing([16.0, 3.0])
                        .show(ui, |ui| {
                            for h in ["ETA PAIR", "COV", "CORR"] {
                                ui.label(egui::RichText::new(h).color(theme::fg3(dark)).size(10.0).strong());
                            }
                            ui.end_row();
                            for (rn, cn, cov, corr) in &off_diag {
                                let pair = format!("{rn} ~ {cn}");
                                ui.label(egui::RichText::new(pair).color(theme::fg2(dark)).size(11.0).monospace());
                                ui.label(egui::RichText::new(fmt_sig4(*cov)).color(theme::fg(dark)).size(11.0));
                                let cc = if corr.abs() > 0.5 { theme::ORANGE }
                                         else if corr.abs() > 0.3 { theme::YELLOW }
                                         else { theme::FG };
                                ui.label(egui::RichText::new(format!("{corr:.3}")).color(cc).size(11.0));
                                ui.end_row();
                            }
                        });
                }
            }

            // ── KAPPA (IOV) ──
            if fit.n_kappa > 0 {
                ui.add_space(8.0);
                section_header(ui, &format!("KAPPA  ({} IOV)", fit.n_kappa));
                params_table(ui, 8, |ui| {
                    omega_table_header(ui);
                    for i in 0..fit.n_kappa {
                        let name = fit.kappa_names.get(i).cloned()
                            .unwrap_or_else(|| format!("KAPPA{}", i + 1));
                        let est = fit.kappa_value(i, i).unwrap_or(f64::NAN);
                        let se  = fit.se_kappa.get(i).copied().unwrap_or(f64::NAN);
                        omega_param_row(ui, &name, f64::NAN, est, se, "log_normal", false, false);
                    }
                });

                // Off-diagonal block_kappa covariances (if any).
                if fit.n_kappa > 1 {
                    let off_diag: Vec<(String, String, f64, f64)> = (1..fit.n_kappa)
                        .flat_map(|r| (0..r).map(move |c| (r, c)))
                        .filter_map(|(r, c)| {
                            let cov  = fit.kappa_value(r, c)?;
                            let corr = fit.kappa_corr(r, c)?;
                            let rn = fit.kappa_names.get(r).cloned()
                                .unwrap_or_else(|| format!("KAPPA{}", r + 1));
                            let cn = fit.kappa_names.get(c).cloned()
                                .unwrap_or_else(|| format!("KAPPA{}", c + 1));
                            Some((rn, cn, cov, corr))
                        })
                        .collect();

                    if !off_diag.is_empty() {
                        let dim = if dark { theme::FG2 } else { egui::Color32::from_gray(100) };
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(
                                "Covariances / correlations  (SE not available for off-diagonal)")
                                .color(dim).size(10.0),
                        );
                        ui.add_space(3.0);
                        egui::Grid::new("kappa_offdiag")
                            .num_columns(3)
                            .spacing([16.0, 3.0])
                            .show(ui, |ui| {
                                for h in ["KAPPA PAIR", "COV", "CORR"] {
                                    ui.label(egui::RichText::new(h).color(theme::fg3(dark))
                                        .size(10.0).strong());
                                }
                                ui.end_row();
                                for (rn, cn, cov, corr) in &off_diag {
                                    let pair = format!("{rn} ~ {cn}");
                                    ui.label(egui::RichText::new(pair).color(theme::fg2(dark))
                                        .size(11.0).monospace());
                                    ui.label(egui::RichText::new(fmt_sig4(*cov))
                                        .color(theme::fg(dark)).size(11.0));
                                    let cc = if corr.abs() > 0.5 { theme::ORANGE }
                                             else if corr.abs() > 0.3 { theme::YELLOW }
                                             else { theme::FG };
                                    ui.label(egui::RichText::new(format!("{corr:.3}"))
                                        .color(cc).size(11.0));
                                    ui.end_row();
                                }
                            });
                    }
                }
            }

            ui.add_space(8.0);

            // ── SIGMA ──
            section_header(ui, &format!("SIGMA  ({})", fit.sigma.len()));
            params_table(ui, 7, |ui| {
                theta_table_header(ui);
                for i in 0..fit.sigma.len() {
                    let name = fit.sigma_names.get(i).cloned()
                        .unwrap_or_else(|| format!("SIGMA{}", i + 1));
                    let init = params.sigma_init.get(i).copied().unwrap_or(f64::NAN);
                    let est  = fit.sigma.get(i).copied().unwrap_or(f64::NAN);
                    let se   = fit.se_sigma.get(i).copied().unwrap_or(f64::NAN);
                    theta_param_row(ui, &name, init, est, se, false, false);
                }
            });

            // ── ETAbar test ──
            if !fit.etabar.is_empty() {
                ui.add_space(8.0);
                section_header(ui, "ETAbar  (H₀: mean ETA = 0)");
                egui::Grid::new("etabar_grid")
                    .num_columns(3)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("ETA").color(theme::fg3(dark)).size(11.0).strong());
                        ui.label(egui::RichText::new("mean").color(theme::fg3(dark)).size(11.0).strong());
                        ui.label(egui::RichText::new("p-value").color(theme::fg3(dark)).size(11.0).strong());
                        ui.end_row();
                        for i in 0..fit.etabar.len() {
                            let name = fit.omega_names.get(i).map(|n| n.as_str()).unwrap_or("ETA");
                            let p = fit.etabar_pvalue.get(i).copied().unwrap_or(f64::NAN);
                            let pcolor = if p.is_nan() { theme::fg3(dark) } else if p > 0.05 { theme::GREEN } else { theme::RED };
                            ui.label(egui::RichText::new(name).color(theme::fg2(dark)).size(12.0).monospace());
                            ui.label(egui::RichText::new(fmt_f64_4dp(fit.etabar[i])).size(12.0));
                            ui.label(egui::RichText::new(fmt_f64_4dp(p)).color(pcolor).size(12.0));
                            ui.end_row();
                        }
                    });
            }
        });
}

fn section_header(ui: &mut egui::Ui, title: &str) {
    let dark = ui.visuals().dark_mode;
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 24.0),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(rect, 0.0, theme::elevated_fill(dark));
    ui.painter().text(
        rect.left_center() + egui::vec2(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(12.0),
        theme::fg2(dark),
    );
    ui.add_space(4.0);
}

fn params_table(ui: &mut egui::Ui, n_cols: usize, add_content: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(egui::Id::new(ui.next_auto_id()))
        .num_columns(n_cols)
        .spacing([8.0, 4.0])
        .min_col_width(40.0)
        .show(ui, add_content);
}

/// Header for THETA / SIGMA rows: no CV% column (CV% is not defined for fixed-effect
/// parameters or residual error on the estimation scale).
fn theta_table_header(ui: &mut egui::Ui) {
    let dark = ui.visuals().dark_mode;
    for h in ["PARAM","INIT→FINAL","ESTIMATE","SE","RSE%","95% LO","95% HI"] {
        ui.label(egui::RichText::new(h).color(theme::fg2(dark)).size(10.0).strong());
    }
    ui.end_row();
}

fn omega_table_header(ui: &mut egui::Ui) {
    let dark = ui.visuals().dark_mode;
    for h in ["PARAM","INIT→FINAL","ESTIMATE","SE","CV%","RSE%","95% LO","95% HI"] {
        ui.label(egui::RichText::new(h).color(theme::fg2(dark)).size(10.0).strong());
    }
    ui.end_row();
}

/// Row for THETA and SIGMA parameters — 7 columns, no CV%.
fn theta_param_row(
    ui: &mut egui::Ui,
    name: &str,
    initial: f64,
    estimate: f64,
    se: f64,
    at_bound: bool,
    fixed: bool,
) {
    let dark = ui.visuals().dark_mode;
    ui.label(egui::RichText::new(name).color(theme::fg(dark)).size(12.0).monospace());
    draw_init_final_cell(ui, initial, estimate, fixed, at_bound);
    let est_color = if at_bound { theme::ORANGE } else { theme::fg(dark) };
    ui.label(egui::RichText::new(fmt_sig4(estimate)).color(est_color).size(12.0));
    ui.label(egui::RichText::new(fmt_sig4(se)).color(theme::fg2(dark)).size(12.0));
    let rse = rse_pct(estimate, se);
    let rse_color = rse_color(rse);
    ui.label(egui::RichText::new(fmt_f64_1dp(rse)).color(rse_color).size(12.0));
    ci_cells(ui, estimate, se);
    ui.end_row();
}

/// Row for OMEGA diagonal entries — 8 columns with CV% after SE.
///
/// CV% formula depends on the ETA parameterisation:
///   - Log-normal  (`CL = TVCL * exp(ETA)`): CV% = sqrt(exp(ω) − 1) × 100
///   - Additive / normal (`CL = TVCL + ETA`): SD% = sqrt(ω) × 100
///   - Logit / custom: shown as "—"
fn omega_param_row(
    ui: &mut egui::Ui,
    name: &str,
    initial: f64,
    estimate: f64,
    se: f64,
    param_type: &str,
    at_bound: bool,
    fixed: bool,
) {
    let dark = ui.visuals().dark_mode;
    ui.label(egui::RichText::new(name).color(theme::fg(dark)).size(12.0).monospace());
    draw_init_final_cell(ui, initial, estimate, fixed, at_bound);
    ui.label(egui::RichText::new(fmt_sig4(estimate)).color(theme::fg(dark)).size(12.0));
    ui.label(egui::RichText::new(fmt_sig4(se)).color(theme::fg2(dark)).size(12.0));
    let cv_cell: Option<(f64, &str)> = if estimate.is_finite() {
        match param_type {
            "log_normal"           => Some(((estimate.exp() - 1.0).sqrt() * 100.0, "CV%")),
            "additive" | "normal"  => Some((estimate.sqrt() * 100.0, "SD%")),
            _                      => None,
        }
    } else { None };
    if let Some((val, label)) = cv_cell {
        let col = if val > 100.0 { theme::RED } else if val > 50.0 { theme::ORANGE } else { theme::fg(dark) };
        ui.label(egui::RichText::new(format!("{val:.1}% {label}")).color(col).size(12.0));
    } else {
        ui.label(egui::RichText::new("—").color(theme::fg3(dark)).size(12.0));
    }
    let rse = rse_pct(estimate, se);
    ui.label(egui::RichText::new(fmt_f64_1dp(rse)).color(rse_color(rse)).size(12.0));
    ci_cells(ui, estimate, se);
    ui.end_row();
}

// ── Shared helpers ────────────────────────────────────────────────────────────

fn rse_pct(estimate: f64, se: f64) -> f64 {
    if estimate != 0.0 && se.is_finite() { (se / estimate).abs() * 100.0 } else { f64::NAN }
}

fn rse_color(rse: f64) -> egui::Color32 {
    if rse.is_nan() { theme::FG3 }
    else if rse < 20.0 { theme::GREEN }
    else if rse < 30.0 { theme::ORANGE }
    else { theme::RED }
}

fn ci_cells(ui: &mut egui::Ui, estimate: f64, se: f64) {
    let dark = ui.visuals().dark_mode;
    if se.is_finite() && estimate.is_finite() {
        let ci_color = theme::fg2(dark);
        ui.label(egui::RichText::new(fmt_sig4(estimate - 1.96 * se)).color(ci_color).size(11.0));
        ui.label(egui::RichText::new(fmt_sig4(estimate + 1.96 * se)).color(ci_color).size(11.0));
    } else {
        ui.label(egui::RichText::new("—").color(theme::fg3(dark)).size(11.0));
        ui.label(egui::RichText::new("—").color(theme::fg3(dark)).size(11.0));
    }
}

/// Paints the Init→Final track cell (110px wide).
fn draw_init_final_cell(
    ui: &mut egui::Ui,
    initial: f64,
    estimate: f64,
    fixed: bool,
    at_bound: bool,
) {
    let desired = egui::vec2(110.0, 14.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::hover());

    if !ui.is_rect_visible(rect) {
        return;
    }

    let dark = ui.visuals().dark_mode;
    let p = ui.painter_at(rect);

    // Track background.
    p.rect_filled(rect, 3.0, theme::elevated_fill(dark));

    if fixed {
        let fix_bg = if dark { theme::BG4 } else { egui::Color32::from_gray(210) };
        p.rect_filled(rect, 3.0, fix_bg);
        p.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "FIX",
            egui::FontId::proportional(9.0),
            theme::fg3(dark),
        );
        return;
    }

    // Tick marks at ×0.1 (−1), ×0.5 (−0.301), ×2 (+0.301), ×10 (+1).
    let cx = rect.center().x;
    for log_r in [-1.0_f32, -0.301, 0.0, 0.301, 1.0] {
        let x = rect.left() + (log_r + 1.0) / 2.0 * rect.width();
        let is_center = log_r == 0.0;
        p.line_segment(
            [
                egui::pos2(x, rect.top() + if is_center { 1.0 } else { 3.0 }),
                egui::pos2(x, rect.bottom() - if is_center { 1.0 } else { 3.0 }),
            ],
            egui::Stroke::new(
                if is_center { 1.5 } else { 0.5 },
                if is_center {
                    egui::Color32::from_gray(180)
                } else {
                    egui::Color32::from_gray(100)
                },
            ),
        );
    }

    let _ = cx; // suppress unused warning

    // Can't draw marker without valid values.
    if initial == 0.0 || initial.is_nan() || estimate.is_nan() {
        return;
    }
    let same_sign = (initial > 0.0) == (estimate > 0.0);
    if !same_sign {
        return;
    }

    let log_ratio = (estimate / initial).abs().log10() as f32;
    let off_scale = log_ratio.abs() > 1.0;
    let clamped = log_ratio.clamp(-1.0, 1.0);
    let marker_x = rect.left() + (clamped + 1.0) / 2.0 * rect.width();
    let cy = rect.center().y;

    // Bound wall.
    if at_bound {
        let wx = if log_ratio < 0.0 { rect.left() + 2.0 } else { rect.right() - 2.0 };
        p.line_segment(
            [egui::pos2(wx, rect.top()), egui::pos2(wx, rect.bottom())],
            egui::Stroke::new(2.0, theme::RED),
        );
    }

    // Marker color: blue for ≤ ×2, orange for >×2.
    let color = if log_ratio.abs() <= 0.301 { theme::ACCENT } else { theme::ORANGE };

    if off_scale {
        let ch = if log_ratio > 0.0 { "▶" } else { "◀" };
        let ex = if log_ratio > 0.0 { rect.right() - 6.0 } else { rect.left() + 6.0 };
        p.text(egui::pos2(ex, cy), egui::Align2::CENTER_CENTER, ch, egui::FontId::proportional(10.0), color);
    } else {
        p.circle_filled(egui::pos2(marker_x, cy), 4.0, color);
    }

    // Hover tooltip: ratio value.
    if response.hovered() {
        let ratio = estimate / initial;
        let tip = if ratio >= 1.0 {
            format!("×{:.2}  (init {:.4} → final {:.4})", ratio, initial, estimate)
        } else {
            format!("÷{:.2}  (init {:.4} → final {:.4})", 1.0 / ratio, initial, estimate)
        };
        response.on_hover_text(tip);
    }
}

// ── Info pill ─────────────────────────────────────────────────────────────────

fn show_info_pill(ui: &mut egui::Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;
    let Some(idx) = state.ui.selected_model else { return };

    let mut changed = false;

    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            egui::Grid::new("info_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .min_col_width(80.0)
                .show(ui, |ui| {
                    // Comment.
                    ui.label(egui::RichText::new("Comment:").color(theme::fg2(dark)).size(12.0));
                    let resp = ui.add(
                        egui::TextEdit::singleline(
                            &mut state.workspace.models[idx].meta.comment,
                        )
                        .desired_width(f32::INFINITY),
                    );
                    if resp.changed() { changed = true; }
                    ui.end_row();

                    // Status.
                    ui.label(egui::RichText::new("Status:").color(theme::fg2(dark)).size(12.0));
                    let cur_status = state.workspace.models[idx].meta.status.clone();
                    egui::ComboBox::from_id_salt("info_status")
                        .selected_text(cur_status.label())
                        .show_ui(ui, |ui| {
                            for s in ModelStatus::all() {
                                if ui.selectable_label(&cur_status == s, s.label()).clicked() {
                                    state.workspace.models[idx].meta.status = s.clone();
                                    changed = true;
                                }
                            }
                        });
                    ui.end_row();

                    // Decision.
                    ui.label(egui::RichText::new("Decision:").color(theme::fg2(dark)).size(12.0));
                    let cur_dec = state.workspace.models[idx].meta.decision.clone();
                    egui::ComboBox::from_id_salt("info_decision")
                        .selected_text(cur_dec.label())
                        .show_ui(ui, |ui| {
                            for d in ModelDecision::all() {
                                if ui.selectable_label(&cur_dec == d, d.label()).clicked() {
                                    state.workspace.models[idx].meta.decision = d.clone();
                                    changed = true;
                                }
                            }
                        });
                    ui.end_row();

                    // Based on.
                    ui.label(egui::RichText::new("Based on:").color(theme::fg2(dark)).size(12.0));
                    let mut based_on = state.workspace.models[idx]
                        .meta
                        .based_on
                        .clone()
                        .unwrap_or_default();
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut based_on)
                            .desired_width(200.0)
                            .hint_text("parent model stem"),
                    );
                    if resp.changed() {
                        state.workspace.models[idx].meta.based_on =
                            if based_on.is_empty() { None } else { Some(based_on) };
                        changed = true;
                    }
                    ui.end_row();

                    // Tags.
                    ui.label(egui::RichText::new("Tags:").color(theme::fg2(dark)).size(12.0));
                    let mut tags_str = state.workspace.models[idx].meta.tags.join(", ");
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut tags_str)
                            .desired_width(f32::INFINITY)
                            .hint_text("comma-separated"),
                    );
                    if resp.changed() {
                        state.workspace.models[idx].meta.tags = tags_str
                            .split(',')
                            .map(|t| t.trim().to_string())
                            .filter(|t| !t.is_empty())
                            .collect();
                        changed = true;
                    }
                    ui.end_row();
                });

            // Notes (multi-line).
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Notes:").color(theme::fg2(dark)).size(12.0));
            let resp = ui.add(
                egui::TextEdit::multiline(
                    &mut state.workspace.models[idx].meta.notes,
                )
                .desired_rows(6)
                .desired_width(f32::INFINITY),
            );
            if resp.changed() { changed = true; }

            // ── Model Structure (from ferx_model_inspect via R) ──────────────
            let stem = state.workspace.models[idx].model.stem.clone();
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Model Structure").strong().size(12.0).color(theme::fg2(dark)));
            ui.add_space(4.0);
            if state.workspace.r_inspecting.contains(&stem) {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0));
                    ui.label(egui::RichText::new("Loading…").color(theme::fg3(dark)).size(11.0));
                });
            } else if state.workspace.r_inspect_failed.contains(&stem) {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("⚠").color(theme::ORANGE).size(11.0));
                    ui.label(
                        egui::RichText::new("Structure inspect failed")
                            .color(theme::ORANGE)
                            .size(11.0),
                    );
                    if ui.small_button("Retry").clicked() {
                        state.workspace.r_inspect_failed.remove(&stem);
                    }
                });
                ui.label(
                    egui::RichText::new(
                        "ferx_model_inspect() could not run. Possible causes:\n\
                         • R or the ferx package is not installed\n\
                         • The model file has a syntax error\n\
                         Check the Settings tab for ferx detection status.",
                    )
                    .color(theme::fg3(dark))
                    .size(11.0),
                );
            } else if let Some(info) = state.workspace.r_model_infos.get(&stem).cloned() {
                egui::Grid::new("model_struct_grid")
                    .num_columns(2)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        let lbl = |ui: &mut egui::Ui, s: &str| {
                            ui.label(egui::RichText::new(s).color(theme::fg3(dark)).size(11.0).monospace());
                        };
                        let val = |ui: &mut egui::Ui, s: &str| {
                            ui.label(egui::RichText::new(s).size(12.0));
                        };

                        if !info.model_type.is_empty() {
                            lbl(ui, "Type");
                            val(ui, &info.model_type);
                            ui.end_row();
                        }
                        if !info.theta_names.is_empty() {
                            lbl(ui, "Parameters");
                            val(ui, &info.theta_names.join(", "));
                            ui.end_row();
                        }
                        if !info.iiv.is_empty() {
                            lbl(ui, "IIV");
                            val(ui, &info.iiv.join(", "));
                            ui.end_row();
                        }
                        if !info.residual.is_empty() {
                            lbl(ui, "Residual");
                            val(ui, &info.residual);
                            ui.end_row();
                        }
                    });
            } else {
                ui.label(
                    egui::RichText::new("R not available or ferx package not installed")
                        .color(theme::fg3(dark))
                        .size(11.0),
                );
            }
        });

    if changed {
        save_meta_for(state, idx);
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Reload editor buffer when the selected model changes.
fn sync_editor_buffer(state: &mut AppState) {
    let current_stem = state
        .ui
        .selected_model
        .and_then(|i| state.workspace.models.get(i))
        .map(|e| e.model.stem.clone());

    if current_stem != state.ui.editor_loaded_stem {
        if let Some(stem) = &current_stem {
            if let Some(idx) = state.ui.selected_model {
                state.ui.editor_buffer = state.workspace.models[idx].model.source.clone();
                state.ui.editor_dirty = false;
                state.ui.editor_loaded_stem = Some(stem.clone());

                // The model file's [fit_options] is authoritative: initialise the
                // run controls from it on load. Covariance defaults on — an absent
                // or commented-out directive means the covariance step still runs;
                // an explicit `covariance = false` in the file is what opts out.
                let opts = crate::io::ferx_file::parse_fit_options(
                    &state.workspace.models[idx].model.source);
                state.ui.run_covariance = opts.covariance.unwrap_or(true);
                if let Some(m) = opts.method   { state.ui.run_method = m; }
                if let Some(g) = opts.gradient { state.ui.run_gradient = g; }
                if let Some(t) = opts.threads  { state.ui.run_threads = t; }
            }
        } else {
            state.ui.editor_buffer.clear();
            state.ui.editor_dirty = false;
            state.ui.editor_loaded_stem = None;
        }
    }
}

fn save_meta_for(state: &mut AppState, _idx: usize) {
    let dir = match &state.workspace.directory {
        Some(d) => d.clone(),
        None => return,
    };
    let Some(app_dir) = state.workspace.app_dir.clone() else { return };
    let meta_map: HashMap<String, _> = state
        .workspace
        .models
        .iter()
        .map(|e| (e.model.stem.clone(), e.meta.clone()))
        .collect();
    let _ = save_model_meta(&app_dir, &dir, &meta_map);
}

/// Syntax-highlight a .ferx source string into an egui LayoutJob.
pub(crate) fn highlight_ferx(text: &str, dark: bool) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let font = egui::FontId::monospace(12.0);

    // Colour palette — dark mode uses bright/light colours; light mode uses
    // saturated-but-dark colours so they read on a white/near-white background.
    let (
        col_plain,      // identifiers, values, plain body
        col_comment,    // # …
        col_header,     // [section]
        col_param_kw,   // theta / omega / sigma / kappa
        col_builtin,    // pk, one_cpt_oral, ode, …
        col_option,     // method, maxiter, gradient, …
        col_number,     // numeric literals
    ) = if dark {(
        theme::FG,
        theme::FG3,
        theme::ACCENT,
        egui::Color32::from_rgb(0x86, 0xc1, 0xff),
        theme::GREEN,
        egui::Color32::from_rgb(0xe8, 0xb5, 0x56),
        theme::YELLOW,
    )} else {(
        egui::Color32::from_rgb(0x0f, 0x11, 0x1a),   // near-black
        egui::Color32::from_rgb(0x6a, 0x72, 0x8a),   // medium gray italic
        egui::Color32::from_rgb(0x19, 0x63, 0xeb),   // dark blue
        egui::Color32::from_rgb(0x17, 0x65, 0xd0),   // medium blue
        egui::Color32::from_rgb(0x1a, 0x8a, 0x4a),   // dark green
        egui::Color32::from_rgb(0x7c, 0x52, 0x00),   // dark amber
        egui::Color32::from_rgb(0xb0, 0x60, 0x00),   // dark orange
    )};

    let plain  = |c: egui::Color32| egui::text::TextFormat { font_id: font.clone(), color: c, ..Default::default() };
    let italic = |c: egui::Color32| egui::text::TextFormat { font_id: font.clone(), color: c, italics: true, ..Default::default() };

    for line in text.split('\n') {
        let tokens = tokenise_line(line);
        let mut pos = 0usize;

        for (start, end, kind) in &tokens {
            if pos < *start {
                job.append(&line[pos..*start], 0.0, plain(col_plain));
            }
            let fmt = match kind {
                TokenKind::SectionHeader   => plain(col_header),
                TokenKind::ParamKeyword    => plain(col_param_kw),
                TokenKind::BuiltinFunction => plain(col_builtin),
                TokenKind::OptionKey       => plain(col_option),
                TokenKind::Number          => plain(col_number),
                TokenKind::Comment         => italic(col_comment),
                TokenKind::Plain           => plain(col_plain),
            };
            job.append(&line[*start..*end], 0.0, fmt);
            pos = *end;
        }

        if pos < line.len() { job.append(&line[pos..], 0.0, plain(col_plain)); }
        job.append("\n", 0.0, plain(col_plain));
    }

    job
}

// ── Formatting helpers ────────────────────────────────────────────────────────

fn fmt_f64_4dp(v: f64) -> String {
    if v.is_nan() { "—".to_string() } else { format!("{:.4}", v) }
}
fn fmt_f64_2dp(v: f64) -> String {
    if v.is_nan() { "—".to_string() } else { format!("{:.2}", v) }
}
fn fmt_f64_1dp(v: f64) -> String {
    if v.is_nan() { "—".to_string() } else { format!("{:.1}", v) }
}
fn fmt_f64_0dp(v: f64) -> String {
    if v.is_nan() { "—".to_string() } else { format!("{:.0}", v) }
}
fn fmt_sig4(v: f64) -> String {
    if v.is_nan() { "—".to_string() } else { format!("{:.4}", v) }
}
fn fmt_duration(secs: f64) -> String {
    if secs <= 0.0 { return "—".to_string(); }
    let s = secs as u64;
    if s < 60 { format!("{s}s") }
    else if s < 3600 { format!("{}m {:02}s", s / 60, s % 60) }
    else { format!("{}h {:02}m", s / 3600, (s % 3600) / 60) }
}

fn shrink_label(ui: &mut egui::Ui, v: f64) {
    let dark = ui.visuals().dark_mode;
    if v.is_nan() {
        ui.label(egui::RichText::new("—").color(theme::fg3(dark)).size(12.0));
        return;
    }
    let color = if v < 20.0 { theme::fg2(dark) } else if v < 30.0 { theme::ORANGE } else { theme::RED };
    ui.label(egui::RichText::new(format!("{:.1}%", v)).color(color).size(12.0));
}

fn kv(ui: &mut egui::Ui, key: &str, val: &str, color: egui::Color32) {
    let dark = ui.visuals().dark_mode;
    ui.label(egui::RichText::new(key).color(theme::fg2(dark)).size(11.0));
    ui.label(egui::RichText::new(val).color(color).size(12.0).strong());
}

// Timestamp helpers — canonical implementations live in workers::run.
use crate::workers::run::{now_iso, now_unix};

// ── Context menu ──────────────────────────────────────────────────────────────

/// Actions that can be triggered by the model-row context menu.
enum CtxAction {
    Run,
    Duplicate,
    ToggleReference,
    ViewRunLog,
    CompareWith(String), // target model stem
    ViewRunRecord,
    Delete,
}

fn apply_ctx_action(
    state:  &mut AppState,
    idx:    usize,
    action: CtxAction,
) {
    match action {
        CtxAction::Run => {
            let stem = state.workspace.models[idx].model.stem.clone();
            // `launch_run` reads the global `run_data_path` directly; make
            // sure it's actually set for this row before calling it (see
            // `resolve_data_path_for_run`'s doc comment).
            if state.ui.run_data_path.is_none() {
                state.ui.run_data_path = resolve_data_path_for_run(state, &stem);
            }
            launch_run(state, idx, &stem);
        }
        CtxAction::Duplicate => {
            if let Some(m) = state.workspace.models.get(idx) {
                let stem = m.model.stem.clone();
                state.ui.duplicate_stem_buf =
                    suggest_duplicate_stem(&stem, &state.workspace.models);
                state.ui.pending_duplicate = Some(idx);
            }
        }
        CtxAction::ToggleReference => {
            state.ui.reference_model = if state.ui.reference_model == Some(idx) {
                None
            } else {
                Some(idx)
            };
        }
        CtxAction::ViewRunLog => {
            if let Some(m) = state.workspace.models.get(idx) {
                if let Some(parent) = m.model.path.parent() {
                    let log = parent.join(format!("{}_run.log", m.model.stem));
                    if log.exists() {
                        if let Err(e) = open::that(&log) {
                            state.ui.status_message = format!("Could not open log: {e}");
                        }
                    } else {
                        state.ui.status_message = format!("Run log not found: {}", log.display());
                    }
                }
            }
        }
        CtxAction::CompareWith(target_stem) => {
            if let Some(m) = state.workspace.models.get(idx) {
                state.ui.compare_a = Some(m.model.stem.clone());
                state.ui.compare_b = Some(target_stem);
            }
        }
        CtxAction::ViewRunRecord => {
            if let Some(m) = state.workspace.models.get(idx) {
                // Jump to History tab and filter to this model.
                state.ui.active_tab = crate::state::Tab::History;
                state.ui.history_filter = m.model.stem.clone();
            }
        }
        CtxAction::Delete => {
            state.ui.pending_delete = Some(idx);
        }
    }
}

// ── Bookmark-name dialog ──────────────────────────────────────────────────────

fn show_bookmark_dialog(ctx: &egui::Context, state: &mut AppState) {
    let already_focused = state.ui.bookmark_dialog_focused;
    let Some((ref dir, ref mut label)) = state.ui.pending_bookmark else { return };
    let dir     = dir.clone();              // avoid borrow conflicts below
    let dir_str = dir.display().to_string(); // computed once, not per-frame inside closure

    let mut confirm      = false;
    let mut cancel       = false;
    let mut just_focused = false;

    egui::Window::new("Bookmark Project")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.set_min_width(320.0);
            let dark = ui.visuals().dark_mode;
            let dim  = if dark { theme::FG2 } else { egui::Color32::from_gray(90) };

            ui.label(
                egui::RichText::new(&dir_str)
                    .size(11.0)
                    .color(dim),
            );
            ui.add_space(8.0);
            ui.label("Project name:");
            ui.add_space(4.0);

            let resp = ui.add(
                egui::TextEdit::singleline(label)
                    .desired_width(f32::INFINITY),
            );
            if !already_focused {
                resp.request_focus();
                just_focused = true;
            }

            let can_confirm = !label.trim().is_empty();
            if resp.lost_focus()
                && can_confirm
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
            {
                confirm = true;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                cancel = true;
            }

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(
                    can_confirm,
                    egui::Button::new("Add bookmark"),
                ).clicked() {
                    confirm = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });

    if just_focused {
        state.ui.bookmark_dialog_focused = true;
    }
    if confirm {
        let label = label.trim().to_string();
        state.workspace.bookmarks.push(
            crate::io::persistence::Bookmark { label, path: dir },
        );
        if let Some(app_dir) = &state.workspace.app_dir.clone() {
            if let Err(e) = crate::io::persistence::save_bookmarks(app_dir, &state.workspace.bookmarks) {
                state.ui.status_message = format!("Could not save bookmarks: {e}");
            }
        }
        state.ui.pending_bookmark = None;
        state.ui.bookmark_dialog_focused = false;
    } else if cancel {
        state.ui.pending_bookmark = None;
        state.ui.bookmark_dialog_focused = false;
    }
}

// ── Duplicate dialog ──────────────────────────────────────────────────────────

fn show_duplicate_dialog(ctx: &egui::Context, state: &mut AppState) {
    let already_focused = state.ui.duplicate_dialog_focused;
    let Some(src_idx) = state.ui.pending_duplicate else { return };

    let src_stem = state.workspace.models
        .get(src_idx)
        .map(|m| m.model.stem.clone())
        .unwrap_or_default();

    let mut close        = false;
    let mut execute       = false;
    let mut just_focused = false;

    egui::Window::new("Duplicate Model")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.set_min_width(340.0);
            let dark = ui.visuals().dark_mode;
            let dim  = if dark { theme::FG2 } else { egui::Color32::from_gray(90) };

            ui.label(
                egui::RichText::new(format!("Duplicating  \"{}\"", src_stem))
                    .size(12.0)
                    .color(dim),
            );
            ui.add_space(10.0);
            ui.label("New model name:");
            ui.add_space(4.0);

            let resp = ui.add(
                egui::TextEdit::singleline(&mut state.ui.duplicate_stem_buf)
                    .desired_width(f32::INFINITY),
            );
            // Auto-focus the text field once when the dialog first opens —
            // guarded by a one-shot flag rather than has_focus(), which
            // would re-fire (and steal focus back) every frame after the
            // user Tabs away to the buttons below.
            if !already_focused {
                resp.request_focus();
                just_focused = true;
            }

            // Validate — name must be non-empty and not already taken.
            let new_stem = state.ui.duplicate_stem_buf.trim().to_string();
            let conflict = state.workspace.models.iter().any(|m| m.model.stem == new_stem);
            let can_ok   = !new_stem.is_empty() && !conflict;

            if conflict {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("⚠  A model with this name already exists")
                        .color(theme::ORANGE)
                        .size(11.0),
                );
            }

            ui.add_space(8.0);
            ui.checkbox(
                &mut state.ui.duplicate_set_as_child,
                format!("Set as child of  \"{}\"  (updates tree lineage)", src_stem),
            );
            ui.label(
                egui::RichText::new(
                    "When checked, the new model's 'Based on' field is set to the source.")
                    .size(10.0)
                    .color(theme::fg3(dark)),
            );

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() { close = true; }
                ui.add_space(8.0);
                if ui
                    .add_enabled(
                        can_ok,
                        egui::Button::new(
                            egui::RichText::new("Duplicate").color(egui::Color32::WHITE),
                        )
                        .fill(theme::ACCENT),
                    )
                    .clicked()
                {
                    execute = true;
                }
            });

            // Enter key confirms when valid.
            if can_ok && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                execute = true;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });

    if just_focused {
        state.ui.duplicate_dialog_focused = true;
    }
    if close {
        state.ui.pending_duplicate = None;
        state.ui.duplicate_dialog_focused = false;
    }
    if execute {
        do_duplicate(state, src_idx);
        state.ui.pending_duplicate = None;
        state.ui.duplicate_dialog_focused = false;
    }
}

fn do_duplicate(state: &mut AppState, src_idx: usize) {
    let Some(model) = state.workspace.models.get(src_idx) else { return };
    let src_path  = model.model.path.clone();
    let src_stem  = model.model.stem.clone();
    let new_stem  = state.ui.duplicate_stem_buf.trim().to_string();
    let set_child = state.ui.duplicate_set_as_child;
    let Some(dir) = src_path.parent() else { return };
    let dst_path  = dir.join(format!("{}.ferx", new_stem));

    match std::fs::copy(&src_path, &dst_path) {
        Ok(_) => {
            // Establish tree lineage: persist based_on for the new model
            // by merging into the existing meta map before the scan.
            if set_child {
                if let (Some(app_dir), Some(dir)) = (&state.workspace.app_dir, &state.workspace.directory) {
                    let mut meta_map = crate::io::persistence::load_model_meta(app_dir, dir);
                    let new_meta = crate::domain::ModelMeta {
                        based_on: Some(src_stem.clone()),
                        ..Default::default()
                    };
                    meta_map.insert(new_stem.clone(), new_meta);
                    let _ = save_model_meta(app_dir, dir, &meta_map);
                }
            }
            state.ui.status_message = format!("Created {new_stem}.ferx (child of {src_stem})");
            state.trigger_scan();
        }
        Err(e) => {
            state.ui.status_message = format!("Duplicate failed: {}", e);
        }
    }
}

/// Suggest the next available name for a duplicate.
/// "run001" → "run002", "mymodel_3" → "mymodel_4", "abc" → "abc_2".
fn suggest_duplicate_stem(stem: &str, models: &[crate::domain::ModelEntry]) -> String {
    let taken: std::collections::HashSet<&str> = models.iter().map(|m| m.model.stem.as_str()).collect();

    // Split stem into (prefix, number) or (stem, None).
    let (prefix, start_n) = split_trailing_number(stem);
    let base = prefix.trim_end_matches('_');

    let mut n = start_n.unwrap_or(1) + 1;
    loop {
        let candidate = format!("{}{:0>3}", base, n);
        if !taken.contains(candidate.as_str()) {
            return candidate;
        }
        n += 1;
        // safety valve
        if n > 999 {
            return format!("{}_copy", stem);
        }
    }
}

fn split_trailing_number(s: &str) -> (&str, Option<u32>) {
    let digits_start = s
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_digit())
        .last()
        .map(|(i, _)| i);

    match digits_start {
        Some(i) if i < s.len() => {
            let num: u32 = s[i..].parse().unwrap_or(0);
            (&s[..i], Some(num))
        }
        _ => (s, None),
    }
}

// ── Delete dialog ─────────────────────────────────────────────────────────────

fn show_delete_dialog(ctx: &egui::Context, state: &mut AppState) {
    let Some(del_idx) = state.ui.pending_delete else { return };

    let stem = state.workspace.models
        .get(del_idx)
        .map(|m| m.model.stem.clone())
        .unwrap_or_default();
    let has_fitrx = state.workspace.models
        .get(del_idx)
        .and_then(|m| m.fitrx_path.as_ref())
        .is_some();

    // Block deletion while model is running.
    let is_running = state.run.active_run
        .as_ref()
        .map(|r| r.record.model_stem == stem)
        .unwrap_or(false);

    let mut close   = false;
    let mut execute = false;

    egui::Window::new("Delete Model")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.set_min_width(320.0);
            let dark = ui.visuals().dark_mode;
            let dim  = if dark { theme::FG2 } else { egui::Color32::from_gray(90) };

            ui.label(
                egui::RichText::new(format!("Delete  \"{}\"?", stem))
                    .strong()
                    .size(14.0)
                    .color(theme::fg(dark)),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("This will permanently remove the .ferx model file.")
                    .color(dim)
                    .size(12.0),
            );
            if has_fitrx {
                ui.label(
                    egui::RichText::new("The .fitrx results bundle will also be deleted.")
                        .color(dim)
                        .size(12.0),
                );
            }
            if is_running {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("⚠  Cannot delete — a run is in progress")
                        .color(theme::ORANGE)
                        .size(11.0),
                );
            }

            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() { close = true; }
                ui.add_space(8.0);
                if ui
                    .add_enabled(
                        !is_running,
                        egui::Button::new(
                            egui::RichText::new("Delete").color(egui::Color32::WHITE),
                        )
                        .fill(theme::RED),
                    )
                    .clicked()
                {
                    execute = true;
                }
            });

            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });

    if close   { state.ui.pending_delete = None; }
    if execute {
        do_delete(state, del_idx);
        state.ui.pending_delete = None;
    }
}

fn show_new_model_dialog(ctx: &egui::Context, state: &mut AppState) {
    if !state.ui.new_model_dialog { return; }

    let mut close  = false;
    let mut create = false;

    egui::Window::new("New Model")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.set_min_width(340.0);
            let dark = ui.visuals().dark_mode;
            let dim  = if dark { theme::FG2 } else { egui::Color32::from_gray(90) };

            ui.label(egui::RichText::new("Create a new model from a template").strong().size(13.0).color(theme::fg(dark)));
            ui.add_space(12.0);

            // Template picker.
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Template:").color(dim).size(12.0));
                egui::ComboBox::from_id_salt("new_model_template_combo")
                    .selected_text(&state.ui.new_model_template)
                    .width(140.0)
                    .show_ui(ui, |ui| {
                        for t in ["1cpt_oral","1cpt_iv","2cpt_oral","2cpt_iv","ode"] {
                            ui.selectable_value(
                                &mut state.ui.new_model_template, t.to_string(), t);
                        }
                    });
            });

            ui.add_space(8.0);

            // Model name field.
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Model name:").color(dim).size(12.0));
                ui.add(
                    egui::TextEdit::singleline(&mut state.ui.new_model_stem)
                        .desired_width(160.0)
                        .hint_text("e.g. run001"),
                );
                ui.label(egui::RichText::new(".ferx").color(dim).size(12.0));
            });

            // Warn if no working directory.
            if state.workspace.directory.is_none() {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("⚠  No working directory — open one first")
                        .color(theme::ORANGE).size(11.0),
                );
            }

            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() { close = true; }
                ui.add_space(8.0);
                let can_create = !state.ui.new_model_stem.trim().is_empty()
                    && state.workspace.directory.is_some()
                    && state.workspace.settings.ferx_binary.is_some();
                if ui
                    .add_enabled(
                        can_create,
                        egui::Button::new(
                            egui::RichText::new("Create").color(egui::Color32::WHITE),
                        )
                        .fill(theme::ACCENT),
                    )
                    .clicked()
                {
                    create = true;
                }
            });

            if ui.input(|i| i.key_pressed(egui::Key::Escape)) { close = true; }
            if ui.input(|i| i.key_pressed(egui::Key::Enter))
                && !state.ui.new_model_stem.trim().is_empty()
                && state.workspace.directory.is_some()
            {
                create = true;
            }
        });

    if close { state.ui.new_model_dialog = false; }
    if create {
        do_create_model(state, ctx);
        state.ui.new_model_dialog = false;
    }
}

/// Spawn a background thread to create the template file, keeping the UI responsive.
/// `Rscript` startup can take 1–5 s; blocking the egui frame loop is not acceptable.
fn do_create_model(state: &mut AppState, ctx: &egui::Context) {
    let Some(dir) = &state.workspace.directory else { return };
    let stem = state.ui.new_model_stem.trim().to_string();
    if stem.is_empty() { return; }

    let path     = dir.join(format!("{stem}.ferx"));
    let template = state.ui.new_model_template.clone();
    let tx       = state.worker_tx.clone();
    let ctx      = ctx.clone();

    state.ui.status_message = format!("Creating {stem}.ferx…");

    std::thread::spawn(move || {
        match crate::io::r_extract::create_model_from_template(&path, &template) {
            Ok(()) => {
                let _ = tx.send(crate::workers::messages::WorkerMsg::ModelCreated(stem));
            }
            Err(e) => {
                let _ = tx.send(crate::workers::messages::WorkerMsg::RTaskError {
                    context: "new_model".to_string(),
                    message: e,
                });
            }
        }
        ctx.request_repaint();
    });
}

fn do_delete(state: &mut AppState, idx: usize) {
    let Some(model) = state.workspace.models.get(idx) else { return };
    let ferx_path  = model.model.path.clone();
    let fitrx_path = model.fitrx_path.clone();
    let stem       = model.model.stem.clone();

    if let Err(e) = std::fs::remove_file(&ferx_path) {
        state.ui.status_message = format!("Delete failed: {}", e);
        return;
    }
    if let Some(p) = fitrx_path {
        let _ = std::fs::remove_file(p); // best-effort
    }

    // Adjust indices pointing past the removed entry.
    state.workspace.models.remove(idx);
    for slot in [&mut state.ui.selected_model, &mut state.ui.reference_model] {
        *slot = match *slot {
            Some(i) if i == idx => None,
            Some(i) if i > idx  => Some(i - 1),
            other               => other,
        };
    }

    state.ui.status_message = format!("Deleted {}.ferx", stem);
}

// ── Compare picker ───────────────────────────────────────────────────────────

/// "Compare Models…" picker window — sets `compare_a`/`compare_b` from an
/// explicit two-model choice, which is what `show_compare_dialog` below
/// already keys off (same trigger the right-click "Compare with…" row
/// action uses; this just gives it a second, discoverable entry point).
fn show_compare_picker(ctx: &egui::Context, state: &mut AppState) {
    if !state.ui.compare_picker_open { return; }

    let is_dark = ctx.style().visuals.dark_mode;
    let fitted_stems: Vec<String> = state.workspace.models.iter()
        .filter(|m| m.fit.is_some())
        .map(|m| m.model.stem.clone())
        .collect();

    let mut close = false;
    let mut start_compare = false;

    // Real OS viewport (matching the About/Run/SIR/Settings popups elsewhere
    // in this codebase): an in-window `egui::Window` here could render
    // partially outside the main app window with no way to reach its Cancel
    // button — reported as "cannot close it anymore and it not fully
    // visible on screen". A real window gets its own native close button
    // and isn't bounded by the main window's canvas.
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("compare_picker"),
        egui::ViewportBuilder::default()
            .with_title("Compare Models")
            .with_inner_size(egui::vec2(360.0, 220.0))
            .with_resizable(true)
            .with_min_inner_size(egui::vec2(300.0, 180.0)),
        |ctx, _class| {
            if is_dark { theme::apply_dark(ctx); } else { theme::apply_light(ctx); }

            if ctx.input(|i| i.viewport().close_requested()) {
                close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }

            egui::CentralPanel::default().show(ctx, |ui| {
                if fitted_stems.len() < 2 {
                    ui.label("Need at least two models with a completed fit to compare.");
                } else {
                    egui::Grid::new("compare_picker_grid").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                        for (row_label, picker_id, selection) in [
                            ("Model A:", "compare_picker_a", &mut state.ui.compare_picker_a),
                            ("Model B:", "compare_picker_b", &mut state.ui.compare_picker_b),
                        ] {
                            ui.label(row_label);
                            egui::ComboBox::from_id_salt(picker_id)
                                .selected_text(selection.clone().unwrap_or_else(|| "Select…".to_string()))
                                .show_ui(ui, |ui| {
                                    for stem in &fitted_stems {
                                        if ui.selectable_label(selection.as_deref() == Some(stem), stem).clicked() {
                                            *selection = Some(stem.clone());
                                        }
                                    }
                                });
                            ui.end_row();
                        }
                    });
                }

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let can_compare = state.ui.compare_picker_a.is_some()
                        && state.ui.compare_picker_b.is_some()
                        && state.ui.compare_picker_a != state.ui.compare_picker_b;
                    if ui.add_enabled(can_compare, egui::Button::new("Compare")).clicked() {
                        start_compare = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
        },
    );

    if start_compare {
        state.ui.compare_a = state.ui.compare_picker_a.take();
        state.ui.compare_b = state.ui.compare_picker_b.take();
        state.ui.compare_picker_open = false;
    } else if close {
        state.ui.compare_picker_open = false;
        state.ui.compare_picker_a = None;
        state.ui.compare_picker_b = None;
    }
}

// ── Compare dialog ────────────────────────────────────────────────────────────

/// DV-vs-PRED scatter points (excluding non-finite rows) and padded axis
/// bounds, for the compare dialog's GOF-comparison plots.
fn dv_vs_pred_points_and_bounds(data: &crate::domain::EvalData) -> (Vec<[f64; 2]>, f64, f64) {
    let [lo, hi] = data.dv_pred_range();
    let pad = (hi - lo) * 0.05;
    let pts: Vec<[f64; 2]> = data.rows.iter()
        .filter(|r| r.pred.is_finite() && r.dv.is_finite())
        .map(|r| [r.pred, r.dv]).collect();
    (pts, lo - pad, hi + pad)
}

#[cfg(test)]
mod dv_vs_pred_points_and_bounds_tests {
    use super::dv_vs_pred_points_and_bounds;
    use crate::domain::{EvalData, PredRow};

    fn row(dv: f64, pred: f64) -> PredRow {
        PredRow {
            id: "1".to_string(), time: 0.0, dv, pred, ipred: pred,
            cwres: 0.0, iwres: 0.0, ebe_ofv: f64::NAN,
        }
    }

    #[test]
    fn excludes_non_finite_rows_and_pads_the_range() {
        let data = EvalData::from_rows(vec![
            row(1.0, 1.2),
            row(f64::NAN, 2.0),   // excluded: DV not finite
            row(3.0, f64::NAN),   // excluded: PRED not finite
            row(5.0, 4.8),
        ]);
        let (pts, lo, hi) = dv_vs_pred_points_and_bounds(&data);

        assert_eq!(pts.len(), 2, "only the two fully-finite rows should be plotted");
        assert_eq!(pts[0], [1.2, 1.0]);
        assert_eq!(pts[1], [4.8, 5.0]);

        // dv_pred_range() spans [1.0, 5.0] (min/max across DV, PRED, IPRED
        // over all rows, including the ones excluded from `pts` above);
        // bounds must pad outward from that, not just from the plotted points.
        assert!(lo < 1.0, "lower bound should be padded below the data minimum");
        assert!(hi > 5.0, "upper bound should be padded above the data maximum");
    }
}

fn show_compare_dialog(ctx: &egui::Context, state: &mut AppState) {
    let (stem_a, stem_b) = match (&state.ui.compare_a, &state.ui.compare_b) {
        (Some(a), Some(b)) => (a.clone(), b.clone()),
        _ => return,
    };

    // Extract fit data before entering closures to avoid borrow conflicts.
    let fit_a = state.workspace.models.iter()
        .find(|m| m.model.stem == stem_a)
        .and_then(|m| m.fit.clone());
    let fit_b = state.workspace.models.iter()
        .find(|m| m.model.stem == stem_b)
        .and_then(|m| m.fit.clone());
    let (fit_a, fit_b) = match (fit_a, fit_b) {
        (Some(a), Some(b)) => (a, b),
        _ => return,
    };

    // Lazy-load GOF prediction data for both models, cached by the
    // (stem_a, stem_b) pairing so each .fitrx is only read once per pairing
    // rather than on every frame the dialog is open (mirrors eval_tab's
    // `eval_loaded_stem` caching pattern).
    if state.ui.compare_gof_loaded_for.as_ref() != Some(&(stem_a.clone(), stem_b.clone())) {
        let fitrx_a = state.workspace.models.iter()
            .find(|m| m.model.stem == stem_a)
            .and_then(|m| m.fitrx_path.clone());
        let fitrx_b = state.workspace.models.iter()
            .find(|m| m.model.stem == stem_b)
            .and_then(|m| m.fitrx_path.clone());
        state.ui.compare_gof_a = fitrx_a.as_deref()
            .and_then(|p| crate::io::fitrx::read_predictions(p).ok().flatten());
        state.ui.compare_gof_b = fitrx_b.as_deref()
            .and_then(|p| crate::io::fitrx::read_predictions(p).ok().flatten());
        state.ui.compare_gof_loaded_for = Some((stem_a.clone(), stem_b.clone()));
    }
    let gof_a = state.ui.compare_gof_a.clone();
    let gof_b = state.ui.compare_gof_b.clone();

    let is_dark = ctx.style().visuals.dark_mode;
    let title = format!("Compare  {stem_a}  vs  {stem_b}");
    let mut close = false;

    // Real OS viewport, not an in-window `egui::Window` — see
    // `show_compare_picker` above for why (same "can't close it, not fully
    // visible" report applies here too, and even more so now that this
    // dialog includes the GOF-comparison plots).
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("compare_dialog"),
        egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size(egui::vec2(720.0, 680.0))
            .with_resizable(true)
            .with_min_inner_size(egui::vec2(480.0, 360.0)),
        |ctx, _class| {
        if is_dark { theme::apply_dark(ctx); } else { theme::apply_light(ctx); }

        if ctx.input(|i| i.viewport().close_requested()) {
            close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            let dark = ui.visuals().dark_mode;
            let dim  = theme::fg2(dark);

            // OFV and AIC are shown as plain per-model rows at the end of the
            // table below (no delta computed for either) — see the "FIT
            // STATISTICS" section after SIGMA.

            // ── Parameter comparison table ────────────────────────────────────
            egui::ScrollArea::both().auto_shrink([false; 2]).show(ui, |ui| {
                egui::Grid::new("compare_grid")
                    .num_columns(7)
                    .spacing([10.0, 4.0])
                    .min_col_width(50.0)
                    .striped(true)
                    .show(ui, |ui| {
                        // Header.
                        let h = |ui: &mut egui::Ui, s: &str| {
                            ui.label(egui::RichText::new(s).color(dim).size(10.0).strong());
                        };
                        h(ui, "PARAM");
                        h(ui, &format!("{stem_a} est."));
                        h(ui, &format!("{stem_a} RSE%"));
                        h(ui, &format!("{stem_b} est."));
                        h(ui, &format!("{stem_b} RSE%"));
                        h(ui, "Δ est.");
                        h(ui, "Δ %");
                        ui.end_row();

                        let fg = theme::fg(dark);

                        // THETA section.
                        // Plain ASCII dashes, not the "──" box-drawing glyph
                        // (U+2500) — that's not covered by the bundled
                        // default font and rendered as tofu boxes (reported).
                        for _ in 0..7 { ui.label(egui::RichText::new("-- THETA --").color(dim).size(10.0)); }
                        ui.end_row();
                        compare_param_rows(ui, fg, dark,
                            &fit_a.theta_names, &fit_b.theta_names,
                            &fit_a.theta, &fit_b.theta,
                            &fit_a.se_theta, &fit_b.se_theta);

                        // OMEGA diagonal section.
                        if fit_a.n_eta > 0 || fit_b.n_eta > 0 {
                            for _ in 0..7 { ui.label(egui::RichText::new("-- OMEGA (diag) --").color(dim).size(10.0)); }
                            ui.end_row();
                            let oa: Vec<f64> = (0..fit_a.n_eta).filter_map(|i| fit_a.omega_value(i,i)).collect();
                            let ob: Vec<f64> = (0..fit_b.n_eta).filter_map(|i| fit_b.omega_value(i,i)).collect();
                            compare_param_rows(ui, fg, dark,
                                &fit_a.omega_names, &fit_b.omega_names,
                                &oa, &ob, &fit_a.se_omega, &fit_b.se_omega);
                        }

                        // SIGMA section.
                        if !fit_a.sigma.is_empty() || !fit_b.sigma.is_empty() {
                            for _ in 0..7 { ui.label(egui::RichText::new("-- SIGMA --").color(dim).size(10.0)); }
                            ui.end_row();
                            compare_param_rows(ui, fg, dark,
                                &fit_a.sigma_names, &fit_b.sigma_names,
                                &fit_a.sigma, &fit_b.sigma,
                                &fit_a.se_sigma, &fit_b.se_sigma);
                        }

                        // Fit statistics — plain per-model values, no delta
                        // (OFV/AIC aren't per-parameter estimates, so RSE%
                        // and the Δ columns don't apply; left blank).
                        for _ in 0..7 { ui.label(egui::RichText::new("-- FIT STATISTICS --").color(dim).size(10.0)); }
                        ui.end_row();
                        for (row_label, val_a, val_b) in [
                            ("OFV", format!("{:.3}", fit_a.ofv), format!("{:.3}", fit_b.ofv)),
                            ("AIC", format!("{:.2}", fit_a.aic), format!("{:.2}", fit_b.aic)),
                        ] {
                            ui.label(egui::RichText::new(row_label).color(fg).size(11.0));
                            ui.label(egui::RichText::new(val_a).color(fg).size(11.0));
                            ui.label(""); // RSE% — n/a for OFV/AIC
                            ui.label(egui::RichText::new(val_b).color(fg).size(11.0));
                            ui.label(""); // RSE% — n/a for OFV/AIC
                            ui.label(""); // Δ est. — deliberately not computed
                            ui.label(""); // Δ % — deliberately not computed
                            ui.end_row();
                        }
                    });

                // ── GOF comparison ─────────────────────────────────────────────
                // Reuses eval_tab's scatter_with_loess as-is (see its
                // `pub(crate)` visibility bump) rather than duplicating the
                // scatter+LOESS plotting logic here.
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);
                ui.label(egui::RichText::new("GOF Comparison — DV vs PRED").color(dim).size(11.0).strong());
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let avail    = ui.available_width();
                    let half_w   = (avail / 2.0 - 6.0).max(150.0);
                    let plot_h   = 220.0;
                    let pt_col   = if dark { egui::Color32::from_rgba_unmultiplied(76, 138, 255, 200) }
                                   else    { egui::Color32::from_rgba_unmultiplied(30, 90, 210, 180) };
                    let ref_col  = if dark { egui::Color32::from_gray(120) } else { egui::Color32::from_gray(160) };
                    let loess_col = theme::ORANGE;

                    for (label, data) in [(&stem_a, &gof_a), (&stem_b, &gof_b)] {
                        ui.vertical(|ui| {
                            ui.set_width(half_w);
                            match data {
                                Some(d) if !d.rows.is_empty() => {
                                    let (pts, lo, hi) = dv_vs_pred_points_and_bounds(d);
                                    crate::ui::eval_tab::scatter_with_loess(
                                        ui, &format!("compare_gof_{label}"), label,
                                        "PRED", "DV", half_w, plot_h, &pts,
                                        pt_col, ref_col, loess_col, false,
                                        crate::ui::eval_tab::PlotKind::Identity { lo, hi },
                                    );
                                }
                                _ => {
                                    ui.set_height(plot_h);
                                    ui.centered_and_justified(|ui| {
                                        ui.label(egui::RichText::new(format!("{label}: no prediction data"))
                                            .color(theme::fg3(dark)).size(11.0));
                                    });
                                }
                            }
                        });
                    }
                });
            });

            ui.add_space(10.0);
            if ui.button("Close").clicked() {
                close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        },
    );

    if close {
        state.ui.compare_a = None;
        state.ui.compare_b = None;
    }
}

/// Render one block of parameter comparison rows inside a 7-column Grid.
fn compare_param_rows(
    ui:       &mut egui::Ui,
    fg:       egui::Color32,
    dark:     bool,
    names_a:  &[String],
    names_b:  &[String],
    ests_a:   &[f64],
    ests_b:   &[f64],
    ses_a:    &[f64],
    ses_b:    &[f64],
) {
    for (i, name) in names_a.iter().enumerate() {
        let est_a = ests_a.get(i).copied().unwrap_or(f64::NAN);
        let se_a  = ses_a.get(i).copied().unwrap_or(f64::NAN);
        let rse_a = if est_a != 0.0 && se_a.is_finite() { (se_a/est_a).abs()*100.0 } else { f64::NAN };

        let (est_b, rse_b) = names_b.iter().position(|n| n == name)
            .map(|j| {
                let eb = ests_b.get(j).copied().unwrap_or(f64::NAN);
                let sb = ses_b.get(j).copied().unwrap_or(f64::NAN);
                let rb = if eb != 0.0 && sb.is_finite() { (sb/eb).abs()*100.0 } else { f64::NAN };
                (eb, rb)
            })
            .unwrap_or((f64::NAN, f64::NAN));

        let d_abs = est_b - est_a;
        let d_pct = if est_a.abs() > 1e-10 { d_abs / est_a.abs() * 100.0 } else { f64::NAN };
        let d_col = if d_pct.abs() > 20.0 { theme::ORANGE } else { theme::fg2(dark) };

        ui.label(egui::RichText::new(name).monospace().size(11.0).color(fg));
        ui.label(egui::RichText::new(fmt_sig4(est_a)).monospace().size(11.0).color(fg));
        ui.label(egui::RichText::new(fmt_f64_1dp(rse_a)).size(11.0).color(rse_color(rse_a)));
        ui.label(egui::RichText::new(fmt_sig4(est_b)).monospace().size(11.0).color(fg));
        ui.label(egui::RichText::new(fmt_f64_1dp(rse_b)).size(11.0).color(rse_color(rse_b)));
        ui.label(egui::RichText::new(fmt_sig4(d_abs)).monospace().size(11.0).color(d_col));
        ui.label(egui::RichText::new(
            if d_pct.is_finite() { format!("{d_pct:+.1}%") } else { "—".to_string() })
            .size(11.0).color(d_col));
        ui.end_row();
    }
}

