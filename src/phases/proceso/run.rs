//! `proceso run`: seed, stream, alerts (Go `runProcesoRun` + `buildPipeline`).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use chrono::Utc;

use anyhow::{Result, anyhow};

use super::classify::{Outage, STORAGE_PROVIDER, Status, Verdict, classify, provider_of};
use super::config::RunConfig;
use super::events::Event;
use super::events::Events;
use super::pipeline::{
    Finished, Fired, GateConfig, Hooks, PhaseSpec, PipelineConfig, run_pipeline,
};
use super::runner::PhaseRunner;
use super::seed::{Seeded, seed};
use super::steps::label;
use crate::context::AppContext;
use crate::domain::{Phase, PhaseState};
use crate::http::{CircuitBreaker, Ticket};
use crate::phases::common::{PhaseReport, PhaseRequest, outage_reason};
use crate::phases::radicacion::progress::{self, StepProgress};

/// What a run did.
#[derive(Debug, Clone)]
pub struct RunSummary {
    pub cargue_id: String,
    /// Invoices that entered the pipeline at the start (before replacements).
    pub seeded: usize,
    pub prepared_now: bool,
    pub finished: Finished,
}

/// Substring of the error `radicacion::run_phase` returns while another
/// machine is filing the same cargue.
const OTHER_FILER: &str = "ya lo está radicando otra máquina";

/// Runs the whole command with the given phases, writing the stream to
/// `events`. The error is the first systemic one (plus the run-wide alerts).
pub fn execute(
    ctx: &AppContext,
    cfg: &RunConfig,
    runner: &PhaseRunner,
    events: &Events,
) -> Result<RunSummary> {
    let seeded = seed(ctx, cfg, events)?;
    let finished = stream(ctx, cfg, runner, events, &seeded);
    let seeded_count = seeded.invoices.len();
    let mut errors: Vec<String> = finished.error.iter().cloned().collect();
    if let Err(alert) = runner.finish(ctx) {
        errors.push(format!("{alert:#}"));
    }
    if crate::shutdown::requested() {
        errors.push("interrumpido".to_owned());
    }
    let summary = RunSummary {
        cargue_id: seeded.cargue_id,
        seeded: seeded_count,
        prepared_now: seeded.prepared_now,
        finished,
    };
    if errors.is_empty() {
        Ok(summary)
    } else {
        Err(anyhow!(errors.join("; ")))
    }
}

fn stream(
    ctx: &AppContext,
    cfg: &RunConfig,
    runner: &PhaseRunner,
    events: &Events,
    seeded: &Seeded,
) -> Finished {
    let pipeline = pipeline_config(cfg, seeded);
    // The breakers decide whether a phase may start: without a retry schedule
    // there is nothing to wait with, so every invoice runs as it always did.
    let breakers = !cfg.tuning.retry_schedule.is_empty();
    let run = |phase: &str, invoice: &str| {
        run_one(ctx, runner, &seeded.cargue_id, phase, invoice, breakers)
    };
    let fire = |batch: &[String]| fire_batch(ctx, cfg, runner, events, &seeded.cargue_id, batch);
    let supply = |n: usize| match &seeded.supplier {
        Some(supplier) => supplier.supply(ctx, events, n),
        None => Ok(Vec::new()),
    };
    // An invoice put to wait for a retry is recorded (`next_attempt_at`), so
    // the wait can be seen; failing to record it never matters to the run.
    let waiting = |phase: &str, invoice: &str, reason: &str, delay: Option<Duration>| {
        let Some(stored) = phase_of(phase) else {
            return;
        };
        let result = match delay {
            Some(delay) => {
                let at = Utc::now() + chrono::Duration::from_std(delay).unwrap_or_default();
                ctx.proceso()
                    .schedule_retry(
                        stored,
                        invoice,
                        Some(&seeded.cargue_id),
                        &outage_reason(reason),
                        at,
                    )
                    .map(|_| ())
            }
            // The run was cancelled: nobody will retry it now.
            None => ctx.proceso().clear_retry(stored, invoice).map(|_| ()),
        };
        if let Err(err) = result {
            tracing::warn!(invoice, phase, error = %err, "retry wait not recorded");
        }
    };
    let hooks = Hooks {
        run: &run,
        fire: pipeline.gate.is_some().then_some(&fire as _),
        supply: seeded.supplier.is_some().then_some(&supply as _),
        waiting: Some(&waiting),
    };
    run_pipeline(&pipeline, &hooks, events)
}

/// The active phases wired in order, the gate and the replacement budget.
fn pipeline_config(cfg: &RunConfig, seeded: &Seeded) -> PipelineConfig {
    let mainline = cfg.active.mainline();
    let spec = |name: &'static str, next: Option<&'static str>| PhaseSpec {
        name,
        label: label(name),
        workers: cfg.workers_of(name),
        next,
    };
    let mut phases: Vec<PhaseSpec> = mainline
        .iter()
        .enumerate()
        .map(|(at, name)| spec(name, mainline.get(at + 1).copied()))
        .collect();
    if cfg.active.has("conciliar") {
        // conciliar hands the invoice back to fev's successor.
        let after_fev = mainline
            .iter()
            .position(|s| *s == "fev")
            .and_then(|at| mainline.get(at + 1).copied());
        phases.push(spec("conciliar", after_fev));
    }
    let gate = cfg.active.has("radicacion").then(|| GateConfig {
        threshold: cfg.batch_radicacion,
        interval: cfg.radicacion_interval,
        label: label("radicacion"),
    });
    let replacing = seeded.supplier.as_ref();
    PipelineConfig {
        entry: mainline.first().copied(),
        phases,
        invoices: seeded.invoices.clone(),
        continue_on_error: cfg.continue_on_error,
        gate,
        target: if replacing.is_some() { cfg.limit } else { 0 },
        replace_cap: replacing.map_or(0, |s| s.pool_size()),
        breaker: cfg.tuning.breaker,
        retry_schedule: cfg.tuning.retry_schedule.clone(),
    }
}

fn phase_of(step: &str) -> Option<Phase> {
    match step {
        "thefactory" => Some(Phase::TheFactory),
        "fev" => Some(Phase::FevValor),
        "conciliar" => Some(Phase::Conciliacion),
        "derechos" => Some(Phase::Derechos),
        "rips" => Some(Phase::Rips),
        "cuv" => Some(Phase::Cuv),
        "soporte" => Some(Phase::Soporte),
        _ => None,
    }
}

/// One phase for one invoice: `--invoices <invoice>` semantics (the gate of
/// the phase is bypassed, the claim still applies).
fn run_one(
    ctx: &AppContext,
    runner: &PhaseRunner,
    cargue_id: &str,
    phase: &str,
    invoice: &str,
    breakers: bool,
) -> Verdict {
    let Some(run) = runner.get(phase) else {
        return Verdict::failed_with(&anyhow!("la fase {phase} no tiene ejecutor"));
    };
    let request = PhaseRequest {
        cargue_id: cargue_id.to_owned(),
        invoices: Some(vec![invoice.to_owned()]),
        workers: None,
    };
    // Providers this phase depends on: storage always, plus its own. When one
    // keeps failing its breaker is open and the phase does not even start: the
    // invoice waits instead of every worker running into the same outage.
    let admission = if breakers {
        match Admission::admit(ctx, phase) {
            Ok(admission) => Some(admission),
            Err((provider, wait)) => return open_verdict(provider, wait),
        }
    } else {
        None
    };
    let stored = || {
        let stored = ctx
            .proceso()
            .load(phase_of(phase)?, Some(&[invoice.to_owned()]))
            .ok()?;
        stored.get(invoice).map(|s| s.state)
    };
    let verdict = classify(phase, invoice, run(ctx, &request), &|| stored());
    if let Some(admission) = admission {
        admission.settle(&verdict);
    }
    verdict
}

/// The breakers that let a phase start, with the tickets they gave. If it is
/// dropped without [`settle`](Admission::settle) (a panic in the phase, an
/// early return) every ticket is handed back, so a half-open probe is never
/// left in flight for good.
struct Admission {
    admitted: Vec<(Arc<CircuitBreaker>, Ticket)>,
    settled: bool,
}

impl Admission {
    /// Asks the breaker of every provider of `phase`; the first one that
    /// refuses gives back the tickets already taken and says how long to wait.
    fn admit(ctx: &AppContext, phase: &str) -> Result<Self, (&'static str, Duration)> {
        let mut admission = Admission {
            admitted: Vec::new(),
            settled: false,
        };
        for provider in providers_of(phase) {
            let breaker = ctx.breakers.get(provider);
            match breaker.allow() {
                Ok(ticket) => admission.admitted.push((breaker, ticket)),
                // Dropping `admission` skips the tickets taken so far.
                Err(wait) => return Err((provider, wait)),
            }
        }
        Ok(admission)
    }

    /// Tells each breaker how the invoice ended: a provider that failed is a
    /// failure; an answer from the others (even "no" for this invoice) is a
    /// success; an invoice left to another machine says nothing.
    ///
    /// Known limit: an invoice that finishes without calling the provider (it
    /// was already done) still counts as a success for the half-open probe.
    fn settle(mut self, verdict: &Verdict) {
        self.settled = true;
        for (breaker, ticket) in &self.admitted {
            match &verdict.outage {
                Some(outage) if outage.provider == breaker.name() => {
                    breaker.record(*ticket, false, verdict.error.as_deref());
                }
                Some(_) => breaker.skip(*ticket),
                None if verdict.status == Status::Skipped => breaker.skip(*ticket),
                None => breaker.record(*ticket, true, None),
            }
        }
    }
}

impl Drop for Admission {
    fn drop(&mut self) {
        if !self.settled {
            for (breaker, ticket) in &self.admitted {
                breaker.skip(*ticket);
            }
        }
    }
}

/// The providers a phase depends on, storage first.
fn providers_of(phase: &str) -> Vec<&'static str> {
    std::iter::once(STORAGE_PROVIDER)
        .chain(provider_of(phase))
        .collect()
}

/// What an open breaker looks like to the pipeline: an outage with the time to
/// wait, without having asked the provider anything.
fn open_verdict(provider: &'static str, wait: Duration) -> Verdict {
    let message = format!(
        "{provider} no responde: se pausó la fase hasta poder volver a probar ({} s)",
        wait.as_secs().max(1)
    );
    Verdict {
        error: Some(message.clone()),
        resumable: true,
        outage: Some(Outage {
            provider,
            wait: Some(wait),
        }),
        ..Verdict::new(Status::Failed, &message)
    }
}

/// Files one batch through `radicacion`. While another machine files the same
/// cargue the batch waits (its lease serializes uploads) instead of failing.
fn fire_batch(
    ctx: &AppContext,
    cfg: &RunConfig,
    runner: &PhaseRunner,
    events: &Events,
    cargue_id: &str,
    batch: &[String],
) -> Fired {
    let (tx, rx) = crossbeam_channel::unbounded();
    let registration = progress::register(
        cargue_id,
        Arc::new(move |invoice, step, what| {
            let _ = tx.send((invoice.to_owned(), step, what));
        }),
    );
    let finished = AtomicBool::new(false);
    std::thread::scope(|scope| {
        // Turns the engine's step transitions into dashboard events.
        scope.spawn(|| {
            let emit =
                |(invoice, step, what): (String, crate::domain::radicacion::Step, StepProgress)| {
                    events.emit(
                        Event::new("radicacion_step")
                            .invoice(&invoice)
                            .phase(&format!("radicacion_{}", step.as_i16()), step.label())
                            .status(what.as_str()),
                    );
                };
            loop {
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(message) => emit(message),
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                        if finished.load(Ordering::SeqCst) {
                            rx.try_iter().for_each(emit);
                            return;
                        }
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
                }
            }
        });
        let fired = file_batch(ctx, cfg, runner, cargue_id, batch);
        drop(registration);
        finished.store(true, Ordering::SeqCst);
        fired
    })
}

fn file_batch(
    ctx: &AppContext,
    cfg: &RunConfig,
    runner: &PhaseRunner,
    cargue_id: &str,
    batch: &[String],
) -> Fired {
    let Some(run) = runner.get("radicacion") else {
        return Fired::Failed("la fase radicacion no tiene ejecutor".to_owned());
    };
    let request = PhaseRequest {
        cargue_id: cargue_id.to_owned(),
        invoices: Some(batch.to_vec()),
        workers: None,
    };
    let give_up = Instant::now() + cfg.tuning.radicacion_wait;
    loop {
        match run(ctx, &request) {
            Ok(report) => return Fired::Done(batch_summary(&report)),
            Err(err) if format!("{err:#}").contains(OTHER_FILER) => {
                let wait = cfg.tuning.radicacion_retry;
                if Instant::now() + wait > give_up || crate::shutdown::requested() {
                    return Fired::OtherMachine(format!("{err:#}"));
                }
                tracing::warn!(
                    cargue = cargue_id,
                    invoices = batch.len(),
                    retry_in_s = wait.as_secs(),
                    gives_up_in_s = give_up.saturating_duration_since(Instant::now()).as_secs(),
                    reason = %format!("{err:#}"),
                    "radicacion batch waiting for the cargue lease"
                );
                std::thread::sleep(wait);
            }
            Err(err) => return Fired::Failed(format!("{err:#}")),
        }
    }
}

fn batch_summary(report: &PhaseReport) -> String {
    let count = |state: PhaseState| {
        report
            .outcomes
            .iter()
            .filter(|o| o.outcome.state == state)
            .count()
    };
    format!(
        "filed={} manual={} blocked={} pending={}",
        count(PhaseState::Done),
        count(PhaseState::RevisionManual),
        count(PhaseState::Failed),
        report.skipped.len()
    )
}
