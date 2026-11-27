//! `fev verificar`: XML-vs-convenio value check (catalog FEV-01..07).
//!
//! * [`verificar`]: compares each invoice's XML total with its convenio total and
//!   records phase `fev_valor`; `proceso run` calls [`run_verificar_phase`].

pub mod cli;
pub mod verificar;
mod xmlsrc;

use anyhow::{Result, bail};

use crate::context::AppContext;
use crate::phases::common::{PhaseReport, PhaseRequest};

pub use cli::{Args, Command};
pub use verificar::{ConvenioInvoice, Verdict, VerificarRun, compare, convenio_total};

/// Entry point of the command line.
pub fn run(ctx: &AppContext, args: Args) -> Result<()> {
    match args.command {
        Command::Verificar(args) => run_verificar(ctx, &args),
    }
}

fn run_verificar(ctx: &AppContext, args: &cli::VerificarArgs) -> Result<()> {
    let req = args.common.to_request()?;
    let invoices = verificar::load_convenio_invoices(ctx, &req)?;
    let run = verificar::verify(ctx, &req, &invoices)?;
    for line in &run.lines {
        println!("{line}");
    }
    if run.mismatches() > 0 {
        bail!(
            "{} factura(s) sin valor coincidente entre XML y convenio",
            run.mismatches()
        );
    }
    Ok(())
}

/// `fev verificar` as a phase (`Phase::FevValor`), called by `proceso run`.
/// Invoices that differ from the convenio are `REVISION_MANUAL` (not an error:
/// `conciliar` repairs them next).
pub fn run_verificar_phase(ctx: &AppContext, req: &PhaseRequest) -> Result<PhaseReport> {
    let invoices = verificar::load_convenio_invoices(ctx, req)?;
    Ok(verificar::verify(ctx, req, &invoices)?.report)
}
