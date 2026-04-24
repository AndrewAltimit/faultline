//! Confidence intervals for Monte Carlo output.
//!
//! Two estimators:
//!
//! * [`wilson_score_interval`] — closed-form 95% CI for binomial
//!   proportions (win rates, phase success rates, detection rates).
//!   Used instead of the textbook Wald approximation because Wilson
//!   retains well-calibrated coverage near `p = 0` and `p = 1`, which
//!   is exactly where scenario rates spend most of their time.
//! * [`percentile_bootstrap_ci`] — non-parametric resampling CI for
//!   continuous metrics (duration, casualties, cost asymmetry). Uses a
//!   caller-supplied [`ChaCha8Rng`] so results are deterministic given
//!   a seed — matching the rest of Faultline.
//!
//! Both are independent of the engine; they only touch already-collected
//! [`RunResult`](faultline_types::stats::RunResult) data.

use faultline_types::stats::ConfidenceInterval;
use rand::Rng;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

/// Standard-normal quantile for a 95% two-sided CI.
pub const Z_95: f64 = 1.959_963_984_540_054;

// ---------------------------------------------------------------------------
// Wilson score interval
// ---------------------------------------------------------------------------

/// A Wilson score interval for a binomial proportion.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct WilsonInterval {
    /// Observed proportion `successes / n`.
    pub p_hat: f64,
    /// Lower bound of the 95% interval.
    pub lower: f64,
    /// Upper bound of the 95% interval.
    pub upper: f64,
    /// Sample size used to compute the interval.
    pub n: u32,
}

impl WilsonInterval {
    /// Half the width of the interval.
    pub fn half_width(&self) -> f64 {
        (self.upper - self.lower) * 0.5
    }
}

impl From<WilsonInterval> for ConfidenceInterval {
    fn from(w: WilsonInterval) -> Self {
        ConfidenceInterval::new(w.p_hat, w.lower, w.upper, w.n)
    }
}

impl From<BootstrapCI> for ConfidenceInterval {
    fn from(b: BootstrapCI) -> Self {
        // Bypasses `ConfidenceInterval::new` deliberately: that constructor
        // enforces `lower <= point <= upper`, which holds for Wilson CIs
        // (point == p_hat, bounds are center ± spread) but *not* for
        // percentile bootstrap CIs on skewed distributions — the sample
        // mean can legitimately fall outside the resample percentile band.
        // Enforcing the stronger invariant here would panic in debug builds
        // and silently emit the skewed bounds in release. Instead, keep
        // the universally-valid `lower <= upper` check and preserve the
        // point estimate as reported by the bootstrap.
        //
        // `n` documents the sample supporting the estimate, not the
        // resample count — saturating cast tolerates pathological sample
        // sizes above `u32::MAX`.
        debug_assert!(
            b.point.is_finite() && b.lower.is_finite() && b.upper.is_finite(),
            "BootstrapCI bounds must be finite: point={} lower={} upper={}",
            b.point,
            b.lower,
            b.upper
        );
        debug_assert!(
            b.lower <= b.upper,
            "BootstrapCI invariant violated: lower={} upper={}",
            b.lower,
            b.upper
        );
        ConfidenceInterval {
            point: b.point,
            lower: b.lower,
            upper: b.upper,
            n: u32::try_from(b.n_samples).unwrap_or(u32::MAX),
        }
    }
}

/// Wilson score 95% confidence interval for a binomial proportion.
///
/// Given `successes` out of `n` trials, returns the 95% interval for
/// the underlying probability. Returns `None` when `n == 0` (the
/// proportion is undefined).
///
/// Formula:
///
/// ```text
///   center = (p̂ + z² / 2n) / (1 + z² / n)
///   spread = z * sqrt(p̂(1-p̂)/n + z²/4n²) / (1 + z² / n)
///   [lower, upper] = center ± spread
/// ```
///
/// Clamps the returned bounds to `[0, 1]` so downstream formatting
/// never surfaces out-of-range values due to floating-point drift.
pub fn wilson_score_interval(successes: u32, n: u32) -> Option<WilsonInterval> {
    if n == 0 {
        return None;
    }
    let n_f = f64::from(n);
    let s_f = f64::from(successes);
    // Clamp p_hat to [0, 1] in case of caller-side rounding.
    let p_hat = (s_f / n_f).clamp(0.0, 1.0);
    let z = Z_95;
    let z2 = z * z;
    let denom = 1.0 + z2 / n_f;
    let center = (p_hat + z2 / (2.0 * n_f)) / denom;
    let spread = z * (p_hat * (1.0 - p_hat) / n_f + z2 / (4.0 * n_f * n_f)).sqrt() / denom;
    Some(WilsonInterval {
        p_hat,
        lower: (center - spread).clamp(0.0, 1.0),
        upper: (center + spread).clamp(0.0, 1.0),
        n,
    })
}

/// Wilson interval from an already-computed rate `p_hat ∈ [0, 1]` and
/// sample size. Convenience for callers that carry rates as `f64`.
pub fn wilson_from_rate(p_hat: f64, n: u32) -> Option<WilsonInterval> {
    if n == 0 {
        return None;
    }
    // Surface upstream drift in debug/test builds; in release, the clamp
    // keeps the arithmetic well-defined rather than emitting NaN/Inf.
    debug_assert!(
        (0.0..=1.0).contains(&p_hat),
        "p_hat {p_hat} out of range [0, 1]"
    );
    let successes = (p_hat.clamp(0.0, 1.0) * f64::from(n)).round() as u32;
    wilson_score_interval(successes, n)
}

// ---------------------------------------------------------------------------
// Bootstrap CI
// ---------------------------------------------------------------------------

/// A percentile-bootstrap confidence interval for a continuous statistic.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct BootstrapCI {
    /// Point estimate on the original sample (the mean).
    pub point: f64,
    /// Lower percentile of the resampled means.
    pub lower: f64,
    /// Upper percentile of the resampled means.
    pub upper: f64,
    /// Size of the original sample the CI was computed from — this is
    /// the "support" of the estimate and is what [`ConfidenceInterval`]
    /// records, not the resample count.
    pub n_samples: usize,
    /// Number of bootstrap resamples drawn to build the distribution.
    /// Affects the *precision* of the CI's endpoints, not the
    /// underlying sample's information content.
    pub n_resamples: usize,
    /// Two-sided alpha level (e.g. `0.05` for a 95% CI).
    pub alpha: f64,
}

/// Percentile bootstrap CI on the mean of `values`.
///
/// Draws `n_resamples` bootstrap samples (with replacement) from the
/// input, takes the mean of each, and returns the `alpha/2` and
/// `1 - alpha/2` percentiles. Deterministic: identical `(values,
/// n_resamples, alpha, rng)` produce identical output.
///
/// Returns `None` if the input is empty or `n_resamples == 0`.
pub fn percentile_bootstrap_ci(
    values: &[f64],
    n_resamples: usize,
    alpha: f64,
    rng: &mut ChaCha8Rng,
) -> Option<BootstrapCI> {
    if values.is_empty() || n_resamples == 0 {
        return None;
    }
    let n_samples = values.len();
    let point = values.iter().copied().sum::<f64>() / n_samples as f64;

    // Special case: single-element input means every resample is
    // identical, so the CI collapses to a point.
    if n_samples == 1 {
        return Some(BootstrapCI {
            point,
            lower: point,
            upper: point,
            n_samples,
            n_resamples,
            alpha,
        });
    }

    let mut resampled_means = Vec::with_capacity(n_resamples);
    for _ in 0..n_resamples {
        let mut sum = 0.0_f64;
        for _ in 0..n_samples {
            let idx = rng.gen_range(0..n_samples);
            sum += values[idx];
        }
        resampled_means.push(sum / n_samples as f64);
    }
    resampled_means.sort_by(|a, b| a.total_cmp(b));

    // Surface caller mistakes in debug/test builds; in release, silently
    // clamp to the usable range so we never emit a degenerate percentile.
    debug_assert!(
        alpha > 0.0 && alpha <= 0.5,
        "alpha {alpha} out of range (0, 0.5]"
    );
    let alpha = alpha.clamp(f64::EPSILON, 0.5);
    let lower = percentile_sorted(&resampled_means, 100.0 * alpha * 0.5);
    let upper = percentile_sorted(&resampled_means, 100.0 * (1.0 - alpha * 0.5));

    Some(BootstrapCI {
        point,
        lower,
        upper,
        n_samples,
        n_resamples,
        alpha,
    })
}

/// Convenience: seeded bootstrap CI (caller supplies only a seed).
pub fn percentile_bootstrap_ci_seeded(
    values: &[f64],
    n_resamples: usize,
    alpha: f64,
    seed: u64,
) -> Option<BootstrapCI> {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    percentile_bootstrap_ci(values, n_resamples, alpha, &mut rng)
}

fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let rank = (p / 100.0) * (sorted.len() as f64 - 1.0);
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    let frac = rank - rank.floor();
    if lower == upper || upper >= sorted.len() {
        sorted[lower.min(sorted.len() - 1)]
    } else {
        sorted[lower] * (1.0 - frac) + sorted[upper] * frac
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wilson_rejects_zero_n() {
        assert!(wilson_score_interval(0, 0).is_none());
        assert!(wilson_from_rate(0.5, 0).is_none());
    }

    #[test]
    fn wilson_p_zero_is_bounded_above_zero() {
        // 0 successes in 100 trials: lower must be exactly 0, upper
        // must be strictly positive (unlike Wald, which collapses to
        // [0, 0] and pretends there is no uncertainty).
        let ci = wilson_score_interval(0, 100).expect("n=100");
        assert!((ci.p_hat - 0.0).abs() < f64::EPSILON);
        assert!((ci.lower - 0.0).abs() < 1e-12);
        assert!(
            ci.upper > 0.0,
            "Wilson upper bound at p=0 should be strictly positive, got {}",
            ci.upper
        );
        assert!(
            ci.upper < 0.05,
            "Wilson upper bound for 0/100 should be small, got {}",
            ci.upper
        );
    }

    #[test]
    fn wilson_p_one_is_bounded_below_one() {
        let ci = wilson_score_interval(100, 100).expect("n=100");
        assert!((ci.p_hat - 1.0).abs() < f64::EPSILON);
        assert!((ci.upper - 1.0).abs() < 1e-12);
        assert!(
            ci.lower < 1.0,
            "Wilson lower bound at p=1 should be strictly below 1"
        );
    }

    #[test]
    fn wilson_narrows_with_sample_size() {
        let small = wilson_score_interval(5, 10).expect("n=10");
        let large = wilson_score_interval(500, 1000).expect("n=1000");
        assert!(
            large.half_width() < small.half_width(),
            "CI width should shrink with n: small={}, large={}",
            small.half_width(),
            large.half_width()
        );
    }

    #[test]
    fn wilson_contains_p_hat() {
        // For any (successes, n) with 0 < successes < n, p̂ must fall
        // inside the CI. Uses a handful of scattered points.
        for (s, n) in [(1, 100), (50, 100), (99, 100), (3, 17), (42, 137)] {
            let ci = wilson_score_interval(s, n).expect("n>0");
            assert!(
                ci.lower <= ci.p_hat && ci.p_hat <= ci.upper,
                "p_hat {} not in [{}, {}] for {}/{}",
                ci.p_hat,
                ci.lower,
                ci.upper,
                s,
                n
            );
        }
    }

    #[test]
    fn wilson_matches_published_reference_values() {
        // Hand-derived reference values using the closed-form Wilson
        // formula from Agresti & Coull (1998). Any drift here almost
        // certainly means an arithmetic error was introduced in the
        // implementation. Tolerance is 1e-4 to absorb the difference
        // between Z_95 = 1.959964... and the rounded 1.960 commonly
        // used in hand-worked textbook examples.
        //
        // ┌─────────────┬───────────────────┐
        // │  (k, n)     │  95% Wilson CI    │
        // ├─────────────┼───────────────────┤
        // │  50 / 100   │  0.4038 – 0.5962  │
        // │  10 / 100   │  0.0552 – 0.1744  │
        // │  90 / 100   │  0.8256 – 0.9448  │
        // │   0 /  10   │  0.0000 – 0.2775  │
        // │  10 /  10   │  0.7225 – 1.0000  │
        // └─────────────┴───────────────────┘
        let tol = 1e-4;
        let cases: &[(u32, u32, f64, f64)] = &[
            (50, 100, 0.4038, 0.5962),
            (10, 100, 0.0552, 0.1744),
            (90, 100, 0.8256, 0.9448),
            (0, 10, 0.0000, 0.2775),
            (10, 10, 0.7225, 1.0000),
        ];
        for &(k, n, lo, hi) in cases {
            let ci = wilson_score_interval(k, n).expect("n>0");
            assert!(
                (ci.lower - lo).abs() < tol,
                "wilson({k}, {n}).lower = {} (expected ~{lo})",
                ci.lower
            );
            assert!(
                (ci.upper - hi).abs() < tol,
                "wilson({k}, {n}).upper = {} (expected ~{hi})",
                ci.upper
            );
        }
    }

    #[test]
    fn wilson_symmetric_at_p_half() {
        // At p̂ = 0.5 the Wilson CI is symmetric about 0.5 — a useful
        // invariant that catches sign errors in the spread term.
        let ci = wilson_score_interval(50, 100).expect("n>0");
        let low_gap = 0.5 - ci.lower;
        let high_gap = ci.upper - 0.5;
        assert!(
            (low_gap - high_gap).abs() < 1e-12,
            "CI at p̂=0.5 should be symmetric: 0.5-lo={low_gap}, hi-0.5={high_gap}"
        );
    }

    #[test]
    fn wilson_from_rate_roundtrips_to_successes() {
        // 0.25 of 100 = 25 successes.
        let from_rate = wilson_from_rate(0.25, 100).expect("n>0");
        let from_count = wilson_score_interval(25, 100).expect("n>0");
        assert!((from_rate.lower - from_count.lower).abs() < 1e-12);
        assert!((from_rate.upper - from_count.upper).abs() < 1e-12);
    }

    #[test]
    fn bootstrap_rejects_empty_input() {
        let mut rng = ChaCha8Rng::seed_from_u64(0);
        assert!(percentile_bootstrap_ci(&[], 100, 0.05, &mut rng).is_none());
    }

    #[test]
    fn bootstrap_rejects_zero_resamples() {
        let mut rng = ChaCha8Rng::seed_from_u64(0);
        let values = [1.0, 2.0, 3.0];
        assert!(percentile_bootstrap_ci(&values, 0, 0.05, &mut rng).is_none());
    }

    #[test]
    fn bootstrap_single_value_is_a_point() {
        let ci = percentile_bootstrap_ci_seeded(&[42.0], 1000, 0.05, 1)
            .expect("single value should produce a CI");
        assert!((ci.point - 42.0).abs() < f64::EPSILON);
        assert!((ci.lower - 42.0).abs() < f64::EPSILON);
        assert!((ci.upper - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn bootstrap_is_deterministic_under_seed() {
        let values: Vec<f64> = (0..50).map(f64::from).collect();
        let a = percentile_bootstrap_ci_seeded(&values, 500, 0.05, 7).expect("non-empty");
        let b = percentile_bootstrap_ci_seeded(&values, 500, 0.05, 7).expect("non-empty");
        assert_eq!(a, b);
    }

    #[test]
    fn bootstrap_different_seeds_yield_different_intervals() {
        // Not a strict guarantee for any seed pair, but for 100
        // resamples on a skewed sample this is overwhelmingly likely.
        let values: Vec<f64> = (0..50).map(f64::from).collect();
        let a = percentile_bootstrap_ci_seeded(&values, 100, 0.05, 1).expect("non-empty");
        let b = percentile_bootstrap_ci_seeded(&values, 100, 0.05, 2).expect("non-empty");
        assert_ne!(a, b, "distinct seeds should not collide");
    }

    #[test]
    fn bootstrap_brackets_sample_mean_for_large_samples() {
        let values: Vec<f64> = (0..200).map(f64::from).collect();
        let ci = percentile_bootstrap_ci_seeded(&values, 2000, 0.05, 123).expect("non-empty");
        let mean = values.iter().copied().sum::<f64>() / values.len() as f64;
        assert!(
            ci.lower <= mean && mean <= ci.upper,
            "point mean {mean} should fall inside CI [{}, {}]",
            ci.lower,
            ci.upper
        );
    }

    #[test]
    fn bootstrap_ci_records_original_sample_size() {
        // n_samples must reflect the input slice length, not the
        // resample count — mixing these inflates perceived precision
        // downstream.
        let values: Vec<f64> = (0..25).map(f64::from).collect();
        let ci = percentile_bootstrap_ci_seeded(&values, 500, 0.05, 1).expect("non-empty");
        assert_eq!(ci.n_samples, 25);
        assert_eq!(ci.n_resamples, 500);
    }

    #[test]
    fn bootstrap_conversion_to_confidence_interval_uses_samples() {
        let values: Vec<f64> = (0..25).map(f64::from).collect();
        let boot = percentile_bootstrap_ci_seeded(&values, 500, 0.05, 1).expect("non-empty");
        let ci: ConfidenceInterval = boot.into();
        assert_eq!(
            ci.n, 25,
            "ConfidenceInterval.n should track the original sample size, not resample count"
        );
    }

    #[test]
    fn bootstrap_conversion_accepts_point_outside_interval() {
        // Percentile bootstrap CIs on skewed distributions can place the
        // sample mean outside [lower, upper]. The `From<BootstrapCI>`
        // impl must not panic on this shape — the stronger `lower <=
        // point <= upper` invariant applies only to Wilson CIs.
        let skewed = BootstrapCI {
            point: 100.0,
            lower: 1.0,
            upper: 5.0,
            n_samples: 20,
            n_resamples: 500,
            alpha: 0.05,
        };
        let ci: ConfidenceInterval = skewed.into();
        assert!((ci.point - 100.0).abs() < f64::EPSILON);
        assert!((ci.lower - 1.0).abs() < f64::EPSILON);
        assert!((ci.upper - 5.0).abs() < f64::EPSILON);
        assert_eq!(ci.n, 20);
    }

    #[test]
    fn bootstrap_coverage_is_reasonable() {
        // Weak statistical smoke test: if we draw many synthetic
        // "samples" from a known distribution and build a bootstrap CI
        // for each, the true mean should be covered by most of them.
        // This won't hit the asymptotic 95% exactly for small samples,
        // but a floor of ~80% catches gross errors.
        let mut rng_outer = ChaCha8Rng::seed_from_u64(99);
        let true_mean = 10.0_f64;
        let mut covered: u32 = 0;
        let trials: u32 = 200;
        let sample_size: u32 = 40;
        for trial in 0..trials {
            // Generate a synthetic "sample" from uniform(0, 20).
            let sample: Vec<f64> = (0..sample_size)
                .map(|_| rng_outer.gen_range(0.0..20.0))
                .collect();
            let ci = percentile_bootstrap_ci_seeded(&sample, 500, 0.05, u64::from(trial))
                .expect("non-empty");
            if ci.lower <= true_mean && true_mean <= ci.upper {
                covered += 1;
            }
        }
        let coverage = f64::from(covered) / f64::from(trials);
        assert!(
            coverage >= 0.80,
            "bootstrap coverage was {coverage:.2} ({covered}/{trials}); expected ≥ 0.80"
        );
    }
}
