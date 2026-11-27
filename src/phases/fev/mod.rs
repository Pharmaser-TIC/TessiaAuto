//! `fev verificar`: XML-vs-convenio value check (catalog FEV-01..07).
//!
//! * [`verificar`]: compares each invoice's XML total with its convenio total and
//!   records phase `fev_valor`; `proceso run` calls [`run_verificar_phase`].

pub mod verificar;
mod xmlsrc;

use anyhow::Result;

use crate::context::AppContext;
use crate::phases::common::{PhaseReport, PhaseRequest};

pub use verificar::{ConvenioInvoice, Verdict, VerificarRun, compare, convenio_total};

/// `fev verificar` as a phase (`Phase::FevValor`), called by `proceso run`.
/// Invoices that differ from the convenio are `REVISION_MANUAL` (not an error:
/// `conciliar` repairs them next).
pub fn run_verificar_phase(ctx: &AppContext, req: &PhaseRequest) -> Result<PhaseReport> {
    let invoices = verificar::load_convenio_invoices(ctx, req)?;
    Ok(verificar::verify(ctx, req, &invoices)?.report)
}
