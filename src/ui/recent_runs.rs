//! Central "Recent runs" log: the last runs from *every* project, live and finished, in one
//! glanceable list opened from the status bar. The History tab stays the place for digging.
//!
//! The list is a pure projection of `RunState::active_runs` (live) and `RunState::run_history`
//! (finished, persisted in `~/.ferxgui/runs.json`); nothing here is stored separately.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::egui;

use crate::app::theme;
use crate::domain::{ActiveRun, JobStatus, RunRecord};
use crate::state::{AppState, Tab};

/// Rows shown before the user asks for more.
const COMPACT_ROWS: usize = 10;
/// Rows shown after "Show more"; everything older is in the History tab.
const EXPANDED_ROWS: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Running,
    Finished,
    Failed,
    Cancelled,
    Unknown,
}

impl Outcome {
    fn word(self) -> &'static str {
        match self {
            Outcome::Running   => "Running",
            Outcome::Finished  => "Finished",
            Outcome::Failed    => "Failed",
            Outcome::Cancelled => "Cancelled",
            Outcome::Unknown   => "Unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Row {
    pub outcome: Outcome,
    pub stem: String,
    pub project: String,
    pub directory: PathBuf,
    pub method: String,
    /// Elapsed (running) or total duration (finished).
    pub duration: String,
    /// OFV for finished runs, a short reason for failed/unknown ones.
    pub detail: String,
    /// End time (finished) or start time (running), `YYYY-MM-DD HH:MM` UTC.
    pub when: String,
    /// Index into `run_history`; `None` for a live run.
    pub hist_idx: Option<usize>,
    /// A failed/unknown run the user has not looked at yet; kept at the top of the finished runs.
    pub pinned: bool,
}

/// The folder's own name, which is how a user thinks of a project.
pub fn project_name(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.display().to_string())
}

fn fmt_secs(secs: u64) -> String {
    if secs < 60 { format!("{secs}s") } else { format!("{}m {:02}s", secs / 60, secs % 60) }
}

fn outcome_of(status: &JobStatus) -> Outcome {
    match status {
        JobStatus::Running   => Outcome::Running,
        JobStatus::Completed => Outcome::Finished,
        JobStatus::Failed    => Outcome::Failed,
        JobStatus::Cancelled => Outcome::Cancelled,
        JobStatus::Unknown   => Outcome::Unknown,
    }
}

fn is_unseen_failure(r: &RunRecord) -> bool {
    !r.acknowledged && matches!(r.status, JobStatus::Failed | JobStatus::Unknown)
}

/// Every run, in display order: live runs (newest first), then unseen failures, then the rest by
/// end time (newest first). The caller truncates.
pub fn collect(active: &HashMap<String, ActiveRun>, history: &[RunRecord]) -> Vec<Row> {
    let mut live: Vec<&ActiveRun> = active.values().collect();
    live.sort_by_key(|r| std::cmp::Reverse(r.started_at));
    let mut rows: Vec<Row> = live.into_iter().map(|r| Row {
        outcome:   Outcome::Running,
        stem:      r.record.model_stem.clone(),
        project:   project_name(&r.record.directory),
        directory: r.record.directory.clone(),
        method:    r.record.method.clone().unwrap_or_default(),
        duration:  fmt_secs(r.started_at.elapsed().as_secs()),
        detail:    String::new(),
        when:      crate::workers::run::format_iso_timestamp(&r.record.started),
        hist_idx:  None,
        pinned:    false,
    }).collect();

    // ISO-8601 UTC strings sort chronologically; a record without `completed` falls back to
    // `started`. Ties break on position, so the later-appended record wins.
    let end_key = |r: &RunRecord| r.completed.clone().unwrap_or_else(|| r.started.clone());
    let mut order: Vec<usize> = (0..history.len()).collect();
    order.sort_by(|&a, &b| {
        is_unseen_failure(&history[b]).cmp(&is_unseen_failure(&history[a]))
            .then_with(|| end_key(&history[b]).cmp(&end_key(&history[a])))
            .then_with(|| b.cmp(&a))
    });

    rows.extend(order.into_iter().map(|i| {
        let r = &history[i];
        let outcome = outcome_of(&r.status);
        let detail = match outcome {
            Outcome::Finished => match r.ofv {
                Some(o) => format!("OFV {o:.2}"),
                None => "OFV —".to_string(),
            },
            Outcome::Failed    => "run failed".to_string(),
            Outcome::Unknown   => "no exit status recorded".to_string(),
            Outcome::Cancelled => "stopped by user".to_string(),
            Outcome::Running   => String::new(),
        };
        Row {
            outcome,
            stem: r.model_stem.clone(),
            project: project_name(&r.directory),
            directory: r.directory.clone(),
            method: r.method.clone().unwrap_or_default(),
            duration: r.duration_secs.map(|d| fmt_secs(d as u64)).unwrap_or_default(),
            detail,
            when: crate::workers::run::format_iso_timestamp(&end_key(r)),
            hist_idx: Some(i),
            pinned: is_unseen_failure(r),
        }
    }));
    rows
}

/// Open a run's model, switching project first when it belongs to another one.
/// Returns the message to show when that is not possible.
pub fn open_run(state: &mut AppState, dir: &Path, stem: &str) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("Project folder no longer exists: {}", dir.display()));
    }
    if state.workspace.directory.as_deref() == Some(dir) {
        let idx = state.workspace.models.iter().position(|m| m.model.stem == stem)
            .ok_or_else(|| format!("{stem} is no longer in {}", project_name(dir)))?;
        state.ui.selected_model = Some(idx);
    } else {
        // The scan that follows `set_directory` selects the model once it is found.
        state.ui.select_after_scan = Some(stem.to_string());
        state.set_directory(dir.to_path_buf());
    }
    state.ui.active_tab = Tab::Models;
    Ok(())
}

// ── UI ───────────────────────────────────────────────────────────────────────

/// Status-bar button that toggles the panel; summarises what is going on without opening it.
pub fn status_button(ui: &mut egui::Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;
    let running = state.run.active_runs.len();
    let failed = state.run.run_history.iter().filter(|r| is_unseen_failure(r)).count();
    let unseen = state.run.unseen_finished;

    let mut text = if running > 0 { format!("{running} running") } else { "Runs".to_string() };
    if unseen > 0 { text.push_str(&format!(" · {unseen} new")); }
    if failed > 0 { text.push_str(&format!(" · {failed} failed")); }
    // Words carry the meaning; colour only reinforces it.
    let color = if failed > 0 {
        theme::danger_text(dark)
    } else if running > 0 {
        theme::accent(dark)
    } else {
        theme::fg2(dark)
    };
    let resp = ui
        .add(egui::Button::new(egui::RichText::new(text).size(11.0).color(color)).frame(false))
        .on_hover_text("Recent runs from all projects");
    if resp.clicked() {
        state.ui.runs_panel_open = !state.ui.runs_panel_open;
    }
    if running > 0 { ui.spinner(); }
}

fn symbol(ui: &mut egui::Ui, outcome: Outcome) {
    match outcome {
        Outcome::Running   => { ui.add(egui::Spinner::new().size(12.0)); }
        Outcome::Finished  => { ui.label(egui::RichText::new("✔").color(theme::GREEN)); }
        Outcome::Failed    => { ui.label(egui::RichText::new("✖").color(theme::RED)); }
        Outcome::Cancelled => { ui.label(egui::RichText::new("–").color(theme::ORANGE)); }
        Outcome::Unknown   => { ui.label(egui::RichText::new("?").color(theme::ORANGE)); }
    }
}

enum Action {
    /// Open the model of this row (project folder + model stem).
    Open { dir: PathBuf, stem: String, hist_idx: Option<usize> },
    Dismiss(usize),
    OpenFolder(PathBuf),
    DismissAllFailures,
    ToggleMore,
    GoToHistory,
}

pub fn show(ctx: &egui::Context, state: &mut AppState) {
    if !state.ui.runs_panel_open { return; }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.ui.runs_panel_open = false;
        return;
    }
    // Looking at the panel is what "seen" means.
    state.run.unseen_finished = 0;

    let dark = state.workspace.settings.theme == crate::io::persistence::Theme::Dark;
    let rows = collect(&state.run.active_runs, &state.run.run_history);
    let total = rows.len();
    let limit = if state.ui.runs_panel_expanded { EXPANDED_ROWS } else { COMPACT_ROWS };
    let shown = &rows[..total.min(limit)];
    let current_dir = state.workspace.directory.clone();
    let mut action: Option<Action> = None;
    let mut close = false;

    egui::Area::new(egui::Id::new("recent_runs_panel"))
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-8.0, -28.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(560.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Recent runs").size(13.0).color(theme::fg(dark)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("✖").on_hover_text("Close (Esc)").clicked() { close = true; }
                        if ui.small_button("All history").clicked() { action = Some(Action::GoToHistory); }
                        if rows.iter().any(|r| r.pinned)
                            && ui.small_button("Dismiss failures").clicked()
                        {
                            action = Some(Action::DismissAllFailures);
                        }
                    });
                });
                ui.separator();
                if shown.is_empty() {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("No runs yet on this computer. Runs from every project appear here.")
                            .size(12.0).color(theme::fg3(dark)),
                    );
                    ui.add_space(6.0);
                    return;
                }
                egui::ScrollArea::vertical().max_height(360.0).auto_shrink([true, true]).show(ui, |ui| {
                    egui::Grid::new("recent_runs_grid").num_columns(7).spacing([12.0, 5.0]).show(ui, |ui| {
                        for row in shown {
                            let gone = !row.directory.is_dir();
                            ui.horizontal(|ui| {
                                symbol(ui, row.outcome);
                                ui.label(egui::RichText::new(row.outcome.word()).size(11.0).color(theme::fg2(dark)));
                            });
                            let name_color = if gone { theme::fg3(dark) } else { theme::fg(dark) };
                            let mut name = egui::RichText::new(&row.stem).size(12.0).color(name_color);
                            if row.pinned { name = name.italics(); }
                            let resp = ui.add(egui::Label::new(name).sense(egui::Sense::click()))
                                .on_hover_text(if gone {
                                    format!("{} (folder no longer exists)", row.directory.display())
                                } else {
                                    format!("{}\nClick to open this model", row.directory.display())
                                });
                            if resp.clicked() {
                                action = Some(Action::Open {
                                    dir: row.directory.clone(),
                                    stem: row.stem.clone(),
                                    hist_idx: row.hist_idx,
                                });
                            }
                            resp.context_menu(|ui| {
                                if ui.button("Open folder").clicked() {
                                    action = Some(Action::OpenFolder(row.directory.clone()));
                                    ui.close_menu();
                                }
                                if let Some(i) = row.hist_idx.filter(|_| row.pinned) {
                                    if ui.button("Dismiss").clicked() {
                                        action = Some(Action::Dismiss(i));
                                        ui.close_menu();
                                    }
                                }
                            });
                            let project_color = if current_dir.as_deref() == Some(row.directory.as_path()) {
                                theme::fg2(dark)
                            } else {
                                theme::fg3(dark)
                            };
                            ui.label(egui::RichText::new(&row.project).size(11.0).color(project_color));
                            ui.label(egui::RichText::new(&row.method).size(11.0).color(theme::fg3(dark)));
                            ui.label(egui::RichText::new(&row.duration).size(11.0).monospace().color(theme::fg2(dark)));
                            ui.label(egui::RichText::new(&row.detail).size(11.0).color(theme::fg2(dark)));
                            ui.label(egui::RichText::new(&row.when).size(11.0).color(theme::fg3(dark)));
                            ui.end_row();
                        }
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    let running = rows.iter().filter(|r| r.outcome == Outcome::Running).count();
                    ui.label(
                        egui::RichText::new(format!("{running} running · {total} runs on this computer"))
                            .size(11.0).color(theme::fg3(dark)),
                    );
                    if total > COMPACT_ROWS {
                        let label = if state.ui.runs_panel_expanded { "Show fewer" } else { "Show more" };
                        if ui.small_button(label).clicked() { action = Some(Action::ToggleMore); }
                    }
                });
            });
        });

    if close { state.ui.runs_panel_open = false; }
    match action {
        None => {}
        Some(Action::ToggleMore) => state.ui.runs_panel_expanded = !state.ui.runs_panel_expanded,
        Some(Action::GoToHistory) => {
            state.ui.active_tab = Tab::History;
            state.ui.runs_panel_open = false;
        }
        Some(Action::OpenFolder(dir)) => {
            if let Err(e) = open::that_detached(&dir) {
                state.ui.status_message = format!("Could not open {}: {e}", dir.display());
            }
        }
        Some(Action::Dismiss(i)) => acknowledge(state, Some(i)),
        Some(Action::DismissAllFailures) => acknowledge(state, None),
        Some(Action::Open { dir, stem, hist_idx }) => {
            // Opening is looking: it clears the failure pin even if the folder has since gone.
            if let Some(i) = hist_idx { acknowledge(state, Some(i)); }
            match open_run(state, &dir, &stem) {
                Ok(()) => state.ui.runs_panel_open = false,
                Err(msg) => state.ui.status_message = msg,
            }
        }
    }
}

/// Mark one failed/unknown run (or all of them) as seen, and save.
fn acknowledge(state: &mut AppState, which: Option<usize>) {
    match which {
        Some(i) => {
            if let Some(r) = state.run.run_history.get_mut(i) { r.acknowledged = true; }
        }
        None => state.run.run_history.iter_mut().for_each(|r| r.acknowledged = true),
    }
    if let Some(warn) = state.run.save_history(state.workspace.app_dir.as_ref()) {
        state.ui.status_message = warn;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(stem: &str, status: JobStatus, completed: &str, dir: &str) -> RunRecord {
        RunRecord {
            id: format!("{stem}-{completed}"), model_stem: stem.into(), tool: "ferx".into(),
            method: Some("FOCEI".into()), status, started: "2026-01-01T00:00:00Z".into(),
            completed: Some(completed.into()), duration_secs: Some(75.0), command: String::new(),
            directory: PathBuf::from(dir), data_path: None, file_hashes: Default::default(),
            ofv: None, acknowledged: false,
        }
    }

    fn live(stem: &str, dir: &str) -> ActiveRun {
        let (cancel_tx, _rx) = std::sync::mpsc::channel();
        ActiveRun {
            record: rec(stem, JobStatus::Running, "", dir),
            started_at: std::time::Instant::now(),
            log_path: PathBuf::new(),
            cancel_tx,
            export_tables: false,
            run_sir_after: false,
        }
    }

    fn stems(rows: &[Row]) -> Vec<&str> { rows.iter().map(|r| r.stem.as_str()).collect() }

    #[test]
    fn live_runs_first_then_unseen_failures_then_newest_first() {
        let history = vec![
            rec("old_ok",  JobStatus::Completed, "2026-01-01T10:00:00Z", "/p/a"),
            rec("old_bad", JobStatus::Failed,    "2026-01-01T09:00:00Z", "/p/a"),
            rec("new_ok",  JobStatus::Completed, "2026-01-02T10:00:00Z", "/p/b"),
        ];
        let mut active = HashMap::new();
        active.insert("running_one".to_string(), live("running_one", "/p/c"));
        let rows = collect(&active, &history);
        assert_eq!(stems(&rows), ["running_one", "old_bad", "new_ok", "old_ok"]);
        assert_eq!(rows[0].outcome, Outcome::Running);
        assert!(rows[1].pinned && !rows[2].pinned);
    }

    #[test]
    fn dismissed_failure_is_no_longer_pinned_and_sorts_by_time() {
        let mut bad = rec("bad", JobStatus::Failed, "2026-01-01T09:00:00Z", "/p");
        bad.acknowledged = true;
        let history = vec![bad, rec("ok", JobStatus::Completed, "2026-01-02T00:00:00Z", "/p")];
        let rows = collect(&HashMap::new(), &history);
        assert_eq!(stems(&rows), ["ok", "bad"]);
        assert!(!rows[1].pinned);
    }

    #[test]
    fn same_model_run_twice_gives_two_rows_and_ties_keep_the_later_record_first() {
        let t = "2026-01-01T10:00:00Z";
        let history = vec![rec("m", JobStatus::Completed, t, "/p"), rec("m", JobStatus::Failed, t, "/p")];
        let rows = collect(&HashMap::new(), &history);
        assert_eq!(rows.len(), 2);
        // The failure is pinned, so it leads; both rows are kept.
        assert_eq!(rows[0].hist_idx, Some(1));
        assert_eq!(rows[1].hist_idx, Some(0));
    }

    #[test]
    fn row_carries_ofv_project_and_duration_from_the_record_alone() {
        let mut r = rec("m", JobStatus::Completed, "2026-01-01T10:00:00Z", "/work/study_x");
        r.ofv = Some(-286.0432);
        let rows = collect(&HashMap::new(), &[r]);
        assert_eq!(rows[0].detail, "OFV -286.04");
        assert_eq!(rows[0].project, "study_x");
        assert_eq!(rows[0].duration, "1m 15s");
        assert_eq!(rows[0].when, "2026-01-01 10:00");
    }

    #[test]
    fn record_without_ofv_shows_a_dash_not_a_made_up_number() {
        let rows = collect(&HashMap::new(), &[rec("m", JobStatus::Completed, "2026-01-01T10:00:00Z", "/p")]);
        assert_eq!(rows[0].detail, "OFV —");
    }

    #[test]
    fn old_runs_json_without_the_new_fields_still_loads() {
        let json = r#"{"id":"a","model_stem":"m","tool":"ferx","status":"completed",
            "started":"2025-01-01T00:00:00Z","command":"x","directory":"/p"}"#;
        let r: RunRecord = serde_json::from_str(json).unwrap();
        assert_eq!(r.ofv, None);
        assert!(!r.acknowledged);
    }

    #[test]
    fn history_is_capped_and_counts_unseen() {
        let mut run = crate::state::RunState::load(None);
        for i in 0..(crate::state::RunState::MAX_HISTORY + 7) {
            run.push_record(rec(&format!("m{i}"), JobStatus::Completed, "2026-01-01T10:00:00Z", "/p"));
        }
        assert_eq!(run.run_history.len(), crate::state::RunState::MAX_HISTORY);
        assert_eq!(run.run_history[0].model_stem, "m7", "the oldest records are the ones dropped");
        assert_eq!(run.unseen_finished, crate::state::RunState::MAX_HISTORY + 7);
    }

    #[test]
    fn opening_a_run_whose_folder_is_gone_reports_it_and_changes_nothing() {
        let mut st = crate::state::AppState::new();
        let before = st.ui.active_tab;
        let err = open_run(&mut st, Path::new("/definitely/not/a/folder"), "m").unwrap_err();
        assert!(err.contains("no longer exists"), "{err}");
        assert_eq!(st.ui.active_tab, before);
        assert!(st.ui.select_after_scan.is_none());
    }

    #[test]
    fn project_name_is_the_folder_name() {
        assert_eq!(project_name(Path::new("/a/b/study")), "study");
        assert_eq!(project_name(Path::new("/")), "/");
    }
}
