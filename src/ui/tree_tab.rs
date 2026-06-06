/// Model ancestry / lineage tree.
///
/// Layout: left-to-right, depth on the x-axis.
///   Col 0 ("Base") = root models (no `based_on` or parent not in list).
///   Col 1 ("Gen 1") = direct children, etc.
///
/// Design adapted from NMGUI2's AncestryTreeWidget (PyQt6 → egui Painter).
use std::collections::HashMap;

use eframe::egui;

use crate::app::theme;
use crate::domain::ModelEntry;
use crate::state::AppState;

// ── Node geometry ─────────────────────────────────────────────────────────────

const NW: f32 = 150.0;   // node width
const NH: f32 = 60.0;    // node height
const HG: f32 = 88.0;    // horizontal gap between columns
const VG: f32 = 24.0;    // vertical gap between rows in same column
const GEN_LABEL_H: f32 = 22.0; // space above first row for generation label

// ── Layout computation ────────────────────────────────────────────────────────

struct NodePos {
    stem:  String,
    col:   usize,
    row:   usize,
}

struct Layout {
    nodes:    Vec<NodePos>,
    children: HashMap<String, Vec<String>>,
    parent:   HashMap<String, String>,
    n_cols:   usize,
}

impl Layout {
    fn logical_pos(&self, col: usize, row: usize) -> egui::Pos2 {
        egui::pos2(
            col as f32 * (NW + HG),
            GEN_LABEL_H + row as f32 * (NH + VG),
        )
    }

    fn node_rect(&self, col: usize, row: usize) -> egui::Rect {
        let tl = self.logical_pos(col, row);
        egui::Rect::from_min_size(tl, egui::vec2(NW, NH))
    }

    /// Total logical canvas size.
    fn canvas_size(&self) -> egui::Vec2 {
        let max_row_per_col: Vec<usize> = {
            let mut cnt = vec![0usize; self.n_cols.max(1)];
            for n in &self.nodes { cnt[n.col] = cnt[n.col].max(n.row + 1); }
            cnt
        };
        let max_rows = *max_row_per_col.iter().max().unwrap_or(&1);
        egui::vec2(
            self.n_cols as f32 * (NW + HG) - HG + 40.0,
            GEN_LABEL_H + max_rows as f32 * (NH + VG) - VG + 40.0,
        )
    }
}

fn compute_layout(models: &[ModelEntry]) -> Layout {
    let by_stem: HashMap<&str, usize> = models
        .iter().enumerate().map(|(i, m)| (m.model.stem.as_str(), i)).collect();

    let mut children: HashMap<String, Vec<String>> = models
        .iter().map(|m| (m.model.stem.clone(), vec![])).collect();
    let mut parent: HashMap<String, String> = HashMap::new();

    let mut roots = vec![];
    for m in models {
        match &m.meta.based_on {
            Some(p) if !p.is_empty() && by_stem.contains_key(p.as_str()) => {
                children.entry(p.clone()).or_default().push(m.model.stem.clone());
                parent.insert(m.model.stem.clone(), p.clone());
            }
            _ => roots.push(m.model.stem.clone()),
        }
    }
    if roots.is_empty() {
        roots = models.iter().map(|m| m.model.stem.clone()).collect();
    }

    // BFS to assign (col, row).
    let mut pos: HashMap<String, (usize, usize)> = HashMap::new();
    let mut depth_count: HashMap<usize, usize> = HashMap::new();
    let mut queue: std::collections::VecDeque<(String, usize)> =
        roots.iter().map(|r| (r.clone(), 0)).collect();

    while let Some((stem, depth)) = queue.pop_front() {
        if pos.contains_key(&stem) { continue; }
        let row = *depth_count.get(&depth).unwrap_or(&0);
        depth_count.insert(depth, row + 1);
        pos.insert(stem.clone(), (depth, row));
        for ch in children.get(&stem).cloned().unwrap_or_default() {
            if !pos.contains_key(&ch) { queue.push_back((ch, depth + 1)); }
        }
    }

    // Orphans (cycle / missing parent).
    let max_col = pos.values().map(|(c, _)| *c).max().unwrap_or(0) + 1;
    let mut orphan_row = 0usize;
    for m in models {
        if !pos.contains_key(&m.model.stem) {
            pos.insert(m.model.stem.clone(), (max_col, orphan_row));
            orphan_row += 1;
        }
    }

    let n_cols = pos.values().map(|(c, _)| *c).max().unwrap_or(0) + 1;
    let nodes = pos.into_iter()
        .map(|(stem, (col, row))| NodePos { stem, col, row })
        .collect();

    Layout { nodes, children, parent, n_cols }
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;

    if state.workspace.models.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new("No models loaded.\nOpen a directory from the Models tab.")
                .color(theme::fg3(dark)).size(13.0));
        });
        return;
    }

    let layout = compute_layout(&state.workspace.models);
    // Store indices so we can look up without holding &ModelEntry across the closure.
    let by_stem_idx: HashMap<String, usize> = state.workspace.models
        .iter().enumerate().map(|(i, m)| (m.model.stem.clone(), i)).collect();

    // Best OFV across all converged models.
    let best_ofv: Option<f64> = state.workspace.models.iter()
        .filter_map(|m| m.fit.as_ref().filter(|f| f.converged).map(|f| f.ofv))
        .reduce(f64::min);

    // Split: tree canvas on the left, info panel on the right.
    let total_w = ui.available_width();
    let info_w  = (total_w * 0.28).clamp(200.0, 320.0);
    let canvas_w = total_w - info_w - 6.0;
    let canvas_h = ui.available_height() - 40.0; // leave room for toolbar

    // ── Toolbar ───────────────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        // Legend.
        for (color, label) in [
            (theme::GREEN,    "Converged"),
            (theme::RED,      "Failed"),
            (theme::fg3(dark), "Not run"),
        ] {
            ui.label(egui::RichText::new("●").color(color).size(12.0));
            ui.label(egui::RichText::new(label).color(theme::fg2(dark)).size(11.0));
            ui.add_space(8.0);
        }
        ui.label(egui::RichText::new("!").color(theme::ORANGE).size(12.0).strong());
        ui.label(egui::RichText::new("Stale").color(theme::fg2(dark)).size(11.0));

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Export PNG")
                .on_hover_text("Save a PNG of the tree canvas to the working directory")
                .clicked()
            {
                state.ui.tree_export_pending = true;
            }
            ui.add_space(6.0);
            if ui.small_button("⊞ Fit").on_hover_text("Reset pan/zoom to fit all nodes").clicked() {
                state.ui.tree_pan  = egui::Vec2::ZERO;
                state.ui.tree_zoom = 0.0; // sentinel: will be set in canvas
            }
        });
    });
    ui.separator();

    // ── Main layout ───────────────────────────────────────────────────────────
    ui.horizontal_top(|ui| {
        // ── Canvas ───────────────────────────────────────────────────────────
        let canvas_size = egui::vec2(canvas_w, canvas_h);
        let (canvas_rect, canvas_resp) = ui.allocate_exact_size(
            canvas_size, egui::Sense::click_and_drag());
        state.ui.tree_canvas_rect = canvas_rect;

        // Auto-fit on first paint or when zoom is reset.
        if state.ui.tree_zoom <= 0.0 {
            let logical = layout.canvas_size();
            let zx = canvas_size.x / logical.x;
            let zy = canvas_size.y / logical.y;
            state.ui.tree_zoom = (zx.min(zy) * 0.92).clamp(0.2, 4.0);
            state.ui.tree_pan  = egui::Vec2::ZERO;
        }

        // Pan / zoom interaction.
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if canvas_rect.contains(ui.input(|i| i.pointer.hover_pos().unwrap_or_default())) && scroll != 0.0 {
            let factor = if scroll > 0.0 { 1.10f32 } else { 1.0 / 1.10 };
            state.ui.tree_zoom = (state.ui.tree_zoom * factor).clamp(0.15, 6.0);
        }
        if canvas_resp.dragged() {
            state.ui.tree_pan += canvas_resp.drag_delta() / state.ui.tree_zoom;
        }

        // Transform helpers.
        let zoom = state.ui.tree_zoom;
        let pan  = state.ui.tree_pan;
        let to_screen = |lp: egui::Pos2| -> egui::Pos2 {
            canvas_rect.min + egui::vec2((lp.x + pan.x) * zoom, (lp.y + pan.y) * zoom)
        };
        let to_logical = |sp: egui::Pos2| -> egui::Pos2 {
            egui::pos2((sp.x - canvas_rect.min.x) / zoom - pan.x,
                       (sp.y - canvas_rect.min.y) / zoom - pan.y)
        };

        let painter = ui.painter_at(canvas_rect);

        // Canvas background.
        painter.rect_filled(canvas_rect, 0.0, if dark { egui::Color32::from_rgb(0x10,0x13,0x1c) } else { egui::Color32::from_gray(245) });

        // Mouse position in logical coords.
        let mouse_logical = ui.input(|i| i.pointer.hover_pos())
            .map(|sp| to_logical(sp));

        let mut new_hovered: Option<String> = None;
        let mut clicked_stem: Option<String> = None;

        // ── Generation column labels ──────────────────────────────────────────
        for col in 0..layout.n_cols {
            let label = if col == 0 { "Base".to_string() } else { format!("Gen {col}") };
            let lx = col as f32 * (NW + HG) + NW * 0.5;
            let sp = to_screen(egui::pos2(lx, 8.0));
            painter.text(
                sp, egui::Align2::CENTER_CENTER, &label,
                egui::FontId::proportional(10.0 * zoom.clamp(0.5, 2.0)),
                theme::fg2(dark),
            );
        }

        // ── Edges + ΔOFV labels ───────────────────────────────────────────────
        let edge_color = if dark { egui::Color32::from_gray(70) } else { egui::Color32::from_gray(180) };

        for node in &layout.nodes {
            let parent_stem = match layout.parent.get(&node.stem) {
                Some(p) => p,
                None    => continue,
            };
            let parent_pos = layout.nodes.iter()
                .find(|n| &n.stem == parent_stem)
                .map(|n| (n.col, n.row));
            let (pcol, prow) = match parent_pos { Some(p) => p, None => continue };

            let pr = layout.node_rect(pcol, prow);
            let cr = layout.node_rect(node.col, node.row);

            // Edge endpoints: right-centre of parent, left-centre of child.
            let p0 = to_screen(pr.right_center());
            let p3 = to_screen(cr.left_center());
            let mid_x_s = (p0.x + p3.x) * 0.5;
            let cp1 = egui::pos2(mid_x_s, p0.y);
            let cp2 = egui::pos2(mid_x_s, p3.y);

            painter.add(egui::Shape::CubicBezier(egui::epaint::CubicBezierShape {
                points:  [p0, cp1, cp2, p3],
                closed:  false,
                fill:    egui::Color32::TRANSPARENT,
                stroke:  egui::epaint::PathStroke::new(1.5 * zoom.clamp(0.4, 2.0), edge_color),
            }));

            // ΔOFV pill at edge midpoint.
            let m_idx = by_stem_idx.get(&node.stem);
            let p_idx = by_stem_idx.get(parent_stem);
            if let (Some(&mi), Some(&pi)) = (m_idx, p_idx) {
                let (cf, pf) = (state.workspace.models[mi].fit.as_ref(),
                                state.workspace.models[pi].fit.as_ref());
                if let (Some(cf), Some(pf)) = (cf, pf) {
                    let delta = cf.ofv - pf.ofv;
                    let d_str = format!("{delta:+.1}");
                    let d_col = if delta < -3.84 { theme::GREEN }
                                else if delta > 0.5 { theme::ORANGE }
                                else { theme::fg2(dark) };
                    let mid = egui::pos2((p0.x + p3.x) * 0.5, (p0.y + p3.y) * 0.5);
                    let fsize = (9.0 * zoom).clamp(6.0, 14.0);
                    // Pill background.
                    let pill_rect = egui::Rect::from_center_size(mid, egui::vec2(fsize * 3.5, fsize * 1.5));
                    let pill_bg = if dark { egui::Color32::from_rgb(0x10,0x13,0x1c) } else { egui::Color32::from_gray(245) };
                    painter.rect_filled(pill_rect, 3.0, pill_bg);
                    painter.rect_stroke(pill_rect, 3.0, egui::Stroke::new(0.8, edge_color), egui::StrokeKind::Middle);
                    painter.text(mid, egui::Align2::CENTER_CENTER, &d_str,
                        egui::FontId::proportional(fsize), d_col);
                }
            }
        }

        // ── Nodes ─────────────────────────────────────────────────────────────
        let selected_stem = state.ui.selected_model
            .and_then(|i| state.workspace.models.get(i))
            .map(|m| m.model.stem.clone());

        for node in &layout.nodes {
            let entry_idx = match by_stem_idx.get(&node.stem) { Some(&i) => i, None => continue };
            let entry     = &state.workspace.models[entry_idx];
            let lr        = layout.node_rect(node.col, node.row);
            let sr       = egui::Rect::from_min_max(to_screen(lr.min), to_screen(lr.max));

            let has_run  = entry.fit.is_some();
            let converged = entry.fit.as_ref().map(|f| f.converged).unwrap_or(false);
            let failed   = has_run && !converged;
            let is_stale = entry.is_stale;
            let is_sel   = selected_stem.as_deref() == Some(&node.stem);
            let is_hov   = mouse_logical.map(|ml| lr.contains(ml)).unwrap_or(false);

            if is_hov { new_hovered = Some(node.stem.clone()); }

            // Background fill.
            let bg = if is_sel {
                egui::Color32::from_rgba_unmultiplied(0x4c, 0x8a, 0xff, 60)
            } else if converged {
                if dark { egui::Color32::from_rgba_unmultiplied(0x3e, 0xc9, 0x7a, 25) }
                else    { egui::Color32::from_rgba_unmultiplied(0x3e, 0xc9, 0x7a, 35) }
            } else if failed {
                if dark { egui::Color32::from_rgba_unmultiplied(0xe8, 0x55, 0x55, 25) }
                else    { egui::Color32::from_rgba_unmultiplied(0xe8, 0x55, 0x55, 35) }
            } else {
                theme::card_fill(dark)
            };
            painter.rect_filled(sr, 5.0 * zoom.clamp(0.3, 2.0), bg);

            // Border.
            let bw  = if is_sel || is_hov { 2.0 } else { 1.0 };
            let bc  = if is_sel { theme::ACCENT } else if is_hov { theme::fg2(dark) } else { theme::fg3(dark) };
            let bstyle = if !has_run {
                // Dashed border for unrun models — approximate with dotted segments.
                // egui doesn't have native dashed stroke, so we just use thinner border.
                egui::Stroke::new(bw * 0.7, egui::Color32::from_rgba_unmultiplied(bc.r(), bc.g(), bc.b(), 120))
            } else {
                egui::Stroke::new(bw, bc)
            };
            painter.rect_stroke(sr, 5.0 * zoom.clamp(0.3, 2.0), bstyle, egui::StrokeKind::Middle);

            // Status dot (top-right corner).
            let dot_col = if converged { theme::GREEN } else if failed { theme::RED } else { theme::fg3(dark) };
            let dot_r   = 4.5 * zoom.clamp(0.3, 1.5);
            painter.circle_filled(
                to_screen(egui::pos2(lr.max.x - 9.0, lr.min.y + 9.0)), dot_r, dot_col);

            // Star (top-left).
            let mut txt_x = lr.min.x + 5.0;
            if entry.meta.starred {
                painter.text(to_screen(egui::pos2(lr.min.x + 4.0, lr.min.y + 2.0)),
                    egui::Align2::LEFT_TOP, "★",
                    egui::FontId::proportional(10.0 * zoom.clamp(0.4, 2.0)),
                    egui::Color32::from_rgb(0xf0, 0xc0, 0x40));
                txt_x += 13.0;
            }

            // Stale badge (!).
            if is_stale {
                painter.text(
                    to_screen(egui::pos2(lr.max.x - 22.0, lr.min.y + 2.0)),
                    egui::Align2::LEFT_TOP, "!",
                    egui::FontId::proportional(11.0 * zoom.clamp(0.4, 2.0)),
                    theme::ORANGE,
                );
            }

            let fg   = theme::fg(dark);
            let fg2c = theme::fg2(dark);
            let fscale = zoom.clamp(0.4, 2.0);

            // Line 1 — stem name (bold, truncated).
            let max_name_w = (lr.width() - txt_x + lr.min.x - 18.0) / zoom;
            let display_stem = truncate_to_width(&node.stem, max_name_w, 10.0 * fscale);
            painter.text(
                to_screen(egui::pos2(txt_x, lr.min.y + 4.0)),
                egui::Align2::LEFT_TOP, &display_stem,
                egui::FontId::proportional(10.5 * fscale), fg,
            );

            // Line 2 — OFV + Δbest.
            if let Some(fit) = &entry.fit {
                let mut ofv_str = format!("{:.2}", fit.ofv);
                if let Some(best) = best_ofv {
                    let d = fit.ofv - best;
                    let suffix = if d.abs() < 0.01 { "  ★".to_string() } else { format!("  Δ{d:+.1}") };
                    ofv_str += &suffix;
                }
                painter.text(
                    to_screen(egui::pos2(txt_x, lr.min.y + 21.0)),
                    egui::Align2::LEFT_TOP, &ofv_str,
                    egui::FontId::proportional(8.5 * fscale), fg2c,
                );

                // Line 3 — method + COV.
                let meth = fit.method.chars().take(7).collect::<String>().to_uppercase();
                painter.text(
                    to_screen(egui::pos2(txt_x, lr.min.y + 36.0)),
                    egui::Align2::LEFT_TOP, &meth,
                    egui::FontId::proportional(7.5 * fscale), fg2c,
                );
                let cov_str = if fit.covariance_ok { "✓" } else { "✗" };
                let cov_col = if fit.covariance_ok { theme::GREEN } else { theme::RED };
                painter.text(
                    to_screen(egui::pos2(lr.max.x - 16.0, lr.min.y + 35.0)),
                    egui::Align2::LEFT_TOP, cov_str,
                    egui::FontId::proportional(9.0 * fscale), cov_col,
                );
            }

            // Click handling.
            if canvas_resp.clicked() {
                if let Some(ml) = mouse_logical {
                    if lr.contains(ml) { clicked_stem = Some(node.stem.clone()); }
                }
            }
        }

        // Apply hover/click results.
        state.ui.tree_hovered = new_hovered;
        if let Some(stem) = clicked_stem {
            if let Some(idx) = state.workspace.models.iter().position(|m| m.model.stem == stem) {
                state.ui.selected_model = Some(idx);
            }
        }

        // ── Info panel ────────────────────────────────────────────────────────
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.set_width(info_w);
            let display_stem = state.ui.tree_hovered.clone()
                .or_else(|| selected_stem.clone());

            if let Some(stem) = display_stem {
                if let Some(&idx) = by_stem_idx.get(&stem) {
                    // Can't pass &state.workspace.models[idx] alongside &mut state,
                    // so pass the index and borrow inside show_info_panel.
                    show_info_panel(ui, idx, state, &layout, dark);
                }
            } else {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("Hover or click a node\nto see model details")
                        .color(theme::fg3(dark)).size(11.0));
                });
            }
        });
    });
}

// ── Info panel ────────────────────────────────────────────────────────────────

fn show_info_panel(
    ui:         &mut egui::Ui,
    entry_idx:  usize,
    state:      &AppState,
    layout:     &Layout,
    dark:       bool,
) {
    let entry = &state.workspace.models[entry_idx];
    let fit   = entry.fit.as_ref();
    let meta  = &entry.meta;
    let dim  = theme::fg2(dark);
    let dim3 = theme::fg3(dark);

    egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
        // ── Header ────────────────────────────────────────────────────────────
        ui.horizontal(|ui| {
            if entry.meta.starred {
                ui.label(egui::RichText::new("★").color(egui::Color32::from_rgb(0xf0,0xc0,0x40)).size(14.0));
            }
            ui.label(egui::RichText::new(&entry.model.stem).size(14.0).strong().color(theme::fg(dark)));
        });

        // Decision badge.
        let (dec_label, dec_col) = match meta.decision {
            crate::domain::ModelDecision::Include     => ("Include",     theme::GREEN),
            crate::domain::ModelDecision::Rejected    => ("Rejected",    theme::RED),
            crate::domain::ModelDecision::Sensitivity => ("Sensitivity", theme::fg2(dark)),
            crate::domain::ModelDecision::Exploratory => ("Exploratory", theme::fg2(dark)),
        };
        ui.label(egui::RichText::new(dec_label).size(10.0).color(dec_col));

        if !meta.comment.is_empty() {
            ui.add_space(2.0);
            ui.label(egui::RichText::new(&meta.comment).size(11.0).color(dim).italics());
        }

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(4.0);

        // ── Fit results ───────────────────────────────────────────────────────
        if let Some(f) = fit {
            let status_col = if f.converged { theme::GREEN } else { theme::RED };
            let status_str = if f.converged { "✓ Converged" } else { "✗ Not converged" };
            info_row(ui, "Status",  status_str, status_col, dark);
            info_row(ui, "Method",  &f.method.to_uppercase(), theme::fg(dark), dark);
            info_row(ui, "OFV",     &format!("{:.4}", f.ofv), theme::fg(dark), dark);
            info_row(ui, "AIC",     &format!("{:.2}", f.aic), dim, dark);
            info_row(ui, "BIC",     &format!("{:.2}", f.bic), dim, dark);
            info_row(ui, "Params",  &f.n_parameters.to_string(), dim, dark);
            info_row(ui, "Subjects",&f.n_subjects.to_string(), dim, dark);
            info_row(ui, "Obs",     &f.n_obs.to_string(), dim, dark);
            info_row(ui, "Iters",   &f.n_iterations.to_string(), dim, dark);

            if f.wall_time_secs > 0.0 {
                let rt = if f.wall_time_secs < 60.0 { format!("{:.0}s", f.wall_time_secs) }
                         else { format!("{:.1}min", f.wall_time_secs / 60.0) };
                info_row(ui, "Runtime", &rt, dim, dark);
            }

            // COV + CN
            let cov_col = if f.covariance_ok { theme::GREEN } else { theme::RED };
            info_row(ui, "Covariance",
                if f.covariance_ok { "✓ OK" } else { "✗ Failed" }, cov_col, dark);
            if f.cov_condition_number.is_finite() {
                let cn_col = if f.cov_condition_number > 1000.0 { theme::ORANGE } else { dim };
                info_row(ui, "Cond. number", &format!("{:.0}", f.cov_condition_number), cn_col, dark);
            }

            // DW
            if let Some(dw) = f.dw_statistic {
                let dw_col = if dw < 1.5 || dw > 2.5 { theme::ORANGE } else { theme::GREEN };
                info_row(ui, "Durbin-Watson", &format!("{dw:.3}"), dw_col, dark);
            }

            // Shrinkage.
            if !f.eta_shrinkage.is_empty() {
                let max_s = f.eta_shrinkage.iter().cloned().fold(f64::NAN, f64::max);
                let s_col = if max_s > 30.0 { theme::ORANGE } else { dim };
                info_row(ui, "Max η shrink", &format!("{:.1}%", max_s * 100.0), s_col, dark);
            }

            // ΔOFV vs parent.
            if let Some(parent_stem) = layout.parent.get(&entry.model.stem) {
                if let Some(parent_entry) = state.workspace.models.iter()
                    .find(|m| &m.model.stem == parent_stem)
                {
                    if let Some(pf) = &parent_entry.fit {
                        let d   = f.ofv - pf.ofv;
                        let col = if d < -3.84 { theme::GREEN } else if d > 0.5 { theme::RED } else { dim };
                        info_row(ui, "ΔOFV vs parent", &format!("{d:+.3}"), col, dark);
                    }
                }
            }

        } else {
            ui.label(egui::RichText::new("Not yet estimated").color(dim3).size(11.0));
        }

        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);

        // ── Timestamps ───────────────────────────────────────────────────────
        if let Some(ref ts) = entry.model.created_at {
            info_row(ui, "Created", ts, dim, dark);
        }

        // Last run from run history.
        let last_run = state.run.run_history.iter().rev()
            .find(|r| r.model_stem == entry.model.stem && r.completed.is_some())
            .and_then(|r| r.completed.as_deref());
        if let Some(ts) = last_run {
            // Show only date+time (first 16 chars of ISO 8601).
            let short = &ts[..ts.len().min(16)];
            info_row(ui, "Last run", short, dim, dark);
        }

        // Lineage.
        if let Some(p) = layout.parent.get(&entry.model.stem) {
            info_row(ui, "Based on", p, theme::ACCENT, dark);
        }

        // Children count.
        let n_ch = layout.children.get(&entry.model.stem).map(|v| v.len()).unwrap_or(0);
        if n_ch > 0 {
            info_row(ui, "Children", &n_ch.to_string(), dim, dark);
        }

        // Stale warning.
        if entry.is_stale {
            ui.add_space(4.0);
            ui.label(egui::RichText::new("⚠ Model file was edited after the last fit — results may be stale")
                .color(theme::ORANGE).size(10.0));
        }

        // Notes.
        if !meta.notes.is_empty() {
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(2.0);
            ui.label(egui::RichText::new(&meta.notes).size(10.5).color(dim));
        }
    });
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn info_row(ui: &mut egui::Ui, key: &str, val: &str, val_color: egui::Color32, dark: bool) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(key).size(10.5).color(theme::fg2(dark)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(val).size(10.5).color(val_color).monospace());
        });
    });
}

fn truncate_to_width(s: &str, max_logical_w: f32, char_w: f32) -> String {
    let max_chars = (max_logical_w / char_w).max(4.0) as usize;
    if s.len() <= max_chars { s.to_string() }
    else { format!("{}…", &s[..max_chars.saturating_sub(1)]) }
}
