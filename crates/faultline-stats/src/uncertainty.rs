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
        ConfidenceInterval {
            point: w.p_hat,
            lower: w.lower,
            upper: w.upper,
            n: w.n,
        }
    }
}

impl From<BootstrapCI> for ConfidenceInterval {
    fn from(b: BootstrapCI) -> Self {
        ConfidenceInterval {
            point: b.point,
            lower: b.lower,
            upper: b.upper,
            n: b.n_resamples as u32,
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
    /// Number of bootstrap resamples used.
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
    let n = values.len();
    let point = values.iter().copied().sum::<f64>() / n as f64;

    // Special case: single-element input means every resample is
    // identical, so the CI collapses to a point.
    if n == 1 {
        return Some(BootstrapCI {
            point,
            lower: point,
            upper: point,
            n_resamples,
            alpha,
        });
    }

    let mut resampled_means = Vec::with_capacity(n_resamples);
    for _ in 0..n_resamples {
        let mut sum = 0.0_f64;
        for _ in 0..n {
            let idx = rng.gen_range(0..n);
            sum += values[idx];
        }
        resampled_means.push(sum / n as f64);
    }
    resampled_means.sort_by(|a, b| a.total_cmp(b));

    let alpha = alpha.clamp(f64::EPSILON, 0.5);
    let lower = percentile_sorted(&resampled_means, 100.0 * alpha * 0.5);
    let upper = percentile_sorted(&resampled_means, 100.0 * (1.0 - alpha * 0.5));

    Some(BootstrapCI {
        point,
        lower,
        upper,
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
}
