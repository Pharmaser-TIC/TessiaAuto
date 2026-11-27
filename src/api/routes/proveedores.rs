//! `GET /api/proveedores`: how the third parties are doing, as the circuit
//! breakers and the retry waits see them.
//!
//! * `proveedores`: one row per provider the phases depend on, always listed
//!   (a provider nobody has called yet is `cerrado` with no calls): its breaker
//!   state, the recent outcomes it remembers, the last failure and, while it
//!   is open, how long until it is probed again.
//! * `en_espera`: the invoices put to wait for a retry (`proceso_estado`,
//!   `FAILED` with a future `next_attempt_at`), by phase in pipeline order,
//!   with the earliest retry of each.
//!
//! The breakers live in this process; the waits come from the database, so
//! they include other machines'. The card polls this every few seconds: a
//! database that cannot answer is "nothing waiting", never an error.

use serde::Serialize;

use crate::api::{ApiResult, ApiState, request::Request, request::Response};
use crate::http::BreakerSnapshot;

/// The providers, in display order: `(name, label)`.
const PROVIDERS: [(&str, &str); 5] = [
    ("cassandra", "Almacenamiento (Cassandra)"),
    ("thefactory", "TheFactory"),
    ("derechos", "Validador de derechos (Mutual Ser)"),
    ("sispro", "SISPRO (Ministerio)"),
    ("medicar", "Medicar"),
];

#[derive(Debug, Serialize, PartialEq)]
pub struct Proveedor {
    pub nombre: &'static str,
    pub etiqueta: &'static str,
    /// `cerrado`, `abierto` or `semiabierto`.
    pub estado: &'static str,
    pub llamadas: usize,
    pub fallas: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ultimo_error: Option<String>,
    /// While open: seconds until the next probe.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reintento_en_s: Option<u64>,
    /// Times it opened since it last closed.
    pub aperturas: u32,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct EnEspera {
    /// Step name (`thefactory`, `derechos`, `cuv`, ...).
    pub fase: &'static str,
    pub facturas: usize,
    /// RFC 3339, UTC.
    pub proximo_reintento: String,
}

#[derive(Debug, Serialize)]
pub struct Answer {
    pub proveedores: Vec<Proveedor>,
    pub en_espera: Vec<EnEspera>,
}

pub fn get(state: &ApiState, _req: &Request) -> ApiResult {
    let snapshots = state.ctx.breakers.snapshots();
    let proveedores = PROVIDERS
        .iter()
        .map(|(name, label)| {
            let seen = snapshots.iter().find(|s| s.name == *name);
            proveedor(name, label, seen)
        })
        .collect();
    let en_espera = match state.ctx.proceso().waiting() {
        Ok(rows) => rows
            .into_iter()
            .map(|w| EnEspera {
                fase: step_of(w.phase.as_str()),
                facturas: w.invoices,
                proximo_reintento: w
                    .next_attempt_at
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            })
            .collect(),
        Err(err) => {
            tracing::warn!(error = %err, "proveedores: no se pudieron leer las esperas");
            Vec::new()
        }
    };
    Ok(Response::json(&Answer {
        proveedores,
        en_espera,
    }))
}

fn proveedor(name: &'static str, label: &'static str, seen: Option<&BreakerSnapshot>) -> Proveedor {
    match seen {
        None => Proveedor {
            nombre: name,
            etiqueta: label,
            estado: "cerrado",
            llamadas: 0,
            fallas: 0,
            ultimo_error: None,
            reintento_en_s: None,
            aperturas: 0,
        },
        Some(s) => Proveedor {
            nombre: name,
            etiqueta: label,
            estado: s.state.as_str(),
            llamadas: s.calls,
            fallas: s.failures,
            ultimo_error: s.last_error.clone(),
            reintento_en_s: s.retry_in.map(|d| d.as_secs().max(1)),
            aperturas: s.opens,
        },
    }
}

/// The step name the dashboard knows a stored phase by.
fn step_of(phase: &'static str) -> &'static str {
    match phase {
        "fev_valor" => "fev",
        "conciliacion" => "conciliar",
        other => other,
    }
}
