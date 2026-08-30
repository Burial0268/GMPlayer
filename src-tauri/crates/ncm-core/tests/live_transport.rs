//! What the wire actually does, as opposed to what the constants assume.
//!
//! Separate from `live_protocol.rs` on purpose. These make raw `reqwest` calls
//! rather than going through the isolate, so they can hold the protocol
//! variable still and move only the transport — same host, same path, same
//! body, h1 against h2. They also want to be free of the isolate's *detached*
//! traffic: a prefetch or a stale refresh started by a protocol test outlives it
//! and would land in the middle of a latency sample here. `cargo test` runs test
//! binaries one after another, so a separate file is what buys that isolation.
//!
//! ```text
//! cargo test -p ncm-core --release --test live_transport -- --ignored --nocapture
//! ```
//!
//! ## Why these exist
//!
//! The crate's transport constants were chosen against a measurement that said
//! Netease answers HTTP/1.1 only. It does not — both hosts negotiate `h2` — and
//! the reason the measurement said otherwise is that `ncm-core` never declared
//! reqwest's `http2` feature, so its client never offered `h2` in ALPN and got
//! back the only thing it asked for. `tauri-plugin-http` *does* declare that
//! feature on the same reqwest version, and Cargo unifies features across a
//! build, so the shipped app has been on HTTP/2 the whole time while
//! `cargo test -p ncm-core` measured HTTP/1.1.
//!
//! Two consequences worth stating, because they outlive this file:
//!
//! * A crate that cares about its wire protocol must declare the feature that
//!   selects it. Inheriting it makes the transport an accident of the workspace.
//! * A measurement that contradicts a published capability is more likely to be
//!   measuring the client than the server. `openssl s_client -alpn h2,http/1.1`
//!   costs nothing and answers it directly.
//!
//! ## Reading the numbers
//!
//! Timings here describe *the path the test ran on*, which for anyone behind a
//! VPN or a TUN-mode proxy is not the path a user takes — a transparent tunnel
//! relays ALPN unchanged, so the capability findings survive it, but every
//! latency figure is the tunnel's. Treat the shape (does a burst cost one round
//! trip or three?) as the finding and the absolute milliseconds as local
//! colour.

use std::time::{Duration, Instant};

/// The two hosts the protocol layer actually splits its traffic across, and
/// they are not the same service: `music.163.com` is weapi behind a commercial
/// CDN, `interface3` is xeapi behind Netease's own nginx. The crate has been
/// caught generalizing one host's behaviour to the other before, so every
/// measurement below runs against both.
const WEAPI_HOST: &str = "https://music.163.com/";
const XEAPI_HOST: &str = "https://interface3.music.163.com/";

fn version_of(v: reqwest::Version) -> &'static str {
    match v {
        reqwest::Version::HTTP_09 => "HTTP/0.9",
        reqwest::Version::HTTP_10 => "HTTP/1.0",
        reqwest::Version::HTTP_11 => "HTTP/1.1",
        reqwest::Version::HTTP_2 => "HTTP/2",
        reqwest::Version::HTTP_3 => "HTTP/3",
        _ => "unknown",
    }
}

/// A client that negotiates whatever it can, which after the `http2` feature is
/// declared means it offers `h2` in ALPN.
fn alpn_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("client")
}

/// The transport the constants were tuned against. `http1_only` forces it
/// regardless of what the feature set allows, which is what makes the two
/// columns below a controlled comparison rather than two separate builds.
fn h1_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .http1_only()
        .build()
        .expect("client")
}

async fn fetch(client: &reqwest::Client, url: &str) -> Option<(reqwest::Version, Duration, usize)> {
    let t = Instant::now();
    let resp = client.get(url).send().await.ok()?;
    let version = resp.version();
    let body = resp.bytes().await.ok()?;
    Some((version, t.elapsed(), body.len()))
}

fn median(v: &mut Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if v.is_empty() {
        return f64::NAN;
    }
    v[v.len() / 2]
}

/// The finding the rest of this file rests on: both hosts speak HTTP/2.
///
/// Asserted rather than printed, because the whole transport configuration —
/// the concurrency ceiling, the pool size, the keepalive strategy — is now
/// justified by it. If Netease ever stops negotiating `h2`, that reasoning has
/// to be revisited and this is where it should fail.
#[tokio::test]
#[ignore = "hits music.163.com"]
async fn both_hosts_negotiate_http2() {
    let client = alpn_client();

    println!("\n  negotiated protocol");
    for url in [WEAPI_HOST, XEAPI_HOST] {
        let (version, _, _) = fetch(&client, url).await.expect("request failed");
        println!("    {url:<42} {}", version_of(version));
        assert_eq!(
            version,
            reqwest::Version::HTTP_2,
            "{url} did not negotiate h2 — the transport constants assume it does"
        );
    }

    // And the control: forcing h1 still works, so a fallback is real rather
    // than theoretical.
    let h1 = h1_client();
    for url in [WEAPI_HOST, XEAPI_HOST] {
        let (version, _, _) = fetch(&h1, url).await.expect("request failed");
        assert_eq!(version, reqwest::Version::HTTP_11);
    }
    println!("    (both also still answer HTTP/1.1 when that is all we offer)\n");
}

/// What HTTP/2 is and is not worth here, cold and warm kept apart.
///
/// A single warm request is a round trip on either protocol and h2 cannot make
/// it shorter, so the only thing worth measuring is a *burst* — a page load
/// fanning out a dozen endpoint calls at once.
///
/// The two cases pull in opposite directions and averaging them hides both:
///
/// * **Cold.** h1 opens a connection per request and those handshakes run
///   *concurrently*, so twelve of them cost about one handshake in wall clock.
///   h2 opens one connection and every request waits behind that single
///   handshake, then shares one congestion window — twelve small responses
///   through one `initcwnd` instead of twelve. h1 is expected to win.
/// * **Warm.** The handshakes are already paid. h1 now needs a free connection
///   per concurrent request; h2 needs a stream, which costs nothing. This is
///   where multiplexing is supposed to show, and it is also the state the app
///   is in for all but the first seconds of a session.
///
/// Measured, it does not show: h1 won warm as well as cold (36 vs 47 ms on
/// `music.163.com`, 49 vs 62 on `interface3`). Worth stating plainly because it
/// is the opposite of the expectation above, and worth *not* concluding much
/// from, since a burst of twelve fits inside `throttle::MAX_IN_FLIGHT`-shaped
/// traffic either way and these milliseconds belong to whatever path the test
/// ran on. The h2 change is justified by reproducibility and by connection
/// reuse, not by this table — see `http::POOL_IDLE_TIMEOUT`.
///
/// Diagnostic rather than asserted: which way each cell falls is a property of
/// the path, and the crate's transport settings are chosen from the *shape*
/// here, not from a threshold this could flake against.
#[tokio::test]
#[ignore = "hits music.163.com"]
async fn burst_cost_cold_and_warm() {
    const BURST: usize = 12;
    const SAMPLES: usize = 5;

    async fn burst(client: &reqwest::Client, url: &str, n: usize) -> Duration {
        let t = Instant::now();
        let mut tasks = Vec::with_capacity(n);
        for _ in 0..n {
            let client = client.clone();
            let url = url.to_string();
            tasks.push(tokio::spawn(async move { fetch(&client, &url).await }));
        }
        for task in tasks {
            task.await.expect("task panicked").expect("request failed");
        }
        t.elapsed()
    }

    println!("\n  {BURST} concurrent requests, median of {SAMPLES}");
    println!(
        "    {:<28} {:>11} {:>11} {:>11} {:>11}",
        "", "cold h1", "cold h2", "warm h1", "warm h2"
    );

    for url in [WEAPI_HOST, XEAPI_HOST] {
        let mut cells = Vec::new();
        for warm in [false, true] {
            for build in [h1_client as fn() -> reqwest::Client, alpn_client] {
                let mut samples = Vec::with_capacity(SAMPLES);
                for _ in 0..SAMPLES {
                    let client = build();
                    if warm {
                        // Warm with a burst, not a single request: one request
                        // leaves h1 with exactly one pooled connection, so the
                        // measured burst would still open eleven and be a cold
                        // measurement wearing a warm label.
                        burst(&client, url, BURST).await;
                    }
                    samples.push(burst(&client, url, BURST).await.as_secs_f64() * 1000.0);
                }
                cells.push(median(&mut samples));
            }
        }
        let host = url.trim_start_matches("https://").trim_end_matches('/');
        println!(
            "    {host:<28} {:>10.1}ms {:>10.1}ms {:>10.1}ms {:>10.1}ms",
            cells[0], cells[1], cells[2], cells[3]
        );
    }
    println!();
}

/// Redo of the pool-idle table, on the transport the app actually uses.
///
/// The committed table was measured on h1 and is what set
/// `http::POOL_IDLE_TIMEOUT`. It found the two hosts disagreeing sharply —
/// `music.163.com` rewarding reuse at every gap, `interface3` punishing it past
/// a few seconds — and the constant is a compromise between them, deliberately
/// wrong for one host.
///
/// That disagreement is an HTTP/1.1 artifact. An h1 keepalive is a short-lived
/// courtesy (nginx defaults to seconds) while an h2 connection is meant to be
/// long-lived and idles out in minutes, so the host that "closes early" only
/// does so on the protocol we were accidentally using. This rerun is what the
/// constant should be set from.
///
/// The third column is the setting under test: h2 PING keepalive holds an idle
/// connection open at the HTTP layer, which both resets the server's idle timer
/// and — the part TCP keepalive cannot do — proves the *connection* is alive
/// rather than just the socket.
///
/// Diagnostic only. It sleeps through several idle gaps on two hosts, so it is
/// slow, and its numbers are path-dependent enough that asserting on them would
/// make it a flake rather than a check.
#[tokio::test]
#[ignore = "slow: sleeps through idle gaps against music.163.com"]
async fn pooled_versus_fresh_across_idle_gaps() {
    const GAPS: [u64; 6] = [0, 3, 8, 12, 30, 60];

    /// The candidate configuration: an idle connection is pinged rather than
    /// merely left alone.
    fn keepalive_client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .http2_keep_alive_interval(Duration::from_secs(10))
            .http2_keep_alive_timeout(Duration::from_secs(5))
            .http2_keep_alive_while_idle(true)
            .build()
            .expect("client")
    }

    println!("\n  ms per request after an idle gap, HTTP/2");
    println!(
        "    {:<6} {:>26} {:>26}",
        "", "music.163.com", "interface3"
    );
    println!(
        "    {:<6} {:>26} {:>26}",
        "idle", "pooled / +ping / fresh", "pooled / +ping / fresh"
    );

    for gap in GAPS {
        let mut cells = [String::new(), String::new()];
        for (i, url) in [WEAPI_HOST, XEAPI_HOST].iter().enumerate() {
            let ms = |r: Option<(reqwest::Version, Duration, usize)>| match r {
                Some((_, d, _)) => format!("{:.0}", d.as_secs_f64() * 1000.0),
                None => "err".to_string(),
            };

            // Pooled: one client, warmed, then asked again after the gap.
            let pooled_client = alpn_client();
            let _ = fetch(&pooled_client, url).await;
            // Pinged: the same, but holding the connection open at the h2 layer.
            let pinged_client = keepalive_client();
            let _ = fetch(&pinged_client, url).await;

            tokio::time::sleep(Duration::from_secs(gap)).await;

            let pooled = ms(fetch(&pooled_client, url).await);
            let pinged = ms(fetch(&pinged_client, url).await);

            // Fresh: a client that has never connected, so it pays DNS+TCP+TLS.
            tokio::time::sleep(Duration::from_millis(200)).await;
            let fresh = ms(fetch(&alpn_client(), url).await);

            cells[i] = format!("{pooled} / {pinged} / {fresh}");
        }
        println!(
            "    {:<6} {:>26} {:>26}",
            format!("{gap}s"),
            cells[0],
            cells[1]
        );
    }
    println!(
        "\n    (pooled far above fresh means the peer dropped the connection during the gap\n     \
         and the reuse attempt paid for discovering that)\n"
    );
}

/// HTTP/2 flow control against the bodies this crate actually receives.
///
/// The trap: h2's default per-stream receive window is 64 KiB, so a body past
/// that size can only advance one window per round trip — a throughput ceiling
/// of `window / RTT` regardless of the available bandwidth. On a path with a
/// 40 ms RTT that is about 1.6 MB/s, which would make h2 dramatically *slower*
/// than h1 for a large enough response. hyper's BDP estimator
/// (`http2_adaptive_window`) grows the window to match the path instead.
/// (Whether this layer *has* a large enough response is the next test's
/// question, and the answer is no — it chunks.)
///
/// Measured as **effective throughput against that ceiling**, because that is
/// the signature that distinguishes the two: a starved stream sits at
/// `64 KiB / RTT` almost exactly, while a healthy one runs far above it. A
/// straight A/B of wall-clock times cannot tell them apart on a body too small
/// to fill even one window, which is what an earlier version of this test got
/// wrong — the largest asset reachable without a signed request is 146 KiB, and
/// at that size all three configurations look identical.
///
/// Diagnostic: the number depends on the path's bandwidth as much as on the
/// window, so what to read is the ratio, not the megabytes.
#[tokio::test]
#[ignore = "hits Netease CDN"]
async fn large_bodies_are_not_starved_by_the_default_window() {
    const LARGE: &str = "https://p1.music.126.net/6y-UleORITEDbvrOLV0Q8A==/5639395138885805.jpg";
    const SAMPLES: usize = 7;
    const DEFAULT_WINDOW: f64 = 64.0 * 1024.0;

    // The RTT this path actually has, so the ceiling below is this machine's
    // and not a number copied from someone else's network.
    let probe = alpn_client();
    let mut rtts = Vec::new();
    for _ in 0..SAMPLES {
        if let Some((_, d, _)) = fetch(&probe, XEAPI_HOST).await {
            rtts.push(d.as_secs_f64() * 1000.0);
        }
    }
    let rtt_ms = median(&mut rtts);
    let ceiling = DEFAULT_WINDOW / (rtt_ms / 1000.0) / 1024.0 / 1024.0;
    println!("\n  path RTT {rtt_ms:.0} ms → a 64 KiB window ceilings at {ceiling:.1} MB/s");

    let fixed = alpn_client();
    let adaptive = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .http2_adaptive_window(true)
        .build()
        .expect("client");
    let h1 = h1_client();

    println!("  {:<26} {:>10} {:>12} {:>10}", "", "median", "throughput", "vs ceiling");
    for (label, client) in [
        ("HTTP/1.1", &h1),
        ("HTTP/2, default window", &fixed),
        ("HTTP/2, adaptive window", &adaptive),
    ] {
        let mut samples = Vec::with_capacity(SAMPLES);
        let mut size = 0usize;
        for _ in 0..SAMPLES {
            match fetch(client, LARGE).await {
                Some((_, d, n)) => {
                    size = n;
                    samples.push(d.as_secs_f64() * 1000.0);
                }
                None => break,
            }
        }
        if samples.is_empty() {
            println!("    {label:<26} request failed");
            continue;
        }
        let ms = median(&mut samples);
        let mb_s = (size as f64 / 1024.0 / 1024.0) / (ms / 1000.0);
        println!(
            "    {label:<26} {ms:>8.1}ms {mb_s:>10.1} MB/s {:>9.1}x  ({} KiB)",
            mb_s / ceiling,
            size / 1024
        );
    }
    println!(
        "\n    (a stream pinned at ~1.0x the ceiling is window-starved; well above it means\n     \
         flow control is not what is limiting this body)\n"
    );
}

/// The same question against the largest response the layer really produces.
///
/// The check above tops out at the 146 KiB the CDN will serve, and flow-control
/// starvation gets *worse* the longer a stream runs, so a clean result there
/// does not automatically transfer to the protocol's own bodies.
///
/// The premise this test was written with — that `playlist_track_all` "runs to
/// megabytes" — turned out to be false, which is the finding. Asking for a
/// 1000-track playlist produces no megabyte response because the endpoint
/// *chunks*: one `playlist/detail` carrying the track ids, then a `song/detail`
/// fan-out, each answer a few hundred KB. So the largest body this crate ever
/// receives is ~250 KB, an order of magnitude below where the 64 KiB window
/// would begin to bite, and `http2_adaptive_window` has nothing to fix.
///
/// This one reaches for the isolate, unlike everything else in this file,
/// because the endpoint is signed and there is no way to issue it from raw
/// `reqwest`. It reads the transport's own timing log rather than wrapping the
/// call: `net` there is the round trip alone, with the isolate's JSON
/// marshalling excluded, which is what throughput has to be computed from.
#[tokio::test]
#[ignore = "hits music.163.com"]
async fn the_largest_real_response_is_not_window_starved() {
    /// Below this a body cannot say anything about a 64 KiB window, so seeing
    /// nothing bigger means the test learned nothing and must not pass quietly.
    const MEANINGFUL: f64 = 128.0 * 1024.0;

    // Recorded by the logger below: the transport's `net {}ms, {} B` fields.
    static TIMINGS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

    struct Capture;
    impl log::Log for Capture {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            if record.target() == "ncm-core::timing" {
                TIMINGS
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(record.args().to_string());
            }
        }
        fn flush(&self) {}
    }
    static LOGGER: Capture = Capture;
    let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Debug));

    let core = ncm_core::NcmCore::new(None)
        .await
        .expect("failed to create the isolate");

    // 云音乐飙升榜 — a stable, public playlist, asked for in full.
    let out = core
        .call("playlist_track_all", r#"{"id":"19723756","limit":"1000"}"#)
        .await
        .expect("call failed");
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("not JSON");
    assert_eq!(parsed["ok"], true, "playlist_track_all did not succeed");

    // `… in 123ms (gated 0ms, net 118ms, 1523456 B)`
    fn net_and_bytes(line: &str) -> Option<(f64, f64)> {
        let (_, tail) = line.split_once("net ")?;
        let (net, rest) = tail.split_once("ms, ")?;
        let (bytes, _) = rest.split_once(" B")?;
        Some((net.parse().ok()?, bytes.parse().ok()?))
    }

    let lines = TIMINGS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let largest = lines
        .iter()
        .filter_map(|l| net_and_bytes(l).map(|(net, bytes)| (bytes, net, l)))
        .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    println!("\n  largest response the protocol layer produced");
    let Some((bytes, net, line)) = largest else {
        panic!("no timing lines were captured at all — the log target moved:\n{lines:#?}");
    };

    let mb_s = (bytes / 1024.0 / 1024.0) / (net / 1000.0);
    println!("    {}", line.trim());
    println!(
        "    → {:.0} KiB, {net:.0} ms end to end ({mb_s:.1} MB/s counting server time)",
        bytes / 1024.0
    );

    assert!(
        bytes >= MEANINGFUL,
        "largest response was only {:.0} KiB, under the {:.0} KiB this test needs before it can \
         say anything about flow control — it would otherwise pass without measuring anything. \
         Either the playlist shrank or the endpoint changed its chunking.\nlines seen: {lines:#?}",
        bytes / 1024.0,
        MEANINGFUL / 1024.0,
    );

    // The *size* is the assertion and the finding; the rate above is not a
    // transfer rate and must not be read as one. `net` is request-write to
    // response-complete, so it contains however long Netease spent building a
    // 244 KiB playlist page — which for this endpoint is most of it. A figure
    // computed that way is a lower bound on transfer speed and nothing more,
    // and it lands *below* the 64 KiB-window ceiling rather than pinned at it,
    // which is the opposite of the starvation signature.
    //
    // The flow-control question is settled by the controlled A/B in
    // `large_bodies_are_not_starved_by_the_default_window`, where the same body
    // is fetched with the default window and with `http2_adaptive_window` and
    // the default wins. This test's job is only to bound how big a body that
    // conclusion has to cover.
    println!("    (size is the finding: a few hundred KB, not the megabytes this once assumed)");
    println!();
}

/// A tripwire for HTTP/3, and a note about a header that is easy to misread.
///
/// `interface3.music.163.com` answers — *sometimes* — with
/// `alt-svc: quic=":443"; ma=2592000; v="44,43,39"`. It looks like an invitation
/// to HTTP/3 and is not one: those are *gQUIC* draft versions, Google's
/// pre-standard QUIC, frozen around 2018, and an IETF HTTP/3 client cannot speak
/// them. A real offer names `h3` plus an RFC 9114 ALPN token.
///
/// "Sometimes" is itself the thing to record. Repeated runs get the header on
/// one request and no header at all on the next, and `music.163.com` has not
/// produced it yet — these are anycast CDN edges and not every node is
/// configured alike, so a single observation of this header (in either
/// direction) proves very little. The assertion therefore keys on `h3`, which is
/// the only reading that would change a decision, rather than on the header
/// being present or absent.
#[tokio::test]
#[ignore = "hits music.163.com"]
async fn no_http3_is_advertised() {
    let client = alpn_client();

    println!("\n  alt-svc");
    for url in [WEAPI_HOST, XEAPI_HOST] {
        let resp = client.get(url).send().await.expect("request failed");
        let alt_svc = resp
            .headers()
            .get("alt-svc")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("(none)")
            .to_string();
        println!("    {url:<42} {alt_svc}");
        // Matches `h3`, `h3-29`, `h3-Q050`… but not a gQUIC `v="44,43,39"`.
        assert!(
            !alt_svc.contains("h3"),
            "{url} now advertises HTTP/3 ({alt_svc}) — h3 was ruled out because nothing offered \
             it, and that has changed"
        );
    }
    println!("    (`v=\"44,43,39\"` is gQUIC, not HTTP/3; edges differ, so absence proves little)\n");
}
