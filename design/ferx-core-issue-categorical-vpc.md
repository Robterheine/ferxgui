# Add categorical / count outcome likelihood + simulation (enables categorical VPC)

**Repo:** FeRx-NLME/ferx-core
**Type:** Feature / upstream enabler
**Filed by:** FeRx GUI (blocks the GUI's categorical VPC feature)

## Summary

FeRx GUI wants to expose the `vpc` R package's **categorical VPC** (`vpc::vpc_cat()`),
which compares the observed probability of each discrete outcome category per
time-bin against simulated probabilities. This requires `ferx_simulate()` to
produce **simulated discrete outcomes** (category draws), which the engine
currently cannot do.

## Evidence this is not yet supported

`src/stats/likelihood.rs` states the only observation variant implemented is the
TTE event:

```
// ObsRecord::Event is the only variant (DiscreteState/Count deferred);
```

The only outcome likelihoods present are Gaussian residual error and the
feature-gated TTE survival hazard. There is no Bernoulli / binomial / ordinal /
categorical / Poisson-count likelihood, and the simulation path
(`SimulationResult.dv_sim = IPRED + √V·ε`) only generates continuous draws.

The `examples/warfarin_logit_f.ferx` model is **not** a categorical model — the
logit transform there constrains the bioavailability *parameter* F to (0,1); the
observed DV is still continuous concentration (`DV ~ proportional`).

## What's needed in ferx-core

1. A discrete-outcome likelihood family (at minimum binary logistic; ideally
   ordered categorical and Poisson/count), i.e. implement the deferred
   `ObsRecord::DiscreteState` / `Count` variants and matching
   `EndpointLikelihood`.
2. A model-file block to declare a categorical/count endpoint (analogous to
   `[event_model]`).
3. Simulation support: `simulate()` must draw a category/count for those
   endpoints (e.g. Bernoulli/multinomial/Poisson sampling from the predicted
   probabilities/rate), surfaced through the extendr `ferx_simulate()` wrapper
   as a discrete DV column.

## Downstream GUI work (already scoped, blocked on the above)

Once `ferx_simulate()` returns discrete simulated DV, the GUI side is small:
route a new `vpc_type == "categorical"` branch through `vpc::vpc_cat(..., vpcdb=TRUE)`,
add a probability-per-category renderer (y ∈ [0,1], one panel per category), and
add a "Categorical" VPC type to the options panel.

## Acceptance / verification

- A worked categorical example model + dataset under `examples/`.
- `ferx_simulate(model, data, n_sim, seed)` returns a discrete DV column whose
  per-bin category frequencies are consistent with the model's predicted
  probabilities.
- `vpc::vpc_cat(sim = ferx_sim, obs = data, ...)` runs without error and produces
  a sensible categorical VPC.
