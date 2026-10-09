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
cat(sprintf("ferx %s, %s\n", as.character(packageVersion("ferx")), R.version.string))
