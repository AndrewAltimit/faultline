//! Shared formatting helpers for the report renderer.
//!
//! Helpers that more than one section consumes live here so the section
//! modules can stay focused on their own structural concerns. Helpers
//! used by exactly one section live in that section's module instead.

use faultline_types::stats::ConfidenceLevel;

/// Escape user-supplied strings for inclusion in a Markdown table cell.
///
/// A literal `|` closes the cell early and breaks table rendering;
/// `\n` / `\r` would split the row across multiple table rows;
/// backticks open inline code spans that can leak formatting into
/// neighboring cells when unbalanced. All can appear in author-
/// supplied scenario fields (phase / indicator names, custom
/// discipline labels, escalation-rung action lists, environment-
/// window IDs), so escape them at the boundary rather than relying
/// on author hygiene.
pub(super) fn escape_md_cell(s: &str) -> String {
    s.replace('\\', r"\\")
        .replace('|', r"\|")
        .replace('`', r"\`")
        .replace(['\n', '\r'], " ")
}

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
    fn escape_md_cell_neutralizes_pipes_and_newlines() {
        // Bare strings: pipe must be escaped, newlines collapsed, backslash
        // doubled so the escape itself is not ambiguous, backticks escaped
        // so an unbalanced one can't open an inline code span that bleeds
        // into adjacent cells.
        assert_eq!(escape_md_cell("a|b"), r"a\|b");
        assert_eq!(escape_md_cell("line1\nline2"), "line1 line2");
        assert_eq!(escape_md_cell("line1\r\nline2"), "line1  line2");
        assert_eq!(escape_md_cell(r"back\slash"), r"back\\slash");
        assert_eq!(escape_md_cell("a`b"), r"a\`b");
        assert_eq!(escape_md_cell("clean"), "clean");
    }

    #[test]
    fn fmt_scalar_buckets_by_magnitude() {
        assert_eq!(fmt_scalar(0.234), "0.234");
        assert_eq!(fmt_scalar(12.345), "12.35");
        assert_eq!(fmt_scalar(2_500.7), "2501");
        assert_eq!(fmt_scalar(-0.5), "-0.500");
        assert_eq!(fmt_scalar(0.0), "0.000");
    }
}
