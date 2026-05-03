//! Methodology & Confidence section: the static appendix explaining
//! how Wilson CIs / bootstrap CIs / confidence buckets / author tags
//! interact.
//!
//! Always emitted — every report should be self-contained and tell the
//! reader how to interpret the numbers above.

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::{CalibrationVerdict, MonteCarloSummary};

use super::ReportSection;

pub(super) struct Methodology;

impl ReportSection for Methodology {
    fn render(&self, summary: &MonteCarloSummary, scenario: &Scenario, out: &mut String) {
        let _ = writeln!(out, "## Methodology & Confidence");

        // Per-scenario calibration confidence tag (Epic N round-two).
        // Surfaces the cross-scenario calibration claim alongside the
        // existing parameter-defensibility tag (`meta.confidence`) and
        // the per-observation Wilson CIs explained below. Renders
        // only when the scenario declares a `historical_analogue` and
        // the summary computed a verdict — the standalone Calibration
        // section above handles the synthetic-disclaimer / no-runs
        // paths.
        if scenario.meta.historical_analogue.is_some()
            && let Some(report) = summary.calibration.as_ref()
        {
            let (glyph, claim) = match report.overall {
                CalibrationVerdict::Pass => {
                    ("[H]", "Passes against the declared historical analogue")
                },
                CalibrationVerdict::Marginal => {
                    ("[M]", "Marginal against the declared historical analogue")
                },
                CalibrationVerdict::Fail => {
                    ("[L]", "Fails against the declared historical analogue")
                },
            };
            let _ = writeln!(
                out,
                "> **Calibration confidence: {} {}.** {} (worst per-observation verdict; see the Calibration section above for the per-observation breakdown).",
                glyph,
                verdict_word(report.overall),
                claim,
            );
            let _ = writeln!(out);
        }

        let _ = writeln!(out, "{}", METHODOLOGY_APPENDIX.trim_start());
    }
}

fn verdict_word(v: CalibrationVerdict) -> &'static str {
    match v {
        CalibrationVerdict::Pass => "Pass",
        CalibrationVerdict::Marginal => "Marginal",
        CalibrationVerdict::Fail => "Fail",
    }
}

const METHODOLOGY_APPENDIX: &str = r#"
This report combines two distinct sources of uncertainty. Mixing them up is a common way to get analysis wrong, so they are reported separately:

- **Sampling uncertainty** (the Wilson CIs below). Given the scenario's specified parameters, how precisely did the Monte Carlo runs estimate the rates shown? More runs shrink these intervals.
- **Parameter uncertainty** (the author-flagged confidence tags). Are the input parameters themselves defensible? A tight Wilson CI around a success rate derived from expert-guess detection probabilities does not mean the real-world success rate is known to that precision.

### 95% confidence intervals
Win rates, phase success rates, detection rates, and the rate-valued feasibility cells use the [Wilson score interval][wilson] at `z ≈ 1.960` (the standard-normal 97.5% quantile). Wilson is used in preference to the textbook Wald approximation because Wald collapses to `[0, 0]` or `[1, 1]` when zero or all runs succeed, implying false certainty for rare events. Wilson retains well-calibrated coverage across `p ∈ [0, 1]`.

Continuous metrics (duration, casualties, resources expended) are summarised by their mean with a 95% **percentile-bootstrap CI** on the mean, plus the 5th / 95th percentiles and standard deviation of the run distribution itself. The bootstrap draws 500 resamples from a deterministic `ChaCha8Rng` seeded from `scenario.simulation.seed` so the report is bit-identical across repeated runs. Keep the two quantities distinct: the bootstrap CI narrows as `n_runs` grows; the 5–95 percentile spread reflects inherent variability in the modelled outcome and does not.

[wilson]: https://en.wikipedia.org/wiki/Binomial_proportion_confidence_interval#Wilson_score_interval

### Confidence bucket derivation
The `[H]` / `[M]` / `[L]` tag on rate-valued feasibility cells is a coarse readability aid derived from the Wilson CI half-width at the scenario's run count:

| Bucket | Wilson half-width | Interpretation |
|---|---|---|
| `H` (High) | `< 0.03` | ±3 percentage points at 95% |
| `M` (Medium) | `< 0.08` | ±8 percentage points at 95% |
| `L` (Low) | otherwise (or `n < 30`) | Wide enough that comparing two `L` values is unsafe |

The `technology_readiness` bucket is a separate diagnostic: it is `L` when fewer than two phases exist in the chain, and otherwise buckets the coefficient of variation of per-phase base-success probabilities (`<0.15` → `H`, `<0.40` → `M`, else `L`). A `L` tag here means the chain's phases vary widely in expected success and a single "readiness" number is lossy, not that the MC estimate is imprecise.

### Author-flagged parameters
Authors can annotate `CampaignPhase.parameter_confidence` and `PhaseCost.confidence` in the TOML scenario to signal how defensible the input numbers are — `High` for commodity-parts costs or published rate cards, `Low` for wide expert estimates. Any phase or cost block flagged `Low` is listed in a dedicated section above when present. This complements, and does not replace, a full sensitivity sweep.

### Scenario-level confidence banner
The optional `[meta].confidence` field tags the scenario as a whole:

| Tag | Intended meaning |
|---|---|
| `High` | Publication-ready rigor — every capability parameter is backed by a cited open source. |
| `Medium` | Working draft — structurally complete but some parameters still rest on expert guess. |
| `Low` | Conceptual sketch — intended to illustrate a mechanic, not to stand as analysis. |

This is a coarse, author-asserted flag. It is *not* derived from the MC output and does not narrow or widen any CI — it tells the reader how much weight to place on the inputs before any sampling question comes into play.

### Calibration confidence (Epic N)
The optional `[meta.historical_analogue]` block opts the scenario into back-testing against a declared precedent. When present and the run set is non-empty, this section surfaces a `Calibration confidence` tag — `Pass` / `Marginal` / `Fail` — that mirrors the per-observation roll-up shown in the Calibration section above. The tag complements (does not replace) the parameter-defensibility tag in the header banner: parameter defensibility says "are the inputs sourceable?"; calibration says "do the outputs match a documented precedent?". The two answer different trust questions and a scenario can in principle Pass one and Fail the other.

A `Fail` verdict here does *not* invalidate the report — it tells the analyst that the scenario's structural model produces outputs inconsistent with the declared precedent at the parameters chosen. The next step is usually a sensitivity sweep on the low-confidence parameters listed above to identify which inputs would need to move for the verdict to flip.
"#;
