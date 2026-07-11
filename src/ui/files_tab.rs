/// File-browser tab — two-pane layout mirroring NMGUI2's FileExplorerTab.
///
/// Layout
/// ------
/// [←]  root / subdir      [All][.ferx][.csv]… [ext…]
/// ────────────────────────────────────────────────────────────────────────────
/// [ file list (folders first) ] | [ content preview ]
///
/// Behaviour
/// ---------
/// - Filter pills: All / .ferx / .fitrx / .csv / .R / .log / .json / .txt
///   plus a free-text override (clears on directory change).
/// - Single-click file  → load preview in right pane.
/// - Double-click folder → navigate into it (push back-stack).
/// - Double-click file   → open with OS default app.
/// - ← back button       → return to previous directory.
/// - Text files: syntax-highlighted for .ferx, plain monospace otherwise.
/// - CSV files: virtualized table (all rows in memory, only visible cells rendered).
/// - Edit/Save/Discard for both text and CSV.
///   Saving a .ferx file also refreshes the Models tab editor and triggers rescan.
/// - Plot view for CSV: X / Y pickers, unity line, LOESS, color-by column.
use std::path::PathBuf;

use eframe::egui;
use egui_extras::{Column, TableBuilder};
use egui_plot::{Line, Plot, PlotPoints, Points};

use crate::app::theme;
use crate::state::{AppState, FilesViewMode};

// ── Constants ─────────────────────────────────────────────────────────────────

/// Pills shown in the nav bar (order = display order).
const PILLS: &[&str] = &["ferx", "fitrx", "csv", "r", "log", "json", "txt"];

/// Extensions treated as binary — shown as "no preview" placeholder.
const BINARY_EXTS: &[&str] = &[
    "fitrx", "png", "jpg", "jpeg", "gif", "bmp", "tiff", "tif", "ico",
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx",
    "zip", "tar", "gz", "bz2", "xz", "7z", "rar",
    "so", "dylib", "dll", "exe", "mp3", "mp4", "wav", "avi", "mov",
];

/// Extensions opened as a spreadsheet table + optional scatter plot.
const TABLE_EXTS: &[&str] = &["csv"];

/// Color palette for categorical color-by (cycles beyond 8 fall back to gray).
const CAT_COLORS: [egui::Color32; 8] = [
    egui::Color32::from_rgb(0x54, 0x7a, 0xf5),
    egui::Color32::from_rgb(0xe0, 0x5a, 0x52),
    egui::Color32::from_rgb(0x2b, 0xad, 0x6e),
    egui::Color32::from_rgb(0xf5, 0xa6, 0x23),
    egui::Color32::from_rgb(0x9b, 0x59, 0xb6),
    egui::Color32::from_rgb(0x1a, 0xbc, 0x9c),
    egui::Color32::from_rgb(0xe9, 0x1e, 0x63),
    egui::Color32::from_rgb(0x97, 0x9b, 0xa8),
];

// ── Public entry point ────────────────────────────────────────────────────────

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    show_unsaved_nav_dialog(ui.ctx(), state);

    // Bootstrap: sync cwd to working directory on first entry or when not set.
    if state.ui.files_cwd.is_none() {
        state.ui.files_cwd = state.workspace.directory.clone();
    }

    // Rebuild directory listing whenever cwd changes.
    if state.ui.files_entries_dir != state.ui.files_cwd {
        rebuild_entries(state);
    }

    let dark = ui.visuals().dark_mode;

    // No working directory configured at all.
    if state.ui.files_cwd.is_none() {
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new(
                "No working directory set.\nConfigure one in Settings.")
                .color(theme::fg3(dark)).size(13.0));
        });
        return;
    }

    show_nav_bar(ui, state, dark);
    ui.separator();

    egui::SidePanel::left("files_left_panel")
        .resizable(true)
        .default_width(300.0)
        .min_width(160.0)
        .show_inside(ui, |ui| {
            show_file_list(ui, state, dark);
        });

    show_preview(ui, state, dark);
}

// ── Nav bar ───────────────────────────────────────────────────────────────────

fn show_nav_bar(ui: &mut egui::Ui, state: &mut AppState, dark: bool) {
    ui.horizontal_wrapped(|ui| {
        // ← Back button.
        let can_back = !state.ui.files_back_stack.is_empty();
        if ui.add_enabled(
            can_back,
            egui::Button::new("←").min_size(egui::vec2(26.0, 22.0)),
        )
        .on_hover_text("Go to previous directory")
        .clicked()
        {
            if let Some(prev) = state.ui.files_back_stack.pop() {
                state.ui.files_cwd      = Some(prev);
                state.ui.files_selected = None;
                state.ui.files_view_mode = FilesViewMode::Empty;
            }
        }

        // Breadcrumb.
        let crumb = breadcrumb_text(
            state.ui.files_cwd.as_deref(),
            state.workspace.directory.as_deref(),
        );
        ui.label(egui::RichText::new(crumb).color(theme::fg2(dark)).size(11.0).monospace());

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(4.0);

        // "All" pill.
        let all_active = state.ui.files_active_exts.is_empty();
        if ui.add(pill_btn("All", all_active, dark)).clicked() {
            state.ui.files_active_exts.clear();
            state.ui.files_ext_input.clear();
            save_filter(state);
            rebuild_entries(state);
        }

        // Extension pills.
        let snapshot: Vec<bool> = PILLS.iter()
            .map(|&e| state.ui.files_active_exts.contains(e))
            .collect();
        for (i, &ext) in PILLS.iter().enumerate() {
            let on = snapshot[i];
            if ui.add(pill_btn(&format!(".{ext}"), on, dark)).clicked() {
                if on { state.ui.files_active_exts.remove(ext); }
                else  { state.ui.files_active_exts.insert(ext.to_string()); }
                state.ui.files_ext_input.clear();
                save_filter(state);
                rebuild_entries(state);
            }
        }

        // Free-text extension override.
        let prev = state.ui.files_ext_input.clone();
        ui.add(
            egui::TextEdit::singleline(&mut state.ui.files_ext_input)
                .desired_width(60.0)
                .hint_text("ext…"),
        )
        .on_hover_text("Type an extension to filter (e.g. r or .py); overrides pills");
        if state.ui.files_ext_input != prev {
            rebuild_entries(state);
        }
    });
}

fn pill_btn(label: &str, active: bool, dark: bool) -> egui::Button<'static> {
    let label = label.to_owned();
    egui::Button::new(
        egui::RichText::new(label).size(11.0)
            .color(if active { egui::Color32::WHITE } else { theme::fg2(dark) }),
    )
    .fill(if active { theme::ACCENT } else { egui::Color32::TRANSPARENT })
    .min_size(egui::vec2(0.0, 20.0))
}

// ── File list (left pane) ─────────────────────────────────────────────────────

fn show_file_list(ui: &mut egui::Ui, state: &mut AppState, dark: bool) {
    let n = state.ui.files_entries.len();

    // Snapshot entry data to avoid borrow conflicts inside the table closure.
    let snapshot: Vec<(String, bool, u64, Option<std::time::SystemTime>, PathBuf)> =
        state.ui.files_entries.iter()
            .map(|e| (e.name.clone(), e.is_dir, e.size, e.modified, e.path.clone()))
            .collect();

    let selected = state.ui.files_selected.clone();

    let mut nav_into:   Option<PathBuf> = None;
    let mut open_file:  Option<PathBuf> = None;
    let mut select:     Option<PathBuf> = None;
    let mut ctx_reveal: Option<PathBuf> = None;
    let mut ctx_copy:   Option<PathBuf> = None;

    egui::ScrollArea::horizontal().show(ui, |ui| {
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(200.0).resizable(true).clip(true))
            .column(Column::initial(72.0).clip(true))
            .column(Column::remainder().clip(true))
            .header(20.0, |mut h| {
                for lbl in ["Name", "Size", "Modified"] {
                    h.col(|ui| {
                        ui.label(egui::RichText::new(lbl).size(11.0)
                            .color(theme::fg2(dark)).strong());
                    });
                }
            })
            .body(|body| {
                body.rows(22.0, n, |mut row| {
                    let i = row.index();
                    let (ref name, is_dir, size, modified, ref path) = snapshot[i];
                    let is_sel = selected.as_ref() == Some(path);
                    row.set_selected(is_sel);

                    row.col(|ui| {
                        let prefix = if is_dir { "📁  " } else { "" };
                        let text = egui::RichText::new(format!("{prefix}{name}"))
                            .size(12.0)
                            .color(theme::fg(dark));
                        let text = if is_dir { text.strong() } else { text };
                        let resp = ui.add(
                            egui::Label::new(text).sense(egui::Sense::click()).truncate()
                        );
                        if resp.double_clicked() {
                            if is_dir { nav_into  = Some(path.clone()); }
                            else       { open_file = Some(path.clone()); }
                        } else if resp.clicked() && !is_dir {
                            select = Some(path.clone());
                        }
                    });
                    row.col(|ui| {
                        let s = if is_dir { "—".to_string() } else { fmt_size(size) };
                        ui.label(egui::RichText::new(s).size(11.0).color(theme::fg2(dark)));
                    });
                    row.col(|ui| {
                        let s = modified.map(fmt_mtime).unwrap_or_else(|| "—".into());
                        let s = if is_dir { "—".to_string() } else { s };
                        ui.label(egui::RichText::new(s).size(11.0).color(theme::fg3(dark)));
                    });

                    let resp = row.response();
                    if resp.double_clicked() {
                        if is_dir { nav_into  = Some(path.clone()); }
                        else       { open_file = Some(path.clone()); }
                    } else if resp.clicked()
                        && !is_dir { select = Some(path.clone()); }

                    let p = path.clone();
                    resp.context_menu(|ui| {
                        if ui.button("Reveal in Finder").clicked() {
                            ctx_reveal = Some(p.clone());
                            ui.close_menu();
                        }
                        if ui.button("Copy path").clicked() {
                            ctx_copy = Some(p.clone());
                            ui.close_menu();
                        }
                    });
                });
            });
    });

    // Apply deferred actions (outside the table borrow).
    if let Some(dir) = nav_into {
        if let Some(cwd) = state.ui.files_cwd.clone() {
            state.ui.files_back_stack.push(cwd);
        }
        state.ui.files_cwd       = Some(dir);
        state.ui.files_selected  = None;
        state.ui.files_view_mode = FilesViewMode::Empty;
    }
    if let Some(path) = open_file  { os_open(&path); }
    if let Some(path) = select {
        if state.ui.files_text_dirty || state.ui.files_csv_dirty {
            state.ui.files_pending_nav = Some(path);
        } else {
            load_file(state, path);
        }
    }
    if let Some(path) = ctx_reveal { reveal_in_finder(&path); }
    if let Some(path) = ctx_copy   {
        ui.ctx().copy_text(path.to_string_lossy().into());
    }
}

// ── Preview pane ──────────────────────────────────────────────────────────────

fn show_preview(ui: &mut egui::Ui, state: &mut AppState, dark: bool) {
    ui.vertical(|ui| {
        show_preview_header(ui, state, dark);
        ui.separator();

        match state.ui.files_view_mode {
            FilesViewMode::Empty  => show_empty_hint(ui, dark),
            FilesViewMode::Binary => show_binary_placeholder(ui, state, dark),
            FilesViewMode::Text   => show_text_view(ui, state, dark),
            FilesViewMode::Table  => show_table_view(ui, state, dark),
            FilesViewMode::Plot   => show_plot_view(ui, state, dark),
        }
    });
}

fn show_preview_header(ui: &mut egui::Ui, state: &mut AppState, dark: bool) {
    ui.horizontal(|ui| {
        let fname = state.ui.files_selected.as_ref()
            .and_then(|p| p.file_name()).and_then(|n| n.to_str())
            .unwrap_or("No file selected");
        ui.label(egui::RichText::new(fname).size(12.0).strong().color(theme::fg(dark)));

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mode = state.ui.files_view_mode;
            match mode {
                FilesViewMode::Text
                    if state.ui.files_text_dirty => {
                        if ui.small_button("Discard").clicked() {
                            if let Some(path) = state.ui.files_selected.clone() {
                                load_file(state, path);
                            }
                        }
                        if ui.add(
                            egui::Button::new(egui::RichText::new("Save").color(egui::Color32::WHITE))
                                .fill(theme::ACCENT).min_size(egui::vec2(0.0, 20.0)),
                        ).clicked() {
                            save_text_file(state);
                        }
                    }
                FilesViewMode::Table => {
                    // Table ↔ Plot switcher.
                    if ui.add(
                        egui::Button::new(egui::RichText::new("Plot").size(11.0))
                            .min_size(egui::vec2(0.0, 20.0)),
                    ).clicked() {
                        state.ui.files_view_mode = FilesViewMode::Plot;
                        state.ui.files_csv_edit_mode = false;
                    }
                    ui.add_space(4.0);
                    if state.ui.files_csv_edit_mode {
                        if ui.small_button("Discard").clicked() {
                            if let Some(path) = state.ui.files_selected.clone() {
                                load_file(state, path);
                            }
                        }
                        if state.ui.files_csv_dirty
                            && ui.add(
                                egui::Button::new(egui::RichText::new("Save")
                                    .color(egui::Color32::WHITE))
                                    .fill(theme::ACCENT).min_size(egui::vec2(0.0, 20.0)),
                            ).clicked() {
                                save_csv_file(state);
                            }
                        if ui.small_button("Done").clicked() {
                            state.ui.files_csv_edit_mode = false;
                            state.ui.files_csv_editing   = None;
                        }
                    } else if ui.small_button("Edit").clicked() {
                        state.ui.files_csv_edit_mode = true;
                    }
                }
                FilesViewMode::Plot
                    if ui.add(
                        egui::Button::new(egui::RichText::new("Table").size(11.0))
                            .min_size(egui::vec2(0.0, 20.0)),
                    ).clicked() => {
                        state.ui.files_view_mode = FilesViewMode::Table;
                    }
                _ => {}
            }
        });
    });
}

// ── Text view ─────────────────────────────────────────────────────────────────

fn show_text_view(ui: &mut egui::Ui, state: &mut AppState, dark: bool) {
    let is_ferx = state.ui.files_text_is_ferx;

    // Show dirty indicator.
    if state.ui.files_text_dirty {
        ui.label(egui::RichText::new("● Unsaved changes").color(theme::ORANGE).size(11.0));
    }

    egui::ScrollArea::both()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            if is_ferx {
                let mut layouter = |ui: &egui::Ui, text: &str, _wrap: f32| {
                    let job = crate::ui::models_tab::highlight_ferx(text, dark);
                    ui.fonts(|f| f.layout_job(job))
                };
                let resp = ui.add(
                    egui::TextEdit::multiline(&mut state.ui.files_text)
                        .font(egui::FontId::monospace(12.0))
                        .desired_rows(40)
                        .desired_width(f32::INFINITY)
                        .layouter(&mut layouter),
                );
                if resp.changed() { state.ui.files_text_dirty = true; }
            } else {
                let resp = ui.add(
                    egui::TextEdit::multiline(&mut state.ui.files_text)
                        .font(egui::FontId::monospace(12.0))
                        .desired_rows(40)
                        .desired_width(f32::INFINITY),
                );
                if resp.changed() { state.ui.files_text_dirty = true; }
            }
        });
}

// ── CSV table view ────────────────────────────────────────────────────────────

fn show_table_view(ui: &mut egui::Ui, state: &mut AppState, dark: bool) {
    let n_rows = state.ui.files_csv_rows.len();
    let n_cols = state.ui.files_csv_headers.len();
    if n_cols == 0 { return; }

    let headers_snap: Vec<String> = state.ui.files_csv_headers.clone();
    let edit_mode = state.ui.files_csv_edit_mode;
    let editing   = state.ui.files_csv_editing;
    let mut new_buf = state.ui.files_csv_edit_buf.clone();

    // Deferred actions — resolved after the immutable row borrow ends.
    let mut deferred_commit: Option<(usize, usize)>         = None;
    let mut deferred_select: Option<(usize, usize, String)> = None;
    // (row_delta, col_delta): Tab=(0,1), Shift-Tab=(0,-1), Enter=(1,0)
    let mut deferred_nav: Option<(i32, i32)> = None;
    let mut deferred_escape = false;

    // Scroll the table to keep the active cell in view on the next frame.
    let scroll_to = editing.map(|(r, _)| r);

    {
        let rows = &state.ui.files_csv_rows;
        let mut tb = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
        if let Some(row) = scroll_to {
            tb = tb.scroll_to_row(row, Some(egui::Align::Center));
        }
        for _ in 0..n_cols {
            tb = tb.column(Column::initial(100.0).resizable(true).clip(true));
        }

        tb.header(22.0, |mut h| {
            for name in &headers_snap {
                h.col(|ui| {
                    ui.label(egui::RichText::new(name).size(11.0)
                        .color(theme::fg2(dark)).strong());
                });
            }
        })
        .body(|body| {
            body.rows(20.0, n_rows, |mut row| {
                let ri = row.index();
                for ci in 0..n_cols {
                    let cell = rows.get(ri).and_then(|r| r.get(ci))
                        .map(String::as_str).unwrap_or("");
                    row.col(|ui| {
                        if edit_mode && editing == Some((ri, ci)) {
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut new_buf)
                                    .desired_width(f32::INFINITY),
                            );
                            if resp.lost_focus() {
                                // Determine why focus was lost to decide navigation.
                                let tab   = ui.input(|i| i.key_pressed(egui::Key::Tab) && !i.modifiers.shift);
                                let stab  = ui.input(|i| i.key_pressed(egui::Key::Tab) && i.modifiers.shift);
                                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                                let esc   = ui.input(|i| i.key_pressed(egui::Key::Escape));
                                if tab        { deferred_nav = Some((0, 1)); }
                                else if stab  { deferred_nav = Some((0, -1)); }
                                else if enter { deferred_nav = Some((1, 0)); }
                                else if esc   { deferred_escape = true; }
                                else          { deferred_commit = Some((ri, ci)); }
                            } else {
                                resp.request_focus();
                            }
                        } else {
                            let resp = ui.add(
                                egui::Label::new(
                                    egui::RichText::new(cell).size(12.0).color(theme::fg(dark))
                                ).sense(egui::Sense::click()).truncate(),
                            );
                            if resp.clicked() && edit_mode {
                                deferred_select = Some((ri, ci, cell.to_string()));
                            }
                        }
                    });
                }
            });
        });
    } // rows borrow ends

    state.ui.files_csv_edit_buf = new_buf;

    if let Some((dr, dc)) = deferred_nav {
        // Commit current value then move to adjacent cell.
        if let Some((r, c)) = editing {
            let val = state.ui.files_csv_edit_buf.clone();
            if let Some(row) = state.ui.files_csv_rows.get_mut(r) {
                if let Some(cell) = row.get_mut(c) { *cell = val; state.ui.files_csv_dirty = true; }
            }
            let (nr, nc) = if dr == 0 && dc == 1 {
                // Tab: right, wrap to next row
                if c + 1 < n_cols { (r, c + 1) } else if r + 1 < n_rows { (r + 1, 0) } else { (r, c) }
            } else if dr == 0 && dc == -1 {
                // Shift+Tab: left, wrap to previous row
                if c > 0 { (r, c - 1) } else if r > 0 { (r - 1, n_cols - 1) } else { (0, 0) }
            } else {
                // Enter: down same column
                if r + 1 < n_rows { (r + 1, c) } else { (r, c) }
            };
            let new_val = state.ui.files_csv_rows.get(nr)
                .and_then(|row| row.get(nc)).cloned().unwrap_or_default();
            state.ui.files_csv_editing  = Some((nr, nc));
            state.ui.files_csv_edit_buf = new_val;
        }
    } else if deferred_escape {
        // Discard — don't write back the edit buffer.
        state.ui.files_csv_editing = None;
    } else if let Some((r, c)) = deferred_commit {
        let val = state.ui.files_csv_edit_buf.clone();
        if let Some(row) = state.ui.files_csv_rows.get_mut(r) {
            if let Some(cell) = row.get_mut(c) { *cell = val; state.ui.files_csv_dirty = true; }
        }
        state.ui.files_csv_editing = None;
    } else if let Some((r, c, val)) = deferred_select {
        state.ui.files_csv_edit_buf = val;
        state.ui.files_csv_editing  = Some((r, c));
    }
}

// ── Plot view ─────────────────────────────────────────────────────────────────

fn show_plot_view(ui: &mut egui::Ui, state: &mut AppState, dark: bool) {
    if state.ui.files_csv_headers.is_empty() { return; }

    let headers = state.ui.files_csv_headers.clone();
    let dim = theme::fg2(dark);

    // Ensure selections remain valid after a new file is loaded.
    if !headers.contains(&state.ui.files_plot_x_col) {
        state.ui.files_plot_x_col = headers[0].clone();
    }
    if !headers.contains(&state.ui.files_plot_y_col) {
        state.ui.files_plot_y_col = headers.get(1).cloned()
            .unwrap_or_else(|| headers[0].clone());
    }

    // ── Controls ─────────────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("X:").color(dim).size(12.0));
        egui::ComboBox::from_id_salt("fp_x")
            .selected_text(&state.ui.files_plot_x_col).width(120.0)
            .show_ui(ui, |ui| {
                for h in &headers {
                    ui.selectable_value(&mut state.ui.files_plot_x_col, h.clone(), h);
                }
            });

        ui.add_space(6.0);
        ui.label(egui::RichText::new("Y:").color(dim).size(12.0));
        egui::ComboBox::from_id_salt("fp_y")
            .selected_text(&state.ui.files_plot_y_col).width(120.0)
            .show_ui(ui, |ui| {
                for h in &headers {
                    ui.selectable_value(&mut state.ui.files_plot_y_col, h.clone(), h);
                }
            });

        ui.add_space(6.0);
        ui.label(egui::RichText::new("Color by:").color(dim).size(12.0));
        let color_display = if state.ui.files_plot_color_col.is_empty() { "None" }
                            else { &state.ui.files_plot_color_col };
        egui::ComboBox::from_id_salt("fp_color")
            .selected_text(color_display).width(110.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.ui.files_plot_color_col, String::new(), "None");
                for h in &headers {
                    ui.selectable_value(&mut state.ui.files_plot_color_col, h.clone(), h);
                }
            });

        ui.add_space(10.0);
        ui.checkbox(&mut state.ui.files_plot_unity, "Unity line");
        ui.add_space(4.0);
        ui.checkbox(&mut state.ui.files_plot_loess, "LOESS");
        ui.add_space(4.0);
        ui.checkbox(&mut state.ui.files_plot_log_x, "Log X");
        ui.add_space(4.0);
        ui.checkbox(&mut state.ui.files_plot_log_y, "Log Y");
    });

    // ── Parse plot data ───────────────────────────────────────────────────────
    let x_col    = state.ui.files_plot_x_col.clone();
    let y_col    = state.ui.files_plot_y_col.clone();
    let color_col = state.ui.files_plot_color_col.clone();
    let log_x    = state.ui.files_plot_log_x;
    let log_y    = state.ui.files_plot_log_y;
    let unity    = state.ui.files_plot_unity;
    let do_loess = state.ui.files_plot_loess;

    let xi = headers.iter().position(|h| h == &x_col);
    let yi = headers.iter().position(|h| h == &y_col);
    let ci = if color_col.is_empty() { None }
             else { headers.iter().position(|h| h == &color_col) };

    let (xi, yi) = match (xi, yi) {
        (Some(a), Some(b)) => (a, b),
        _ => {
            ui.label(egui::RichText::new("Select valid X and Y columns above.")
                .color(theme::fg3(dark)));
            return;
        }
    };

    let mut all_pts: Vec<(f64, f64, String)> = Vec::new();
    for row in &state.ui.files_csv_rows {
        let xv = row.get(xi).and_then(|s| s.trim().parse::<f64>().ok());
        let yv = row.get(yi).and_then(|s| s.trim().parse::<f64>().ok());
        if let (Some(x), Some(y)) = (xv, yv) {
            let lx = if log_x { x.ln() } else { x };
            let ly = if log_y { y.ln() } else { y };
            if lx.is_finite() && ly.is_finite() {
                let cat = ci.and_then(|c| row.get(c)).cloned().unwrap_or_default();
                all_pts.push((lx, ly, cat));
            }
        }
    }

    if all_pts.is_empty() {
        ui.label(egui::RichText::new("No numeric data for the selected columns.")
            .color(theme::fg3(dark)));
        return;
    }

    // Build per-category point groups.
    let groups: Vec<(String, Vec<[f64; 2]>)> = if ci.is_some() {
        let mut cats: Vec<String> = all_pts.iter().map(|(_, _, c)| c.clone()).collect();
        cats.sort(); cats.dedup();
        cats.into_iter().map(|cat| {
            let pts = all_pts.iter()
                .filter(|(_, _, c)| c == &cat)
                .map(|&(x, y, _)| [x, y])
                .collect();
            (cat, pts)
        }).collect()
    } else {
        let pts = all_pts.iter().map(|&(x, y, _)| [x, y]).collect();
        vec![(String::new(), pts)]
    };

    let xy_pts: Vec<[f64; 2]> = all_pts.iter().map(|&(x, y, _)| [x, y]).collect();
    let loess_line = if do_loess {
        crate::ui::eval_tab::loess(&xy_pts, 0.35)
    } else {
        vec![]
    };

    let x_min = xy_pts.iter().fold(f64::INFINITY,     |a, p| a.min(p[0]));
    let x_max = xy_pts.iter().fold(f64::NEG_INFINITY, |a, p| a.max(p[0]));
    let y_min = xy_pts.iter().fold(f64::INFINITY,     |a, p| a.min(p[1]));
    let y_max = xy_pts.iter().fold(f64::NEG_INFINITY, |a, p| a.max(p[1]));

    let x_label = if log_x { format!("ln({x_col})") } else { x_col.clone() };
    let y_label = if log_y { format!("ln({y_col})") } else { y_col.clone() };
    let plot_id  = format!("files_scatter_{}_{}", x_col, y_col);

    // ── Plot ──────────────────────────────────────────────────────────────────
    Plot::new(plot_id)
        .x_axis_label(x_label)
        .y_axis_label(y_label)
        .legend(egui_plot::Legend::default())
        .show(ui, |p| {
            for (i, (label, pts)) in groups.iter().enumerate() {
                let col = if ci.is_none() {
                    if dark { egui::Color32::from_rgba_unmultiplied(76, 138, 255, 200) }
                    else    { egui::Color32::from_rgba_unmultiplied(30, 90, 210, 180) }
                } else {
                    CAT_COLORS[i % CAT_COLORS.len()]
                };
                let name = if label.is_empty() { y_col.clone() } else { label.clone() };
                p.points(
                    Points::new(PlotPoints::new(pts.clone()))
                        .radius(2.5).color(col).name(name),
                );
            }

            if unity {
                let lo = x_min.min(y_min);
                let hi = x_max.max(y_max);
                let col = if dark { egui::Color32::from_gray(130) }
                          else    { egui::Color32::from_gray(160) };
                p.line(Line::new(PlotPoints::new(vec![[lo, lo], [hi, hi]]))
                    .color(col).width(1.5).name("Unity"));
            }

            if loess_line.len() > 1 {
                p.line(Line::new(PlotPoints::new(loess_line.clone()))
                    .color(theme::ORANGE).width(2.0).name("LOESS"));
            }
        });
}

// ── Placeholder views ─────────────────────────────────────────────────────────

fn show_binary_placeholder(ui: &mut egui::Ui, state: &AppState, dark: bool) {
    let ext = state.ui.files_selected.as_ref()
        .and_then(|p| p.extension()).and_then(|e| e.to_str())
        .unwrap_or("").to_uppercase();
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(egui::RichText::new(format!("No preview for {ext} files."))
                .size(14.0).color(theme::fg3(dark)));
            ui.add_space(8.0);
            ui.label(egui::RichText::new(
                "Double-click the file to open it in the default app.")
                .size(12.0).color(theme::fg3(dark)));
        });
    });
}

fn show_empty_hint(ui: &mut egui::Ui, dark: bool) {
    ui.centered_and_justified(|ui| {
        ui.label(egui::RichText::new("Select a file to preview it.")
            .size(13.0).color(theme::fg3(dark)));
    });
}

// ── File loading ──────────────────────────────────────────────────────────────

/// Confirmation dialog shown when the user clicks a different file while the
/// current one has unsaved edits — otherwise `load_file` would silently
/// discard them.
fn show_unsaved_nav_dialog(ctx: &egui::Context, state: &mut AppState) {
    let Some(target) = state.ui.files_pending_nav.clone() else { return };

    let mut cancel  = false;
    let mut discard = false;
    let mut save    = false;

    egui::Window::new("Unsaved changes")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            let dark = ui.visuals().dark_mode;
            ui.set_min_width(320.0);
            ui.label(egui::RichText::new("Unsaved changes").strong().size(14.0).color(theme::fg(dark)));
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("This file has unsaved edits. Save before switching?")
                    .color(theme::fg2(dark)).size(12.0),
            );
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked()  { cancel  = true; }
                if ui.button("Discard").clicked() { discard = true; }
                ui.add_space(8.0);
                if ui.add(
                    egui::Button::new(egui::RichText::new("Save").color(egui::Color32::WHITE))
                        .fill(theme::ACCENT),
                ).clicked() { save = true; }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) { cancel = true; }
        });

    if cancel {
        state.ui.files_pending_nav = None;
    } else if discard {
        state.ui.files_pending_nav = None;
        load_file(state, target);
    } else if save {
        if state.ui.files_text_dirty { save_text_file(state); }
        if state.ui.files_csv_dirty  { save_csv_file(state); }
        // Both flags are mutually exclusive in practice (load_file routes a
        // path to exactly one view mode by extension), so this really just
        // asks "did the save that mattered succeed?" — if either save
        // failed, its flag stays set (see save_text_file/save_csv_file,
        // which only clear on Ok) and we leave the edit in place rather
        // than discarding it via load_file.
        if !state.ui.files_text_dirty && !state.ui.files_csv_dirty {
            state.ui.files_pending_nav = None;
            load_file(state, target);
        }
    }
}

fn load_file(state: &mut AppState, path: PathBuf) {
    if state.ui.files_selected.as_ref() == Some(&path) { return; }

    // Reset edit state when switching files.
    state.ui.files_text_dirty    = false;
    state.ui.files_csv_edit_mode = false;
    state.ui.files_csv_dirty     = false;
    state.ui.files_csv_editing   = None;
    state.ui.files_selected      = Some(path.clone());

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

    if BINARY_EXTS.contains(&ext.as_str()) {
        state.ui.files_view_mode = FilesViewMode::Binary;
        return;
    }

    if TABLE_EXTS.contains(&ext.as_str()) {
        if let Some((headers, rows)) = load_csv_data(&path) {
            let x = headers.first().cloned().unwrap_or_default();
            let y = headers.get(1).cloned().unwrap_or_else(|| x.clone());
            state.ui.files_csv_headers    = headers;
            state.ui.files_csv_rows       = rows;
            state.ui.files_plot_x_col     = x;
            state.ui.files_plot_y_col     = y;
            state.ui.files_plot_color_col = String::new();
            state.ui.files_view_mode      = FilesViewMode::Table;
            return;
        }
        // Fall through to text view if CSV parse fails.
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => {
            state.ui.files_text         = content;
            state.ui.files_text_dirty   = false;
            state.ui.files_text_is_ferx = ext == "ferx";
            state.ui.files_view_mode    = FilesViewMode::Text;
        }
        Err(_) => {
            state.ui.files_view_mode = FilesViewMode::Binary;
        }
    }
}

fn load_csv_data(path: &PathBuf) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_path(path)
        .ok()?;
    let headers: Vec<String> = rdr.headers().ok()?.iter().map(str::to_owned).collect();
    if headers.is_empty() { return None; }
    let rows: Vec<Vec<String>> = rdr.records()
        .filter_map(|r| r.ok().map(|rec| rec.iter().map(str::to_owned).collect()))
        .collect();
    Some((headers, rows))
}

// ── Save ──────────────────────────────────────────────────────────────────────

fn save_text_file(state: &mut AppState) {
    let path = match state.ui.files_selected.clone() { Some(p) => p, None => return };

    match std::fs::write(&path, state.ui.files_text.as_bytes()) {
        Ok(()) => {
            state.ui.files_text_dirty = false;
            state.ui.status_message   = format!("Saved {}", path.display());

            // Sync Models tab editor if the same .ferx file is currently open.
            let stem = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned);
            if stem.is_some() && stem == state.ui.editor_loaded_stem {
                state.ui.editor_buffer = state.ui.files_text.clone();
                state.ui.editor_dirty  = false;
            }

            state.trigger_scan();
        }
        Err(e) => {
            state.ui.status_message = format!("Save failed: {e}");
        }
    }
}

fn save_csv_file(state: &mut AppState) {
    let path = match state.ui.files_selected.clone() { Some(p) => p, None => return };

    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut wtr = csv::WriterBuilder::new().from_path(&path)?;
        wtr.write_record(&state.ui.files_csv_headers)?;
        for row in &state.ui.files_csv_rows { wtr.write_record(row)?; }
        wtr.flush()?;
        Ok(())
    })();

    match result {
        Ok(()) => {
            state.ui.files_csv_dirty     = false;
            state.ui.files_csv_edit_mode = false;
            state.ui.files_csv_editing   = None;
            state.ui.status_message      = format!("Saved {}", path.display());
        }
        Err(e) => { state.ui.status_message = format!("CSV save failed: {e}"); }
    }
}

// ── Directory rebuild ─────────────────────────────────────────────────────────

fn rebuild_entries(state: &mut AppState) {
    let cwd = match &state.ui.files_cwd {
        Some(p) => p.clone(),
        None => {
            state.ui.files_entries     = Vec::new();
            state.ui.files_entries_dir = None;
            return;
        }
    };

    state.ui.files_entries_dir = Some(cwd.clone());

    let typed = state.ui.files_ext_input.trim().trim_start_matches('.').to_lowercase();
    let active: std::collections::HashSet<String> = if !typed.is_empty() {
        std::iter::once(typed).collect()
    } else {
        state.ui.files_active_exts.clone()
    };

    let Ok(iter) = std::fs::read_dir(&cwd) else {
        state.ui.files_entries = Vec::new();
        return;
    };

    let mut folders: Vec<crate::state::FilesEntry> = Vec::new();
    let mut files:   Vec<crate::state::FilesEntry> = Vec::new();

    for entry in iter.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) if !n.starts_with('.') => n.to_owned(),
            _ => continue,
        };
        let is_dir = path.is_dir();
        let (size, modified) = match std::fs::metadata(&path) {
            Ok(m) => (m.len(), m.modified().ok()),
            Err(_) => (0, None),
        };

        if is_dir {
            folders.push(crate::state::FilesEntry { name, path, is_dir: true, size, modified });
        } else {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            if active.is_empty() || active.contains(&ext) {
                files.push(crate::state::FilesEntry { name, path, is_dir: false, size, modified });
            }
        }
    }

    folders.sort_by_key(|a| a.name.to_lowercase());
    files.sort_by_key(|a| a.name.to_lowercase());
    folders.extend(files);
    state.ui.files_entries = folders;
}

fn save_filter(state: &mut AppState) {
    state.workspace.settings.file_extensions =
        state.ui.files_active_exts.iter().cloned().collect();
    state.workspace.save_settings();
}

// ── OS helpers ────────────────────────────────────────────────────────────────

fn os_open(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    { let _ = std::process::Command::new("open").arg(path).spawn(); }
    #[cfg(target_os = "linux")]
    { let _ = std::process::Command::new("xdg-open").arg(path).spawn(); }
    #[cfg(target_os = "windows")]
    { let _ = std::process::Command::new("cmd").args(["/c", "start", ""]).arg(path).spawn(); }
}

fn reveal_in_finder(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    { let _ = std::process::Command::new("open").arg("-R").arg(path).spawn(); }
    #[cfg(target_os = "linux")]
    {
        let dir = path.parent().unwrap_or(path);
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
    #[cfg(target_os = "windows")]
    { let _ = std::process::Command::new("explorer").arg("/select,").arg(path).spawn(); }
}

// ── Formatting helpers ────────────────────────────────────────────────────────

fn fmt_size(n: u64) -> String {
    if n < 1_024          { format!("{n} B") }
    else if n < 1_048_576 { format!("{:.1} KB", n as f64 / 1_024.0) }
    else                  { format!("{:.1} MB", n as f64 / 1_048_576.0) }
}

fn fmt_mtime(t: std::time::SystemTime) -> String {
    use std::time::UNIX_EPOCH;
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    crate::workers::run::unix_to_datetime(secs)
}

fn breadcrumb_text(cwd: Option<&std::path::Path>, root: Option<&std::path::Path>) -> String {
    let cwd  = match cwd  { Some(p) => p, None => return "—".to_string() };
    let root = match root { Some(p) => p, None => return cwd.display().to_string() };
    if cwd == root {
        return root.file_name().and_then(|n| n.to_str()).unwrap_or("—").to_string();
    }
    match cwd.strip_prefix(root) {
        Ok(rel) => {
            let root_name = root.file_name().and_then(|n| n.to_str()).unwrap_or("root");
            let parts: Vec<&str> = rel.components()
                .filter_map(|c| {
                    if let std::path::Component::Normal(n) = c { n.to_str() } else { None }
                })
                .collect();
            format!("{}  /  {}", root_name, parts.join("  /  "))
        }
        Err(_) => cwd.display().to_string(),
    }
}
