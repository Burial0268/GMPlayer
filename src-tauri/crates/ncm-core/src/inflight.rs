//! Collapse identical calls that are already in flight.
//!
//! [`crate::cache`] removes a repeat once an answer has landed; between the
//! request going out and the response arriving it does nothing, and that gap is
//! the whole ~80 ms a call costs. Anything that fans out inside it pays twice
//! for one answer.
//!
//! Which happens constantly. The player and the desktop lyric window both ask
//! for the current track's `lyric_new` when it changes; a view that remounts
//! reissues the `playlist_detail` its previous instance was still waiting on; a
//! search box re-sends a suggestion as the user keeps typing. None of those are
//! bugs at the call site — they are independent components each asking for what
//! they need — so the fix belongs here.
//!
//! ## Scope
//!
//! Gated on the cache's allowlist, which is the same question asked once: an
//! endpoint whose answer may be *remembered* is read-only and idempotent, so
//! two callers may certainly share one request. Nothing is coalesced that the
//! cache would not have stored, so `like`, `scrobble` and every login path go
//! out on their own as before. The key is the cache key, so it already carries
//! the cookie and two accounts never share a request.
//!
//! [`crate::batch`] handles the endpoints where two calls differ only by id and
//! can be *merged* rather than shared; this is the narrower case where they are
//! the same call outright.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use tokio::sync::oneshot;

type Answer = Result<Arc<str>, String>;

pub(crate) struct Inflight {
    /// One entry per request outstanding right now, holding whoever arrived
    /// after it started. Removed as each request completes, so this is sized by
    /// concurrency rather than by traffic.
    calls: Mutex<HashMap<String, Vec<oneshot::Sender<Answer>>>>,
}

impl Inflight {
    pub(crate) fn new() -> Self {
        Self {
            calls: Mutex::new(HashMap::new()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Vec<oneshot::Sender<Answer>>>> {
        // A poisoned lock means a panic elsewhere in the crate. This map is
        // coordination state; recovering it beats turning one panic into every
        // later call failing.
        self.calls.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Run `dispatch` unless an identical call is already running, in which case
    /// wait for that one's answer.
    ///
    /// The second element of the return says whether this caller is the one that
    /// actually made the request. Callers use it to store the answer once rather
    /// than once per waiter — a shared `playlist_track_all` reaches megabytes,
    /// and re-inserting it per waiter would copy it that many times for no
    /// effect.
    pub(crate) async fn run<F, Fut>(&self, key: &str, dispatch: F) -> Result<(String, bool), String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<String, String>>,
    {
        let mut rx = {
            let mut calls = self.lock();
            match calls.get_mut(key) {
                Some(waiters) => {
                    let (tx, rx) = oneshot::channel();
                    waiters.push(tx);
                    Some(rx)
                }
                None => {
                    calls.insert(key.to_owned(), Vec::new());
                    None
                }
            }
        };

        if let Some(rx) = rx.take() {
            return match rx.await {
                Ok(answer) => answer.map(|body| (body.to_string(), false)),
                // The caller that owned the request was cancelled — its own
                // budget ran out, or the window it belonged to went away.
                // Saying so beats waiting out this caller's budget too.
                Err(_) => Err(
                    "the request that would have answered this call was cancelled \
                               before it completed"
                        .to_owned(),
                ),
            };
        }

        // Registered as the owner, so the entry has to come back out however
        // this returns — including by being dropped on `ncm_request`'s timeout,
        // which would otherwise leave a key nothing will ever answer and
        // deadlock every later caller for it.
        let mut owned = Owned {
            inflight: self,
            key,
            held: true,
        };
        let result = dispatch().await;
        let shared: Answer = match &result {
            Ok(body) => Ok(Arc::from(body.as_str())),
            Err(e) => Err(e.clone()),
        };
        for tx in owned.finish() {
            let _ = tx.send(shared.clone());
        }
        result.map(|body| (body, true))
    }
}

/// Holds the in-flight entry for one key and gives it back however the owner
/// leaves.
struct Owned<'a> {
    inflight: &'a Inflight,
    key: &'a str,
    held: bool,
}

impl Owned<'_> {
    fn finish(&mut self) -> Vec<oneshot::Sender<Answer>> {
        self.held = false;
        self.inflight.lock().remove(self.key).unwrap_or_default()
    }
}

impl Drop for Owned<'_> {
    fn drop(&mut self) {
        if !self.held {
            return;
        }
        // Dropping the senders is what tells anyone queued behind a cancelled
        // owner to stop waiting.
        let _ = self.inflight.lock().remove(self.key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    /// The property this exists for.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn identical_concurrent_calls_make_one_request() {
        let inflight = Arc::new(Inflight::new());
        let calls = Arc::new(AtomicUsize::new(0));

        let mut tasks = Vec::new();
        for _ in 0..8 {
            let inflight = inflight.clone();
            let calls = calls.clone();
            tasks.push(tokio::spawn(async move {
                inflight
                    .run("lyric_new\u{0}id=1", || async {
                        calls.fetch_add(1, Ordering::SeqCst);
                        // Long enough that the others certainly arrive while
                        // this is outstanding.
                        tokio::time::sleep(Duration::from_millis(30)).await;
                        Ok("the answer".to_owned())
                    })
                    .await
            }));
        }

        let mut leaders = 0;
        for task in tasks {
            let (body, led) = task.await.unwrap().unwrap();
            assert_eq!(body, "the answer");
            if led {
                leaders += 1;
            }
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(leaders, 1, "exactly one caller should report having led");
    }

    /// Different keys must not share, or one account would be handed another's
    /// answer.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn different_keys_do_not_share() {
        let inflight = Arc::new(Inflight::new());
        let calls = Arc::new(AtomicUsize::new(0));

        let mut tasks = Vec::new();
        for key in ["a", "b", "c"] {
            let inflight = inflight.clone();
            let calls = calls.clone();
            tasks.push(tokio::spawn(async move {
                inflight
                    .run(key, || async {
                        calls.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        Ok(key.to_owned())
                    })
                    .await
            }));
        }
        for task in tasks {
            let _ = task.await.unwrap().unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    /// A failure is shared as faithfully as a success — with the reason, not a
    /// timeout.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_failure_reaches_every_waiter() {
        let inflight = Arc::new(Inflight::new());
        let mut tasks = Vec::new();
        for _ in 0..4 {
            let inflight = inflight.clone();
            tasks.push(tokio::spawn(async move {
                inflight
                    .run("k", || async {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        Err("transport error (reset) via music.163.com".to_owned())
                    })
                    .await
            }));
        }
        for task in tasks {
            assert_eq!(
                task.await.unwrap().unwrap_err(),
                "transport error (reset) via music.163.com"
            );
        }
        // ...and the key is free again.
        assert!(inflight.lock().is_empty());
    }

    /// The failure mode that makes the drop guard load-bearing: without it a
    /// cancelled owner leaves a key nothing will ever answer, and every later
    /// caller for it waits out its own budget.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_cancelled_owner_frees_the_key() {
        let inflight = Arc::new(Inflight::new());

        let owner = {
            let inflight = inflight.clone();
            tokio::spawn(async move {
                inflight
                    .run("k", || async {
                        tokio::time::sleep(Duration::from_secs(60)).await;
                        Ok("never".to_owned())
                    })
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;

        let follower = {
            let inflight = inflight.clone();
            tokio::spawn(async move { inflight.run("k", || async { Ok("late".to_owned()) }).await })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        owner.abort();

        assert!(
            follower.await.unwrap().is_err(),
            "the follower was not told"
        );
        assert!(inflight.lock().is_empty(), "the key outlived its owner");

        // And the key works again rather than being poisoned for the session.
        let (body, led) = inflight
            .run("k", || async { Ok("fresh".to_owned()) })
            .await
            .unwrap();
        assert_eq!((body.as_str(), led), ("fresh", true));
    }

    /// A call that finds nothing in flight pays nothing: no sleep, no channel,
    /// straight through.
    #[tokio::test]
    async fn a_solitary_call_is_not_delayed() {
        let inflight = Inflight::new();
        let start = std::time::Instant::now();
        let (body, led) = inflight
            .run("k", || async { Ok("answer".to_owned()) })
            .await
            .unwrap();
        assert!(start.elapsed() < Duration::from_millis(5));
        assert_eq!((body.as_str(), led), ("answer", true));
        assert!(inflight.lock().is_empty());
    }
}
