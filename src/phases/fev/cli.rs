//! Command line of `fev verificar`.

use clap::{Args as ClapArgs, Subcommand};

use crate::phases::common::CommonArgs;

/// XML de factura electrónica.
#[derive(Debug, Clone, ClapArgs)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    #[command(
        name = "verificar",
        about = "Verifica el total del XML contra el total del convenio",
        long_about = "Compara, por factura, el total del XML (PayableAmount o TaxInclusiveAmount; del primer \
.xml de XML|ZIP/<F>_XML_PDF.zip, si no <F>.xml) con la suma del 'vlr. total' de las líneas del convenio \
(diferencia < 0,01). No escribe archivos: registra la fase fev_valor (DONE; REVISION_MANUAL si difiere; \
FAILED si el XML no tiene valor) y guarda el total del XML en factura_valor_xml. Termina con código 1 \
cuando alguna factura no coincide. Cada ejecución revisa todas las facturas."
    )]
    Verificar(VerificarArgs),
}

/// Arguments of the `verificar` subcommand.
#[derive(Debug, Clone, ClapArgs)]
pub struct VerificarArgs {
    #[command(flatten)]
    pub common: CommonArgs,
}
