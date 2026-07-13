use eframe::egui;
use crate::state::{AppState, Tab};
use crate::workers::run_manifest::{RunManifest, scan_manifests};

/// Design tokens.
///
/// Dark-mode constants are bare names (`BG2`, `FG2`, …).
/// Use the helper functions (`card_fill(dark)`, `fg2(dark)`, …) whenever
/// rendering must work in both themes — they return the appropriate value
/// based on the `dark` flag from `ui.visuals().dark_mode`.
pub mod theme {
    use eframe::egui::Color32;

    // ── Dark-mode tokens ──────────────────────────────────────────────────
    pub const BG:      Color32 = Color32::from_rgb(0x1a, 0x1a, 0x20);
    pub const BG2:     Color32 = Color32::from_rgb(0x22, 0x22, 0x2c);
    pub const BG3:     Color32 = Color32::from_rgb(0x2a, 0x2a, 0x36);
    pub const BG4:     Color32 = Color32::from_rgb(0x32, 0x32, 0x3f);
    pub const BORDER:  Color32 = Color32::from_rgb(0x7a, 0x7a, 0x94);
    pub const FG:      Color32 = Color32::from_rgb(0xdd, 0xe0, 0xee);
    pub const FG2:     Color32 = Color32::from_rgb(0x9a, 0x9d, 0xb8);
    pub const FG3:     Color32 = Color32::from_rgb(0x6a, 0x6d, 0x88);
    pub const ACCENT:  Color32 = Color32::from_rgb(0x4c, 0x8a, 0xff);
    pub const GREEN:   Color32 = Color32::from_rgb(0x3e, 0xc9, 0x7a);
    pub const RED:     Color32 = Color32::from_rgb(0xe8, 0x55, 0x55);
    pub const ORANGE:  Color32 = Color32::from_rgb(0xe8, 0x95, 0x40);
    pub const YELLOW:  Color32 = Color32::from_rgb(0xd4, 0xc0, 0x60);
    pub const STAR:    Color32 = Color32::from_rgb(0xf0, 0xc0, 0x40);

    // ── Light-mode equivalents ────────────────────────────────────────────
    const BG2_L:    Color32 = Color32::from_rgb(0xf5, 0xf6, 0xf9);
    const BG3_L:    Color32 = Color32::from_rgb(0xed, 0xee, 0xf3);
    const FG_L:     Color32 = Color32::from_rgb(0x0f, 0x11, 0x1a);
    const FG2_L:    Color32 = Color32::from_rgb(0x50, 0x54, 0x69);  // WCAG AA on white
    const FG3_L:    Color32 = Color32::from_rgb(0x8c, 0x8f, 0xa3);
    // `ACCENT` (0x4c8aff) is tuned for dark backgrounds — as *text* on a
    // light background it only measures ~3.1 contrast (below the 4.5 AA
    // floor). This darker/more saturated variant is what `light_visuals()`
    // already used locally for its own accent-as-text needs (hyperlinks,
    // the active widget state); it's promoted to a shared constant here so
    // other call sites (e.g. tab-strip active-state text) can reuse it via
    // `accent(dark)` instead of reaching for the dark-only `ACCENT`.
    pub const ACCENT_LIGHT: Color32 = Color32::from_rgb(0x25, 0x63, 0xeb);

    // ── Theme-aware helpers ───────────────────────────────────────────────

    /// Card / panel fill — the dominant surface colour for raised frames.
    pub fn card_fill(dark: bool)     -> Color32 { if dark { BG2    } else { BG2_L } }
    /// Elevated fill — slightly more prominent than card (section headers, hover).
    pub fn elevated_fill(dark: bool) -> Color32 { if dark { BG3    } else { BG3_L } }
    /// Primary text colour.
    pub fn fg(dark: bool)            -> Color32 { if dark { FG     } else { FG_L  } }
    /// Secondary / label text colour.  Meets WCAG AA on both themes.
    pub fn fg2(dark: bool)           -> Color32 { if dark { FG2    } else { FG2_L } }
    /// Muted / hint / placeholder text colour.
    pub fn fg3(dark: bool)           -> Color32 { if dark { FG3    } else { FG3_L } }
    /// Accent colour safe to use as *text* in either theme (see `ACCENT_LIGHT`).
    pub fn accent(dark: bool)        -> Color32 { if dark { ACCENT } else { ACCENT_LIGHT } }

    // ── Theme application ─────────────────────────────────────────────────

    pub fn apply_dark(ctx: &eframe::egui::Context) {
        ctx.set_visuals(dark_visuals());
    }

    pub fn apply_light(ctx: &eframe::egui::Context) {
        ctx.set_visuals(light_visuals());
    }

    pub(crate) fn dark_visuals() -> eframe::egui::Visuals {
        let mut v = eframe::egui::Visuals::dark();
        v.panel_fill            = BG;
        v.window_fill           = BG2;
        v.extreme_bg_color      = BG4;
        v.faint_bg_color        = BG3;
        v.widgets.noninteractive.bg_fill       = BG2;
        v.widgets.noninteractive.fg_stroke.color = FG2;
        v.widgets.inactive.bg_fill             = BG3;
        v.widgets.inactive.fg_stroke.color     = FG;
        v.widgets.hovered.bg_fill              = BG4;
        v.widgets.active.bg_fill               = ACCENT;
        v.widgets.active.fg_stroke.color       = eframe::egui::Color32::WHITE;
        v.selection.bg_fill    = ACCENT.linear_multiply(0.4);
        // Selected-state text: WHITE, not ACCENT — `interact_selectable()` (egui's
        // mechanism behind `selectable_label`/segmented controls) paints this
        // color as the *text* on top of `selection.bg_fill` above. Using the same
        // accent hue for both was reported as "light blue on blue" / hard to read
        // (measured contrast ratio ~2.0, below even the WCAG AA large-text floor
        // of 3.0); WHITE on this background measures ~6.6, comfortably AA-normal.
        v.selection.stroke.color = eframe::egui::Color32::WHITE;
        v.hyperlink_color      = ACCENT;
        v.window_stroke        = eframe::egui::Stroke::new(1.0, BORDER);
        v
    }

    pub(crate) fn light_visuals() -> eframe::egui::Visuals {
        let accent = ACCENT_LIGHT;
        let mut v  = eframe::egui::Visuals::light();
        // Surface hierarchy — matches the light-mode token values above.
        v.panel_fill            = eframe::egui::Color32::from_gray(248);
        v.window_fill           = BG2_L;
        v.faint_bg_color        = BG3_L;
        v.extreme_bg_color      = eframe::egui::Color32::WHITE;
        v.widgets.noninteractive.bg_fill       = BG2_L;
        v.widgets.noninteractive.fg_stroke.color = FG2_L;
        v.widgets.inactive.bg_fill             = BG3_L;
        v.widgets.inactive.fg_stroke.color     = FG_L;
        v.widgets.hovered.bg_fill              = eframe::egui::Color32::from_rgb(0xda, 0xdb, 0xe3);
        v.widgets.active.bg_fill               = accent;
        v.widgets.active.fg_stroke.color       = eframe::egui::Color32::WHITE;
        // Solid (not translucent) accent background with WHITE text — mirrors
        // `widgets.active` above. The previous translucent bg_fill (alpha
        // 45/255) mostly showed the pale panel through it, so the same-hue
        // `accent` text sat at only ~3.5 contrast (below the 4.5 AA-normal
        // floor); solid bg + WHITE text measures ~5.2.
        v.selection.bg_fill    = accent;
        v.selection.stroke.color = eframe::egui::Color32::WHITE;
        v.hyperlink_color      = accent;
        v.window_stroke        = eframe::egui::Stroke::new(1.0, eframe::egui::Color32::from_gray(210));
        v
    }
}

#[cfg(test)]
mod theme_contrast_tests {
    use super::theme::{dark_visuals, light_visuals};
    use eframe::egui::Color32;

    /// WCAG 2.x relative luminance + contrast ratio, computed directly from
    /// sRGB channel bytes (no egui context needed — pure color math).
    fn relative_luminance(c: Color32) -> f64 {
        let to_linear = |ch: u8| {
            let c = ch as f64 / 255.0;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * to_linear(c.r()) + 0.7152 * to_linear(c.g()) + 0.0722 * to_linear(c.b())
    }

    fn contrast_ratio(a: Color32, b: Color32) -> f64 {
        let (la, lb) = (relative_luminance(a), relative_luminance(b));
        let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
        (hi + 0.05) / (lo + 0.05)
    }

    // WCAG AA for normal-size text.
    const AA_NORMAL_TEXT: f64 = 4.5;

    /// Regression test for the "light blue on blue" / hard-to-read report:
    /// `selection.stroke.color` is the text color `interact_selectable()`
    /// paints on top of `selection.bg_fill` for any selected
    /// `selectable_label`/segmented control (e.g. VPC's "Continuous" toggle,
    /// the Dark/Light theme picker). Both themes must clear WCAG AA.
    #[test]
    fn selection_text_meets_aa_contrast_in_dark_theme() {
        let v = dark_visuals();
        let ratio = contrast_ratio(v.selection.stroke.color, v.selection.bg_fill);
        assert!(
            ratio >= AA_NORMAL_TEXT,
            "dark-theme selection text/bg contrast is {ratio:.2}, below AA floor of {AA_NORMAL_TEXT}"
        );
    }

    #[test]
    fn selection_text_meets_aa_contrast_in_light_theme() {
        let v = light_visuals();
        let ratio = contrast_ratio(v.selection.stroke.color, v.selection.bg_fill);
        assert!(
            ratio >= AA_NORMAL_TEXT,
            "light-theme selection text/bg contrast is {ratio:.2}, below AA floor of {AA_NORMAL_TEXT}"
        );
    }
}

// ---------------------------------------------------------------------------
// App struct
// ---------------------------------------------------------------------------

pub struct FerxApp {
    state: AppState,
}

impl FerxApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply_dark(&cc.egui_ctx);
        // Bump default font sizes — egui defaults are 14pt body but feel cramped here.
        {
            use egui::{FontId, TextStyle};
            let mut style = (*cc.egui_ctx.style()).clone();
            style.text_styles = [
                (TextStyle::Heading,  FontId::proportional(16.0)),
                (TextStyle::Body,     FontId::proportional(13.0)),
                (TextStyle::Monospace,FontId::monospace(12.0)),
                (TextStyle::Button,   FontId::proportional(13.0)),
                (TextStyle::Small,    FontId::proportional(11.0)),
            ].into();
            cc.egui_ctx.set_style(style);
        }
        let mut state = AppState::new();
        // Auto-scan if a working directory was persisted.
        if state.workspace.directory.is_some() {
            state.trigger_scan();
        }
        // Reconnect any ferx processes that outlived the previous GUI session.
        reconnect_orphaned_runs(&mut state);

        // Surface any startup warnings (missing home dir, corrupt settings file, etc.).
        if !state.workspace.startup_warnings.is_empty() {
            state.ui.status_message = state.workspace.startup_warnings.join("; ");
        }

        // Detect the ferx package via R on a background thread so the UI stays
        // responsive.  Skipped when the user has set a custom path.
        if state.workspace.ferx_binary_source == crate::io::persistence::FerxBinarySource::Detecting {
            let tx  = state.worker_tx.clone();
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                let result = crate::io::persistence::detect_ferx_from_r();
                let _ = tx.send(crate::workers::messages::WorkerMsg::FerxBinaryDetected(result));
                ctx.request_repaint();
            });
        }

        Self { state }
    }
}

impl eframe::App for FerxApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Collect incoming screenshot (requested last frame for tree export).
        if self.state.ui.tree_export_awaiting {
            let canvas_rect = self.state.ui.tree_canvas_rect;
            let ppp = ctx.pixels_per_point();
            // Screenshots arrive as events.
            let screenshot = ctx.input(|i| {
                i.events.iter().find_map(|e| {
                    if let egui::Event::Screenshot { image, .. } = e {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(img) = screenshot {
                self.state.ui.tree_export_awaiting = false;
                save_tree_png(&img, canvas_rect, ppp, &mut self.state);
            }
        }

        // Intercept the main window's close request while the Files tab has
        // unsaved edits, so quitting can't silently discard them the same
        // way switching files could (see files_tab's own guard).
        if ctx.input(|i| i.viewport().close_requested()) && !self.state.ui.quit_confirmed {
            let dirty = self.state.ui.files_text_dirty || self.state.ui.files_csv_dirty;
            if dirty {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.state.ui.quit_unsaved_dialog = true;
            }
        }
        show_quit_unsaved_dialog(ctx, &mut self.state);

        // Drain worker messages first.
        self.state.process_worker_messages();
        // Auto-advance the sequential run queue if no run is active.
        crate::ui::models_tab::advance_queue(&mut self.state);
        // Lazily trigger R model inspection for the currently selected model.
        trigger_r_inspect(&mut self.state, ctx);
        // Fire the vpc package version check once at startup so the result is
        // available everywhere (About popup, VPC tab banner) without requiring
        // the user to visit the VPC tab first.
        if self.state.ui.vpc_pkg_status.is_none() && !self.state.ui.vpc_pkg_checking {
            self.state.ui.vpc_pkg_checking = true;
            let tx  = self.state.worker_tx.clone();
            let ctx2 = ctx.clone();
            std::thread::spawn(move || {
                let res = crate::io::r_extract::vpc_package_version();
                let _ = tx.send(crate::workers::messages::WorkerMsg::VpcPkgStatus(res));
                ctx2.request_repaint();
            });
        }

        // Apply theme.
        match self.state.workspace.theme() {
            crate::io::persistence::Theme::Dark => theme::apply_dark(ctx),
            crate::io::persistence::Theme::Light => theme::apply_light(ctx),
        }

        // Keyboard shortcuts: Ctrl+1 … Ctrl+9.
        ctx.input(|i| {
            for (idx, tab) in Tab::ALL.iter().enumerate() {
                let key = match idx {
                    0 => egui::Key::Num1,
                    1 => egui::Key::Num2,
                    2 => egui::Key::Num3,
                    3 => egui::Key::Num4,
                    4 => egui::Key::Num5,
                    5 => egui::Key::Num6,
                    6 => egui::Key::Num7,
                    7 => egui::Key::Num8,
                    8 => egui::Key::Num9,
                    _ => return,
                };
                if i.modifiers.ctrl && i.key_pressed(key) {
                    self.state.ui.active_tab = *tab;
                }
            }
        });

        // Cmd+, on macOS / Ctrl+, on Windows and Linux — `Modifiers::command`
        // is egui's cross-platform abstraction for this (true Cmd on macOS,
        // aliased to Ctrl elsewhere), so no per-OS branching is needed here.
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Comma)) {
            self.state.ui.settings_open = true;
        }

        // Panel declaration order matters: top panels first, then bottom panels
        // (outermost first), then side panels, then central.
        render_menu_bar(ctx, &mut self.state);
        render_header(ctx, &mut self.state);
        render_status_bar(ctx, &self.state);
        render_sidebar(ctx, &mut self.state);
        render_body(ctx, &mut self.state);
        render_run_popup(ctx, &mut self.state);
        render_sir_popup(ctx, &mut self.state);
        render_about_popup(ctx, &mut self.state);
        render_settings_popup(ctx, &mut self.state);

        // Request repaint while a run is active to keep log streaming live.
        if self.state.run.active_run.is_some() {
            ctx.request_repaint();
        }
        // Request repaint while SIR is running to update elapsed time.
        if !self.state.workspace.sir_running.is_empty() {
            ctx.request_repaint();
        }

        // Request screenshot for tree export (result arrives next frame via Event::Screenshot).
        if self.state.ui.tree_export_pending {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            self.state.ui.tree_export_pending  = false;
            self.state.ui.tree_export_awaiting = true;
        }
    }

    /// Called once, only when the app is actually about to terminate (never
    /// while a quit is still cancellable via `show_quit_unsaved_dialog`).
    /// R helper subprocesses (VPC/SIR/Simulate/etc.) are not meant to
    /// survive the GUI closing, unlike detached fit runs — kill any still
    /// in flight so they don't linger as orphans.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        crate::io::r_extract::kill_all_helper_pids();
    }
}

// ---------------------------------------------------------------------------
// Menu bar — File / View / About. An in-window bar drawn by egui itself, not
// the OS-native macOS menu bar (that would need a separate crate wired
// through the window handle); this renders identically on macOS/Linux/
// Windows with no platform-specific code. There is deliberately no "Edit"
// menu: the only editable surface in the app is the model script editor,
// and its Cut/Copy/Paste/Undo already work via native OS shortcuts (handled
// internally by egui's `TextEdit`) — there's nothing else "Edit" would
// expose without inventing a feature that isn't there today.
// ---------------------------------------------------------------------------

fn render_menu_bar(ctx: &egui::Context, state: &mut AppState) {
    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Settings…").on_hover_text("Cmd/Ctrl+,").clicked() {
                    state.ui.settings_open = true;
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    ui.close_menu();
                }
            });

            ui.menu_button("View", |ui| {
                let theme = &mut state.workspace.settings.theme;
                let changed = ui.radio_value(theme, crate::io::persistence::Theme::Dark, "Dark").changed()
                    | ui.radio_value(theme, crate::io::persistence::Theme::Light, "Light").changed();
                if changed {
                    if let Some(w) = state.workspace.save_settings() { state.ui.status_message = w; }
                }

                ui.separator();
                let mut collapsed = state.ui.sidebar_collapsed;
                if ui.checkbox(&mut collapsed, "Collapse Sidebar").changed() {
                    set_sidebar_collapsed(state, collapsed);
                }

                ui.separator();
                for tab in Tab::ALL {
                    let label = format!("{}    Ctrl+{}", tab.label(), tab.shortcut_index());
                    if ui.selectable_label(state.ui.active_tab == *tab, label).clicked() {
                        state.ui.active_tab = *tab;
                        ui.close_menu();
                    }
                }
            });

            if ui.button("About").clicked() {
                state.ui.about_open = true;
            }
        });
    });
}

// ---------------------------------------------------------------------------
// Header bar  (44 px)
// ---------------------------------------------------------------------------

fn render_header(ctx: &egui::Context, state: &mut AppState) {
    // 32 px — just enough for context + run indicators.
    // The window title bar already shows "FeRx GUI", so we don't repeat it.
    egui::TopBottomPanel::top("header")
        .exact_height(32.0)
        .show(ctx, |ui| {
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let dim    = if ui.visuals().dark_mode { theme::FG3 } else { egui::Color32::from_gray(140) };
                ui.add_space(8.0);

                // Fe·Rx wordmark — the app identity.  (The working directory is
                // shown with its controls in the Models tab, so we don't repeat
                // it here as a breadcrumb.)
                crate::ui::icons::show_ferx_logo(ui, 15.0);

                // Version badge — small, beside the logo.
                ui.add_space(3.0);
                ui.label(
                    egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                        .color(dim)
                        .size(10.0),
                );

                // Scanning spinner.
                if state.workspace.scanning {
                    ui.add_space(6.0);
                    ui.spinner();
                }

                // Active run indicator — shown when the popup is closed so there's
                // always a visible status.  Clicking re-opens the popup.
                if let Some(run) = &state.run.active_run {
                    if !state.ui.run_popup_open {
                        ui.add_space(10.0);
                        ui.spinner();
                        let label_resp = ui.add(
                            egui::Label::new(
                                egui::RichText::new(format!("Running: {}", run.record.model_stem))
                                    .color(theme::ACCENT)
                                    .size(12.0),
                            ).sense(egui::Sense::click()),
                        ).on_hover_text("Click to open run output");
                        if label_resp.clicked() {
                            state.ui.run_popup_open = true;
                        }
                    }
                }

                // SIR running indicator — visible when the SIR popup is closed.
                if !state.workspace.sir_running.is_empty() && !state.ui.sir_popup_open {
                    let sir_stem = state.workspace.sir_running.iter().next()
                        .cloned().unwrap_or_default();
                    ui.add_space(10.0);
                    ui.spinner();
                    let sir_resp = ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!("SIR: {sir_stem}"))
                                .color(theme::ACCENT)
                                .size(12.0),
                        ).sense(egui::Sense::click()),
                    ).on_hover_text("Click to open SIR progress");
                    if sir_resp.clicked() {
                        state.ui.sir_popup_open = true;
                    }
                }

                // Right-side buttons. About/Settings now live in the menu bar
                // (File > Settings…, and a top-level About button) — kept out
                // of this row to avoid duplicate affordances for the same action.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(8.0);
                    if state.workspace.settings.rstudio_path.is_some()
                        && ui.small_button("Open RStudio").clicked()
                    {
                        if let Some(path) = &state.workspace.settings.rstudio_path {
                            if let Err(e) = open::that(path) {
                                state.ui.status_message = format!("Could not open RStudio: {e}");
                            }
                        }
                    }
                });
            });
        });
}

// ---------------------------------------------------------------------------
// Sidebar  (82 px or 48 px when collapsed)
// ---------------------------------------------------------------------------

fn render_sidebar(ctx: &egui::Context, state: &mut AppState) {
    // Give the sidebar a distinct fill so the boundary is clear without any
    // separator line — same approach macOS source lists use.
    let is_dark = state.workspace.settings.theme == crate::io::persistence::Theme::Dark;
    let sidebar_fill = if is_dark {
        theme::BG
    } else {
        egui::Color32::from_gray(242)
    };
    let width = if state.ui.sidebar_collapsed { 48.0 } else { 82.0 };

    egui::SidePanel::left("sidebar")
        .exact_width(width)
        .resizable(false)
        .show_separator_line(false)
        .frame(egui::Frame::new().fill(sidebar_fill))
        .show(ctx, |ui| {
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                for tab in Tab::ALL {
                    let active = state.ui.active_tab == *tab;
                    let dark = ui.visuals().dark_mode;

                    // Button geometry
                    let (btn_w, btn_h) = if state.ui.sidebar_collapsed {
                        (40.0_f32, 40.0_f32)
                    } else {
                        (74.0_f32, 54.0_f32)
                    };
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(btn_w, btn_h),
                        egui::Sense::click(),
                    );

                    // Colours
                    let (bg, fg) = if active {
                        (theme::ACCENT, egui::Color32::WHITE)
                    } else if response.hovered() {
                        let hbg = if dark { theme::BG3 } else { egui::Color32::from_gray(224) };
                        let hfg = if dark { theme::FG } else { ui.visuals().text_color() };
                        (hbg, hfg)
                    } else {
                        let ibg = egui::Color32::TRANSPARENT;
                        let ifg = if dark { theme::FG2 } else { ui.visuals().text_color() };
                        (ibg, ifg)
                    };

                    // Background (rounded)
                    ui.painter().rect_filled(rect, 6.0_f32, bg);

                    // Icon
                    let icon_y = if state.ui.sidebar_collapsed {
                        rect.center().y
                    } else {
                        rect.top() + btn_h * 0.37
                    };
                    crate::ui::icons::paint_tab_icon(
                        ui.painter(),
                        *tab,
                        egui::pos2(rect.center().x, icon_y),
                        9.0,
                        fg,
                    );

                    // Label (expanded only)
                    if !state.ui.sidebar_collapsed {
                        ui.painter().text(
                            egui::pos2(rect.center().x, rect.top() + btn_h * 0.76),
                            egui::Align2::CENTER_CENTER,
                            tab.label(),
                            egui::FontId::proportional(10.5),
                            fg,
                        );
                    }

                    // Interaction
                    if response.clicked() {
                        state.ui.active_tab = *tab;
                    }
                    response.on_hover_text(tab.label());
                    ui.add_space(2.0);
                }

                // Collapse toggle at the bottom.
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.add_space(4.0);
                    let dark = ui.visuals().dark_mode;
                    let toggle_fg = if dark { theme::FG3 } else { egui::Color32::from_gray(150) };
                    let icon = if state.ui.sidebar_collapsed { "▶" } else { "◀" };
                    if ui
                        .button(egui::RichText::new(icon).color(toggle_fg).size(10.0))
                        .on_hover_text(if state.ui.sidebar_collapsed { "Expand" } else { "Collapse" })
                        .clicked()
                    {
                        set_sidebar_collapsed(state, !state.ui.sidebar_collapsed);
                    }
                });
            });
        });
}

/// Sets the sidebar collapsed/expanded state and persists it — shared by the
/// sidebar's own toggle button and the View menu's "Collapse Sidebar" item.
fn set_sidebar_collapsed(state: &mut AppState, collapsed: bool) {
    state.ui.sidebar_collapsed = collapsed;
    state.workspace.settings.sidebar_collapsed = collapsed;
    if let Some(w) = state.workspace.save_settings() { state.ui.status_message = w; }
}

// ---------------------------------------------------------------------------
// Body — routes to the active tab's panel
// ---------------------------------------------------------------------------

fn render_body(ctx: &egui::Context, state: &mut AppState) {
    egui::CentralPanel::default().show(ctx, |ui| {
        match state.ui.active_tab {
            Tab::Models => crate::ui::models_tab::show(ui, state),
            Tab::Files => crate::ui::files_tab::show(ui, state),
            Tab::Tree => crate::ui::tree_tab::show(ui, state),
            Tab::Evaluation => crate::ui::eval_tab::show(ui, state),
            Tab::Vpc => crate::ui::vpc_tab::show(ui, state),
            Tab::Uncertainty => crate::ui::sir_tab::show(ui, state),
            Tab::Simulate => crate::ui::simulate_tab::show(ui, state),
            Tab::SimPlot => crate::ui::sim_tab::show(ui, state),
            Tab::History => crate::ui::history_tab::show(ui, state),
        }
    });
}

// ---------------------------------------------------------------------------
// Quit-confirmation dialog — shown when the main window is asked to close
// while the Files tab has unsaved edits (mirrors files_tab's own
// switch-file guard, so quitting can't discard work any more silently than
// switching files can).
// ---------------------------------------------------------------------------

fn show_quit_unsaved_dialog(ctx: &egui::Context, state: &mut AppState) {
    if !state.ui.quit_unsaved_dialog { return; }

    let mut cancel  = false;
    let mut discard = false;

    egui::Window::new("Quit FeRx GUI?")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            let dark = ui.visuals().dark_mode;
            ui.set_min_width(340.0);
            ui.label(egui::RichText::new("Unsaved changes").strong().size(14.0).color(theme::fg(dark)));
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("You have unsaved edits in the Files tab. Quit anyway?")
                    .color(theme::fg2(dark)).size(12.0),
            );
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() { cancel = true; }
                if ui.add(
                    egui::Button::new(egui::RichText::new("Discard && Quit").color(egui::Color32::WHITE))
                        .fill(theme::RED),
                ).clicked() { discard = true; }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) { cancel = true; }
        });

    if cancel {
        state.ui.quit_unsaved_dialog = false;
    } else if discard {
        state.ui.quit_unsaved_dialog = false;
        state.ui.quit_confirmed = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

// ---------------------------------------------------------------------------
// Settings popup (floating window, not a sidebar tab — opened via the header
// button or Cmd/Ctrl+, to match the native-app convention of Preferences
// being a separate window rather than part of the main document view).
// ---------------------------------------------------------------------------

fn render_settings_popup(ctx: &egui::Context, state: &mut AppState) {
    if !state.ui.settings_open { return; }

    let is_dark = ctx.style().visuals.dark_mode;
    let mut do_close = false;

    // Real OS viewport, matching the About/Run/SIR popups elsewhere in this
    // file — see the Wayland caveat noted on `render_run_popup`, which
    // applies here too since this uses the same mechanism.
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("settings_popup"),
        egui::ViewportBuilder::default()
            .with_title("Settings")
            .with_inner_size(egui::vec2(520.0, 560.0))
            .with_min_inner_size(egui::vec2(420.0, 360.0)),
        |ctx, _class| {
            if is_dark { theme::apply_dark(ctx); } else { theme::apply_light(ctx); }

            if ctx.input(|i| i.viewport().close_requested()) {
                do_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            // Esc also closes it, matching typical Preferences-window behaviour.
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                do_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }

            egui::CentralPanel::default().show(ctx, |ui| {
                render_settings(ui, state);
            });
        },
    );

    if do_close {
        state.ui.settings_open = false;
    }
}

fn render_settings(ui: &mut egui::Ui, state: &mut AppState) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(20.0);
        ui.heading("Settings");
        ui.add_space(20.0);

        // Constrain width so it doesn't stretch across a wide window.
        ui.set_max_width(460.0);

        // ── Working Directory ───────────────────────────────────────────────
        settings_section_label(ui, "Working Directory");
        ui.group(|ui| {
            ui.set_width(440.0);
            let dir_str = state
                .workspace
                .settings
                .working_directory
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "Not set".to_string());
            let is_set = state.workspace.settings.working_directory.is_some();
            let path_color = if is_set {
                ui.visuals().text_color()
            } else {
                if ui.visuals().dark_mode { theme::FG3 } else { egui::Color32::from_gray(160) }
            };
            ui.add(egui::Label::new(
                egui::RichText::new(&dir_str).monospace().size(12.0).color(path_color),
            ).truncate());
            ui.add_space(8.0);
            if ui.button("Choose…").clicked() {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    state.set_directory(dir);
                }
            }
        });
        ui.add_space(16.0);

        // ── FeRx Engine (via R) ──────────────────────────────────────────────
        settings_section_label(ui, "FeRx Engine (R package)");
        ui.group(|ui| {
            ui.set_width(440.0);

            // Source badge
            use crate::io::persistence::FerxBinarySource;
            let (source_text, source_color) = match &state.workspace.ferx_binary_source {
                FerxBinarySource::Detecting  => ("Detecting R + ferx package…",        theme::fg3(ui.visuals().dark_mode)),
                FerxBinarySource::RPackage   => ("✔ ferx package found — runs via R",   theme::GREEN),
                FerxBinarySource::SystemPath => ("Found on system PATH",                ui.visuals().text_color()),
                FerxBinarySource::Custom     => ("Custom Rscript path",                 theme::FG2),
                FerxBinarySource::NotFound   => ("ferx package not found via R",        theme::ORANGE),
            };
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(source_text).size(11.0).color(source_color));

                // "Re-detect" button when detection failed — lets user retry
                // without restarting the app (useful after fixing PATH/R install).
                if (state.workspace.ferx_binary_source == FerxBinarySource::NotFound
                    || state.workspace.ferx_binary_source == FerxBinarySource::Detecting)
                    && ui.small_button("Re-detect").clicked() {
                        state.workspace.ferx_binary_source = FerxBinarySource::Detecting;
                        let tx  = state.worker_tx.clone();
                        let ctx = ui.ctx().clone();
                        std::thread::spawn(move || {
                            let result = crate::io::persistence::detect_ferx_from_r();
                            let _ = tx.send(
                                crate::workers::messages::WorkerMsg::FerxBinaryDetected(result)
                            );
                            ctx.request_repaint();
                        });
                    }
            });

            // ferx package version, when known.
            if let Some(ver) = &state.workspace.ferx_version {
                ui.label(
                    egui::RichText::new(format!("ferx package v{ver}"))
                        .size(11.0)
                        .color(theme::fg2(ui.visuals().dark_mode)),
                );
            }
            ui.add_space(4.0);

            // Path display
            let found = state.workspace.settings.ferx_binary.is_some();
            let bin_str = state.workspace.settings.ferx_binary
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "—".to_string());
            let bin_color = if found {
                if ui.visuals().dark_mode { theme::FG2 } else { egui::Color32::from_gray(80) }
            } else {
                egui::Color32::TRANSPARENT  // nothing to show
            };
            if found {
                ui.add(egui::Label::new(
                    egui::RichText::new(&bin_str).monospace().size(11.0).color(bin_color),
                ).truncate());
                ui.add_space(6.0);
            }

            // Hint when ferx isn't available.
            if state.workspace.ferx_binary_source == FerxBinarySource::NotFound {
                ui.label(
                    egui::RichText::new(
                        "Install R and the ferx package:\n\
                         devtools::install_github(\"FeRx-NLME/ferx-r\")\n\
                         then click Re-detect. Or browse to your Rscript manually.",
                    )
                    .color(theme::fg3(ui.visuals().dark_mode))
                    .size(11.0),
                );
                ui.add_space(6.0);
            }

            // Browse (to Rscript) + optional Reset
            ui.horizontal(|ui| {
                if ui.button("Browse to Rscript…").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_file() {
                        state.workspace.settings.ferx_binary = Some(path);
                        state.workspace.settings.ferx_binary_custom = true;
                        state.workspace.ferx_binary_source = FerxBinarySource::Custom;
                        if let Some(w) = state.workspace.save_settings() { state.ui.status_message = w; }
                    }
                }

                if state.workspace.ferx_binary_source == FerxBinarySource::Custom {
                    let reset = ui.add(
                        egui::Button::new(
                            egui::RichText::new("Reset to auto-detect").size(12.0)
                        )
                    ).on_hover_text("Clear the custom path and re-run auto-detection");
                    if reset.clicked() {
                        state.workspace.settings.ferx_binary_custom = false;
                        state.workspace.settings.ferx_binary = None;
                        state.workspace.ferx_binary_source = FerxBinarySource::Detecting;
                        if let Some(w) = state.workspace.save_settings() { state.ui.status_message = w; }
                        // Kick off background R detection
                        let tx  = state.worker_tx.clone();
                        let ctx = ui.ctx().clone();
                        std::thread::spawn(move || {
                            let result = crate::io::persistence::detect_ferx_from_r();
                            let _ = tx.send(
                                crate::workers::messages::WorkerMsg::FerxBinaryDetected(result)
                            );
                            ctx.request_repaint();
                        });
                    }
                }
            });
        });
        ui.add_space(16.0);

        // ── Appearance ──────────────────────────────────────────────────────
        settings_section_label(ui, "Appearance");
        ui.group(|ui| {
            ui.set_width(440.0);
            ui.label("Theme");
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let is_dark = state.workspace.settings.theme == crate::io::persistence::Theme::Dark;
                if ui.selectable_label(is_dark, "  Dark  ").clicked() {
                    state.workspace.settings.theme = crate::io::persistence::Theme::Dark;
                    if let Some(w) = state.workspace.save_settings() { state.ui.status_message = w; }
                }
                if ui.selectable_label(!is_dark, "  Light  ").clicked() {
                    state.workspace.settings.theme = crate::io::persistence::Theme::Light;
                    if let Some(w) = state.workspace.save_settings() { state.ui.status_message = w; }
                }
            });
        });
    });
}

fn settings_section_label(ui: &mut egui::Ui, title: &str) {
    let color = if ui.visuals().dark_mode { theme::FG2 } else { egui::Color32::from_gray(80) };
    ui.label(egui::RichText::new(title).strong().size(11.0).color(color));
    ui.add_space(4.0);
}

// ---------------------------------------------------------------------------
// Floating run-output popup
// ---------------------------------------------------------------------------

fn render_run_popup(ctx: &egui::Context, state: &mut AppState) {
    use crate::workers::messages::CancelMode;

    // Auto-open when a new run starts (unique run ID, so re-running the same model works).
    if let Some(run) = &state.run.active_run {
        let run_id = run.record.id.clone();
        if state.ui.run_popup_last_run_id.as_deref() != Some(run_id.as_str()) {
            state.ui.run_popup_open = true;
            state.ui.run_popup_last_run_id = Some(run_id);
            // If a popup from a previous run was already open (just not in the
            // foreground), setting `run_popup_open` above is a no-op — the OS
            // window is already alive, so its content updates but nothing
            // raises it, leaving the user unsure whether the new run actually
            // started. Explicitly bring it to front for every new run.
            ctx.send_viewport_cmd_to(
                egui::ViewportId::from_hash_of("run_popup"),
                egui::ViewportCommand::Focus,
            );
        }
    }

    if !state.ui.run_popup_open { return; }

    // Pre-compute everything read from `state` so the closure below can be
    // FnMut without holding a borrow on state across the call.
    let is_dark     = state.workspace.settings.theme == crate::io::persistence::Theme::Dark;
    let dim_fg      = if is_dark { theme::FG2 } else { egui::Color32::from_gray(100) };
    let log_fg      = if is_dark { theme::FG2 } else { egui::Color32::from_gray(50) };
    let (dot_color, stem, elapsed, status_text) = run_panel_status(state);
    let has_active  = state.run.active_run.is_some();
    let queue_len   = state.run.run_queue.len();
    let log_text    = state.run.log_text.clone();
    let log_path    = state.run.active_run.as_ref()
        .map(|r| r.log_path.to_string_lossy().to_string())
        .unwrap_or_default();
    let active_stem = state.run.active_run.as_ref()
        .map(|r| r.record.model_stem.clone())
        .unwrap_or_default();

    let title = if has_active {
        format!("Running: {stem}")
    } else {
        format!("Run: {stem}")
    };

    // Action flags written inside the closure, applied after.
    let mut do_close  = false;
    let mut do_detach = false;
    let mut do_stop   = false;
    let mut do_kill   = false;

    // Use a real OS viewport so the close button matches the host OS (native
    // red circle on macOS, standard × on Windows / Linux).
    // Note: show_viewport_immediate spawns a child OS window, which requires a
    // display server that supports multiple windows. On Wayland without XWayland
    // this may silently no-op; users running native Wayland should set
    // WINIT_UNIX_BACKEND=x11 or enable XWayland as a workaround.
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("run_popup"),
        egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size(egui::vec2(520.0, 280.0))
            .with_min_inner_size(egui::vec2(300.0, 120.0)),
        |ctx, _class| {
            // Apply theme so the popup matches the main window.
            if is_dark { theme::apply_dark(ctx); } else { theme::apply_light(ctx); }

            // Native close button → honour it.
            if ctx.input(|i| i.viewport().close_requested()) {
                do_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                do_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }

            egui::CentralPanel::default().show(ctx, |ui| {
                // ── Status row ─────────────────────────────────────────
                ui.horizontal(|ui| {
                    let (dot_rect, _) = ui.allocate_exact_size(
                        egui::vec2(10.0, 10.0), egui::Sense::hover(),
                    );
                    ui.painter().circle_filled(dot_rect.center(), 4.5, dot_color);
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(&status_text).size(11.0).color(dim_fg));
                    if !elapsed.is_empty() {
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(&elapsed).size(11.0).color(dim_fg).monospace());
                    }
                    if queue_len > 0 {
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(format!("↓ {queue_len} queued")).size(11.0).color(dim_fg));
                    }
                    if has_active {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(
                                egui::Button::new(egui::RichText::new("Kill").size(11.0).color(theme::RED))
                                    .stroke(egui::Stroke::new(1.0, theme::RED))
                                    .fill(egui::Color32::TRANSPARENT)
                                    .min_size(egui::vec2(38.0, 18.0)),
                            ).on_hover_text(if cfg!(unix) { "Terminate immediately (SIGKILL)" } else { "Terminate immediately (force kill)" }).clicked() {
                                do_kill = true;
                            }
                            ui.add_space(4.0);
                            if ui.add(
                                egui::Button::new(egui::RichText::new("Stop").size(11.0))
                                    .min_size(egui::vec2(42.0, 18.0)),
                            ).on_hover_text(if cfg!(unix) { "Request graceful stop (SIGTERM)" } else { "Request graceful stop (CTRL_BREAK → kill after 5 s)" }).clicked() {
                                do_stop = true;
                            }
                        });
                    }
                });

                // ── Log path + Detach (active run only) ───────────────
                if has_active && !log_path.is_empty() {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Log:").color(dim_fg).size(10.0));
                        ui.add(egui::Label::new(
                            egui::RichText::new(&log_path).monospace().size(10.0).color(dim_fg)
                        ).truncate());
                        if ui.small_button("Copy").on_hover_text("Copy log path to clipboard").clicked() {
                            ui.ctx().copy_text(log_path.clone());
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("✔ Detached — run continues if GUI closes")
                                .color(theme::GREEN).size(10.0),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(
                                egui::Button::new(egui::RichText::new("Detach").size(11.0))
                                    .min_size(egui::vec2(52.0, 18.0)),
                            ).on_hover_text(
                                "Uncouple this run from the GUI.\n\
                                 The model continues running; the popup closes.\n\
                                 Monitor progress with:  tail -f <log path>\n\
                                 Restart the GUI to reconnect when done."
                            ).clicked() {
                                do_detach = true;
                                do_close  = true;
                            }
                        });
                    });
                }

                ui.separator();

                // ── Log scroll ─────────────────────────────────────────
                egui::ScrollArea::vertical()
                    .id_salt("run_popup_log")
                    .stick_to_bottom(true)
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        if log_text.is_empty() {
                            let hint = if is_dark { theme::FG3 } else { egui::Color32::from_gray(160) };
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("Run output will appear here").color(hint).size(12.0));
                        } else {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&log_text)
                                        .font(egui::FontId::monospace(11.0))
                                        .color(log_fg),
                                ).wrap(),
                            );
                        }
                    });
            });
        },
    );

    // Apply actions now that the viewport closure has returned.
    if do_kill {
        if let Some(r) = &state.run.active_run {
            let _ = r.cancel_tx.send(CancelMode::Kill);
        }
    }
    if do_stop {
        if let Some(r) = &state.run.active_run {
            let _ = r.cancel_tx.send(CancelMode::Graceful);
        }
    }
    if do_detach {
        state.run.active_run = None;
        state.ui.status_message = format!("Detached — {active_stem} continues in background");
    }
    if do_close {
        state.ui.run_popup_open = false;
    }
}

fn render_sir_popup(ctx: &egui::Context, state: &mut AppState) {
    // Auto-open when a new SIR run starts for a stem we haven't seen yet.
    let current_stem = state.workspace.sir_running.iter().next().cloned()
        .or_else(|| state.ui.sir_popup_last_stem.clone());
    if let Some(ref stem) = state.workspace.sir_running.iter().next().cloned() {
        if state.ui.sir_popup_last_stem.as_deref() != Some(stem.as_str()) {
            state.ui.sir_popup_open      = true;
            state.ui.sir_popup_last_stem = Some(stem.clone());
            // Same reasoning as the Run popup: bring an already-open window
            // to front for every new SIR run, not just the first one.
            ctx.send_viewport_cmd_to(
                egui::ViewportId::from_hash_of("sir_popup"),
                egui::ViewportCommand::Focus,
            );
        }
    }

    if !state.ui.sir_popup_open { return; }

    // Nothing to show if we have no stem yet.
    let stem = match &current_stem {
        Some(s) => s.clone(),
        None    => return,
    };

    let is_dark     = state.workspace.settings.theme == crate::io::persistence::Theme::Dark;
    let is_running  = state.workspace.sir_running.contains(&stem);
    let result      = state.workspace.sir_results.get(&stem).cloned();
    let elapsed_sec = state.workspace.sir_started_at.get(&stem)
        .map(|t| t.elapsed().as_secs());

    // Gather display values before the closure borrows state.
    let n_samples   = state.ui.sir_n_samples;
    let n_resamples = state.ui.sir_n_resamples;
    let seed        = state.ui.sir_seed;

    let (ess, ess_pct, low_ess) = if let Some(ref r) = result {
        let pct = if n_resamples > 0 { r.sir_ess / n_resamples as f64 * 100.0 } else { 0.0 };
        (Some(r.sir_ess), pct, pct < 20.0)
    } else {
        (None, 0.0, false)
    };

    let title = if is_running {
        format!("SIR running — {stem}")
    } else {
        format!("SIR complete — {stem}")
    };

    let mut do_close    = false;
    let mut go_to_sir   = false;

    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("sir_popup"),
        egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size(egui::vec2(420.0, 200.0))
            .with_min_inner_size(egui::vec2(300.0, 140.0)),
        |ctx, _class| {
            if is_dark { theme::apply_dark(ctx); } else { theme::apply_light(ctx); }

            if ctx.input(|i| i.viewport().close_requested()) {
                do_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                do_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }

            let dim = if is_dark { theme::FG2 } else { egui::Color32::from_gray(100) };

            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_space(8.0);

                if is_running {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(18.0));
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new("SIR running…").size(13.0));
                        if let Some(secs) = elapsed_sec {
                            let m = secs / 60;
                            let s = secs % 60;
                            let elapsed = if m > 0 { format!("{m}m {s:02}s") } else { format!("{s}s") };
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new(elapsed).size(12.0).color(dim).monospace());
                        }
                    });
                } else {
                    let (icon, col) = if low_ess {
                        ("⚠  SIR complete — low ESS", theme::ORANGE)
                    } else {
                        ("✔  SIR complete", theme::GREEN)
                    };
                    ui.label(egui::RichText::new(icon).color(col).size(13.0).strong());
                    if let Some(ess_v) = ess {
                        ui.label(
                            egui::RichText::new(format!(
                                "Effective sample size: {ess_v:.1} / {n_resamples}  ({ess_pct:.0}%)"
                            ))
                            .size(12.0)
                            .color(if low_ess { theme::ORANGE } else { dim }),
                        );
                        if low_ess {
                            ui.label(
                                egui::RichText::new(
                                    "Consider increasing Samples on the SIR tab and re-running.",
                                )
                                .size(11.0)
                                .color(theme::ORANGE),
                            );
                        }
                    }
                }

                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{n_samples} samples · {n_resamples} resamples · seed {seed}"
                    ))
                    .size(10.0)
                    .color(dim),
                );

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.add(
                        egui::Button::new(egui::RichText::new("Go to SIR tab").size(12.0))
                            .fill(theme::ACCENT)
                            .min_size(egui::vec2(110.0, 26.0)),
                    ).clicked() {
                        go_to_sir = true;
                        do_close  = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    ui.add_space(8.0);
                    if ui.add(
                        egui::Button::new(egui::RichText::new("Close").size(12.0))
                            .min_size(egui::vec2(70.0, 26.0)),
                    ).clicked() {
                        do_close = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
        },
    );

    if go_to_sir {
        state.ui.active_tab = crate::state::Tab::Uncertainty;
    }
    if do_close {
        state.ui.sir_popup_open = false;
        state.ui.sir_popup_last_stem = None;
    }
}

fn render_about_popup(ctx: &egui::Context, state: &mut AppState) {
    if !state.ui.about_open { return; }

    let is_dark = ctx.style().visuals.dark_mode;
    let mut do_close = false;

    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("about_popup"),
        egui::ViewportBuilder::default()
            .with_title("About FeRx GUI")
            .with_inner_size(egui::vec2(460.0, 490.0))
            .with_resizable(true)
            .with_min_inner_size(egui::vec2(380.0, 420.0)),
        |ctx, _class| {
            if is_dark { theme::apply_dark(ctx); } else { theme::apply_light(ctx); }

            if ctx.input(|i| i.viewport().close_requested()) {
                do_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                do_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }

            let dim = if is_dark { egui::Color32::from_gray(140) } else { egui::Color32::from_gray(110) };

            egui::CentralPanel::default().show(ctx, |ui| {
                // ScrollArea ensures nothing is cut off regardless of DPI or font size.
                egui::ScrollArea::vertical()
                    .id_salt("about_scroll")
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                    ui.add_space(14.0);

                    // ── Logo + title ──────────────────────────────────────
                    // `show_ferx_logo` already renders the "FeRx"+"GUI"
                    // wordmark next to the curve icon — it just needs a
                    // horizontal layout to flow correctly (its caller in the
                    // header already provides one; `vertical_centered` here
                    // does not, which previously stacked "FeRx"/"GUI"
                    // vertically). A second, separate "FeRx GUI" label
                    // used to follow it — redundant, and additionally
                    // rendered in `Color32::WHITE` (`.strong()` resolves to
                    // `visuals.widgets.active.text_color()`, which this
                    // theme sets to white for text on accent-filled
                    // buttons — invisible on this popup's plain light-mode
                    // background). Removed rather than recoloured.
                    ui.vertical_centered(|ui| {
                        ui.horizontal(|ui| {
                            crate::ui::icons::show_ferx_logo(ui, 36.0);
                        });
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(
                            format!("v{}  ·  Population PK/PD modelling",
                                    env!("CARGO_PKG_VERSION")))
                            .size(12.0).color(dim));
                        ui.add_space(3.0);
                        ui.label(egui::RichText::new("Made by Rob ter Heine")
                            .size(12.0).color(dim));
                    });

                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(8.0);

                    // ── System info ───────────────────────────────────────
                    egui::Grid::new("about_sys")
                        .num_columns(2)
                        .spacing([16.0, 5.0])
                        .show(ui, |ui| {
                            let r_ver = state.workspace.r_version
                                .as_deref().unwrap_or("not detected");
                            let ferx_ver = state.workspace.ferx_version
                                .as_deref().unwrap_or("not detected");
                            let vpc_ver = match &state.ui.vpc_pkg_status {
                                Some(Ok(v)) => format!("v{v}"),
                                Some(Err(_)) => "not installed".to_string(),
                                None => "checking…".to_string(),
                            };
                            for (label, value) in [
                                ("R",            r_ver),
                                ("ferx package", ferx_ver),
                                ("vpc package",  vpc_ver.as_str()),
                            ] {
                                ui.label(egui::RichText::new(label).size(11.0).color(dim));
                                ui.label(egui::RichText::new(value).size(11.0).monospace());
                                ui.end_row();
                            }
                        });

                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(10.0);

                    // ── Links ─────────────────────────────────────────────
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("Documentation & resources")
                            .size(11.0).color(dim));
                        ui.add_space(8.0);
                        ui.hyperlink_to(
                            "github.com/Robterheine/ferxgui  —  FeRx GUI source code",
                            "https://github.com/Robterheine/ferxgui",
                        );
                        ui.add_space(6.0);
                        ui.hyperlink_to(
                            "ferx-nlme.github.io  —  FeRx NLME documentation",
                            "https://ferx-nlme.github.io/",
                        );
                        ui.add_space(6.0);
                        ui.hyperlink_to(
                            "vpc.ronkeizer.com  —  vpc R package documentation",
                            "https://vpc.ronkeizer.com/",
                        );
                    });

                    ui.add_space(14.0);
                    ui.separator();

                    // ── Footer ────────────────────────────────────────────
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("MIT licence").size(10.5).color(dim));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Close").clicked() {
                                do_close = true;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        });
                    });
                });
            });
        },
    );

    if do_close { state.ui.about_open = false; }
}

/// Returns (dot_color, model_stem, elapsed_str, status_label) for the panel header.
fn run_panel_status(state: &AppState) -> (egui::Color32, String, String, String) {
    if let Some(run) = &state.run.active_run {
        let secs = run.started_at.elapsed().as_secs();
        let elapsed = if secs < 60 {
            format!("{:02}s", secs)
        } else {
            format!("{}:{:02}", secs / 60, secs % 60)
        };
        return (theme::ORANGE, run.record.model_stem.clone(), elapsed, "Running".into());
    }
    if let Some(last) = state.run.run_history.last() {
        let dot = match last.status {
            crate::domain::JobStatus::Completed => theme::GREEN,
            crate::domain::JobStatus::Failed    => theme::RED,
            _                                   => theme::FG3,
        };
        let elapsed = last.duration_secs
            .map(|d| {
                let s = d as u64;
                if s < 60 { format!("{:02}s", s) } else { format!("{}:{:02}", s / 60, s % 60) }
            })
            .unwrap_or_default();
        return (dot, last.model_stem.clone(), elapsed, last.status.label().into());
    }
    let dim = egui::Color32::from_gray(if state.workspace.settings.theme
        == crate::io::persistence::Theme::Dark { 80 } else { 185 });
    (dim, "No recent run".into(), String::new(), String::new())
}

// ---------------------------------------------------------------------------
// Status bar  (22 px)
// ---------------------------------------------------------------------------

fn render_status_bar(ctx: &egui::Context, state: &AppState) {
    egui::TopBottomPanel::bottom("status_bar")
        .exact_height(22.0)
        .show(ctx, |ui| {
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                if !state.ui.status_message.is_empty() {
                    let msg_fg = if ui.visuals().dark_mode { theme::FG2 } else { egui::Color32::from_gray(100) };
                    ui.label(
                        egui::RichText::new(&state.ui.status_message)
                            .color(msg_fg)
                            .size(11.0),
                    );
                }
            });
        });
}

// ---------------------------------------------------------------------------
// Startup: reconnect orphaned runs
// ---------------------------------------------------------------------------

/// Called once at startup.  Scans `~/.ferxgui/running/` for any run manifests
/// left over from a previous session.  If the PID is still alive, starts a
/// monitor/tailer thread to reconnect to it so the run panel shows its output.
fn reconnect_orphaned_runs(state: &mut AppState) {
    use crate::workers::messages::CancelMode;
    use crate::workers::run::reconnect_orphan;
    use crate::domain::{ActiveRun, JobStatus, RunRecord};
    use std::collections::HashMap;

    let app_dir = match &state.workspace.app_dir {
        Some(d) => d.clone(),
        None => return,
    };

    let manifests = scan_manifests(&app_dir);
    if manifests.is_empty() { return; }

    // We can only track one active run in the current UI.  Pick the most
    // recently modified manifest if there are several.
    let Some((mfst_path, manifest)) = manifests
        .into_iter()
        .max_by_key(|(p, _)| {
            p.metadata().and_then(|m| m.modified()).ok()
        })
    else { return };

    if !RunManifest::is_pid_alive(manifest.pid) {
        // Process already gone — remove stale manifest.
        RunManifest::remove(&mfst_path);
        return;
    }

    // Reconstruct a minimal RunRecord (full details not in manifest).
    let record = RunRecord {
        id:            manifest.run_id.clone(),
        model_stem:    manifest.model_stem.clone(),
        tool:          "ferx".to_string(),
        method:        None,
        status:        JobStatus::Running,
        started:       String::new(),
        completed:     None,
        duration_secs: None,
        command:       manifest.command.clone(),
        directory:     manifest.directory.clone(),
        data_path:     None,
        file_hashes:   HashMap::new(),
    };

    let (cancel_tx, cancel_rx) = std::sync::mpsc::channel::<CancelMode>();
    let tx = state.worker_tx.clone();

    reconnect_orphan(
        manifest.clone(),
        mfst_path.clone(),
        record.clone(),
        tx,
        cancel_rx,
    );

    state.run.active_run = Some(ActiveRun {
        record,
        started_at:    std::time::Instant::now(), // approximate
        log_path:      manifest.log_path,
        cancel_tx,
        export_tables:  false, // not known for reconnected runs; user can re-run if needed
        run_sir_after:  false,
    });
    state.ui.run_popup_open = true;
    state.ui.run_popup_last_run_id = Some(manifest.run_id.clone());
    state.ui.status_message = format!("Reconnected to running: {}", manifest.model_stem);
}

// ---------------------------------------------------------------------------
// Tree PNG export helper
// ---------------------------------------------------------------------------

fn save_tree_png(
    screenshot: &egui::ColorImage,
    canvas_rect: egui::Rect,
    ppp: f32,
    state: &mut AppState,
) {
    // Convert logical rect → physical pixels, clamped to image bounds.
    let img_w = screenshot.width() as u32;
    let img_h = screenshot.height() as u32;

    let x0 = ((canvas_rect.min.x * ppp).round() as u32).min(img_w);
    let y0 = ((canvas_rect.min.y * ppp).round() as u32).min(img_h);
    let x1 = ((canvas_rect.max.x * ppp).round() as u32).min(img_w);
    let y1 = ((canvas_rect.max.y * ppp).round() as u32).min(img_h);
    let cw = x1.saturating_sub(x0);
    let ch = y1.saturating_sub(y0);
    if cw == 0 || ch == 0 { return; }

    // Build cropped RGBA byte vec.
    let mut rgba: Vec<u8> = Vec::with_capacity((cw * ch * 4) as usize);
    for py in y0..y1 {
        for px in x0..x1 {
            let c = screenshot.pixels[(py as usize) * screenshot.width() + px as usize];
            rgba.extend_from_slice(&[c.r(), c.g(), c.b(), c.a()]);
        }
    }

    // Build output path: working_dir / tree_{unix}.png
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dir = state.workspace.directory.clone()
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let path = dir.join(format!("tree_{ts}.png"));

    match image::RgbaImage::from_raw(cw, ch, rgba) {
        Some(img) => {
            if let Err(e) = img.save(&path) {
                state.ui.status_message = format!("Tree export failed: {e}");
            } else if let Err(e) = open::that(&path) {
                state.ui.status_message = format!("Tree saved to {} (could not open: {e})", path.display());
            } else {
                state.ui.status_message = format!("Tree exported → {}", path.display());
            }
        }
        None => state.ui.status_message = "Tree export failed: could not build image".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Lazy R model inspection
// ---------------------------------------------------------------------------

/// Called every frame.  Kicks off `ferx_model_inspect()` in the background —
/// once per stem — but only while the Info pill is actually being viewed, so we
/// don't pay R's startup cost on every model click.
fn trigger_r_inspect(state: &mut AppState, ctx: &egui::Context) {
    use crate::io::persistence::FerxBinarySource;
    use crate::state::{ModelPill, Tab};

    // Only when the Info pill is on screen.
    if state.ui.active_tab != Tab::Models || state.ui.active_model_pill != ModelPill::Info {
        return;
    }

    // Only proceed when R is known to be available.
    if matches!(
        state.workspace.ferx_binary_source,
        FerxBinarySource::NotFound | FerxBinarySource::Detecting
    ) {
        return;
    }

    let stem = match state.ui.selected_model
        .and_then(|i| state.workspace.models.get(i))
        .map(|e| e.model.stem.clone())
    {
        Some(s) => s,
        None => return,
    };

    // Skip if already done, in flight, or previously failed.
    if state.workspace.r_model_infos.contains_key(&stem)
        || state.workspace.r_inspecting.contains(&stem)
        || state.workspace.r_inspect_failed.contains(&stem)
    {
        return;
    }

    let path = match state.ui.selected_model
        .and_then(|i| state.workspace.models.get(i))
        .map(|e| e.model.path.clone())
    {
        Some(p) => p,
        None => return,
    };

    state.workspace.r_inspecting.insert(stem.clone());
    let tx  = state.worker_tx.clone();
    let ctx = ctx.clone();

    std::thread::spawn(move || {
        match crate::io::r_extract::inspect_model(&path) {
            Ok(info) => {
                let _ = tx.send(crate::workers::messages::WorkerMsg::RInspectComplete {
                    stem,
                    info: Box::new(info),
                });
            }
            Err(e) => {
                let _ = tx.send(crate::workers::messages::WorkerMsg::RTaskError {
                    context: format!("inspect {stem}"),
                    message: e,
                });
            }
        }
        ctx.request_repaint();
    });
}
