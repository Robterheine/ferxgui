//! Small shared helpers.

use std::cmp::Ordering;
use std::sync::mpsc::Sender;

use crate::workers::messages::WorkerMsg;

/// Total order on `f64` with NaN sorted last (after +inf), so a NaN in a sort
/// key can neither panic nor scramble the rest of the order.
pub fn cmp_nan_last(a: f64, b: f64) -> Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true)   => Ordering::Equal,
        (true, false)  => Ordering::Greater,
        (false, true)  => Ordering::Less,
        (false, false) => a.total_cmp(&b),
    }
}

/// Longest prefix of `s` holding at most `n` bytes that ends on a char
/// boundary. Safe on multi-byte text (slicing `&s[..n]` panics mid-char).
pub fn truncate_chars(s: &str, n: usize) -> &str {
    if s.len() <= n { return s; }
    let mut end = n;
    while !s.is_char_boundary(end) { end -= 1; }
    &s[..end]
}

/// Run `job` on a new thread; a panic is turned into `WorkerMsg::RTaskError`
/// instead of silently killing the thread and leaving the UI waiting.
pub fn spawn_guarded<F>(name: &str, tx: Sender<WorkerMsg>, job: F)
where
    F: FnOnce() + Send + 'static,
{
    let context = name.to_string();
    std::thread::spawn(move || {
        if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)) {
            let message = payload.downcast_ref::<&str>().map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "worker panicked".to_string());
            let _ = tx.send(WorkerMsg::RTaskError { context, message: format!("internal error: {message}") });
        }
    });
}

/// Test support: fixtures and live-R checks.
#[cfg(test)]
pub mod testsupport {
    use std::path::PathBuf;

    pub fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
    }

    /// True when a test that needs `what` should run. When it is unavailable the test is skipped,
    /// unless `FERX_REQUIRE_FIXTURES=1` (set in CI), in which case that is a failure.
    pub fn available(cond: bool, what: &str) -> bool {
        if !cond && std::env::var("FERX_REQUIRE_FIXTURES").as_deref() == Ok("1") {
            panic!("FERX_REQUIRE_FIXTURES=1 but {what} is not available");
        }
        cond
    }

    /// Rscript on PATH with the ferx and vpc packages installed.
    pub fn r_with_ferx() -> bool {
        use std::sync::OnceLock;
        static OK: OnceLock<bool> = OnceLock::new();
        *OK.get_or_init(|| {
            std::process::Command::new("Rscript")
                .args(["-e", "quit(status = !(requireNamespace('ferx', quietly=TRUE) && requireNamespace('vpc', quietly=TRUE)))"])
                .output().map(|o| o.status.success()).unwrap_or(false)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nan_sorts_never_panic() {
        // Deterministic xorshift so the test needs no rand crate.
        let mut s = 0x9E3779B97F4A7C15u64;
        let mut next = move || { s ^= s << 13; s ^= s >> 7; s ^= s << 17; s };
        for _ in 0..2000 {
            let len = (next() % 201) as usize;
            let mut v: Vec<f64> = (0..len).map(|_| {
                if next() % 5 == 0 { f64::NAN } else { (next() % 2000) as f64 / 10.0 - 100.0 }
            }).collect();
            v.sort_by(|a, b| cmp_nan_last(*a, *b));
            let first_nan = v.iter().position(|x| x.is_nan()).unwrap_or(v.len());
            assert!(v[first_nan..].iter().all(|x| x.is_nan()), "NaN not last");
            assert!(v[..first_nan].windows(2).all(|w| w[0] <= w[1]), "not sorted");
        }
    }

    #[test]
    fn truncate_is_char_safe() {
        let s = format!("{}η{}", "a".repeat(499), "b".repeat(10)); // η spans bytes 499..501
        let t = truncate_chars(&s, 500);
        assert_eq!(t.len(), 499);
        assert_eq!(truncate_chars("short", 500), "short");
    }

    #[test]
    fn spawn_guarded_reports_panic() {
        let (tx, rx) = std::sync::mpsc::channel();
        spawn_guarded("boom", tx, || panic!("kaboom"));
        match rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap() {
            WorkerMsg::RTaskError { context, message } => {
                assert_eq!(context, "boom");
                assert!(message.contains("kaboom"));
            }
            _ => panic!("wrong message"),
        }
    }
}
