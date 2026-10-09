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
