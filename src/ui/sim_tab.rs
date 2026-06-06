/// Monte Carlo simulation prediction interval plot tab.
///
/// Layout: fixed left panel (cards + pinned Plot button), plot canvas on the right.
/// All computation is pure Rust — no R required.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui;
use egui_plot::{HLine, Line, Plot, PlotPoints, Points, Polygon};

use crate::app::theme;
use crate::domain::{BandSpec, FilterRow, SimBandData, SimData, SimPlotResult};
use crate::domain::sim_types::RefLine;
use crate::state::AppState;
use crate::workers::messages::WorkerMsg;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const OPS: &[&str] = &["==", "!=", ">", "<", ">=", "<="];

struct BandPreset(f32, f32, u8, u8, u8, f32);

const PRESETS: &[&[BandPreset]] = &[
    &[BandPreset(5.0,  95.0,  0x56, 0x9c, 0xd6, 0.25),
      BandPreset(25.0, 75.0,  0x56, 0x9c, 0xd6, 0.40)],
    &[BandPreset(2.5,  97.5,  0x4e, 0xc9, 0x94, 0.20),
      BandPreset(10.0, 90.0,  0x4e, 0xc9, 0x94, 0.35)],
    &[BandPreset(10.0, 90.0,  0xce, 0x91, 0x78, 0.30)],
    &[BandPreset(5.0,  95.0,  0x56, 0x9c, 0xd6, 0.25)],
];
const PRESET_LABELS: &[&str] = &[
    "5/95 + 25/75", "2.5/97.5 + 10/90", "10/90 only", "5/95 only", "Custom",
];

const REP_NAMES: &[&str] = &[
    "REP", "IREP", "SIM", "REPLICATE", "SIMNO", "SIM_NO", "REP_NO", "SIM_NUM", "NSIM", "ISIM",
];

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;

    // Publication PNG export via plotters (triggered on the frame after the
    // button is clicked so the UI has a chance to repaint the "exporting" state).
    if state.sim.export_pending {
        state.sim.export_pending = false;
        // Resolve axis labels: fall back to column name if the user left them blank.
        let x_lbl = if state.sim.x_label.trim().is_empty() { state.sim.x_col.clone() }
                    else { state.sim.x_label.clone() };
        let y_lbl = if state.sim.y_label.trim().is_empty() { state.sim.y_col.clone() }
                    else { state.sim.y_label.clone() };
        if let Some(result) = &state.sim.result.clone() {
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name("sim_plot.png")
                .add_filter("PNG image", &["png"])
                .save_file()
            {
                match export_publication_png(&state.sim, result, &path, &x_lbl, &y_lbl) {
                    Ok(()) => {
                        if let Err(e) = open::that(&path) {
                            state.ui.status_message = format!("Sim plot saved (could not open: {e})");
                        }
                    }
                    Err(e) => { state.ui.status_message = format!("Sim plot export failed: {e}"); }
                }
            }
        }
    }

    let left_w = 330.0_f32;

    // horizontal_top gives us a row; the left slice gets an explicit vertical()
    // so its own content stacks top-down (not sideways).
    ui.horizontal_top(|ui| {
        // ── Left: vertical column containing scrollable cards + pinned button ──
        ui.vertical(|ui| {
            ui.set_width(left_w);

            let avail_h = ui.available_height();
            let btn_h   = 44.0;
            let scroll_h = (avail_h - btn_h).max(100.0);

            egui::ScrollArea::vertical()
                .id_salt("sim_left_scroll")
                .max_height(scroll_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(left_w - 6.0); // a little less to avoid clipping scrollbar
                    show_cards(ui, state, dark);
                });

            // Status line
            if !state.sim.status.is_empty() {
                let status = state.sim.status.clone();
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(&status)
                        .size(10.5)
                        .color(theme::fg2(dark)),
                );
            }

            // Plot button — pinned at the bottom, always visible
            ui.add_space(2.0);
            let running = state.sim.running;
            let has_data = state.sim.data.is_some();
            let btn_label = if running { "Computing…" } else { "Plot" };
            if ui.add_enabled(
                has_data && !running,
                egui::Button::new(
                    egui::RichText::new(btn_label).size(14.0).strong()
                )
                .fill(theme::ACCENT)
                .min_size(egui::vec2(ui.available_width(), 34.0)),
            ).clicked() {
                run_computation(state);
            }
            if running {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        egui::RichText::new("Computing quantiles…")
                            .color(theme::fg2(dark))
                            .size(11.0),
                    );
                });
            }
        });

        // ── Separator ──────────────────────────────────────────────────────
        ui.separator();

        // ── Right: plot panel ─────────────────────────────────────────────
        ui.vertical(|ui| {
            show_plot_panel(ui, state, dark);
        });
    });
}

// ---------------------------------------------------------------------------
// Cards — all stacked vertically inside the scroll area
// ---------------------------------------------------------------------------

fn show_cards(ui: &mut egui::Ui, state: &mut AppState, dark: bool) {
    let dim = theme::fg2(dark);

    // ── DATA ─────────────────────────────────────────────────────────────
    section(ui, "DATA", true, dark, |ui| {
        ui.label(
            egui::RichText::new(
                "Tip: Load a simulation output file that contains a replicate column \
                 (REP, IREP, SIM, SIMNO, ID…) alongside an independent variable \
                 (e.g. TIME) and a dependent variable (e.g. IPRED)."
            )
            .size(10.5)
            .color(dim)
            .italics(),
        );
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            let edit_w = ui.available_width() - 60.0;
            let resp = ui.add(
                egui::TextEdit::singleline(&mut state.sim.file_path)
                    .hint_text("Simulation table or CSV file…")
                    .desired_width(edit_w),
            );
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                load_sim_file(state);
            }
            if ui.add(
                egui::Button::new("…").min_size(egui::vec2(28.0, 22.0))
            ).on_hover_text("Browse").clicked() {
                if let Some(p) = rfd::FileDialog::new().pick_file() {
                    state.sim.file_path = p.to_string_lossy().to_string();
                }
            }
            if ui.add(
                egui::Button::new(egui::RichText::new("Load").size(12.0))
                    .fill(theme::ACCENT)
                    .min_size(egui::vec2(44.0, 22.0)),
            ).clicked() {
                load_sim_file(state);
            }
        });

        let lbl = if state.sim.data.is_some() {
            state.sim.data_label.clone()
        } else {
            "No file loaded".to_string()
        };
        ui.label(egui::RichText::new(lbl).color(dim).size(10.5));
    });

    // ── VARIABLES ─────────────────────────────────────────────────────────
    let cols: Vec<String> = state.sim.data.as_ref()
        .map(|d| d.columns.clone())
        .unwrap_or_default();

    section(ui, "VARIABLES", true, dark, |ui| {
        labeled_combo(ui, "Replicate col:", &cols, &mut state.sim.rep_col, dim);
        labeled_combo(ui, "X-axis:",        &cols, &mut state.sim.x_col,   dim);
        labeled_combo(ui, "Y-axis:",        &cols, &mut state.sim.y_col,   dim);
    });

    // ── PREDICTION INTERVAL BANDS ─────────────────────────────────────────
    section(ui, "PREDICTION INTERVAL BANDS", true, dark, |ui| {
        // Preset row
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Preset:").color(dim).size(11.0));
            let prev = state.sim.preset_idx;
            egui::ComboBox::from_id_salt("sim_preset")
                .selected_text(PRESET_LABELS[state.sim.preset_idx])
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for (i, lbl) in PRESET_LABELS.iter().enumerate() {
                        ui.selectable_value(&mut state.sim.preset_idx, i, *lbl);
                    }
                });
            if state.sim.preset_idx != prev && state.sim.preset_idx < PRESETS.len() {
                apply_preset(state);
            }
        });
        ui.add_space(4.0);

        // Band rows
        let mut remove_idx: Option<usize> = None;
        let n = state.sim.bands.len();
        for i in 0..n {
            band_row(ui, i, state, dark, &mut remove_idx);
        }
        if let Some(i) = remove_idx {
            state.sim.bands.remove(i);
            state.sim.preset_idx = PRESET_LABELS.len() - 1; // Custom
        }

        if state.sim.bands.len() < 4 {
            ui.add_space(2.0);
            if ui.add(
                egui::Button::new(egui::RichText::new("+ Add band").size(11.0))
                    .min_size(egui::vec2(ui.available_width(), 26.0)),
            ).clicked() {
                state.sim.bands.push(BandSpec::new(
                    5.0, 95.0, egui::Color32::from_rgb(0x56, 0x9c, 0xd6), 0.25,
                ));
                state.sim.preset_idx = PRESET_LABELS.len() - 1;
            }
        }
    });

    // ── APPEARANCE ────────────────────────────────────────────────────────
    section(ui, "APPEARANCE", false, dark, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Median colour:").color(dim).size(11.0));
            // Wide color swatch using custom drawing + color_edit_button overlay
            wide_color_button(ui, &mut state.sim.median_color, 80.0);
            ui.add_space(10.0);
            ui.label(egui::RichText::new("Width:").color(dim).size(11.0));
            ui.add(
                egui::DragValue::new(&mut state.sim.median_lw)
                    .range(0.5_f32..=6.0)
                    .speed(0.1)
                    .fixed_decimals(1),
            );
        });
        ui.add_space(6.0);
        // Axis labels for publication export.
        egui::Grid::new("sim_axis_labels").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label(egui::RichText::new("X label").size(11.0).color(dim));
            ui.add(egui::TextEdit::singleline(&mut state.sim.x_label)
                .hint_text(if state.sim.x_col.is_empty() { "X column" } else { &state.sim.x_col })
                .desired_width(f32::INFINITY));
            ui.end_row();
            ui.label(egui::RichText::new("Y label").size(11.0).color(dim));
            ui.add(egui::TextEdit::singleline(&mut state.sim.y_label)
                .hint_text(if state.sim.y_col.is_empty() { "Y column" } else { &state.sim.y_col })
                .desired_width(f32::INFINITY));
            ui.end_row();
        });
        ui.label(egui::RichText::new("Used in publication export (leave blank to use column names).")
            .size(9.5).color(dim).italics());
        ui.add_space(4.0);
        ui.checkbox(&mut state.sim.log_y, "Logarithmic Y-axis");
        ui.horizontal(|ui| {
            ui.checkbox(&mut state.sim.smooth, "Smooth curves (LOESS)");
            if state.sim.smooth {
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Span:").color(dim).size(11.0));
                ui.add(
                    egui::DragValue::new(&mut state.sim.smooth_frac)
                        .range(0.05_f32..=1.0)
                        .speed(0.01)
                        .fixed_decimals(2),
                )
                .on_hover_text("LOESS bandwidth fraction");
            }
        });
    });

    // ── FILTERS ──────────────────────────────────────────────────────────
    section(ui, "FILTERS", true, dark, |ui| {
        ui.checkbox(&mut state.sim.mdv_filter, "Exclude MDV=1 rows");
        ui.add_space(4.0);

        if !state.sim.filters.is_empty() {
            // Column header
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Column").color(dim).size(9.0).strong());
                ui.add_space(56.0);
                ui.label(egui::RichText::new("Op").color(dim).size(9.0).strong());
                ui.add_space(40.0);
                ui.label(egui::RichText::new("Value").color(dim).size(9.0).strong());
            });
        }

        let mut remove_fi: Option<usize> = None;
        let n_filters = state.sim.filters.len();
        for i in 0..n_filters {
            let avail_cols = cols.clone();
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt(egui::Id::new("sim_fcol").with(i))
                    .selected_text(&state.sim.filters[i].col)
                    .width(100.0)
                    .show_ui(ui, |ui| {
                        for c in &avail_cols {
                            ui.selectable_value(&mut state.sim.filters[i].col, c.clone(), c);
                        }
                    });
                egui::ComboBox::from_id_salt(egui::Id::new("sim_fop").with(i))
                    .selected_text(&state.sim.filters[i].op)
                    .width(52.0)
                    .show_ui(ui, |ui| {
                        for op in OPS {
                            ui.selectable_value(&mut state.sim.filters[i].op, op.to_string(), *op);
                        }
                    });
                ui.add(
                    egui::TextEdit::singleline(&mut state.sim.filters[i].val)
                        .desired_width(64.0)
                        .hint_text("value"),
                );
                if ui.small_button("×").on_hover_text("Remove").clicked() {
                    remove_fi = Some(i);
                }
            });
        }
        if let Some(i) = remove_fi { state.sim.filters.remove(i); }

        if state.sim.filters.len() < 6 {
            if ui.add(
                egui::Button::new(egui::RichText::new("+ Add filter").size(11.0))
                    .min_size(egui::vec2(ui.available_width(), 26.0)),
            ).clicked() {
                let mut row = FilterRow::new();
                if let Some(c) = cols.first() { row.col = c.clone(); }
                state.sim.filters.push(row);
            }
        }
    });

    // ── REFERENCE LINES ───────────────────────────────────────────────────
    section(ui, "REFERENCE LINES", false, dark, |ui| {
        ui.label(
            egui::RichText::new("Horizontal dashed lines, e.g. IC90, MEC, BLOQ.")
                .color(dim).size(10.5),
        );
        ui.add_space(4.0);

        let mut remove_rl: Option<usize> = None;
        let n_rl = state.sim.ref_lines.len();
        for i in 0..n_rl {
            ui.horizontal(|ui| {
                ui.checkbox(&mut state.sim.ref_lines[i].visible, "");
                ui.add(
                    egui::DragValue::new(&mut state.sim.ref_lines[i].y)
                        .speed(0.1)
                        .fixed_decimals(3)
                        .prefix("y = "),
                );
                ui.add_space(4.0);
                ui.add(
                    egui::TextEdit::singleline(&mut state.sim.ref_lines[i].label)
                        .hint_text("label")
                        .desired_width(70.0),
                );
                ui.add_space(4.0);
                wide_color_button(ui, &mut state.sim.ref_lines[i].color, 40.0);
                ui.add_space(4.0);
                if ui.small_button("×").on_hover_text("Remove").clicked() {
                    remove_rl = Some(i);
                }
            });
        }
        if let Some(i) = remove_rl { state.sim.ref_lines.remove(i); }

        if state.sim.ref_lines.len() < 6 {
            if ui.add(
                egui::Button::new(egui::RichText::new("+ Add line").size(11.0))
                    .min_size(egui::vec2(ui.available_width(), 26.0)),
            ).clicked() {
                let colors = [
                    egui::Color32::from_rgb(0xe0, 0x4c, 0x4c),
                    egui::Color32::from_rgb(0xe0, 0x9a, 0x2c),
                    egui::Color32::from_rgb(0x2c, 0xa0, 0x60),
                    egui::Color32::from_rgb(0x7c, 0x3a, 0xb8),
                    egui::Color32::from_rgb(0x20, 0x80, 0xc0),
                    egui::Color32::from_rgb(0x60, 0x60, 0x60),
                ];
                let idx = state.sim.ref_lines.len() % colors.len();
                state.sim.ref_lines.push(RefLine::new(0.0, "", colors[idx]));
            }
        }
    });

    // ── OBSERVED DATA OVERLAY ─────────────────────────────────────────────
    section(ui, "OBSERVED DATA OVERLAY", false, dark, |ui| {
        ui.label(
            egui::RichText::new("Overlay observed data from a separate file (optional).")
                .color(dim).size(10.5),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let ew = ui.available_width() - 80.0;
            ui.add(
                egui::TextEdit::singleline(&mut state.sim.obs_path)
                    .hint_text("Observed file…")
                    .desired_width(ew),
            );
            if ui.add(egui::Button::new("…").min_size(egui::vec2(28.0, 22.0))).clicked() {
                if let Some(p) = rfd::FileDialog::new().pick_file() {
                    state.sim.obs_path = p.to_string_lossy().to_string();
                }
            }
            if ui.add(
                egui::Button::new(egui::RichText::new("Load").size(12.0))
                    .fill(theme::ACCENT)
                    .min_size(egui::vec2(44.0, 22.0)),
            ).clicked() {
                load_obs_file(state);
            }
        });

        let obs_cols = state.sim.obs_columns.clone();
        if !obs_cols.is_empty() {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("X:").color(dim).size(11.0));
                egui::ComboBox::from_id_salt("sim_obs_x")
                    .selected_text(&state.sim.obs_x_col)
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for c in &obs_cols {
                            ui.selectable_value(&mut state.sim.obs_x_col, c.clone(), c);
                        }
                    });
                ui.label(egui::RichText::new("Y:").color(dim).size(11.0));
                egui::ComboBox::from_id_salt("sim_obs_y")
                    .selected_text(&state.sim.obs_y_col)
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for c in &obs_cols {
                            ui.selectable_value(&mut state.sim.obs_y_col, c.clone(), c);
                        }
                    });
            });
        }
        if !state.sim.obs_label.is_empty() {
            let lbl = state.sim.obs_label.clone();
            ui.label(egui::RichText::new(lbl).color(dim).size(10.5));
        }
    });

    ui.add_space(4.0);
}

// ---------------------------------------------------------------------------
// Single band row
// ---------------------------------------------------------------------------

fn band_row(
    ui:         &mut egui::Ui,
    i:          usize,
    state:      &mut AppState,
    dark:       bool,
    remove_idx: &mut Option<usize>,
) {
    let dim = theme::fg2(dark);
    let fill = theme::elevated_fill(dark);

    egui::Frame::new()
        .fill(fill)
        .inner_margin(egui::Margin { left: 6, right: 6, top: 4, bottom: 4 })
        .corner_radius(egui::CornerRadius::same(4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                // Visibility checkbox
                ui.checkbox(&mut state.sim.bands[i].visible, "")
                    .on_hover_text("Show/hide");

                // Lo
                ui.label(egui::RichText::new("Lo").color(dim).size(9.5));
                ui.add(
                    egui::DragValue::new(&mut state.sim.bands[i].lo_pct)
                        .range(0.0_f32..=49.9)
                        .speed(0.5)
                        .suffix("%")
                        .max_decimals(1),
                )
                .on_hover_text("Lower percentile");

                // Hi
                ui.label(egui::RichText::new("Hi").color(dim).size(9.5));
                ui.add(
                    egui::DragValue::new(&mut state.sim.bands[i].hi_pct)
                        .range(50.1_f32..=100.0)
                        .speed(0.5)
                        .suffix("%")
                        .max_decimals(1),
                )
                .on_hover_text("Upper percentile");

                // Alpha
                ui.label(egui::RichText::new("α").color(dim).size(9.5));
                ui.add(
                    egui::DragValue::new(&mut state.sim.bands[i].alpha)
                        .range(0.05_f32..=1.0)
                        .speed(0.01)
                        .fixed_decimals(2),
                )
                .on_hover_text("Opacity");

                // Wide colour swatch (remaining space minus × button)
                let rm_w  = 22.0;
                let gap   = ui.style().spacing.item_spacing.x;
                let remaining = (ui.available_width() - rm_w - gap).max(20.0);
                wide_color_button(ui, &mut state.sim.bands[i].color, remaining);

                // Remove
                if ui.add(
                    egui::Button::new(
                        egui::RichText::new("×").color(theme::fg2(dark)).size(11.0)
                    )
                    .min_size(egui::vec2(rm_w, 18.0))
                    .frame(false),
                ).on_hover_text("Remove band").clicked() {
                    *remove_idx = Some(i);
                }
            });
        });
    ui.add_space(4.0);
}

// ---------------------------------------------------------------------------
// Plot panel (right side)
// ---------------------------------------------------------------------------

fn show_plot_panel(ui: &mut egui::Ui, state: &mut AppState, dark: bool) {
    let dim = theme::fg2(dark);

    // Toolbar
    ui.horizontal(|ui| {
        if let Some(result) = &state.sim.result {
            let n = result.times.len();
            let nv = state.sim.bands.iter().filter(|b| b.visible).count();
            ui.label(
                egui::RichText::new(format!("{n} unique X values · {nv} band(s)"))
                    .color(dim).size(10.5),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(
                state.sim.result.is_some(),
                egui::Button::new(egui::RichText::new("Save PNG…").size(11.0))
                    .min_size(egui::vec2(80.0, 22.0)),
            ).clicked() {
                state.sim.export_pending = true;
            }
        });
    });

    if state.sim.result.is_none() {
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new("Load a simulation file and click Plot.")
                    .color(theme::fg3(dark)).size(13.0),
            );
        });
        return;
    }

    let result = state.sim.result.as_ref().unwrap();
    let times  = &result.times;
    let smooth      = state.sim.smooth;
    let smooth_frac = state.sim.smooth_frac as f64;
    let log_y       = state.sim.log_y;
    let maybe_log   = |v: f64| -> f64 { if log_y && v > 0.0 { v.log10() } else { v } };

    let y_label = if log_y {
        format!("log₁₀({})", state.sim.y_col)
    } else {
        state.sim.y_col.clone()
    };

    Plot::new("sim_plot")
        .height((ui.available_height() - 4.0).max(200.0))
        .width(ui.available_width())
        .x_axis_label(&state.sim.x_col)
        .y_axis_label(&y_label)
        .show_grid(true)
        .allow_drag(true)
        .allow_zoom(true)
        .legend(egui_plot::Legend::default())
        .show(ui, |pui| {
            // Bands (draw widest first so narrower bands appear on top)
            for (spec, band) in state.sim.bands.iter().zip(result.bands.iter()).rev() {
                if !spec.visible { continue; }
                draw_ribbon(pui, times, band, spec, smooth, smooth_frac, &maybe_log);
            }

            // Median from first visible band
            if let Some((_s, band)) = state.sim.bands.iter()
                .zip(result.bands.iter()).find(|(s, _)| s.visible)
            {
                let med = curve_pts(times, &band.med, smooth, smooth_frac, &maybe_log);
                pui.line(
                    Line::new(PlotPoints::new(med))
                        .name("Median")
                        .color(state.sim.median_color)
                        .width(state.sim.median_lw),
                );
            }

            // Reference lines — only create a legend entry when a label is set.
            for rl in &state.sim.ref_lines {
                if !rl.visible { continue; }
                let y = maybe_log(rl.y);
                if !y.is_finite() { continue; }
                let hl = HLine::new(y)
                    .color(rl.color)
                    .width(1.5)
                    .style(egui_plot::LineStyle::Dashed { length: 8.0 });
                pui.hline(if rl.label.is_empty() { hl } else { hl.name(&rl.label) });
            }

            // Observed scatter
            if let Some((ox, oy)) = &state.sim.obs_xy {
                let pts: Vec<[f64; 2]> = ox.iter().zip(oy.iter())
                    .map(|(&x, &y)| [x, maybe_log(y)])
                    .filter(|[_, y]| y.is_finite())
                    .collect();
                pui.points(
                    Points::new(PlotPoints::new(pts))
                        .name("Observed")
                        .color(egui::Color32::from_rgba_unmultiplied(
                            theme::FG2.r(), theme::FG2.g(), theme::FG2.b(), 180,
                        ))
                        .radius(3.0),
                );
            }
        });
}

// ---------------------------------------------------------------------------
// Ribbon
// ---------------------------------------------------------------------------

/// Draw a prediction-interval ribbon as N-1 convex quadrilateral strips.
///
/// `egui_plot::Polygon` uses `Shape::convex_polygon` (fan triangulation from
/// vertex 0), which is incorrect for the non-convex loop shape of a ribbon on a
/// curved PK profile.  Decomposing into adjacent trapezoids guarantees each
/// sub-polygon is convex.  The first strip carries the legend name; subsequent
/// strips share it silently.
fn draw_ribbon(
    pui:         &mut egui_plot::PlotUi,
    times:       &[f64],
    band:        &SimBandData,
    spec:        &BandSpec,
    smooth:      bool,
    smooth_frac: f64,
    maybe_log:   &impl Fn(f64) -> f64,
) {
    let lo = curve_pts(times, &band.lo, smooth, smooth_frac, maybe_log);
    let hi = curve_pts(times, &band.hi, smooth, smooth_frac, maybe_log);
    let n  = lo.len().min(hi.len());
    if n < 2 { return; }

    let fill   = egui::Color32::from_rgba_unmultiplied(
        spec.color.r(), spec.color.g(), spec.color.b(), (spec.alpha * 255.0) as u8,
    );
    // Thin same-color stroke drives the egui legend color.
    let stroke = egui::Stroke::new(0.5, egui::Color32::from_rgba_unmultiplied(
        spec.color.r(), spec.color.g(), spec.color.b(), 160,
    ));
    fn fmt_p(v: f32) -> String {
        if v.fract() < 1e-4 { format!("{}", v as i32) } else { format!("{v:.1}") }
    }
    let name = format!("{}–{}% PI", fmt_p(spec.lo_pct), fmt_p(spec.hi_pct));

    for i in 0..n - 1 {
        let quad = vec![lo[i], lo[i + 1], hi[i + 1], hi[i]];
        let p = Polygon::new(PlotPoints::new(quad)).fill_color(fill).stroke(stroke);
        let p = if i == 0 { p.name(&name) } else { p };
        pui.polygon(p);
    }
}

fn curve_pts(
    times:       &[f64],
    vals:        &[f64],
    smooth:      bool,
    smooth_frac: f64,
    maybe_log:   &impl Fn(f64) -> f64,
) -> Vec<[f64; 2]> {
    let raw: Vec<[f64; 2]> = times.iter().zip(vals.iter())
        .map(|(&x, &y)| [x, maybe_log(y)])
        .filter(|[_, y]| y.is_finite())
        .collect();
    if smooth && raw.len() > 4 {
        let s = crate::ui::eval_tab::loess(&raw, smooth_frac);
        if !s.is_empty() { return s; }
    }
    raw
}

// ---------------------------------------------------------------------------
// File loading
// ---------------------------------------------------------------------------

fn load_sim_file(state: &mut AppState) {
    let path = PathBuf::from(state.sim.file_path.trim());
    if !path.is_file() { state.sim.status = "File not found.".into(); return; }
    match parse_table_file(&path) {
        Ok(data) => {
            let n = data.n_rows; let nc = data.columns.len();
            state.sim.data_label = format!(
                "{}  ({} rows, {} cols)",
                path.file_name().unwrap_or_default().to_string_lossy(), n, nc,
            );
            state.sim.status = format!("Loaded {n} rows, {nc} columns");
            populate_columns(state, &data);
            state.sim.data   = Some(Arc::new(data));
            state.sim.result = None;
        }
        Err(e) => { state.sim.status = format!("Load error: {e}"); }
    }
}

fn load_obs_file(state: &mut AppState) {
    let path = PathBuf::from(state.sim.obs_path.trim());
    if !path.is_file() { state.sim.status = "Observed file not found.".into(); return; }
    match parse_table_file(&path) {
        Ok(data) => {
            state.sim.obs_columns = data.columns.clone();
            if !data.columns.contains(&state.sim.obs_x_col) {
                state.sim.obs_x_col = data.columns.iter()
                    .find(|c| *c == &state.sim.x_col)
                    .cloned().unwrap_or_default();
            }
            if !data.columns.contains(&state.sim.obs_y_col) {
                state.sim.obs_y_col = data.columns.iter()
                    .find(|c| *c == &state.sim.y_col)
                    .cloned().unwrap_or_default();
            }
            state.sim.obs_label = format!("{} observed rows loaded", data.n_rows);
            let ox = data.col_data.get(&state.sim.obs_x_col).cloned().unwrap_or_default();
            let oy = data.col_data.get(&state.sim.obs_y_col).cloned().unwrap_or_default();
            if ox.len() == oy.len() { state.sim.obs_xy = Some((ox, oy)); }
        }
        Err(e) => { state.sim.status = format!("Observed load error: {e}"); }
    }
}

fn populate_columns(state: &mut AppState, data: &SimData) {
    let cols = &data.columns;
    if !cols.contains(&state.sim.rep_col) {
        state.sim.rep_col = cols.iter()
            .find(|c| REP_NAMES.iter().any(|r| r.eq_ignore_ascii_case(c)))
            .cloned().unwrap_or_default();
    }
    if !cols.contains(&state.sim.x_col) {
        state.sim.x_col = cols.iter().find(|c| c.eq_ignore_ascii_case("TIME"))
            .cloned().unwrap_or_else(|| cols.first().cloned().unwrap_or_default());
    }
    if !cols.contains(&state.sim.y_col) {
        state.sim.y_col = ["IPRED", "DV", "PRED"].iter()
            .find_map(|cand| cols.iter().find(|c| c.eq_ignore_ascii_case(cand)))
            .cloned()
            .unwrap_or_else(|| cols.get(1).cloned().unwrap_or_default());
    }
    for fr in &mut state.sim.filters {
        if !cols.contains(&fr.col) {
            fr.col = cols.first().cloned().unwrap_or_default();
        }
    }
}

// ---------------------------------------------------------------------------
// Computation
// ---------------------------------------------------------------------------

fn run_computation(state: &mut AppState) {
    let data = match state.sim.data.clone() { Some(d) => d, None => return };
    if state.sim.running { return; }
    if state.sim.rep_col.is_empty() || state.sim.x_col.is_empty() || state.sim.y_col.is_empty() {
        state.sim.status = "Select Replicate, X and Y columns first.".into();
        return;
    }
    let band_pcts: Vec<(f64, f64)> = state.sim.bands.iter()
        .filter(|b| b.visible)
        .map(|b| (b.lo_pct as f64, b.hi_pct as f64))
        .collect();
    if band_pcts.is_empty() {
        state.sim.status = "No visible PI bands.".into();
        return;
    }

    state.sim.running = true;
    state.sim.status  = "Computing quantiles…".into();

    let tx         = state.worker_tx.clone();
    let rep_col    = state.sim.rep_col.clone();
    let x_col      = state.sim.x_col.clone();
    let y_col      = state.sim.y_col.clone();
    let mdv_filter = state.sim.mdv_filter;
    let filters    = state.sim.filters.clone();

    std::thread::spawn(move || {
        match compute_quantiles(&data, &x_col, &y_col, &rep_col, &band_pcts, &filters, mdv_filter) {
            Ok(r)  => { let _ = tx.send(WorkerMsg::SimComplete(Box::new(r))); }
            Err(e) => { let _ = tx.send(WorkerMsg::SimError(e)); }
        }
    });
}

// ---------------------------------------------------------------------------
// Quantile computation
// ---------------------------------------------------------------------------

fn compute_quantiles(
    data:       &SimData,
    x_col:      &str,
    y_col:      &str,
    rep_col:    &str,
    band_pcts:  &[(f64, f64)],
    filters:    &[FilterRow],
    mdv_filter: bool,
) -> Result<SimPlotResult, String> {
    let x_arr   = data.col_data.get(x_col).ok_or_else(|| format!("Column '{x_col}' not found"))?;
    let y_arr   = data.col_data.get(y_col).ok_or_else(|| format!("Column '{y_col}' not found"))?;
    let rep_arr = data.col_data.get(rep_col).ok_or_else(|| format!("Column '{rep_col}' not found"))?;
    let n = data.n_rows;

    let mut mask = vec![true; n];

    if mdv_filter {
        if let Some(mdv) = data.col_data.get("MDV") {
            for i in 0..n { if mdv[i] == 1.0 { mask[i] = false; } }
        }
    }
    for fr in filters {
        if fr.col.is_empty() || fr.val.is_empty() { continue; }
        let col_data = match data.col_data.get(&fr.col) { Some(c) => c, None => continue };
        let nval = fr.val.parse::<f64>().ok();
        for i in 0..n {
            if !mask[i] { continue; }
            let cv = col_data[i];
            let keep = match (nval, fr.op.as_str()) {
                (Some(v), "==") => cv == v,
                (Some(v), "!=") => cv != v,
                (Some(v), ">")  => cv >  v,
                (Some(v), "<")  => cv <  v,
                (Some(v), ">=") => cv >= v,
                (Some(v), "<=") => cv <= v,
                _               => true,
            };
            if !keep { mask[i] = false; }
        }
    }

    let mut triples: Vec<(u64, u64, f64)> = Vec::with_capacity(n);
    for i in 0..n {
        if !mask[i] { continue; }
        let r = rep_arr[i]; let x = x_arr[i]; let y = y_arr[i];
        if r.is_nan() || x.is_nan() || y.is_nan() { continue; }
        triples.push((r.to_bits(), x.to_bits(), y));
    }
    if triples.is_empty() { return Err("No rows remain after filtering.".into()); }

    triples.sort_unstable_by_key(|(r, x, _)| (*r, *x));

    let mut rep_x_means: Vec<(u64, f64)> = Vec::new();
    let mut i = 0;
    while i < triples.len() {
        let (rep, x, _) = triples[i];
        let (mut sum, mut cnt) = (0.0f64, 0usize);
        while i < triples.len() && triples[i].0 == rep && triples[i].1 == x {
            sum += triples[i].2; cnt += 1; i += 1;
        }
        rep_x_means.push((x, sum / cnt as f64));
    }

    rep_x_means.sort_unstable_by_key(|(x, _)| *x);

    let mut times:  Vec<f64>      = Vec::new();
    let mut all_ys: Vec<Vec<f64>> = Vec::new();
    let mut j = 0;
    while j < rep_x_means.len() {
        let xb = rep_x_means[j].0;
        let mut ys = Vec::new();
        while j < rep_x_means.len() && rep_x_means[j].0 == xb {
            ys.push(rep_x_means[j].1); j += 1;
        }
        times.push(f64::from_bits(xb));
        all_ys.push(ys);
    }
    if times.is_empty() { return Err("No valid data after grouping.".into()); }

    for ys in &mut all_ys { ys.sort_unstable_by(f64::total_cmp); }

    let mut unique_pcts: Vec<f64> = vec![50.0];
    for (lo, hi) in band_pcts { unique_pcts.push(*lo); unique_pcts.push(*hi); }
    unique_pcts.sort_unstable_by(f64::total_cmp);
    unique_pcts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);

    let pct_arrays: HashMap<u64, Vec<f64>> = unique_pcts.iter().map(|&p| {
        let arr: Vec<f64> = all_ys.iter().map(|ys| percentile(ys, p / 100.0)).collect();
        (p.to_bits(), arr)
    }).collect();

    let med = pct_arrays[&50.0f64.to_bits()].clone();
    let bands = band_pcts.iter().map(|(lo, hi)| SimBandData {
        lo:  pct_arrays[&lo.to_bits()].clone(),
        med: med.clone(),
        hi:  pct_arrays[&hi.to_bits()].clone(),
    }).collect();

    Ok(SimPlotResult { times, bands })
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let n = sorted.len();
    if n == 0 { return f64::NAN; }
    if n == 1 { return sorted[0]; }
    let h = p * (n - 1) as f64;
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(n - 1);
    sorted[lo] * (1.0 - (h - lo as f64)) + sorted[hi] * (h - lo as f64)
}

// ---------------------------------------------------------------------------
// File parsing
// ---------------------------------------------------------------------------

fn parse_table_file(path: &Path) -> Result<SimData, String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    if is_nonmem_table(&content) {
        parse_nonmem_table(&content)
    } else {
        parse_csv_file(path)
    }
}

/// Returns true for space-delimited NONMEM output, with or without a TABLE header.
///
/// Two signals:
///  1. Any of the first 5 lines starts with "TABLE" (standard sdtab/patab).
///  2. The first non-empty line has more whitespace-separated tokens than
///     comma-separated ones, and at least 3 tokens — characteristic of NONMEM
///     files saved with a .csv extension but still space-delimited.
fn is_nonmem_table(content: &str) -> bool {
    let first_lines: Vec<&str> = content.lines().take(5).collect();
    if first_lines.iter().any(|l| l.trim_start().starts_with("TABLE")) {
        return true;
    }
    if let Some(line) = first_lines.iter().find(|l| !l.trim().is_empty()) {
        let n_space = line.split_whitespace().count();
        let n_comma = line.split(',').count();
        return n_space > n_comma && n_space > 2;
    }
    false
}

fn parse_nonmem_table(content: &str) -> Result<SimData, String> {
    let mut columns: Vec<String> = Vec::new();
    let mut col_vecs: Vec<Vec<f64>> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("TABLE") { continue; }
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        if tokens.is_empty() { continue; }

        if tokens.iter().any(|t| t.parse::<f64>().is_err()) {
            // Header line
            if columns.is_empty() {
                columns  = dedup_upper(tokens.iter().map(|t| t.to_uppercase()).collect());
                col_vecs = vec![Vec::new(); columns.len()];
            }
        } else if tokens.len() == columns.len() {
            for (i, tok) in tokens.iter().enumerate() {
                col_vecs[i].push(tok.parse::<f64>().unwrap_or(f64::NAN));
            }
        }
    }
    if columns.is_empty() { return Err("No header found in table file".into()); }
    let n_rows  = col_vecs.first().map(|v| v.len()).unwrap_or(0);
    let col_data = columns.iter().zip(col_vecs).map(|(k, v)| (k.clone(), v)).collect();
    Ok(SimData { columns, col_data, n_rows })
}

fn parse_csv_file(path: &Path) -> Result<SimData, String> {
    let mut rdr = csv::Reader::from_path(path).map_err(|e| e.to_string())?;
    let headers: Vec<String> = dedup_upper(
        rdr.headers().map_err(|e| e.to_string())?
            .iter().map(|h| h.to_uppercase()).collect(),
    );
    let mut col_vecs: Vec<Vec<f64>> = vec![Vec::new(); headers.len()];
    for record in rdr.records() {
        let record = record.map_err(|e| e.to_string())?;
        if record.len() != headers.len() { continue; }
        for (i, field) in record.iter().enumerate() {
            col_vecs[i].push(field.trim().parse::<f64>().unwrap_or(f64::NAN));
        }
    }
    let n_rows  = col_vecs.first().map(|v| v.len()).unwrap_or(0);
    let col_data = headers.iter().zip(col_vecs).map(|(k, v)| (k.clone(), v)).collect();
    Ok(SimData { columns: headers, col_data, n_rows })
}

fn dedup_upper(mut cols: Vec<String>) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for c in &mut cols {
        let cnt = seen.entry(c.clone()).or_insert(0);
        *cnt += 1;
        if *cnt > 1 { *c = format!("{c}_{cnt}"); }
    }
    cols
}

// ---------------------------------------------------------------------------
// Preset application
// ---------------------------------------------------------------------------

fn apply_preset(state: &mut AppState) {
    let idx = state.sim.preset_idx;
    if idx >= PRESETS.len() { return; }
    state.sim.bands.clear();
    for BandPreset(lo, hi, r, g, b, alpha) in PRESETS[idx] {
        state.sim.bands.push(BandSpec::new(*lo, *hi, egui::Color32::from_rgb(*r, *g, *b), *alpha));
    }
}

// ---------------------------------------------------------------------------
// Publication PNG export (plotters — white background, 300 DPI equivalent)
// ---------------------------------------------------------------------------

/// Render the sim plot to a publication-ready PNG using the `plotters` crate.
///
/// Output: 2400 × 1500 px (8 × 5 in at 300 DPI), white background.
/// Pure Rust — no R, no GPU, no screen capture.
///
/// # Fix notes
/// - Bands: `Polygon::new(poly, color.filled())` — the `.filled()` call is
///   critical; omitting it produces `filled: false` and the polygon is invisible.
/// - Legend: attached to the actual band `draw_series`, not a dummy zero-size
///   Rectangle.  Dummy series produce phantom legend entries.
/// - Dashes: each segment is a separate two-point `LineSeries`; no NaN hacks.
fn export_publication_png(
    sim:    &crate::domain::SimTabState,
    result: &crate::domain::SimPlotResult,
    path:   &std::path::Path,
    x_lbl:  &str,
    y_lbl:  &str,
) -> Result<(), String> {
    use plotters::prelude::*;

    // Register Ubuntu-Light exactly once per process.
    // The font is stored in assets/fonts/ within the repository — path is stable
    // across platforms, Rust versions, and registry configurations.
    // (Ubuntu Font Licence 1.0 permits redistribution.)
    static FONT_REGISTERED: std::sync::Once = std::sync::Once::new();
    FONT_REGISTERED.call_once(|| {
        const UBUNTU_LIGHT: &[u8] =
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts/Ubuntu-Light.ttf"));
        let _ = plotters::style::register_font(
            "sans-serif",
            plotters::style::FontStyle::Normal,
            UBUNTU_LIGHT,
        );
    });

    const W: u32 = 2400;
    const H: u32 = 1500;

    let root = BitMapBackend::new(path, (W, H)).into_drawing_area();
    root.fill(&WHITE).map_err(|e| e.to_string())?;

    let times = &result.times;
    if times.is_empty() { return Err("No time points to plot".into()); }

    let log_y = sim.log_y;
    let ly = |v: f64| -> f64 { if log_y && v > 0.0 { v.log10() } else { v } };

    // ── Compute bounds ────────────────────────────────────────────────────
    let mut y_min = f64::INFINITY;
    let mut y_max = f64::NEG_INFINITY;
    let mut absorb = |v: f64| { let lv = ly(v); if lv.is_finite() { y_min = y_min.min(lv); y_max = y_max.max(lv); }};
    for band in &result.bands {
        band.lo.iter().chain(&band.hi).chain(&band.med).for_each(|&v| absorb(v));
    }
    sim.ref_lines.iter().filter(|r| r.visible).for_each(|r| absorb(r.y));
    if let Some((_, oy)) = &sim.obs_xy { oy.iter().for_each(|&v| absorb(v)); }
    if !y_min.is_finite() || !y_max.is_finite() || (y_max - y_min).abs() < 1e-12 {
        y_min = 0.0; y_max = 1.0;
    }
    let pad_y = (y_max - y_min) * 0.08;
    let (y_lo, y_hi) = (y_min - pad_y, y_max + pad_y);

    let x_min = times.first().copied().unwrap_or(0.0);
    let x_max = times.last().copied().unwrap_or(1.0);
    let pad_x = (x_max - x_min) * 0.03;
    let (x_lo, x_hi) = (x_min - pad_x, x_max + pad_x);

    let y_lbl_str = if log_y { format!("log₁₀({})", y_lbl) } else { y_lbl.to_string() };

    // ── Build chart ───────────────────────────────────────────────────────
    let mut chart = ChartBuilder::on(&root)
        .margin(60)
        .x_label_area_size(80)
        .y_label_area_size(90)
        .build_cartesian_2d(x_lo..x_hi, y_lo..y_hi)
        .map_err(|e| e.to_string())?;

    chart.configure_mesh()
        .x_desc(x_lbl)
        .y_desc(y_lbl_str.as_str())
        .label_style(("sans-serif", 26))
        .axis_desc_style(("sans-serif", 30))
        .x_labels(8)
        .y_labels(8)
        .draw()
        .map_err(|e| e.to_string())?;

    // ── Bands (widest first → narrower bands render on top) ───────────────
    for (spec, band) in sim.bands.iter().zip(result.bands.iter()).rev() {
        if !spec.visible { continue; }
        let [r, g, b] = [spec.color.r(), spec.color.g(), spec.color.b()];
        let alpha_f = spec.alpha as f64;

        // FIX 1: must call .filled() — default ShapeStyle has filled:false,
        // which makes the polygon invisible regardless of color.
        let fill_style   = RGBAColor(r, g, b, alpha_f).filled();
        let legend_style = RGBAColor(r, g, b, 0.65_f64).filled();

        let lo_pts: Vec<(f64, f64)> = times.iter().zip(band.lo.iter())
            .map(|(&t, &v)| (t, ly(v))).filter(|(_, v)| v.is_finite()).collect();
        let hi_pts: Vec<(f64, f64)> = times.iter().zip(band.hi.iter())
            .map(|(&t, &v)| (t, ly(v))).filter(|(_, v)| v.is_finite()).collect();
        if lo_pts.len() < 2 || hi_pts.len() < 2 { continue; }

        // Close the polygon: lo forward, hi backward.
        let mut poly = lo_pts.clone();
        poly.extend(hi_pts.iter().rev());

        let label = format!("{}th\u{2013}{}th percentile", spec.lo_pct as u32, spec.hi_pct as u32);

        // FIX 2: attach legend to the actual polygon draw_series; no dummy series.
        chart.draw_series(std::iter::once(Polygon::new(poly, fill_style)))
            .map_err(|e| e.to_string())?
            .label(label)
            .legend(move |(x, y)| {
                Rectangle::new([(x, y - 7), (x + 22, y + 7)], legend_style)
            });
    }

    // ── Median line ───────────────────────────────────────────────────────
    if let Some((_, band)) = sim.bands.iter().zip(result.bands.iter()).find(|(s, _)| s.visible) {
        let med: Vec<(f64, f64)> = times.iter().zip(band.med.iter())
            .map(|(&t, &v)| (t, ly(v))).filter(|(_, v)| v.is_finite()).collect();
        let [r, g, b] = [sim.median_color.r(), sim.median_color.g(), sim.median_color.b()];
        chart.draw_series(LineSeries::new(med, RGBColor(r, g, b).stroke_width(5)))
            .map_err(|e| e.to_string())?
            .label("Median")
            .legend(move |(x, y)| PathElement::new(
                vec![(x, y), (x + 22, y)], RGBColor(r, g, b).stroke_width(5)
            ));
    }

    // ── Observed overlay ──────────────────────────────────────────────────
    if let Some((ox, oy)) = &sim.obs_xy {
        let pts: Vec<(f64, f64)> = ox.iter().zip(oy.iter())
            .map(|(&x, &y)| (x, ly(y))).filter(|(_, v)| v.is_finite()).collect();
        if !pts.is_empty() {
            chart.draw_series(pts.iter().map(|&(x, y)|
                Circle::new((x, y), 4, BLACK.mix(0.5).filled())
            )).map_err(|e| e.to_string())?
              .label("Observed")
              .legend(|(x, y)| Circle::new((x + 11, y), 5, BLACK.mix(0.5).filled()));
        }
    }

    // ── Reference / threshold lines ───────────────────────────────────────
    // FIX 5: each dash is a clean two-point LineSeries; no NaN segment hacks.
    for rl in &sim.ref_lines {
        if !rl.visible { continue; }
        let yv = ly(rl.y);
        if !yv.is_finite() || yv < y_lo || yv > y_hi { continue; }
        let [r, g, b] = [rl.color.r(), rl.color.g(), rl.color.b()];
        let line_style = RGBColor(r, g, b).stroke_width(3);
        let dash_len   = (x_hi - x_lo) / 50.0;
        let mut x = x_lo;
        let mut on = true;
        while x < x_hi {
            let xe = (x + dash_len).min(x_hi);
            if on {
                chart.draw_series(LineSeries::new(vec![(x, yv), (xe, yv)], line_style))
                    .map_err(|e| e.to_string())?;
            }
            x = xe;
            on = !on;
        }
        // Text annotation at right edge, slightly above the line.
        if !rl.label.is_empty() {
            let lbl = rl.label.clone();
            chart.draw_series(std::iter::once(
                Text::new(lbl,
                    (x_hi - pad_x * 1.5, yv + pad_y * 0.35),
                    ("sans-serif", 26).into_font().color(&RGBColor(r, g, b)))
            )).map_err(|e| e.to_string())?;
        }
    }

    // ── Legend ────────────────────────────────────────────────────────────
    chart.configure_series_labels()
        .background_style(WHITE.mix(0.85))
        .border_style(BLACK.mix(0.25))
        .label_font(("sans-serif", 26))
        .position(SeriesLabelPosition::UpperRight)
        .draw()
        .map_err(|e| e.to_string())?;

    root.present().map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// UI helpers
// ---------------------------------------------------------------------------

/// Collapsible section card with uppercase bold title, matching nmgui2's style.
fn section<R>(
    ui:       &mut egui::Ui,
    title:    &str,
    expanded: bool,
    dark:     bool,
    content:  impl FnOnce(&mut egui::Ui) -> R,
) {
    let border = if dark { egui::Color32::from_gray(55) } else { egui::Color32::from_gray(210) };
    egui::Frame::new()
        .fill(theme::card_fill(dark))
        .inner_margin(egui::Margin::same(0))
        .stroke(egui::Stroke::new(1.0, border))
        .corner_radius(egui::CornerRadius::same(5))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let header_resp = egui::CollapsingHeader::new(
                egui::RichText::new(title)
                    .size(11.0)
                    .strong()
                    .color(theme::fg2(dark)),
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
            let _ = header_resp;
        });
    ui.add_space(4.0);
}

/// Labeled combo that fills the remaining width.
fn labeled_combo(
    ui:      &mut egui::Ui,
    label:   &str,
    options: &[String],
    current: &mut String,
    dim:     egui::Color32,
) {
    ui.horizontal(|ui| {
        ui.add_sized(
            egui::vec2(95.0, 18.0),
            egui::Label::new(egui::RichText::new(label).color(dim).size(11.0)),
        );
        egui::ComboBox::from_id_salt(label)
            .selected_text(current.as_str())
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for opt in options {
                    ui.selectable_value(current, opt.clone(), opt);
                }
            });
    });
    ui.add_space(2.0);
}

/// Wide color swatch: draws a filled rectangle + opens the egui color picker on click.
fn wide_color_button(ui: &mut egui::Ui, color: &mut egui::Color32, width: f32) {
    let height = 20.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, height),
        egui::Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        // Checkerboard for alpha preview
        let tile = 5.0_f32;
        let nx = ((width / tile).ceil() as usize).max(1);
        let ny = ((height / tile).ceil() as usize).max(1);
        for ty in 0..ny {
            for tx in 0..nx {
                let checker = if (tx + ty) % 2 == 0 {
                    egui::Color32::from_gray(200)
                } else {
                    egui::Color32::WHITE
                };
                let r = egui::Rect::from_min_size(
                    rect.min + egui::vec2(tx as f32 * tile, ty as f32 * tile),
                    egui::vec2(tile, tile),
                ).intersect(rect);
                painter.rect_filled(r, 0.0, checker);
            }
        }
        painter.rect_filled(rect, 3.0, *color);
        let border = if response.hovered() {
            egui::Color32::from_gray(180)
        } else {
            egui::Color32::from_gray(100)
        };
        painter.rect_stroke(rect, 3.0, egui::Stroke::new(1.0, border), egui::StrokeKind::Middle);
    }

    // Open egui's built-in color picker popup on click
    let popup_id = ui.id().with(("sim_color_popup", rect.min.x as i32, rect.min.y as i32));
    if response.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }
    egui::popup::popup_below_widget(ui, popup_id, &response, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_width(230.0);
        egui::color_picker::color_picker_color32(ui, color, egui::color_picker::Alpha::Opaque);
    });

    response.on_hover_text("Click to change colour");
}
