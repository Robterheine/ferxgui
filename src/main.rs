mod app;
mod domain;
mod io;
mod notify;
mod state;
mod ui;
mod workers;

#[cfg(test)]
mod glyph_coverage_tests {
    use std::path::Path;

    /// U+2713 (✓ CHECK MARK) and U+2717 (✗ BALLOT X) are not covered by any
    /// font bundled by egui's `default_fonts` feature — confirmed by direct
    /// cmap inspection of the four font files it embeds (Ubuntu-Light,
    /// NotoEmoji-Regular, emoji-icon-font, Hack-Regular). Any occurrence in
    /// UI-rendered text shows as an empty "tofu" box (reported bug, found in
    /// 7 files across the codebase). Use ✔ (U+2714 HEAVY CHECK MARK) / ✖
    /// (U+2716 HEAVY MULTIPLICATION X) instead — both confirmed covered.
    ///
    /// `notify.rs` is exempt: it only builds strings for OS-native
    /// notifications (osascript / notify-send / PowerShell toast), which
    /// render with the OS's own fonts, not egui's — not subject to this gap.
    #[test]
    fn no_uncovered_check_or_cross_glyphs_in_ui_source() {
        let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        visit(&src_dir, &mut offenders);
        assert!(
            offenders.is_empty(),
            "found uncovered U+2713 (✓) / U+2717 (✗) glyphs in: {offenders:#?}\n\
             use ✔ (U+2714) / ✖ (U+2716) instead"
        );
    }

    fn visit(dir: &Path, offenders: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, offenders);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let file_name = path.file_name().and_then(|n| n.to_str());
            // notify.rs: OS-native notification text, not egui-rendered (see
            // doc comment above). main.rs: this test's own doc comment and
            // assert message reference the glyphs by name for documentation
            // purposes, which isn't a real occurrence to flag.
            if file_name == Some("notify.rs") || file_name == Some("main.rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            for (i, line) in text.lines().enumerate() {
                if line.contains('\u{2713}') || line.contains('\u{2717}') {
                    offenders.push(format!("{}:{}", path.display(), i + 1));
                }
            }
        }
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("FeRx GUI")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 600.0])
            .with_icon(build_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "FeRx GUI",
        options,
        Box::new(|cc| Ok(Box::new(app::FerxApp::new(cc)))),
    )
}

// ── Icon renderer ─────────────────────────────────────────────────────────────
//
// Draws a pharmacokinetic concentration–time curve with observation points on a
// rounded dark-navy tile, with the small "FeRxGUI" wordmark beneath it.  Every
// shape is rendered with SDF-based antialiasing so it stays smooth at any dock /
// taskbar size.  No external image or font crate needed.

const ICON_SZ: usize = 512;

type Rgb = [u8; 3];
const ICON_NAVY:   Rgb = [0x1d, 0x26, 0x3a];
const ICON_ORANGE: Rgb = [0xbf, 0x5e, 0x2a];
const ICON_LIGHT:  Rgb = [0xe8, 0xec, 0xf6];
const ICON_AXIS:   Rgb = [0x44, 0x52, 0x6e];

fn build_icon() -> std::sync::Arc<eframe::egui::viewport::IconData> {
    let mut buf = vec![0u8; ICON_SZ * ICON_SZ * 4];

    // Rounded-rect navy background (transparent corners).
    let r_corner = 96.0_f32;
    for y in 0..ICON_SZ {
        for x in 0..ICON_SZ {
            let a = rounded_rect_alpha(x as f32 + 0.5, y as f32 + 0.5, ICON_SZ as f32, r_corner);
            if a > 0.0 {
                let i = (y * ICON_SZ + x) * 4;
                buf[i]   = ICON_NAVY[0];
                buf[i+1] = ICON_NAVY[1];
                buf[i+2] = ICON_NAVY[2];
                buf[i+3] = (a * 255.0) as u8;
            }
        }
    }

    // ── Axes ──────────────────────────────────────────────────────────────
    let left = 104.0; let right = 430.0;
    let top  = 120.0; let bottom = 348.0;
    capsule(&mut buf, left, top, left, bottom, 2.5, ICON_AXIS);     // y-axis
    capsule(&mut buf, left, bottom, right, bottom, 2.5, ICON_AXIS); // x-axis

    // ── PK (Bateman) curve: C(t) = e^-ke·t − e^-ka·t ────────────────────────
    let ka = 1.05_f32; let ke = 0.30_f32; let t_max = 14.0_f32;
    let n = 90;
    let mut cmax = 0.0_f32;
    for i in 0..=n {
        let t = t_max * i as f32 / n as f32;
        let c = (-ke * t).exp() - (-ka * t).exp();
        if c > cmax { cmax = c; }
    }
    let plot_w = right - left - 16.0;
    let plot_h = bottom - top - 8.0;
    let map = |t: f32, c: f32| -> (f32, f32) {
        (left + 12.0 + plot_w * (t / t_max), bottom - plot_h * (c / cmax))
    };
    let mut pts = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let t = t_max * i as f32 / n as f32;
        let c = (-ke * t).exp() - (-ka * t).exp();
        pts.push(map(t, c));
    }
    stroke_polyline(&mut buf, &pts, 6.0, ICON_ORANGE);

    // ── Observation dots on the curve ───────────────────────────────────────
    for &t in &[1.2_f32, 3.0, 5.5, 8.5, 11.5] {
        let c = (-ke * t).exp() - (-ka * t).exp();
        let (x, y) = map(t, c);
        disc(&mut buf, x, y, 9.0, ICON_NAVY);   // halo so dots read on the line
        disc(&mut buf, x, y, 6.5, ICON_LIGHT);
        disc(&mut buf, x, y, 3.5, ICON_ORANGE);
    }

    // ── Wordmark "FeRxGUI" ──────────────────────────────────────────────────
    draw_wordmark(&mut buf);

    std::sync::Arc::new(eframe::egui::viewport::IconData {
        rgba: buf,
        width:  ICON_SZ as u32,
        height: ICON_SZ as u32,
    })
}

// ── Wordmark ────────────────────────────────────────────────────────────────

fn draw_wordmark(buf: &mut [u8]) {
    let glyphs = ['F', 'e', 'R', 'x', 'G', 'U', 'I'];
    let ch = 60.0_f32;          // glyph cell height
    let cw = 37.0_f32;          // glyph cell width
    let gap = 8.0_f32;
    let total = glyphs.len() as f32 * cw + (glyphs.len() as f32 - 1.0) * gap;
    let mut gx = (ICON_SZ as f32 - total) / 2.0;
    let top_y = 397.0_f32;
    let r = 4.8_f32;            // thicker strokes for legibility at small sizes
    for g in glyphs {
        for stroke in glyph_strokes(g) {
            let mapped: Vec<(f32, f32)> = stroke.iter()
                .map(|(ux, uy)| (gx + ux * cw, top_y + uy * ch))
                .collect();
            stroke_polyline(buf, &mapped, r, ICON_LIGHT);
        }
        gx += cw + gap;
    }
}

/// Stroke polylines for each needed glyph in a unit cell (x,y ∈ 0..1, y down).
fn glyph_strokes(g: char) -> Vec<Vec<(f32, f32)>> {
    match g {
        // ── Uppercase glyphs ──────────────────────────────────────────────
        'F' => vec![
            vec![(0.24,0.06),(0.24,0.95)],           // stem
            vec![(0.24,0.06),(0.78,0.06)],            // top bar
            vec![(0.24,0.47),(0.68,0.47)],            // mid bar
        ],
        'R' => vec![
            vec![(0.22,0.06),(0.22,0.95)],            // stem
            vec![(0.22,0.06),(0.62,0.06),(0.80,0.19), // bowl (top half)
                 (0.80,0.38),(0.62,0.52),(0.22,0.52)],
            vec![(0.52,0.52),(0.80,0.95)],            // diagonal leg
        ],
        // ── Lowercase glyphs ──────────────────────────────────────────────
        'f' => vec![
            vec![(0.42,0.10),(0.42,0.95)],
            vec![(0.42,0.13),(0.52,0.04),(0.66,0.07)],
            vec![(0.18,0.40),(0.66,0.40)],
        ],
        'e' => vec![
            vec![(0.20,0.56),(0.74,0.56)],
            vec![(0.74,0.56),(0.73,0.40),(0.58,0.28),(0.38,0.28),
                 (0.22,0.41),(0.18,0.62),(0.30,0.86),(0.54,0.93),(0.74,0.83)],
        ],
        'r' => vec![
            vec![(0.30,0.30),(0.30,0.95)],
            vec![(0.30,0.46),(0.46,0.30),(0.68,0.33)],
        ],
        'x' => vec![
            vec![(0.20,0.30),(0.72,0.95)],
            vec![(0.72,0.30),(0.20,0.95)],
        ],
        'G' => vec![
            vec![(0.82,0.26),(0.60,0.07),(0.36,0.08),(0.18,0.27),
                 (0.15,0.58),(0.28,0.88),(0.58,0.95),(0.82,0.82),
                 (0.82,0.58),(0.60,0.58)],
        ],
        'U' => vec![
            vec![(0.20,0.06),(0.20,0.66),(0.34,0.90),(0.58,0.95),
                 (0.80,0.74),(0.80,0.06)],
        ],
        'I' => vec![
            vec![(0.50,0.06),(0.50,0.95)],
            vec![(0.34,0.06),(0.66,0.06)],
            vec![(0.34,0.95),(0.66,0.95)],
        ],
        _ => vec![],
    }
}

// ── SDF primitives ────────────────────────────────────────────────────────────

fn stroke_polyline(buf: &mut [u8], pts: &[(f32, f32)], r: f32, color: Rgb) {
    for w in pts.windows(2) {
        capsule(buf, w[0].0, w[0].1, w[1].0, w[1].1, r, color);
    }
}

/// Anti-aliased rounded-square coverage in `0.0..=1.0` for pixel centre (px,py).
fn rounded_rect_alpha(px: f32, py: f32, sz: f32, r: f32) -> f32 {
    let half = sz / 2.0;
    let cx = px - half;
    let cy = py - half;
    let inner = half - r;
    let dx = cx.abs() - inner;
    let dy = cy.abs() - inner;
    let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
    let inside = dx.max(dy).min(0.0);
    let dist = outside + inside - r;     // <0 inside
    smoothstep(1.0, -1.0, dist)
}

/// Anti-aliased filled circle.
fn disc(buf: &mut [u8], cx: f32, cy: f32, r: f32, color: Rgb) {
    let aa = 1.5;
    let x0 = (cx - r - aa).max(0.0) as usize;
    let x1 = ((cx + r + aa).ceil() as usize + 1).min(ICON_SZ);
    let y0 = (cy - r - aa).max(0.0) as usize;
    let y1 = ((cy + r + aa).ceil() as usize + 1).min(ICON_SZ);
    for py in y0..y1 {
        for px in x0..x1 {
            let fx = px as f32 + 0.5;
            let fy = py as f32 + 0.5;
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let a = smoothstep(aa, -aa, d - r);
            if a > 0.0 { icon_blend(buf, px, py, color, a); }
        }
    }
}

/// Anti-aliased filled capsule (line segment with rounded ends) from A to B.
fn capsule(buf: &mut [u8], ax: f32, ay: f32, bx: f32, by: f32, r: f32, color: Rgb) {
    let aa = 1.5;
    let pad = r + aa;
    let x0 = (ax.min(bx) - pad).max(0.0) as usize;
    let x1 = ((ax.max(bx) + pad).ceil() as usize + 1).min(ICON_SZ);
    let y0 = (ay.min(by) - pad).max(0.0) as usize;
    let y1 = ((ay.max(by) + pad).ceil() as usize + 1).min(ICON_SZ);
    let dx = bx - ax; let dy = by - ay;
    let len2 = dx * dx + dy * dy;
    for py in y0..y1 {
        for px in x0..x1 {
            let fx = px as f32 + 0.5;
            let fy = py as f32 + 0.5;
            let t = if len2 < 1e-6 { 0.0 }
                    else { (((fx - ax) * dx + (fy - ay) * dy) / len2).clamp(0.0, 1.0) };
            let cx = ax + t * dx; let cy = ay + t * dy;
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let a = smoothstep(aa, -aa, d - r);
            if a > 0.0 { icon_blend(buf, px, py, color, a); }
        }
    }
}

/// Alpha-blend `color` over the pixel at (px,py), accumulating coverage.
fn icon_blend(buf: &mut [u8], px: usize, py: usize, color: Rgb, a: f32) {
    let i = (py * ICON_SZ + px) * 4;
    for k in 0..3 {
        buf[i + k] = (buf[i + k] as f32 + (color[k] as f32 - buf[i + k] as f32) * a) as u8;
    }
    let cur = buf[i + 3] as f32;
    buf[i + 3] = (cur + (255.0 - cur) * a) as u8;
}

#[inline]
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
