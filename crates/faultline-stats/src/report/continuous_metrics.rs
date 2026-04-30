//! Continuous Metrics section: per-metric mean (with optional 95%
//! percentile-bootstrap CI), median, 5th–95th percentile, std dev.
//!
//! Elided when no scalar distributions were captured.

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::{DistributionStats, MetricType, MonteCarloSummary};

use super::ReportSection;
use super::util::fmt_scalar;

pub(super) struct ContinuousMetrics;

impl ReportSection for ContinuousMetrics {
    fn render(&self, summary: &MonteCarloSummary, _scenario: &Scenario, out: &mut String) {
        if summary.metric_distributions.is_empty() {
            return;
        }
        // Header must match cell content: if any metric lacks a bootstrap CI
        // (e.g. a legacy `MonteCarloSummary` deserialized from a pre-bootstrap
        // build where `bootstrap_ci_mean` defaults to `None`), `fmt_mean_with_bootstrap`
        // falls back to a bare mean for those rows. A blanket "Mean (95% bootstrap CI)"
        // header would then mislabel those cells.
        let all_have_ci = summary
            .metric_distributions
            .values()
            .all(|s| s.bootstrap_ci_mean.is_some());
        let mean_header = if all_have_ci {
            "Mean (95% bootstrap CI)"
        } else {
            "Mean"
        };
        let _ = writeln!(out, "## Continuous Metrics");
        let _ = writeln!(
            out,
            "| Metric | {mean_header} | Median | 5th – 95th pct | Std dev |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|");
        for (metric, stats) in &summary.metric_distributions {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} – {} | {} |",
                metric_label(metric),
                fmt_mean_with_bootstrap(stats, all_have_ci),
                fmt_scalar(stats.median),
                fmt_scalar(stats.percentile_5),
                fmt_scalar(stats.percentile_95),
                fmt_scalar(stats.std_dev),
            );
        }
        if all_have_ci {
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "_Bootstrap CIs use 500 percentile-bootstrap resamples seeded from the scenario. Percentiles describe the *distribution* of run outcomes — not uncertainty on the mean._"
            );
        }
        let _ = writeln!(out);
    }
}

fn metric_label(m: &MetricType) -> String {
    match m {
        MetricType::Duration => "Duration (ticks)".into(),
        MetricType::FinalTension => "Final tension".into(),
        MetricType::TotalCasualties => "Total casualties".into(),
        MetricType::InfrastructureDamage => "Infrastructure damage".into(),
        MetricType::CivilianDisplacement => "Civilian displacement".into(),
        MetricType::ResourcesExpended => "Resources expended".into(),
        MetricType::Custom(s) => s.clone(),
    }
}

// `show_ci` must mirror the column header: if the header does not advertise
// a bootstrap CI (because some other row in the same table lacks one), this
// row must also suppress its bounds even if its own `bootstrap_ci_mean` is
// `Some(..)`. Otherwise the cell carries CI syntax under a plain "Mean" header.
fn fmt_mean_with_bootstrap(stats: &DistributionStats, show_ci: bool) -> String {
    match (show_ci, stats.bootstrap_ci_mean.as_ref()) {
        (true, Some(ci)) => format!(
            "{} ({} – {})",
            fmt_scalar(stats.mean),
            fmt_scalar(ci.lower),
            fmt_scalar(ci.upper)
        ),
        _ => fmt_scalar(stats.mean),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::test_support::{empty_summary, minimal_scenario};
    use faultline_types::stats::ConfidenceInterval;

    fn dist_with_ci(mean: f64, ci: Option<ConfidenceInterval>) -> DistributionStats {
        DistributionStats {
            mean,
            median: mean,
            std_dev: 1.0,
            min: mean - 2.0,
            max: mean + 2.0,
            percentile_5: mean - 1.0,
            percentile_95: mean + 1.0,
            bootstrap_ci_mean: ci,
        }
    }

    #[test]
    fn elides_when_no_distributions() {
        let mut out = String::new();
        ContinuousMetrics.render(&empty_summary(), &minimal_scenario(), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn header_advertises_ci_only_when_every_row_has_one() {
        // If any row lacks a CI, the column header must read "Mean" (not
        // "Mean (95% bootstrap CI)") and that row's cell must omit
        // bounds — otherwise the cell carries CI syntax under a plain
        // header. Pin both halves of the contract.
        let mut summary = empty_summary();
        summary.metric_distributions.insert(
            MetricType::Duration,
            dist_with_ci(10.0, Some(ConfidenceInterval::new(10.0, 9.5, 10.5, 100))),
        );
        summary
            .metric_distributions
            .insert(MetricType::TotalCasualties, dist_with_ci(50.0, None));
        let mut out = String::new();
        ContinuousMetrics.render(&summary, &minimal_scenario(), &mut out);
        assert!(out.contains("| Mean |"), "header should be bare 'Mean'");
        assert!(
            !out.contains("Mean (95% bootstrap CI)"),
            "header should not advertise CI when one row lacks it; got:\n{out}"
        );
        // Even the row that *has* a CI must render bare under the bare header.
        assert!(
            !out.contains("(9.50 – 10.50)") && !out.contains("(9.5 – 10.5)"),
            "CI must be suppressed when header is bare; got:\n{out}"
        );
    }
}
