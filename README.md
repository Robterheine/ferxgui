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
| **ferx** R package | 0.1.5 | See installation below |
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
