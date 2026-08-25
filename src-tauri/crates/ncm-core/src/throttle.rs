//! Admission control for outbound API calls.
//!
//! A page load fans out a dozen endpoint calls at once, and the playback
//! planner resolves the next track on top of whatever the UI is already doing.
//! Netease answers a burst like that by *refusing connections* far more often
//! than by returning 429 — which arrives here as a `reqwest` transport error,
//! becomes `httpRequest: transport error (…)` in the axios shim, and is then
//! mapped by upstream `util/request.js` to `{ status: 502, code: 502 }`. That
//! is the 502 the app reports as a failed request; it never came from Netease.
//!
//! Retrying such a burst immediately makes it worse, so the client shapes its
//! own traffic instead. The governing rule is that **a host that is answering
//! must not be charged anything**: the entire point of this crate is the
//! ~38–48 ms hop it removes, and a client that hedges against a rate limit
//! nobody is enforcing hands that straight back and then some.
//!
//! So both controls start disengaged and are bought with actual refusals:
//!
//! * **Concurrency, additive-increase/multiplicative-decrease.** A fresh host
//!   gets [`MAX_IN_FLIGHT`] slots, which is what the connection pool in
//!   [`crate::http`] retains and the same number browsers have long allowed per
//!   host. Each refusal halves the slots down to [`MIN_IN_FLIGHT`]; every
//!   [`SLOTS_RECOVER_AFTER`] clean responses hand one back. Halving is the part
//!   that matters — a burst is refused in parallel, so the cap collapses within
//!   one burst rather than after a dozen.
//! * **Spacing between request starts.** Zero while the host is healthy. The
//!   first refusal installs a gap, each further one doubles it, every clean
//!   response halves it, and a host left alone long enough is forgiven
//!   outright.
//!
//! Retry backoff is the same mechanism rather than a second one: a retried
//! request re-enters [`HostGate::admit`] and waits out the gap its own failure
//! just installed — and so does every other request queued behind it. Because
//! the ordering is central there is no thundering herd to jitter away.
//!
//! A note on how the numbers were picked, because the first attempt got this
//! wrong: a *fixed* cap of three was measured against `music.163.com` while
//! that address was already being throttled by an hour of test traffic, and it
//! looked good only because everything was slow. A fixed cap of three turns a
//! twelve-call page load into four serial round trips, which is a permanent
//! cost paid for an occasional problem. Anything that slows the healthy path
//! belongs behind a refusal, not in a constant.
//!
//! Scope note: this paces *this process*. It cannot see what a rate limit is
//! actually counting (an account, an IP, a device id), so it is a good citizen
//! rather than a guarantee.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Concurrent requests a healthy host gets.
///
/// Equal to `http::POOL_MAX_IDLE_PER_HOST`, so a burst never opens a connection
/// the pool will not keep afterwards, and equal to the per-host limit browsers
/// have enforced for HTTP/1.1 for the same reason.
const MAX_IN_FLIGHT: usize = 6;

/// Floor under sustained refusal. Never zero, and never one: a single slot
/// makes the playback resolver queue behind whatever the UI is doing, which
/// turns a rate limit into a gap between tracks.
const MIN_IN_FLIGHT: usize = 2;

/// Clean responses that buy one slot back.
const SLOTS_RECOVER_AFTER: u32 = 4;

/// Gap installed by the first refusal. Doubles per consecutive refusal.
const SPACING_MIN: Duration = Duration::from_millis(100);

/// Ceiling on the gap.
///
/// Low on purpose. Spacing is the *secondary* control here — the concurrency
/// cap is what actually answers a refusal — and a gap is charged to every
/// request on the host, so a second of it turns a twelve-call page load into
/// twelve seconds. Past this the caller's own timeout budget is the more useful
/// limit anyway.
const SPACING_MAX: Duration = Duration::from_millis(800);

/// How far ahead of the present the start queue may be pushed.
///
/// [`Pace::penalise`] charges a gap per refusal and a burst is refused in
/// parallel, so several failures at the ceiling would queue more waiting than
/// any caller's budget — long enough that everyone behind them sleeps through
/// it and reports a timeout they never actually incurred. Past this the host is
/// not being paced any more, it is down, and the honest answer is the error.
const MAX_QUEUE_AHEAD: Duration = Duration::from_secs(2);

/// Decay past this and the host counts as healthy again. Without a floor the
/// gap would asymptote towards zero and never actually reach it, leaving a
/// sleep in front of every request forever.
const SPACING_FLOOR: Duration = Duration::from_millis(30);

/// A host nobody has provoked for this long starts clean — full slots, no gap.
///
/// Decay alone is not enough: a session that gets refused once and then goes
/// quiet for ten minutes would come back still carrying the penalty, because
/// only a *success* decays it, and after a quiet stretch there have been none.
const SPACING_FORGET: Duration = Duration::from_secs(30);

/// What a refusal cost the host, for the log line.
pub(crate) struct Backoff {
    pub spacing: Duration,
    pub in_flight: usize,
}

/// Per-host gates, created on first use.
///
/// The key set is bounded in practice by the handful of hosts the bundled
/// protocol layer talks to (`music.163.com`, `interface*.music.163.com`, …);
/// redirects are not followed, so nothing a server says can add entries.
pub(crate) struct Governor {
    hosts: Mutex<HashMap<String, Arc<HostGate>>>,
}

impl Governor {
    pub(crate) fn new() -> Self {
        Self {
            hosts: Mutex::new(HashMap::new()),
        }
    }

    /// The gate for `key`, which is a host plus an explicit port when there is
    /// one. Ports matter: two services on one host are two rate limits.
    pub(crate) fn host(&self, key: &str) -> Arc<HostGate> {
        // A poisoned lock here means a panic somewhere else in the crate, which
        // the "no panics across the FFI boundary" rule says must not become a
        // second panic. The map is a plain cache; recovering it is safe.
        let mut hosts = self.hosts.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(gate) = hosts.get(key) {
            return gate.clone();
        }
        let gate = Arc::new(HostGate::new());
        hosts.insert(key.to_owned(), gate.clone());
        gate
    }

    /// Whether any host is currently being paced.
    ///
    /// Read before speculative work — a prefetch, a stale refresh — so that
    /// traffic nobody is waiting for disappears the moment the client is being
    /// pushed back on. Speculation is only ever worth it against a healthy
    /// service: the same refusal that installs pacing is what would make the
    /// prefetch land *behind* the request the user is actually waiting for.
    ///
    /// Deliberately coarse. It asks "is anything unwell" rather than resolving
    /// the host a given call would use, which is not knowable until the protocol
    /// layer has built the URL — by which point the request is already being
    /// made.
    pub(crate) fn is_strained(&self) -> bool {
        let hosts = self.hosts.lock().unwrap_or_else(|e| e.into_inner());
        hosts.values().any(|gate| gate.is_paced())
    }
}

/// Build the [`Governor::host`] key for a URL.
///
/// Borrowed whenever the port is implicit, which is every request the protocol
/// layer makes — the lookup then costs no allocation at all.
pub(crate) fn host_key(url: &reqwest::Url) -> Cow<'_, str> {
    let host = url.host_str().unwrap_or("");
    match url.port() {
        None => Cow::Borrowed(host),
        Some(port) => Cow::Owned(format!("{host}:{port}")),
    }
}

pub(crate) struct HostGate {
    slots: Arc<Semaphore>,
    pace: Mutex<Pace>,
}

impl HostGate {
    fn new() -> Self {
        Self::with_ceiling(MAX_IN_FLIGHT)
    }

    fn with_ceiling(ceiling: usize) -> Self {
        Self {
            slots: Arc::new(Semaphore::new(ceiling)),
            pace: Mutex::new(Pace::new(ceiling)),
        }
    }

    /// Wait for this host's pacing window, then for a free slot.
    ///
    /// Pacing first on purpose: acquiring the slot first would hold it through
    /// the sleep and shrink the effective concurrency to nothing the moment a
    /// gap is installed.
    ///
    /// `budget` is what the caller has left. `None` means the queue alone is
    /// longer than that, and the caller is told now rather than being parked
    /// into a timeout — which also keeps it from lengthening the queue for
    /// everyone behind it.
    ///
    /// The returned permit must be held for the whole request — dropping it
    /// early hands the slot to the next caller while this one is still on the
    /// wire.
    ///
    /// On a healthy host this reaches `acquire_owned` with no lock contention,
    /// no sleep and a permit already available, which is the whole design goal.
    pub(crate) async fn admit(&self, budget: Duration) -> Option<OwnedSemaphorePermit> {
        let wait = {
            let mut pace = self.locked_pace();
            let now = Instant::now();

            // A long enough untroubled stretch wipes the slate before anything
            // else is decided.
            let restore = pace.forgive(now);
            if restore > 0 {
                self.slots.add_permits(restore);
            }
            // Slots can only be withdrawn while they are free, so a shrink that
            // happened mid-burst leaves a debt. Settle what we can here; a
            // later caller settles the rest.
            if pace.debt > 0 {
                pace.debt -= self.slots.forget_permits(pace.debt);
            }

            pace.reserve(now, budget)?
        };

        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
        // The semaphore is never closed, so this cannot fail; if it somehow
        // did, proceeding unmetered beats failing a request over bookkeeping.
        Some(
            self.slots
                .clone()
                .acquire_owned()
                .await
                .expect("the host semaphore is never closed"),
        )
    }

    /// Record that this host pushed back. `hint` is a server-supplied
    /// `Retry-After`, which wins over the computed gap when it is longer.
    pub(crate) fn penalise(&self, hint: Option<Duration>) -> Backoff {
        let mut pace = self.locked_pace();
        let backoff = pace.penalise(Instant::now(), hint);
        // Take back what we can now; the rest becomes debt for `admit`.
        let withdraw = pace.debt;
        if withdraw > 0 {
            pace.debt -= self.slots.forget_permits(withdraw);
        }
        backoff
    }

    /// Record a clean response.
    pub(crate) fn relax(&self) {
        let mut pace = self.locked_pace();
        let grant = pace.relax();
        if grant > 0 {
            self.slots.add_permits(grant);
        }
    }

    /// Whether this host is currently carrying a penalty — a gap between
    /// requests, or fewer slots than a healthy host gets.
    fn is_paced(&self) -> bool {
        let pace = self.locked_pace();
        !pace.spacing.is_zero() || pace.in_flight < pace.ceiling
    }

    fn locked_pace(&self) -> std::sync::MutexGuard<'_, Pace> {
        self.pace.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The pacing state for one host.
///
/// Split out from [`HostGate`] and given an explicit `now` so the policy is
/// testable without a clock: every method below is pure arithmetic over
/// `Instant`s the caller supplies. Slot changes are returned rather than
/// applied, because the semaphore is not this type's to touch.
struct Pace {
    /// Earliest instant at which the next request on this host may start.
    next_start: Instant,
    /// Enforced gap between request starts. Zero while the host is healthy.
    spacing: Duration,
    /// When `spacing` was last raised.
    penalised_at: Instant,
    /// Slots a healthy host gets back up to.
    ceiling: usize,
    /// Slots currently allowed.
    in_flight: usize,
    /// Slots owed to a shrink that could not be taken because they were all
    /// checked out at the time.
    debt: usize,
    /// Clean responses since the last refusal.
    clean: u32,
}

impl Pace {
    fn new(ceiling: usize) -> Self {
        let now = Instant::now();
        Self {
            next_start: now,
            spacing: Duration::ZERO,
            penalised_at: now,
            ceiling,
            in_flight: ceiling,
            debt: 0,
            clean: 0,
        }
    }

    /// Wipe the penalty if nothing has provoked this host in a long time.
    /// Returns slots to hand back to the semaphore.
    fn forgive(&mut self, now: Instant) -> usize {
        if self.spacing.is_zero() && self.in_flight == self.ceiling {
            return 0;
        }
        if now.saturating_duration_since(self.penalised_at) < SPACING_FORGET {
            return 0;
        }
        self.spacing = Duration::ZERO;
        self.clean = 0;
        self.restore_slots(self.ceiling - self.in_flight)
    }

    /// Claim the next start slot and report how long the caller must wait, or
    /// `None` when that wait is longer than `budget`.
    ///
    /// Declining leaves `next_start` untouched, which is the point: a caller
    /// that cannot afford the queue must not extend it for the ones that can.
    ///
    /// `next_start` is clamped forward to `now` on every call, so an idle host
    /// never banks credit it could spend as a burst later — which is exactly
    /// the behaviour that got us refused in the first place.
    fn reserve(&mut self, now: Instant, budget: Duration) -> Option<Duration> {
        let at = self.next_start.max(now);
        let wait = at.saturating_duration_since(now);
        if wait > budget {
            return None;
        }
        self.next_start = at + self.spacing;
        Some(wait)
    }

    fn penalise(&mut self, now: Instant, hint: Option<Duration>) -> Backoff {
        let doubled = if self.spacing.is_zero() {
            SPACING_MIN
        } else {
            self.spacing.saturating_mul(2)
        };
        // A `Retry-After` is the server stating its own terms; honour it when
        // it asks for more than we would have taken, but never let it push past
        // the ceiling — an hour-long header would otherwise wedge the host for
        // the rest of the session.
        self.spacing = doubled.max(hint.unwrap_or(Duration::ZERO)).min(SPACING_MAX);
        self.penalised_at = now;
        self.clean = 0;

        // Multiplicative decrease. A burst is refused in parallel, so this
        // reaches the floor inside one burst instead of trailing it.
        let target = (self.in_flight / 2).max(MIN_IN_FLIGHT);
        self.debt += self.in_flight - target;
        self.in_flight = target;

        // Charge the gap immediately so the retry that follows — and anything
        // already queued behind it — pays it, instead of racing out the moment
        // a slot frees. Bounded, because each parallel failure charges again.
        self.next_start = (self.next_start.max(now) + self.spacing).min(now + MAX_QUEUE_AHEAD);

        Backoff {
            spacing: self.spacing,
            in_flight: self.in_flight,
        }
    }

    /// Returns slots to hand back to the semaphore.
    fn relax(&mut self) -> usize {
        if !self.spacing.is_zero() {
            let decayed = self.spacing / 2;
            self.spacing = if decayed < SPACING_FLOOR {
                Duration::ZERO
            } else {
                decayed
            };
        }

        if self.in_flight >= self.ceiling {
            return 0;
        }
        self.clean += 1;
        if self.clean < SLOTS_RECOVER_AFTER {
            return 0;
        }
        self.clean = 0;
        self.restore_slots(1)
    }

    /// Widen the allowance by `n`, paying off any outstanding debt first —
    /// those slots were never actually withdrawn, so handing them back would
    /// mint permits the semaphore never lost.
    fn restore_slots(&mut self, n: usize) -> usize {
        self.in_flight += n;
        let cancelled = n.min(self.debt);
        self.debt -= cancelled;
        n - cancelled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Most tests run against a caller with plenty of budget left, so
    /// `reserve` never declines for that reason. The one that exercises the
    /// decline says so.
    const RICH: Duration = Duration::from_secs(60);

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    fn wait(pace: &mut Pace, now: Instant) -> Duration {
        pace.reserve(now, RICH).expect("a rich caller is never declined")
    }

    /// The property the whole module is answerable to: a host that answers
    /// cleanly is charged nothing at all, however hard it is being used.
    #[test]
    fn a_healthy_host_is_never_charged() {
        let base = Instant::now();
        let mut pace = Pace::new(MAX_IN_FLIGHT);
        pace.next_start = base;

        for _ in 0..50 {
            assert_eq!(wait(&mut pace, base), Duration::ZERO);
            assert_eq!(pace.relax(), 0, "a healthy host has nothing to hand back");
        }
        assert_eq!(pace.in_flight, MAX_IN_FLIGHT);
        assert_eq!(pace.spacing, Duration::ZERO);
    }

    /// A burst is refused in parallel, so the cap has to collapse within one
    /// burst — halving per refusal is what makes that happen.
    #[test]
    fn refusals_halve_the_slot_count_within_one_burst() {
        let base = Instant::now();
        let mut pace = Pace::new(MAX_IN_FLIGHT);

        assert_eq!(pace.penalise(base, None).in_flight, 3);
        assert_eq!(pace.penalise(base, None).in_flight, MIN_IN_FLIGHT);
        // And it stops there rather than starving the resolver.
        assert_eq!(pace.penalise(base, None).in_flight, MIN_IN_FLIGHT);
        assert_eq!(pace.debt, MAX_IN_FLIGHT - MIN_IN_FLIGHT);
    }

    #[test]
    fn clean_responses_buy_slots_back_one_at_a_time() {
        let base = Instant::now();
        let mut pace = Pace::new(MAX_IN_FLIGHT);
        pace.penalise(base, None);
        pace.penalise(base, None);
        assert_eq!(pace.in_flight, MIN_IN_FLIGHT);

        // The withdrawal never reached the semaphore, so recovery cancels the
        // debt instead of minting permits.
        for _ in 0..SLOTS_RECOVER_AFTER {
            assert_eq!(pace.relax(), 0);
        }
        assert_eq!(pace.in_flight, MIN_IN_FLIGHT + 1);
        assert_eq!(pace.debt, MAX_IN_FLIGHT - MIN_IN_FLIGHT - 1);
    }

    /// Debt that *was* settled against the semaphore has to come back as real
    /// permits, or the host would shrink permanently.
    #[test]
    fn settled_debt_is_returned_as_permits() {
        let base = Instant::now();
        let mut pace = Pace::new(MAX_IN_FLIGHT);
        pace.penalise(base, None);
        pace.debt = 0; // as if `admit` had withdrawn them all

        for _ in 0..SLOTS_RECOVER_AFTER - 1 {
            assert_eq!(pace.relax(), 0);
        }
        assert_eq!(pace.relax(), 1);
        assert_eq!(pace.in_flight, 4);
    }

    #[test]
    fn refusals_space_requests_and_double() {
        let base = Instant::now();
        let mut pace = Pace::new(MAX_IN_FLIGHT);
        pace.next_start = base;

        assert_eq!(pace.penalise(base, None).spacing, SPACING_MIN);
        // The gap is charged straight away, so the retry waits it out.
        assert_eq!(wait(&mut pace, base), SPACING_MIN);
        // ...and so does anything queued behind it.
        assert_eq!(wait(&mut pace, base), SPACING_MIN * 2);

        assert_eq!(pace.penalise(base, None).spacing, SPACING_MIN * 2);
        assert_eq!(pace.penalise(base, None).spacing, SPACING_MIN * 4);
    }

    #[test]
    fn spacing_is_capped() {
        let base = Instant::now();
        let mut pace = Pace::new(MAX_IN_FLIGHT);
        for _ in 0..20 {
            pace.penalise(base, None);
        }
        assert_eq!(pace.spacing, SPACING_MAX);
    }

    #[test]
    fn parallel_refusals_cannot_queue_past_the_ceiling() {
        let base = Instant::now();
        let mut pace = Pace::new(MAX_IN_FLIGHT);
        pace.next_start = base;

        for _ in 0..20 {
            pace.penalise(base, None);
        }
        assert_eq!(wait(&mut pace, base), MAX_QUEUE_AHEAD);
    }

    #[test]
    fn a_caller_that_cannot_afford_the_queue_declines_without_joining_it() {
        let base = Instant::now();
        let mut pace = Pace::new(MAX_IN_FLIGHT);
        pace.next_start = at(base, 1_500);
        pace.spacing = Duration::from_millis(500);
        pace.penalised_at = base;

        assert_eq!(pace.reserve(base, Duration::from_secs(1)), None);
        // Declining left the queue exactly as it was, so the caller behind it
        // still sees the same wait rather than a longer one.
        assert_eq!(
            pace.reserve(base, RICH),
            Some(Duration::from_millis(1_500)),
            "a declined caller must not have advanced the queue"
        );
    }

    #[test]
    fn success_decays_spacing_to_nothing() {
        let base = Instant::now();
        let mut pace = Pace::new(MAX_IN_FLIGHT);
        pace.next_start = base;
        pace.penalise(base, None);

        let mut steps = 0;
        while !pace.spacing.is_zero() {
            pace.relax();
            steps += 1;
            assert!(steps < 32, "spacing never reached zero");
        }
        // Back to no pacing at all, not merely a small gap.
        assert_eq!(wait(&mut pace, at(base, 10_000)), Duration::ZERO);
    }

    #[test]
    fn retry_after_wins_when_longer_but_stays_bounded() {
        let base = Instant::now();
        let mut pace = Pace::new(MAX_IN_FLIGHT);

        assert_eq!(
            pace.penalise(base, Some(Duration::from_millis(500))).spacing,
            Duration::from_millis(500)
        );
        // A shorter hint does not undo the escalation we already decided on.
        assert_eq!(
            pace.penalise(base, Some(Duration::from_millis(10))).spacing,
            SPACING_MAX
        );
        // An absurd hint is clamped rather than obeyed.
        assert_eq!(
            pace.penalise(base, Some(Duration::from_secs(3600))).spacing,
            SPACING_MAX
        );
    }

    #[test]
    fn an_idle_host_is_forgiven_slots_and_all() {
        let base = Instant::now();
        let mut pace = Pace::new(MAX_IN_FLIGHT);
        pace.next_start = base;
        pace.penalise(base, None);
        pace.penalise(base, None);
        pace.debt = 0; // as if the withdrawals had been settled
        assert_eq!(pace.in_flight, MIN_IN_FLIGHT);

        // Still paced while the session is warm: the debt from the refusal has
        // been paid by now, but two back-to-back requests are still spaced.
        let warm = at(base, 1_000);
        assert_eq!(pace.forgive(warm), 0);
        assert_eq!(wait(&mut pace, warm), Duration::ZERO);
        assert!(!wait(&mut pace, warm).is_zero());

        let idle = at(base, SPACING_FORGET.as_millis() as u64 + 1_000);
        assert_eq!(pace.forgive(idle), MAX_IN_FLIGHT - MIN_IN_FLIGHT);
        assert_eq!(pace.spacing, Duration::ZERO);
        assert_eq!(pace.in_flight, MAX_IN_FLIGHT);
        assert_eq!(wait(&mut pace, idle), Duration::ZERO);
    }

    #[test]
    fn an_idle_host_banks_no_burst_credit() {
        let base = Instant::now();
        let mut pace = Pace::new(MAX_IN_FLIGHT);
        pace.next_start = base;
        pace.spacing = Duration::from_millis(100);
        pace.penalised_at = base;

        // Ten seconds of silence must not buy a hundred free requests.
        let later = at(base, 10_000);
        assert_eq!(wait(&mut pace, later), Duration::ZERO);
        assert_eq!(wait(&mut pace, later), Duration::from_millis(100));
    }

    #[test]
    fn host_key_avoids_allocating_on_the_common_path() {
        let implicit = reqwest::Url::parse("https://interface.music.163.com/eapi/song").unwrap();
        assert!(matches!(host_key(&implicit), Cow::Borrowed("interface.music.163.com")));

        let explicit = reqwest::Url::parse("https://music.163.com:8443/weapi/x").unwrap();
        assert_eq!(host_key(&explicit), "music.163.com:8443");
    }

    #[test]
    fn gates_are_per_host_and_stable() {
        let governor = Governor::new();
        let a = governor.host("interface.music.163.com");
        let b = governor.host("interface.music.163.com");
        let c = governor.host("music.163.com");

        assert!(Arc::ptr_eq(&a, &b));
        assert!(!Arc::ptr_eq(&a, &c));
    }

    /// What speculative work reads before deciding to run. A healthy client must
    /// report healthy, or prefetching would never happen at all; a refused one
    /// must report strained, or prefetches would pile onto a burst that is
    /// already being shed.
    #[test]
    fn strain_is_visible_to_speculative_work() {
        let governor = Governor::new();
        assert!(!governor.is_strained(), "an untouched client is not strained");

        let gate = governor.host("interface3.music.163.com");
        // A host nobody has provoked stays out of the way.
        assert!(!governor.is_strained());

        gate.penalise(None);
        assert!(
            governor.is_strained(),
            "a refusal must switch speculation off"
        );

        // And it stays off until the host has actually recovered — both the gap
        // and the withdrawn slots have to come back.
        for _ in 0..64 {
            gate.relax();
        }
        assert!(
            !governor.is_strained(),
            "a recovered host must let speculation resume"
        );
    }

    /// Strain on one host is enough. The protocol layer spreads a single screen
    /// across `music.163.com`, `interface*.music.163.com` and
    /// `interfacepc.music.163.com`, and the thing being rate limited is the
    /// client, not the hostname.
    #[test]
    fn strain_on_any_host_counts() {
        let governor = Governor::new();
        let _ = governor.host("music.163.com");
        governor.host("interface3.music.163.com").penalise(None);
        assert!(governor.is_strained());
    }

    // ── end-to-end cost of the gate ──────────────────────────────
    //
    // These run the real `admit` against a simulated request so the latency the
    // cap actually adds is a number in the test output rather than an argument.

    async fn fan_out(gate: Arc<HostGate>, calls: usize, each: Duration) -> Duration {
        let start = Instant::now();
        let mut tasks = Vec::with_capacity(calls);
        for _ in 0..calls {
            let gate = gate.clone();
            tasks.push(tokio::spawn(async move {
                let slot = gate.admit(RICH).await.expect("admitted");
                tokio::time::sleep(each).await;
                drop(slot);
            }));
        }
        for t in tasks {
            t.await.expect("task panicked");
        }
        start.elapsed()
    }

    /// A twelve-call page load costs two round trips at the default ceiling,
    /// and would cost four at a cap of three. That difference is why the cap is
    /// not a constant any more.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fan_out_cost_is_the_number_of_waves() {
        const EACH: Duration = Duration::from_millis(60);

        let healthy = fan_out(Arc::new(HostGate::with_ceiling(MAX_IN_FLIGHT)), 12, EACH).await;
        let narrow = fan_out(Arc::new(HostGate::with_ceiling(3)), 12, EACH).await;
        println!("  12 calls @{EACH:?}: ceiling {MAX_IN_FLIGHT} took {healthy:?}, ceiling 3 took {narrow:?}");

        // Two waves versus four, with room for scheduler noise on either side.
        assert!(healthy >= EACH * 2, "{healthy:?} is fewer than two waves");
        assert!(healthy < EACH * 4, "{healthy:?} is more than two waves");
        assert!(narrow >= EACH * 4, "{narrow:?} is fewer than four waves");
    }

    /// And a burst that fits under the ceiling costs exactly one round trip —
    /// the gate adds no wave of its own.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_burst_within_the_ceiling_is_one_round_trip() {
        const EACH: Duration = Duration::from_millis(60);

        let elapsed = fan_out(Arc::new(HostGate::new()), MAX_IN_FLIGHT, EACH).await;
        println!("  {MAX_IN_FLIGHT} calls @{EACH:?}: {elapsed:?}");
        assert!(elapsed < EACH * 2, "{elapsed:?} is more than one wave");
    }
}
