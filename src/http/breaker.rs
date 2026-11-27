//! A circuit breaker per provider: when a third party keeps failing, stop
//! asking it for a while instead of making every invoice wait out its own
//! retries, then let one probe through to see whether it is back.
//!
//! It judges *outcomes* (a call ended well, or ended because the provider or
//! the network is down); what counts as an outage is decided by the caller.
//! States:
//!
//! * **Closed**: calls go through; the last `window` outcomes are remembered.
//!   When at least `min_calls` are known and `failure_ratio` of them failed, it
//!   opens.
//! * **Open**: [`CircuitBreaker::allow`] refuses and says how long to wait.
//!   After the open period one caller is let through (half-open).
//! * **Half-open**: that single probe decides. Success closes the breaker;
//!   failure reopens it for twice as long (capped at `max_open`). Other callers
//!   are refused for a moment while the probe is in flight.
//!
//! Every admitted call holds a [`Ticket`]: only the ticket of the half-open
//! probe can close, reopen or release the breaker, so a late result of a call
//! admitted before it opened cannot decide anything.
//!
//! It is per process and keeps nothing on disk.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Tuning of one breaker.
#[derive(Debug, Clone)]
pub struct BreakerConfig {
    /// How many recent outcomes are remembered.
    pub window: usize,
    /// Outcomes needed before the failure share is judged.
    pub min_calls: usize,
    /// Share of failures in the window that opens the breaker.
    pub failure_ratio: f64,
    /// First open period; doubles on every consecutive reopening.
    pub open_for: Duration,
    /// Cap of the open period.
    pub max_open: Duration,
}

impl Default for BreakerConfig {
    fn default() -> Self {
        Self {
            window: 20,
            min_calls: 8,
            failure_ratio: 0.5,
            open_for: Duration::from_secs(30),
            max_open: Duration::from_secs(5 * 60),
        }
    }
}

/// Where a breaker is, for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerState {
    Closed,
    Open,
    HalfOpen,
}

impl BreakerState {
    pub fn as_str(self) -> &'static str {
        match self {
            BreakerState::Closed => "cerrado",
            BreakerState::Open => "abierto",
            BreakerState::HalfOpen => "semiabierto",
        }
    }
}

/// Proof that a call was admitted by [`CircuitBreaker::allow`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ticket {
    /// This call is the half-open probe: its result decides the breaker.
    probe: bool,
}

/// A point-in-time view of one breaker.
#[derive(Debug, Clone, PartialEq)]
pub struct BreakerSnapshot {
    pub name: String,
    pub state: BreakerState,
    /// Outcomes currently remembered and how many of them failed.
    pub calls: usize,
    pub failures: usize,
    /// The last failure that was recorded, if any.
    pub last_error: Option<String>,
    /// While open: how long until a probe is allowed.
    pub retry_in: Option<Duration>,
    /// Times it has opened since it last closed.
    pub opens: u32,
}

enum State {
    Closed,
    Open { until: Instant },
    HalfOpen { probing: bool },
}

struct Inner {
    state: State,
    /// `true` = the call ended well.
    recent: VecDeque<bool>,
    /// Consecutive openings without a success in between (sets the open period).
    opens: u32,
    last_error: Option<String>,
    /// Outcomes and failures that opened it, kept while it is not closed so
    /// the display can say what happened (the window is cleared on opening).
    tripped: (usize, usize),
}

type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

/// The breaker of one provider. Share it behind an [`Arc`].
pub struct CircuitBreaker {
    name: String,
    cfg: BreakerConfig,
    clock: Clock,
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for CircuitBreaker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CircuitBreaker")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl CircuitBreaker {
    pub fn new(name: &str, cfg: BreakerConfig) -> Self {
        Self::with_clock(name, cfg, Arc::new(Instant::now))
    }

    /// Like [`CircuitBreaker::new`] with a caller-provided clock (tests).
    pub fn with_clock(name: &str, cfg: BreakerConfig, clock: Clock) -> Self {
        Self {
            name: name.to_owned(),
            cfg,
            clock,
            inner: Mutex::new(Inner {
                state: State::Closed,
                recent: VecDeque::new(),
                opens: 0,
                last_error: None,
                tripped: (0, 0),
            }),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// May a call go through? `Err(wait)` while the breaker is open (or a probe
    /// is in flight): do not call, retry after `wait`. An admitted call must
    /// end with [`record`](Self::record) or [`skip`](Self::skip).
    pub fn allow(&self) -> Result<Ticket, Duration> {
        let now = (self.clock)();
        let mut inner = self.lock();
        match inner.state {
            State::Closed => Ok(Ticket { probe: false }),
            State::Open { until } => {
                if now >= until {
                    inner.state = State::HalfOpen { probing: true };
                    Ok(Ticket { probe: true })
                } else {
                    Err(until - now)
                }
            }
            State::HalfOpen { probing: false } => {
                inner.state = State::HalfOpen { probing: true };
                Ok(Ticket { probe: true })
            }
            // One probe at a time: the others wait a moment for its verdict.
            State::HalfOpen { probing: true } => Err(Duration::from_secs(1)),
        }
    }

    /// An admitted call says nothing about the provider (the invoice was left
    /// to another machine, or the failure was somewhere else). If it was the
    /// half-open probe, the next caller probes instead.
    pub fn skip(&self, ticket: Ticket) {
        if !ticket.probe {
            return;
        }
        let mut inner = self.lock();
        if let State::HalfOpen { probing } = &mut inner.state {
            *probing = false;
        }
    }

    /// Records how an admitted call ended. `error` describes a failure (kept
    /// for display). Only the probe decides while the breaker is not closed.
    pub fn record(&self, ticket: Ticket, ok: bool, error: Option<&str>) {
        let now = (self.clock)();
        let mut inner = self.lock();
        if !ok {
            inner.last_error = error.map(str::to_owned);
        }
        match inner.state {
            State::HalfOpen { .. } if ticket.probe => {
                if ok {
                    inner.state = State::Closed;
                    inner.recent.clear();
                    inner.opens = 0;
                    inner.tripped = (0, 0);
                } else {
                    self.open(&mut inner, now);
                }
            }
            // A late result of a call admitted before it opened, or a call
            // that is not the probe: it changes nothing.
            State::HalfOpen { .. } | State::Open { .. } => {}
            State::Closed => {
                inner.recent.push_back(ok);
                while inner.recent.len() > self.cfg.window.max(1) {
                    inner.recent.pop_front();
                }
                if !ok && self.tripped(&inner) {
                    self.open(&mut inner, now);
                }
            }
        }
    }

    fn tripped(&self, inner: &Inner) -> bool {
        let calls = inner.recent.len();
        if calls < self.cfg.min_calls.max(1) {
            return false;
        }
        let failures = inner.recent.iter().filter(|ok| !**ok).count();
        failures as f64 / calls as f64 >= self.cfg.failure_ratio
    }

    fn open(&self, inner: &mut Inner, now: Instant) {
        let doubled = self
            .cfg
            .open_for
            .saturating_mul(1u32 << inner.opens.min(16));
        let period = doubled.min(self.cfg.max_open);
        inner.opens = inner.opens.saturating_add(1);
        let calls = inner.recent.len();
        if calls > 0 {
            inner.tripped = (calls, inner.recent.iter().filter(|ok| !**ok).count());
        }
        inner.recent.clear();
        inner.state = State::Open {
            until: now + period,
        };
    }

    pub fn snapshot(&self) -> BreakerSnapshot {
        let now = (self.clock)();
        let inner = self.lock();
        let (state, retry_in) = match inner.state {
            State::Closed => (BreakerState::Closed, None),
            State::Open { until } if now < until => (BreakerState::Open, Some(until - now)),
            // The period is over: the next caller probes.
            State::Open { .. } | State::HalfOpen { .. } => (BreakerState::HalfOpen, None),
        };
        let (calls, failures) = match state {
            BreakerState::Closed => (
                inner.recent.len(),
                inner.recent.iter().filter(|ok| !**ok).count(),
            ),
            // Not closed: what opened it.
            _ => inner.tripped,
        };
        BreakerSnapshot {
            name: self.name.clone(),
            state,
            calls,
            failures,
            last_error: inner.last_error.clone(),
            retry_in,
            opens: inner.opens,
        }
    }
}

/// The breakers of a process, by provider name, created on first use.
#[derive(Debug, Default)]
pub struct Breakers {
    by_name: Mutex<BTreeMap<String, Arc<CircuitBreaker>>>,
    cfg: BreakerConfig,
}

impl Breakers {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every breaker uses `cfg` (tests use short periods).
    pub fn with_config(cfg: BreakerConfig) -> Self {
        Self {
            by_name: Mutex::new(BTreeMap::new()),
            cfg,
        }
    }

    /// The breaker of `provider`.
    pub fn get(&self, provider: &str) -> Arc<CircuitBreaker> {
        let mut map = self.by_name.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(provider.to_owned())
            .or_insert_with(|| Arc::new(CircuitBreaker::new(provider, self.cfg.clone())))
            .clone()
    }

    /// A view of every breaker created so far, by name.
    pub fn snapshots(&self) -> Vec<BreakerSnapshot> {
        let map = self.by_name.lock().unwrap_or_else(|e| e.into_inner());
        map.values().map(|b| b.snapshot()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A breaker whose clock only moves when the test says so.
    fn breaker(cfg: BreakerConfig) -> (CircuitBreaker, Arc<Mutex<Duration>>) {
        let base = Instant::now();
        let offset = Arc::new(Mutex::new(Duration::ZERO));
        let shared = offset.clone();
        let clock: Clock = Arc::new(move || base + *shared.lock().expect("clock"));
        (CircuitBreaker::with_clock("sispro", cfg, clock), offset)
    }

    fn advance(offset: &Arc<Mutex<Duration>>, by: Duration) {
        *offset.lock().expect("clock") += by;
    }

    fn small() -> BreakerConfig {
        BreakerConfig {
            window: 10,
            min_calls: 4,
            failure_ratio: 0.5,
            open_for: Duration::from_secs(30),
            max_open: Duration::from_secs(120),
        }
    }

    fn fail(b: &CircuitBreaker, times: usize) {
        for _ in 0..times {
            let ticket = b.allow().expect("admitted");
            b.record(ticket, false, Some("down"));
        }
    }

    fn succeed(b: &CircuitBreaker, times: usize) {
        for _ in 0..times {
            let ticket = b.allow().expect("admitted");
            b.record(ticket, true, None);
        }
    }

    /// Opens a breaker and waits out its period: the next `allow` is the probe.
    fn ready_to_probe() -> (CircuitBreaker, Arc<Mutex<Duration>>) {
        let (b, offset) = breaker(small());
        fail(&b, 4);
        advance(&offset, Duration::from_secs(31));
        (b, offset)
    }

    #[test]
    fn it_stays_closed_until_enough_calls_failed() {
        let (b, _) = breaker(small());
        fail(&b, 3);
        assert!(b.allow().is_ok(), "3 failures are below the minimum sample");
        assert_eq!(b.snapshot().state, BreakerState::Closed);
        let ticket = b.allow().expect("admitted");
        b.record(ticket, false, Some("down"));
        assert!(b.allow().is_err(), "4 of 4 failed: open");
    }

    #[test]
    fn successes_dilute_failures_below_the_ratio() {
        let (b, _) = breaker(small());
        succeed(&b, 6);
        fail(&b, 3);
        assert!(b.allow().is_ok(), "3 of 9 is under half");
        assert_eq!(b.snapshot().state, BreakerState::Closed);
    }

    #[test]
    fn an_open_breaker_refuses_and_says_how_long_to_wait() {
        let (b, offset) = breaker(small());
        fail(&b, 4);
        assert_eq!(b.allow().expect_err("open"), Duration::from_secs(30));
        advance(&offset, Duration::from_secs(10));
        assert_eq!(b.allow().expect_err("still open"), Duration::from_secs(20));
        let snap = b.snapshot();
        assert_eq!(snap.state, BreakerState::Open);
        assert_eq!(
            (snap.calls, snap.failures),
            (4, 4),
            "it says what opened it, not an empty window"
        );
        assert_eq!(snap.retry_in, Some(Duration::from_secs(20)));
        assert_eq!(snap.last_error.as_deref(), Some("down"));
    }

    #[test]
    fn after_the_period_one_probe_goes_through_and_the_rest_wait() {
        let (b, _offset) = ready_to_probe();
        assert!(b.allow().is_ok(), "the probe");
        assert!(b.allow().is_err(), "a second caller waits for its verdict");
        assert_eq!(b.snapshot().state, BreakerState::HalfOpen);
    }

    #[test]
    fn a_successful_probe_closes_it_and_forgets_the_past() {
        let (b, _offset) = ready_to_probe();
        let probe = b.allow().expect("probe");
        b.record(probe, true, None);
        let snap = b.snapshot();
        assert_eq!(
            (snap.state, snap.calls, snap.opens),
            (BreakerState::Closed, 0, 0)
        );
        assert!(b.allow().is_ok());
    }

    #[test]
    fn a_failed_probe_reopens_for_twice_as_long_up_to_the_cap() {
        let (b, offset) = breaker(small());
        fail(&b, 4);
        for want in [60u64, 120, 120] {
            advance(&offset, Duration::from_secs(121));
            let probe = b.allow().expect("probe");
            b.record(probe, false, Some("still down"));
            assert_eq!(b.allow().expect_err("reopened"), Duration::from_secs(want));
        }
    }

    #[test]
    fn a_probe_that_says_nothing_lets_the_next_caller_probe() {
        let (b, _offset) = ready_to_probe();
        let probe = b.allow().expect("probe");
        b.skip(probe);
        assert!(b.allow().is_ok(), "nobody judged it: another caller probes");
    }

    #[test]
    fn a_late_result_of_a_call_admitted_before_it_opened_changes_nothing() {
        let (b, offset) = breaker(small());
        // Five calls are admitted while it is closed; four fail and open it.
        let late = b.allow().expect("admitted while closed");
        fail(&b, 4);
        assert_eq!(b.snapshot().state, BreakerState::Open);
        // Their result arrives while it is open ...
        b.record(late, true, None);
        assert_eq!(b.snapshot().state, BreakerState::Open);
        // ... and while the real probe is in flight: it must not close it
        // nor free the probe slot.
        advance(&offset, Duration::from_secs(31));
        let probe = b.allow().expect("probe");
        b.record(late, true, None);
        b.skip(late);
        assert_eq!(b.snapshot().state, BreakerState::HalfOpen);
        assert!(
            b.allow().is_err(),
            "the probe is still the only one in flight"
        );
        b.record(probe, false, Some("still down"));
        assert_eq!(b.snapshot().state, BreakerState::Open);
    }

    #[test]
    fn only_the_recent_window_counts() {
        let (b, _) = breaker(small());
        fail(&b, 3);
        succeed(&b, 10);
        let snap = b.snapshot();
        assert_eq!(
            (snap.calls, snap.failures),
            (10, 0),
            "old failures slid out"
        );
        assert!(b.allow().is_ok());
    }

    #[test]
    fn the_registry_hands_out_one_breaker_per_provider() {
        let breakers = Breakers::with_config(small());
        let a = breakers.get("thefactory");
        assert!(Arc::ptr_eq(&a, &breakers.get("thefactory")));
        breakers.get("sispro");
        let names: Vec<_> = breakers.snapshots().into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["sispro", "thefactory"]);
    }
}
