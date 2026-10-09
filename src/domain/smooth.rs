//! Local regression smoother matching R's `loess()` / ggplot2's default `geom_smooth`:
//! local quadratic, tricube weights over the nearest `ceil(span * n)` points, fitted directly
//! at each evaluation point (`surface = "direct"`). On-screen curves therefore agree with the
//! exported figures.

/// Solves the 3×3 system `a x = b` by Gaussian elimination with partial pivoting.
fn solve3(mut a: [[f64; 3]; 3], mut b: [f64; 3]) -> Option<[f64; 3]> {
    for c in 0..3 {
        let p = (c..3).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        if a[p][c].abs() < 1e-300 { return None; }
        a.swap(c, p); b.swap(c, p);
        for r in c + 1..3 {
            let f = a[r][c] / a[c][c];
            for k in c..3 { a[r][k] -= f * a[c][k]; }
            b[r] -= f * b[c];
        }
    }
    let mut x = [0.0; 3];
    for r in (0..3).rev() {
        let s: f64 = (r + 1..3).map(|k| a[r][k] * x[k]).sum();
        x[r] = (b[r] - s) / a[r][r];
    }
    Some(x)
}

/// Fitted value at `x0` from the finite `(x, y)` pairs; None when it is not estimable.
pub fn loess_at(xs: &[f64], ys: &[f64], span: f64, x0: f64) -> Option<f64> {
    let n = xs.len();
    // R: q = floor(n * span + 1e-5) points in the neighbourhood.
    let q = ((n as f64 * span + 1e-5).floor() as usize).clamp(3, n);
    let mut d: Vec<f64> = xs.iter().map(|x| (x - x0).abs()).collect();
    let mut sorted = d.clone();
    sorted.sort_by(f64::total_cmp);
    let mut h = sorted[q - 1];
    if span > 1.0 { h *= span; } // R widens the window for span > 1 (one predictor: span^(1/p))
    if h <= 0.0 { return None; }
    let (mut a, mut b) = ([[0.0f64; 3]; 3], [0.0f64; 3]);
    for i in 0..n {
        d[i] /= h;
        if d[i] >= 1.0 { continue; }
        let w = (1.0 - d[i].powi(3)).powi(3);
        let u = xs[i] - x0; // centred so the intercept is the fit at x0
        let basis = [1.0, u, u * u];
        for r in 0..3 {
            for c in 0..3 { a[r][c] += w * basis[r] * basis[c]; }
            b[r] += w * basis[r] * ys[i];
        }
    }
    solve3(a, b).map(|s| s[0]).filter(|v| v.is_finite())
}

/// Smooths `points` on a 61-point grid across their x range. Empty for fewer than 4 finite
/// points or no x spread.
pub fn loess(points: &[[f64; 2]], span: f64) -> Vec<[f64; 2]> {
    let (xs, ys): (Vec<f64>, Vec<f64>) = points.iter()
        .filter(|p| p[0].is_finite() && p[1].is_finite()).map(|p| (p[0], p[1])).unzip();
    if xs.len() < 4 { return vec![]; }
    let lo = xs.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if hi - lo < 1e-10 { return vec![]; }
    (0..=60).filter_map(|i| {
        let x0 = lo + (hi - lo) * i as f64 / 60.0;
        loess_at(&xs, &ys, span, x0).map(|y| [x0, y])
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Case { x: Vec<f64>, y: Vec<f64>, grid: Vec<f64>, fit: Vec<f64> }
    #[derive(Deserialize)]
    struct Golden { a: Case, b: Case }

    #[test]
    fn loess_matches_r_direct() {
        let g: Golden = serde_json::from_str(include_str!("../../tests/golden/loess.json")).unwrap();
        for (name, c) in [("a", g.a), ("b", g.b)] {
            let pts: Vec<[f64; 2]> = c.x.iter().zip(&c.y).map(|(x, y)| [*x, *y]).collect();
            let got = loess(&pts, 0.75);
            assert_eq!(got.len(), c.grid.len(), "{name}");
            for (k, p) in got.iter().enumerate() {
                assert!((p[0] - c.grid[k]).abs() < 1e-9, "{name} grid {k}");
                assert!((p[1] - c.fit[k]).abs() < 1e-6, "{name} point {k}: got {} want {}", p[1], c.fit[k]);
            }
        }
    }
}
