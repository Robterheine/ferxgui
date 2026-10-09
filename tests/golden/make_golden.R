# Generates golden values for the Rust statistics code. Run: Rscript tests/golden/make_golden.R
suppressMessages(library(jsonlite))
set.seed(1)
chi <- expand.grid(df = 1:30, x = signif(10^seq(-6, 2, length.out = 25), 6))
chi$p <- pchisq(chi$x, chi$df, lower.tail = FALSE)
tt <- expand.grid(df = 1:30, t = c(1e-6, 0.01, 0.3, 1, 2.0, 3.5, 8, 25, 100))
tt$p <- 2 * pt(-abs(tt$t), tt$df)
pv <- list(c(0.01, 0.04, 0.03, 0.5, 0.2), c(0.5, 0.5, 0.001), c(1, 0.9, 0.02, 0.02, 0.3, NA), runif(12))
bh <- lapply(pv, function(p) list(p = p, adj = p.adjust(p, "BH")))
z <- expand.grid(r = c(-0.9, -0.3, 0, 0.12, 0.5, 0.95), n = c(5, 10, 100))
z$lo <- tanh(atanh(z$r) - qnorm(0.975) / sqrt(z$n - 3))
z$hi <- tanh(atanh(z$r) + qnorm(0.975) / sqrt(z$n - 3))
qn <- data.frame(p = c(1e-10, 1e-4, 0.01, 0.025, 0.3, 0.5, 0.8, 0.975, 0.999))
qn$q <- qnorm(qn$p)
out <- list(
  versions = list(R = R.version.string),
  chi2 = chi, t = tt, bh = bh, fisher = z, qnorm = qn
)
writeLines(toJSON(out, digits = NA, auto_unbox = TRUE, na = "null"), "tests/golden/stats.json")

# ---- LOESS (ggplot2 geom_smooth default: degree 2, span 0.75, tricube, direct fit) ----
set.seed(7)
mk <- function(x, y) {
  grid <- seq(min(x), max(x), length.out = 61)
  f <- loess(y ~ x, span = 0.75, degree = 2, family = "gaussian",
             control = loess.control(surface = "direct"))
  list(x = x, y = y, grid = grid, fit = as.numeric(predict(f, data.frame(x = grid))))
}
x1 <- sort(runif(40, 0, 24)); y1 <- 3 * exp(-0.2 * x1) + rnorm(40, 0, 0.2)
x2 <- rep(c(0.5, 1, 2, 4, 8, 12, 24), each = 6); y2 <- -0.2 * log(x2) + rnorm(42, 0, 0.5)
out2 <- list(a = mk(x1, y1), b = mk(x2, y2))
writeLines(toJSON(out2, digits = NA, auto_unbox = FALSE, na = "null"), "tests/golden/loess.json")

# ---- Simulation band: vpc::vpc() prediction bounds (median over replicates of per-replicate percentiles) ----
suppressMessages(library(vpc))
set.seed(3)
ids <- 1:6; times <- c(1, 2, 4)
obs <- expand.grid(id = ids, time = times); obs$dv <- rnorm(nrow(obs), 10, 2)
sim <- do.call(rbind, lapply(1:5, function(s) {
  d <- obs[, c("id", "time")]; d$sim <- s; d$dv <- rnorm(nrow(d), 10, 2); d }))
sim <- sim[order(sim$sim, sim$id, sim$time), ]
v <- suppressWarnings(suppressMessages(
  vpc(sim = sim, obs = obs, bins = c(0.5, 1.5, 3, 5), pi = c(0.1, 0.9), ci = c(0.05, 0.95), vpcdb = TRUE)))
d <- as.data.frame(v$vpc_dat)
writeLines(toJSON(list(
  vpc_version = as.character(packageVersion("vpc")),
  sim = list(rep = sim$sim, time = sim$time, dv = sim$dv),
  lo = d$q5.med, med = d$q50.med, hi = d$q95.med, times = times
), digits = NA, auto_unbox = FALSE), "tests/golden/simband.json")
