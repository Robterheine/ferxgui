//! Small statistics kernel: tail probabilities, multiplicity adjustment and intervals.
//!
//! Hand-written (no `statrs`) and checked against R over a grid in the tests; if a golden
//! test ever fails, switch to `statrs` rather than loosening the tolerance.

use std::f64::consts::PI;

/// ln Γ(x) for x > 0 (Lanczos, g = 7, n = 9; relative error ~1e-15).
fn ln_gamma(x: f64) -> f64 {
    const C: [f64; 9] = [
        0.999_999_999_999_809_9, 676.520_368_121_885_1, -1_259.139_216_722_402_8,
        771.323_428_777_653_1, -176.615_029_162_140_6, 12.507_343_278_686_905,
        -0.138_571_095_265_720_12, 9.984_369_578_019_572e-6, 1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        return (PI / (PI * x).sin()).ln() - ln_gamma(1.0 - x);
    }
    let x = x - 1.0;
    let mut a = C[0];
    let t = x + 7.5;
    for (i, c) in C.iter().enumerate().skip(1) { a += c / (x + i as f64); }
    0.5 * (2.0 * PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
}

/// Regularised upper incomplete gamma Q(a, x).
fn gamma_q(a: f64, x: f64) -> f64 {
    if x <= 0.0 { return 1.0; }
    if x.is_infinite() { return 0.0; }
    let ln_pre = a * x.ln() - x - ln_gamma(a);
    if x < a + 1.0 {
        // Series for P(a, x); Q = 1 - P.
        let (mut sum, mut term, mut n) = (1.0 / a, 1.0 / a, a);
        for _ in 0..10_000 {
            n += 1.0;
            term *= x / n;
            sum += term;
            if term.abs() < sum.abs() * 1e-17 { break; }
        }
        (1.0 - sum * ln_pre.exp()).clamp(0.0, 1.0)
    } else {
        // Continued fraction for Q (modified Lentz).
        let tiny = 1e-300;
        let mut b = x + 1.0 - a;
        let mut c = 1.0 / tiny;
        let mut d = 1.0 / b;
        let mut h = d;
        for i in 1..10_000 {
            let an = -(i as f64) * (i as f64 - a);
            b += 2.0;
            d = an * d + b; if d.abs() < tiny { d = tiny; }
            c = b + an / c; if c.abs() < tiny { c = tiny; }
            d = 1.0 / d;
            let del = d * c;
            h *= del;
            if (del - 1.0).abs() < 1e-16 { break; }
        }
        (ln_pre.exp() * h).clamp(0.0, 1.0)
    }
}

/// P(X > x) for X ~ χ²(df).
pub fn chi2_sf(x: f64, df: f64) -> f64 {
    if x.is_nan() || df.is_nan() || df <= 0.0 { return f64::NAN; }
    gamma_q(df / 2.0, x / 2.0)
}

/// Regularised incomplete beta I_x(a, b).
fn beta_inc(a: f64, b: f64, x: f64) -> f64 {
    if x <= 0.0 { return 0.0; }
    if x >= 1.0 { return 1.0; }
    let ln_front = ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln();
    let cf = |a: f64, b: f64, x: f64| -> f64 {
        let tiny = 1e-300;
        let (qab, qap, qam) = (a + b, a + 1.0, a - 1.0);
        let mut c = 1.0;
        let mut d = 1.0 - qab * x / qap; if d.abs() < tiny { d = tiny; }
        d = 1.0 / d;
        let mut h = d;
        for m in 1..10_000 {
            let m = m as f64;
            let m2 = 2.0 * m;
            let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
            d = 1.0 + aa * d; if d.abs() < tiny { d = tiny; }
            c = 1.0 + aa / c; if c.abs() < tiny { c = tiny; }
            d = 1.0 / d; h *= d * c;
            let aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
            d = 1.0 + aa * d; if d.abs() < tiny { d = tiny; }
            c = 1.0 + aa / c; if c.abs() < tiny { c = tiny; }
            d = 1.0 / d;
            let del = d * c;
            h *= del;
            if (del - 1.0).abs() < 1e-16 { break; }
        }
        h
    };
    if x < (a + 1.0) / (a + b + 2.0) {
        (ln_front.exp() * cf(a, b, x) / a).clamp(0.0, 1.0)
    } else {
        (1.0 - ln_front.exp() * cf(b, a, 1.0 - x) / b).clamp(0.0, 1.0)
    }
}

/// Two-sided p-value of a t statistic with `df` degrees of freedom.
pub fn student_t_two_sided_p(t: f64, df: f64) -> f64 {
    if t.is_nan() || df.is_nan() || df <= 0.0 { return f64::NAN; }
    if t.is_infinite() { return 0.0; }
    // p = I_{df/(df+t²)}(df/2, 1/2) = 1 − I_{t²/(df+t²)}(1/2, df/2). Take whichever argument is
    // small, so neither form suffers cancellation near p ≈ 1.
    let y = t * t / (df + t * t);
    if y < 0.5 { 1.0 - beta_inc(0.5, df / 2.0, y) } else { beta_inc(df / 2.0, 0.5, 1.0 - y) }
}

/// Benjamini–Hochberg adjusted p-values (R `p.adjust(p, "BH")`). NaN entries stay NaN and do
/// not count towards the number of tests.
pub fn benjamini_hochberg(p: &[f64]) -> Vec<f64> {
    let mut idx: Vec<usize> = (0..p.len()).filter(|&i| !p[i].is_nan()).collect();
    let n = idx.len();
    let mut out = vec![f64::NAN; p.len()];
    idx.sort_by(|&a, &b| p[b].total_cmp(&p[a])); // descending
    let mut running = f64::INFINITY;
    for (k, &i) in idx.iter().enumerate() {
        let rank = n - k; // descending order: first element has rank n
        running = running.min(p[i] * n as f64 / rank as f64);
        out[i] = running.min(1.0);
    }
    out
}

/// Scale on which a Wald interval is built, chosen from how the parameter is bounded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CiScale {
    /// Symmetric: estimate ± z·SE. For parameters that may be negative.
    Natural,
    /// Delta method on ln(estimate). For parameters that cannot be negative (θ with a lower bound
    /// of 0 or more, ω², σ).
    Log,
    /// Delta method on logit((estimate − lo) / (hi − lo)). For parameters on a unit interval
    /// (declared bounds within 0…1, e.g. a bioavailability fraction), so the interval stays inside.
    Logit { lo: f64, hi: f64 },
}

impl From<bool> for CiScale {
    /// `true` = log scale. Kept so callers that only distinguish positive from unrestricted stay simple.
    fn from(log_scale: bool) -> Self { if log_scale { CiScale::Log } else { CiScale::Natural } }
}

/// 95 % Wald interval on the given scale, back-transformed. Falls back to the natural scale when the
/// estimate is outside what the scale allows (not positive; not strictly inside the bounds). None
/// when either input is not finite. Approximate: it assumes the likelihood is near-normal on that
/// scale and is unreliable for estimates at a bound or with few subjects.
pub fn wald_ci95(est: f64, se: f64, scale: impl Into<CiScale>) -> Option<(f64, f64)> {
    if !est.is_finite() || !se.is_finite() { return None; }
    let z = qnorm(0.975);
    match scale.into() {
        CiScale::Log if est > 0.0 => {
            let h = z * se / est;
            Some((est * (-h).exp(), est * h.exp()))
        }
        CiScale::Logit { lo, hi } if lo.is_finite() && hi.is_finite() && hi > lo && est > lo && est < hi => {
            let w = hi - lo;
            let p = (est - lo) / w;
            let t = (p / (1.0 - p)).ln();
            let h = z * se / ((est - lo) * (hi - est) / w);
            let back = |x: f64| lo + w / (1.0 + (-x).exp());
            Some((back(t - h), back(t + h)))
        }
        _ => Some((est - z * se, est + z * se)),
    }
}

/// Coefficient of variation (%) of a log-normal random effect with variance `omega2`:
/// 100·√(exp(ω²) − 1). Defined only for log-normal ETAs.
pub fn cv_pct_lognormal(omega2: f64) -> f64 { 100.0 * (omega2.exp() - 1.0).sqrt() }

/// Two-sided p-value of Pearson r from n pairs (the `cor.test` t statistic, df = n − 2).
pub fn pearson_p(r: f64, n: usize) -> f64 {
    if n < 3 || !r.is_finite() || r.abs() > 1.0 { return f64::NAN; }
    if r.abs() == 1.0 { return 0.0; }
    let df = (n - 2) as f64;
    student_t_two_sided_p(r * (df / (1.0 - r * r)).sqrt(), df)
}

/// Recovers the number of pairs behind a reported (r, p) from `cor.test`, since the bridge does
/// not return n. Exact when p was computed the standard way; None when no n reproduces p.
pub fn n_from_r_p(r: f64, p: f64) -> Option<usize> {
    if !r.is_finite() || !p.is_finite() || r == 0.0 || p <= 0.0 || p >= 1.0 { return None; }
    // |r| fixed: p decreases as n grows. Bisect on n.
    let (mut lo, mut hi) = (3usize, 200_000usize);
    if pearson_p(r, hi) > p || pearson_p(r, lo) < p { return None; }
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if pearson_p(r, mid) > p { lo = mid } else { hi = mid }
    }
    let best = [lo, hi].into_iter()
        .min_by(|&a, &b| (pearson_p(r, a) - p).abs().total_cmp(&(pearson_p(r, b) - p).abs()))?;
    ((pearson_p(r, best) - p).abs() <= 1e-6 * p.max(1e-12)).then_some(best)
}

/// Standard normal quantile (Wichura, AS 241 PPND16; ~1e-16 relative accuracy).
// The coefficients are the published AS 241 constants verbatim; truncating them to satisfy
// `excessive_precision` would mean re-typing a verified algorithm.
#[allow(clippy::excessive_precision)]
pub fn qnorm(p: f64) -> f64 {
    if !(0.0..=1.0).contains(&p) || p.is_nan() { return f64::NAN; }
    if p == 0.0 { return f64::NEG_INFINITY; }
    if p == 1.0 { return f64::INFINITY; }
    let q = p - 0.5;
    if q.abs() <= 0.425 {
        let r = 0.180625 - q * q;
        return q * poly(&[3.387132872796366608, 133.14166789178437745, 1971.5909503065514427,
            13731.693765509461125, 45921.953931549871457, 67265.770927008700853,
            33430.575583588128105, 2509.0809287301226727], r)
            / poly(&[1.0, 42.313330701600911252, 687.1870074920579083, 5394.1960214247511077,
            21213.794301586595867, 39307.89580009271061, 28729.085735721942674,
            5226.495278852545925], r);
    }
    let r = if q < 0.0 { p } else { 1.0 - p };
    let r = (-r.ln()).sqrt();
    let val = if r <= 5.0 {
        let r = r - 1.6;
        poly(&[1.42343711074968357734, 4.6303378461565452959, 5.7694972214606914055,
            3.64784832476320460504, 1.27045825245236838258, 0.24178072517745061177,
            0.0227238449892691845833, 7.7454501427834140764e-4], r)
            / poly(&[1.0, 2.05319162663775882187, 1.6763848301838038494, 0.68976733498510000455,
            0.14810397642748007459, 0.0151986665636164571966, 5.475938084995344946e-4,
            1.05075007164441684324e-9], r)
    } else {
        let r = r - 5.0;
        poly(&[6.6579046435011037772, 5.4637849111641143699, 1.7848265399172913358,
            0.29656057182850489123, 0.026532189526576123093, 0.0012426609473880784386,
            2.71155556874348757815e-5, 2.01033439929228813265e-7], r)
            / poly(&[1.0, 0.59983220655588793769, 0.13692988092273580531,
            0.0148753612908506148525, 7.868691311456132591e-4, 1.8463183175100546818e-5,
            1.4215117583164458887e-7, 2.04426310338993978564e-15], r)
    };
    if q < 0.0 { -val } else { val }
}

fn poly(c: &[f64], x: f64) -> f64 { c.iter().rev().fold(0.0, |acc, &k| acc * x + k) }

/// Fisher-z confidence interval for a correlation `r` from `n` pairs. None when n ≤ 3 or |r| ≥ 1.
pub fn fisher_z_ci(r: f64, n: usize, level: f64) -> Option<(f64, f64)> {
    if n <= 3 || !r.is_finite() || r.abs() >= 1.0 { return None; }
    let z = r.atanh();
    let h = qnorm(0.5 + level / 2.0) / ((n - 3) as f64).sqrt();
    Some(((z - h).tanh(), (z + h).tanh()))
}

/// Two-sided p-value of a one-sample t test of mean = 0, with mean and SD of `x` (NaN dropped).
/// Returns (mean, sd, t, df, p); None with fewer than 2 values or zero spread.
pub fn one_sample_t(x: &[f64]) -> Option<(f64, f64, f64, f64, f64)> {
    let v: Vec<f64> = x.iter().copied().filter(|a| a.is_finite()).collect();
    let n = v.len();
    if n < 2 { return None; }
    let mean = v.iter().sum::<f64>() / n as f64;
    let sd = (v.iter().map(|a| (a - mean).powi(2)).sum::<f64>() / (n - 1) as f64).sqrt();
    if sd == 0.0 { return None; }
    let t = mean / (sd / (n as f64).sqrt());
    let df = (n - 1) as f64;
    Some((mean, sd, t, df, student_t_two_sided_p(t, df)))
}

/// Wilson score interval for a proportion `k / n` at the given confidence level.
pub fn wilson_ci(k: usize, n: usize, level: f64) -> Option<(f64, f64)> {
    if n == 0 { return None; }
    let z = qnorm(0.5 + level / 2.0);
    let (nf, p) = (n as f64, k as f64 / n as f64);
    let denom = 1.0 + z * z / nf;
    let centre = (p + z * z / (2.0 * nf)) / denom;
    let half = z * (p * (1.0 - p) / nf + z * z / (4.0 * nf * nf)).sqrt() / denom;
    Some(((centre - half).max(0.0), (centre + half).min(1.0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)] struct Chi { df: f64, x: f64, p: f64 }
    #[derive(Deserialize)] struct T { df: f64, t: f64, p: f64 }
    #[derive(Deserialize)] struct Bh { p: Vec<Option<f64>>, adj: Vec<Option<f64>> }
    #[derive(Deserialize)] struct Fz { r: f64, n: usize, lo: f64, hi: f64 }
    #[derive(Deserialize)] struct Qn { p: f64, q: f64 }
    #[derive(Deserialize)] struct Golden { chi2: Vec<Chi>, t: Vec<T>, bh: Vec<Bh>, fisher: Vec<Fz>, qnorm: Vec<Qn> }

    fn golden() -> Golden { serde_json::from_str(include_str!("../../tests/golden/stats.json")).unwrap() }

    /// Agreement within 1e-10 relative, or 1e-14 absolute for values near zero.
    fn near(a: f64, b: f64) -> bool { (a - b).abs() <= 1e-14 + 1e-10 * b.abs() }

    #[test]
    fn chi2_sf_matches_pchisq_grid() {
        for c in golden().chi2 {
            let got = chi2_sf(c.x, c.df);
            assert!(near(got, c.p), "df={} x={} got={got:e} want={:e}", c.df, c.x, c.p);
        }
        // The cut-offs quoted in the plan.
        assert!((chi2_sf(3.841, 1.0) - 0.0500137).abs() < 1e-6);
        assert!((chi2_sf(7.815, 3.0) - 0.0499939).abs() < 1e-6);
    }

    #[test]
    fn t_two_sided_matches_pt() {
        for c in golden().t {
            let got = student_t_two_sided_p(c.t, c.df);
            assert!(near(got, c.p), "df={} t={} got={got:e} want={:e}", c.df, c.t, c.p);
        }
    }

    #[test]
    fn bh_matches_p_adjust() {
        for case in golden().bh {
            let p: Vec<f64> = case.p.iter().map(|v| v.unwrap_or(f64::NAN)).collect();
            let got = benjamini_hochberg(&p);
            for (g, w) in got.iter().zip(&case.adj) {
                match w {
                    Some(w) => assert!(near(*g, *w), "{g} vs {w}"),
                    None => assert!(g.is_nan()),
                }
            }
        }
    }

    #[test]
    fn fisher_and_qnorm_match_r() {
        for q in golden().qnorm {
            assert!(near(qnorm(q.p), q.q), "p={} got={} want={}", q.p, qnorm(q.p), q.q);
        }
        for z in golden().fisher {
            let (lo, hi) = fisher_z_ci(z.r, z.n, 0.95).unwrap();
            assert!(near(lo, z.lo) && near(hi, z.hi), "r={} n={}", z.r, z.n);
        }
    }

    #[test]
    fn wilson_known_value() {
        // 5/20 at 95 %: textbook Wilson interval [0.1119, 0.4687].
        let (lo, hi) = wilson_ci(5, 20, 0.95).unwrap();
        assert!((lo - 0.1119).abs() < 1e-3 && (hi - 0.4687).abs() < 1e-3, "{lo} {hi}");
    }

    #[test]
    fn ci_log_scale_for_positive_params() {
        // Warfarin FOCEI, plan §5.6: TVCL est 0.13270, SE 0.0070...; log-scale target.
        let (lo, hi) = wald_ci95(0.13270, 0.007_08, true).unwrap();
        assert!(lo > 0.0 && (lo - 0.1195).abs() < 5e-4 && (hi - 0.1473).abs() < 5e-4, "{lo} {hi}");
        // A variance with a large relative SE: natural scale goes negative, log scale cannot.
        let (nlo, _) = wald_ci95(0.0841, 0.1, false).unwrap();
        let (llo, _) = wald_ci95(0.0841, 0.1, true).unwrap();
        assert!(nlo < 0.0 && llo > 0.0);
        assert_eq!(wald_ci95(f64::NAN, 1.0, true), None);
    }

    #[test]
    fn cv_lognormal_golden() {
        for (w2, cv) in [(0.028589, 17.03), (0.009592, 9.82), (0.335870, 63.18)] {
            assert!((cv_pct_lognormal(w2) - cv).abs() < 0.005, "{w2}");
        }
    }

    #[test]
    fn n_is_recovered_from_r_and_p() {
        for (r, n) in [(0.31, 37usize), (-0.52, 12), (0.08, 140), (0.9, 6)] {
            assert_eq!(n_from_r_p(r, pearson_p(r, n)), Some(n), "r={r} n={n}");
        }
        assert_eq!(n_from_r_p(0.3, 0.5), None);
    }

    #[test]
    fn logit_interval_stays_inside_the_unit_interval() {
        // A fraction near 1 with a large SE: the symmetric interval exceeds 1, the logit one cannot.
        let (nat_lo, nat_hi) = wald_ci95(0.9, 0.1, CiScale::Natural).unwrap();
        assert!(nat_hi > 1.0 && nat_lo < 0.9);
        let (lo, hi) = wald_ci95(0.9, 0.1, CiScale::Logit { lo: 0.0, hi: 1.0 }).unwrap();
        assert!(lo > 0.0 && hi < 1.0 && lo < 0.9 && hi > 0.9, "{lo} {hi}");
        // Symmetric on the logit scale around the estimate's logit.
        let t = |x: f64| (x / (1.0 - x)).ln();
        assert!(((t(hi) - t(0.9)) - (t(0.9) - t(lo))).abs() < 1e-9);
        // An estimate on the bound falls back to natural rather than producing NaN.
        assert!(wald_ci95(1.0, 0.1, CiScale::Logit { lo: 0.0, hi: 1.0 }).unwrap().0.is_finite());
    }
}

