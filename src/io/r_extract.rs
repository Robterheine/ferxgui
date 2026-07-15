/// Run small R scripts via `Rscript --vanilla` and parse their JSON output.
///
/// Scripts are embedded as raw string literals so the binary is self-contained.
/// Both `inspect_model` and `compute_vpc` are blocking and must be called from
/// a background thread.
use std::path::Path;

use crate::domain::{CheckInitResult, CovScreenResult, EtaCovResult, RModelInfo, SimRunConfig, SimRunResult, SirCi, SirResult, VpcConfig, VpcResult};

// ---------------------------------------------------------------------------
// Embedded R scripts
// ---------------------------------------------------------------------------

const INSPECT_R: &str = r#"
args <- commandArgs(trailingOnly = TRUE)
if (length(args) < 1) stop("usage: inspect.R <model_path>")
model_path <- args[1]

suppressMessages(suppressWarnings(library(ferx)))
suppressMessages(suppressWarnings(library(jsonlite)))

# ferx_model_inspect() emits a human-readable summary to stdout (via cat/print).
# capture.output() intercepts that stdout so it never reaches the pipe.
# invisible() suppresses auto-printing of capture.output()'s return value
# (the captured lines as a character vector) which would otherwise also
# contaminate stdout in non-interactive Rscript mode.
invisible(capture.output(info <- ferx_model_inspect(model_path)))

nullable <- function(x) if (is.null(x) || length(x) == 0) list() else as.list(as.character(x))

out <- list(
  model_type  = if (is.null(info$model_type)  || length(info$model_type)  == 0) ""
                else paste(as.character(info$model_type), collapse = " "),
  theta_names = nullable(info$theta_names),
  iiv         = nullable(info$iiv),
  residual    = if (is.null(info$residual) || length(info$residual) == 0) ""
                else paste(as.character(info$residual), collapse = " ")
)

cat(toJSON(out, auto_unbox = TRUE))
"#;

const CHECK_INIT_R: &str = r#"
args <- commandArgs(trailingOnly = TRUE)
if (length(args) < 2) stop("usage: check_init.R <model_path> <data_path>")
model_path <- args[1]
data_path  <- args[2]

suppressMessages(suppressWarnings(library(ferx)))
suppressMessages(suppressWarnings(library(jsonlite)))

chk <- ferx_check_init(model_path, data_path, method = "focei")
s   <- chk$summary

finite_or_null <- function(x) if (is.finite(x)) x else NULL

out <- list(
  n_iter    = as.integer(s$n_iter),
  ofv_start = finite_or_null(s$ofv_start),
  ofv_end   = finite_or_null(s$ofv_end),
  ofv_drop  = finite_or_null(s$ofv_drop),
  converged = isTRUE(s$converged)
)
cat(toJSON(out, auto_unbox = TRUE, na = "null"))
"#;

/// Run script — spawned (detached) to fit a model and write a `.fitrx` bundle.
/// Args: <model> <data> <method> <covariance> <out.fitrx> [gradient] [settings_json] [threads] [optimizer_trace]
pub const RUN_FERX_R: &str = r#"
args <- commandArgs(trailingOnly = TRUE)
if (length(args) < 5) stop("usage: run_ferx.R <model> <data> <method> <covariance> <out.fitrx> [gradient] [settings_json] [threads] [optimizer_trace]")
model_path <- args[1]
data_path  <- args[2]
method_raw <- args[3]
covariance <- tolower(args[4]) == "true"
out_path   <- args[5]

suppressMessages(library(ferx))
suppressMessages(library(jsonlite))

# Method chain: "saem+focei" -> c("saem", "focei")
method <- trimws(strsplit(method_raw, "\\+")[[1]])

cat(sprintf("[ferxgui] fitting %s  (method=%s, covariance=%s)\n",
            basename(model_path), paste(method, collapse="+"), covariance))
flush(stdout())

gradient        <- if (length(args) >= 6 && nchar(args[6]) > 0) args[6] else "auto"
settings_val    <- if (length(args) >= 7 && nchar(args[7]) > 0) jsonlite::fromJSON(args[7]) else NULL
threads_n       <- if (length(args) >= 8 && nchar(args[8]) > 0) as.integer(args[8]) else NULL
optimizer_trace <- if (length(args) >= 9 && args[9] == "true") TRUE else FALSE

fit <- ferx_fit(model = model_path, data = data_path,
                method = method, covariance = covariance,
                gradient = gradient,
                threads = threads_n,
                optimizer_trace = optimizer_trace,
                settings = settings_val)
ferx_save_fit(fit, out_path)
cat(sprintf("[ferxgui] saved %s\n", out_path))
"#;

// VPC bridge: all statistics are computed by the `vpc` package (vpcdb = TRUE);
// this script only fits/simulates (caching the sim dataset) and hands the
// package's computed tables back as JSON. Takes one arg: a JSON config file.
const VPC_R: &str = r#"
args <- commandArgs(trailingOnly = TRUE)
if (length(args) < 1) stop("usage: vpc.R <config_json_path>")

emit_error <- function(kind, msg) {
  cat(jsonlite::toJSON(list(error = msg, error_kind = kind), auto_unbox = TRUE))
  quit(save = "no", status = 0)
}

if (!requireNamespace("jsonlite", quietly = TRUE))
  stop("jsonlite package not available")
if (!requireNamespace("ferx", quietly = TRUE))
  emit_error("ferx_not_installed", "The 'ferx' package is not installed.")
if (!requireNamespace("vpc", quietly = TRUE))
  emit_error("vpc_not_installed", "The 'vpc' package is not installed. Install it in R with install.packages(\"vpc\").")

suppressMessages(suppressWarnings({ library(ferx); library(vpc); library(jsonlite) }))

cfg <- jsonlite::fromJSON(args[1])

# ---- Fit + simulate, cached by config hash so option tweaks are cheap -------
sim_dat <- NULL; obs <- NULL
if (!is.null(cfg$cache_path) && file.exists(cfg$cache_path)) {
  cached  <- readRDS(cfg$cache_path)
  obs     <- cached$obs
  sim_dat <- cached$sim
} else {
  fit <- if (!is.null(cfg$fitrx_path) && file.exists(cfg$fitrx_path)) {
    ferx_load_fit(cfg$fitrx_path)
  } else {
    invisible(ferx_fit(cfg$model_path, cfg$data_path))
  }
  sim_dat <- invisible(ferx_simulate(cfg$model_path, cfg$data_path,
                                     n_sim = cfg$n_sim, seed = cfg$seed, fit = fit))
  obs <- fit$sdtab
  if (!is.null(cfg$cache_path)) {
    dir.create(dirname(cfg$cache_path), showWarnings = FALSE, recursive = TRUE)
    # Write-then-rename so a concurrent reader (another compute/export call
    # racing on the same cache path) only ever sees the old complete file or
    # the new complete file, never a partial write.
    tmp_cache <- paste0(cfg$cache_path, ".tmp.", Sys.getpid())
    # invisible() matters here: file.rename() returns a visible logical, and
    # this whole if/else is itself a bare top-level statement, so its result
    # would otherwise auto-print (e.g. "[1] TRUE") to stdout ahead of this
    # script's intended output — breaking a strict JSON parse on the Rust
    # side the moment a fresh (uncached) simulation is computed.
    invisible(tryCatch({
      saveRDS(list(obs = obs, sim = sim_dat), tmp_cache)
      file.rename(tmp_cache, cfg$cache_path)
    }, error = function(e) { unlink(tmp_cache); NULL }))
  }
}

names(obs)     <- tolower(names(obs))
names(sim_dat) <- tolower(names(sim_dat))
dv_col <- if ("dv_sim" %in% names(sim_dat)) "dv_sim" else "dv"

# ---- Stratification: merge columns from the original data if needed --------
vpc_warnings <- character(0)
strat_arg <- NULL
if (!is.null(cfg$stratify) && length(cfg$stratify) > 0) {
  strat_cols <- cfg$stratify[nchar(trimws(cfg$stratify)) > 0]
  if (length(strat_cols) > 0) {
    orig <- tryCatch({ d <- read.csv(cfg$data_path); names(d) <- tolower(names(d)); d },
                     error = function(e) NULL)
    if (!is.null(orig)) {
      for (col in strat_cols) {
        if (col %in% names(orig)) {
          if (!col %in% names(obs)) {
            key <- intersect(c("id", "time"), names(orig))
            m   <- unique(orig[, c(key, col), drop = FALSE])
            obs     <- merge(obs,     m, by = key, all.x = TRUE, suffixes = c("", ".z"))
            sim_dat <- merge(sim_dat, m, by = key, all.x = TRUE, suffixes = c("", ".z"))
            names(obs)     <- gsub("\\.z$", "", names(obs))
            names(sim_dat) <- gsub("\\.z$", "", names(sim_dat))
          }
        }
      }
    }
    strat_arg <- strat_cols[strat_cols %in% names(obs) & strat_cols %in% names(sim_dat)]
    missing_cols <- setdiff(strat_cols, strat_arg)
    if (length(missing_cols) > 0) {
      vpc_warnings <- c(vpc_warnings, paste0(
        "Stratification column(s) not found in data: ",
        paste(missing_cols, collapse = ", "), " — stratification ignored for these columns."
      ))
    }
    if (length(strat_arg) == 0) strat_arg <- NULL
  }
}

# ---- pcVPC: expose PRED from sdtab to the vpc package ---------------------
obs_cols_list <- list(dv = "dv", idv = "time", id = "id")
sim_cols_list <- list(dv = dv_col, idv = "time", id = "id", sim = "sim")
if (isTRUE(cfg$pred_corr)) {
  if ("pred" %in% names(obs)) {
    obs_cols_list$pred <- "pred"
  } else {
    emit_error("pred_corr_no_pred",
      "Prediction-corrected VPC requires PRED in the fit output (sdtab). Ensure the model converged and produces PRED.")
  }
}

# ---- Binning argument -------------------------------------------------------
bins_arg <- if (identical(cfg$bins_type, "manual") && length(cfg$manual_bins) > 0)
  as.numeric(cfg$manual_bins) else cfg$bins_type

# ---- Route to vpc() or vpc_cens() ------------------------------------------
vpc_type  <- if (!is.null(cfg$vpc_type)) cfg$vpc_type else "continuous"
lloq_val  <- if (!is.null(cfg$lloq)  && !is.na(as.numeric(cfg$lloq)))  as.numeric(cfg$lloq)  else NULL
uloq_val  <- if (!is.null(cfg$uloq)  && !is.na(as.numeric(cfg$uloq)))  as.numeric(cfg$uloq)  else NULL
facet_val <- if (!is.null(cfg$facet) && nchar(cfg$facet) > 0) cfg$facet else "wrap"

db <- if (identical(vpc_type, "censored")) {
  tryCatch(suppressWarnings(vpc::vpc_cens(
    sim = sim_dat, obs = obs,
    obs_cols = obs_cols_list,
    sim_cols = sim_cols_list,
    lloq     = lloq_val,
    uloq     = uloq_val,
    bins     = bins_arg,
    n_bins   = cfg$n_bins,
    ci       = c(cfg$ci_lo, cfg$ci_hi),
    stratify = strat_arg,
    facet    = facet_val,
    smooth   = isTRUE(cfg$smooth),
    vpcdb    = TRUE, verbose = FALSE
  )), error = function(e) emit_error("vpc_failed", conditionMessage(e)))
} else {
  tryCatch(suppressWarnings(vpc::vpc(
    sim = sim_dat, obs = obs,
    obs_cols = obs_cols_list,
    sim_cols = sim_cols_list,
    bins     = bins_arg,
    n_bins   = cfg$n_bins,
    pi       = c(cfg$pi_lo, cfg$pi_hi),
    ci       = c(cfg$ci_lo, cfg$ci_hi),
    pred_corr            = isTRUE(cfg$pred_corr),
    pred_corr_lower_bnd  = as.numeric(cfg$pred_corr_lower_bnd),
    stratify = strat_arg,
    facet    = facet_val,
    log_y    = isTRUE(cfg$log_y),
    vpcdb    = TRUE, verbose = FALSE
  )), error = function(e) emit_error("vpc_failed", conditionMessage(e)))
}

obs_pts <- data.frame(time = obs[["time"]], dv = obs[["dv"]])

result <- list(
  vpc_dat     = db$vpc_dat,
  aggr_obs    = db$aggr_obs,
  bins        = as.numeric(db$bins),
  obs_points  = obs_pts,
  vpc_mode    = vpc_type,
  lloq        = if (!is.null(lloq_val)) lloq_val else NA_real_,
  uloq        = if (!is.null(uloq_val)) uloq_val else NA_real_,
  strat_names = if (!is.null(strat_arg)) as.list(strat_arg) else list(),
  pi_lo       = cfg$pi_lo,
  pi_hi       = cfg$pi_hi,
  warnings    = as.list(vpc_warnings)
)

cat(toJSON(result, auto_unbox = TRUE, na = "null", digits = 6))
"#;

// Simulation bridge: calls ferx_simulate() (basis = initial estimates when
// cfg$fitrx_path is absent, fitted estimates via ferx_load_fit() otherwise)
// and writes the result CSV directly from R (no JSON round-trip — the merged
// table can be large: n_sim * n_input_rows).
//
// ferx_simulate()'s own return is limited to observation-time rows (SIM, ID,
// TIME, CMT, IPRED, DV_SIM, plus a DRAW column even for the non-uncertainty
// case) — it does not carry covariates, EVID, AMT, etc. from the input
// dataset. To give the GUI a table with the full original row (covariates,
// EVID, CMT, ...) alongside the simulated values, this script replicates the
// original input dataset once per SIM value, then left-joins IPRED/DV_SIM
// onto matching rows by (SIM, ID, TIME[, CMT]). Dose rows (EVID = 1) have no
// simulated counterpart (ferx_simulate() only predicts at observation times)
// and are kept with IPRED/DV_SIM blank, so EVID remains meaningful in the
// output and dosing rows stay visible/filterable downstream.
const SIM_R: &str = r#"
args <- commandArgs(trailingOnly = TRUE)
if (length(args) < 1) stop("usage: simulate.R <config_json_path>")

emit_error <- function(kind, msg) {
  cat(jsonlite::toJSON(list(error = msg, error_kind = kind), auto_unbox = TRUE))
  quit(save = "no", status = 0)
}

if (!requireNamespace("jsonlite", quietly = TRUE))
  stop("jsonlite package not available")
if (!requireNamespace("ferx", quietly = TRUE))
  emit_error("ferx_not_installed", "The 'ferx' package is not installed.")

suppressMessages(suppressWarnings({ library(ferx); library(jsonlite) }))

cfg <- jsonlite::fromJSON(args[1])

uncertainty_method <- if (!is.null(cfg$uncertainty_method) && nchar(cfg$uncertainty_method) > 0)
  cfg$uncertainty_method else NULL

fit <- NULL
if (!is.null(cfg$fitrx_path) && nchar(cfg$fitrx_path) > 0) {
  if (!file.exists(cfg$fitrx_path))
    emit_error("fitrx_missing", paste0("Fit bundle not found: ", cfg$fitrx_path))
  fit <- tryCatch(ferx_load_fit(cfg$fitrx_path),
                   error = function(e) emit_error("fitrx_load_failed", conditionMessage(e)))
}

if (!is.null(uncertainty_method) && is.null(fit))
  emit_error("fitrx_required", "Parameter-uncertainty simulation requires a saved fit (.fitrx).")

# ferx_load_fit() does not restore SIR resamples (unlike theta/omega/sigma/
# cov_matrix, which round-trip fine) — the GUI already has them cached from
# the SIR tab's own run, so they're sent inline in the config and re-injected
# here rather than re-running SIR.
if (identical(uncertainty_method, "sir")) {
  if (is.null(cfg$sir_resamples_flat) || length(cfg$sir_resamples_flat) == 0 ||
      is.null(cfg$sir_resamples_n) || is.null(cfg$sir_resamples_dim))
    emit_error("sir_resamples_required",
      "SIR uncertainty simulation requires SIR results with samples kept — run SIR in the SIR tab first (Keep resamples enabled).")
  fit$sir_resamples     <- as.numeric(cfg$sir_resamples_flat)
  fit$sir_resamples_n   <- as.integer(cfg$sir_resamples_n)
  fit$sir_resamples_dim <- as.integer(cfg$sir_resamples_dim)
}

sim <- if (!is.null(uncertainty_method)) {
  tryCatch(
    ferx_simulate_with_uncertainty(cfg$model_path, cfg$data_path, fit,
      n_uncertainty_draws = cfg$n_uncertainty_draws, n_sim_per_draw = cfg$n_sim_per_draw,
      method = uncertainty_method, seed = cfg$seed),
    error = function(e) emit_error("simulate_failed", conditionMessage(e))
  )
} else {
  tryCatch(
    ferx_simulate(cfg$model_path, cfg$data_path, n_sim = cfg$n_sim, seed = cfg$seed, fit = fit),
    error = function(e) emit_error("simulate_failed", conditionMessage(e))
  )
}
names(sim) <- toupper(names(sim))

orig <- tryCatch(read.csv(cfg$data_path), error = function(e) emit_error("data_read_failed", conditionMessage(e)))
names(orig) <- toupper(names(orig))

# "Replicate dimensions": SIM alone for the plain path, DRAW+SIM when
# ferx_simulate_with_uncertainty() adds a parameter-draw dimension. The rest
# of the merge is identical either way — the original dataset is replicated
# once per unique combination of these columns, then IPRED/DV_SIM are
# left-joined onto matching rows by (replicate dims..., ID, TIME[, CMT]).
rep_dims <- intersect(c("DRAW", "SIM"), names(sim))
join_key <- c(rep_dims, "ID", "TIME")
if ("CMT" %in% names(orig) && "CMT" %in% names(sim)) join_key <- c(join_key, "CMT")

sim_keep_cols <- intersect(c(rep_dims, "ID", "TIME", "CMT", "IPRED", "DV_SIM"), names(sim))
sim_keep <- sim[, sim_keep_cols, drop = FALSE]

rep_combos <- unique(sim_keep[, rep_dims, drop = FALSE])
expanded <- do.call(rbind, lapply(seq_len(nrow(rep_combos)), function(i) {
  d <- orig
  for (col in rep_dims) d[[col]] <- rep_combos[i, col]
  d
}))

merged <- merge(expanded, sim_keep, by = join_key, all.x = TRUE)
out_cols <- c(setdiff(names(orig), rep_dims), rep_dims, "IPRED", "DV_SIM")
merged <- merged[, out_cols]
merged <- merged[do.call(order, merged[, c(rep_dims, "ID", "TIME")]), ]

dir.create(dirname(cfg$out_path), showWarnings = FALSE, recursive = TRUE)
write.csv(merged, cfg$out_path, row.names = FALSE, na = "")

result <- list(
  out_path = cfg$out_path,
  n_rows   = nrow(merged),
  columns  = as.list(names(merged))
)
cat(toJSON(result, auto_unbox = TRUE))
"#;

const SIR_R: &str = r#"
args <- commandArgs(trailingOnly = TRUE)
if (length(args) < 4) stop("usage: sir.R <fitrx_path> <sir_samples> <sir_resamples> <sir_seed> [keep_samples]")
fitrx_path    <- args[1]
sir_samples   <- as.integer(args[2])
sir_resamples <- as.integer(args[3])
sir_seed      <- as.integer(args[4])
keep_samples  <- if (length(args) >= 5) tolower(args[5]) == "true" else TRUE

suppressMessages(library(ferx))
suppressMessages(library(jsonlite))

fit     <- ferx_load_fit(fitrx_path)
sir_fit <- ferx_sir(fit,
  sir_samples      = sir_samples,
  sir_resamples    = sir_resamples,
  sir_seed         = sir_seed,
  sir_keep_samples = keep_samples)

ci_group <- function(m) {
  if (is.null(m) || nrow(m) == 0)
    return(list(names = list(), lo = list(), hi = list()))
  list(
    names = as.list(rownames(m)),
    lo    = as.list(as.numeric(m[, "lower"])),
    hi    = as.list(as.numeric(m[, "upper"]))
  )
}

result <- list(
  sir_ess = sir_fit$sir_ess,
  theta   = ci_group(sir_fit$sir_ci_theta),
  omega   = ci_group(sir_fit$sir_ci_omega),
  sigma   = ci_group(sir_fit$sir_ci_sigma)
)

# Correlation matrix + per-parameter samples (when resamples were kept).
if (keep_samples &&
    !is.null(sir_fit$sir_resamples) &&
    !is.null(sir_fit$sir_resamples_n) &&
    isTRUE(sir_fit$sir_resamples_n > 0)) {

  n_r   <- sir_fit$sir_resamples_n
  n_dim <- sir_fit$sir_resamples_dim

  # Raw (unconstrained-parameterisation) resamples, persisted as-is —
  # ferx_simulate_with_uncertainty(method = "sir") needs exactly this shape
  # re-injected onto a fit object later (ferx_load_fit() doesn't restore it),
  # so this must NOT go through the natural-scale back-transform below.
  result$sir_resamples_flat <- as.list(as.numeric(sir_fit$sir_resamples))
  result$sir_resamples_n    <- n_r
  result$sir_resamples_dim  <- n_dim

  # Resamples are packed row-major: [resample1_p1, resample1_p2, ..., resample2_p1, ...]
  mat <- matrix(sir_fit$sir_resamples, nrow = n_r, ncol = n_dim, byrow = TRUE)

  # Build canonical parameter name vector: theta, omega diagonal, sigma.
  theta_names <- names(sir_fit$theta)
  n_eta <- nrow(sir_fit$omega)
  en    <- sir_fit$eta_names
  omega_names <- if (!is.null(en) && length(en) == n_eta) en
                 else paste0("OMEGA(", seq_len(n_eta), ",", seq_len(n_eta), ")")
  sn    <- sir_fit$sigma_names
  sigma_names <- if (!is.null(sn) && length(sn) == length(sir_fit$sigma))
                   as.character(sn)
                 else paste0("SIGMA(", seq_along(sir_fit$sigma), ")")
  all_names <- c(theta_names, omega_names, sigma_names)[seq_len(n_dim)]

  # Back-transform from ferx's internal unconstrained parameterisation to
  # natural scale so histograms match the point estimates and CIs:
  #   Theta (positive-bounded): log → exp(x)
  #   Omega diagonal:           log-Cholesky = 0.5*log(var) → exp(2*x)
  #   Sigma:                    log(sigma_SD) → exp(x)
  n_theta_params <- length(theta_names)
  n_omega_params <- min(n_eta, n_dim - n_theta_params)
  n_sigma_params <- n_dim - n_theta_params - n_omega_params

  if (n_theta_params > 0) {
    mat[, seq_len(n_theta_params)] <- exp(mat[, seq_len(n_theta_params)])
  }
  if (n_omega_params > 0) {
    omega_cols <- seq(n_theta_params + 1, n_theta_params + n_omega_params)
    mat[, omega_cols] <- exp(2 * mat[, omega_cols])
  }
  if (n_sigma_params > 0) {
    sigma_cols <- seq(n_theta_params + n_omega_params + 1, n_dim)
    mat[, sigma_cols] <- exp(mat[, sigma_cols])
  }

  # Empirical correlation matrix (fall back to identity on error).
  corr <- tryCatch(cor(mat), error = function(e) diag(n_dim))

  # Per-parameter marginal samples as named lists.
  colnames(mat) <- all_names
  param_samples <- setNames(
    lapply(seq_len(n_dim), function(i) as.list(mat[, i])),
    all_names
  )

  result$corr_names    <- as.list(all_names)
  result$corr_dim      <- n_dim
  result$corr_flat     <- as.list(as.numeric(corr))  # row-major
  result$param_samples <- param_samples
}

cat(toJSON(result, auto_unbox = TRUE, na = "null"))
"#;

/// GOF export script.
/// Args: <data_csv> <output_path> <format> <width_mm> <cwres_x_1> <cwres_x_2> <loess> <ci_lines>
const GOF_EXPORT_R: &str = r#"
args <- commandArgs(trailingOnly = TRUE)
data_path      <- args[1]
output_path    <- args[2]
format_str     <- args[3]   # "pdf" | "png300" | "png600" | "svg"
width_mm       <- as.numeric(args[4])
cwres_x_1      <- args[5]
cwres_x_2      <- args[6]
include_loess  <- tolower(args[7]) == "true"
include_ci     <- tolower(args[8]) == "true"

data     <- read.csv(data_path)
height_mm <- width_mm           # square layout
w_in      <- width_mm  / 25.4
h_in      <- height_mm / 25.4
dpi       <- if (format_str == "png600") 600 else 300

# Choose rendering backend.
use_gg <- requireNamespace("ggplot2",   quietly = TRUE) &&
          requireNamespace("patchwork", quietly = TRUE)

safe_col <- function(df, col, fallback) {
  if (col %in% names(df)) col else fallback
}
x1 <- safe_col(data, cwres_x_1, "TIME")
x2 <- safe_col(data, cwres_x_2, "PRED")

if (use_gg) {
  suppressPackageStartupMessages({
    library(ggplot2)
    library(patchwork)
  })
  th <- theme_bw(base_size = 9) +
    theme(panel.grid.minor = element_blank(),
          strip.background = element_blank(),
          plot.margin = unit(c(2,2,2,2), "mm"))

  loess_lyr <- if (include_loess)
    geom_smooth(method = "loess", se = FALSE, color = "darkorange2",
                linewidth = 0.9, formula = y ~ x)
  else NULL

  ci_lyrs <- if (include_ci)
    list(geom_hline(yintercept =  2, linetype = "dashed", color = "gray50", linewidth = 0.5),
         geom_hline(yintercept = -2, linetype = "dashed", color = "gray50", linewidth = 0.5))
  else list()

  mk_pts <- function(xv, yv) {
    df <- data[is.finite(data[[xv]]) & is.finite(data[[yv]]), ]
    aes_map <- aes_string(x = xv, y = yv)
    list(df = df, aes = aes_map)
  }
  d1 <- mk_pts("PRED",  "DV");  d2 <- mk_pts("IPRED", "DV")
  d3 <- mk_pts(x1, "CWRES");   d4 <- mk_pts(x2, "CWRES")

  identity_line <- geom_abline(slope = 1, intercept = 0, color = "gray50", linewidth = 0.8)
  zero_line     <- geom_hline(yintercept = 0,             color = "gray50", linewidth = 0.8)

  p1 <- ggplot(d1$df, d1$aes) + geom_point(alpha = 0.4, size = 1) + identity_line + loess_lyr + th + labs(x = "PRED",  y = "DV")
  p2 <- ggplot(d2$df, d2$aes) + geom_point(alpha = 0.4, size = 1) + identity_line + loess_lyr + th + labs(x = "IPRED", y = "DV")
  p3 <- ggplot(d3$df, d3$aes) + geom_point(alpha = 0.4, size = 1) + zero_line + ci_lyrs + loess_lyr + th + labs(x = x1, y = "CWRES")
  p4 <- ggplot(d4$df, d4$aes) + geom_point(alpha = 0.4, size = 1) + zero_line + ci_lyrs + loess_lyr + th + labs(x = x2, y = "CWRES")

  fig <- (p1 | p2) / (p3 | p4)
  dev_str <- switch(format_str, "pdf" = "pdf", "svg" = "svg", "png")
  suppressMessages(
    ggsave(output_path, fig, width = width_mm, height = height_mm,
           units = "mm", dpi = dpi, device = dev_str)
  )
} else {
  # ── Base R fallback (always available) ──────────────────────────────────
  open_dev <- function(path, fmt, w, h, dpi) {
    switch(fmt,
      "pdf" = pdf(path, width = w, height = h),
      "svg" = svg(path, width = w, height = h),
             png(path, width = width_mm, height = height_mm, units = "mm", res = dpi)
    )
  }
  open_dev(output_path, format_str, w_in, h_in, dpi)
  op <- par(mfrow = c(2, 2), mar = c(4, 4, 1.5, 1))

  mk_gof <- function(xv, yv, xlab, ylab, refline = "identity") {
    df <- data[is.finite(data[[xv]]) & is.finite(data[[yv]]), ]
    plot(df[[xv]], df[[yv]], xlab = xlab, ylab = ylab,
         pch = 16, cex = 0.5, col = rgb(0, 0, 0, 0.4))
    if (refline == "identity") abline(0, 1, col = "gray50", lwd = 1.5)
    else                       abline(h = 0, col = "gray50", lwd = 1.5)
    if (include_ci && refline != "identity") {
      abline(h =  2, lty = 2, col = "gray60")
      abline(h = -2, lty = 2, col = "gray60")
    }
    if (include_loess && nrow(df) > 4) {
      lo <- tryCatch(loess(as.formula(paste(yv, "~", xv)), data = df),
                     error = function(e) NULL)
      if (!is.null(lo)) {
        xg <- seq(min(df[[xv]]), max(df[[xv]]), length.out = 100)
        yg <- predict(lo, newdata = setNames(data.frame(xg), xv))
        lines(xg, yg, col = "darkorange2", lwd = 1.5)
      }
    }
  }
  mk_gof("PRED",  "DV",     "PRED",  "DV")
  mk_gof("IPRED", "DV",     "IPRED", "DV")
  mk_gof(x1,      "CWRES",  x1,      "CWRES", refline = "zero")
  mk_gof(x2,      "CWRES",  x2,      "CWRES", refline = "zero")
  par(op)
  # invisible() matters here: dev.off() visibly returns the newly-current
  # device's number/name (e.g. "null device \n 1"), which would otherwise
  # auto-print ahead of cat(output_path) below — and export_gof() on the
  # Rust side treats the *entire* captured stdout as the output path, so
  # that stray text would corrupt the path it hands back.
  invisible(dev.off())
}
cat(output_path)
"#;

// ferx-r >= 0.2.0 removed the exported ferx_eta_cov(fit, data) function
// (FeRx-NLME/ferx-r#226): the ETA-covariate screen is now computed
// automatically at fit time (and recomputed by ferx_load_fit() from the
// dataset path recorded on the fit) and exposed as the fit$eta_cov field.
const ETA_COV_R: &str = r#"
args <- commandArgs(trailingOnly = TRUE)
if (length(args) < 1) stop("usage: eta_cov.R <fitrx_path>")
fitrx_path <- args[1]

suppressMessages(library(ferx))
suppressMessages(library(jsonlite))

fit    <- ferx_load_fit(fitrx_path)
result <- fit$eta_cov

if (is.null(result) || !is.data.frame(result) || nrow(result) == 0) {
  # fit$eta_cov is NULL for two distinct reasons: a legitimate "nothing to
  # screen" result (too few subjects / no numeric covariates), or the
  # original dataset could no longer be read from its recorded path. Tell
  # these apart so the GUI doesn't show a misleading "no pairs found"
  # message when the real cause is a moved/missing data file.
  data_path    <- tryCatch(fit$data_path, error = function(e) NULL)
  data_missing <- !is.null(data_path) && length(data_path) == 1 &&
                  !is.na(data_path) && nzchar(data_path) && !file.exists(data_path)
  cat(toJSON(list(rows = list(), data_unavailable = data_missing), auto_unbox = TRUE))
} else {
  rows <- lapply(seq_len(nrow(result)), function(i) {
    r_v   <- result$r[i]
    p_v   <- result$p_val[i]
    flg   <- is.character(result$flag[i]) && nchar(trimws(result$flag[i])) > 0
    list(
      eta       = as.character(result$eta[i]),
      covariate = as.character(result$covariate[i]),
      r         = if (is.na(r_v))   NULL else as.numeric(r_v),
      p_val     = if (is.na(p_v))   NULL else as.numeric(p_v),
      flag      = flg
    )
  })
  cat(toJSON(list(rows = rows, data_unavailable = FALSE), auto_unbox = TRUE, na = "null"))
}
"#;

// ferx_cov_screen(fit) (ferx-r >= 0.2.0) — the formal covariate screen built
// on the model's own declared [covariates] block (fit$covtab), as opposed to
// fit$eta_cov's informal scan of every numeric dataset column. Kept public
// upstream (not folded into a fit-object field like eta_cov was), so this
// still calls it directly.
const COV_SCREEN_R: &str = r#"
args <- commandArgs(trailingOnly = TRUE)
if (length(args) < 1) stop("usage: cov_screen.R <fitrx_path>")
fitrx_path <- args[1]

suppressMessages(library(ferx))
suppressMessages(library(jsonlite))

fit <- ferx_load_fit(fitrx_path)

# Check preconditions ourselves rather than relying on ferx_cov_screen()'s
# own message()+NULL fallback, so the two distinct "not applicable" reasons
# (no declared covariates vs. no random effects at all) are told apart
# without depending on parsing its console message text.
no_covariates <- is.null(fit$covtab)   || !is.data.frame(fit$covtab)
no_etas       <- is.null(fit$ebe_etas) || !is.data.frame(fit$ebe_etas)

if (no_covariates || no_etas) {
  cat(toJSON(list(rows = list(), no_covariates = no_covariates, no_etas = no_etas),
             auto_unbox = TRUE))
} else {
  # ferx_cov_screen() prints its summary table to the console — capture it.
  invisible(capture.output(result <- ferx_cov_screen(fit)))

  if (is.null(result) || !is.data.frame(result) || nrow(result) == 0) {
    cat(toJSON(list(rows = list(), no_covariates = FALSE, no_etas = FALSE), auto_unbox = TRUE))
  } else {
    rows <- lapply(seq_len(nrow(result)), function(i) {
      ebe_v <- result$ebe[i]
      eta_v <- result$eta[i]
      list(
        parameter = as.character(result$parameter[i]),
        covariate = as.character(result$covariate[i]),
        cov_type  = as.character(result$type[i]),
        ebe       = if (is.na(ebe_v)) NULL else as.numeric(ebe_v),
        eta       = if (is.na(eta_v)) NULL else as.numeric(eta_v)
      )
    })
    cat(toJSON(list(rows = rows, no_covariates = FALSE, no_etas = FALSE),
               auto_unbox = TRUE, na = "null"))
  }
}
"#;

const CREATE_MODEL_R: &str = r#"
args <- commandArgs(trailingOnly = TRUE)
if (length(args) < 2) stop("usage: create_model.R <output_path> <template>")
out_path <- args[1]
template <- args[2]

suppressMessages(library(ferx))

ferx_model(
  template  = template,
  path      = out_path,
  edit      = FALSE,
  overwrite = TRUE,
  print     = FALSE
)
cat(out_path)
"#;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Write the detached run script to `app_dir/run_ferx.R` and return its path.
/// The file must persist for the lifetime of the detached process, so we keep
/// it in the app data directory rather than a temp file.
pub fn ensure_run_script(app_dir: &Path) -> std::io::Result<std::path::PathBuf> {
    let path = app_dir.join("run_ferx.R");
    std::fs::write(&path, RUN_FERX_R)?;
    Ok(path)
}

/// Call `ferx_model_inspect(model_path)` via R and return the parsed result.
/// Blocking — run from a background thread.
pub fn inspect_model(model_path: &Path) -> Result<RModelInfo, String> {
    let json = run_script(INSPECT_R, &[path_as_str(model_path)?])?;
    serde_json::from_str(&json)
        .map_err(|e| format!("inspect JSON parse error: {e}\nR output: {json}"))
}

/// Call `ferx_check_init()` via R for a 5-iteration pilot fit.
/// Blocking — run from a background thread.
pub fn compute_check_init(model_path: &Path, data_path: &Path) -> Result<CheckInitResult, String> {
    let json = run_script(CHECK_INIT_R, &[
        path_as_str(model_path)?,
        path_as_str(data_path)?,
    ])?;
    serde_json::from_str(&json)
        .map_err(|e| format!("check_init JSON parse error: {e}\nR output: {}", &json[..json.len().min(500)]))
}

/// Compute a VPC by delegating all statistics to the `vpc` R package
/// (`vpcdb = TRUE`). Simulates once and caches the dataset, so changing only
/// display options (PI/CI/bins) re-runs just the fast statistics step.
/// Blocking — run from a background thread.
pub fn compute_vpc(cfg: &VpcConfig) -> Result<VpcResult, String> {
    let cfg_json = serde_json::to_string(cfg)
        .map_err(|e| format!("could not serialize VPC config: {e}"))?;

    // The bridge reads its options from a JSON file (too many to pass positionally).
    let cfg_path = unique_temp_path("ferxgui_vpccfg", "json")?;
    std::fs::write(&cfg_path, cfg_json)
        .map_err(|e| format!("could not write VPC config: {e}"))?;

    let cfg_path_str = path_as_str(&cfg_path)?;
    let json = run_script(VPC_R, &[cfg_path_str]);
    let _ = std::fs::remove_file(&cfg_path);
    let json = json?;

    // The bridge reports installation / computation failures as a JSON error object.
    if let Ok(err) = serde_json::from_str::<RBridgeError>(&json) {
        return Err(err.error);
    }
    serde_json::from_str(&json)
        .map_err(|e| format!("VPC JSON parse error: {e}\nR output: {}", &json[..json.len().min(500)]))
}

/// Run `ferx_simulate()` and write the merged CSV (original data columns +
/// SIM + IPRED/DV_SIM) directly from R. Blocking — run from a background thread.
pub fn compute_simulation(cfg: &SimRunConfig) -> Result<SimRunResult, String> {
    let cfg_json = serde_json::to_string(cfg)
        .map_err(|e| format!("could not serialize simulation config: {e}"))?;

    let cfg_path = unique_temp_path("ferxgui_simcfg", "json")?;
    std::fs::write(&cfg_path, cfg_json)
        .map_err(|e| format!("could not write simulation config: {e}"))?;

    let cfg_path_str = path_as_str(&cfg_path)?;
    let json = run_script(SIM_R, &[cfg_path_str]);
    let _ = std::fs::remove_file(&cfg_path);
    let json = json?;

    if let Ok(err) = serde_json::from_str::<RBridgeError>(&json) {
        return Err(err.error);
    }
    serde_json::from_str(&json)
        .map_err(|e| format!("simulation JSON parse error: {e}\nR output: {}", &json[..json.len().min(500)]))
}

/// A structured error emitted by an R bridge script (e.g. package not installed).
#[derive(serde::Deserialize)]
struct RBridgeError {
    error: String,
    #[allow(dead_code)]
    #[serde(default)]
    error_kind: String,
}

// Renders the *actual* `vpc` ggplot to a PNG (publication-quality figure),
// reusing the same simulated-dataset cache as `compute_vpc`. Args: <config_json> <png_path>.
// Exposed as a public const so the GUI can load it into the editable script field.
pub const VPC_PLOT_R_DEFAULT: &str = r#"
args <- commandArgs(trailingOnly = TRUE)
if (length(args) < 2) stop("usage: vpc_plot.R <config_json_path> <png_path>")
png_path <- args[2]

if (!requireNamespace("ferx", quietly = TRUE))    stop("The 'ferx' package is not installed.")
if (!requireNamespace("vpc", quietly = TRUE))     stop("The 'vpc' package is not installed.")
if (!requireNamespace("ggplot2", quietly = TRUE)) stop("The 'ggplot2' package is not installed.")

suppressMessages(suppressWarnings({ library(ferx); library(vpc); library(ggplot2) }))

cfg <- jsonlite::fromJSON(args[1])

sim_dat <- NULL; obs <- NULL
if (!is.null(cfg$cache_path) && file.exists(cfg$cache_path)) {
  cached  <- readRDS(cfg$cache_path)
  obs     <- cached$obs
  sim_dat <- cached$sim
} else {
  fit <- if (!is.null(cfg$fitrx_path) && file.exists(cfg$fitrx_path)) {
    ferx_load_fit(cfg$fitrx_path)
  } else {
    invisible(ferx_fit(cfg$model_path, cfg$data_path))
  }
  sim_dat <- invisible(ferx_simulate(cfg$model_path, cfg$data_path,
                                     n_sim = cfg$n_sim, seed = cfg$seed, fit = fit))
  obs <- fit$sdtab
  if (!is.null(cfg$cache_path)) {
    dir.create(dirname(cfg$cache_path), showWarnings = FALSE, recursive = TRUE)
    # Write-then-rename so a concurrent reader (another compute/export call
    # racing on the same cache path) only ever sees the old complete file or
    # the new complete file, never a partial write.
    tmp_cache <- paste0(cfg$cache_path, ".tmp.", Sys.getpid())
    # invisible() matters here: file.rename() returns a visible logical, and
    # this whole if/else is itself a bare top-level statement, so its result
    # would otherwise auto-print (e.g. "[1] TRUE") to stdout ahead of this
    # script's intended output — breaking a strict JSON parse on the Rust
    # side the moment a fresh (uncached) simulation is computed.
    invisible(tryCatch({
      saveRDS(list(obs = obs, sim = sim_dat), tmp_cache)
      file.rename(tmp_cache, cfg$cache_path)
    }, error = function(e) { unlink(tmp_cache); NULL }))
  }
}

names(obs)     <- tolower(names(obs))
names(sim_dat) <- tolower(names(sim_dat))
dv_col <- if ("dv_sim" %in% names(sim_dat)) "dv_sim" else "dv"

# Stratification: merge columns from original data
strat_arg <- NULL
if (!is.null(cfg$stratify) && length(cfg$stratify) > 0) {
  strat_cols <- cfg$stratify[nchar(trimws(cfg$stratify)) > 0]
  if (length(strat_cols) > 0) {
    orig <- tryCatch({ d <- read.csv(cfg$data_path); names(d) <- tolower(names(d)); d },
                     error = function(e) NULL)
    if (!is.null(orig)) {
      for (col in strat_cols) {
        if (col %in% names(orig) && !col %in% names(obs)) {
          key <- intersect(c("id", "time"), names(orig))
          m   <- unique(orig[, c(key, col), drop = FALSE])
          obs     <- merge(obs,     m, by = key, all.x = TRUE, suffixes = c("", ".z"))
          sim_dat <- merge(sim_dat, m, by = key, all.x = TRUE, suffixes = c("", ".z"))
          names(obs)     <- gsub("\\.z$", "", names(obs))
          names(sim_dat) <- gsub("\\.z$", "", names(sim_dat))
        }
      }
    }
    strat_arg <- strat_cols[strat_cols %in% names(obs) & strat_cols %in% names(sim_dat)]
    if (length(strat_arg) == 0) strat_arg <- NULL
  }
}

obs_cols_list <- list(dv = "dv", idv = "time", id = "id")
sim_cols_list <- list(dv = dv_col, idv = "time", id = "id", sim = "sim")
if (isTRUE(cfg$pred_corr)) {
  if ("pred" %in% names(obs)) {
    obs_cols_list$pred <- "pred"
  } else {
    stop("Prediction-corrected VPC requires PRED in the fit output (sdtab). Ensure the model converged and produces PRED.")
  }
}

bins_arg  <- if (identical(cfg$bins_type, "manual") && length(cfg$manual_bins) > 0)
  as.numeric(cfg$manual_bins) else cfg$bins_type
vpc_type  <- if (!is.null(cfg$vpc_type)) cfg$vpc_type else "continuous"
lloq_val  <- if (!is.null(cfg$lloq)  && !is.na(as.numeric(cfg$lloq)))  as.numeric(cfg$lloq)  else NULL
uloq_val  <- if (!is.null(cfg$uloq)  && !is.na(as.numeric(cfg$uloq)))  as.numeric(cfg$uloq)  else NULL
facet_val <- if (!is.null(cfg$facet) && nchar(cfg$facet) > 0) cfg$facet else "wrap"

# Build the vpc theme from the appearance config (display-only options).
theme_upd <- list()
if (!is.null(cfg$band_color) && nchar(cfg$band_color) > 0) {
  theme_upd$sim_pi_fill     <- cfg$band_color
  theme_upd$sim_median_fill <- cfg$band_color
}
if (!is.null(cfg$sim_pi_alpha))     theme_upd$sim_pi_alpha     <- cfg$sim_pi_alpha
if (!is.null(cfg$sim_median_alpha)) theme_upd$sim_median_alpha <- cfg$sim_median_alpha
if (!is.null(cfg$obs_color) && nchar(cfg$obs_color) > 0) {
  theme_upd$obs_color        <- cfg$obs_color
  theme_upd$obs_median_color <- cfg$obs_color
  theme_upd$obs_ci_color     <- cfg$obs_color
}
if (!is.null(cfg$obs_median_linetype))  theme_upd$obs_median_linetype  <- cfg$obs_median_linetype
if (!is.null(cfg$obs_median_linewidth)) theme_upd$obs_median_linewidth <- cfg$obs_median_linewidth
if (!is.null(cfg$obs_ci_linetype))      theme_upd$obs_ci_linetype      <- cfg$obs_ci_linetype
if (!is.null(cfg$obs_ci_linewidth))     theme_upd$obs_ci_linewidth     <- cfg$obs_ci_linewidth
# Empty bin-separator colour means "hide": map to NA.
if (!is.null(cfg$bin_separators_color)) {
  theme_upd$bin_separators_color <- if (nchar(cfg$bin_separators_color) > 0) cfg$bin_separators_color else NA
}
if (!is.null(cfg$loq_color) && nchar(cfg$loq_color) > 0) theme_upd$loq_color <- cfg$loq_color
vpc_theme <- if (length(theme_upd) > 0) vpc::new_vpc_theme(update = theme_upd) else vpc::new_vpc_theme()

pl <- if (identical(vpc_type, "censored")) {
  suppressWarnings(vpc::vpc_cens(
    sim = sim_dat, obs = obs,
    obs_cols = obs_cols_list, sim_cols = sim_cols_list,
    lloq = lloq_val, uloq = uloq_val,
    bins = bins_arg, n_bins = cfg$n_bins,
    ci = c(cfg$ci_lo, cfg$ci_hi),
    stratify = strat_arg, facet = facet_val,
    smooth = isTRUE(cfg$smooth),
    show = list(obs_dv = FALSE),
    vpc_theme = vpc_theme, vpcdb = FALSE, verbose = FALSE
  ))
} else {
  suppressWarnings(vpc::vpc(
    sim = sim_dat, obs = obs,
    obs_cols = obs_cols_list, sim_cols = sim_cols_list,
    bins = bins_arg, n_bins = cfg$n_bins,
    pi = c(cfg$pi_lo, cfg$pi_hi), ci = c(cfg$ci_lo, cfg$ci_hi),
    pred_corr = isTRUE(cfg$pred_corr),
    pred_corr_lower_bnd = as.numeric(cfg$pred_corr_lower_bnd),
    stratify = strat_arg, facet = facet_val,
    log_y = isTRUE(cfg$log_y), smooth = isTRUE(cfg$smooth),
    show = list(obs_dv = isTRUE(cfg$show_points)),
    vpc_theme = vpc_theme, vpcdb = FALSE, verbose = FALSE
  ))
}

ggplot2::ggsave(png_path, plot = pl, width = 8, height = 5, dpi = 150)
cat(png_path)
"#;

/// Render the real `vpc` ggplot to a PNG (reusing the sim cache) and return its path.
/// `script` is the R script text to run (usually `VPC_PLOT_R_DEFAULT` but may be
/// user-edited). Blocking — run from a background thread.
pub fn export_vpc_plot(cfg: &VpcConfig, png_path: &Path, script: &str) -> Result<(), String> {
    let cfg_json = serde_json::to_string(cfg)
        .map_err(|e| format!("could not serialize VPC config: {e}"))?;
    let cfg_path = unique_temp_path("ferxgui_vpcplotcfg", "json")?;
    std::fs::write(&cfg_path, cfg_json)
        .map_err(|e| format!("could not write VPC config: {e}"))?;

    let cfg_str = path_as_str(&cfg_path)?;
    let png_str = path_as_str(png_path)?;
    let res = run_script(script, &[cfg_str, png_str]);
    let _ = std::fs::remove_file(&cfg_path);
    res.map(|_| ())
}

/// Quick check that the `vpc` package is installed; returns its version string.
/// Blocking but fast — run from a background thread for the status banner.
pub fn vpc_package_version() -> Result<String, String> {
    let script = r#"
if (requireNamespace("vpc", quietly = TRUE)) {
  cat(as.character(packageVersion("vpc")))
} else {
  cat("__MISSING__")
}
"#;
    let out = run_script(script, &[])?;
    if out == "__MISSING__" || out.is_empty() {
        Err("not installed".to_string())
    } else {
        Ok(out)
    }
}

/// Deterministic cache file for a VPC simulated dataset, keyed by the inputs
/// that actually affect the simulation (model, data, fit, n_sim, seed).
/// Display options (PI/CI/bins) are deliberately excluded so tweaking them
/// reuses the cache.
pub fn vpc_cache_path(model_path: &Path, data_path: &Path, fitrx_path: Option<&Path>, n_sim: u32, seed: u32) -> Result<std::path::PathBuf, String> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    model_path.hash(&mut h);
    data_path.hash(&mut h);
    n_sim.hash(&mut h);
    seed.hash(&mut h);
    // Fold in the fit bundle's mtime so a re-fit invalidates the cache.
    if let Some(fp) = fitrx_path {
        fp.hash(&mut h);
        if let Ok(meta) = std::fs::metadata(fp) {
            if let Ok(modified) = meta.modified() {
                if let Ok(dur) = modified.duration_since(std::time::UNIX_EPOCH) {
                    dur.as_secs().hash(&mut h);
                }
            }
        }
    }
    Ok(helper_temp_dir()?
        .join("ferxgui_vpc_cache")
        .join(format!("{:016x}.rds", h.finish())))
}

/// Run `ferx_sir()` via R against a saved `.fitrx` bundle.
/// Blocking — run from a background thread.
pub fn compute_sir(
    fitrx_path:   &Path,
    n_samples:    u32,
    n_resamples:  u32,
    seed:         u32,
    keep_samples: bool,
) -> Result<SirResult, String> {
    let json = run_script(SIR_R, &[
        path_as_str(fitrx_path)?,
        &n_samples.to_string(),
        &n_resamples.to_string(),
        &seed.to_string(),
        if keep_samples { "true" } else { "false" },
    ])?;
    parse_sir_result(&json)
        .map_err(|e| format!("SIR JSON parse error: {e}\nR output: {}", &json[..json.len().min(500)]))
}

fn parse_sir_result(json: &str) -> Result<SirResult, serde_json::Error> {
    use std::collections::HashMap;

    #[derive(serde::Deserialize, Default)]
    struct CiGroup {
        #[serde(default)] names: Vec<String>,
        #[serde(default)] lo:    Vec<f64>,
        #[serde(default)] hi:    Vec<f64>,
    }
    impl CiGroup {
        fn into_cis(self) -> Vec<SirCi> {
            self.names.into_iter()
                .zip(self.lo)
                .zip(self.hi)
                .map(|((name, lo), hi)| SirCi { name, lo, hi })
                .collect()
        }
    }
    #[derive(serde::Deserialize, Default)]
    struct Wire {
        #[serde(default)] sir_ess:       f64,
        #[serde(default)] theta:         CiGroup,
        #[serde(default)] omega:         CiGroup,
        #[serde(default)] sigma:         CiGroup,
        #[serde(default)] corr_names:    Vec<String>,
        #[serde(default)] corr_dim:      usize,
        #[serde(default)] corr_flat:     Vec<f64>,
        #[serde(default)] param_samples: HashMap<String, Vec<f64>>,
        #[serde(default)] sir_resamples_flat: Vec<f64>,
        #[serde(default)] sir_resamples_n:    usize,
        #[serde(default)] sir_resamples_dim:  usize,
    }
    let w: Wire = serde_json::from_str(json)?;
    Ok(SirResult {
        sir_ess:       w.sir_ess,
        theta:         w.theta.into_cis(),
        omega:         w.omega.into_cis(),
        sigma:         w.sigma.into_cis(),
        corr_names:    w.corr_names,
        corr_dim:      w.corr_dim,
        corr_flat:     w.corr_flat,
        param_samples: w.param_samples,
        sir_resamples_flat: w.sir_resamples_flat,
        sir_resamples_n:    w.sir_resamples_n,
        sir_resamples_dim:  w.sir_resamples_dim,
    })
}

/// Call `ferx_eta_cov()` via R to screen EBE ETAs against dataset covariates.
/// Blocking — run from a background thread.
pub fn compute_eta_cov(fitrx_path: &Path) -> Result<EtaCovResult, String> {
    let json = run_script(ETA_COV_R, &[path_as_str(fitrx_path)?])?;
    serde_json::from_str(&json)
        .map_err(|e| format!("eta_cov JSON parse error: {e}\nR output: {}", &json[..json.len().min(500)]))
}

pub fn compute_cov_screen(fitrx_path: &Path) -> Result<CovScreenResult, String> {
    let json = run_script(COV_SCREEN_R, &[path_as_str(fitrx_path)?])?;
    serde_json::from_str(&json)
        .map_err(|e| format!("cov_screen JSON parse error: {e}\nR output: {}", &json[..json.len().min(500)]))
}

/// Export a 4-panel GOF figure via an R/ggplot2 script.
/// `data_csv` is a path to a temporary CSV with the prediction rows.
/// Blocking — call from a background thread.
pub fn export_gof(
    data_csv:      &std::path::Path,
    output_path:   &std::path::Path,
    format:        &str,
    width_mm:      u32,
    cwres_x_1:     &str,
    cwres_x_2:     &str,
    loess:         bool,
    ci_lines:      bool,
) -> Result<String, String> {
    let out = run_script(GOF_EXPORT_R, &[
        path_as_str(data_csv)?,
        path_as_str(output_path)?,
        format,
        &width_mm.to_string(),
        cwres_x_1,
        cwres_x_2,
        if loess    { "true" } else { "false" },
        if ci_lines { "true" } else { "false" },
    ])?;
    Ok(out.trim().to_string())
}

/// Create a `.ferx` model file from a built-in template.
/// `template` is one of: "1cpt_oral", "1cpt_iv", "2cpt_oral", "2cpt_iv", "ode".
/// Blocking — run from a background thread or directly on user action.
pub fn create_model_from_template(out_path: &Path, template: &str) -> Result<(), String> {
    let _ = run_script(CREATE_MODEL_R, &[
        path_as_str(out_path)?,
        template,
    ])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Convert a `Path` to `&str`, returning a descriptive error on non-UTF-8 paths.
fn path_as_str(p: &std::path::Path) -> Result<&str, String> {
    p.to_str().ok_or_else(|| format!(
        "path contains non-UTF-8 characters and cannot be passed to Rscript: {}",
        p.display()
    ))
}

/// Returns a private, per-user subdirectory of the OS temp directory for
/// R-helper temp files, creating and permission-hardening it as needed.
///
/// Unlike writing directly into the shared OS temp root, this can't be
/// pre-planted or raced by another local user on a shared multi-user
/// machine (e.g. a central FeRx GUI server accessed over SSH/X11 — a
/// documented supported deployment, see README's "Linux SSH note"). This
/// is a hard failure, not a fallback: if the directory can't be verified
/// as safely ours, every R-helper operation that needs it fails loudly
/// with a clear message, rather than silently writing through whatever is
/// actually at this path. A silent fallback to the shared temp root would
/// make the whole control trivially defeatable — any local user could
/// pre-create this exact path themselves, once, and every subsequent
/// victim would silently get the unprotected behavior back.
///
/// Residual limitation: closing the check-then-act window between
/// verifying an existing entry and using it entirely would need
/// filesystem primitives `std::fs` doesn't expose portably (e.g.
/// `O_NOFOLLOW`-based atomic open). What's here minimizes that window
/// (one stat call, not three, spread across the smallest possible span)
/// rather than eliminating it outright; the easy, un-timed attack this
/// function exists to close (pre-creating the directory once, requiring
/// no race at all) is fully closed by the ownership check below.
///
/// No hardening needed on Windows: `%TEMP%` is already per-user by OS
/// default, so `create_dir_all` alone is sufficient there.
fn private_temp_dir() -> Result<std::path::PathBuf, String> {
    let dir = std::env::temp_dir().join("ferxgui-helper");

    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Something is already at this path — verify (one stat
                // call) that it's a real directory before trusting it,
                // then re-assert 0700. Both checks are load-bearing and
                // any failure here is fatal to the caller: if another
                // local user pre-created this exact path — a symlink, or
                // a directory they own — we must refuse to use it, not
                // silently proceed into it.
                let meta = std::fs::symlink_metadata(&dir)
                    .map_err(|e| format!("could not stat {}: {e}", dir.display()))?;
                if meta.file_type().is_symlink() {
                    return Err(format!(
                        "{} is a symlink, possibly planted by another user on a shared machine",
                        dir.display(),
                    ));
                }
                if !meta.is_dir() {
                    return Err(format!("{} exists and is not a directory", dir.display()));
                }
                // chmod requires ownership (or root) — this is the actual
                // gate that rejects a directory another user pre-created.
                std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                    .map_err(|e| format!(
                        "could not restrict permissions on {} (likely owned by another user): {e}",
                        dir.display(),
                    ))?;
            }
            Err(e) => return Err(format!("could not create {}: {e}", dir.display())),
        }
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }

    Ok(dir)
}

/// Cached wrapper around `private_temp_dir()` — established exactly once
/// per process and reused for the session. Only success is cached: a
/// failure (e.g. a transient filesystem hiccup, not necessarily an
/// attacker) is retried on the next call rather than permanently poisoning
/// every R-helper feature for the rest of the session. Re-checking on
/// every failed call can never reintroduce the vulnerable fallback this
/// function replaces — the check is either satisfied (safe to use) or it
/// errors, every time.
fn helper_temp_dir() -> Result<std::path::PathBuf, String> {
    static DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    if let Some(dir) = DIR.get() {
        return Ok(dir.clone());
    }
    let dir = private_temp_dir()?;
    Ok(DIR.get_or_init(|| dir).clone())
}

/// Returns a path in the private helper temp directory (see
/// `helper_temp_dir`) that is unique to this process and this call, via a
/// PID + monotonic-counter suffix — so concurrent calls (including from
/// other ferxgui instances) never collide on the same filename.
pub(crate) fn unique_temp_path(prefix: &str, ext: &str) -> Result<std::path::PathBuf, String> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = helper_temp_dir()?;
    Ok(dir.join(format!("{prefix}_{}_{seq}.{ext}", std::process::id())))
}

/// Maximum time to wait for an R helper call before killing it and
/// returning a timeout error. Generous by design — VPC/SIR computations are
/// documented as potentially taking "a few minutes" for large n_sim/resample
/// counts (user-configurable), so this is a safety net against a genuinely
/// hung process, not a tight budget. Every `run_script` call funnels through
/// here, including fast ones (version checks, template creation) — for
/// those this never has any effect since they always finish in well under
/// this window.
const R_HELPER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// Removes its tracked temp files when dropped, regardless of how
/// `run_script` exits (success or any early error return) — so the temp
/// files it creates (script, stdout capture, stderr capture) are cleaned up
/// on every code path without repeating `remove_file` calls at each site.
struct TempFileGuard(Vec<std::path::PathBuf>);

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Write `script` to a temp file, run `Rscript --vanilla <tmp> [args…]`,
/// and return stdout on success or the stderr text as an Err.
///
/// stdout/stderr are captured via temp files rather than piped, and the
/// child is polled with `try_wait()` instead of the blocking `.output()` —
/// this is required to enforce `R_HELPER_TIMEOUT`, and file capture (rather
/// than pipes) avoids the classic pipe-buffer deadlock a poll loop would
/// otherwise risk: a piped child that produces more output than the OS pipe
/// buffer holds while nothing is draining it between polls would block on
/// its own `write()` forever, defeating the very timeout being added. A
/// file write never blocks on a reader, so polling alongside it is safe.
fn run_script(script: &str, args: &[&str]) -> Result<String, String> {
    let rscript = find_rscript()
        .ok_or_else(|| "Rscript not found. Install R or add it to PATH.".to_string())?;

    // Uniquely named temp files — PID + monotonic counter avoids collisions
    // between concurrent R helpers.
    let tmp_path    = unique_temp_path("ferxgui", "R")?;
    let stdout_path = unique_temp_path("ferxgui_stdout", "log")?;
    let stderr_path = unique_temp_path("ferxgui_stderr", "log")?;
    let _cleanup = TempFileGuard(vec![tmp_path.clone(), stdout_path.clone(), stderr_path.clone()]);

    std::fs::write(&tmp_path, script)
        .map_err(|e| format!("could not write temp R script: {e}"))?;
    let stdout_file = std::fs::File::create(&stdout_path)
        .map_err(|e| format!("could not create stdout capture file: {e}"))?;
    let stderr_file = std::fs::File::create(&stderr_path)
        .map_err(|e| format!("could not create stderr capture file: {e}"))?;

    let mut cmd = r_command(&rscript);
    cmd.arg("--vanilla").arg(&tmp_path);
    for a in args { cmd.arg(a); }
    cmd.stdin(std::process::Stdio::null())
       .stdout(stdout_file)
       .stderr(stderr_file);

    let mut child = cmd.spawn()
        .map_err(|e| format!("failed to start {}: {e}", rscript.display()))?;
    let pid = child.id();
    register_helper_pid(pid);

    let start = std::time::Instant::now();
    let mut exit_status: Option<std::process::ExitStatus> = None;
    let mut timed_out = false;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => { exit_status = Some(status); break; }
            Ok(None) => {
                if start.elapsed() > R_HELPER_TIMEOUT {
                    timed_out = true;
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                unregister_helper_pid(pid);
                return Err(format!("wait error: {e}"));
            }
        }
    }
    unregister_helper_pid(pid);

    if timed_out {
        return Err(format!(
            "Rscript did not finish within {R_HELPER_TIMEOUT:?} and was terminated.",
        ));
    }

    // Guard against pathological R output that could exhaust memory — check
    // the on-disk size before reading, so a huge file is never materialized
    // in memory just to be rejected.
    const MAX_OUTPUT_BYTES: u64 = 10 * 1024 * 1024; // 10 MB
    let stdout_len = std::fs::metadata(&stdout_path).map(|m| m.len()).unwrap_or(0);
    if stdout_len > MAX_OUTPUT_BYTES {
        return Err(format!(
            "R output too large ({stdout_len} bytes > {MAX_OUTPUT_BYTES} byte limit)",
        ));
    }

    let stdout_bytes = std::fs::read(&stdout_path)
        .map_err(|e| format!("could not read R stdout capture: {e}"))?;

    if exit_status.map(|s| s.success()).unwrap_or(false) {
        Ok(String::from_utf8_lossy(&stdout_bytes).trim().to_string())
    } else {
        let stderr_text = std::fs::read(&stderr_path)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_else(|e| format!("(could not read R stderr capture: {e})"));
        Err(stderr_text)
    }
}

// ---------------------------------------------------------------------------
// In-flight helper PID tracking (for cleanup on app quit — see kill_all_helper_pids)
// ---------------------------------------------------------------------------

static HELPER_PIDS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<u32>>> =
    std::sync::OnceLock::new();

fn helper_pids() -> &'static std::sync::Mutex<std::collections::HashSet<u32>> {
    HELPER_PIDS.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

fn register_helper_pid(pid: u32) {
    if let Ok(mut set) = helper_pids().lock() { set.insert(pid); }
}

fn unregister_helper_pid(pid: u32) {
    if let Ok(mut set) = helper_pids().lock() { set.remove(&pid); }
}

/// Kill every currently-tracked R helper subprocess (VPC/SIR/Simulate/etc.
/// — anything spawned via `run_script`). Called once from the app's
/// `on_exit` hook so a helper still mid-flight when the GUI quits doesn't
/// linger as an orphaned process — unlike fit runs (`workers::run`), these
/// are not meant to survive the GUI closing.
pub fn kill_all_helper_pids() {
    let pids: Vec<u32> = match helper_pids().lock() {
        Ok(mut set) => {
            let pids: Vec<u32> = set.iter().copied().collect();
            set.clear();
            pids
        }
        Err(_) => return,
    };
    for pid in pids {
        crate::workers::run::kill_hard(pid);
    }
}

/// Build a `Command` for a short-lived R helper call.
///
/// On Windows, sets `CREATE_NO_WINDOW` so spawning a console app (Rscript,
/// tasklist, …) from this GUI process does not flash a console window.
pub fn r_command(program: &Path) -> std::process::Command {
    let cmd = std::process::Command::new(program);
    apply_no_window(cmd)
}

/// Apply the Windows `CREATE_NO_WINDOW` flag (no-op elsewhere).
pub fn apply_no_window(cmd: std::process::Command) -> std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut cmd = cmd;
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    }
    #[cfg(not(windows))]
    {
        cmd
    }
}

/// Locate the `Rscript` executable.
///
/// GUI apps launched from Finder/Spotlight (macOS) or Explorer (Windows) only
/// inherit a bare PATH that usually omits the R install dir.  We therefore
/// search PATH first, then `R_HOME`, then well-known per-platform locations.
pub fn find_rscript() -> Option<std::path::PathBuf> {
    let exe = rscript_exe_name();

    // 1. On the current process PATH.
    if let Some(p) = which_on_path(exe) { return Some(p); }

    // 2. R_HOME/bin (and bin/x64 on Windows) if the env var is set.
    if let Some(home) = std::env::var_os("R_HOME") {
        let home = std::path::PathBuf::from(home);
        for p in [home.join("bin").join(exe), home.join("bin").join("x64").join(exe)] {
            if p.is_file() { return Some(p); }
        }
    }

    // 3. Per-platform well-known locations.
    platform_rscript_candidates().into_iter().find(|p| p.is_file())
}

#[cfg(windows)]
fn rscript_exe_name() -> &'static str { "Rscript.exe" }
#[cfg(not(windows))]
fn rscript_exe_name() -> &'static str { "Rscript" }

#[cfg(not(windows))]
fn platform_rscript_candidates() -> Vec<std::path::PathBuf> {
    [
        "/Library/Frameworks/R.framework/Resources/bin/Rscript",
        "/Library/Frameworks/R.framework/Versions/Current/Resources/bin/Rscript",
        "/usr/local/bin/Rscript",
        "/opt/homebrew/bin/Rscript",
        "/usr/bin/Rscript",
        "/usr/lib/R/bin/Rscript",
    ]
    .iter()
    .map(std::path::PathBuf::from)
    .collect()
}

#[cfg(windows)]
fn platform_rscript_candidates() -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let roots = ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"];
    for var in roots {
        let Some(root) = std::env::var_os(var) else { continue };
        let r_dir = std::path::PathBuf::from(root).join("R");
        let Ok(entries) = std::fs::read_dir(&r_dir) else { continue };
        for entry in entries.flatten() {
            let base = entry.path();
            out.push(base.join("bin").join("x64").join("Rscript.exe"));
            out.push(base.join("bin").join("Rscript.exe"));
        }
    }
    out
}

/// Walk PATH looking for `name`.
fn which_on_path(name: &str) -> Option<std::path::PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let p = dir.join(name);
        if p.is_file() { return Some(p); }
    }
    None
}

#[cfg(test)]
mod simulation_tests {
    use super::*;

    /// Live integration test against a real ferx-r example model + covariate
    /// dataset (self-documented "skip on other machines", matching the
    /// existing `warfarin_fitrx_parses_correctly` pattern). Confirms the
    /// merge-back-to-input-data behaviour end-to-end: row count matches
    /// `n_sim * input_rows`, dose rows (EVID=1) get blank IPRED/DV_SIM since
    /// `ferx_simulate()` only predicts at observation times, observation rows
    /// get populated IPRED/DV_SIM, and covariate columns from the input
    /// dataset survive into the output untouched.
    #[test]
    fn simulate_merges_covariates_and_evid_from_input_data() {
        let base = std::path::Path::new(
            "/Users/robterheine/Downloads/ferx_inspect/ferx-r/ferx-r-main"
        );
        let model_path = base.join("inst/examples/models/two_cpt_oral_cov.ferx");
        let data_path  = base.join("examples/data/two_cpt_oral_cov.csv");
        if !model_path.exists() || !data_path.exists() { return; } // skip on other machines

        let out_path = std::env::temp_dir().join("ferxgui_test_sim_output.csv");
        let cfg = SimRunConfig {
            model_path: model_path.to_string_lossy().into_owned(),
            data_path:  data_path.to_string_lossy().into_owned(),
            fitrx_path: None, // "initial estimates" basis — no fit required
            n_sim: 2,
            seed:  42,
            out_path: out_path.to_string_lossy().into_owned(),
            uncertainty_method: None,
            n_uncertainty_draws: None,
            n_sim_per_draw: None,
            sir_resamples_flat: None,
            sir_resamples_n: None,
            sir_resamples_dim: None,
        };

        let result = compute_simulation(&cfg).expect("simulation should succeed");
        assert_eq!(result.out_path, cfg.out_path);

        // Count input rows directly rather than hardcoding, so the test
        // still holds if the fixture file changes.
        let input_rows = std::fs::read_to_string(&data_path).unwrap().lines().count() - 1; // minus header
        assert_eq!(result.n_rows, input_rows * cfg.n_sim as usize);

        for col in ["ID", "TIME", "DV", "EVID", "CMT", "WT", "CRCL", "SIM", "IPRED", "DV_SIM"] {
            assert!(result.columns.contains(&col.to_string()), "missing column: {col}");
        }

        let written = std::fs::read_to_string(&out_path).expect("output CSV should exist");
        let mut rdr = csv::Reader::from_reader(written.as_bytes());
        let headers: Vec<String> = rdr.headers().unwrap().iter().map(str::to_string).collect();
        let evid_idx    = headers.iter().position(|h| h == "EVID").unwrap();
        let ipred_idx   = headers.iter().position(|h| h == "IPRED").unwrap();
        let dv_sim_idx  = headers.iter().position(|h| h == "DV_SIM").unwrap();
        let wt_idx      = headers.iter().position(|h| h == "WT").unwrap();

        let mut saw_dose_row = false;
        let mut saw_obs_row  = false;
        for record in rdr.records() {
            let record = record.unwrap();
            let is_dose = &record[evid_idx] == "1";
            let ipred_blank  = record[ipred_idx].trim().is_empty();
            let dv_sim_blank = record[dv_sim_idx].trim().is_empty();
            assert!(!record[wt_idx].trim().is_empty(), "covariate WT must survive the merge on every row");
            if is_dose {
                saw_dose_row = true;
                assert!(ipred_blank && dv_sim_blank, "dose rows must have blank IPRED/DV_SIM");
            } else {
                saw_obs_row = true;
                assert!(!ipred_blank && !dv_sim_blank, "observation rows must have populated IPRED/DV_SIM");
            }
        }
        assert!(saw_dose_row, "fixture should contain at least one dose row");
        assert!(saw_obs_row,  "fixture should contain at least one observation row");

        let _ = std::fs::remove_file(&out_path);
    }

    /// Live integration test for Phase 3's asymptotic-uncertainty path.
    /// Self-fits the small warfarin example with `covariance = TRUE` (rather
    /// than depending on a pre-existing `.fitrx` whose convergence state
    /// fluctuates with whatever was last run on this machine — see
    /// `io::fitrx::tests::warfarin_fitrx_parses_correctly`), then confirms
    /// `ferx_simulate_with_uncertainty()` produces a merged CSV whose row
    /// count is `n_uncertainty_draws * n_sim_per_draw * n_obs`, and that
    /// `IPRED` genuinely varies across draws (proof the parameter-uncertainty
    /// dimension is real, not just replicated eta/epsilon noise at one draw).
    /// Self-documented "skip on other machines".
    #[test]
    fn simulate_with_asymptotic_uncertainty_produces_draw_varying_output() {
        let base = std::path::Path::new(
            "/Users/robterheine/Downloads/ferx_inspect/ferx-r/ferx-r-main"
        );
        let model_path = base.join("inst/examples/models/warfarin.ferx");
        let data_path  = base.join("inst/examples/data/warfarin.csv");
        if !model_path.exists() || !data_path.exists() { return; } // skip on other machines

        let fitrx_path = std::env::temp_dir().join("ferxgui_test_uncertainty.fitrx");
        let fit_script = r#"
args <- commandArgs(trailingOnly = TRUE)
suppressMessages(library(ferx))
fit <- ferx_fit(args[1], args[2], method = "focei", covariance = TRUE)
ferx_save_fit(fit, args[3])
cat("ok")
"#;
        run_script(fit_script, &[
            path_as_str(&model_path).unwrap(),
            path_as_str(&data_path).unwrap(),
            path_as_str(&fitrx_path).unwrap(),
        ]).expect("warfarin fit with covariance should succeed");

        let out_path = std::env::temp_dir().join("ferxgui_test_uncertainty_output.csv");
        let cfg = SimRunConfig {
            model_path: model_path.to_string_lossy().into_owned(),
            data_path:  data_path.to_string_lossy().into_owned(),
            fitrx_path: Some(fitrx_path.to_string_lossy().into_owned()),
            n_sim: 0, // unused in uncertainty mode
            seed:  1,
            out_path: out_path.to_string_lossy().into_owned(),
            uncertainty_method: Some("asymptotic".to_string()),
            n_uncertainty_draws: Some(3),
            n_sim_per_draw: Some(2),
            sir_resamples_flat: None,
            sir_resamples_n: None,
            sir_resamples_dim: None,
        };

        let result = compute_simulation(&cfg).expect("uncertainty simulation should succeed");

        let input_rows = std::fs::read_to_string(&data_path).unwrap().lines().count() - 1;
        let n_obs = std::fs::read_to_string(&data_path).unwrap()
            .lines().skip(1)
            .filter(|l| l.split(',').nth(3) == Some("0")) // EVID column
            .count();
        assert_eq!(result.n_rows, 6 * n_obs + 6 * (input_rows - n_obs));
        assert!(result.columns.contains(&"DRAW".to_string()), "output must carry the DRAW column");

        let written = std::fs::read_to_string(&out_path).expect("output CSV should exist");
        let mut rdr = csv::Reader::from_reader(written.as_bytes());
        let headers: Vec<String> = rdr.headers().unwrap().iter().map(str::to_string).collect();
        let draw_idx  = headers.iter().position(|h| h == "DRAW").unwrap();
        let id_idx    = headers.iter().position(|h| h == "ID").unwrap();
        let time_idx  = headers.iter().position(|h| h == "TIME").unwrap();
        let ipred_idx = headers.iter().position(|h| h == "IPRED").unwrap();

        // Same (ID, TIME) observation under DRAW=1 vs DRAW=2 should differ —
        // proof the parameter draw actually varies, not just noise reseeded
        // at a fixed parameter set.
        let mut by_draw: std::collections::HashMap<(String, String), String> = std::collections::HashMap::new();
        for record in rdr.records() {
            let record = record.unwrap();
            if &record[draw_idx] == "1" && !record[ipred_idx].trim().is_empty() {
                by_draw.insert((record[id_idx].to_string(), record[time_idx].to_string()), record[ipred_idx].to_string());
            }
        }
        assert!(!by_draw.is_empty(), "should have found DRAW=1 observation rows");

        let out_content2 = std::fs::read_to_string(&out_path).unwrap();
        let mut rdr2 = csv::Reader::from_reader(out_content2.as_bytes());
        let mut found_difference = false;
        for record in rdr2.records() {
            let record = record.unwrap();
            if &record[draw_idx] == "2" && !record[ipred_idx].trim().is_empty() {
                let key = (record[id_idx].to_string(), record[time_idx].to_string());
                if let Some(draw1_ipred) = by_draw.get(&key) {
                    if draw1_ipred != &record[ipred_idx] { found_difference = true; break; }
                }
            }
        }
        assert!(found_difference, "IPRED should differ across parameter draws");

        let _ = std::fs::remove_file(&out_path);
        let _ = std::fs::remove_file(&fitrx_path);
    }

    /// Live integration test for Phase 3's SIR-uncertainty path — the one
    /// that needs raw resamples re-injected onto a freshly `ferx_load_fit()`-ed
    /// fit, since `ferx_load_fit()` does not itself restore them. Runs the
    /// full real pipeline: fit → `compute_sir()` (the existing SIR bridge) →
    /// pass its `sir_resamples_flat/_n/_dim` into `SimRunConfig` →
    /// `compute_simulation()`. Confirms the output merges correctly and that
    /// IPRED genuinely varies across draws. Self-documented "skip on other
    /// machines".
    #[test]
    fn simulate_with_sir_uncertainty_produces_draw_varying_output() {
        let base = std::path::Path::new(
            "/Users/robterheine/Downloads/ferx_inspect/ferx-r/ferx-r-main"
        );
        let model_path = base.join("inst/examples/models/warfarin.ferx");
        let data_path  = base.join("inst/examples/data/warfarin.csv");
        if !model_path.exists() || !data_path.exists() { return; } // skip on other machines

        let fitrx_path = std::env::temp_dir().join("ferxgui_test_sir_uncertainty.fitrx");
        let fit_script = r#"
args <- commandArgs(trailingOnly = TRUE)
suppressMessages(library(ferx))
fit <- ferx_fit(args[1], args[2], method = "focei", covariance = TRUE)
ferx_save_fit(fit, args[3])
cat("ok")
"#;
        run_script(fit_script, &[
            path_as_str(&model_path).unwrap(),
            path_as_str(&data_path).unwrap(),
            path_as_str(&fitrx_path).unwrap(),
        ]).expect("warfarin fit with covariance should succeed");

        let sir = compute_sir(&fitrx_path, 200, 100, 1, true)
            .expect("SIR with kept samples should succeed");
        assert!(sir.sir_resamples_n > 0, "SIR result should carry raw resamples");
        assert!(!sir.sir_resamples_flat.is_empty());

        let out_path = std::env::temp_dir().join("ferxgui_test_sir_uncertainty_output.csv");
        let cfg = SimRunConfig {
            model_path: model_path.to_string_lossy().into_owned(),
            data_path:  data_path.to_string_lossy().into_owned(),
            fitrx_path: Some(fitrx_path.to_string_lossy().into_owned()),
            n_sim: 0, // unused in uncertainty mode
            seed:  1,
            out_path: out_path.to_string_lossy().into_owned(),
            uncertainty_method: Some("sir".to_string()),
            n_uncertainty_draws: Some(3),
            n_sim_per_draw: Some(2),
            sir_resamples_flat: Some(sir.sir_resamples_flat.clone()),
            sir_resamples_n: Some(sir.sir_resamples_n),
            sir_resamples_dim: Some(sir.sir_resamples_dim),
        };

        let result = compute_simulation(&cfg).expect("SIR uncertainty simulation should succeed");
        assert!(result.columns.contains(&"DRAW".to_string()));

        let written = std::fs::read_to_string(&out_path).expect("output CSV should exist");
        let mut rdr = csv::Reader::from_reader(written.as_bytes());
        let headers: Vec<String> = rdr.headers().unwrap().iter().map(str::to_string).collect();
        let draw_idx  = headers.iter().position(|h| h == "DRAW").unwrap();
        let id_idx    = headers.iter().position(|h| h == "ID").unwrap();
        let time_idx  = headers.iter().position(|h| h == "TIME").unwrap();
        let ipred_idx = headers.iter().position(|h| h == "IPRED").unwrap();

        let mut by_draw: std::collections::HashMap<(String, String), String> = std::collections::HashMap::new();
        for record in rdr.records() {
            let record = record.unwrap();
            if &record[draw_idx] == "1" && !record[ipred_idx].trim().is_empty() {
                by_draw.insert((record[id_idx].to_string(), record[time_idx].to_string()), record[ipred_idx].to_string());
            }
        }
        assert!(!by_draw.is_empty());

        let out_content2 = std::fs::read_to_string(&out_path).unwrap();
        let mut rdr2 = csv::Reader::from_reader(out_content2.as_bytes());
        let mut found_difference = false;
        for record in rdr2.records() {
            let record = record.unwrap();
            if &record[draw_idx] == "2" && !record[ipred_idx].trim().is_empty() {
                let key = (record[id_idx].to_string(), record[time_idx].to_string());
                if let Some(draw1_ipred) = by_draw.get(&key) {
                    if draw1_ipred != &record[ipred_idx] { found_difference = true; break; }
                }
            }
        }
        assert!(found_difference, "IPRED should differ across SIR parameter draws");

        let _ = std::fs::remove_file(&out_path);
        let _ = std::fs::remove_file(&fitrx_path);
    }
}
