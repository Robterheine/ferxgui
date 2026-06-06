use eframe::egui;
use egui_extras::{Column, TableBuilder};

use crate::app::theme;
use crate::domain::JobStatus;
use crate::state::{AppState, HistorySortCol, Tab};

// ── Public entry point ────────────────────────────────────────────────────────

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;

    // ── Top bar ───────────────────────────────────────────────────────────
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);

        let fg2 = if dark { theme::FG2 } else { egui::Color32::from_gray(80) };
        ui.label(egui::RichText::new("Run History").strong().size(14.0).color(fg2));

        ui.add_space(16.0);

        // Filter box.
        let filter_resp = ui.add(
            egui::TextEdit::singleline(&mut state.ui.history_filter)
                .hint_text("Filter…")
                .desired_width(180.0),
        );
        if filter_resp.changed() {
            state.ui.history_selected = None;
        }
        if !state.ui.history_filter.is_empty() {
            ui.add_space(4.0);
            let dim = if dark { theme::FG3 } else { egui::Color32::from_gray(140) };
            if ui
                .add(egui::Button::new(egui::RichText::new("✕").size(10.0).color(dim)).frame(false))
                .on_hover_text("Clear filter")
                .clicked()
            {
                state.ui.history_filter.clear();
                state.ui.history_selected = None;
            }
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            let n = state.run.run_history.len();
            if n > 0 {
                let warn_color = if dark { theme::RED } else { egui::Color32::from_rgb(180, 40, 40) };
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("Clear All").size(12.0).color(warn_color),
                        )
                        .stroke(egui::Stroke::new(1.0, warn_color))
                        .fill(egui::Color32::TRANSPARENT),
                    )
                    .on_hover_text("Remove all run history entries")
                    .clicked()
                {
                    state.run.run_history.clear();
                    state.ui.history_selected = None;
                    state.run.save_history(state.workspace.app_dir.as_ref());
                }
                ui.add_space(8.0);
            }
            let count_color = if dark { theme::FG3 } else { egui::Color32::from_gray(140) };
            let visible = filtered_count(&state.ui.history_filter, &state.run.run_history);
            let label = if visible == n {
                format!("{} run{}", n, if n == 1 { "" } else { "s" })
            } else {
                format!("{} / {} run{}", visible, n, if n == 1 { "" } else { "s" })
            };
            ui.label(egui::RichText::new(label).size(11.0).color(count_color));
        });
    });

    ui.add_space(4.0);

    if state.run.run_history.is_empty() {
        show_empty(ui, dark);
        return;
    }

    // ── Build sorted + filtered index ─────────────────────────────────────
    let indices = sorted_filtered_indices(state);

    // ── Detail panel height (shown when a row is selected) ────────────────
    let detail_h = if state.ui.history_selected.is_some() { 96.0 } else { 0.0 };
    let table_h  = (ui.available_height() - detail_h).max(40.0);

    // ── Table ─────────────────────────────────────────────────────────────
    let text_h = 20.0;
    let header_h = 22.0;

    let selected_row_idx = state.ui.history_selected;
    let sort_col = state.ui.history_sort_col;
    let sort_asc = state.ui.history_sort_asc;

    // Snapshot data needed for the table (avoids borrow issues with state).
    let rows_snap: Vec<RowData> = indices
        .iter()
        .map(|&i| build_row(i, state))
        .collect();

    let mut clicked_row: Option<usize> = None; // index into indices[]
    let mut navigate_to_stem: Option<String> = None;
    let mut rerun_stem: Option<String> = None;

    egui::ScrollArea::vertical()
        .id_salt("history_outer")
        .max_height(table_h)
        .show(ui, |ui| {
            TableBuilder::new(ui)
                .striped(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::exact(22.0))    // status dot
                .column(Column::remainder().clip(true))  // model stem
                .column(Column::exact(72.0))    // method
                .column(Column::exact(90.0))    // OFV
                .column(Column::exact(62.0))    // duration
                .column(Column::exact(150.0))   // started
                .header(header_h, |mut header| {
                    // Status (no label — just the dot column)
                    header.col(|_ui| {});

                    col_header(&mut header, "Model",    HistorySortCol::Model,    sort_col, sort_asc, dark);
                    col_header(&mut header, "Method",   HistorySortCol::Method,   sort_col, sort_asc, dark);
                    col_header(&mut header, "OFV",      HistorySortCol::Ofv,      sort_col, sort_asc, dark);
                    col_header(&mut header, "Duration", HistorySortCol::Duration, sort_col, sort_asc, dark);
                    col_header(&mut header, "Started",  HistorySortCol::Started,  sort_col, sort_asc, dark);
                })
                .body(|mut body| {
                    for (view_idx, row) in rows_snap.iter().enumerate() {
                        let is_selected = selected_row_idx == Some(view_idx);
                        let row_fill = if is_selected {
                            theme::ACCENT.linear_multiply(0.25)
                        } else {
                            egui::Color32::TRANSPARENT
                        };

                        body.row(text_h, |mut r| {
                            // Status dot.
                            r.col(|ui| {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(14.0, 14.0), egui::Sense::hover(),
                                );
                                ui.painter().circle_filled(rect.center(), 5.0, row.dot_color);
                            });

                            // Model stem.
                            r.col(|ui| {
                                if is_selected {
                                    ui.painter().rect_filled(
                                        ui.available_rect_before_wrap(),
                                        0.0,
                                        row_fill,
                                    );
                                }
                                let stem_color = if dark { theme::FG } else { ui.visuals().text_color() };
                                let resp = ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&row.stem)
                                            .size(12.0)
                                            .color(stem_color),
                                    )
                                    .truncate()
                                    .sense(egui::Sense::click()),
                                );
                                if resp.clicked() {
                                    clicked_row = Some(view_idx);
                                }
                            });

                            // Method.
                            r.col(|ui| {
                                let fg3 = if dark { theme::FG2 } else { egui::Color32::from_gray(80) };
                                ui.label(egui::RichText::new(&row.method).size(11.0).color(fg3));
                            });

                            // OFV.
                            r.col(|ui| {
                                let fg3 = if dark { theme::FG2 } else { egui::Color32::from_gray(80) };
                                ui.label(egui::RichText::new(&row.ofv_str).size(11.0).color(fg3).monospace());
                            });

                            // Duration.
                            r.col(|ui| {
                                let fg3 = if dark { theme::FG2 } else { egui::Color32::from_gray(80) };
                                ui.label(egui::RichText::new(&row.duration_str).size(11.0).color(fg3).monospace());
                            });

                            // Started.
                            r.col(|ui| {
                                let fg3 = if dark { theme::FG3 } else { egui::Color32::from_gray(120) };
                                ui.label(egui::RichText::new(&row.started_str).size(11.0).color(fg3));
                            });
                        });
                    }
                });
        });

    // Apply click to state (after borrow ends).
    if let Some(view_idx) = clicked_row {
        if state.ui.history_selected == Some(view_idx) {
            state.ui.history_selected = None; // toggle off
        } else {
            state.ui.history_selected = Some(view_idx);
        }
    }

    // ── Column-header sort toggle (read from egui memory) ─────────────────
    // We handle sort clicks via id_salt — see col_header().
    for (col, id_str) in [
        (HistorySortCol::Model,    "hist_sort_Model"),
        (HistorySortCol::Method,   "hist_sort_Method"),
        (HistorySortCol::Ofv,      "hist_sort_OFV"),
        (HistorySortCol::Duration, "hist_sort_Duration"),
        (HistorySortCol::Started,  "hist_sort_Started"),
    ] {
        let id = egui::Id::new(id_str);
        if ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(false) {
            ui.ctx().data_mut(|d| d.remove::<bool>(id));
            if state.ui.history_sort_col == col {
                state.ui.history_sort_asc = !state.ui.history_sort_asc;
            } else {
                state.ui.history_sort_col = col;
                state.ui.history_sort_asc = matches!(col, HistorySortCol::Model | HistorySortCol::Method);
            }
            state.ui.history_selected = None;
        }
    }

    // ── Detail panel ──────────────────────────────────────────────────────
    if let Some(view_idx) = state.ui.history_selected {
        if let Some(&hist_idx) = indices.get(view_idx) {
            if let Some(rec) = state.run.run_history.get(hist_idx) {
                let rec = rec.clone(); // clone to end the borrow
                show_detail(ui, &rec, dark, &mut navigate_to_stem, &mut rerun_stem);
            }
        }
    }

    // ── Handle navigation / re-run requests ───────────────────────────────
    if let Some(stem) = navigate_to_stem {
        if let Some(idx) = state.workspace.models.iter().position(|m| m.model.stem == stem) {
            state.ui.selected_model = Some(idx);
            state.ui.active_tab = Tab::Models;
        }
    }
    if let Some(_stem) = rerun_stem {
        // TODO: wire up re-run from history once run queue is implemented.
    }
}

// ── Column header with sort indicator ────────────────────────────────────────

fn col_header(
    header: &mut egui_extras::TableRow<'_, '_>,
    label: &str,
    col: HistorySortCol,
    active: HistorySortCol,
    asc: bool,
    dark: bool,
) {
    let id_str = format!("hist_sort_{}", label);
    header.col(|ui| {
        let is_active = col == active;
        let fg = if dark { theme::FG2 } else { egui::Color32::from_gray(80) };
        let active_fg = if dark { theme::FG } else { egui::Color32::from_gray(20) };
        let text = if is_active {
            let arrow = if asc { " ▲" } else { " ▼" };
            format!("{}{}", label, arrow)
        } else {
            label.to_string()
        };
        let resp = ui.add(
            egui::Button::new(
                egui::RichText::new(text)
                    .size(11.0)
                    .strong()
                    .color(if is_active { active_fg } else { fg }),
            )
            .frame(false),
        );
        if resp.clicked() {
            // Signal via egui memory — read back in the main loop.
            let id = egui::Id::new(id_str);
            ui.ctx().data_mut(|d| d.insert_temp(id, true));
        }
    });
}

// ── Row data snapshot ─────────────────────────────────────────────────────────

struct RowData {
    stem:         String,
    method:       String,
    ofv_str:      String,
    ofv_val:      f64,   // for sorting
    duration_str: String,
    duration_val: f64,   // for sorting
    started_str:  String,
    dot_color:    egui::Color32,
    _status_ord:  u8,    // reserved for future status-column sort
}

fn build_row(hist_idx: usize, state: &AppState) -> RowData {
    let rec = &state.run.run_history[hist_idx];

    let dot_color = match &rec.status {
        JobStatus::Completed => theme::GREEN,
        JobStatus::Failed    => theme::RED,
        JobStatus::Cancelled => theme::ORANGE,
        JobStatus::Running   => theme::ACCENT,
    };
    let status_ord = match &rec.status {
        JobStatus::Completed => 0,
        JobStatus::Failed    => 1,
        _                    => 2,
    };

    // OFV: look up the live model entry by stem.
    let ofv_val = state.workspace.models.iter()
        .find(|m| m.model.stem == rec.model_stem)
        .and_then(|m| m.fit.as_ref())
        .map(|f| f.ofv)
        .unwrap_or(f64::NAN);
    let ofv_str = if ofv_val.is_finite() {
        format!("{:.2}", ofv_val)
    } else {
        "—".to_string()
    };

    let duration_val = rec.duration_secs.unwrap_or(f64::NAN);
    let duration_str = rec.duration_secs
        .map(|d| {
            let s = d as u64;
            if s < 60 { format!("{:02}s", s) }
            else { format!("{}:{:02}", s / 60, s % 60) }
        })
        .unwrap_or_else(|| "—".to_string());

    let method = rec.method.clone().unwrap_or_else(|| "—".to_string());

    // Parse ISO-8601 started string into something readable: strip the T, trim seconds.
    let started_str = format_timestamp(&rec.started);

    RowData {
        stem:         rec.model_stem.clone(),
        method,
        ofv_str,
        ofv_val,
        duration_str,
        duration_val,
        started_str,
        dot_color,
        _status_ord: status_ord,
    }
}

fn format_timestamp(iso: &str) -> String {
    // "2024-01-15T14:32:07.123456789+00:00" → "2024-01-15  14:32"
    let s = iso.replace('T', "  ");
    let s = s.trim_end_matches(|c: char| c == 'Z' || c == '+' || c.is_numeric() || c == ':');
    // Drop seconds if present (keep HH:MM).
    let parts: Vec<&str> = s.splitn(2, "  ").collect();
    if parts.len() == 2 {
        let time = parts[1].trim();
        // Take at most HH:MM.
        let hhmm: String = time.chars().take(5).collect();
        format!("{}  {}", parts[0].trim(), hhmm)
    } else {
        s.to_string()
    }
}

// ── Sorted + filtered index builder ──────────────────────────────────────────

fn sorted_filtered_indices(state: &AppState) -> Vec<usize> {
    let filter = state.ui.history_filter.to_lowercase();

    // Build row data snapshots for sorting.
    let snaps: Vec<(usize, RowData)> = state.run.run_history.iter()
        .enumerate()
        .filter(|(_, r)| {
            if filter.is_empty() { return true; }
            r.model_stem.to_lowercase().contains(&filter)
                || r.method.as_deref().unwrap_or("").to_lowercase().contains(&filter)
                || r.status.label().to_lowercase().contains(&filter)
        })
        .map(|(i, _)| (i, build_row(i, state)))
        .collect();

    let col = state.ui.history_sort_col;
    let asc = state.ui.history_sort_asc;

    let mut indexed: Vec<usize> = snaps.iter().map(|(i, _)| *i).collect();
    let snap_map: std::collections::HashMap<usize, usize> =
        snaps.iter().enumerate().map(|(vi, (hi, _))| (*hi, vi)).collect();

    indexed.sort_by(|&a, &b| {
        let ra = &snaps[snap_map[&a]].1;
        let rb = &snaps[snap_map[&b]].1;
        let ord = match col {
            HistorySortCol::Model    => ra.stem.cmp(&rb.stem),
            HistorySortCol::Method   => ra.method.cmp(&rb.method),
            HistorySortCol::Ofv      => ra.ofv_val.partial_cmp(&rb.ofv_val)
                                           .unwrap_or(std::cmp::Ordering::Equal),
            HistorySortCol::Duration => ra.duration_val.partial_cmp(&rb.duration_val)
                                           .unwrap_or(std::cmp::Ordering::Equal),
            HistorySortCol::Started  => {
                let ha = &state.run.run_history[a].started;
                let hb = &state.run.run_history[b].started;
                ha.cmp(hb)
            }
        };
        if asc { ord } else { ord.reverse() }
    });

    indexed
}

fn filtered_count(filter: &str, history: &[crate::domain::RunRecord]) -> usize {
    if filter.is_empty() { return history.len(); }
    let f = filter.to_lowercase();
    history.iter().filter(|r| {
        r.model_stem.to_lowercase().contains(&f)
            || r.method.as_deref().unwrap_or("").to_lowercase().contains(&f)
            || r.status.label().to_lowercase().contains(&f)
    }).count()
}

// ── Detail panel ─────────────────────────────────────────────────────────────

fn show_detail(
    ui: &mut egui::Ui,
    rec: &crate::domain::RunRecord,
    dark: bool,
    navigate_to_stem: &mut Option<String>,
    _rerun_stem: &mut Option<String>,
) {
    let border = if dark { theme::BG3 } else { egui::Color32::from_gray(210) };
    ui.separator();

    egui::Frame::new()
        .fill(if dark { egui::Color32::from_rgb(0x1e, 0x1e, 0x28) } else { egui::Color32::from_gray(248) })
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            // Header row: stem + status badge + action buttons.
            ui.horizontal(|ui| {
                let fg = if dark { theme::FG } else { ui.visuals().text_color() };
                ui.label(egui::RichText::new(&rec.model_stem).strong().size(13.0).color(fg));

                ui.add_space(8.0);
                let (dot, label) = match &rec.status {
                    JobStatus::Completed => (theme::GREEN,  "Completed"),
                    JobStatus::Failed    => (theme::RED,    "Failed"),
                    JobStatus::Cancelled => (theme::ORANGE, "Cancelled"),
                    JobStatus::Running   => (theme::ACCENT, "Running"),
                };
                let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 4.0, dot);
                let dim = if dark { theme::FG2 } else { egui::Color32::from_gray(90) };
                ui.label(egui::RichText::new(label).size(11.0).color(dim));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(4.0);
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new("Go to model →").size(11.0))
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::new(1.0, border)),
                        )
                        .on_hover_text("Switch to Models tab and select this model")
                        .clicked()
                    {
                        *navigate_to_stem = Some(rec.model_stem.clone());
                    }
                });
            });

            ui.add_space(4.0);

            // Command line.
            let mono_fg = if dark { theme::FG2 } else { egui::Color32::from_gray(60) };
            ui.horizontal(|ui| {
                let dim = if dark { theme::FG3 } else { egui::Color32::from_gray(130) };
                ui.label(egui::RichText::new("cmd").size(10.0).color(dim));
                ui.add_space(6.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&rec.command)
                            .monospace()
                            .size(11.0)
                            .color(mono_fg),
                    )
                    .truncate(),
                );
            });

            // Directory.
            ui.horizontal(|ui| {
                let dim = if dark { theme::FG3 } else { egui::Color32::from_gray(130) };
                ui.label(egui::RichText::new("dir").size(10.0).color(dim));
                ui.add_space(6.0);
                let dir_str = rec.directory.to_string_lossy();
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(dir_str.as_ref())
                            .monospace()
                            .size(11.0)
                            .color(mono_fg),
                    )
                    .truncate(),
                );
                ui.add_space(4.0);
                if ui
                    .add(egui::Button::new(egui::RichText::new("↗").size(11.0)).frame(false))
                    .on_hover_text("Open in Finder / Explorer")
                    .clicked()
                {
                    let _ = open::that(&rec.directory);
                }
            });
        });
}

// ── Empty state ───────────────────────────────────────────────────────────────

fn show_empty(ui: &mut egui::Ui, dark: bool) {
    let dim = if dark { theme::FG3 } else { egui::Color32::from_gray(160) };
    ui.centered_and_justified(|ui| {
        ui.label(
            egui::RichText::new("No runs yet.\n\nStart a run from the Models tab → Run pill.")
                .color(dim)
                .size(13.0),
        );
    });
}
