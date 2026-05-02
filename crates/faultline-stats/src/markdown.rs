//! Crate-level Markdown rendering helpers shared between the report
//! pipeline and the explain pipeline.
//!
//! Both surfaces emit Markdown tables built from author-supplied
//! strings (faction / phase / indicator names, custom labels). They
//! must apply the *same* escape rules so a cell that's safe in one
//! report is safe in the other; keeping the helper in one place
//! prevents silent divergence when the rules need to grow (e.g.
//! neutralizing additional table-layout characters).
//!
//! Visibility is `pub(crate)` so the `report::util` re-export and
//! `explain::render_markdown` can call into it without exposing the
//! escape contract on the public API surface.

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
pub(crate) fn escape_md_cell(s: &str) -> String {
    s.replace('\\', r"\\")
        .replace('|', r"\|")
        .replace('`', r"\`")
        .replace(['\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_md_cell_neutralizes_pipes_and_newlines() {
        // Bare strings: pipe must be escaped, newlines collapsed,
        // backslash doubled so the escape itself is not ambiguous,
        // backticks escaped so an unbalanced one can't open an inline
        // code span that bleeds into adjacent cells.
        assert_eq!(escape_md_cell("a|b"), r"a\|b");
        assert_eq!(escape_md_cell("line1\nline2"), "line1 line2");
        assert_eq!(escape_md_cell("line1\r\nline2"), "line1  line2");
        assert_eq!(escape_md_cell(r"back\slash"), r"back\\slash");
        assert_eq!(escape_md_cell("a`b"), r"a\`b");
        assert_eq!(escape_md_cell("clean"), "clean");
    }
}
