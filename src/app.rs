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
    /// Frames rendered so far; popups wait until the font atlas has been warmed.
    frames: u32,
    /// Keeps the GPU font texture in step with egui's font atlas (see `FontSync`).
    font_sync: FontSync,
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
        // Auto-scan if a working directory was persisted — but only if it
        // still exists on disk. A moved/deleted project folder previously
        // left the user with a silently empty model list and no
        // explanation; surface it as a startup warning instead. The setting
        // itself is left alone (not cleared) — the folder may be back next
        // launch (e.g. a temporarily unmounted drive).
        if let Some(dir) = state.workspace.directory.clone() {
            if dir.exists() {
                state.trigger_scan();
            } else {
                state.workspace.startup_warnings.push(format!(
                    "Warning: the last project folder could not be found — {} — pick one via the project menu.",
                    dir.display()
                ));
            }
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
            crate::util::spawn_guarded("app:232", tx.clone(), move || {
                let result = crate::io::persistence::detect_ferx_from_r();
                let _ = tx.send(crate::workers::messages::WorkerMsg::FerxBinaryDetected(result));
                ctx.request_repaint();
            });
        }

        Self { state, frames: 0, font_sync: FontSync::new() }
    }
}

/// How often the whole font atlas is re-uploaded as a safety net.
const FONT_RESYNC_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// Self-healing for a GPU font texture that has drifted out of step with egui's own atlas.
///
/// egui hands each font-atlas upload out exactly once. If a backend drops one (eframe 0.31's
/// glow paths with several windows can; see egui PR #8250), the GPU texture stays behind and
/// *all* text turns into squashed, scrambled glyphs until the atlas is next rebuilt. Re-sending
/// the complete atlas replaces the GPU texture at the right size and heals it. This does that:
/// for a few frames after the atlas size or the set of open popups changes (the moments an
/// upload can go missing), and every `FONT_RESYNC_INTERVAL` otherwise. It runs from the main
/// window's own pass, where the upload is applied with its paint.
struct FontSync {
    last_size: [usize; 2],
    last_popups: [bool; 4],
    last_upload: std::time::Instant,
    pending: u8,
}

impl FontSync {
    fn new() -> Self {
        Self {
            last_size: [0, 0],
            last_popups: [false; 4],
            last_upload: std::time::Instant::now(),
            pending: 0,
        }
    }

    /// Whether a full re-upload is due this frame.
    fn due(&mut self, atlas_size: [usize; 2], popups: [bool; 4], now: std::time::Instant) -> bool {
        if atlas_size != self.last_size {
            self.last_size = atlas_size;
            self.pending = 3;
        }
        if popups != self.last_popups {
            self.last_popups = popups;
            self.pending = 3;
        }
        let due = self.pending > 0 || now.duration_since(self.last_upload) >= FONT_RESYNC_INTERVAL;
        self.pending = self.pending.saturating_sub(1);
        if due { self.last_upload = now; }
        due
    }

    /// Queues a complete font-atlas upload for this pass, if one is due.
    fn resync(&mut self, ctx: &egui::Context, popups: [bool; 4]) {
        let size = ctx.fonts(|f| f.font_image_size());
        if self.due(size, popups, std::time::Instant::now()) {
            upload_full_font_atlas(ctx);
        }
    }
}

/// Queues a full (not partial) upload of egui's current font atlas for this pass. A full
/// delta recreates the GPU texture at the atlas's size, so any earlier drift is gone.
fn upload_full_font_atlas(ctx: &egui::Context) {
    let image = ctx.fonts(|f| f.image());
    let delta = egui::epaint::ImageDelta::full(image, egui::TextureOptions::LINEAR);
    ctx.tex_manager().write().set(egui::TextureId::default(), delta);
}

/// Rasterises every glyph the popup windows draw, at every font size they draw it,
/// from the main window's own pass.
///
/// egui only pre-rasterises printable ASCII for the five `TextStyle` fonts. Any other
/// (family, size) is first rasterised on demand, and for the popups that happens inside
/// their nested viewport pass. Glyphs added there were not reliably reaching the GPU font
/// texture on eframe 0.31 (glow), so e.g. the Run popup's monospace 10/11 pt and 10 pt
/// text rendered as scrambled glyph fragments while 11/13 pt proportional text (which
/// *is* pre-rasterised) stayed fine. Warming them here moves that work out of the nested
/// pass. A cached layout is a hash lookup, so calling this every frame is cheap, and it
/// re-warms automatically whenever egui rebuilds its fonts (e.g. a DPI change).
fn warm_popup_glyphs(ctx: &egui::Context) {
    use egui::FontId;
    // Printable ASCII, plus every non-ASCII character used in the popups' own labels.
    const GLYPHS: &str = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`\
        abcdefghijklmnopqrstuvwxyz{|}~°·×—…→↓⚠✔✖";
    // Sizes used by render_{run,sir,about,settings}_popup; fractional sizes included.
    const PROPORTIONAL: [f32; 6] = [10.0, 10.5, 11.0, 12.0, 13.0, 18.0];
    const MONOSPACE: [f32; 3] = [10.0, 11.0, 12.0];
    ctx.fonts(|fonts| {
        let color = egui::Color32::WHITE;
        for size in PROPORTIONAL {
            fonts.layout_no_wrap(GLYPHS.to_owned(), FontId::proportional(size), color);
        }
        for size in MONOSPACE {
            fonts.layout_no_wrap(GLYPHS.to_owned(), FontId::monospace(size), color);
        }
    });
}

impl eframe::App for FerxApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        warm_popup_glyphs(ctx);
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
            if self.state.ui.has_unsaved_edits() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.state.ui.quit_unsaved_dialog = true;
            }
        }
        show_quit_unsaved_dialog(ctx, &mut self.state);

        write_drafts(&mut self.state);

        // Drain worker messages first.
        self.state.process_worker_messages();
        // Auto-advance the sequential run queue if no run is active.
        crate::ui::models_tab::advance_queue(&mut self.state);
        // A run waiting on its pre-run validation completes off-thread; keep polling.
        if !self.state.run.pending_validate.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }
        // Lazily trigger R model inspection for the currently selected model.
        trigger_r_inspect(&mut self.state, ctx);
        // Fire the vpc package version check once at startup so the result is
        // available everywhere (About popup, VPC tab banner) without requiring
        // the user to visit the VPC tab first.
        if self.state.ui.vpc_pkg_status.is_none() && !self.state.ui.vpc_pkg_checking {
            self.state.ui.vpc_pkg_checking = true;
            let tx  = self.state.worker_tx.clone();
            let ctx2 = ctx.clone();
            crate::util::spawn_guarded("app:388", tx.clone(), move || {
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
        // Popups are separate OS windows whose passes run nested inside this one.
        // Hold them back for the first frame so `warm_popup_glyphs` has been
        // uploaded by this window's own pass before any popup can exist.
        if self.frames > 0 {
            render_run_popup(ctx, &mut self.state);
            render_sir_popup(ctx, &mut self.state);
            render_about_popup(ctx, &mut self.state);
            render_settings_popup(ctx, &mut self.state);
        }
        self.frames = self.frames.saturating_add(1);

        // Last thing in the pass, after every nested popup pass has finished, so this
        // main-window pass is the one that carries the upload to the GPU.
        let popups = [
            self.state.ui.run_popup_open,
            self.state.ui.sir_popup_open,
            self.state.ui.about_open,
            self.state.ui.settings_open,
        ];
        self.font_sync.resync(ctx, popups);

        // Request repaint while a run is active to keep log streaming live.
        if !self.state.run.active_runs.is_empty() {
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
                // always a visible status.  Clicking re-opens the popup (which
                // follows whichever run started most recently — see `display_run`).
                if !state.run.active_runs.is_empty() && !state.ui.run_popup_open {
                    ui.add_space(10.0);
                    ui.spinner();
                    let label_text = match state.run.active_runs.len() {
                        1 => format!(
                            "Running: {}",
                            state.run.active_runs.values().next().unwrap().record.model_stem
                        ),
                        n => format!("{n} runs active"),
                    };
                    let label_resp = ui.add(
                        egui::Label::new(
                            egui::RichText::new(label_text)
                                .color(theme::ACCENT)
                                .size(12.0),
                        ).sense(egui::Sense::click()),
                    ).on_hover_text("Click to open run output");
                    if label_resp.clicked() {
                        state.ui.run_popup_open = true;
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
                egui::RichText::new("You have unsaved edits (Files tab or model editor). Quit anyway?")
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
                        crate::util::spawn_guarded("app:923", tx.clone(), move || {
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
                        crate::util::spawn_guarded("app:1000", tx.clone(), move || {
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

        // ── Concurrent Runs ──────────────────────────────────────────────────
        settings_section_label(ui, "Concurrent Runs");
        ui.group(|ui| {
            ui.set_width(440.0);
            ui.horizontal(|ui| {
                ui.label("Max concurrent fits:");
                let mut max_concurrent = state.workspace.settings.max_concurrent_runs;
                if ui.add(egui::DragValue::new(&mut max_concurrent).range(1..=16)).changed() {
                    state.workspace.settings.max_concurrent_runs = max_concurrent;
                    if let Some(w) = state.workspace.save_settings() { state.ui.status_message = w; }
                }
            });
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(
                    "How many ferx fits may run at the same time. Default 1 matches earlier \
                     behaviour — raise it to fit several models in parallel."
                )
                .color(theme::fg3(ui.visuals().dark_mode))
                .size(11.0),
            );

            // Oversubscription warning — display only, never overrides the
            // user's setting. `run_threads` is the Run tab's current thread
            // setting (0 = auto, passed to ferx as NULL — ferx-r 0.3.0 documents
            // the actual default as cores − 1, capped at 8, not "every core").
            let cores = std::thread::available_parallelism().map(std::num::NonZeroUsize::get).unwrap_or(1);
            if let Some(warning) = oversubscription_warning(
                state.workspace.settings.max_concurrent_runs,
                state.ui.run_threads,
                cores,
            ) {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!("⚠ {warning}"))
                        .color(theme::ORANGE)
                        .size(11.0),
                );
            }
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

/// Plain-language warning when `max_concurrent` fits at `threads` each would
/// ask for more logical cores than the machine has — each fit competing for
/// the same cores tends to run *slower* than running them one at a time, not
/// faster. `threads == 0` means "auto" (ferx passes `NULL` through to R, so a
/// single fit may claim every core) so any `max_concurrent > 1` combined
/// with auto-threads always oversubscribes. Pure and display-only: never
/// used to change the user's setting, only to explain the consequence of it.
fn oversubscription_warning(max_concurrent: usize, threads: u32, cores: usize) -> Option<String> {
    if max_concurrent <= 1 {
        return None;
    }
    if threads == 0 {
        // ferx-r 0.3.0 documents the real "auto" default as cores − 1, capped at 8
        // (not "every core") — quote that estimate rather than the raw core count.
        let auto_threads = cores.saturating_sub(1).min(8);
        return Some(format!(
            "{max_concurrent} concurrent fits with threads set to \"auto\" — each fit may claim \
             up to {auto_threads} core(s) on this machine, so they will likely run slower together \
             than one at a time. Consider setting an explicit per-run thread count on the Run tab."
        ));
    }
    let requested = max_concurrent as u64 * threads as u64;
    if requested > cores as u64 {
        return Some(format!(
            "{max_concurrent} concurrent fits × {threads} threads each = {requested} threads \
             requested, more than the {cores} core(s) available — they will likely run slower \
             together than one at a time. Consider lowering the thread count or the concurrent-fits limit."
        ));
    }
    None
}

#[cfg(test)]
mod oversubscription_warning_tests {
    use super::oversubscription_warning;

    #[test]
    fn no_warning_when_only_one_concurrent_run_is_allowed() {
        assert!(oversubscription_warning(1, 0, 8).is_none());
        assert!(oversubscription_warning(1, 32, 8).is_none());
    }

    #[test]
    fn warns_when_threads_is_auto_and_more_than_one_concurrent_run_is_allowed() {
        assert!(oversubscription_warning(2, 0, 8).is_some());
    }

    #[test]
    fn no_warning_when_explicit_threads_fit_within_available_cores() {
        assert!(oversubscription_warning(2, 2, 8).is_none()); // 4 <= 8
    }

    #[test]
    fn warns_when_explicit_threads_exceed_available_cores() {
        assert!(oversubscription_warning(4, 4, 8).is_some()); // 16 > 8
    }

    #[test]
    fn boundary_exactly_equal_to_core_count_is_not_a_warning() {
        // 4 * 2 == 8 cores exactly — fully subscribed, not OVER-subscribed.
        assert!(oversubscription_warning(4, 2, 8).is_none());
    }
}

// ---------------------------------------------------------------------------
// Floating run-output popup
// ---------------------------------------------------------------------------

/// Which run the single floating run-popup (and the header's busy indicator)
/// displays, now that several models may run concurrently: the one that
/// started most recently. This generalises the old single-`active_run`
/// semantics — where "the run" and "the newest run" were trivially the same
/// thing — without adding a new run picker or one window per run (the
/// approved design deliberately keeps the existing single popup).
fn display_run(state: &AppState) -> Option<&crate::domain::ActiveRun> {
    state.run.active_runs.values().max_by_key(|r| r.started_at)
}

/// Number of most-recent log lines the Run popup lays out.
const RUN_POPUP_TAIL_LINES: usize = 400;

/// Returns the last `max_lines` lines of `text` and how many earlier lines were cut.
fn log_tail(text: &str, max_lines: usize) -> (&str, usize) {
    let total = text.bytes().filter(|&b| b == b'\n').count() + 1;
    if total <= max_lines {
        return (text, 0);
    }
    let mut cut = text.len();
    let mut seen = 0usize;
    for (i, b) in text.bytes().enumerate().rev() {
        if b == b'\n' {
            seen += 1;
            if seen == max_lines {
                cut = i + 1;
                break;
            }
        }
    }
    (&text[cut..], total - max_lines)
}

#[cfg(test)]
mod log_tail_tests {
    use super::log_tail;

    #[test]
    fn short_text_is_untouched() {
        assert_eq!(log_tail("a\nb\nc", 5), ("a\nb\nc", 0));
    }

    #[test]
    fn long_text_keeps_the_last_lines() {
        let (t, hidden) = log_tail("1\n2\n3\n4\n5", 2);
        assert_eq!(t, "4\n5");
        assert_eq!(hidden, 3);
    }
}

fn render_run_popup(ctx: &egui::Context, state: &mut AppState) {
    use crate::workers::messages::CancelMode;

    // Auto-open when a new run starts (unique run ID, so re-running the same model works).
    if let Some(run) = display_run(state) {
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
    let has_active  = display_run(state).is_some();
    let queue_len   = state.run.run_queue.len();
    // `stem` (from `run_panel_status`) is this popup's displayed run when
    // one is active, or the last-finished run's stem otherwise — either way
    // it's the right key into the per-stem retained log.
    let log_text    = state.run.run_logs.get(&stem)
        .map(|l| l.log_text.clone())
        .unwrap_or_default();
    let log_path    = display_run(state)
        .map(|r| r.log_path.to_string_lossy().to_string())
        .unwrap_or_default();
    let active_stem = display_run(state)
        .map(|r| r.record.model_stem.clone())
        .unwrap_or_default();
    // Only shown once the run itself has finished (export runs as a
    // follow-up step after completion) and only for the model it happened
    // for — without this, a failure here had zero indicator, not even a
    // false "success", since there's no in-flight state for this step at all.
    let export_tables_error = state.ui.export_tables_error.as_ref()
        .filter(|(s, _)| *s == stem)
        .map(|(_, msg)| msg.clone());

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

                if let Some(err) = &export_tables_error {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("⚠ Save output tables failed").color(theme::RED).size(11.0).strong(),
                    );
                    ui.label(egui::RichText::new(err).color(theme::RED).size(10.0));
                }

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
                            // Lay out only the tail: one galley over the whole 5,000-line
                            // buffer is ~500k glyphs re-tessellated every frame, which is
                            // what made the popup's text turn to garbage late in long runs.
                            let (tail, hidden) = log_tail(&log_text, RUN_POPUP_TAIL_LINES);
                            if hidden > 0 {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "… {hidden} earlier lines not shown (full output in the log file)"))
                                        .color(dim_fg).size(10.0),
                                );
                            }
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(tail)
                                        .font(egui::FontId::monospace(11.0))
                                        .color(log_fg),
                                ).wrap(),
                            );
                        }
                    });
            });
        },
    );

    // Apply actions now that the viewport closure has returned. `active_stem`
    // was captured above (from the same `display_run` this popup rendered),
    // so these always act on the run the user was actually looking at.
    if do_kill {
        if let Some(r) = state.run.active_runs.get(&active_stem) {
            let _ = r.cancel_tx.send(CancelMode::Kill);
        }
    }
    if do_stop {
        if let Some(r) = state.run.active_runs.get(&active_stem) {
            let _ = r.cancel_tx.send(CancelMode::Graceful);
        }
    }
    if do_detach {
        state.run.active_runs.remove(&active_stem);
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
    let error       = state.workspace.sir_error.get(&stem).cloned();
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
    } else if error.is_some() {
        format!("SIR failed — {stem}")
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
                } else if let Some(ref err) = error {
                    ui.label(
                        egui::RichText::new("✖  SIR failed").color(theme::RED).size(13.0).strong(),
                    );
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(err).color(theme::RED).size(11.0));
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
    if let Some(run) = display_run(state) {
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
/// left over from a previous session.  For every one whose PID is still
/// alive, starts a monitor/tailer thread to reconnect to it so the run panel
/// shows its output.
///
/// Reconnects *every* live manifest, not just one — an earlier version of
/// this function could only track a single active run in the UI, so it kept
/// the most-recently-modified manifest and silently abandoned the rest
/// (their processes kept running, untracked, forever). Now that
/// `active_runs` is keyed by stem, there's no reason to drop any of them,
/// even if there happen to be more live manifests than `max_concurrent_runs`
/// allows for *new* launches — that limit only gates starting fresh runs,
/// not reconnecting ones that are already running.
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

    let mut reconnected_stems: Vec<String> = Vec::new();
    let mut last_run_id: Option<String> = None;

    for (mfst_path, manifest) in manifests {
        // Gone, or the PID now belongs to a different process (recorded start time differs).
        let same_process = manifest.identity.as_ref().is_none_or(|i| i.is_still_that_process());
        let alive = RunManifest::is_pid_alive(manifest.pid) && same_process;

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

        if !alive {
            // The run ended while the GUI was closed. Record it with what we can establish
            // (status file + a valid bundle), instead of silently forgetting it.
            let mut done = record.clone();
            let code = crate::workers::run::orphan_exit_code(manifest.status_path.as_deref(), &done);
            done.status = match code {
                0 => JobStatus::Completed,
                crate::workers::run::EXIT_UNKNOWN => JobStatus::Unknown,
                _ => JobStatus::Failed,
            };
            done.completed = Some(crate::workers::run::now_iso());
            state.run.run_history.push(done);
            if let Some(sp) = &manifest.status_path { let _ = std::fs::remove_file(sp); }
            RunManifest::remove(&mfst_path);
            continue;
        }

        let (cancel_tx, cancel_rx) = std::sync::mpsc::channel::<CancelMode>();
        let tx = state.worker_tx.clone();

        reconnect_orphan(
            manifest.clone(),
            mfst_path.clone(),
            record.clone(),
            tx,
            cancel_rx,
        );

        let stem = manifest.model_stem.clone();
        state.run.active_runs.insert(stem.clone(), ActiveRun {
            record,
            started_at:    std::time::Instant::now(), // approximate
            log_path:      manifest.log_path,
            cancel_tx,
            export_tables:  false, // not known for reconnected runs; user can re-run if needed
            run_sir_after:  false,
        });
        last_run_id = Some(manifest.run_id);
        reconnected_stems.push(stem);
    }

    if let Some(run_id) = last_run_id {
        state.ui.run_popup_open = true;
        state.ui.run_popup_last_run_id = Some(run_id);
    }
    state.ui.status_message = match reconnected_stems.as_slice() {
        [] => return,
        [one] => format!("Reconnected to running: {one}"),
        many => format!("Reconnected to {} running fits: {}", many.len(), many.join(", ")),
    };
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

    crate::util::spawn_guarded("app:1995", tx.clone(), move || {
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

#[cfg(test)]
mod warm_popup_glyph_tests {
    use super::warm_popup_glyphs;

    fn lit_pixels(ctx: &egui::Context) -> usize {
        ctx.fonts(|f| f.image().pixels.iter().filter(|&&p| p > 0.0).count())
    }

    /// The warm-up must put glyphs for the popup-only font sizes into the atlas
    /// during a pass of its own, so they are never first rasterised in a nested one.
    #[test]
    fn warming_rasterises_popup_only_sizes() {
        let plain = egui::Context::default();
        let _ = plain.run(Default::default(), |_| {});
        let baseline = lit_pixels(&plain);

        let warmed = egui::Context::default();
        let _ = warmed.run(Default::default(), warm_popup_glyphs);
        let after = lit_pixels(&warmed);

        assert!(after > baseline + 2_000, "baseline {baseline}, warmed {after}");
    }

    #[test]
    fn warming_again_is_a_cache_hit_and_adds_nothing() {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), warm_popup_glyphs);
        let first = lit_pixels(&ctx);
        let _ = ctx.run(Default::default(), warm_popup_glyphs);
        assert_eq!(first, lit_pixels(&ctx));
    }
}

#[cfg(test)]
mod font_sync_tests {
    use super::{upload_full_font_atlas, FontSync, FONT_RESYNC_INTERVAL};
    use std::time::{Duration, Instant};

    #[test]
    fn resyncs_after_atlas_growth_then_settles_then_heartbeats() {
        let t0 = Instant::now();
        let mut s = FontSync::new();
        s.last_upload = t0;
        let calm = [false; 4];
        // First sight of an atlas size counts as a change: three frames of re-uploads.
        assert!(s.due([16384, 64], calm, t0));
        assert!(s.due([16384, 64], calm, t0));
        assert!(s.due([16384, 64], calm, t0));
        assert!(!s.due([16384, 64], calm, t0), "quiet once settled");
        assert!(!s.due([16384, 64], calm, t0 + Duration::from_secs(1)));
        // The atlas grows: more re-uploads.
        assert!(s.due([16384, 128], calm, t0 + Duration::from_secs(1)));
        // A popup opens: more re-uploads.
        let open = [true, false, false, false];
        for _ in 0..3 { assert!(s.due([16384, 128], open, t0 + Duration::from_secs(1))); }
        assert!(!s.due([16384, 128], open, t0 + Duration::from_secs(1)));
        // Slow heartbeat even with nothing changing.
        assert!(s.due([16384, 128], open, t0 + Duration::from_secs(1) + FONT_RESYNC_INTERVAL));
    }

    /// The upload must be a *full* texture update at exactly the atlas's current size,
    /// which is what recreates a stale GPU texture.
    #[test]
    fn upload_is_a_full_delta_at_the_atlas_size() {
        let ctx = egui::Context::default();
        let out = ctx.run(Default::default(), |ctx| {
            // Grow the atlas past its initial height so a size mismatch would be visible.
            ctx.fonts(|f| {
                for size in [10.0, 11.0, 12.0, 13.0, 16.0, 18.0] {
                    f.layout_no_wrap("The quick brown fox 0123456789".into(),
                        egui::FontId::proportional(size), egui::Color32::WHITE);
                }
            });
            upload_full_font_atlas(ctx);
        });
        let size = ctx.fonts(|f| f.font_image_size());
        let font_deltas: Vec<_> = out.textures_delta.set.iter()
            .filter(|(id, _)| *id == egui::TextureId::default()).collect();
        let last = &font_deltas.last().expect("a font texture upload").1;
        assert!(last.pos.is_none(), "must be a full update, not a partial one");
        assert_eq!([last.image.width(), last.image.height()], size);
    }
}

/// Every 30 s while an editor is dirty, copy its text to `<app dir>/drafts/` (not the project).
fn write_drafts(state: &mut AppState) {
    if !state.ui.has_unsaved_edits() { state.ui.last_draft_at = None; return; }
    let due = state.ui.last_draft_at.is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(30));
    if !due { return; }
    state.ui.last_draft_at = Some(std::time::Instant::now());
    let Some(app_dir) = state.workspace.app_dir.clone() else { return };
    if state.ui.editor_dirty {
        if let Some(m) = state.ui.selected_model.and_then(|i| state.workspace.models.get(i)) {
            crate::io::textdoc::write_draft(&app_dir, &m.model.path, &state.ui.editor_buffer);
        }
    }
    if state.ui.files_text_dirty {
        if let Some(p) = &state.ui.files_selected {
            crate::io::textdoc::write_draft(&app_dir, p, &state.ui.files_text);
        }
    }
}
