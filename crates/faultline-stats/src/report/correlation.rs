//! Output Correlation Matrix section: Pearson correlations
//! across the per-run scalars in the batch.
//!
//! Elided when no matrix was computed, when the matrix is empty, or
//! when every off-diagonal entry is `None` (degenerate scenario where
//! every output is constant).

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloSummary;

use super::ReportSection;

pub(super) struct CorrelationMatrix;

impl ReportSection for CorrelationMatrix {
    fn render(&self, summary: &MonteCarloSummary, _scenario: &Scenario, out: &mut String) {
        let matrix = match summary.correlation_matrix.as_ref() {
            Some(m) if !m.labels.is_empty() => m,
            _ => return,
        };
        // If every off-diagonal entry is None (degenerate scenario where
        // every output is constant) the matrix is uninformative — elide
        // the section rather than print a wall of `—`.
        let n = matrix.labels.len();
        let any_off_diag = (0..n)
            .flat_map(|i| (0..n).map(move |j| (i, j)))
            .filter(|(i, j)| i != j)
            .any(|(i, j)| matrix.values[i * n + j].is_some());
        if !any_off_diag {
            return;
        }
        let _ = writeln!(out, "## Output Correlation Matrix");
        let _ = writeln!(
            out,
            "Pearson correlations across the {} runs in the batch. A constant series shows up as `—` (correlation undefined). High |r| between two outputs flags shared underlying drivers; near-zero r means they move independently across runs.",
            matrix.n
        );
        let _ = writeln!(out);
        // Header.
        let _ = write!(out, "|     |");
        for label in &matrix.labels {
            let _ = write!(out, " `{}` |", label);
        }
        let _ = writeln!(out);
        let _ = write!(out, "|---|");
        for _ in &matrix.labels {
            let _ = write!(out, "---|");
        }
        let _ = writeln!(out);
        for (i, row_label) in matrix.labels.iter().enumerate() {
            let _ = write!(out, "| `{}` |", row_label);
            for j in 0..n {
                match matrix.values[i * n + j] {
                    Some(v) => {
                        let _ = write!(out, " {:+.2} |", v);
                    },
                    None => {
                        let _ = write!(out, " — |");
                    },
                }
            }
            let _ = writeln!(out);
        }
        let _ = writeln!(out);
    }
}
