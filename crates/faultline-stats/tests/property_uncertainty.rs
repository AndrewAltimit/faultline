//! Property tests for `faultline_stats::uncertainty`.
//!
//! Existing unit tests in `uncertainty.rs` pin specific reference values
//! (Agresti & Coull table) and a handful of hand-picked `(successes, n)`
//! cases. These properties cover the *invariants* across the entire
//! input space — the kind of "for all seeds / all inputs" guarantees
//! the deterministic, seeded design is built to support but the fixed-
//! seed test files don't reach.
//!
//! Three invariants worth pinning per the May 2026 refresh:
//!
//! 1. **Wilson bounds always contain the point estimate.** The closed-
//!    form Wilson algebra satisfies this exactly, but floating-point
//!    drift can push the lower bound microscopically above `p_hat` (or
//!    the upper bound below it) at the boundary. The `min(p_hat)` /
//!    `max(p_hat)` guards in `wilson_score_interval` exist *because* of
//!    this; the property test pins that they keep working under
//!    random `(successes, n)`.
//! 2. **Wilson bounds shrink monotonically with sample size.** For a
//!    fixed underlying rate `p`, increasing `n` must not widen the CI.
//!    A regression here would silently let users believe more data
//!    yields less precision.
//! 3. **Bootstrap CI is bit-identical for the same seed.** The
//!    determinism contract is the foundation of `--verify`, the
//!    manifest replay system, and every CI guard around it. A
//!    randomized-input cross-check is much cheaper insurance than
//!    waiting for `verify-bundled` to catch a regression.

use faultline_stats::uncertainty::{
    percentile_bootstrap_ci_seeded, wilson_from_rate, wilson_score_interval,
};
use proptest::prelude::*;

proptest! {
    /// For any `(successes, n)` with `0 < n <= 10_000` and `successes <= n`,
    /// the Wilson interval must satisfy `lower <= p_hat <= upper` and
    /// stay clamped to `[0, 1]`. The implementation enforces this via
    /// `min(p_hat)` / `max(p_hat)` clamps after the closed-form
    /// algebra; the property guards against a refactor that drops them.
    #[test]
    fn wilson_bounds_contain_point_estimate(
        n in 1u32..=10_000,
        frac in 0u32..=10_000,
    ) {
        // Map `frac` to `successes` modulo `n+1` so we cover the full
        // `[0, n]` range without rejecting samples — proptest's reject-
        // and-resample mode is much slower for tightly constrained
        // pairs like this.
        let successes = frac % (n + 1);
        let ci = wilson_score_interval(successes, n)
            .expect("n > 0 so wilson is defined");
        prop_assert!(ci.lower >= 0.0, "lower {} below 0", ci.lower);
        prop_assert!(ci.upper <= 1.0, "upper {} above 1", ci.upper);
        prop_assert!(
            ci.lower <= ci.p_hat,
            "lower {} > p_hat {}",
            ci.lower,
            ci.p_hat
        );
        prop_assert!(
            ci.p_hat <= ci.upper,
            "p_hat {} > upper {}",
            ci.p_hat,
            ci.upper
        );
        prop_assert!(
            ci.lower <= ci.upper,
            "lower {} > upper {}",
            ci.lower,
            ci.upper
        );
    }

    /// Increasing the sample size at a fixed underlying rate must not
    /// widen the CI. We compare half-widths at `n` vs `10*n` for a
    /// random rate; the larger sample's interval must be at least as
    /// tight (allowing a 1e-9 slack to absorb floating-point noise).
    #[test]
    fn wilson_narrows_with_more_samples(
        rate_thousandths in 0u32..=1_000,
        n_small in 10u32..=100,
    ) {
        let rate = f64::from(rate_thousandths) / 1_000.0;
        let n_large = n_small * 10;
        let small = wilson_from_rate(rate, n_small)
            .expect("n > 0");
        let large = wilson_from_rate(rate, n_large)
            .expect("n > 0");
        let small_w = small.upper - small.lower;
        let large_w = large.upper - large.lower;
        // Strict monotonicity at p_hat == 0 with very small n collapses
        // because the lower bound is already pinned to 0 in both — the
        // 1e-9 slack covers that boundary case without weakening the
        // invariant on the body of the input space.
        prop_assert!(
            large_w <= small_w + 1e-9,
            "n={} half-width {} should be ≤ n={} half-width {}",
            n_large,
            large_w,
            n_small,
            small_w
        );
    }

    /// `wilson_from_rate(p, n)` and `wilson_score_interval(round(p*n), n)`
    /// must agree to floating-point tolerance. This pins the
    /// rate→count→interval composition so the call sites in
    /// `analysis.rs` (which sometimes carry rates as `f64`) stay in
    /// lock-step with the canonical count-based path.
    #[test]
    fn wilson_from_rate_matches_count_form(
        rate_thousandths in 0u32..=1_000,
        n in 1u32..=10_000,
    ) {
        let rate = f64::from(rate_thousandths) / 1_000.0;
        let from_rate = wilson_from_rate(rate, n).expect("n > 0");
        let successes = (rate * f64::from(n)).round() as u32;
        let from_count = wilson_score_interval(successes, n).expect("n > 0");
        prop_assert!((from_rate.lower - from_count.lower).abs() < 1e-12);
        prop_assert!((from_rate.upper - from_count.upper).abs() < 1e-12);
        prop_assert_eq!(from_rate.n, from_count.n);
    }

    /// Two bootstrap CIs computed from the same `(values, seed)` must be
    /// bit-identical. The determinism contract for `percentile_bootstrap_ci_seeded`
    /// is that identical inputs always produce identical output; the
    /// pinned unit test only checks one fixed input pair, this checks
    /// it across random samples.
    #[test]
    fn bootstrap_seeded_is_deterministic(
        seed in any::<u64>(),
        values in proptest::collection::vec(-100.0_f64..100.0, 1..50),
    ) {
        let a = percentile_bootstrap_ci_seeded(&values, 200, 0.05, seed)
            .expect("non-empty input");
        let b = percentile_bootstrap_ci_seeded(&values, 200, 0.05, seed)
            .expect("non-empty input");
        prop_assert_eq!(a.point, b.point);
        prop_assert_eq!(a.lower, b.lower);
        prop_assert_eq!(a.upper, b.upper);
        prop_assert_eq!(a.n_samples, b.n_samples);
        prop_assert_eq!(a.n_resamples, b.n_resamples);
    }

    /// The bootstrap CI must always satisfy `lower <= upper`. (Per the
    /// `From<BootstrapCI>` impl note in `uncertainty.rs`, the *point*
    /// can fall outside the interval on skewed samples — but the
    /// bound-pair invariant always holds.)
    #[test]
    fn bootstrap_lower_le_upper(
        seed in any::<u64>(),
        values in proptest::collection::vec(-1000.0_f64..1000.0, 2..40),
    ) {
        let ci = percentile_bootstrap_ci_seeded(&values, 200, 0.05, seed)
            .expect("non-empty input");
        prop_assert!(
            ci.lower <= ci.upper,
            "lower {} > upper {}",
            ci.lower,
            ci.upper
        );
        prop_assert!(ci.point.is_finite());
        prop_assert!(ci.lower.is_finite());
        prop_assert!(ci.upper.is_finite());
    }
}
