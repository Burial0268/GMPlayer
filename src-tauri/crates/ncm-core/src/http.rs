//! HTTP transport for the protocol layer.
//!
//! Async on purpose. The JS side issues one request per endpoint call, and a
//! page load fans out a dozen of those; with a blocking client a single isolate
//! can only have one request in flight, so those calls serialize and the whole
//! point of removing the proxy hop is lost. Awaiting instead lets the isolate
//! run other jobs while a response is outstanding.
//!
//! `reqwest` rather than a standalone client because hyper/reqwest is already
//! linked into the app through `tauri-plugin-http` and `tauri` itself — this
//! adds no new stack.
//!
//! Keep-alive is worth a lot on one host and very little on another, and the two
//! constants below carry the measurement rather than a rule of thumb:
//! `music.163.com` reuses a connection for 25–390 ms of saving per call, while
//! `interface3.music.163.com` closes early enough that reuse past a few seconds
//! is a wasted round trip. reqwest's pool is per-client, so the timeout favours
//! the host that is asked far more often and [`TCP_KEEPALIVE`] plus
//! [`ATTEMPT_TIMEOUT`] bound what being wrong costs.
//!
//! The failure that shape exists to prevent: a request written into a socket the
//! peer has silently dropped produces neither a response nor an error. It hangs.
//! Without a per-attempt ceiling it hangs for the entire call budget and then
//! surfaces as `transport error (timeout)`, which upstream `util/request.js`
//! reports as a 502 — a failure the user sees, invented entirely on this side of
//! the socket.
//!
//! ## Bursts
//!
//! Fanning a dozen calls out at once is also how a client gets rate limited,
//! and Netease's answer to a burst is usually a refused or reset connection
//! rather than a 429 — which surfaces as `transport error (…)`, becomes a
//! rejected axios promise, and is finally reported by upstream
//! `util/request.js` as `{ status: 502, code: 502 }`. That 502 is ours, not
//! theirs. [`crate::throttle`] shapes the outbound traffic so the burst stays
//! inside what the service tolerates, and the retry loop below resends only the
//! failures that provably never reached an origin.

use std::time::{Duration, Instant};

use crate::throttle::{host_key, Governor};

/// Default *total* budget for a call, retries included.
///
/// Netease answers in tens of milliseconds, so anything past a second or two is
/// not a slow request — it is a request that will never be answered, and the
/// usual cause is a pooled socket the peer has silently dropped. The budget is
/// therefore what bounds that damage: it was 20 s, which meant one stale
/// connection cost twenty seconds and then surfaced as
/// `transport error (timeout)` → an in-band 502 the caller reports as a failure.
///
/// Sized to stay under `ncm::CALL_TIMEOUT` (25 s at the IPC boundary) so a slow
/// network still reports its real error rather than the isolate looking wedged,
/// while being tight enough that a hung attempt is abandoned in seconds. The
/// retry loop spends *this* budget rather than adding to it.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Ceiling on a single attempt.
///
/// Separate from the total budget, and the reason a stale connection is now
/// survivable rather than fatal. A request written into a socket the peer has
/// forgotten produces no response and no error — it simply hangs — so the only
/// way to notice is to stop waiting. At a p50 of ~80 ms, an attempt still
/// unanswered after this is not slow, it is lost.
///
/// The retry that follows gets a different connection: the failed one is
/// discarded rather than returned to the pool, so the resend is not a second
/// gamble on the same dead socket.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(4);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a connection may sit unused before the pool drops it.
///
/// **The two hosts disagree about this, and the measurement was worth redoing.**
/// An earlier version set 300 s on the reasoning that a per-track resolve would
/// otherwise always handshake; that was cut to 5 s after measuring
/// `interface3.music.163.com` and finding a pooled connection *slower* than a
/// fresh one after an idle gap. Both readings were real, and generalizing from
/// the second one was the mistake — it made the other host much worse:
///
/// | idle | `music.163.com` pooled / fresh | `interface3` pooled / fresh |
/// |------|-------------------------------|-----------------------------|
/// | 0 ms | **21 / 46 ms** | 100 / 217 ms |
/// | 3 s | **30 / 266 ms** | 132 / 195 ms |
/// | 8 s | **19 / 56 ms** | **523** / 203 ms |
/// | 12 s | **20 / 410 ms** | 296 / 202 ms |
///
/// `music.163.com` — weapi, so `song_detail`, `cloudsearch`, login — holds a
/// connection open happily and reuse is worth 25–390 ms every single call.
/// `interface3` (xeapi, `song_url_v1`) closes early and reuse turns into a
/// wasted round trip past ~5 s.
///
/// reqwest's pool timeout is per-client, not per-host, so this is one number for
/// both and it is set to favour the host that is asked more often: search runs
/// while the user types, `song_detail` backs every list, and a track resolve
/// happens once every few minutes. Being occasionally stale on `interface3`
/// costs one slow resolve; reconnecting on `music.163.com` costs every call.
///
/// [`TCP_KEEPALIVE`] is what makes the long form safe — see there.
const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Bounds what the pool retains. Kept in step with `throttle::MAX_IN_FLIGHT`,
/// which is the concurrency a healthy host gets: a request beyond it would open
/// a connection this pool then immediately drops.
const POOL_MAX_IDLE_PER_HOST: usize = 6;

/// Keepalive probes on pooled connections.
///
/// This is what makes [`POOL_IDLE_TIMEOUT`]'s long form safe, so the two move
/// together. A graceful close is already handled — the peer's FIN makes the
/// socket readable and hyper drops it from the pool before handing it out — but a
/// *silent* drop, which is what a cellular NAT or a stateful firewall does to an
/// idle binding, leaves a socket that looks alive and swallows whatever is
/// written to it. That is the case that used to hang for the whole call budget.
///
/// Probing every few seconds means the OS discovers the dead peer while the
/// connection is still idle, so the pool has already discarded it by the time a
/// request wants one. Short enough to beat the pool timeout to the problem,
/// long enough not to keep a phone's radio busy.
const TCP_KEEPALIVE: Duration = Duration::from_secs(10);

/// Refuse absurd response bodies rather than letting a malformed or hostile
/// response drive an unbounded allocation.
const MAX_BODY: usize = 32 * 1024 * 1024;

/// Total attempts for one call, first try included.
///
/// Two resends, because the failure this exists for is a burst being shed:
/// either the pacing gap the first failure installs is enough to get through,
/// or the service is not merely busy and a third attempt is just more load.
const MAX_ATTEMPTS: u32 = 3;

/// A retry is only worth starting if this much of the budget survives it.
/// Resending into two hundred milliseconds produces a timeout instead of an
/// answer, and replaces a truthful error with a misleading one.
const MIN_RETRY_BUDGET: Duration = Duration::from_secs(2);

pub struct HttpResponse {
    pub status: u16,
    pub status_text: String,
    /// Lowercased header names. `set-cookie` keeps every value; the protocol
    /// layer maps over it as an array.
    pub headers: Vec<(String, Vec<String>)>,
    pub body: Vec<u8>,
}

#[derive(Debug)]
pub struct HttpError(String);

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a transport failure was, and whether resending is safe.
///
/// The distinction is not about how likely the resend is to succeed — it is
/// about whether the origin can already have acted on the request. Most of the
/// protocol layer is POST, including endpoints that are read-only in every
/// sense but the verb, so the HTTP layer cannot tell a lookup from a `scrobble`
/// and must assume the worst wherever a response was already being produced.
struct Failure {
    kind: &'static str,
    resend: bool,
}

fn classify(e: &reqwest::Error) -> Failure {
    // Connect first: a connect *timeout* is both, and it is the one timeout
    // that is provably safe to resend because nothing was ever written.
    if e.is_connect() {
        return Failure {
            kind: "connect",
            resend: true,
        };
    }
    if e.is_timeout() {
        // The request is on the wire and unanswered. Resending it would be a
        // second `like`, a second `scrobble`, a second comment — and this layer
        // cannot tell those from a lookup, because nearly everything upstream is
        // a POST. Bounded by `ATTEMPT_TIMEOUT` rather than retried: the caller
        // gets a truthful error in seconds instead of a duplicated side effect.
        return Failure {
            kind: "timeout",
            resend: false,
        };
    }
    if e.is_body() || e.is_decode() {
        // The response had already started; the origin ran the call.
        return Failure {
            kind: "body",
            resend: false,
        };
    }
    if e.is_request() {
        // Everything hyper reports while sending and before a response line:
        // in practice a reset. hyper-util already resent this once by itself if
        // the connection came from the pool (`retry_canceled_requests`), so
        // what reaches here is a *fresh* connection the peer closed on us —
        // which is what being shed by an edge looks like.
        return Failure {
            kind: "reset",
            resend: true,
        };
    }
    Failure {
        kind: "transport",
        resend: false,
    }
}

/// How a completed response bears on the host's health.
enum Pressure {
    /// Explicit "not now". The request was refused rather than processed, so
    /// resending it is safe.
    Refused,
    /// The edge is unwell. Slow down, but do not resend: a 502 can equally mean
    /// the origin ran the call and only the answer was lost.
    Strained,
    None,
}

fn pressure(status: reqwest::StatusCode) -> Pressure {
    match status.as_u16() {
        429 | 503 => Pressure::Refused,
        500 | 502 | 504 => Pressure::Strained,
        _ => Pressure::None,
    }
}

/// `Retry-After`, when it is stated in seconds.
///
/// The HTTP-date form is deliberately not parsed: it needs a date parser for a
/// spelling no Netease endpoint uses, and falling back to the computed gap is
/// the same order of magnitude anyway.
fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let raw = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    raw.trim().parse::<u64>().ok().map(Duration::from_secs)
}

fn collect_headers(resp: &reqwest::Response) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for name in resp.headers().keys() {
        let values: Vec<String> = resp
            .headers()
            .get_all(name)
            .iter()
            .filter_map(|v| v.to_str().ok().map(str::to_string))
            .collect();
        out.push((name.as_str().to_ascii_lowercase(), values));
    }
    out
}

pub struct HttpClient {
    client: reqwest::Client,
    governor: Governor,
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClient {
    /// Whether the client is currently being paced by any host.
    ///
    /// See [`Governor::is_strained`]. Read by [`crate::NcmCore::prefetch`] to
    /// drop speculative work rather than add to a burst that is already being
    /// refused.
    pub(crate) fn is_strained(&self) -> bool {
        self.governor.is_strained()
    }

    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            // No client-wide `timeout`: the budget is per call and is spent
            // across attempts, so each attempt sets its own from what is left.
            // A default here would silently give every retry a fresh 20s.
            .pool_idle_timeout(POOL_IDLE_TIMEOUT)
            .tcp_keepalive(TCP_KEEPALIVE)
            .pool_max_idle_per_host(POOL_MAX_IDLE_PER_HOST)
            // Netease answers 302 on some endpoints and the protocol layer
            // wants to see them, exactly as they surfaced through the deployed
            // server.
            .redirect(reqwest::redirect::Policy::none())
            // No cookie jar, which is reqwest's default and is deliberate here:
            // the protocol layer sets every header the Netease client sends,
            // including its own cookie handling, and an implicit jar would
            // inject cookies the signed request did not account for. The
            // `cookies` feature is intentionally not enabled.
            .build()
            // Only fails if the TLS backend cannot initialize, which is not
            // something the caller can do anything about.
            .unwrap_or_else(|e| panic!("could not build the HTTP client: {e}"));
        Self {
            client,
            governor: Governor::new(),
        }
    }

    pub async fn request(
        &self,
        method: &str,
        url: &str,
        headers: &[(String, String)],
        body: Option<Vec<u8>>,
        timeout: Option<Duration>,
    ) -> Result<HttpResponse, HttpError> {
        let method = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|_| HttpError("unsupported HTTP method".into()))?;
        // Parsed once rather than per attempt, and needed anyway to key the
        // per-host gate.
        let url = reqwest::Url::parse(url).map_err(|_| HttpError("malformed request URL".into()))?;
        let host = host_key(&url);
        let gate = self.governor.host(&host);

        let deadline = Instant::now() + timeout.unwrap_or(DEFAULT_TIMEOUT);
        let mut attempt: u32 = 1;

        loop {
            // Held for the whole attempt: dropping it early would hand the slot
            // on while this request is still on the wire. Released on every
            // exit from this iteration, `continue` included.
            //
            // A `None` means the host's queue is already longer than what is
            // left of the budget. Reporting that now beats sleeping through the
            // remainder and reporting a timeout that never reached the wire.
            let queued = Instant::now();
            let waiting = deadline.saturating_duration_since(queued);
            let Some(_slot) = gate.admit(waiting).await else {
                return Err(HttpError(format!(
                    "throttled at the client: {host} is being paced and this call's budget \
                     is shorter than the queue"
                )));
            };
            // Split out because these two are answerable to different things: a
            // gate wait is our own pacing and should be zero on a healthy host,
            // while the round trip is the network and is the floor. Confusing
            // them sends you optimizing the wrong half.
            let gated = queued.elapsed();
            let sent = Instant::now();

            let budget = deadline.saturating_duration_since(Instant::now());
            if budget.is_zero() {
                return Err(HttpError(describe("timeout", &host, attempt)));
            }
            // Each attempt is bounded by the smaller of what is left and
            // `ATTEMPT_TIMEOUT`, so one hung connection cannot eat the whole
            // budget and leave nothing for a retry to run in.
            let attempt_budget = budget.min(ATTEMPT_TIMEOUT);

            let mut req = self
                .client
                .request(method.clone(), url.clone())
                .timeout(attempt_budget);
            for (k, v) in headers {
                req = req.header(k.as_str(), v.as_str());
            }
            if let Some(b) = &body {
                // Cloned because an attempt consumes it. Request bodies here
                // are encrypted parameter envelopes — hundreds of bytes — and
                // the one endpoint that would upload a file is stubbed out of
                // the bundle.
                req = req.body(b.clone());
            }

            // A 4xx/5xx is a response, not a transport failure: the protocol
            // layer parses those bodies (Netease reports `code` in-band and the
            // caller maps 301/302 to real states). Only transport errors are
            // `Err`.
            let resp = match req.send().await {
                Ok(resp) => resp,
                Err(e) => {
                    // Every transport failure is a health signal even when it
                    // is not a resendable one: whatever it was, this host is
                    // not answering cleanly right now.
                    let failure = classify(&e);
                    let backoff = gate.penalise(None);
                    log::warn!(
                        target: "ncm-core",
                        "{host}: {} (attempt {attempt}/{MAX_ATTEMPTS}); {} in flight, {}ms apart",
                        failure.kind,
                        backoff.in_flight,
                        backoff.spacing.as_millis()
                    );
                    let left = deadline.saturating_duration_since(Instant::now());
                    if failure.resend && may_retry(attempt, left, backoff.spacing) {
                        attempt += 1;
                        continue;
                    }
                    // The URL can carry a signed CDN token or query
                    // credentials, so report the host and the kind of failure
                    // without echoing the request.
                    return Err(HttpError(describe(failure.kind, &host, attempt)));
                }
            };

            let status = resp.status();
            let net = sent.elapsed();
            let pressure = pressure(status);
            match pressure {
                Pressure::Refused => {
                    let backoff = gate.penalise(retry_after(resp.headers()));
                    log::warn!(
                        target: "ncm-core",
                        "{host}: refused with {} (attempt {attempt}/{MAX_ATTEMPTS}); {} in flight, {}ms apart",
                        status.as_u16(),
                        backoff.in_flight,
                        backoff.spacing.as_millis()
                    );
                    let left = deadline.saturating_duration_since(Instant::now());
                    if may_retry(attempt, left, backoff.spacing) {
                        attempt += 1;
                        continue;
                    }
                }
                Pressure::Strained => {
                    let backoff = gate.penalise(None);
                    log::warn!(
                        target: "ncm-core",
                        "{host}: answered {}; {} in flight, {}ms apart",
                        status.as_u16(),
                        backoff.in_flight,
                        backoff.spacing.as_millis()
                    );
                }
                Pressure::None => {}
            }

            let status_text = status.canonical_reason().unwrap_or_default().to_string();
            let response_headers = collect_headers(&resp);

            // Check the advertised length first so an oversized body is refused
            // before it is buffered, then bound the actual read as well — the
            // header is a hint, not a guarantee.
            if resp.content_length().is_some_and(|n| n > MAX_BODY as u64) {
                return Err(HttpError("response body exceeded the size limit".into()));
            }
            let body = match resp.bytes().await {
                Ok(b) => b,
                Err(_) => {
                    // A body that dies mid-stream is the same signal as a reset
                    // connection. It is simply too late to resend: the origin
                    // has already run the call.
                    gate.penalise(None);
                    return Err(HttpError(describe("body", &host, attempt)));
                }
            };
            if body.len() > MAX_BODY {
                return Err(HttpError("response body exceeded the size limit".into()));
            }

            // Only a clean exchange decays the pacing gap — a 429 we ran out of
            // attempts on is still a 429.
            if matches!(pressure, Pressure::None) {
                gate.relax();
            }

            // The one line that says where a slow call went. `gated` is our own
            // pacing and should be zero; `net` is the round trip and is the
            // floor. A `net` far above the usual means the connection was cold —
            // which is the difference between ~80 ms and ~190 ms and is why the
            // pool timeouts are sized the way they are.
            log::debug!(
                target: "ncm-core::timing",
                "{host}{} {} in {}ms (gated {}ms, net {}ms, {} B{})",
                url.path(),
                status.as_u16(),
                queued.elapsed().as_millis(),
                gated.as_millis(),
                net.as_millis(),
                body.len(),
                if attempt > 1 { format!(", attempt {attempt}") } else { String::new() },
            );

            return Ok(HttpResponse {
                status: status.as_u16(),
                status_text,
                headers: response_headers,
                body: body.to_vec(),
            });
        }
    }
}

fn may_retry(attempt: u32, remaining: Duration, spacing: Duration) -> bool {
    // `spacing` is added because the next attempt waits it out in `admit`
    // before it sends anything.
    attempt < MAX_ATTEMPTS && remaining >= spacing + MIN_RETRY_BUDGET
}

fn describe(kind: &str, host: &str, attempt: u32) -> String {
    if attempt > 1 {
        format!("transport error ({kind}) via {host} after {attempt} attempts")
    } else {
        format!("transport error ({kind}) via {host}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_stop_at_the_attempt_ceiling() {
        let plenty = Duration::from_secs(19);
        assert!(may_retry(1, plenty, Duration::ZERO));
        assert!(may_retry(2, plenty, Duration::ZERO));
        assert!(!may_retry(MAX_ATTEMPTS, plenty, Duration::ZERO));
    }

    /// The budget has to hold more than one attempt, or a single hung connection
    /// eats the whole call and the retry that would have rescued it never runs.
    /// This is the shape that turns a dead pooled socket into a user-visible 502.
    #[test]
    fn the_budget_holds_more_than_one_hung_attempt() {
        assert!(
            ATTEMPT_TIMEOUT < DEFAULT_TIMEOUT,
            "an attempt must not be able to consume the entire budget"
        );
        assert!(
            ATTEMPT_TIMEOUT + MIN_RETRY_BUDGET <= DEFAULT_TIMEOUT,
            "a hung first attempt must leave enough budget for a retry to be worth starting"
        );
        // And the whole thing has to finish inside the IPC ceiling
        // (`ncm::CALL_TIMEOUT`, 25s), or the frontend reports the isolate as
        // wedged instead of the network as slow.
        assert!(DEFAULT_TIMEOUT < Duration::from_secs(25));
    }

    /// Netease answers in tens of milliseconds, so the attempt ceiling exists to
    /// notice a request that will never be answered — not to cut off a slow one.
    #[test]
    fn the_attempt_ceiling_is_far_above_a_real_response() {
        // Two orders of magnitude above the measured p50 of ~80 ms.
        assert!(ATTEMPT_TIMEOUT >= Duration::from_secs(2));
        // ...and low enough that a hang is noticed while the user is still
        // waiting rather than after they have given up.
        assert!(ATTEMPT_TIMEOUT <= Duration::from_secs(5));
    }

    #[test]
    fn retries_stop_when_the_budget_cannot_hold_one() {
        // The gap the failure installed is spent before the resend, so it
        // counts against what is left.
        assert!(may_retry(1, Duration::from_secs(3), Duration::from_millis(150)));
        assert!(!may_retry(1, Duration::from_secs(2), Duration::from_millis(150)));
        assert!(!may_retry(1, Duration::from_millis(500), Duration::ZERO));
    }

    #[test]
    fn backpressure_statuses_are_classified() {
        let refused = |c: u16| matches!(pressure(reqwest::StatusCode::from_u16(c).unwrap()), Pressure::Refused);
        let strained = |c: u16| matches!(pressure(reqwest::StatusCode::from_u16(c).unwrap()), Pressure::Strained);

        assert!(refused(429));
        assert!(refused(503));
        assert!(strained(502));
        assert!(strained(504));
        // In-band Netease codes ride on 200s, and 301 is "not logged in" —
        // neither is a reason to slow down.
        assert!(!refused(200) && !strained(200));
        assert!(!refused(301) && !strained(301));
        assert!(!refused(400) && !strained(400));
    }

    #[test]
    fn retry_after_reads_seconds_only() {
        let mut headers = reqwest::header::HeaderMap::new();
        assert_eq!(retry_after(&headers), None);

        headers.insert(reqwest::header::RETRY_AFTER, "3".parse().unwrap());
        assert_eq!(retry_after(&headers), Some(Duration::from_secs(3)));

        headers.insert(
            reqwest::header::RETRY_AFTER,
            "Wed, 21 Oct 2015 07:28:00 GMT".parse().unwrap(),
        );
        assert_eq!(retry_after(&headers), None);
    }

    #[test]
    fn failure_messages_name_the_host_but_not_the_request() {
        assert_eq!(
            describe("reset", "interface.music.163.com", 1),
            "transport error (reset) via interface.music.163.com"
        );
        assert_eq!(
            describe("connect", "music.163.com", 3),
            "transport error (connect) via music.163.com after 3 attempts"
        );
    }
}
