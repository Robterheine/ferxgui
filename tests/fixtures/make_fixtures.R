# Rebuilds the .fitrx fixtures. Run from the repository root: Rscript tests/fixtures/make_fixtures.R
suppressMessages(library(ferx))
d <- "tests/fixtures"
fit1 <- ferx_fit(file.path(d, "warfarin.ferx"), file.path(d, "warfarin.csv"),
                 method = "focei", covariance = TRUE)
ferx_save_fit(fit1, file.path(d, "warfarin.fitrx"))
fit2 <- ferx_fit(file.path(d, "warfarin_block_omega.ferx"), file.path(d, "warfarin_block_omega.csv"),
                 method = "focei", covariance = TRUE)
ferx_save_fit(fit2, file.path(d, "warfarin_block_omega.fitrx"))
fit3 <- ferx_fit(file.path(d, "emax_pkpd.ferx"), file.path(d, "emax_pkpd.csv"),
                 method = "focei", covariance = FALSE, gradient = "fd")
ferx_save_fit(fit3, file.path(d, "emax_pkpd.fitrx"))
# warfarin_bad_init.ferx is warfarin.ferx with TVCL init 50 (above its upper bound 10)
cat(sprintf("ferx %s, %s\n", as.character(packageVersion("ferx")), R.version.string))

# Warfarin with an identity-packed theta (negative declared lower bound): plan §5.8.
src <- readLines(file.path(d, "warfarin.ferx"))
src <- sub("theta TVKA(1.0, 0.01, 50.0)", "theta TVKA(1.0, 0.01, 50.0)\n  theta ADD_CL(0.0, -0.1, 0.1)", src, fixed = TRUE)
src <- sub("CL = TVCL * exp(ETA_CL)", "CL = TVCL * exp(ETA_CL) + ADD_CL", src, fixed = TRUE)
writeLines(src, file.path(d, "warfarin_add_cl.ferx"))
fit4 <- ferx_fit(file.path(d, "warfarin_add_cl.ferx"), file.path(d, "warfarin.csv"),
                 method = "focei", covariance = TRUE)
ferx_save_fit(fit4, file.path(d, "warfarin_add_cl.fitrx"))

# Warfarin with a FIXed theta (fixed flags, blank correlation row/column).
src <- readLines(file.path(d, "warfarin.ferx"))
src <- sub("theta TVKA(1.0, 0.01, 50.0)", "theta TVKA(1.0, 0.01, 50.0) FIX", src, fixed = TRUE)
writeLines(src, file.path(d, "warfarin_fix.ferx"))
ferx_save_fit(ferx_fit(file.path(d, "warfarin_fix.ferx"), file.path(d, "warfarin.csv"),
                       method = "focei", covariance = TRUE), file.path(d, "warfarin_fix.fitrx"))

# Warfarin with an inline MAP prior.
src <- readLines(file.path(d, "warfarin.ferx"))
src <- sub("theta TVCL(0.134, 0.001, 10.0)", "theta TVCL(0.134, 0.001, 10.0) prior(0.15, rse = 10%)", src, fixed = TRUE)
writeLines(src, file.path(d, "warfarin_prior.ferx"))
ferx_save_fit(ferx_fit(file.path(d, "warfarin_prior.ferx"), file.path(d, "warfarin.csv"),
                       method = "focei", covariance = TRUE), file.path(d, "warfarin_prior.fitrx"))

# IOV (kappa) fit.
ferx_save_fit(ferx_fit(file.path(d, "warfarin_iov.ferx"), file.path(d, "warfarin_iov.csv"),
                       method = "focei", covariance = TRUE), file.path(d, "warfarin_iov.fitrx"))
