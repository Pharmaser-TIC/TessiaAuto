//! One module directory per phase, so each phase writer only touches its own.
//!
//! | module | what it does | `run_phase` (used by `proceso run`) |
//! |---|---|---|
//! | [`convenio_descargar`] | downloads the convenio CSV | no |
//! | [`convenio_preparar`] | prepares the invoices of a cargue | yes |
//! | [`thefactory`] | XML, PDF and CPScan support | yes |
//! | [`derechos`] | rights validation | yes |
//! | [`fev`] | XML total against the convenio | `run_verificar_phase` |
//! | [`conciliar`] | rebuilds the lines of the invoices that differ | yes |
//! | [`rips`] | RIPS JSON | yes |
//! | [`cuv`] | CUV from SISPRO | yes |
//! | [`soporte`] | CRC support zip | yes |
//! | [`radicacion`] | filing in Mutual Ser | yes |
//! | [`proceso`] | the whole per-invoice pipeline | no (it is the caller) |
//!
//! Every phase module has `run_phase(ctx, &PhaseRequest) -> PhaseReport` for
//! the orchestrator. See [`common`] for the contract.

pub mod common;

pub mod conciliar;
pub mod convenio_descargar;
pub mod convenio_preparar;
pub mod cuv;
pub mod derechos;
pub mod fev;
pub mod proceso;
pub mod radicacion;
pub mod rips;
pub mod soporte;
pub mod thefactory;
