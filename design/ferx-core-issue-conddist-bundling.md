# Bundle `conddist.csv` into `.fitrx` (enables Individual conditional distributions in FeRx GUI)

**Repo:** FeRx-NLME/ferx-core
**Type:** Feature / upstream enabler
**Filed by:** FeRx GUI (blocks the GUI's "Cond. Dist." Evaluation section)
**Relates to:** #257, PR #265 (merged — the SAEM conditional-distribution algorithm)

## Summary

PR #265 added the SAEM post-fit conditional-distribution pass
(`saem_conddist::run_conditional_distribution`), gated behind `[fit_options]`
`conddist = true` (SAEM-only). It writes `{model}-conddist.csv` as a sidecar
file next to the model output via `output::write_conddist_csv`. This data
(per-subject conditional mean, SD, and mode of η) is the shrinkage-unbiased
basis for η diagnostics — the direct analogue of Monolix's Conditional
Distribution task and saemix's `conddist.saemix`. FeRx GUI wants to visualise
it in the Evaluation tab, but everything it reads comes from inside the
`.fitrx` ZIP bundle, and this result is not currently written there.

## Evidence

`src/io/fitrx.rs`, in the bundle-writer, still carries this comment verbatim:

```rust
// The conditional-distribution pass is not persisted to .fitrx yet
// (#257); a loaded fit reports `None` for it.
cond_dist: None,
```

Only `ebes.csv` is zipped into the bundle (`zip.start_file("ebes.csv", ...)`
followed by `write_ebes_csv`); there is no equivalent `conddist.csv` entry, so
`cond_dist` on a bundle-loaded `FitResult` is always `None` even when the
original fit had `conddist = true` and a sidecar CSV was written to disk.

## What's needed

Mirror the existing `ebes.csv` block in the bundle writer
(`src/io/fitrx.rs`), gated on `result.cond_dist.is_some()`:

```rust
// --- conddist.csv (only when conddist pass ran) --------------------------------
if let Some(cd) = &result.cond_dist {
    zip.start_file("conddist.csv", zopts)?;
    write_conddist_csv_to_writer(&mut zip, result, cd)?;
    entries.push("conddist.csv".into());
}
```

`write_conddist_csv_to_writer` should mirror the existing
`output::write_conddist_csv` (same `ID, ETA, COND_MEAN, COND_SD, COND_MODE`
schema — mean/SD from `cd.cond_mean`/`cd.cond_sd`, mode from the subject's
existing EBE) but write to a `Write` impl (the zip entry) instead of a file
path, matching how `write_ebes_csv` is already structured.

The reader side (`fitrx::read_fit_json`/bundle loader) needs the symmetric
change: populate `cond_dist: Some(CondDist { .. })` from `conddist.csv` when
present in the archive, instead of hardcoding `None`.

## Downstream GUI work (already scoped, blocked on the above)

Once `conddist.csv` round-trips through `.fitrx`: a `read_conddist()` reader
in ferxgui's `io/fitrx.rs` (mirroring `read_ebes()`), a `CondDistData` domain
type, and a new "Cond. Dist." Evaluation section with three sub-views
(distribution histogram + `N(0, ω_jj)` overlay, per-subject caterpillar plot,
Mode-vs-Mean scatter). `read_conddist()` returning `Ok(None)` for
bundles written before this change is the designed fallback, so the GUI side
can ship ahead of this and activate automatically once it lands.

## Acceptance / verification

- A `.fitrx` produced from a fit with `conddist = true` (e.g.
  `examples/warfarin_saem_conddist.ferx`) contains a `conddist.csv` entry with
  the `ID, ETA, COND_MEAN, COND_SD, COND_MODE` schema.
- Loading that bundle back (`ferx_load_fit()` / the Rust bundle reader)
  populates `cond_dist` as `Some(..)`, matching the values from the original
  in-process `FitResult` (round-trip test).
- A `.fitrx` from a fit without `conddist = true`, or from a non-SAEM method,
  has no `conddist.csv` entry and loads with `cond_dist: None` — unchanged
  from current behaviour.
