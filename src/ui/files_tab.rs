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
    "png", "jpg", "jpeg", "gif", "bmp", "tiff", "tif", "ico",
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
    show_save_conflict_dialog(ui.ctx(), state);

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
        // Text labels, not arrow glyphs: the bundled fonts draw "←" as an empty box, which hid the
        // only way back (reported: no way to leave a sub-folder).
        let can_back = !state.ui.files_back_stack.is_empty();
        if ui.add_enabled(can_back, egui::Button::new("Back").min_size(egui::vec2(40.0, 22.0)))
            .on_hover_text("Go to the previous directory")
            .clicked()
        {
            if let Some(prev) = state.ui.files_back_stack.pop() {
                state.ui.files_cwd      = Some(prev);
                state.ui.files_selected = None;
                state.ui.files_view_mode = FilesViewMode::Empty;
            }
        }

        // Up: the parent folder, never above the project folder.
        let parent = parent_within_root(state.ui.files_cwd.as_deref(), state.workspace.directory.as_deref());
        if ui.add_enabled(parent.is_some(), egui::Button::new("Up").min_size(egui::vec2(34.0, 22.0)))
            .on_hover_text("Go up one folder")
            .clicked()
        {
            if let (Some(up), Some(cwd)) = (parent, state.ui.files_cwd.clone()) {
                state.ui.files_back_stack.push(cwd);
                state.ui.files_cwd       = Some(up);
                state.ui.files_selected  = None;
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
            FilesViewMode::Bundle => super::bundle_view::show(ui, state, dark),
            FilesViewMode::Error  => show_doc_error(ui, state, dark),
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
        // `TableBuilder` only ever manages its own *vertical* scroll area
        // internally (egui_extras hardcodes the horizontal component off) —
        // wrapping it in our own horizontal `ScrollArea` is what lets a table
        // wider than the panel actually be scrolled to reach later columns,
        // instead of just clipping them with no way to get to them.
        egui::ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| {
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
        }); // ScrollArea::horizontal
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
        ui.checkbox(&mut state.ui.files_plot_linear, "Linear fit")
            .on_hover_text("Ordinary least-squares line through the plotted points (on the log scale when an axis is log)");
        ui.add_space(4.0);
        ui.checkbox(&mut state.ui.files_plot_log_x, "Log X");
        ui.add_space(4.0);
        ui.checkbox(&mut state.ui.files_plot_log_y, "Log Y");
    });

    // Axis limits (data units, blank = automatic) and figure export.
    let mut export_clicked = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("X range:").color(dim).size(12.0));
        limit_field(ui, &mut state.ui.files_plot_xmin, "min");
        ui.label(egui::RichText::new("to").color(dim).size(12.0));
        limit_field(ui, &mut state.ui.files_plot_xmax, "max");
        ui.add_space(10.0);
        ui.label(egui::RichText::new("Y range:").color(dim).size(12.0));
        limit_field(ui, &mut state.ui.files_plot_ymin, "min");
        ui.label(egui::RichText::new("to").color(dim).size(12.0));
        limit_field(ui, &mut state.ui.files_plot_ymax, "max");
        if ui.small_button("Auto").on_hover_text("Clear all limits").clicked() {
            for s in [&mut state.ui.files_plot_xmin, &mut state.ui.files_plot_xmax,
                      &mut state.ui.files_plot_ymin, &mut state.ui.files_plot_ymax] { s.clear(); }
        }
        ui.add_space(10.0);
        if ui.button("Export figure…").on_hover_text("Save the plot as a PNG (white background, 2400 × 1500)").clicked() {
            export_clicked = true;
        }
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
            let lx = if log_x { x.log10() } else { x };
            let ly = if log_y { y.log10() } else { y };
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
    let linear = if state.ui.files_plot_linear { linear_fit(&xy_pts) } else { None };
    let loess_line = if do_loess {
        crate::ui::eval_tab::loess(&xy_pts, 0.75)
    } else {
        vec![]
    };

    let x_min = xy_pts.iter().fold(f64::INFINITY,     |a, p| a.min(p[0]));
    let x_max = xy_pts.iter().fold(f64::NEG_INFINITY, |a, p| a.max(p[0]));
    let y_min = xy_pts.iter().fold(f64::INFINITY,     |a, p| a.min(p[1]));
    let y_max = xy_pts.iter().fold(f64::NEG_INFINITY, |a, p| a.max(p[1]));

    let x_label = if log_x { format!("log10({x_col})") } else { x_col.clone() };
    let y_label = if log_y { format!("log10({y_col})") } else { y_col.clone() };
    let plot_id  = format!("files_scatter_{}_{}", x_col, y_col);

    // Limits are typed in data units; on a log axis they are converted to log10 (and must be > 0).
    let xlo = parse_limit(&state.ui.files_plot_xmin, log_x);
    let xhi = parse_limit(&state.ui.files_plot_xmax, log_x);
    let ylo = parse_limit(&state.ui.files_plot_ymin, log_y);
    let yhi = parse_limit(&state.ui.files_plot_ymax, log_y);
    let any_limit = xlo.is_some() || xhi.is_some() || ylo.is_some() || yhi.is_some();
    let x_range = axis_range((x_min, x_max), xlo, xhi);
    let y_range = axis_range((y_min, y_max), ylo, yhi);

    let colors: Vec<egui::Color32> = (0..groups.len()).map(|i| {
        if ci.is_none() {
            if dark { egui::Color32::from_rgba_unmultiplied(76, 138, 255, 200) }
            else    { egui::Color32::from_rgba_unmultiplied(30, 90, 210, 180) }
        } else { CAT_COLORS[i % CAT_COLORS.len()] }
    }).collect();
    if export_clicked {
        let spec = FilesPlotSpec {
            groups: groups.iter().zip(&colors)
                .map(|((l, p), c)| (if l.is_empty() { y_col.clone() } else { l.clone() }, p.clone(), *c)).collect(),
            x_label: x_label.clone(), y_label: y_label.clone(), x_range, y_range,
            unity: unity.then(|| [x_min.min(y_min), x_max.max(y_max)]),
            loess: loess_line.clone(),
            linear: linear.map(|f| (f.line(x_min, x_max), f.label())),
        };
        let default_name = format!("{}_vs_{}.png", y_col, x_col);
        if let Some(path) = rfd::FileDialog::new().set_file_name(default_name)
            .add_filter("PNG image", &["png"]).save_file()
        {
            state.ui.status_message = match export_files_plot_png(&spec, &path) {
                Ok(()) => format!("Figure saved to {}", path.display()),
                Err(e) => format!("Figure export failed: {e}"),
            };
        }
    }

    // ── Plot ──────────────────────────────────────────────────────────────────
    Plot::new(plot_id)
        .x_axis_label(x_label)
        .y_axis_label(y_label)
        .legend(egui_plot::Legend::default())
        .show(ui, |p| {
            // With any limit set, the view is pinned to the requested range.
            if any_limit {
                p.set_plot_bounds(egui_plot::PlotBounds::from_min_max(
                    [x_range.0, y_range.0], [x_range.1, y_range.1]));
            }
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
            if let Some(f) = &linear {
                p.line(Line::new(PlotPoints::new(f.line(x_min, x_max).to_vec()))
                    .color(LINEAR_COLOR).width(2.0).name(f.label()));
            }
        });
}

/// A small min/max entry; blank means automatic. Invalid text is shown in red and ignored.
fn limit_field(ui: &mut egui::Ui, text: &mut String, hint: &str) {
    let bad = !text.trim().is_empty() && text.trim().parse::<f64>().is_err();
    let mut te = egui::TextEdit::singleline(text).desired_width(54.0).hint_text(hint);
    if bad { te = te.text_color(theme::RED); }
    ui.add(te).on_hover_text("Axis limit in data units (blank = automatic)");
}

/// An axis limit typed in data units, as a plotted coordinate: None when blank or not a number;
/// on a log axis it must be positive and is converted to log10.
fn parse_limit(text: &str, log: bool) -> Option<f64> {
    let v: f64 = text.trim().parse().ok()?;
    if !v.is_finite() { return None; }
    if log { (v > 0.0).then(|| v.log10()) } else { Some(v) }
}

/// The plotted range of an axis: user limits where given, otherwise the data extent padded by 5 %.
/// A user pair that is not increasing falls back to the automatic range.
fn axis_range(data: (f64, f64), user_lo: Option<f64>, user_hi: Option<f64>) -> (f64, f64) {
    let (mut lo, mut hi) = data;
    if !lo.is_finite() || !hi.is_finite() { lo = 0.0; hi = 1.0; }
    if (hi - lo).abs() < 1e-12 { lo -= 0.5; hi += 0.5; }
    let pad = (hi - lo) * 0.05;
    let (auto_lo, auto_hi) = (lo - pad, hi + pad);
    let (l, h) = (user_lo.unwrap_or(auto_lo), user_hi.unwrap_or(auto_hi));
    if l < h { (l, h) } else { (auto_lo, auto_hi) }
}

/// Colour of the linear trendline (distinct from the LOESS orange and the category palette).
const LINEAR_COLOR: egui::Color32 = egui::Color32::from_rgb(0xd6, 0x3b, 0x8f);

/// Ordinary least-squares line y = intercept + slope · x through the plotted points.
#[derive(Debug, Clone, Copy, PartialEq)]
struct LinearFit { intercept: f64, slope: f64, r2: f64, n: usize }

impl LinearFit {
    fn line(&self, x0: f64, x1: f64) -> [[f64; 2]; 2] {
        [[x0, self.intercept + self.slope * x0], [x1, self.intercept + self.slope * x1]]
    }
    fn label(&self) -> String {
        let sign = if self.intercept < 0.0 { '-' } else { '+' };
        format!("Linear: y = {:.4} x {sign} {:.4}  (R² = {:.3}, n = {})",
                self.slope, self.intercept.abs(), self.r2, self.n)
    }
}

/// OLS fit of finite points; None with fewer than 3 points or no spread in x.
fn linear_fit(pts: &[[f64; 2]]) -> Option<LinearFit> {
    let p: Vec<&[f64; 2]> = pts.iter().filter(|p| p[0].is_finite() && p[1].is_finite()).collect();
    let n = p.len();
    if n < 3 { return None; }
    let nf = n as f64;
    let (mx, my) = (p.iter().map(|q| q[0]).sum::<f64>() / nf, p.iter().map(|q| q[1]).sum::<f64>() / nf);
    let sxx: f64 = p.iter().map(|q| (q[0] - mx).powi(2)).sum();
    if sxx <= 0.0 { return None; }
    let sxy: f64 = p.iter().map(|q| (q[0] - mx) * (q[1] - my)).sum();
    let syy: f64 = p.iter().map(|q| (q[1] - my).powi(2)).sum();
    let slope = sxy / sxx;
    let r2 = if syy > 0.0 { sxy * sxy / (sxx * syy) } else { 1.0 };
    Some(LinearFit { intercept: my - slope * mx, slope, r2, n })
}

/// Everything needed to draw the figure outside egui.
struct FilesPlotSpec {
    groups:  Vec<(String, Vec<[f64; 2]>, egui::Color32)>,
    x_label: String,
    y_label: String,
    x_range: (f64, f64),
    y_range: (f64, f64),
    unity:   Option<[f64; 2]>,
    loess:   Vec<[f64; 2]>,
    /// Linear trendline endpoints and its legend text.
    linear:  Option<([[f64; 2]; 2], String)>,
}

/// Renders the scatter plot to a PNG (2400 × 1500, white background) with `plotters`: same data,
/// colours, scales and limits as on screen. Points outside the limits are clipped by the chart.
fn export_files_plot_png(spec: &FilesPlotSpec, path: &std::path::Path) -> Result<(), String> {
    use plotters::prelude::*;
    const UBUNTU_LIGHT: &[u8] =
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts/Ubuntu-Light.ttf"));
    static FONT: std::sync::Once = std::sync::Once::new();
    FONT.call_once(|| {
        let _ = plotters::style::register_font("sans-serif", plotters::style::FontStyle::Normal, UBUNTU_LIGHT);
    });

    let root = BitMapBackend::new(path, (2400, 1500)).into_drawing_area();
    root.fill(&WHITE).map_err(|e| e.to_string())?;
    let mut chart = ChartBuilder::on(&root)
        .margin(60).x_label_area_size(90).y_label_area_size(100)
        .build_cartesian_2d(spec.x_range.0..spec.x_range.1, spec.y_range.0..spec.y_range.1)
        .map_err(|e| e.to_string())?;
    chart.configure_mesh()
        .x_desc(spec.x_label.as_str()).y_desc(spec.y_label.as_str())
        .label_style(("sans-serif", 28)).axis_desc_style(("sans-serif", 32))
        .x_labels(8).y_labels(8)
        .draw().map_err(|e| e.to_string())?;

    let (xr, yr) = (spec.x_range, spec.y_range);
    for (name, pts, c) in &spec.groups {
        let col = RGBColor(c.r(), c.g(), c.b());
        let inside: Vec<(f64, f64)> = pts.iter().map(|p| (p[0], p[1]))
            .filter(|&(x, y)| x >= xr.0 && x <= xr.1 && y >= yr.0 && y <= yr.1).collect();
        chart.draw_series(inside.iter().map(|&(x, y)| Circle::new((x, y), 7, col.mix(0.75).filled())))
            .map_err(|e| e.to_string())?
            .label(name.as_str())
            .legend(move |(x, y)| Circle::new((x + 12, y), 7, col.mix(0.75).filled()));
    }
    if let Some([lo, hi]) = spec.unity {
        chart.draw_series(LineSeries::new(vec![(lo, lo), (hi, hi)], BLACK.mix(0.45).stroke_width(3)))
            .map_err(|e| e.to_string())?.label("Unity")
            .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 24, y)], BLACK.mix(0.45).stroke_width(3)));
    }
    if spec.loess.len() > 1 {
        let orange = RGBColor(0xe8, 0x95, 0x40);
        chart.draw_series(LineSeries::new(spec.loess.iter().map(|p| (p[0], p[1])), orange.stroke_width(5)))
            .map_err(|e| e.to_string())?.label("LOESS")
            .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 24, y)], orange.stroke_width(5)));
    }
    if let Some((ends, label)) = &spec.linear {
        let c = RGBColor(LINEAR_COLOR.r(), LINEAR_COLOR.g(), LINEAR_COLOR.b());
        chart.draw_series(LineSeries::new(ends.iter().map(|p| (p[0], p[1])), c.stroke_width(5)))
            .map_err(|e| e.to_string())?.label(label.as_str())
            .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 24, y)], c.stroke_width(5)));
    }
    chart.configure_series_labels()
        .background_style(WHITE.mix(0.85)).border_style(BLACK.mix(0.25))
        .label_font(("sans-serif", 28)).position(SeriesLabelPosition::UpperRight)
        .draw().map_err(|e| e.to_string())?;
    root.present().map_err(|e| e.to_string())?;
    Ok(())
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

fn show_doc_error(ui: &mut egui::Ui, state: &mut AppState, dark: bool) {
    let Some(err) = state.ui.files_doc_error.clone() else { return };
    ui.add_space(24.0);
    ui.label(egui::RichText::new("This file is not opened for editing").strong().size(14.0).color(theme::ORANGE));
    ui.add_space(6.0);
    ui.label(egui::RichText::new(err.to_string()).size(12.0).color(theme::fg2(dark)));
    if matches!(err, crate::io::textdoc::DocError::NotUtf8 { .. }) {
        ui.add_space(10.0);
        if ui.button("Convert to UTF-8 (Windows-1252 assumed, backup kept)").clicked() {
            if let Some(path) = state.ui.files_selected.clone() {
                match crate::io::textdoc::convert_cp1252_file(&path) {
                    Ok(bak) => {
                        state.ui.status_message = format!("Converted; original kept as {}", bak.display());
                        state.ui.files_selected = None; // force a reload
                        load_file(state, path);
                    }
                    Err(e) => state.ui.status_message = format!("Convert failed: {e}"),
                }
            }
        }
    }
}

/// Shown when the file on disk changed after it was loaded.
fn show_save_conflict_dialog(ctx: &egui::Context, state: &mut AppState) {
    if !state.ui.files_save_conflict { return; }
    let (mut overwrite, mut reload, mut cancel) = (false, false, false);
    egui::Window::new("File changed on disk")
        .collapsible(false).resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.label("This file was changed outside FeRx after you opened it.");
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() { cancel = true; }
                if ui.button("Reload (lose my edits)").clicked() { reload = true; }
                if ui.button("Overwrite (backup kept)").clicked() { overwrite = true; }
            });
        });
    if cancel { state.ui.files_save_conflict = false; }
    if reload {
        state.ui.files_save_conflict = false;
        if let Some(p) = state.ui.files_selected.take() { load_file(state, p); }
    }
    if overwrite {
        state.ui.files_save_conflict = false;
        state.ui.files_text_disk_hash = None; // skip the check this once
        if let Some(d) = state.ui.files_csv_doc.as_mut() { d.disk_hash.clear(); }
        if state.ui.files_text_dirty { save_text_file(state); }
        if state.ui.files_csv_dirty { save_csv_file(state); }
    }
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

    state.ui.files_bundle = None;
    state.ui.files_bundle_error = None;
    if ext == "fitrx" {
        // A fitted bundle is a zip: show the inspector instead of a "no preview" placeholder.
        match crate::io::bundle::inspect(&path) {
            Ok(info) => state.ui.files_bundle = Some(super::bundle_view::BundleViewState::new(info)),
            Err(e) => state.ui.files_bundle_error = Some(e),
        }
        state.ui.files_view_mode = FilesViewMode::Bundle;
        return;
    }

    if BINARY_EXTS.contains(&ext.as_str()) {
        state.ui.files_view_mode = FilesViewMode::Binary;
        return;
    }

    if TABLE_EXTS.contains(&ext.as_str()) {
        state.ui.files_csv_doc = None;
        state.ui.files_doc_error = None;
        match crate::io::textdoc::load_csv(&path) {
            Err(e) => {
                state.ui.files_doc_error = Some(e);
                state.ui.files_view_mode = FilesViewMode::Error;
                return;
            }
            Ok(doc) => {
            let (headers, rows) = (doc.headers.clone(), doc.rows.clone());
            state.ui.files_csv_doc = Some(doc);
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
        }
    }

    match std::fs::read(&path).map_err(|_| ()).and_then(|b| String::from_utf8(b).map_err(|_| ())) {
        Ok(content) => {
            state.ui.files_text_disk_hash = Some(crate::io::textdoc::sha256_hex(content.as_bytes()));
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

// ── Save ──────────────────────────────────────────────────────────────────────

fn save_text_file(state: &mut AppState) {
    let path = match state.ui.files_selected.clone() { Some(p) => p, None => return };

    match crate::io::textdoc::save_checked(
        &path, state.ui.files_text.as_bytes(), state.ui.files_text_disk_hash.as_deref())
    {
        Ok(true) => {
            state.ui.files_text_dirty = false;
            if let Some(a) = &state.workspace.app_dir { crate::io::textdoc::remove_draft(a, &path); }
            state.ui.files_text_disk_hash = Some(crate::io::textdoc::sha256_hex(state.ui.files_text.as_bytes()));
            state.ui.status_message   = format!("Saved {}", path.display());

            // Sync Models tab editor if the same .ferx file is currently open.
            let stem = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned);
            if stem.is_some() && stem == state.ui.editor_loaded_stem {
                state.ui.editor_buffer = state.ui.files_text.clone();
                state.ui.editor_dirty  = false;
            }

            state.trigger_scan();
        }
        Ok(false) => state.ui.files_save_conflict = true,
        Err(e) => {
            state.ui.status_message = format!("Save failed: {e}");
        }
    }
}

fn save_csv_file(state: &mut AppState) {
    let path = match state.ui.files_selected.clone() { Some(p) => p, None => return };
    let Some(mut doc) = state.ui.files_csv_doc.take() else {
        state.ui.status_message = "CSV save failed: the file is not loaded".into();
        return;
    };
    doc.headers = state.ui.files_csv_headers.clone();
    doc.rows = state.ui.files_csv_rows.clone();

    let result = doc.to_bytes().map_err(std::io::Error::other).and_then(|bytes| {
        let expected = (!doc.disk_hash.is_empty()).then_some(doc.disk_hash.as_str());
        crate::io::textdoc::save_checked(&path, &bytes, expected).map(|ok| (ok, bytes))
    });

    match result {
        Ok((true, _)) => {
            state.ui.files_csv_dirty     = false;
            state.ui.files_csv_edit_mode = false;
            state.ui.files_csv_editing   = None;
            state.ui.status_message      = format!("Saved {} (previous version kept as .bak)", path.display());
            // Reload the document so later saves compare against what is now on disk.
            state.ui.files_csv_doc = crate::io::textdoc::load_csv(&path).ok();
        }
        Ok((false, _)) => {
            state.ui.files_save_conflict = true;
            state.ui.files_csv_doc = Some(doc);
        }
        Err(e) => {
            state.ui.status_message = format!("CSV save failed: {e}");
            state.ui.files_csv_doc = Some(doc);
        }
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
    let _ = open::that_detached(path);
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
    {
        use std::os::windows::process::CommandExt;
        // One raw argument so a path containing spaces or `&` stays quoted.
        let _ = std::process::Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", path.display())).spawn();
    }
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

/// The parent of `cwd`, when `cwd` is inside the project folder and is not the project folder itself.
fn parent_within_root(cwd: Option<&std::path::Path>, root: Option<&std::path::Path>) -> Option<PathBuf> {
    let (cwd, root) = (cwd?, root?);
    if cwd == root || !cwd.starts_with(root) { return None; }
    cwd.parent().map(|p| p.to_path_buf())
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

#[cfg(test)]
mod up_tests {
    use super::parent_within_root;
    use std::path::Path;

    #[test]
    fn up_goes_to_the_parent_but_never_above_the_project_folder() {
        let root = Path::new("/p/POPCORN");
        assert_eq!(parent_within_root(Some(Path::new("/p/POPCORN/ferx_outputs")), Some(root)),
                   Some(root.to_path_buf()));
        assert_eq!(parent_within_root(Some(Path::new("/p/POPCORN/a/b")), Some(root)),
                   Some(Path::new("/p/POPCORN/a").to_path_buf()));
        assert_eq!(parent_within_root(Some(root), Some(root)), None, "already at the project folder");
        assert_eq!(parent_within_root(Some(Path::new("/elsewhere")), Some(root)), None);
        assert_eq!(parent_within_root(None, Some(root)), None);
    }
}

#[cfg(test)]
mod axis_tests {
    use super::*;

    #[test]
    fn limits_parse_in_data_units_and_log_needs_positive() {
        assert_eq!(parse_limit(" 2.5 ", false), Some(2.5));
        assert_eq!(parse_limit("-3", false), Some(-3.0));
        assert_eq!(parse_limit("", false), None);
        assert_eq!(parse_limit("abc", false), None);
        assert_eq!(parse_limit("100", true), Some(2.0));
        assert_eq!(parse_limit("0", true), None);
        assert_eq!(parse_limit("-5", true), None);
    }

    #[test]
    fn range_uses_user_limits_else_padded_data() {
        let (lo, hi) = axis_range((0.0, 10.0), None, None);
        assert!((lo + 0.5).abs() < 1e-12 && (hi - 10.5).abs() < 1e-12);
        assert_eq!(axis_range((0.0, 10.0), Some(2.0), Some(8.0)), (2.0, 8.0));
        let (lo, hi) = axis_range((0.0, 10.0), Some(2.0), None);
        assert_eq!(lo, 2.0); assert!((hi - 10.5).abs() < 1e-12);
        // Reversed or equal limits are ignored; a single-valued axis still has a width.
        assert_eq!(axis_range((0.0, 10.0), Some(8.0), Some(2.0)), axis_range((0.0, 10.0), None, None));
        let (lo, hi) = axis_range((5.0, 5.0), None, None);
        assert!(lo < 5.0 && hi > 5.0);
        let (lo, hi) = axis_range((f64::INFINITY, f64::NEG_INFINITY), None, None);
        assert!(lo < hi);
    }

    #[test]
    fn export_writes_a_png() {
        let spec = FilesPlotSpec {
            groups: vec![("CL".into(), vec![[0.1, 5.0], [0.2, 6.0], [9.0, 99.0]], egui::Color32::from_rgb(76, 138, 255))],
            x_label: "SMICR".into(), y_label: "CL".into(),
            x_range: (0.05, 0.3), y_range: (4.0, 7.0), // the third point is outside and must be clipped
            unity: Some([0.0, 10.0]), loess: vec![[0.1, 5.0], [0.2, 6.0]],
            linear: Some(([[0.1, 5.0], [0.2, 6.0]], "Linear".into())),
        };
        let p = std::env::temp_dir().join(format!("ferxgui_fig_{}.png", std::process::id()));
        export_files_plot_png(&spec, &p).unwrap();
        let bytes = std::fs::read(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert!(bytes.len() > 5_000, "{} bytes", bytes.len());
    }

    #[test]
    fn linear_fit_recovers_a_known_line_and_rejects_degenerate_input() {
        let pts: Vec<[f64; 2]> = (0..10).map(|i| [i as f64, 2.0 + 0.5 * i as f64]).collect();
        let f = linear_fit(&pts).unwrap();
        assert!((f.slope - 0.5).abs() < 1e-12 && (f.intercept - 2.0).abs() < 1e-12 && (f.r2 - 1.0).abs() < 1e-12);
        // Matches R lm(y ~ x): x = 1..5, y = 2,1,4,3,5 -> slope 0.8, intercept 0.6, R² = 0.64.
        let g = linear_fit(&[[1.0, 2.0], [2.0, 1.0], [3.0, 4.0], [4.0, 3.0], [5.0, 5.0]]).unwrap();
        assert!((g.slope - 0.8).abs() < 1e-12 && (g.intercept - 0.6).abs() < 1e-12 && (g.r2 - 0.64).abs() < 1e-12);
        assert!(linear_fit(&[[1.0, 1.0], [1.0, 2.0], [1.0, 3.0]]).is_none(), "no spread in x");
        assert!(linear_fit(&[[1.0, 1.0], [2.0, 2.0]]).is_none(), "too few points");
        assert!(g.label().contains("R² = 0.640") && g.label().contains("n = 5"));
    }
}

