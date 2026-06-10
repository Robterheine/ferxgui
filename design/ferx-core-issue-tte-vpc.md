# Apply horizon/administrative censoring in `simulate_tte` + surface event DV in `ferx_simulate` (enables TTE VPC)

**Repo:** FeRx-NLME/ferx-core
**Type:** Feature / fidelity fix (upstream enabler)
**Filed by:** FeRx GUI (blocks the GUI's time-to-event VPC feature)

## Summary

FeRx GUI wants to expose the `vpc` R package's **time-to-event VPC**
(`vpc::vpc_tte()`), which compares observed Kaplan-Meier survival curves against
the simulated survival prediction interval. The engine already simulates event
*times*, but two gaps make the simulated data unsuitable for a statistically
valid TTE VPC.

## What already works

`src/survival/mod.rs::simulate_tte()` draws event times for exponential / Weibull
/ Gompertz hazards, handles `TENTRY` left-truncation, and emits
`SimOutcome::Event { time, observed }`. So event-time generation exists.

## Gap 1 — no administrative / horizon censoring (statistical correctness)

`simulate_tte()` documents:

> *"Administrative censoring is not applied here — the event time is drawn from
> the unconditional distribution. … Until then, every draw is an uncensored
> event — simulated data will not match the censoring pattern of the reference
> scripts. (Phase 2 will add `[simulation] horizon` support.)"*

A TTE VPC overlays the observed KM curve (which reflects the study's censoring /
observation window) on the simulated survival PI. If every simulated subject is
an uncensored event with no study horizon, the simulated KM curves are biased and
the VPC is **misleading**, not merely incomplete. Implementing the planned
`[simulation] horizon` (set `observed = false` and `time = horizon` when
`t_event >= horizon`) is the blocker.

## Gap 2 — event outcome not confirmed in the R wrapper

The documented `ferx_simulate()` return is the continuous shape
(`SIM, ID, TIME, IPRED, DV_SIM`), and the README's VPC example only shows
continuous `vpc()`. `vpc::vpc_tte()` needs, per simulated subject, an event/censor
indicator (DV: 1 = event, 0 = censored) and the event TIME, plus the `SIM` index.
Confirm/expose `SimOutcome::Event` through the extendr wrapper as a
`vpc_tte`-compatible data.frame (DV + TIME + SIM, with the censor coding above).

## Downstream GUI work (already scoped, blocked on the above)

Once `ferx_simulate()` surfaces censored event outcomes: route a
`vpc_type == "time-to-event"` branch through `vpc::vpc_tte(..., rtte, events,
kmmc, vpcdb=TRUE)`, add a Kaplan-Meier step-curve renderer (survival PI ribbon +
observed KM with CI + censoring ticks), and add TTE controls (RTTE toggle, event
selection, KMMC, as-percentage).

## Acceptance / verification

- `[simulation] horizon` (or equivalent) applied in `simulate_tte`, producing a
  realistic mix of events and right-censored subjects.
- `ferx_simulate()` returns a `vpc_tte`-compatible data.frame for a TTE model.
- Simulated KM median tracks the analytic survival function for
  `examples/tte_weibull.ferx` / `tte_exponential.ferx`; `vpc::vpc_tte()` runs
  without error and the observed KM falls within the simulated PI for a
  correctly-specified model.
