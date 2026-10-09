/// Detached run management.
///
/// # Architecture
///
/// 1. `spawn_detached_run` — sets up the log file, spawns ferx in a new
///    session (setsid on Unix), writes the run manifest, then starts a
///    combined monitor/tailer thread.
///
/// 2. `reconnect_orphan` — called on app startup for each manifest whose PID
///    is still alive.  Starts an equivalent monitor/tailer thread without a
///    Child handle, using PID polling instead.
///
/// Both variants send the same `WorkerMsg` variants back to the main thread.
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crate::domain::{JobStatus, RunRecord};
use super::messages::{CancelMode, WorkerMsg};
use super::procid::{may_signal, ProcessIdentity};
use super::run_manifest::RunManifest;

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Result returned to the UI thread after a successful launch.
pub struct SpawnedRun {
    pub log_path: PathBuf,
}

/// Spawn `ferx` as a detached subprocess, redirect its output to a log file,
/// write a run manifest, and start a background monitor/tailer thread.
///
/// Returns `Err` only if the process could not be started at all.
pub fn spawn_detached_run(
    record: RunRecord,
    ferx_binary: PathBuf,
    args: Vec<String>,
    cwd: PathBuf,
    log_path: PathBuf,
    manifest_path: PathBuf,
    status_path: Option<PathBuf>,
    tx: Sender<WorkerMsg>,
    cancel_rx: Receiver<CancelMode>,
) -> std::io::Result<SpawnedRun> {
    // Open (or create) the log file — truncate so each run starts fresh.
    let log_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)?;
    let log_stderr = log_file.try_clone()?;

    // Build the command.
    let mut cmd = std::process::Command::new(&ferx_binary);
    cmd.args(&args)
        .current_dir(&cwd)
        .stdin(std::process::Stdio::null())
        .stdout(log_file)
        .stderr(log_stderr);

    // Detach from the parent's session on Unix so the child survives
    // the GUI being closed.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                extern "C" { fn setsid() -> i32; }
                setsid();
                Ok(())
            });
        }
    }

    // On Windows, detach from the parent console, start a new process group,
    // and break out of any enclosing Job Object so the child is not killed
    // when the SSH/RDP session's job handle is closed.
    // CREATE_BREAKAWAY_FROM_JOB is silently ignored if the parent job
    // does not permit breakaway — it never causes an error.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS:         u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB);
    }

    let child = cmd.spawn().map_err(|e| {
        std::io::Error::new(
            e.kind(),
            format!("Failed to start {}: {}", ferx_binary.display(), e),
        )
    })?;

    let pid = child.id();

    // Write the manifest atomically.
    let manifest = RunManifest {
        version: RunManifest::VERSION,
        run_id: record.id.clone(),
        model_stem: record.model_stem.clone(),
        pid,
        log_path: log_path.clone(),
        command: record.command.clone(),
        directory: cwd,
        status_path,
        identity: ProcessIdentity::of(pid),
    };
    manifest.write(&manifest_path)?;

    // Spawn the combined monitor + tailer thread.
    let mp2  = manifest_path.clone();
    let lp2  = log_path.clone();
    let rec2 = record;
    let identity = manifest.identity.clone();
    std::thread::spawn(move || {
        fresh_run_worker(child, pid, identity, rec2, lp2, mp2, tx, cancel_rx);
    });

    Ok(SpawnedRun { log_path })
}

/// Reconnect to an orphaned run (PID still alive after GUI restart).
/// Starts a monitor/tailer thread identical to the fresh variant but using
/// PID polling instead of `Child::try_wait`.
pub fn reconnect_orphan(
    manifest: RunManifest,
    manifest_path: PathBuf,
    record: RunRecord,
    tx: Sender<WorkerMsg>,
    cancel_rx: Receiver<CancelMode>,
) {
    let log_path  = manifest.log_path.clone();
    let pid       = manifest.pid;
    let identity  = manifest.identity.clone();
    let status    = manifest.status_path.clone();
    std::thread::spawn(move || {
        orphan_worker(pid, identity, status, record, log_path, manifest_path, tx, cancel_rx);
    });
}

// ---------------------------------------------------------------------------
// Worker threads
// ---------------------------------------------------------------------------

/// Monitor + tailer for a freshly spawned Child.
fn fresh_run_worker(
    child: std::process::Child,
    pid: u32,
    identity: Option<ProcessIdentity>,
    record: RunRecord,
    log_path: PathBuf,
    manifest_path: PathBuf,
    tx: Sender<WorkerMsg>,
    cancel_rx: Receiver<CancelMode>,
) {
    let mp   = manifest_path.clone();
    let tx2  = tx.clone();
    let stem = record.model_stem.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        fresh_run_worker_inner(child, pid, identity, record, log_path, manifest_path, tx, cancel_rx);
    }));
    if result.is_err() {
        RunManifest::remove(&mp);
        let _ = tx2.send(WorkerMsg::RunError {
            stem,
            message: "Internal error: run worker thread panicked unexpectedly".to_string(),
        });
    }
}

fn fresh_run_worker_inner(
    mut child: std::process::Child,
    pid: u32,
    identity: Option<ProcessIdentity>,
    record: RunRecord,
    log_path: PathBuf,
    manifest_path: PathBuf,
    tx: Sender<WorkerMsg>,
    cancel_rx: Receiver<CancelMode>,
) {
    let mut log_reader = LogReader::new(log_path.clone(), record.model_stem.clone());
    let mut tail_tick  = 0u32;

    let exit_code = loop {
        // ── Cancel check ────────────────────────────────────────────────
        if let Ok(mode) = cancel_rx.try_recv() {
            apply_cancel(mode, &mut child, pid, identity.as_ref(), &tx, &record.model_stem);
            let _ = child.wait();
            finish(record, -1, &manifest_path, &tx);
            return;
        }

        // ── Tail log every ~300 ms (3 × 100 ms ticks) ──────────────────
        tail_tick = tail_tick.wrapping_add(1);
        if tail_tick % 3 == 0 {
            log_reader.drain(&tx);
        }

        // ── Check for exit ──────────────────────────────────────────────
        match child.try_wait() {
            Ok(Some(status)) => {
                // Drain any remaining output.
                log_reader.drain(&tx);
                break status.code().unwrap_or(-1);
            }
            Ok(None) => {}
            Err(e) => {
                let _ = tx.send(WorkerMsg::RunError {
                    stem: record.model_stem.clone(),
                    message: format!("wait error: {e}"),
                });
                RunManifest::remove(&manifest_path);
                return;
            }
        }

        std::thread::sleep(Duration::from_millis(100));
    };

    finish(record, exit_code, &manifest_path, &tx);
}

/// Monitor + tailer for a reconnected orphan (no Child handle).
fn orphan_worker(
    pid: u32,
    identity: Option<ProcessIdentity>,
    status_path: Option<PathBuf>,
    record: RunRecord,
    log_path: PathBuf,
    manifest_path: PathBuf,
    tx: Sender<WorkerMsg>,
    cancel_rx: Receiver<CancelMode>,
) {
    let mp   = manifest_path.clone();
    let tx2  = tx.clone();
    let stem = record.model_stem.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        orphan_worker_inner(pid, identity, status_path, record, log_path, manifest_path, tx, cancel_rx);
    }));
    if result.is_err() {
        RunManifest::remove(&mp);
        let _ = tx2.send(WorkerMsg::RunError {
            stem,
            message: "Internal error: orphan worker thread panicked unexpectedly".to_string(),
        });
    }
}

fn orphan_worker_inner(
    pid: u32,
    identity: Option<ProcessIdentity>,
    status_path: Option<PathBuf>,
    record: RunRecord,
    log_path: PathBuf,
    manifest_path: PathBuf,
    tx: Sender<WorkerMsg>,
    cancel_rx: Receiver<CancelMode>,
) {
    let mut log_reader = LogReader::new(log_path, record.model_stem.clone());
    let mut tick = 0u32;

    loop {
        // ── Cancel check ────────────────────────────────────────────────
        if let Ok(mode) = cancel_rx.try_recv() {
            if !may_signal(identity.as_ref(), pid) {
                // Never signal a PID we cannot prove is still our run (it may have been reused).
                let _ = tx.send(WorkerMsg::RunLine {
                    stem: record.model_stem.clone(),
                    line: format!("[ferxgui] Cancel refused: could not verify that process {pid} is still \
                                   this run (its start time is missing or differs). Nothing was signalled."),
                });
                continue;
            }
            kill_pid(mode, pid);
            // Give it a moment before draining final output.
            std::thread::sleep(Duration::from_millis(500));
            log_reader.drain(&tx);
            finish(record, -1, &manifest_path, &tx);
            return;
        }

        // ── Tail log every ~500 ms (5 × 100 ms ticks) ──────────────────
        tick = tick.wrapping_add(1);
        if tick % 5 == 0 {
            log_reader.drain(&tx);
        }

        // ── PID liveness check (every 5 ticks = 500 ms) ─────────────────
        // Throttled so Windows doesn't spawn `tasklist` at 10 Hz.
        if tick % 5 == 0 && !RunManifest::is_pid_alive(pid) {
            log_reader.drain(&tx);
            // We cannot read the exit code of a process we did not start; the run script wrote its
            // status to a file, and a completed run must also have left a valid bundle.
            let code = orphan_exit_code(status_path.as_deref(), &record);
            finish(record, code, &manifest_path, &tx);
            return;
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Apply a cancel request to a Child.
fn apply_cancel(
    mode: CancelMode,
    child: &mut std::process::Child,
    pid: u32,
    identity: Option<&ProcessIdentity>,
    tx: &Sender<WorkerMsg>,
    stem: &str,
) {
    // We hold the Child (not yet reaped), so its PID cannot have been reused; the identity check
    // still guards the PID-addressed signals against any bookkeeping error.
    if !may_signal(identity, pid) {
        let _ = tx.send(WorkerMsg::RunLine {
            stem: stem.to_string(),
            line: "[ferxgui] process identity could not be verified; using the process handle only".into(),
        });
        let _ = child.kill();
        return;
    }
    match mode {
        CancelMode::Graceful => {
            // Send the platform's "please stop cleanly" signal, then wait up to
            // 5 s before escalating to a hard kill.
            #[cfg(unix)]
            sigterm(pid);
            #[cfg(windows)]
            ctrl_break(pid); // CTRL_BREAK_EVENT to the child's process group
            #[cfg(not(any(unix, windows)))]
            { let _ = pid; }
            // Grace period.
            for _ in 0..50 {
                if child.try_wait().map(|s| s.is_some()).unwrap_or(true) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            // Escalate if still alive.
            let _ = child.kill();
        }
        CancelMode::Kill => {
            let _ = child.kill();
        }
    }
}

/// Signal a PID directly (for orphan cancellation).
fn kill_pid(mode: CancelMode, pid: u32) {
    match mode {
        CancelMode::Graceful => {
            #[cfg(unix)] sigterm(pid);
            #[cfg(not(unix))] kill_hard(pid);
            std::thread::sleep(Duration::from_secs(5));
            kill_hard(pid);
        }
        CancelMode::Kill => kill_hard(pid),
    }
}

#[cfg(unix)]
fn sigterm(pid: u32) {
    if !super::procid::pid_in_range(pid) { return; }
    extern "C" { fn kill(pid: i32, sig: i32) -> i32; }
    unsafe { kill(pid as i32, 15); } // SIGTERM = 15
}

/// On Windows, send CTRL_BREAK_EVENT to the child's process group so Rscript
/// (and any R scripts it's running) can handle the signal and exit cleanly.
/// The child must have been launched with `CREATE_NEW_PROCESS_GROUP` for this
/// to work — which our spawn_detached_run already sets.
#[cfg(windows)]
fn ctrl_break(pid: u32) {
    extern "system" {
        fn GenerateConsoleCtrlEvent(ctrl_event: u32, process_group_id: u32) -> i32;
    }
    const CTRL_BREAK_EVENT: u32 = 1;
    // Ignore the return value — we always escalate to kill() after the grace
    // period regardless, and CTRL_BREAK may not be catchable by Rscript.
    unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid); }
}

pub(crate) fn kill_hard(pid: u32) {
    if !super::procid::pid_in_range(pid) { return; }
    #[cfg(unix)]
    { extern "C" { fn kill(pid: i32, sig: i32) -> i32; }
      unsafe { kill(pid as i32, 9); } } // SIGKILL = 9
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("taskkill");
        cmd.args(["/F", "/PID", &pid.to_string()]);
        let _ = crate::io::r_extract::apply_no_window(cmd).spawn();
    }
}

/// Exit code reported for a run whose outcome could not be determined.
pub const EXIT_UNKNOWN: i32 = -2;

/// Outcome of a run that ended while the GUI was not watching: Completed only when the run script
/// recorded status 0 AND the bundle parses with a finite OFV; a recorded non-zero status is a
/// failure; anything else is Unknown rather than a guess.
pub fn orphan_exit_code(status_path: Option<&Path>, record: &RunRecord) -> i32 {
    let status = status_path
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| t.trim().parse::<i32>().ok());
    let bundle = record.directory.join(format!("{}.fitrx", record.model_stem));
    classify_orphan(status, || {
        crate::io::fitrx::read_fit_summary(&bundle).map(|f| f.ofv.is_finite()).unwrap_or(false)
    })
}

fn classify_orphan(status: Option<i32>, bundle_ok: impl FnOnce() -> bool) -> i32 {
    match status {
        Some(0) if bundle_ok() => 0,
        Some(0) => EXIT_UNKNOWN, // claimed success, but no valid result: do not call it Completed
        Some(n) => if n == 0 { 1 } else { n },
        None => EXIT_UNKNOWN,
    }
}

/// Send `RunFinished` and remove the manifest.
fn finish(mut record: RunRecord, exit_code: i32, manifest_path: &Path, tx: &Sender<WorkerMsg>) {
    if let Some(m) = RunManifest::load(manifest_path) {
        if let Some(sp) = m.status_path { let _ = std::fs::remove_file(sp); }
    }
    record.completed = Some(now_iso());
    record.status = match exit_code {
        0 => JobStatus::Completed,
        EXIT_UNKNOWN => JobStatus::Unknown,
        _ => JobStatus::Failed,
    };
    RunManifest::remove(manifest_path);
    let _ = tx.send(WorkerMsg::RunFinished { exit_code, record: Box::new(record) });
}

// ---------------------------------------------------------------------------
// Log reader — keeps a seek position across calls
// ---------------------------------------------------------------------------

struct LogReader {
    path: PathBuf,
    file: Option<File>,
    pos:  u64,
    /// Model stem this log belongs to — tags every `RunLine` sent so the
    /// main thread can route it to the right per-stem log (see
    /// `RunState::run_logs`) when several runs are tailed concurrently.
    stem: String,
}

impl LogReader {
    fn new(path: PathBuf, stem: String) -> Self {
        Self { path, file: None, pos: 0, stem }
    }

    fn drain(&mut self, tx: &Sender<WorkerMsg>) {
        // Open lazily — file may not exist yet at the start of a run.
        if self.file.is_none() {
            self.file = File::open(&self.path).ok();
        }
        let Some(f) = &mut self.file else { return };
        let _ = f.seek(SeekFrom::Start(self.pos));
        let mut reader = BufReader::new(&*f);
        loop {
            let mut buf = Vec::new();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buf);
                    let trimmed = line.trim_end_matches(['\n', '\r']).to_string();
                    if tx.send(WorkerMsg::RunLine { stem: self.stem.clone(), line: trimmed }).is_err() {
                        return;
                    }
                }
                Err(_) => break,
            }
        }
        self.pos = reader.into_inner().stream_position().unwrap_or(self.pos);
    }
}

// ---------------------------------------------------------------------------
// Timestamp helpers  (pub so models_tab can import instead of duplicating)
// ---------------------------------------------------------------------------

/// Format any Unix-epoch second count as `YYYY-MM-DD HH:MM` (no seconds).
pub fn unix_to_datetime(secs: u64) -> String {
    let (year, month, day, hour, min, _) = unix_to_parts(secs);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{min:02}")
}

/// Reformats a `now_iso()`-produced timestamp ("YYYY-MM-DDTHH:MM:SSZ") to
/// the same "YYYY-MM-DD HH:MM" convention `unix_to_datetime` uses, for the
/// run-record timestamps stored as strings rather than raw unix seconds.
/// Falls back to returning the input unchanged if it doesn't contain a
/// `T` — reachable for `RunRecord` timestamps loaded from a hand-edited or
/// schema-drifted `runs.json`, not just malformed in-memory data.
pub fn format_iso_timestamp(iso: &str) -> String {
    match iso.split_once('T') {
        Some((date, rest)) => format!("{date} {}", rest.get(0..5).unwrap_or(rest)),
        None => iso.to_string(),
    }
}

fn unix_to_parts(total_secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    let sec  = (total_secs % 60) as u32;
    let min  = ((total_secs / 60) % 60) as u32;
    let hour = ((total_secs / 3600) % 24) as u32;
    let mut rem_days = (total_secs / 86400) as u32;
    let mut year = 1970u32;
    loop {
        let diy = if is_leap(year) { 366 } else { 365 };
        if rem_days < diy { break; }
        rem_days -= diy;
        year += 1;
    }
    let leap = is_leap(year);
    let month_days: [u32; 12] = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 1u32;
    for &md in &month_days {
        if rem_days < md { break; }
        rem_days -= md;
        month += 1;
    }
    (year, month, rem_days + 1, hour, min, sec)
}

pub fn now_unix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Formats unix seconds as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn now_iso() -> String {
    let (year, month, day, hour, min, sec) = unix_to_parts(now_unix());
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

fn is_leap(y: u32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

#[cfg(test)]
mod log_reader_tests {
    use super::*;

    #[test]
    fn log_reader_keeps_lines_after_bad_byte() {
        let path = std::env::temp_dir().join(format!("ferxgui_log_{}.txt", std::process::id()));
        std::fs::write(&path, b"first line\n\xe9t\xe9 ok\nthird line\n").unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        LogReader::new(path.clone(), "m".into()).drain(&tx);
        drop(tx);
        let lines: Vec<String> = rx.iter().filter_map(|m| match m {
            WorkerMsg::RunLine { line, .. } => Some(line), _ => None }).collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[2], "third line");
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod format_iso_timestamp_tests {
    use super::*;

    #[test]
    fn formats_a_real_now_iso_output_with_single_space() {
        let out = format_iso_timestamp(&now_iso());
        assert!(out.contains(' ') && !out.contains("  "), "expected single-space separator, got: {out}");
        assert!(!out.contains('T'), "T should have been replaced, got: {out}");
    }

    #[test]
    fn falls_back_to_input_unchanged_when_no_t_present() {
        assert_eq!(format_iso_timestamp("not-a-timestamp"), "not-a-timestamp");
    }
}

#[cfg(test)]
mod orphan_status_tests {
    use super::*;

    #[test]
    fn orphan_without_status_is_unknown() {
        assert_eq!(classify_orphan(None, || true), EXIT_UNKNOWN);
    }

    #[test]
    fn orphan_status_zero_and_valid_bundle_is_completed() {
        assert_eq!(classify_orphan(Some(0), || true), 0);
        // success claimed but no valid bundle: not Completed
        assert_eq!(classify_orphan(Some(0), || false), EXIT_UNKNOWN);
    }

    #[test]
    fn orphan_status_nonzero_is_failed() {
        assert_eq!(classify_orphan(Some(1), || true), 1);
        assert_eq!(classify_orphan(Some(137), || false), 137);
    }

    #[cfg(unix)]
    #[test]
    fn kill_refused_on_identity_mismatch() {
        use super::super::procid::ProcessIdentity;
        let mut child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        let dir = std::env::temp_dir().join(format!("ferxgui_orphan_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = RunManifest {
            version: RunManifest::VERSION, run_id: "r".into(), model_stem: "m".into(), pid,
            log_path: dir.join("m.log"), command: "sleep".into(), directory: dir.clone(),
            status_path: Some(dir.join("m.status")),
            identity: Some(ProcessIdentity { pid, start_time: "definitely-not-its-start-time".into() }),
        };
        let record = RunRecord {
            id: "r".into(), model_stem: "m".into(), tool: "ferx".into(), method: None,
            status: JobStatus::Running, started: String::new(), completed: None, duration_secs: None,
            command: String::new(), directory: dir.clone(), data_path: None, file_hashes: Default::default(),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let (cancel_tx, cancel_rx) = std::sync::mpsc::channel();
        reconnect_orphan(manifest, dir.join("m.runmfst"), record, tx, cancel_rx);
        cancel_tx.send(CancelMode::Kill).unwrap();
        // The worker must say it refused, and the process must still be alive.
        let mut refused = false;
        let t0 = std::time::Instant::now();
        while t0.elapsed() < Duration::from_secs(3) && !refused {
            if let Ok(WorkerMsg::RunLine { line, .. }) = rx.recv_timeout(Duration::from_millis(200)) {
                refused = line.contains("Cancel refused");
            }
        }
        assert!(refused, "worker did not report the refusal");
        assert!(child.try_wait().unwrap().is_none(), "child must be untouched");
        // Real kill with the true identity works; the worker then reports Unknown (no status file).
        assert!(super::super::procid::may_signal(ProcessIdentity::of(pid).as_ref(), pid));
        kill_hard(pid);
        let _ = child.wait();
        let mut finished = None;
        let t1 = std::time::Instant::now();
        while t1.elapsed() < Duration::from_secs(6) && finished.is_none() {
            if let Ok(WorkerMsg::RunFinished { exit_code, record }) = rx.recv_timeout(Duration::from_millis(300)) {
                finished = Some((exit_code, record.status.clone()));
            }
        }
        assert_eq!(finished, Some((EXIT_UNKNOWN, JobStatus::Unknown)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
