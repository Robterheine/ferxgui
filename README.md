# FeRx GUI

A desktop application for building and evaluating population pharmacokinetic / pharmacodynamic (PK/PD) models with [FeRx NLME](https://ferx-nlme.github.io/).

Built with Rust + [egui](https://github.com/emilk/egui). Runs on macOS, Windows, and Linux.

---

## Features

### Model workflow
- Browse, create, and edit `.ferx` model files with syntax highlighting
- Run models with configurable methods (FOCE, FOCEI, SAEM, Gradient Newton), covariance step, optimizer trace, and output tables
- Live log streaming, run queue, process detachment (runs survive GUI close or SSH disconnect)
- Model ancestry tree with ΔOFV labels, pan/zoom, and PNG export

### Evaluation
- Goodness-of-fit plots (4-panel: DV vs PRED/IPRED, CWRES vs time/IPRED) with LOESS overlay and log-scale toggle
- Individual fits with subject paging
- iOFV waterfall per subject
- Convergence trace (OFV + method-specific metric: MH accept rate, LM λ, gradient norm)
- ETA–covariate correlations
- Parameter correlation heatmap from the covariance matrix

### Visual Predictive Check (VPC)
Powered by the R [vpc package](https://vpc.ronkeizer.com/) — all two-stage statistics, binning, and confidence bands are computed by the package; FeRx GUI renders them natively and interactively.

- **Continuous VPC** with configurable prediction intervals (5/95, 10/90, 25/75, custom), confidence intervals, and multiple binning methods (Jenks, k-means, density, time, data, manual)
- **Prediction-corrected VPC (pcVPC)** — normalises by population predictions from `fit$sdtab`
- **Censored / BLOQ VPC** — fraction-below-LOQ vs time via `vpc_cens()`, with LLOQ/ULOQ reference lines
- **Stratification** — split into panels by any dataset column (CMT, analyte, dose group, covariate); up to two stratification variables
- Sim cache: first run simulates; changing display options (PI, CI, binning) re-runs the ~1 s statistics step without re-simulating
- **R ggplot export** — editable R script editor (OS-native window) lets you customise the ggplot and save a publication-quality PNG

### SIR uncertainty
- Sequential Importance Resampling with effective sample size, 95% CI table, parameter correlation heatmap, and marginal distribution histograms

### Simulation plot
Pure-Rust simulation plotter (no R required): load NONMEM-format or CSV simulation output, configure prediction-interval bands, MDV/column filters, observed data overlay, log Y-axis, and export PNG.

### Files tab
Two-pane file browser with filter pills, CSV table viewer with **in-place cell editing** (Tab/Shift-Tab/Enter navigation, Escape to discard), scatter plot with LOESS and log axes, and `.ferx` syntax-highlighted editor.

### Run report
Scrollable in-app run report with parameter tables, DW statistic, IWRES lag-1 r, shrinkage, parameter correlation heatmap, and HTML export.

---

## Requirements

### R (required)
FeRx NLME runs entirely inside R. FeRx GUI calls `Rscript` for all modelling operations.

| Software | Minimum version | Notes |
|---|---|---|
| **R** | 4.2 | [r-project.org](https://www.r-project.org/) |
| **ferx** R package | 0.2.0 | See installation below |
| **vpc** R package | 1.0 | Required for VPC tab |
| **ggplot2** R package | 3.0 | Required for R ggplot export |
| **jsonlite** R package | — | Usually installed with R |

### Rust build toolchain (to build from source)
| Tool | Version |
|---|---|
| Rust | stable (1.76+) |
| Cargo | bundled with Rust |

---

## Installing R packages

```r
# Install ferx from GitHub (requires devtools or remotes)
install.packages("devtools")
devtools::install_github("FeRx-NLME/ferx-r")

# Install vpc and ggplot2 from CRAN
install.packages(c("vpc", "ggplot2", "jsonlite"))
```

---

## Installation

### macOS (recommended: Homebrew)

```bash
# 1. Install Homebrew if not present
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# 2. Install R
brew install --cask r

# 3. Install Rust
brew install rustup
rustup-init          # follow the prompts; choose default install

# 4. Clone and build FeRx GUI
git clone https://github.com/robterheine/ferxgui.git
cd ferxgui
cargo build --release

# 5. Run
./target/release/ferxgui
```

The first launch auto-detects `Rscript` and the `ferx` package. If detection fails, go to **Settings** and browse to your `Rscript` executable manually.

### Windows

1. **Install R** — download the installer from [r-project.org](https://cran.r-project.org/bin/windows/base/). Accept defaults; ensure R is added to `PATH`.

2. **Install Rust** — download `rustup-init.exe` from [rustup.rs](https://rustup.rs/) and run it. Select the default installation.

3. **Clone and build**
   ```powershell
   git clone https://github.com/robterheine/ferxgui.git
   cd ferxgui
   cargo build --release
   ```

4. **Run**
   ```powershell
   .\target\release\ferxgui.exe
   ```

> **Note for Windows:** FeRx processes are spawned with `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP` so they survive terminal close and SSH sessions. If your environment uses Job Objects that prevent breakaway (e.g. some CI runners), set `loginctl enable-linger` or run as a normal desktop user.

### Linux (Ubuntu / Debian)

```bash
# 1. Install system dependencies (needed by egui/eframe)
sudo apt-get update
sudo apt-get install -y \
  r-base \
  libgtk-3-dev \
  libxcb-render0-dev \
  libxcb-shape0-dev \
  libxcb-xfixes0-dev \
  libxkbcommon-dev \
  libssl-dev

# 2. Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# 3. Clone and build
git clone https://github.com/robterheine/ferxgui.git
cd ferxgui
cargo build --release

# 4. Run
./target/release/ferxgui
```

> **Linux SSH note:** If you use FeRx GUI over SSH/X11, model runs are spawned with `setsid()` so they are immune to SIGHUP. However, if your system uses `KillUserProcesses=yes` in `/etc/systemd/logind.conf`, background processes may be killed when your session ends. Ask your administrator to run `loginctl enable-linger <your-username>` to allow lingering processes.

---

## Quick start

1. Launch FeRx GUI and go to **Settings** to confirm R / ferx are detected (shown in green).
2. Set your **working directory** — the folder containing your `.ferx` model files.
3. Select a model in the **Models** tab, choose your data file in the Run pill, and click **Run**.
4. After a successful run, explore **Evaluation**, **VPC**, and **SIR** tabs for diagnostics.

Test models and data for the warfarin example are included in the `ferx` R package under `inst/examples/`.

---

## Configuration

FeRx GUI stores its settings in `~/.ferxgui/settings.json`. You can edit this file directly or use the **Settings** tab in the application. Key fields:

| Field | Description |
|---|---|
| `ferx_binary` | Path to `Rscript` (auto-detected; override here if needed) |
| `working_directory` | Default folder opened on launch |
| `rstudio_path` | Optional path to RStudio executable (adds "Open RStudio" button) |

---

## Building for release

```bash
cargo build --release
```

The binary is at `target/release/ferxgui` (or `ferxgui.exe` on Windows). No installer is required — copy the single binary to any location on your `PATH`.

---

## Continuous integration

CI runs on every push to `main` / `master` via GitHub Actions (`.github/workflows/ci.yml`), building and testing on macOS, Windows, and Ubuntu.

---

## Changelog

### v0.8.6 (2026-07-08) — About popup: duplicate title removed, invisible text fixed

**Fixed: the About popup's title text was nearly invisible, and "FeRx GUI" appeared twice**
- `show_ferx_logo()` already renders "FeRx" + "GUI" as its own wordmark next to the curve icon; the About popup then repeated a separate "FeRx GUI" label right after it. That label was also hard to read: `.strong()` resolves to `visuals.widgets.active.text_color()`, which this theme sets to white in both themes (correct for text on accent-filled buttons, invisible on this popup's plain light-mode background).
- Also fixed the underlying layout bug that caused the wordmark itself to render as two stacked lines instead of one: `show_ferx_logo` assumes a horizontal layout from its caller (correct in the header, where it's already inside one), but the About popup called it inside a vertical, centered layout. Now wrapped in its own horizontal group.
- The duplicate white-text pattern likely affects other `.strong()` labels elsewhere in the app that sit on a plain (non-accent-filled) background — flagged as a follow-up, not fixed in this pass.

### v0.8.5 (2026-07-07) — model-list context menu overhaul, run-launch guard fix, declared-dataset column

**Changed: the model-list right-click menu**
- Removed "Open in Finder" and the three Copy path/folder/name items.
- Added a **Run** submenu: flip Method, Covariance step, and Optimizer trace right there, then "Run now" — no need to visit the Run pill first. Shares the same launch guard the Run pill uses (`can_launch_run`), so it can't be used to sneak a second run past an already-active one.
- "View run log" / "View run record…" are now greyed out (with a tooltip) for a model that has never run, instead of silently going nowhere.

**Fixed: Run was disabled for a model that had already run, just not the currently-selected one**
- The data path used to launch a run is a single global field, only auto-populated for whichever model is currently *selected* in the list. Right-clicking a different model — one that had run before, just not this session's selected one — saw that field as unset and showed Run as permanently disabled. It now falls back to that specific model's own run-history entry when the global field is empty.

**Fixed: the Run submenu showed a cluster of redundant triangles**
- `egui::menu_button` already draws its own submenu arrow; a manually-typed `▶` on both sides of the label was piling two more triangles on top of it. Now just "Run".

**New: a DATA column in the model list**
- Shows the dataset path declared in a model's own `[data]` block (ferx's equivalent of NONMEM's `$DATA` record — `path = warfarin.csv`, resolved relative to the model file's directory), blank when the model has none. Parsing this also surfaced that ferxgui doesn't yet read a model's own `[data]` block when launching a run — it always supplies an explicit external data path, which silently overrides the model's declaration; noted as a follow-up, not fixed in this pass.

### v0.8.0 (2026-07-07) — usability pass from peer feedback: editor/focus bugs, contrast, native popups, model comparison

A full triage of external peer feedback on the GUI (see `design/ferx-gui-peer-feedback-plan.md`), plus five further bugs found while manually verifying the fixes.

**Fixed: Enter in the model editor jumped out to the Output pill**
- The model list's `Space`/`Enter` row shortcuts read global key state every frame, with no check for whether another widget (e.g. the code editor) currently had keyboard focus — so pressing Enter while typing in the editor also triggered the list's "jump to Output" shortcut. Now guarded on focus.

**Fixed: the code editor's cursor jumped to the wrong line while typing**
- The editor's syntax-highlighting layouter cached its output keyed on the *previous* frame's buffer, but `egui::TextEdit` calls the layouter again mid-frame with the *post-edit* text to compute cursor placement — the stale cache mismatched what was asked, corrupting the cursor's row/column mapping. Fixed by keying the cache on the text actually queried, not an external buffer reference.

**Fixed: VPC "Log y-axis" changed the data but not the axis labels**
- The log transform was applied to the data *and* combined with `egui_plot::log_grid_spacer` (designed for raw, untransformed data) — double-applying log spacing and leaving the axis labelled in small linear numbers instead of 1/10/100/1000. Replaced with a proper axis-tick formatter that converts back to power-of-ten labels.

**Fixed: readability/contrast**
- App-wide: any selected `selectable_label` (VPC's Continuous/Censored toggle, the theme picker, etc.) rendered accent-coloured text on an accent-coloured background — fixed at the theme level (`Visuals::selection`), which fixes every instance at once, in both themes.
- The Editor's **Save** button had unreadable near-white text on its green fill in dark mode; the Output pill's warning cards used a translucent orange wash that went nearly invisible in light mode. Both now use explicit, theme-independent, high-contrast colours.
- The VPC and SIR tabs now show the selected model's name (previously only the Evaluation tab did), so it's never ambiguous which model's fit is being visualised.

**Changed: layout**
- The Editor/Run/Output/Parameters/Info/Report row now reads as an actual tab strip (accent underline on the active tab, no background fill) instead of a row of pill-shaped buttons.
- Individual Fits now lays subjects out in a roughly square grid (2×2, 3×3, …) instead of forcing 2 columns regardless of how many are shown per page; the per-page count goes up to 9 (new default) instead of maxing at 6.

**New: Settings is a floating window, not a sidebar tab**
- Opens via a header button or **Cmd/Ctrl+,**, matching the native-app convention that Preferences lives in its own window, not the main document view.

**New: a File / View / About menu bar**
- An in-window bar (not the OS-native macOS menu bar — that would need a separate crate and platform-specific integration this pass didn't take on), so it renders identically on macOS/Linux/Windows. File has Settings and Quit; View has the theme toggle, sidebar collapse, and the tab list; there's no Edit menu, since the editor's Cut/Copy/Paste/Undo already work via native OS shortcuts with nothing else for it to expose.

**New: model comparison — GOF plots, a discoverable entry point, real windows**
- The existing (but easy-to-miss, right-click-only) "Compare with…" now has a visible **Compare Models…** button in the Models tab toolbar, opening a picker restricted to models with a completed fit.
- The compare dialog gained a GOF-comparison section (DV vs PRED, side by side per model), reusing the Evaluation tab's existing scatter/LOESS plotting code rather than duplicating it.
- Both the picker and the compare dialog are now real OS windows (matching the existing About/Run/SIR/Settings popups), not in-window dialogs — the latter could render partially outside the main window with the Cancel/Close button unreachable (reported: "cannot close it anymore and it not fully visible on screen").
- The comparison table now ends with a plain OFV/AIC row per model (no delta computed) instead of a separate ΔOFV/ΔAIC/LRT summary line above the table.

**Fixed: app-wide missing-glyph "tofu" boxes**
- ✓ (U+2713) and ✗ (U+2717) — used as status icons in the model list, Check Init, Info pill, VPC package status, and the compare dialog — aren't covered by *any* font `eframe`'s `default_fonts` feature bundles (confirmed by direct inspection of each font's character map), so they always rendered as empty boxes. Replaced with ✔/✖ (U+2714/U+2716), which are covered. Added a regression test that scans the whole source tree for the broken codepoints so this can't silently reappear.

**Fixed: switching to an unrun model kept showing the previous model's GOF plots**
- The Evaluation tab's prediction-data reload was gated on the newly-selected model having a completed fit, so switching *to* a model without one skipped the staleness check entirely and left the previous model's data cached and displayed.

**Fixed: a failed "Check inits" was invisible**
- A failed run only surfaced as a few words in the small status bar at the bottom of the window — easy to miss entirely ("spinner appears then disappears, nothing else"). Failures now show a proper error card with the actual R error message, next to the Check inits button.

**Fixed: starting a run showed the previous run's log history**
- The run popup's log text is a separately-maintained cache of the log-line buffer, rebuilt incrementally rather than every frame; starting a new run cleared the buffer but not this cache, so the popup kept showing the entire previous run's output until enough new lines arrived to push it out.

### v0.7.0 (2026-07-06) — fit.json parsing fix for single-parameter/single-warning models, surfaced parse errors

**Fixed: a completed run could show "no run output" for a valid, converged fit**
- R's jsonlite `auto_unbox = TRUE` serializes a length-1 vector as a bare scalar instead of a single-element array — e.g. a model with exactly one theta serializes `theta.estimates` as `0.134`, not `[0.134]`; a fit with exactly one warning serializes `warnings` as a bare string, not a one-element array. This is not a rare edge case: a single-method (non-chained) fit's `method_chain` collapses the same way, and it hit the bundled warfarin tutorial model directly, whose `.fitrx` had a `converged: true`, fully valid fit that FeRx GUI reported as "no run output yet."
- Several fields (`sigma`, `shrinkage_eps`, `method_chain`, `eta_param_info`) already had deliberate scalar-or-array handling for exactly this reason — `warnings`, and the nested `theta`/`omega`/`iov` names/SE/shrinkage/estimates fields, did not. All of them now go through the same scalar-or-array conversion, so a model with exactly one theta, one ETA, one kappa, or one warning parses correctly. Consolidated two byte-identical duplicate helper functions in the process.
- Added a dedicated regression test reproducing this exact single-element shape end-to-end, independent of any local file, so this class of bug is caught on every machine and in CI — not just by chance on whichever `.fitrx` happens to be lying around during manual testing.

**Fixed: a `.fitrx` parse failure was silently indistinguishable from "never run"**
- The scanner discarded any `.fitrx` parse error via `.ok()`, so a bundle that failed to parse for *any* reason — this bug, a future ferx-schema change, a corrupt file — looked identical to a model that had simply never been run. `ModelEntry` now carries the parse error when this happens, distinguished from "never run" everywhere it's shown: the Models list (a distinct row colour, with the actual error on hover), and the Output, Parameters, Report, and Param Corr empty-states, which now explain that a bundle exists but couldn't be read instead of prompting the user to re-run a model that already produced valid results.

### v0.6.0 (2026-07-06) — ferx 0.2.0 impact audit: ETA-Cov fix + declared-covariate screen, convergence trace fix, DSL parser hardening

A full audit of the ferx-core/ferx-r 0.2.0 release against every R call and parser FeRx GUI relies on. Two things were confirmed broken, one recently-added GUI feature is now confirmed live, one latent parser bug (unrelated to the version bump, but found while checking the new DSL sections) is fixed, and the ETA-Cov section gains a second, more formal screening view.

**Fixed: ETA-Covariate screen — `ferx_eta_cov(fit, data)` removed in ferx-r 0.2.0**
- ferx-r's [fit-accessor cleanup](https://github.com/FeRx-NLME/ferx-r/issues/226) removed `ferx_eta_cov()`, `ferx_cor_matrix()`, and `ferx_estimates()` as callable functions, replacing them with fields computed automatically at fit time (`fit$eta_cov`, `fit$cor_matrix`, `fit$estimates`). FeRx GUI's ETA-Cov section called the removed function directly and would fail on any ferx-r ≥ 0.2.0 install.
- Switched to reading `fit$eta_cov` after `ferx_load_fit()`. This also **simplifies the feature**: the dataset picker and "Run ETA-Cov Screen" setup step are gone — the screen now loads automatically the moment a fit exists, matching how the other Evaluation sections already behave, since ferx-r recomputes it from the dataset path recorded on the fit itself.
- `fit$eta_cov` can be empty for two different reasons — too few subjects, or the original dataset no longer being readable at its recorded path — which the previous single-message empty state couldn't tell apart. The GUI now distinguishes them and explains the second case explicitly instead of silently showing a misleading "no pairs found."

**New: Declared Covariates view, merged into the ETA-Cov section**
- The ETA-Cov section now has two views, toggled at the top: **Dataset Scan** (the informal `fit$eta_cov` screen above — correlates raw EBEs against every numeric dataset column, no covariate needs to be declared) and **Declared Covariates** (new — `ferx_cov_screen(fit)`, a formal screen using the model's own `[covariates]` block, aggregated to one value per subject exactly as the model would use it, reporting association against both the raw individual parameter estimate and its random effect).
- Each view carries a short caption plus a "ⓘ" hover with the full statistical explanation — including that the two views use independently-calibrated thresholds (Dataset Scan: |r| ≥ 0.3; Declared Covariates: |association| ≥ 0.2, both ferx-r's own defaults) and that neither view's flagged pairs are themselves a formal covariate test.
- The Declared Covariates table labels its two measure columns "EBE ASSOC." / "ETA ASSOC." rather than reusing `ferx_cov_screen()`'s bare `eta` column name, which would otherwise read as "the ETA value itself" rather than "association strength with the ETA."
- Distinguishes three legitimate empty states: no `[covariates]` block declared (the common case for most models), no random effects to screen against, and nothing clearing the threshold — each explained separately.
- Computed lazily and independently per view (its own cache, its own in-flight tracking) — switching views only triggers an R call for whichever one hasn't been computed yet, not both up front.

**Fixed: convergence trace lost after a temp-file cleanup**
- ferx-r 0.2.0 now bundles the optimizer trace directly inside `.fitrx` as `trace.csv`, specifically because the external temp file `trace_path` points to "usually doesn't survive" a reboot or OS temp-file cleanup (its own words, from the ferx-r source). FeRx GUI's Convergence tab only ever read the external path, so any bundle whose temp file had since been cleaned up showed "Trace file not found" — even though the trace was sitting right there in the bundle.
- The Convergence tab now reads `trace.csv` from inside the bundle first, falling back to the external path only for older bundles that don't carry it.
- Added a **Monotonic OFV** toggle (on by default) to the Convergence tab: FOCE/FOCEI iterations now show the running-minimum OFV, hiding the transient upticks from rejected line-search trial steps — matching the default of ferx-r's own `plot(fit)`. The reported "Final OFV" always stays the raw value regardless of the toggle; only the plotted line is smoothed.

**Confirmed working, no GUI change needed: Cond. Dist. section**
- The `conddist.csv` bundling this was waiting on ([ferx-core#675](https://github.com/FeRx-NLME/ferx-core/issues/675), filed in v0.5.0) shipped in ferx-core/ferx-r 0.2.0, in exactly the schema FeRx GUI already reads. The "Cond. Dist." Evaluation section added in v0.5.0 is live for any SAEM fit run with `conddist = true`.

**Fixed: an unrecognised `[section]` could leak into whatever came before it**
- ferx-core 0.2.0 adds several new model-file sections (`[event_model]` for joint PK-TTE, `[adaptive_dosing]`, `[initial_conditions]`). Checking these against the `.ferx` parser surfaced a pre-existing bug unrelated to the version bump: an unrecognised bracketed section didn't reset the parser's "current section" state, so its content — and anything after it, up to the next recognised section — could be silently misattributed to whichever recognised section came before it. Fixed, and the four new section names are now recognised (editor syntax highlighting for `[...]` headers already worked regardless, since it colours by bracket pattern, not by a recognised-name list).

**Investigated, no action needed:** every other `ferx_*` call FeRx GUI makes (`ferx_check_init`, `ferx_fit`, `ferx_load_fit`, `ferx_model`, `ferx_model_inspect`, `ferx_save_fit`, `ferx_simulate`, `ferx_sir`) is unaffected by ferx-core/ferx-r 0.2.0's other breaking changes (section-function collapse, `ferx_plot_trace` → `plot()`, `ferx_selection_excluded` removal, and the various renames) — none of them touch calls FeRx GUI actually makes.

**Backlog, not implemented this round:** dedicated GUI controls for the large set of new `[fit_options]` keys (`npde_*`, `imp_*`/`impmap_*`, `cov_inner_tol`, `outer_xtol`/`outer_ftol`, `global_search`, `bloq_method`, `mu_referencing`, `iov_column`, `inits_from_nca`, and others) — all of these already work correctly when set directly in the model file; this is about deciding which, if any, deserve a dedicated widget rather than staying file-only.

### v0.5.0 (2026-07-02) — model-file fit options, SAEM conditional distributions, ferx-r 0.2.0 compatibility

**Model file `[fit_options]` is now authoritative**
- `covariance`, `method`, `gradient`, and `threads` are parsed from the model file's `[fit_options]` block and used to initialise the Run pill whenever a model is selected. Previously these keywords were only recognised for syntax highlighting — the values were parsed but silently discarded, so editing them (including commenting one out) had no effect on what actually ran.
- **Covariance is now opt-in from the file**: an absent or commented-out `covariance` line means the covariance step is off, matching what commenting it out visibly implies. Fixes a bug where disabling covariance in the model file left the previous (ticked) Run-pill state in effect, so the fit ran — and reported results — with covariance still on.
- Per-run overrides in the Run pill still work as before; the file only sets the starting point on load.

**New Evaluation section: Cond. Dist. (SAEM conditional distributions)**
- Reads `conddist.csv` from `.fitrx` bundles (when present) and adds a "Cond. Dist." tab to the Evaluation view with three sub-views: a per-ETA histogram of the conditional mean with a theoretical `N(0, ω)` overlay and distribution-based shrinkage annotation, a caterpillar plot of per-subject mean ± SD, and a Mode-vs-Mean scatter against the identity line.
- Distribution-based shrinkage (`1 - SD(cond_mean)/√ω_jj`) is computed client-side, since it isn't part of the CSV schema.
- The empty-state hint is method-aware: SAEM fits without `conddist.csv` are told to re-run with `conddist = true`; non-SAEM fits are told the feature requires SAEM, with no dead-end retry prompt.
- **This section is currently dormant for everyone**: `conddist.csv` bundling into `.fitrx` hasn't shipped in `ferx-core` yet ([ferx-core#675](https://github.com/FeRx-NLME/ferx-core/issues/675), filed as part of this work). The reader degrades cleanly (`None`) on any bundle without it, so this activates automatically — no ferxgui update needed — once that lands.

**Fixed: `ferx_model_new` — removed in ferx-r 0.2.0**
- ferx-r's [API cleanup](https://github.com/FeRx-NLME/ferx-r/issues/223) removed `ferx_model_new()` (hard break, no deprecation shim) in favour of a `template =` scaffold mode on `ferx_model()`. FeRx GUI's "Create model" feature called the removed function directly and would fail on any ferx-r ≥ 0.2.0 install. Updated the call site; the **minimum supported ferx R package version is now 0.2.0** (was 0.1.5).
- Audited the rest of ferx-r's cleanup tracking issue against every `ferx_*` function FeRx GUI calls: one further item is a known upcoming break to watch (`ferx_eta_cov(fit, data)` is slated to drop its `data` argument once [ferx-r#226](https://github.com/FeRx-NLME/ferx-r/issues/226) lands — not yet merged, no action needed today) and one is worth re-checking once unblocked (trace storage moving onto the fit object, [ferx-r#228](https://github.com/FeRx-NLME/ferx-r/issues/228), could affect how the Convergence tab locates its trace CSV — blocked upstream on `ferx-core#640`, not yet merged either).

---

### v0.4.0 (2026-06-10) — VPC appearance theming

**VPC plot theming (mirrors the `vpc` package `new_vpc_theme()`)**
- New **Theme** controls in the VPC Appearance panel: simulated prediction-interval and median band opacity; observed median and 5th/95th line width and style (solid/dashed/dotted); observed point size; bin-separator toggle and colour; and LLOQ/ULOQ reference-line colour.
- Theme applies **live** to the native egui render (display-only — no re-simulation) and is forwarded to the R ggplot export via `vpc::new_vpc_theme()`, so the exported publication figure matches what's on screen.
- Defaults mirror the package (`sim_pi` opacity 0.15, `sim_median` opacity 0.30, solid observed median, dashed 5th/95th). A **Reset appearance** button restores them.
- Bin separators are now controlled by their own toggle (previously tied to the vertical-grid checkbox), matching the package's treatment of them as a distinct theme element.

**Notes on scope**
- Categorical VPC (`vpc_cat`) and time-to-event VPC (`vpc_tte`) were investigated and found to be blocked upstream in `ferx-core` (no categorical/count outcome likelihood; TTE simulation lacks horizon censoring). Tracking drafts live under `design/`. Theming was the only one of the three remaining `vpc` features buildable today.

### v0.3.0 (2026-06-10) — statistical fixes, security hardening, performance

**Statistical correctness**
- **LRT in Compare dialog**: degrees of freedom are now computed from the difference in estimated-parameter counts between the two models, with a chi-square critical-value table for df 1–20. The previous code had an unreachable branch and always reported 1 df regardless of model complexity.
- **Log-scale GOF toggle**: the "Log scale" checkbox in the Evaluation tab now actually log₁₀-transforms the plotted points, the LOESS trendline, and the identity reference line. Non-positive observations are filtered out. Previously the toggle had no visual effect.
- **Parameter comparison tooltip**: the ÷ (shrinkage) branch now shows the reciprocal ratio for parameters that decreased from initial to final estimate; previously both branches showed the same ×ratio string.

**Security**
- Notification strings containing special characters (e.g. `"` on macOS, `'` on Windows) are now properly escaped before insertion into AppleScript / PowerShell command strings.

**Performance**
- `push_log` rebuilt the full cached log string on every incoming line (O(n²) with a 5,000-line ring buffer). The common path is now an O(1) append; the buffer-full eviction path is O(n) drain of the front entry — a significant improvement when R floods stdout.
- Windows orphan-run liveness poll throttled from 100 ms to 500 ms, eliminating ~10 `tasklist` subprocess spawns per second per reconnected run.

**Code quality**
- Removed dead `ModelUpdated` (1,256-byte enum variant and the source of the `large_enum_variant` clippy warning), `VersionCheckResult`, and the phantom "Update available" header badge — no version check was ever implemented.
- `RunRecord` in `WorkerMsg::RunFinished` is now boxed; reduces the channel message size from 296 to ~72 bytes.
- Removed unused `window_geometry` field from `Settings` (was serialised but never read back).
- Sim Plot `==`/`!=` column filters now use a 1 × 10⁻⁹ relative epsilon instead of exact float equality.
- Log-axis label in Sim Plot uses portable ASCII `log10(...)` instead of Unicode subscript characters that may be absent from the bundled font.
- Guarded `.unwrap()` calls replaced with `match`/`if let`/`.expect()`.
- 63 clippy warnings resolved (from 69 down to 6); remaining 6 are pre-existing `too_many_arguments` and one `dead_code` field.

---

### v0.2.1 (2026-06-07) — UI polish and bug fixes

**Models tab — project bookmarks redesign**
- Replaced the ambiguous ☆/★ bookmark icon (which clashed visually with the per-model ★ column) with an explicit **"+ Bookmark project" / "✓ Bookmark project"** pill button.
- Adding a bookmark now opens a name dialog so the project can be given a meaningful label; the directory path is shown as context, Enter confirms, Escape cancels.
- The bookmarks dropdown is renamed to **Projects**.

**Bookmark dialog bug fixes (from code review)**
- Switching directories while the name dialog was open could bookmark the wrong directory; `set_directory` now closes the dialog.
- Pressing Enter with an empty name field no longer silently dismisses the dialog without saving.
- Escape key now dismisses the dialog (parity with all other dialogs).

---

### v0.2.0 (2026-06-06) — bug-fix release

**Security**
- Fixed ZIP path traversal in `.fitrx` reader: entry names containing `..` or absolute paths are now rejected.
- Added per-entry and per-R-output size limits (256 MB / 10 MB) to prevent memory exhaustion.

**Correctness**
- SIR: fixed duplicate auto-trigger when a run finished while SIR was already running; added cancellation support (closing the popup now stops a queued SIR sleep).
- SIR: fixed context-string ambiguity that caused error messages to be attributed to the wrong run.
- VPC bins: NaN and Inf are no longer accepted as valid manual bin values.
- VPC R script: emits a warning when a requested stratification column is not found in the data.
- Worker threads: panics are now caught and reported as run errors instead of leaving the run stuck.
- `.ferx` model files with non-UTF-8 bytes are loaded with replacement characters instead of silently skipped.
- Non-UTF-8 paths passed to Rscript now produce an explicit error rather than silent data loss.
- Windows R path search: `bin/x64` is now joined correctly on all platforms.

**Reliability**
- Corrupt `settings.json` is backed up to `.json.bak` and defaults are used with a visible warning; write failures are also surfaced.
- Startup warnings (missing home directory, corrupt settings) are shown in the status bar.
- Per-frame log-text allocation eliminated: `log_text` is rebuilt only when new lines arrive.
- Syntax highlighter in the model editor is now cached across frames.
- All `open::that()` errors (open in Finder, open log, open RStudio, etc.) are shown in the status bar.
- Duplicate model stems can no longer be added to the run queue.
- `save_settings()` and `save_bookmarks()` failures are surfaced to the status bar.
- Windows desktop notification: fixed PowerShell line-continuation character (`\` → `` ` ``).

**Cleanup**
- Removed unused `notify` (filesystem watcher) crate and dead `FsEvent` message variant.
- Removed dead `is_terminal()` method; cleaned up `#[allow(dead_code)]` attributes.

---

## Acknowledgements

- **FeRx NLME** by the FeRx-NLME team — [ferx-nlme.github.io](https://ferx-nlme.github.io/)
- **vpc R package** by Ronkeizer et al. — [vpc.ronkeizer.com](https://vpc.ronkeizer.com/)
- **egui / eframe** by Emil Ernerfeldt and contributors

---

## Licence

MIT — see [LICENSE](LICENSE).
