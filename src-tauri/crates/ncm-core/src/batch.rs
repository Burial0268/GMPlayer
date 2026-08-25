//! Merge concurrent calls that differ only by id into one request.
//!
//! The reason this exists is a measurement. The isolate costs ~12 µs per call
//! and a 1.5 MB JSON round trip through QuickJS costs ~7 ms; one warm request
//! to Netease costs ~80 ms. So essentially all of a call's latency is the round
//! trip, and the only way to be meaningfully faster is to make fewer of them.
//! [`crate::cache`] removes repeats *after* an answer lands; this removes them
//! while they are still in flight.
//!
//! The shape it is for is an N+1: `SmallSongData.vue` asks for one
//! `song_detail` per card, so a list of twenty renders twenty calls in the same
//! tick. [`crate::throttle`] lets six of those run at once, so they cost four
//! round trips — ~320 ms — where one batched request costs ~80 ms. Upstream's
//! `song_detail` already takes a comma-separated `ids` and answers with a
//! `songs` array, so the merge is native to the endpoint rather than something
//! we synthesize.
//!
//! ## Why a window rather than queueing behind the in-flight request
//!
//! The obvious design — send the first call immediately, collect everything
//! that arrives while it is outstanding, send that as a second batch — adds no
//! latency to a solitary call. It is also *slower* than doing nothing for any
//! burst that fits inside the concurrency ceiling: six parallel calls cost one
//! round trip, while first-then-the-rest costs two.
//!
//! So arrivals are collected for [`WINDOW`] instead. A burst from a list render
//! is enqueued in a single JS tick and crosses IPC within a few hundred
//! microseconds, so a few milliseconds catches all of it, and a call that finds
//! no company pays only the window — ~6% of the round trip it was going to make
//! anyway.
//!
//! ## What may be merged
//!
//! An allowlist keyed on the endpoint, like the cache's, and for the same
//! reason: whether two calls can share a request is a property of the upstream
//! module, not something to infer. Each entry names the query key holding the
//! ids and the response arrays to split back out, and both were verified
//! against the live service — a batched `song_url_v1` entry carries the same
//! fields, bitrate, size, fee and playable URL as the same id fetched alone.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Map, Value};
use tokio::sync::oneshot;

/// How long a group collects arrivals before it dispatches.
///
/// Charged to every batchable call, so it is sized against the ~80 ms round
/// trip it exists to avoid rather than against the burst it wants to catch: a
/// burst arrives in well under a millisecond, and anything longer here is
/// latency paid by calls that turn out to be alone.
const WINDOW: Duration = Duration::from_millis(5);

/// How an endpoint's calls merge and how its answer comes apart again.
pub(crate) struct BatchSpec {
    /// Query key holding the id list. Upstream reads it as a comma-separated
    /// string in every case here.
    id_param: &'static str,
    /// Arrays inside `body` that carry one entry per requested id, and the
    /// field within an entry that says which id it is for.
    arrays: &'static [(&'static str, &'static str)],
    /// Ids per outbound request. A caller that already brings this many skips
    /// the batcher outright — it has nothing to gain and would only pay the
    /// window.
    max_ids: usize,
    /// Query keys that make a call unmergeable when present at all.
    blockers: &'static [&'static str],
}

/// `query.ids` is split on commas and mapped into a `c` array, so the id list
/// is the endpoint's own interface. `privileges` comes back alongside `songs`
/// and is keyed the same way.
static SONG_DETAIL: BatchSpec = BatchSpec {
    id_param: "ids",
    arrays: &[("songs", "id"), ("privileges", "id")],
    // Upstream's own comment caps this at 1000; the app's `listenTogether`
    // path already chunks at 100, so match that rather than inventing a
    // second number.
    max_ids: 100,
    blockers: &[],
};

/// `query.id` is interpolated straight into `ids: '[' + query.id + ']'`, so a
/// comma-separated list is passed through as a JSON array unchanged.
///
/// `unblock` blocks the merge because it is not the same request: that branch
/// resolves one id through `matchID` and synthesizes its own single-entry
/// response without ever reaching Netease.
static SONG_URL_V1: BatchSpec = BatchSpec {
    id_param: "id",
    arrays: &[("data", "id")],
    // Conservative. This is the playback critical path and the real batch is
    // the planner's prefill window — a handful of tracks, never dozens.
    max_ids: 10,
    blockers: &["unblock"],
};

fn spec_for(endpoint: &str) -> Option<&'static BatchSpec> {
    match endpoint {
        "song_detail" => Some(&SONG_DETAIL),
        "song_url_v1" => Some(&SONG_URL_V1),
        _ => None,
    }
}

/// A call the batcher can represent, and everything needed to reissue it for a
/// different set of ids.
pub(crate) struct Plan {
    spec: &'static BatchSpec,
    /// Calls sharing this may share a request. Every query key except the id
    /// list takes part — `level` and `cookie` above all, because both change
    /// what the server answers.
    group: String,
    ids: Vec<String>,
    /// The original query minus the id list. Volatile keys are stripped from
    /// the *group* but kept here: the outbound request should look exactly
    /// like the one the caller asked for.
    template: Map<String, Value>,
}

/// Whether this call can be merged with others, and how.
///
/// `None` for anything not on the allowlist, anything carrying a blocker, and
/// anything whose id list will not parse — all of which fall through to being
/// sent on their own.
pub(crate) fn plan(endpoint: &str, query_json: &str) -> Option<Plan> {
    let spec = spec_for(endpoint)?;
    let Ok(Value::Object(mut map)) = serde_json::from_str::<Value>(query_json) else {
        return None;
    };
    if spec.blockers.iter().any(|k| map.contains_key(*k)) {
        return None;
    }

    let raw = map.remove(spec.id_param)?;
    let ids = parse_ids(&raw)?;
    if ids.is_empty() {
        return None;
    }

    let group = format!("{endpoint}\u{0}{}", crate::cache::canonical_map(&map));
    Some(Plan {
        spec,
        group,
        ids,
        template: map,
    })
}

/// Ids as the query spells them, accepting the number form as well.
///
/// `song.getUrl` passes `id` as a JS number and `song.getDetail` joins an array
/// into a string, so both forms reach here from the same app.
fn parse_ids(raw: &Value) -> Option<Vec<String>> {
    let text = match raw {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let ids: Vec<String> = text
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    // A non-numeric id is not something the merge can key on coming back, and
    // is more likely a caller doing something this table did not anticipate.
    ids.iter()
        .all(|id| id.bytes().all(|b| b.is_ascii_digit()))
        .then_some(ids)
}

impl Plan {
    /// The query for one outbound request covering `ids`.
    fn query_for(&self, ids: &[String]) -> String {
        let mut map = self.template.clone();
        map.insert(self.spec.id_param.to_owned(), Value::String(ids.join(",")));
        Value::Object(map).to_string()
    }
}

/// The envelope, as a string every waiter can be handed a clone of.
type Answer = Result<Arc<str>, String>;

struct Waiter {
    ids: Vec<String>,
    tx: oneshot::Sender<Answer>,
}

#[derive(Default)]
struct Group {
    queued: Vec<Waiter>,
    /// Whether someone is already sleeping out the window for this group. Only
    /// the leader dispatches; everyone else just waits to be answered.
    leader: bool,
}

pub(crate) struct Batcher {
    /// Keyed by [`Plan::group`], so the map holds one entry per distinct
    /// (endpoint, parameters-except-ids) in flight — a handful in practice,
    /// and emptied as each batch goes out.
    groups: Mutex<HashMap<String, Group>>,
    /// [`WINDOW`], except in tests. A few milliseconds is the right production
    /// value and a hopeless one to write a deterministic cancellation test
    /// against, so the tests below hand in something they can reason about
    /// instead of racing the clock.
    window: Duration,
}

impl Batcher {
    pub(crate) fn new() -> Self {
        Self::with_window(WINDOW)
    }

    fn with_window(window: Duration) -> Self {
        Self {
            groups: Mutex::new(HashMap::new()),
            window,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Group>> {
        // A poisoned lock means a panic elsewhere in the crate. The map is
        // coordination state, not data anyone can be wrong about; recovering it
        // beats turning one panic into every later call failing.
        self.groups.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Serve `plan`, merging it with anything else that shows up inside
    /// [`WINDOW`].
    ///
    /// `dispatch` sends one request and returns its envelope; it is called once
    /// per outbound batch, by whichever caller happened to open the window.
    pub(crate) async fn run<F, Fut>(&self, plan: &Plan, dispatch: F) -> Result<String, String>
    where
        F: Fn(String) -> Fut,
        Fut: Future<Output = Result<String, String>>,
    {
        // A caller that already fills a request gains nothing from waiting for
        // company, and the window would be pure cost.
        if plan.ids.len() >= plan.spec.max_ids {
            return dispatch(plan.query_for(&plan.ids)).await;
        }

        let (tx, rx) = oneshot::channel();
        let lead = {
            let mut groups = self.lock();
            let group = groups.entry(plan.group.clone()).or_default();
            group.queued.push(Waiter {
                ids: plan.ids.clone(),
                tx,
            });
            !std::mem::replace(&mut group.leader, true)
        };

        if lead {
            // The guard exists for cancellation: `ncm_request` drops this
            // future on its 25s timeout, and a leader that vanished while
            // holding the flag would leave the group with nobody to dispatch
            // it and no way to elect a replacement.
            let mut lead = Leadership {
                batcher: self,
                key: &plan.group,
                held: true,
            };
            tokio::time::sleep(self.window).await;
            let batch = lead.stand_down();
            self.dispatch(plan, batch, &dispatch).await;
        }

        match rx.await {
            Ok(answer) => answer.map(|body| body.to_string()),
            // The leader was dropped — cancelled, or timed out — before it
            // could answer. Saying so beats hanging until this caller's own
            // budget runs out.
            Err(_) => Err(
                "the batched request that would have answered this call was \
                           cancelled before it completed"
                    .to_owned(),
            ),
        }
    }

    /// Send `batch` as as few requests as it fits into, then hand each waiter
    /// the slice of the answer it asked for.
    async fn dispatch<F, Fut>(&self, plan: &Plan, batch: Vec<Waiter>, dispatch: &F)
    where
        F: Fn(String) -> Fut,
        Fut: Future<Output = Result<String, String>>,
    {
        if batch.is_empty() {
            return;
        }

        // Unique, in first-seen order: two cards for the same song are one id
        // on the wire, which is the in-flight half of what the cache does after
        // the fact.
        let mut ids: Vec<String> = Vec::new();
        for waiter in &batch {
            for id in &waiter.ids {
                if !ids.contains(id) {
                    ids.push(id.clone());
                }
            }
        }

        let mut merged = Merged::new(plan.spec);
        // Chunks are sequential. One only exists when enough callers each
        // brought several ids to overflow a request, which no current call site
        // does; parallelising a case that does not arise is not worth the
        // machinery.
        for chunk in ids.chunks(plan.spec.max_ids) {
            match dispatch(plan.query_for(chunk)).await {
                Ok(envelope) => {
                    if let Some(verbatim) = merged.absorb(plan.spec, &envelope) {
                        // Not an answer that can be split — an upstream error
                        // code, or a shape this table does not recognise. Hand
                        // it over untouched: callers act on in-band codes like
                        // 301, and losing the body would lose the reason.
                        answer_all(batch, Ok(verbatim));
                        return;
                    }
                }
                Err(e) => {
                    answer_all(batch, Err(e));
                    return;
                }
            }
        }

        for waiter in batch {
            let slice = merged.slice_for(plan.spec, &waiter.ids);
            // A receiver that has gone away is a caller that was cancelled,
            // which says nothing about the answer.
            let _ = waiter.tx.send(Ok(slice));
        }
    }
}

fn answer_all(batch: Vec<Waiter>, answer: Answer) {
    for waiter in batch {
        let _ = waiter.tx.send(answer.clone());
    }
}

/// The merged answer, indexed so a waiter's slice is a lookup rather than a
/// scan.
struct Merged {
    status: u64,
    cookie: Value,
    /// `body` with the split arrays taken out, from the first chunk. Every
    /// other field — `code` above all — is shared by every waiter.
    body: Map<String, Value>,
    /// One index per entry in [`BatchSpec::arrays`], id to entries.
    ///
    /// A `Vec` per id rather than a single value: the arrays here are one entry
    /// per id in practice, but silently dropping a second one would be a very
    /// quiet way to be wrong.
    indexes: Vec<HashMap<String, Vec<Value>>>,
    seen: bool,
}

impl Merged {
    fn new(spec: &BatchSpec) -> Self {
        Self {
            status: 200,
            cookie: Value::Array(Vec::new()),
            body: Map::new(),
            indexes: spec.arrays.iter().map(|_| HashMap::new()).collect(),
            seen: false,
        }
    }

    /// Fold one chunk's envelope in.
    ///
    /// Returns the envelope untouched when it cannot be split, which is the
    /// signal to hand it to every waiter verbatim.
    fn absorb(&mut self, spec: &BatchSpec, envelope: &str) -> Option<Arc<str>> {
        let verbatim = || Some(Arc::from(envelope));

        let Ok(Value::Object(mut parsed)) = serde_json::from_str::<Value>(envelope) else {
            return verbatim();
        };
        if parsed.get("ok") != Some(&Value::Bool(true)) {
            return verbatim();
        }
        let Some(Value::Object(mut body)) = parsed.remove("body") else {
            return verbatim();
        };
        // Every array this endpoint splits on has to actually be there. If one
        // is missing the response is not the shape this table describes, and
        // guessing at it would hand callers a silently empty result.
        if spec
            .arrays
            .iter()
            .any(|(name, _)| !matches!(body.get(*name), Some(Value::Array(_))))
        {
            return verbatim();
        }

        for (slot, (name, id_field)) in spec.arrays.iter().enumerate() {
            let Some(Value::Array(entries)) = body.remove(*name) else {
                continue;
            };
            for entry in entries {
                let Some(id) = entry.get(*id_field).and_then(id_of) else {
                    continue;
                };
                self.indexes[slot].entry(id).or_default().push(entry);
            }
        }

        if !self.seen {
            self.status = parsed.get("status").and_then(Value::as_u64).unwrap_or(200);
            self.cookie = parsed
                .remove("cookie")
                .unwrap_or_else(|| Value::Array(Vec::new()));
            self.body = body;
            self.seen = true;
        }
        None
    }

    /// The envelope for one waiter.
    ///
    /// Built by hand rather than through a `Map` because field order is
    /// load-bearing: `cache::is_storable` reads the envelope's *shape* to
    /// decide whether it may be remembered, and `serde_json::Map` is sorted, so
    /// a serialized struct would come out `{"body":…` and silently turn caching
    /// off for everything batched.
    fn slice_for(&self, spec: &BatchSpec, ids: &[String]) -> Arc<str> {
        let mut body = self.body.clone();
        for (slot, (name, _)) in spec.arrays.iter().enumerate() {
            let mut mine: Vec<Value> = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(entries) = self.indexes[slot].get(id) {
                    mine.extend(entries.iter().cloned());
                }
            }
            body.insert((*name).to_owned(), Value::Array(mine));
        }

        Arc::from(format!(
            r#"{{"ok":true,"status":{},"body":{},"cookie":{}}}"#,
            self.status,
            Value::Object(body),
            self.cookie,
        ))
    }
}

/// An entry's id as a string, however the response spells it.
///
/// Netease answers with numbers where the query carried strings, so both are
/// normalized to the decimal text the query used.
fn id_of(v: &Value) -> Option<String> {
    match v {
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// Holds the dispatch flag for a group and releases it however the leader
/// leaves — normally through [`Leadership::stand_down`], or by being dropped
/// when the caller is cancelled.
struct Leadership<'a> {
    batcher: &'a Batcher,
    key: &'a str,
    held: bool,
}

impl Leadership<'_> {
    /// Hand back the flag and take everything the window collected.
    fn stand_down(&mut self) -> Vec<Waiter> {
        self.held = false;
        self.release()
    }

    fn release(&self) -> Vec<Waiter> {
        let mut groups = self.batcher.lock();
        let Some(group) = groups.get_mut(self.key) else {
            return Vec::new();
        };
        group.leader = false;
        let batch = std::mem::take(&mut group.queued);
        // Nothing queued and nobody leading: the group has no state left worth
        // a map entry, and leaving them would grow one per distinct parameter
        // set for the life of the process.
        groups.remove(self.key);
        batch
    }
}

impl Drop for Leadership<'_> {
    fn drop(&mut self) {
        if !self.held {
            return;
        }
        // Dropping the waiters drops their senders, so anyone queued behind a
        // cancelled leader learns about it instead of waiting out their budget.
        let _ = self.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(body: &str) -> String {
        format!(r#"{{"ok":true,"status":200,"body":{body},"cookie":[]}}"#)
    }

    #[test]
    fn only_allowlisted_endpoints_merge() {
        assert!(plan("song_detail", r#"{"ids":"1,2"}"#).is_some());
        assert!(plan("song_url_v1", r#"{"id":1,"level":"exhigh"}"#).is_some());
        for endpoint in ["lyric_new", "playlist_detail", "like", "scrobble"] {
            assert!(plan(endpoint, r#"{"id":1}"#).is_none(), "{endpoint}");
        }
    }

    /// The unblock branch never reaches Netease and answers for exactly one id,
    /// so merging it would hand the other callers someone else's URL.
    #[test]
    fn a_blocker_takes_the_call_off_the_batched_path() {
        assert!(plan("song_url_v1", r#"{"id":1,"unblock":"true"}"#).is_none());
        assert!(plan("song_url_v1", r#"{"id":1,"unblock":"false"}"#).is_none());
    }

    /// Anything that changes what the server answers has to split the group, or
    /// a lossless request would be served an `exhigh` URL.
    #[test]
    fn parameters_other_than_the_ids_split_the_group() {
        let a = plan("song_url_v1", r#"{"id":1,"level":"exhigh"}"#).unwrap();
        let b = plan("song_url_v1", r#"{"id":2,"level":"exhigh"}"#).unwrap();
        let c = plan("song_url_v1", r#"{"id":3,"level":"lossless"}"#).unwrap();
        let d = plan(
            "song_url_v1",
            r#"{"id":4,"level":"exhigh","cookie":"MUSIC_U=other"}"#,
        )
        .unwrap();

        assert_eq!(a.group, b.group);
        assert_ne!(a.group, c.group);
        assert_ne!(a.group, d.group, "two accounts must not share a request");
    }

    /// The cache-buster is on 56 call sites and would otherwise put every call
    /// in a group of its own, which is exactly no batching at all.
    #[test]
    fn the_timestamp_cache_buster_does_not_split_the_group() {
        let a = plan("song_detail", r#"{"ids":"1","timestamp":1}"#).unwrap();
        let b = plan("song_detail", r#"{"ids":"2","timestamp":99999}"#).unwrap();
        assert_eq!(a.group, b.group);
        // ...but it still goes out on the wire, because the request should look
        // like the one the caller asked for.
        assert!(a.query_for(&["1".into()]).contains("timestamp"));
    }

    #[test]
    fn ids_are_read_in_both_the_string_and_number_forms() {
        assert_eq!(
            plan("song_detail", r#"{"ids":"1, 2 ,3"}"#).unwrap().ids,
            ["1", "2", "3"]
        );
        assert_eq!(
            plan("song_url_v1", r#"{"id":347230}"#).unwrap().ids,
            ["347230"]
        );
        assert!(plan("song_detail", r#"{"ids":""}"#).is_none());
        assert!(plan("song_detail", r#"{"ids":["1"]}"#).is_none());
        // A non-numeric id cannot be matched back to a response entry.
        assert!(plan("song_detail", r#"{"ids":"1,abc"}"#).is_none());
    }

    #[test]
    fn the_outbound_query_carries_the_merged_id_list() {
        let plan = plan("song_url_v1", r#"{"id":1,"level":"exhigh"}"#).unwrap();
        let q: Value =
            serde_json::from_str(&plan.query_for(&["1".into(), "2".into(), "3".into()])).unwrap();
        assert_eq!(q["id"], "1,2,3");
        assert_eq!(q["level"], "exhigh");
    }

    /// The property the whole module rests on: a waiter gets exactly what it
    /// would have got asking on its own, and nothing belonging to anyone else.
    #[test]
    fn a_waiter_gets_only_its_own_entries() {
        let mut merged = Merged::new(&SONG_DETAIL);
        assert!(merged
            .absorb(
                &SONG_DETAIL,
                &envelope(
                    r#"{"code":200,"songs":[{"id":1,"name":"a"},{"id":2,"name":"b"},{"id":3,"name":"c"}],"privileges":[{"id":1,"fee":8},{"id":2,"fee":0},{"id":3,"fee":8}]}"#
                )
            )
            .is_none());

        let slice: Value =
            serde_json::from_str(&merged.slice_for(&SONG_DETAIL, &["2".into()])).unwrap();
        assert_eq!(slice["ok"], true);
        assert_eq!(slice["status"], 200);
        // Fields that are not per-id are shared, not dropped.
        assert_eq!(slice["body"]["code"], 200);
        assert_eq!(slice["body"]["songs"].as_array().unwrap().len(), 1);
        assert_eq!(slice["body"]["songs"][0]["name"], "b");
        assert_eq!(slice["body"]["privileges"].as_array().unwrap().len(), 1);
        assert_eq!(slice["body"]["privileges"][0]["fee"], 0);
    }

    /// An id the server did not answer for comes back as an empty array, which
    /// is what a single-id call for a nonexistent song returns anyway. Half the
    /// ids in a 50-id `song_detail` batch are routinely missing.
    #[test]
    fn an_id_the_server_skipped_yields_an_empty_array() {
        let mut merged = Merged::new(&SONG_DETAIL);
        merged.absorb(
            &SONG_DETAIL,
            &envelope(r#"{"code":200,"songs":[{"id":1}],"privileges":[{"id":1}]}"#),
        );
        let slice: Value =
            serde_json::from_str(&merged.slice_for(&SONG_DETAIL, &["999".into()])).unwrap();
        assert_eq!(slice["body"]["songs"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn chunks_accumulate_into_one_answer() {
        let mut merged = Merged::new(&SONG_URL_V1);
        merged.absorb(
            &SONG_URL_V1,
            &envelope(r#"{"code":200,"data":[{"id":1,"br":1}]}"#),
        );
        merged.absorb(
            &SONG_URL_V1,
            &envelope(r#"{"code":200,"data":[{"id":2,"br":2}]}"#),
        );

        let slice: Value =
            serde_json::from_str(&merged.slice_for(&SONG_URL_V1, &["2".into(), "1".into()]))
                .unwrap();
        let data = slice["body"]["data"].as_array().unwrap();
        assert_eq!(data.len(), 2);
        // In the order the waiter asked for them.
        assert_eq!(data[0]["br"], 2);
        assert_eq!(data[1]["br"], 1);
    }

    /// The batched envelope has to keep the exact field order the cache reads,
    /// or every batched answer would silently stop being cacheable.
    #[test]
    fn a_batched_envelope_is_still_storable() {
        let mut merged = Merged::new(&SONG_DETAIL);
        merged.absorb(
            &SONG_DETAIL,
            &envelope(r#"{"code":200,"songs":[{"id":1}],"privileges":[{"id":1}]}"#),
        );
        let slice = merged.slice_for(&SONG_DETAIL, &["1".into()]);
        assert!(slice.starts_with(r#"{"ok":true"#), "{slice}");
        assert!(slice.ends_with(r#""cookie":[]}"#), "{slice}");
    }

    /// An upstream error is handed over untouched: callers act on in-band codes
    /// like 301, and a split would lose the body that explains them.
    #[test]
    fn an_unsplittable_answer_is_passed_through_verbatim() {
        let failure =
            r#"{"ok":false,"status":301,"body":{"code":301,"msg":"需要登录"},"cookie":[]}"#;
        let mut merged = Merged::new(&SONG_DETAIL);
        assert_eq!(
            merged.absorb(&SONG_DETAIL, failure).as_deref(),
            Some(failure)
        );

        // ...and so is a success whose shape this table does not describe,
        // rather than being turned into an empty result.
        let mut merged = Merged::new(&SONG_DETAIL);
        assert!(merged
            .absorb(&SONG_DETAIL, &envelope(r#"{"code":200}"#))
            .is_some());
    }

    // ── coordination ─────────────────────────────────────────────

    /// Concurrent single-id calls have to become one request, which is the
    /// entire point.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_burst_becomes_one_request() {
        let batcher = Arc::new(Batcher::new());
        let sent = Arc::new(Mutex::new(Vec::<String>::new()));

        let mut tasks = Vec::new();
        for id in 1..=20u32 {
            let batcher = batcher.clone();
            let sent = sent.clone();
            tasks.push(tokio::spawn(async move {
                let plan = plan("song_detail", &format!(r#"{{"ids":"{id}"}}"#)).unwrap();
                batcher
                    .run(&plan, |query| {
                        let sent = sent.clone();
                        async move {
                            sent.lock().unwrap().push(query.clone());
                            let q: Value = serde_json::from_str(&query).unwrap();
                            let songs: Vec<String> = q["ids"]
                                .as_str()
                                .unwrap()
                                .split(',')
                                .map(|i| format!(r#"{{"id":{i}}}"#))
                                .collect();
                            Ok(envelope(&format!(
                                r#"{{"code":200,"songs":[{0}],"privileges":[{0}]}}"#,
                                songs.join(",")
                            )))
                        }
                    })
                    .await
                    .map(|out| (id, out))
            }));
        }

        for task in tasks {
            let (id, out) = task.await.unwrap().unwrap();
            let v: Value = serde_json::from_str(&out).unwrap();
            let songs = v["body"]["songs"].as_array().unwrap();
            assert_eq!(songs.len(), 1, "id {id} got {} songs", songs.len());
            assert_eq!(songs[0]["id"], id, "id {id} was handed someone else's song");
        }

        let sent = sent.lock().unwrap();
        assert_eq!(
            sent.len(),
            1,
            "20 calls made {} requests: {sent:?}",
            sent.len()
        );
    }

    /// Two callers asking for the same id are one id on the wire.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn duplicate_ids_are_requested_once() {
        let batcher = Arc::new(Batcher::new());
        let sent = Arc::new(Mutex::new(Vec::<String>::new()));

        let mut tasks = Vec::new();
        for _ in 0..5 {
            let batcher = batcher.clone();
            let sent = sent.clone();
            tasks.push(tokio::spawn(async move {
                let plan = plan("song_detail", r#"{"ids":"7"}"#).unwrap();
                batcher
                    .run(&plan, |query| {
                        let sent = sent.clone();
                        async move {
                            sent.lock().unwrap().push(query);
                            Ok(envelope(
                                r#"{"code":200,"songs":[{"id":7}],"privileges":[{"id":7}]}"#,
                            ))
                        }
                    })
                    .await
            }));
        }
        for task in tasks {
            let out = task.await.unwrap().unwrap();
            let v: Value = serde_json::from_str(&out).unwrap();
            assert_eq!(v["body"]["songs"][0]["id"], 7);
        }

        let sent = sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        let q: Value = serde_json::from_str(&sent[0]).unwrap();
        assert_eq!(q["ids"], "7", "the same id was asked for more than once");
    }

    /// A caller that already fills a request must not pay the window.
    #[tokio::test]
    async fn a_full_request_skips_the_window() {
        let batcher = Batcher::new();
        let ids: Vec<String> = (0..SONG_DETAIL.max_ids).map(|i| i.to_string()).collect();
        let plan = plan("song_detail", &format!(r#"{{"ids":"{}"}}"#, ids.join(","))).unwrap();

        let start = std::time::Instant::now();
        let out = batcher
            .run(&plan, |_| async { Ok(envelope(r#"{"code":200}"#)) })
            .await
            .unwrap();
        assert!(start.elapsed() < WINDOW, "waited {:?}", start.elapsed());
        // Straight through, so the envelope is whatever the endpoint said.
        assert_eq!(out, envelope(r#"{"code":200}"#));
    }

    /// A failed batch fails everyone in it, with the reason intact rather than
    /// a timeout.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_failed_batch_reports_to_every_waiter() {
        let batcher = Arc::new(Batcher::new());
        let mut tasks = Vec::new();
        for id in 1..=4u32 {
            let batcher = batcher.clone();
            tasks.push(tokio::spawn(async move {
                let plan = plan("song_detail", &format!(r#"{{"ids":"{id}"}}"#)).unwrap();
                batcher
                    .run(&plan, |_| async {
                        Err("transport error (reset)".to_owned())
                    })
                    .await
            }));
        }
        for task in tasks {
            assert_eq!(task.await.unwrap().unwrap_err(), "transport error (reset)");
        }
    }

    /// A leader that is cancelled mid-window must not wedge the group: the
    /// waiters behind it learn about it, and the next call can lead.
    ///
    /// A long window rather than the production one, so "both callers have
    /// joined and neither has dispatched" is a state this can actually be in
    /// rather than a race it has to win.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_cancelled_leader_does_not_wedge_the_group() {
        const WIDE: Duration = Duration::from_millis(500);
        let batcher = Arc::new(Batcher::with_window(WIDE));

        let leader = {
            let batcher = batcher.clone();
            tokio::spawn(async move {
                let plan = plan("song_detail", r#"{"ids":"1"}"#).unwrap();
                batcher
                    .run(&plan, |_| async { Ok(envelope(r#"{"code":200}"#)) })
                    .await
            })
        };
        // Both tasks are inside the window with plenty of it left.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let follower = {
            let batcher = batcher.clone();
            tokio::spawn(async move {
                let plan = plan("song_detail", r#"{"ids":"2"}"#).unwrap();
                batcher
                    .run(&plan, |_| async { Ok(envelope(r#"{"code":200}"#)) })
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            batcher
                .lock()
                .get(&plan("song_detail", r#"{"ids":"1"}"#).unwrap().group)
                .map(|g| g.queued.len()),
            Some(2),
            "both callers should be queued before the leader is cancelled"
        );
        leader.abort();

        // The follower is told, rather than waiting out its caller's budget.
        assert!(follower.await.unwrap().is_err());

        // ...and the group is usable again.
        let plan = plan("song_detail", r#"{"ids":"3"}"#).unwrap();
        let out = batcher
            .run(&plan, |_| async {
                Ok(envelope(
                    r#"{"code":200,"songs":[{"id":3}],"privileges":[{"id":3}]}"#,
                ))
            })
            .await
            .unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["body"]["songs"][0]["id"], 3);
        assert!(batcher.lock().is_empty(), "the group outlived its batch");
    }

    /// The map must not accumulate an entry per parameter set for the life of
    /// the process.
    #[tokio::test]
    async fn groups_do_not_outlive_their_batch() {
        let batcher = Batcher::new();
        for id in 0..10u32 {
            let plan = plan("song_url_v1", &format!(r#"{{"id":{id},"level":"l{id}"}}"#)).unwrap();
            let _ = batcher
                .run(&plan, |_| async {
                    Ok(envelope(r#"{"code":200,"data":[]}"#))
                })
                .await;
        }
        assert!(batcher.lock().is_empty());
    }
}
