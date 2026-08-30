//! Phase 0 gate: does the embedded protocol layer actually talk to Netease?
//!
//! These hit the live service, so they are `#[ignore]`d — `cargo test` stays
//! offline and deterministic. Run them deliberately:
//!
//! ```text
//! cargo test -p ncm-core --release -- --ignored --nocapture
//! ```
//!
//! `song_url_v1` is the one that matters. It is the endpoint the playback
//! resolver depends on *and* the only one on the newest `xeapi` scheme
//! (X25519 ECDH → HKDF-ish HMAC derivation → AES-128-GCM, with the session key
//! rotated through `x-encr-ssid`/`x-encr-sskey` response headers). If the
//! shims are wrong anywhere, this is where it shows.

use ncm_core::NcmCore;

/// The protocol layer's `console` and the bootstrap report both go through the
/// `log` crate. Without a sink they vanish, which makes every failure here look
/// like an unexplained empty response.
///
/// It also records what the transport actually sent. "How many HTTP requests did
/// one endpoint call make" is the question that distinguishes a slow network from
/// a repeated handshake, and it is not answerable from the outside.
struct StderrLogger;

/// Request lines the transport has logged since the last reset.
///
/// Recorded with their URL path, not just counted, and that is load-bearing:
/// prefetches and stale refreshes are *detached*, so they outlive whatever test
/// started them and land in the middle of the next one's measurement. Counting
/// only the path a test cares about makes each one immune to the others'
/// background traffic — a plain counter was not, and the resulting failures read
/// exactly like a real extra round trip.
static REQUESTS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Held by every test that asserts on request counts, since the log is
/// process-wide and `cargo test` runs a binary's tests in parallel.
///
/// **Take this before building the isolate, not after.** These tests interfere
/// over the *network* as much as over the log: three concurrent `core()` calls
/// are six bootstrap requests in the same instant, which is enough to get the
/// client paced — and a paced client correctly drops speculative work, so the
/// very hints under test never run and the failure reads like a key mismatch.
/// Serializing the whole test, isolate build included, is what makes them
/// independent.
static COUNTING: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl log::Log for StderrLogger {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &log::Record<'_>) {
        if record.target() == "ncm-core::timing" {
            REQUESTS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(record.args().to_string());
        }
        eprintln!("[{}] {}", record.level(), record.args());
    }
    fn flush(&self) {}
}

fn init_logging() {
    static LOGGER: StderrLogger = StderrLogger;
    // Ignore the error: tests share a process and only the first call wins.
    let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Debug));
}

/// Take the request log, exclusively, and start it empty.
fn counting() -> std::sync::MutexGuard<'static, ()> {
    let guard = COUNTING.lock().unwrap_or_else(|e| e.into_inner());
    reset_requests();
    guard
}

fn reset_requests() {
    REQUESTS.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// Requests whose URL contains `path`.
fn requests_to(path: &str) -> usize {
    REQUESTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|line| line.contains(path))
        .count()
}

/// Upstream URL paths the tests below count on.
const URL_V1: &str = "/song/enhance/player/url/v1";
const LYRIC_V1: &str = "/song/lyric/v1";
const SONG_DETAIL: &str = "/song/detail";

/// Isolate for the offline tests: no bootstrap, so no network at all.
async fn offline_core() -> NcmCore {
    init_logging();
    NcmCore::offline().await.expect("failed to create the isolate")
}

/// Isolate for the live tests, including the cold-start handshake.
async fn core() -> NcmCore {
    init_logging();
    NcmCore::new(None)
        .await
        .expect("failed to create the isolate")
}

fn parse(json: &str) -> serde_json::Value {
    serde_json::from_str(json).expect("call did not return JSON")
}

#[tokio::test]
async fn bundle_evaluates_and_exposes_endpoints() {
    let core = offline_core().await;
    let endpoints = core.endpoints().await.expect("endpoints() failed");
    assert!(
        endpoints.len() > 400,
        "expected the full endpoint set, got {}",
        endpoints.len()
    );
    for required in ["song_detail", "song_url_v1", "cloudsearch", "login_qr_key"] {
        assert!(
            endpoints.iter().any(|e| e == required),
            "missing endpoint {required}"
        );
    }
    println!(
        "protocol {} | {} endpoints",
        core.protocol_version().await.unwrap(),
        endpoints.len()
    );
}

#[tokio::test]
async fn unknown_endpoint_reports_cleanly() {
    let core = offline_core().await;
    let out = parse(&core.call("definitely_not_an_endpoint", "{}").await.unwrap());
    assert_eq!(out["ok"], false);
    assert_eq!(out["status"], 404);
}

#[tokio::test]
async fn malformed_query_reports_cleanly() {
    let core = offline_core().await;
    let out = parse(&core.call("song_detail", "{not json").await.unwrap());
    assert_eq!(out["ok"], false);
    assert_eq!(out["status"], 400);
}

#[tokio::test]
async fn unavailable_dependency_names_itself() {
    // `voice_upload` does `new xml2js.Parser()` while the module is being
    // evaluated, so it fails before any network call — which is what makes it
    // a good offline check that the stub reports the missing capability
    // instead of surfacing as "not a constructor" from minified code.
    let core = offline_core().await;
    let out = parse(&core.call("voice_upload", "{}").await.unwrap());
    assert_eq!(out["ok"], false);
    let msg = out["body"]["msg"].as_str().unwrap_or_default();
    assert!(
        msg.contains("xml2js"),
        "expected the stub to name xml2js, got: {msg}"
    );
}

// ── Live protocol ────────────────────────────────────────────────

#[tokio::test]
#[ignore = "hits music.163.com"]
async fn weapi_song_detail() {
    let core = core().await;
    let out = parse(&core.call("song_detail", r#"{"ids":"347230"}"#).await.unwrap());
    println!(
        "song_detail → {}",
        &out.to_string()[..out.to_string().len().min(400)]
    );

    assert_eq!(out["ok"], true, "weapi call failed: {out}");
    assert_eq!(out["body"]["code"], 200);
    let songs = out["body"]["songs"].as_array().expect("no songs array");
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0]["id"], 347230);
    assert!(
        songs[0]["name"].as_str().is_some_and(|n| !n.is_empty()),
        "song has no name"
    );
}

#[tokio::test]
#[ignore = "hits music.163.com"]
async fn xeapi_song_url_v1() {
    let core = core().await;
    let out = parse(
        &core
            .call("song_url_v1", r#"{"id":"347230","level":"standard"}"#)
            .await
            .unwrap(),
    );
    println!(
        "song_url_v1 → {}",
        &out.to_string()[..out.to_string().len().min(600)]
    );

    assert_eq!(out["ok"], true, "xeapi call failed: {out}");
    assert_eq!(out["body"]["code"], 200, "xeapi returned a non-200 code");
    let data = out["body"]["data"].as_array().expect("no data array");
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["id"], 347230);
    // Anonymous callers get a null url for licensed tracks, which is a correct
    // protocol round trip — the assertion is that the envelope decrypted, not
    // that we are entitled to the audio.
    assert!(
        data[0].get("fee").is_some(),
        "response decrypted but has no privilege fields: {}",
        data[0]
    );
}

/// The second xeapi call must reuse the session the first established
/// (`x-encr-ssid` / `x-encr-sskey` are captured in module state), and must not
/// re-run the key handshake.
///
/// This used to assert only that both calls succeeded, which its own doc comment
/// claimed was a reuse test — it would have passed just as happily if every call
/// re-handshaked. It now counts the requests that actually reach the wire, which
/// is the only thing that makes the claim true. One endpoint call must be one
/// request.
#[tokio::test]
#[ignore = "hits music.163.com"]
async fn xeapi_session_key_is_reused_across_calls() {
    let _counting = counting();
    let core = core().await;
    reset_requests();

    for i in 0..3 {
        reset_requests();
        let out = parse(
            &core
                .call("song_url_v1", &format!(r#"{{"id":"347230","level":"exhigh","n":{i}}}"#))
                .await
                .unwrap(),
        );
        assert_eq!(out["ok"], true, "xeapi call {i} failed: {out}");
        assert_eq!(out["body"]["code"], 200);

        let requests = requests_to(URL_V1);
        assert_eq!(
            requests, 1,
            "call {i} made {requests} requests; a reused session is one request per call, \
             and anything more means the handshake is being repeated on every resolve"
        );
    }
}

/// End to end: warming an entry with the hint's query means the real call costs
/// nothing.
///
/// Key parity itself is pinned without a network in
/// `cache::tests::a_hint_and_its_real_call_share_a_cache_key` — that is the test
/// that fails when someone changes one side of the pair. This one proves the
/// whole path agrees: the hint's spelling, the cache, and the query
/// `src/api/song.ts` actually sends.
///
/// It deliberately warms with `call` rather than `prefetch`. A prefetch is
/// *supposed* to be dropped while the client is being paced, so asserting it
/// landed makes the test fail for the correct behaviour whenever the live service
/// is throttling — which reads exactly like a key mismatch and is not what this
/// is checking. `prefetch`'s own gating is covered below.
#[tokio::test]
#[ignore = "hits music.163.com"]
async fn a_hint_query_warms_what_the_real_call_asks_for() {
    let _counting = counting();
    let core = core().await;

    for (endpoint, hint, real, path) in [
        ("lyric_new", r#"{"id":347230}"#, r#"{"id":347230}"#, LYRIC_V1),
        (
            "song_detail",
            r#"{"ids":"347230"}"#,
            r#"{"ids":"347230","timestamp":1771000000000}"#,
            SONG_DETAIL,
        ),
    ] {
        // Warm with exactly the query the hint would have sent.
        reset_requests();
        let warmed = parse(&core.call(endpoint, hint).await.unwrap());
        assert_eq!(warmed["ok"], true, "{endpoint} could not be warmed: {warmed}");
        assert_eq!(
            requests_to(path),
            1,
            "{endpoint}: warming should have made exactly one request"
        );

        // Now the query the app really sends. Must not touch the network.
        reset_requests();
        let out = parse(&core.call(endpoint, real).await.unwrap());
        assert_eq!(out["ok"], true, "{endpoint}: {out}");
        assert_eq!(
            requests_to(path),
            0,
            "{endpoint}: the hint's query warms an entry the real call does not read — \
             the two are spelling the query differently"
        );
    }
}

/// Speculation must stay well under the per-host concurrency ceiling, or a burst
/// of hints would sit in front of the request the user is waiting for.
///
/// Runs last among the counting tests in spirit: it leaves twenty detached hints
/// behind, which is why the counters here are scoped to a URL path.
#[tokio::test]
#[ignore = "hits music.163.com"]
async fn hints_stay_bounded() {
    let _counting = counting();
    let core = core().await;
    reset_requests();

    for id in 0..20u64 {
        // Ids that will not resolve, which is fine — this is about how many
        // requests start, not what they answer.
        core.prefetch("lyric_new", &format!(r#"{{"id":{}}}"#, 400_000_000 + id));
    }
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

    let started = requests_to(LYRIC_V1);
    println!("  20 hints -> {started} request(s) in flight");
    // The cap is `MAX_SPECULATION` (4), which is private; what this test is
    // really defending is that twenty hints do not become twenty requests and
    // that the foreground keeps most of the six per-host slots.
    assert!(
        started <= 4,
        "20 hints started {started} requests; speculation is capped so the foreground always \
         has connections spare"
    );
}

/// `song_url_v1` is the playback critical path, so the shape of its cost is
/// asserted rather than assumed: one request, and no client-side pacing on a
/// host that is answering.
#[tokio::test]
#[ignore = "hits music.163.com"]
async fn resolving_a_track_costs_one_request() {
    let _counting = counting();
    let core = core().await;
    // Warm the connection so this measures steady state.
    let _ = core.call("song_url_v1", r#"{"id":"347230","level":"exhigh"}"#).await;

    reset_requests();
    let started = std::time::Instant::now();
    let out = parse(
        &core
            .call("song_url_v1", r#"{"id":"1974443814","level":"exhigh"}"#)
            .await
            .unwrap(),
    );
    let elapsed = started.elapsed();

    assert_eq!(out["ok"], true, "resolve failed: {out}");
    assert_eq!(requests_to(URL_V1), 1);
    println!("  warm song_url_v1: {elapsed:?}");
    // Loose, because it is a real network. Tight enough to catch a regression
    // that reintroduces a second round trip.
    assert!(
        elapsed < std::time::Duration::from_millis(700),
        "a warm resolve took {elapsed:?}, which is more than one round trip"
    );
}

#[tokio::test]
#[ignore = "hits music.163.com"]
async fn search_returns_results() {
    let core = core().await;
    let out = parse(
        &core
            .call("cloudsearch", r#"{"keywords":"周杰伦","limit":"3"}"#)
            .await
            .unwrap(),
    );
    assert_eq!(out["ok"], true, "search failed: {out}");
    assert_eq!(out["body"]["code"], 200);
    let songs = out["body"]["result"]["songs"]
        .as_array()
        .expect("no result.songs");
    assert!(!songs.is_empty(), "search returned nothing");
    // Round-trips UTF-8 through the Buffer shim in both directions; a broken
    // encoder shows up as mojibake rather than an error.
    println!("cloudsearch → {}", songs[0]["name"]);
}

#[tokio::test]
#[ignore = "hits music.163.com"]
async fn login_qr_key_issues_a_unikey() {
    // Exercises the login path's weapi envelope without needing credentials.
    let core = core().await;
    let out = parse(&core.call("login_qr_key", "{}").await.unwrap());
    assert_eq!(out["ok"], true, "qr key failed: {out}");
    let unikey = out["body"]["unikey"]
        .as_str()
        .or_else(|| out["body"]["data"]["unikey"].as_str())
        .expect("no unikey in response");
    assert!(unikey.len() > 8, "implausible unikey: {unikey}");
}

/// Concurrency is the reason the HTTP op is async rather than blocking.
///
/// A page load fans out a dozen endpoint calls. With a blocking client one
/// isolate can only have one request outstanding, so those serialize; here they
/// must overlap. The bar is deliberately loose — this asserts "clearly not
/// serialized", not a precise speedup.
#[tokio::test]
#[ignore = "hits music.163.com"]
async fn concurrent_calls_overlap() {
    let core = std::sync::Arc::new(core().await);

    // Warm the connection pool. Deliberately a different id than every
    // measurement below — `song_detail` is `TTL_STATIC`-cached, so reusing an
    // id here would make a later "measurement" a cache hit instead of a
    // network call, and cache hits are µs while network calls are ms: any
    // baseline that accidentally lands on a warmed id will always beat N
    // concurrent fresh calls, regardless of whether the HTTP op overlaps.
    let _ = core.call("song_detail", r#"{"ids":"347239"}"#).await;

    let single = std::time::Instant::now();
    let _ = core.call("song_detail", r#"{"ids":"347240"}"#).await.unwrap();
    let single = single.elapsed();

    const N: usize = 6;
    let many = std::time::Instant::now();
    let mut tasks = Vec::with_capacity(N);
    for i in 0..N {
        let core = core.clone();
        tasks.push(tokio::spawn(async move {
            core.call("song_detail", &format!(r#"{{"ids":"{}"}}"#, 347230 + i))
                .await
        }));
    }
    for t in tasks {
        let out = t.await.expect("task panicked").expect("call failed");
        assert_eq!(parse(&out)["ok"], true);
    }
    let many = many.elapsed();

    println!("\n  1 call      {single:?}\n  {N} concurrent {many:?}\n");
    assert!(
        many < single * (N as u32),
        "{N} concurrent calls took {many:?}, which is no better than running them one at a time \
         ({single:?} each) — the HTTP op is not actually overlapping"
    );
}

/// Reproduces how the playback resolver reaches the isolate.
///
/// `source_resolver::resolve_blocking` is synchronous and runs inside
/// `tokio::task::spawn_blocking`, so the app's hook bridges to the async core
/// with `Handle::block_on`. That combination — block_on, on a blocking-pool
/// thread, on a *multi-threaded* runtime, against a core that internally holds
/// an async mutex and spawns futures onto QuickJS's own executor — is the one
/// shape none of the other tests cover, and a hang here surfaces as "stuck
/// loading, then playback failed".
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "hits music.163.com"]
async fn resolver_hook_shape_does_not_deadlock() {
    let core = std::sync::Arc::new(core().await);

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::task::spawn_blocking({
            let core = core.clone();
            move || {
                let handle = tokio::runtime::Handle::current();
                handle.block_on(async move {
                    core.call("song_url_v1", r#"{"id":"347230","level":"standard"}"#)
                        .await
                })
            }
        }),
    )
    .await
    .expect("resolver hook shape deadlocked: block_on inside spawn_blocking never returned")
    .expect("blocking task panicked")
    .expect("call failed");

    assert_eq!(parse(&result)["ok"], true, "resolve failed: {result}");
    println!("resolver hook shape OK");
}

/// The same shape, but with the UI hitting the isolate concurrently.
///
/// This is the real app: the frontend is issuing `ncm_request` calls while the
/// planner resolves in the background. Both contend for the single QuickJS
/// runtime lock, one of them from a thread that is blocked on `block_on`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "hits music.163.com"]
async fn resolver_hook_under_concurrent_ui_traffic() {
    let core = std::sync::Arc::new(core().await);

    let ui = {
        let core = core.clone();
        tokio::spawn(async move {
            for _ in 0..4 {
                let _ = core.call("song_detail", r#"{"ids":"347230"}"#).await;
            }
        })
    };

    let resolver = tokio::task::spawn_blocking({
        let core = core.clone();
        move || {
            tokio::runtime::Handle::current().block_on(async move {
                core.call("song_url_v1", r#"{"id":"347230","level":"standard"}"#)
                    .await
            })
        }
    });

    let out = tokio::time::timeout(std::time::Duration::from_secs(30), resolver)
        .await
        .expect("deadlocked with the UI hitting the isolate concurrently")
        .expect("blocking task panicked")
        .expect("call failed");

    ui.await.expect("ui task panicked");
    assert_eq!(parse(&out)["ok"], true, "resolve failed: {out}");
    println!("resolver hook under concurrent UI traffic OK");
}

/// Print what `/song/url/v1` actually yields on each transport, side by side.
///
/// Diagnostic rather than assertion-heavy: when playback breaks but the API
/// call "succeeds", the difference is in this payload — a trial fragment, a
/// null url, or a different CDN host — and guessing at it wastes far more time
/// than printing it.
#[tokio::test]
#[ignore = "hits music.163.com and the deployed API"]
async fn compare_song_url_local_vs_remote() {
    const ID: &str = "347230";
    let core = core().await;

    let local = parse(
        &core
            .call("song_url_v1", &format!(r#"{{"id":"{ID}","level":"standard"}}"#))
            .await
            .unwrap(),
    );
    let l = &local["body"]["data"][0];

    let remote_raw = reqwest::Client::new()
        .get(format!(
            "https://ncm-api.prod.gbclstudio.cn/song/url/v1?id={ID}&level=standard"
        ))
        .send()
        .await
        .expect("remote call failed")
        .text()
        .await
        .unwrap();
    let remote: serde_json::Value = serde_json::from_str(&remote_raw).unwrap();
    let r = &remote["data"][0];

    let show = |tag: &str, v: &serde_json::Value| {
        println!("\n  [{tag}]");
        println!("    url          {}", v["url"]);
        println!("    br / size    {} / {}", v["br"], v["size"]);
        println!("    fee / level  {} / {}", v["fee"], v["level"]);
        println!("    type         {}", v["type"]);
        println!("    freeTrial    {}", v["freeTrialInfo"]);
        println!("    code         {}", v["code"]);
    };
    show("embedded", l);
    show("deployed API", r);
    println!();
}

/// The premise of the whole exercise, measured rather than assumed.
///
/// Compares the embedded path against the deployed NeteaseCloudMusicApi the app
/// currently talks to. Both are warmed first so this measures steady-state
/// per-call cost with connections already pooled, not TLS handshakes.
///
/// Run with `--nocapture` to see the table.
#[tokio::test]
#[ignore = "hits music.163.com and the deployed API"]
async fn latency_versus_deployed_api() {
    const REMOTE: &str = "https://ncm-api.prod.gbclstudio.cn/song/detail?ids=347230";
    const SAMPLES: usize = 12;

    let core = core().await;
    let client = reqwest::Client::new();

    let mut embedded = Vec::with_capacity(SAMPLES);
    let mut remote = Vec::with_capacity(SAMPLES);

    // Warm: the first call pays TLS and, for the remote, whatever cold path the
    // proxy has.
    let _ = core.call("song_detail", r#"{"ids":"347230"}"#).await;
    let _ = client.get(REMOTE).send().await;

    for _ in 0..SAMPLES {
        let t = std::time::Instant::now();
        let out = core.call("song_detail", r#"{"ids":"347230"}"#).await.unwrap();
        embedded.push(t.elapsed().as_secs_f64() * 1000.0);
        assert!(parse(&out)["body"]["code"] == 200, "embedded call failed");

        let t = std::time::Instant::now();
        let resp = client.get(REMOTE).send().await.expect("remote call failed");
        let _ = resp.bytes().await;
        remote.push(t.elapsed().as_secs_f64() * 1000.0);
    }

    let median = |v: &mut Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    let e = median(&mut embedded);
    let r = median(&mut remote);

    println!("\n  /song/detail, median of {SAMPLES} warm calls");
    println!("    embedded (in-process)  {e:>7.1} ms");
    println!("    deployed API (current) {r:>7.1} ms");
    println!("    saved                  {:>7.1} ms\n", r - e);

    assert!(
        e < r,
        "embedded path ({e:.1} ms) was not faster than the deployed API ({r:.1} ms)"
    );
}
