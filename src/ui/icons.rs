use eframe::egui;
use crate::state::Tab;

// ── FeRxGUI brand mark ──────────────────────────────────────────────────────

/// Renders the FeRxGUI brand inline: a small PK-curve mark followed by the
/// "FeRxGUI" wordmark.  Mirrors the application icon.
///
/// `font_size` is the point size of the wordmark text; the mark scales with it.
pub fn show_ferx_logo(ui: &mut egui::Ui, font_size: f32) {
    let dark = ui.visuals().dark_mode;

    // "FeRx" is dark navy on light, near-white on dark.
    let fe_color = if dark {
        egui::Color32::from_rgb(0xdd, 0xe2, 0xf0)
    } else {
        egui::Color32::from_rgb(0x1d, 0x26, 0x3a)
    };
    // Rust/sienna orange — identical in both themes (matches the curve).
    let rx_color = egui::Color32::from_rgb(0xbf, 0x5e, 0x2a);
    let axis_color = if dark {
        egui::Color32::from_rgb(0x6a, 0x6d, 0x88)
    } else {
        egui::Color32::from_gray(170)
    };

    // ── PK-curve mark ──
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(font_size * 1.55, font_size * 1.45),
        egui::Sense::hover(),
    );
    paint_pk_mark(ui.painter(), rect, rx_color, axis_color);

    // ── "FeRxGUI" wordmark ──
    let prev = ui.spacing().item_spacing.x;
    ui.spacing_mut().item_spacing.x = 4.0;
    ui.label(
        egui::RichText::new("FeRx").size(font_size).strong().color(fe_color),
    );
    ui.spacing_mut().item_spacing.x = 0.0;
    ui.label(
        egui::RichText::new("GUI").size(font_size).strong().color(rx_color),
    );
    ui.spacing_mut().item_spacing.x = prev;
}

/// Paint a tiny pharmacokinetic concentration–time curve (Bateman shape) with a
/// baseline and a peak dot, filling `rect`.
fn paint_pk_mark(
    painter: &egui::Painter,
    rect: egui::Rect,
    curve: egui::Color32,
    axis: egui::Color32,
) {
    let x0 = rect.left() + rect.width() * 0.08;
    let x1 = rect.right() - rect.width() * 0.06;
    let y_bot = rect.bottom() - rect.height() * 0.16;
    let y_top = rect.top() + rect.height() * 0.10;

    // Baseline (x-axis).
    painter.line_segment(
        [egui::pos2(x0, y_bot), egui::pos2(x1, y_bot)],
        egui::Stroke::new(1.0, axis),
    );

    // Bateman curve: C(t) = e^-ke·t − e^-ka·t, normalised to the peak.
    let ka = 1.05_f32; let ke = 0.30_f32; let t_max = 14.0_f32;
    let n = 48;
    let mut cmax = 0.0_f32;
    for i in 0..=n {
        let t = t_max * i as f32 / n as f32;
        let c = (-ke * t).exp() - (-ka * t).exp();
        if c > cmax { cmax = c; }
    }
    let mut pts = Vec::with_capacity(n + 1);
    let mut peak = egui::pos2(x0, y_bot);
    for i in 0..=n {
        let t = t_max * i as f32 / n as f32;
        let c = (-ke * t).exp() - (-ka * t).exp();
        let x = x0 + (x1 - x0) * (t / t_max);
        let y = y_bot - (y_bot - y_top) * (c / cmax);
        let p = egui::pos2(x, y);
        if (c - cmax).abs() < 1e-6 { peak = p; }
        pts.push(p);
    }
    painter.add(egui::Shape::line(pts, egui::Stroke::new(1.8, curve)));
    painter.circle_filled(peak, 2.0, curve);
}

/// Paint a tab icon centred at `center`.  `s` is the half-size; the icon
/// fits in a roughly `2s × 2s` box.  All shapes are pure geometry — no fonts,
/// no emoji — so they render identically on every OS.
pub fn paint_tab_icon(
    painter: &egui::Painter,
    tab: Tab,
    center: egui::Pos2,
    s: f32,
    color: egui::Color32,
) {
    let st = egui::Stroke::new(1.5, color);
    let cx = center.x;
    let cy = center.y;
    match tab {
        Tab::Models      => models(painter, cx, cy, s, color),
        Tab::Files       => files(painter, cx, cy, s, st),
        Tab::Tree        => tree(painter, cx, cy, s, st),
        Tab::Evaluation  => evaluation(painter, cx, cy, s, color),
        Tab::Vpc         => vpc(painter, cx, cy, s, st),
        Tab::Uncertainty => uncertainty(painter, cx, cy, s, color, st),
        Tab::SimPlot     => simplot(painter, cx, cy, s, st),
        Tab::History     => history(painter, cx, cy, s, color, st),
        Tab::Settings    => settings(painter, cx, cy, s, color, st),
    }
}

// ── helper: offset from (cx,cy) ──────────────────────────────────────────────
#[inline]
fn p(cx: f32, cy: f32, dx: f32, dy: f32) -> egui::Pos2 {
    egui::pos2(cx + dx, cy + dy)
}

// ── helper: polyline through a list of Pos2 ──────────────────────────────────
fn poly(painter: &egui::Painter, pts: Vec<egui::Pos2>, st: egui::Stroke) {
    for i in 0..pts.len().saturating_sub(1) {
        painter.line_segment([pts[i], pts[i + 1]], st);
    }
}

// ── Models: 2×2 grid of filled rounded squares (data table) ──────────────────
fn models(painter: &egui::Painter, cx: f32, cy: f32, s: f32, color: egui::Color32) {
    let sq = s * 0.46;
    let off = sq + s * 0.09;
    for (dx, dy) in [(-off, -off), (off, -off), (-off, off), (off, off)] {
        painter.rect_filled(
            egui::Rect::from_center_size(egui::pos2(cx + dx, cy + dy), egui::vec2(sq * 1.85, sq * 1.85)),
            2.0_f32,
            color,
        );
    }
}

// ── Files: folder outline with tab ───────────────────────────────────────────
fn files(painter: &egui::Painter, cx: f32, cy: f32, s: f32, st: egui::Stroke) {
    let w = s * 1.45;
    let h = s * 1.1;
    let body_top = cy - h * 0.38;
    // folder body (draw manually to avoid StrokeKind API)
    let body = egui::Rect::from_center_size(egui::pos2(cx, cy + s * 0.1), egui::vec2(w, h));
    painter.line_segment([body.left_top(),   body.right_top()],    st);
    painter.line_segment([body.right_top(),  body.right_bottom()], st);
    painter.line_segment([body.right_bottom(),body.left_bottom()], st);
    painter.line_segment([body.left_bottom(), body.left_top()],    st);
    // tab on top-left
    let tab_h = s * 0.3;
    let tab_w = w * 0.4;
    let lx = cx - w / 2.0;
    poly(painter, vec![
        p(lx, body_top, 0.0, 0.0),
        p(lx, body_top, 0.0, -tab_h),
        p(lx + tab_w, body_top, 0.0, -tab_h),
        p(lx + tab_w + tab_h * 0.6, body_top, 0.0, 0.0),
    ], st);
}

// ── Tree: root + two children connected by a T-junction ──────────────────────
fn tree(painter: &egui::Painter, cx: f32, cy: f32, s: f32, st: egui::Stroke) {
    let r = s * 0.24;
    let root  = egui::pos2(cx, cy - s * 0.52);
    let left  = egui::pos2(cx - s * 0.55, cy + s * 0.45);
    let right = egui::pos2(cx + s * 0.55, cy + s * 0.45);
    painter.circle_stroke(root,  r, st);
    painter.circle_stroke(left,  r, st);
    painter.circle_stroke(right, r, st);
    let mid_y = root.y + (left.y - root.y) * 0.55;
    painter.line_segment([p(cx, root.y, 0.0, r),  egui::pos2(cx, mid_y)], st);
    painter.line_segment([egui::pos2(left.x, mid_y), egui::pos2(right.x, mid_y)], st);
    painter.line_segment([egui::pos2(left.x, mid_y),  p(left.x,  left.y,  0.0, -r)], st);
    painter.line_segment([egui::pos2(right.x, mid_y), p(right.x, right.y, 0.0, -r)], st);
}

// ── Evaluation: 3 ascending filled bars + baseline ───────────────────────────
fn evaluation(painter: &egui::Painter, cx: f32, cy: f32, s: f32, color: egui::Color32) {
    let bottom = cy + s * 0.62;
    let bw     = s * 0.32;
    let gap    = s * 0.15;
    let total  = 3.0 * bw + 2.0 * gap;
    let x0     = cx - total / 2.0 + bw / 2.0;
    for (i, h_frac) in [0.45_f32, 0.72, 1.1].iter().enumerate() {
        let x = x0 + i as f32 * (bw + gap);
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(x - bw / 2.0, bottom - s * h_frac),
                egui::pos2(x + bw / 2.0, bottom),
            ),
            1.0_f32,
            color,
        );
    }
}

// ── VPC: median curve + upper/lower percentile curves ────────────────────────
fn vpc(painter: &egui::Painter, cx: f32, cy: f32, s: f32, st: egui::Stroke) {
    // median line (thick)
    poly(painter, vec![
        p(cx, cy, -s * 0.72, s * 0.28),
        p(cx, cy, -s * 0.24, -s * 0.05),
        p(cx, cy,  s * 0.24, -s * 0.15),
        p(cx, cy,  s * 0.72, -s * 0.38),
    ], st);
    // percentile lines (thin)
    let thin = egui::Stroke::new(1.0, st.color);
    poly(painter, vec![
        p(cx, cy, -s * 0.72, -s * 0.12),
        p(cx, cy, -s * 0.24, -s * 0.45),
        p(cx, cy,  s * 0.24, -s * 0.55),
        p(cx, cy,  s * 0.72, -s * 0.78),
    ], thin);
    poly(painter, vec![
        p(cx, cy, -s * 0.72,  s * 0.62),
        p(cx, cy, -s * 0.24,  s * 0.34),
        p(cx, cy,  s * 0.24,  s * 0.25),
        p(cx, cy,  s * 0.72,  s * 0.02),
    ], thin);
}

// ── Uncertainty (SIR): bell-curve outline + peak dot ─────────────────────────
fn uncertainty(painter: &egui::Painter, cx: f32, cy: f32, s: f32, color: egui::Color32, st: egui::Stroke) {
    poly(painter, vec![
        p(cx, cy, -s * 0.75,  s * 0.5),
        p(cx, cy, -s * 0.45,  s * 0.18),
        p(cx, cy,  0.0,       -s * 0.55),
        p(cx, cy,  s * 0.45,  s * 0.18),
        p(cx, cy,  s * 0.75,  s * 0.5),
    ], st);
    // baseline
    painter.line_segment([p(cx, cy, -s * 0.75, s * 0.5), p(cx, cy, s * 0.75, s * 0.5)], st);
    // peak dot
    painter.circle_filled(p(cx, cy, 0.0, -s * 0.55), s * 0.16, color);
}

// ── SimPlot: observed trace (bold) + one simulation trace (thin) ─────────────
fn simplot(painter: &egui::Painter, cx: f32, cy: f32, s: f32, st: egui::Stroke) {
    poly(painter, vec![
        p(cx, cy, -s * 0.72,  s * 0.32),
        p(cx, cy, -s * 0.36, -s * 0.42),
        p(cx, cy,  0.0,        s * 0.08),
        p(cx, cy,  s * 0.36,  -s * 0.52),
        p(cx, cy,  s * 0.72,  -s * 0.18),
    ], st);
    let thin = egui::Stroke::new(1.0, st.color);
    poly(painter, vec![
        p(cx, cy, -s * 0.72,  s * 0.58),
        p(cx, cy, -s * 0.36, -s * 0.1),
        p(cx, cy,  0.0,        s * 0.32),
        p(cx, cy,  s * 0.36,  -s * 0.22),
        p(cx, cy,  s * 0.72,   s * 0.08),
    ], thin);
}

// ── History: clock face with 10:10 hands ─────────────────────────────────────
fn history(painter: &egui::Painter, cx: f32, cy: f32, s: f32, color: egui::Color32, st: egui::Stroke) {
    painter.circle_stroke(egui::pos2(cx, cy), s * 0.72, st);
    painter.line_segment([egui::pos2(cx, cy), p(cx, cy, -s * 0.33, -s * 0.5)], st);
    painter.line_segment([egui::pos2(cx, cy), p(cx, cy,  s * 0.33, -s * 0.5)], st);
    painter.circle_filled(egui::pos2(cx, cy), s * 0.12, color);
}

// ── Settings: three horizontal slider lines with knobs ───────────────────────
fn settings(painter: &egui::Painter, cx: f32, cy: f32, s: f32, color: egui::Color32, st: egui::Stroke) {
    let hw = s * 0.74;
    let r  = s * 0.24;
    let ys   = [cy - s * 0.56, cy, cy + s * 0.56];
    let kxs  = [cx - s * 0.22, cx + s * 0.28, cx - s * 0.08];
    for (&y, &kx) in ys.iter().zip(kxs.iter()) {
        painter.line_segment([egui::pos2(cx - hw, y), egui::pos2(cx + hw, y)], st);
        painter.circle_stroke(egui::pos2(kx, y), r, st);
        painter.circle_filled(egui::pos2(kx, y), r * 0.45, color);
    }
}
