# Remediation status (v0.10.0)

Executes `REMEDIATION_PLAN.md` (audit items A-01 … A-49). "Done" means implemented and covered by a
test that fails without the change; items needing a platform or a person I could not use here are
under *Open*.

| Package | Items | Status |
|---|---|---|
| WP-0 hotfix | A-11, A-12, A-13, A-18, A-20, A-32, A-36, A-37 | Done |
| WP-1 identity | A-05, A-06, A-27 (provenance), A-29 (covariance) | Done |
| WP-2 data integrity | A-01, A-02, A-28, A-29, A-30, A-31 | Done |
| WP-3 figures | A-03, A-04, A-08, A-21 … A-27, A-48, A-49 | Done (A-49: labelled as packed, not converted) |
| WP-4 parity | A-07, A-09, A-10, A-41, A-46 | Done |
| WP-5 statistics | A-10 (ETAbar), A-16, A-24, A-40 | Done (hand-written functions pass the R grid; `statrs` not needed) |
| WP-6 process safety | A-12, A-14, A-15, A-43 | Done on Unix; Windows code written, not run here |
| WP-7 platforms | A-19, A-33, A-34, A-35 | Done as code/docs; see Open |
| WP-8 tests/CI | A-17, A-44 | Done |
| WP-9 polish | A-38, A-42, A-45, A-47 | Done |

## Open

1. **Windows is unverified.** The Windows branches (`procid.rs` Win32 calls, Reveal with `raw_arg`,
   the toast env pass-through test) compile only on Windows; the `windows-latest` CI leg runs them.
   The manual checklist (Reveal selects the file; a path such as `Rénée` with spaces reaches R)
   remains [U].
2. **MSRV is declared, not measured** (`rust-version = "1.85"`); the `msrv` CI job will confirm or
   force a bump. The `is_multiple_of` calls that would have required 1.87+ were removed.
3. **Scientific sign-off (§6b)** — prediction-interval default, log-scale CIs, CV%, the LRT rule,
   SIR presentation, NPDE seed/Wilson CI, DW/lag-1 scaling and ETA-covariate BH — has not been
   reviewed by a pharmacometrician; the release notes say so.
4. **Block-omega / IOV SIR histograms** stay hidden until ferx documents the packed order (upstream
   request drafted in the audit folder, not posted).
5. Not done from the plan: a one-time in-app note on first launch of 0.10.0; the optional CI
   across replicates for the simulation band; the editor hint on a rejected header such as
   `[ parameters ]` (the parser already refuses it; `ferx_grammar::is_rejected_header` is ready).
