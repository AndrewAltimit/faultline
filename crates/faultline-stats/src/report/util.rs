//! Shared formatting helpers for the report renderer.
//!
//! Helpers that more than one section consumes live here so the section
//! modules can stay focused on their own structural concerns. Helpers
//! used by exactly one section live in that section's module instead.

use faultline_types::stats::ConfidenceLevel;

// Re-exported from the crate-level `markdown` module so the report
// submodules and the `explain` renderer share the exact same escape
// rules. Keeping the function in one place prevents silent divergence
// when the rules need to grow (e.g. neutralizing additional table-
// layout characters).
pub(super) use crate::markdown::escape_md_cell;

/// Adaptive number formatting: proportions get three decimals, larger
/// magnitudes round to whole units. Keeps the metrics table legible
/// whether it's showing `0.234` tension or `2_500` casualties.
pub(super) fn fmt_scalar(v: f64) -> String {
    let abs = v.abs();
    if abs < 1.0 {
        format!("{v:.3}")
    } else if abs < 100.0 {
        format!("{v:.2}")
    } else {
        format!("{v:.0}")
    }
}

pub(super) fn confidence_word(c: &ConfidenceLevel) -> &'static str {
    match c {
        ConfidenceLevel::High => "High",
        ConfidenceLevel::Medium => "Medium",
        ConfidenceLevel::Low => "Low",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_scalar_buckets_by_magnitude() {
        assert_eq!(fmt_scalar(0.234), "0.234");
        assert_eq!(fmt_scalar(12.345), "12.35");
        assert_eq!(fmt_scalar(2_500.7), "2501");
        assert_eq!(fmt_scalar(-0.5), "-0.500");
        assert_eq!(fmt_scalar(0.0), "0.000");
    }
}
