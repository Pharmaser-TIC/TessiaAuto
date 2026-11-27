# resiliencia-terceros
Objective: a third-party outage (network/5xx) delays an invoice instead of cancelling the whole `proceso run`.
Branch: feat/resiliencia-terceros | Delivery: ask-on-risk | Runner: `cargo test` (no Postgres/Redis locally: DB-backed tests fail on base)

## Specs
- S1: The transport error shows its root cause, not only "error sending request" (derechos token POST and Cassandra upload both showed just that).
- S2: A storage transport/5xx failure ("almacenamiento de objetos: POST .../files/upload: se rindió tras 3 intentos: request or response body error") does not abort the run: the invoice stays FAILED and is retried next run; the error is still reported.
- S3: Uploads to storage retry with more margin than the current 3 attempts / 250 ms doubling (< 1 s in total).
- S4: A network failure getting the derechos token is NOT reported as "credenciales o token rechazados aun despues de renovarlo": it has its own message, stops validation leaving the invoices untouched, is resumable and not fatal.
- S5: A real credential rejection (token HTTP 400/401/403, validate 401/403) keeps the existing fatal message and behavior. Other failures keep their behavior (non-transient generic Err still aborts with `continue_on_error=false`).
- Out of scope (pending in docs/tareas-pendientes/resiliencia-ante-terceros.md): scheduled retries with `next_attempt_at`, per-provider circuit breaker, cuv idempotence, per-provider visibility, sispro/radicacion token classification.

## Tasks
- [x] T1 (S1) transport error message with root-cause chain — inline — commit: 8c3bc35 (RED observed, GREEN)
- [x] T2 (S2,S3) typed storage outage + resumable verdict + upload retry margin — inline — commit: af2d655 (RED observed: 4 tests failed, then GREEN)
- [x] T3 (S4,S5) derechos token/transport outage vs credential rejection — inline — commit: 3c88a25 (RED was a compile failure on the new variants, then GREEN; S5 pinned by existing 401/403 tests that still pass)

## Log
- L1 (2026-11-27, user): "vamos con la actividad resikiencia, en el rust, vamos!" (task agreed earlier: "todo esto es genial, tenlo como una tarea por hacer")
- L2 evidence: 2026-11-24 runs. derechos: `derechosapi: token: POST https://rino.mutualser.com/realms/right-validation/protocol/openid-connect/token: se rindió tras 4 intentos: error sending request` labelled as credentials (client.rs:214). thefactory: Cassandra upload `request or response body error` aborted the run (pipeline/mod.rs:463, continue_on_error=false, error not resumable).
- L3 verification (2026-11-27): `cargo test --no-fail-fast` on main and on the branch fail the SAME 439 tests (diff of names is empty): all need Postgres/Redis that this machine lacks. New tests are DB-free and pass. `cargo clippy --all-targets`: no warnings.
- L4 behavior notes: (a) in `proceso run` a transient outage marks the invoice FAILED+resumable (run continues, error still reported at the end); the standalone `thefactory descargar` command still stops its own pool on the first worker error. (b) derechos: a transport failure on a validate call now stops the validation (invoices untouched) instead of sending the invoice to REVISION_MANUAL. (c) sispro/radicacion token fetches keep classifying every failure as before.
- L5 not run: no end-to-end run against the real providers (needs credentials and Postgres).
