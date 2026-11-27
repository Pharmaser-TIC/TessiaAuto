//! The contract between `proceso run` and the phases.
//!
//! A phase's `run_phase` receives a [`PhaseRequest`], does the work for the
//! invoices of the cargue (claiming them in `proceso_estado`, running its
//! worker threads, storing outcomes) and returns a [`PhaseReport`].
//!
//! Recommended shape of a phase's `run_phase`:
//!
//! 1. resolve the invoices (`req.invoices`, or all of the cargue's);
//! 2. `ctx.proceso().load(phase, ..)`, apply `domain::phase::gate` for the ones
//!    not asked for explicitly, and put the skipped ones in `report.skipped`;
//! 3. `claim_available` the rest with `ctx.claim_options(cargue_id)`;
//! 4. work with `util::par::map` (`req.workers` or the phase default), calling
//!    `save_outcome` per invoice; a `false` return means the claim was taken
//!    over: drop the result;
//! 5. return the outcomes that were stored.

use crate::domain::{Outcome, PhaseState};

/// What the orchestrator asks of a phase.
#[derive(Debug, Clone, Default)]
pub struct PhaseRequest {
    /// The cargue (CSV stem) being processed.
    pub cargue_id: String,
    /// Only these invoices (normalized); `None` = every invoice of the cargue.
    /// Explicitly requested invoices bypass the gate, as `--invoices` does in Go.
    pub invoices: Option<Vec<String>>,
    /// Worker threads; `None` = the phase's default.
    pub workers: Option<usize>,
}

impl PhaseRequest {
    pub fn new(cargue_id: &str) -> Self {
        Self {
            cargue_id: cargue_id.to_owned(),
            ..Self::default()
        }
    }

    /// `workers` or `default`, at least 1.
    pub fn workers_or(&self, default: usize) -> usize {
        self.workers.unwrap_or(default).max(1)
    }
}

/// The result stored for one invoice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvoiceOutcome {
    pub invoice: String,
    pub outcome: Outcome,
}

/// Counters of a phase run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    pub done: usize,
    pub failed: usize,
    pub excluded: usize,
    pub manual_review: usize,
    /// Skipped by the gate or because another machine owns them.
    pub skipped: usize,
}

/// What a phase did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PhaseReport {
    /// Outcomes this run stored, in processing order.
    pub outcomes: Vec<InvoiceOutcome>,
    /// `(invoice, reason)` the run did not process (gate reason such as
    /// `RIPS_NO_OK`, or `EN_PROCESO_OTRA_MAQUINA`).
    pub skipped: Vec<(String, String)>,
}

impl PhaseReport {
    pub fn push(&mut self, invoice: &str, outcome: Outcome) {
        self.outcomes.push(InvoiceOutcome {
            invoice: invoice.to_owned(),
            outcome,
        });
    }

    pub fn skip(&mut self, invoice: &str, reason: &str) {
        self.skipped.push((invoice.to_owned(), reason.to_owned()));
    }

    pub fn counters(&self) -> Counters {
        let mut counters = Counters {
            skipped: self.skipped.len(),
            ..Counters::default()
        };
        for stored in &self.outcomes {
            match stored.outcome.state {
                PhaseState::Done => counters.done += 1,
                PhaseState::Failed => counters.failed += 1,
                PhaseState::Excluded => counters.excluded += 1,
                PhaseState::RevisionManual => counters.manual_review += 1,
                PhaseState::EnProgreso => {}
            }
        }
        counters
    }

    /// One Spanish summary line for the end of a command.
    pub fn summary(&self) -> String {
        let c = self.counters();
        format!(
            "{} procesadas: {} listas, {} fallidas, {} excluidas, {} en revisión manual, {} omitidas",
            self.outcomes.len(),
            c.done,
            c.failed,
            c.excluded,
            c.manual_review,
            c.skipped
        )
    }

    /// True when any invoice failed (the command should exit non-zero only
    /// when its Go counterpart does; most phases keep FAILED as data).
    pub fn has_failures(&self) -> bool {
        self.counters().failed > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_and_summary_follow_the_stored_outcomes() {
        let mut report = PhaseReport::default();
        report.push("A", Outcome::done());
        report.push("B", Outcome::failed("X"));
        report.push("C", Outcome::excluded("Y"));
        report.push("D", Outcome::manual_review("Z"));
        report.skip("E", "RIPS_NO_OK");
        assert_eq!(
            report.counters(),
            Counters {
                done: 1,
                failed: 1,
                excluded: 1,
                manual_review: 1,
                skipped: 1
            }
        );
        assert!(report.has_failures());
        assert_eq!(
            report.summary(),
            "4 procesadas: 1 listas, 1 fallidas, 1 excluidas, 1 en revisión manual, 1 omitidas"
        );
    }

    #[test]
    fn workers_default_to_the_phase_and_are_at_least_one() {
        let zero = PhaseRequest {
            workers: Some(0),
            ..PhaseRequest::new("C1")
        };
        assert_eq!(zero.workers_or(10), 1);
        assert_eq!(PhaseRequest::new("C1").workers_or(10), 10);
    }
}
