//! File and folder dialogs that never block the UI thread on Linux.
//!
//! A dialog is requested under an id and its result is collected later with [`take`], from the same
//! place in the UI code every frame. On Linux the dialog runs on a worker thread, so a dialog that
//! never appears (no GTK, a window hidden behind the app over X forwarding) cannot freeze the
//! window; a second request after a few seconds starts a fresh dialog instead of being ignored,
//! and a dialog that fails at once is reported. macOS and Windows keep their native dialogs,
//! called directly as before.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Kind { #[default] OpenFile, SaveFile, OpenFolder }

#[derive(Clone, Debug, Default)]
pub struct Spec {
    pub kind: Kind,
    pub filters: Vec<(String, Vec<String>)>,
    pub file_name: Option<String>,
}

impl Spec {
    pub fn open_file() -> Self { Self { kind: Kind::OpenFile, ..Self::default() } }
    pub fn save_file(name: impl Into<String>) -> Self {
        Self { kind: Kind::SaveFile, file_name: Some(name.into()), ..Self::default() }
    }
    pub fn folder() -> Self { Self { kind: Kind::OpenFolder, ..Self::default() } }
    pub fn filter(mut self, label: &str, exts: &[&str]) -> Self {
        self.filters.push((label.to_string(), exts.iter().map(|e| e.to_string()).collect()));
        self
    }
}

enum Slot { Pending(Instant), Done(Option<PathBuf>) }

static SLOTS: Mutex<Option<HashMap<String, Slot>>> = Mutex::new(None);
static NOTES: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// A second request for the same id after this long starts a new dialog (the first may be hidden).
const RETRY_AFTER: Duration = Duration::from_secs(10);
/// A dialog that returns "cancelled" faster than a person can react did not really show.
const INSTANT_FAIL: Duration = Duration::from_millis(400);

fn with_slots<R>(f: impl FnOnce(&mut HashMap<String, Slot>) -> R) -> R {
    let mut g = SLOTS.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(HashMap::new))
}

fn with_notes<R>(f: impl FnOnce(&mut HashMap<String, String>) -> R) -> R {
    let mut g = NOTES.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(HashMap::new))
}

fn set_note(id: &str, msg: &str) { with_notes(|n| { n.insert(id.to_string(), msg.to_string()); }); }

/// A message to show in the status bar about a dialog that could not be shown or is already open.
pub fn take_note() -> Option<String> {
    with_notes(|n| { let k = n.keys().next().cloned()?; n.remove(&k) })
}

#[cfg(test)]
fn take_note_for(id: &str) -> Option<String> { with_notes(|n| n.remove(id)) }

/// Show a dialog; the result is later returned by [`take`] under the same `id`.
pub fn request(id: &str, spec: Spec, ctx: &eframe::egui::Context) {
    let ctx = ctx.clone();
    start(id, cfg!(target_os = "linux"), move || show(&spec), move || ctx.request_repaint());
}

/// The chosen path once the dialog has closed with a selection; None while it is open, when it was
/// cancelled, or when nothing was requested. Each result is returned once.
pub fn take(id: &str) -> Option<PathBuf> {
    with_slots(|s| match s.get(id) {
        Some(Slot::Done(_)) => match s.remove(id) { Some(Slot::Done(p)) => p, _ => None },
        _ => None,
    })
}

/// True while the dialog for `id` is open.
pub fn pending(id: &str) -> bool {
    with_slots(|s| matches!(s.get(id), Some(Slot::Pending(_))))
}

fn start(
    id: &str,
    threaded: bool,
    job: impl FnOnce() -> Option<PathBuf> + Send + std::panic::UnwindSafe + 'static,
    repaint: impl Fn() + Send + 'static,
) {
    let proceed = with_slots(|s| {
        if let Some(Slot::Pending(since)) = s.get(id) {
            if since.elapsed() < RETRY_AFTER { return false; }
        }
        s.insert(id.to_string(), Slot::Pending(Instant::now()));
        true
    });
    if !proceed {
        set_note(id, "A file dialog is already open. If you cannot see it, it may be behind this window; \
                  click again in a few seconds to open a new one.");
        return;
    }
    let id = id.to_string();
    let run = move || {
        let t0 = Instant::now();
        let result = std::panic::catch_unwind(job).unwrap_or(None);
        if result.is_none() && t0.elapsed() < INSTANT_FAIL {
            set_note(&id, "The file dialog could not be shown (no GTK dialog is available on this \
                      connection). Type the path instead where the screen offers a path box.");
        }
        with_slots(|s| { s.insert(id, Slot::Done(result)); });
        repaint();
    };
    if threaded { std::thread::spawn(run); } else { run(); }
}

fn show(spec: &Spec) -> Option<PathBuf> {
    let mut d = rfd::FileDialog::new();
    for (label, exts) in &spec.filters {
        let ex: Vec<&str> = exts.iter().map(String::as_str).collect();
        d = d.add_filter(label, &ex);
    }
    if let Some(n) = &spec.file_name { d = d.set_file_name(n); }
    match spec.kind {
        Kind::OpenFile => d.pick_file(),
        Kind::SaveFile => d.save_file(),
        Kind::OpenFolder => d.pick_folder(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn wait_done(id: &str) -> Option<PathBuf> {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(3) {
            if !pending(id) { return take(id); }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("dialog {id} never finished");
    }

    #[test]
    fn a_selection_is_returned_once() {
        start("t_sel", true, || Some(PathBuf::from("/x/y")), || {});
        assert_eq!(wait_done("t_sel"), Some(PathBuf::from("/x/y")));
        assert_eq!(take("t_sel"), None, "each result is taken once");
    }

    #[test]
    fn cancel_gives_nothing_and_a_panicking_dialog_does_not_leave_it_pending() {
        start("t_cancel", false, || None, || {});
        assert_eq!(take("t_cancel"), None);
        assert!(!pending("t_cancel"));
        start("t_panic", true, || panic!("no display"), || {});
        assert_eq!(wait_done("t_panic"), None);
        assert!(take_note_for("t_panic").is_some(), "an instant failure is reported");
    }

    #[test]
    fn a_second_request_while_open_is_refused_with_a_note() {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let started = Arc::new(AtomicUsize::new(0));
        let s1 = started.clone();
        start("t_busy", true, move || { s1.fetch_add(1, Ordering::SeqCst); let _ = rx.recv(); None }, || {});
        std::thread::sleep(Duration::from_millis(50));
        assert!(pending("t_busy"));
        let s2 = started.clone();
        start("t_busy", true, move || { s2.fetch_add(1, Ordering::SeqCst); None }, || {});
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(started.load(Ordering::SeqCst), 1, "the second dialog must not start yet");
        assert!(take_note_for("t_busy").unwrap().contains("already open"));
        let _ = tx.send(());
        let _ = wait_done("t_busy");
    }

    #[test]
    fn spec_builders() {
        let s = Spec::open_file().filter("CSV", &["csv", "txt"]);
        assert_eq!(s.kind, Kind::OpenFile);
        assert_eq!(s.filters, vec![("CSV".to_string(), vec!["csv".to_string(), "txt".to_string()])]);
        assert_eq!(Spec::save_file("a.png").file_name.as_deref(), Some("a.png"));
        assert_eq!(Spec::folder().kind, Kind::OpenFolder);
    }
}
