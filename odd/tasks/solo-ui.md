# solo-ui
Objective: keep only the code the desktop UI (and its in-process API and jobs) reaches; remove the command-line layer and what only it used.
Branch: refactor/solo-ui (stacked on chore/quitar-comandos-sueltos) | Delivery: ask-on-risk | Runner: `cargo test --no-fail-fast`; baseline = the set of tests that already fail on main because there is no Postgres/Redis here (439 on main, 422 after the previous branch)

## Specs
- S1: "quiero que solo quede lo que se hace mediante la ui, nadie usara la cli. si la api llama a la cli, conservalo, si son independientes una de la otra, deja en el codigo solamente lo que sirve en la ui unicamente."
- S2 (finding, verified): the API and the jobs never call the CLI (no `clap` in `src/api` or `src/jobs`; they call phase functions directly). The CLI layer is independent, so it goes.
- S3 (finding, verified): `AppContext::from_settings` already connects and migrates, so `db migrate` is not needed by the window.
- S4 (assumption, to confirm): keep `--config <file>` and `TESSIA_CONFIG` so a shortcut can still point the window at its settings file; `main.rs` keeps the "no arguments open the window" behavior.
- S5: Nothing the UI reaches may change behavior. The phase library entry points the API/jobs use (`run_phase*`, `plan_preparation`, `commit_preparation`, `PhaseRunner::real`, ...) stay.
- S6: Tests that covered kept behavior through a CLI entry point are rewritten against the library function; tests of CLI-only behavior (parsing, help, exit codes) are removed.
- Not touched: the persisted `fev_ajuste` phase name (rows already stored must still read).

## Tasks
- [ ] T1 (S2,S3,S4,S5,S6) binary + `crate::cli`, `config::cli`, `db::cli`, per-phase `cli.rs` and `run` entry points, clap — inline — commit: pending
- [ ] T2 (S1) dead-code sweep from the UI roots (cascade: code only the CLI used) — inline — commit: pending
- [ ] T3 (S6) docs: README, architecture.md (tracked); manual and process map (untracked, listed in the report) — inline — commit: pending

## Log
- L1 (2026-11-27, user): "quiero que solo quede lo que se hace mediante la ui, nadie usara la cli. si la api llama a la cli, conservalo, si son independientes una de la otra, deja en el codigo solamente lo que sirve en la ui unicamente."
- L2 (2026-11-27): the previous request ("todos los comandos sueltos que no usa el flujo, borralos") is commit e01617c on chore/quitar-comandos-sueltos; this feature builds on it.
