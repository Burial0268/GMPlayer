//! Response cache for read-only endpoints.
//!
//! The reason this exists is a measurement. A warm request to Netease costs
//! ~80 ms and a connection that has to be established first costs ~190 ms, while
//! the isolate around it costs ~12 µs. So a call *is* its round trip, and the
//! only lever that matters is not making one.
//!
//! Two tiers, and the second is what makes a cold start fast:
//!
//! * **Memory.** Process-wide, so every window shares it and a WebView reload
//!   does not throw it away.
//! * **Disk** ([`crate::disk`]). Survives the process. Without it every launch
//!   re-fetches the same home page and the same lyrics over a connection it has
//!   to open first — which is the entire reason a cold start felt slow, and is
//!   avoidable because a song's title has not changed since the app closed.
//!
//! ## Stale-while-revalidate
//!
//! An entry has two bounds: `expires`, after which it is no longer *fresh*, and
//! a longer stale window during which it may still be *served* while a refresh
//! runs behind it. That is the difference between a 5-minute-old playlist page
//! costing 80 ms and costing nothing, and it is why re-opening a view after a
//! long idle stretch is instant instead of paying a reconnect.
//!
//! Only classes where a slightly old answer is harmless get a stale window; see
//! [`Policy`]. Nothing that the app can change from another screen does, because
//! there the correct answer is the current one.
//!
//! ## What is cacheable
//!
//! An allowlist, not a heuristic. The bundle exposes 439 endpoints and most of
//! them mutate something; a cache that guesses would eventually serve a stale
//! like state or replay a login. [`policy_for`] names every endpoint we have
//! actually reasoned about and nothing else is stored.
//!
//! Three further conditions apply on the way in, and each one is a failure mode
//! that was cheap to close:
//!
//! * **Only `ok: true`.** An error is not an answer worth remembering, and a
//!   transient 502 would otherwise stick for the whole TTL.
//! * **Cookies are stripped, not tolerated.** What gets stored always has an
//!   empty cookie array, so a `Set-Cookie` can never be replayed to resurrect a
//!   session the app has moved on from — and an envelope trying to set something
//!   credential-shaped is refused outright. This used to *refuse* any response
//!   that set a cookie, which silently disabled most of the cache: every eapi
//!   response carries a ten-year tracking cookie. See [`storable_form`].
//! * **Keyed including the cookie.** Two accounts must never share an entry,
//!   and logging out changes the key rather than needing an invalidation hook.
//!
//! The `ok` and shape checks read the envelope `normalize` produces in
//! `scripts/build-ncm-protocol.mjs`. If that shape ever changes the checks stop
//! matching and caching silently turns off, which is the right way for this to
//! fail.
//!
//! ## Writes drop what they invalidate
//!
//! A TTL cannot see the app changing something itself, so the endpoints that do
//! are declared in [`invalidated_by`] and their answer's cache entries are
//! dropped when the write goes through. That is what lets the lists the app
//! mutates be cached at all rather than being permanently excluded.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use crate::disk::DiskCache;

/// Metadata that effectively does not change: a song's title, an album's track
/// list, an artist's biography.
const TTL_STATIC: Duration = Duration::from_secs(30 * 60);

/// Curated and editorial lists, charts, search. These do change, just not on
/// the timescale of a user clicking around.
const TTL_LIST: Duration = Duration::from_secs(5 * 60);

/// Read-only facts about the signed-in user. Short, because the app can change
/// them from another screen and there is no invalidation hook.
const TTL_USER: Duration = Duration::from_secs(60);

/// Lists the app mutates itself. Cacheable only because [`invalidated_by`]
/// drops them on the write that changes them; the TTL is the backstop for a
/// change made on another device.
const TTL_MUTABLE: Duration = Duration::from_secs(2 * 60);

/// How long past `expires` an answer may still be *served* while a refresh runs
/// behind it.
///
/// Generous for metadata, because the failure mode is showing a song's own title
/// a day late, and the alternative is a reconnect the user waits through.
const STALE_STATIC: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Same idea for editorial lists, but short enough that a chart is not a week
/// out of date on screen.
const STALE_LIST: Duration = Duration::from_secs(24 * 60 * 60);

/// Entries kept in memory, whichever bound is reached first.
const MAX_ENTRIES: usize = 256;

/// Total cached bytes in memory. `playlist_track_all` alone reaches megabytes,
/// so an entry count on its own is not a bound.
const MAX_BYTES: usize = 16 * 1024 * 1024;

/// Query keys that must not take part in the cache key.
///
/// `timestamp: Date.now()` is on 56 call sites in `src/api/*.ts`, where it
/// exists to defeat the *deployed* API's cache. Left in the key it would defeat
/// this one too — every call would be a unique miss — so it is stripped here
/// rather than removed there, because the remote transport is still a supported
/// fallback and still wants it.
const VOLATILE_KEYS: [&str; 2] = ["timestamp", "_t"];

/// How long an endpoint's answer may be reused, and how.
#[derive(Clone, Copy)]
pub(crate) struct Policy {
    /// How long the answer counts as fresh.
    ttl: Duration,
    /// How long past that it may still be served while a refresh runs. `ZERO`
    /// means an expired entry is simply a miss.
    stale: Duration,
    /// Whether it may be written to disk.
    ///
    /// Off for the short-lived classes: a 60-second entry would be past its TTL
    /// long before the next launch could read it, so a file is pure churn. Off
    /// for search, which is keyed by whatever the user typed and would fill the
    /// directory with entries nobody asks for twice.
    persist: bool,
}

/// The policy for an endpoint, or `None` if its answer may not be reused.
///
/// Deliberately absent, and worth stating so nobody adds them later without
/// thinking it through:
///
/// * `song_url_v1`, `song_download_url`, `mv_url` — signed, expiring URLs.
///   A cached one plays for a while and then silently stops resolving.
/// * `personal_fm`, `recommend_songs`, `history_recommend_songs` — being
///   different every time is the entire feature.
/// * everything under `login`, `logout`, `captcha`, `scrobble`, `listentogether`
///   — authentication, side effects and realtime state.
fn policy_for(endpoint: &str) -> Option<Policy> {
    let policy = match endpoint {
        // Metadata. The most worth persisting: none of it changes, and it is
        // what a cold start spends its round trips on.
        "song_detail"
        | "album"
        | "album_detail"
        | "artist_detail"
        | "artist_songs"
        | "artist_album"
        | "artist_mv"
        | "artist_top_song"
        | "artists"
        | "mv_detail"
        | "simi_song"
        | "simi_playlist"
        | "simi_mv"
        | "song_wiki_summary"
        // Lyrics are the most-repeated read in the app: fetched on every track
        // change, on every window that shows them, and again whenever a view
        // remounts — and a song's lyric does not change. `lyric_new` is what
        // `LyricsProcessor` actually calls.
        | "lyric"
        | "lyric_new" => Policy {
            ttl: TTL_STATIC,
            stale: STALE_STATIC,
            persist: true,
        },

        // Lists and charts.
        "playlist_detail"
        | "playlist_track_all"
        | "album_new"
        | "album_newest"
        | "artist_list"
        | "top_artists"
        | "top_playlist"
        | "top_playlist_highquality"
        | "toplist"
        | "toplist_artist"
        | "toplist_detail"
        | "banner"
        | "homepage_dragon_ball" => Policy {
            ttl: TTL_LIST,
            stale: STALE_LIST,
            persist: true,
        },

        // Search. Cached for the repeat within a session — a suggestion the box
        // just made, a query re-run by going back — but not persisted: keyed by
        // free text, so the hit rate across launches is not worth the files.
        "search_hot_detail" | "search_suggest" | "cloudsearch" => Policy {
            ttl: TTL_LIST,
            stale: Duration::ZERO,
            persist: false,
        },

        // Lists the app mutates itself. Safe only in combination with
        // `invalidated_by`, and never served stale: after a write the right
        // answer is the current one, so an expired entry is a miss.
        "likelist" | "user_playlist" | "album_sublist" | "artist_sublist" => Policy {
            ttl: TTL_MUTABLE,
            stale: Duration::ZERO,
            persist: false,
        },

        // Read-only facts about the signed-in user.
        "user_detail" | "user_level" | "user_subcount" => Policy {
            ttl: TTL_USER,
            stale: Duration::ZERO,
            persist: false,
        },

        _ => return None,
    };
    Some(policy)
}

/// Endpoints whose answers a write invalidates.
///
/// A TTL cannot see the app changing something itself: liking a song, creating a
/// playlist, subscribing to an album. Without this the choice is between not
/// caching those lists at all — which is what this crate used to do — and a
/// heart that springs back or a playlist that reappears after deletion.
///
/// Returned as the endpoint *names* to drop, every entry for each of them. Coarse
/// on purpose: dropping every `likelist` entry on a `like` costs one refetch and
/// cannot be wrong, where matching the specific id would have to model each
/// endpoint's parameters and would silently miss. A name is matched whole, in both
/// tiers, so `user_detail` is never caught by something naming `user_playlist`.
///
/// The rule for adding one: name every list the write *disturbs*, not just the
/// obvious one. What a write changes about the account is often wider than the
/// endpoint it went to — a `like` moves 我喜欢的音乐's rows and its count as well
/// as the id list, and `playlist_subscribe` changes which playlists exist rather
/// than anything inside one.
fn invalidated_by(endpoint: &str) -> &'static [&'static str] {
    match endpoint {
        "like" => &["likelist", "playlist_track_all", "playlist_detail"],
        "playlist_tracks" | "playlist_create" | "playlist_delete" | "playlist_update"
        | "playlist_name_update" | "playlist_tracks_update" | "playlist_cover_update" => &[
            "user_playlist",
            "playlist_detail",
            "playlist_track_all",
            "user_subcount",
        ],
        // Collecting or un-collecting someone else's playlist changes which
        // playlists the account *has*, so it is `user_playlist` that goes stale —
        // and this was missing, which made the write look like it had not
        // happened. `PlayListView.toChangeLike` calls `setUserPlayLists` the
        // moment `/playlist/subscribe` succeeds, and that refetch was answered
        // from a two-minute-old `user_playlist`: the sidebar did not gain the
        // playlist and the dropdown kept offering to collect one the account had
        // just collected. `user_subcount` feeds the same list's page size.
        "playlist_subscribe" => &["user_playlist", "user_subcount"],
        "album_sub" => &["album_sublist"],
        "artist_sub" => &["artist_sublist"],
        // Trashing an FM track takes it out of 我喜欢的音乐 when it was in
        // there, so the liked playlist's rows and count are stale too — not
        // just the id list. Without these the frontend's reconcile after a
        // dislike would be answered from the cache with the pre-write page.
        "fm_trash" => &["likelist", "playlist_track_all", "playlist_detail"],
        // Logging in or out changes which cookie the keys carry, so the old
        // entries are unreachable rather than wrong — but the *anonymous* ones
        // are now the wrong answer for a user who has an account.
        "login" | "login_cellphone" | "login_qr_check" | "logout" => {
            &["likelist", "user_playlist", "user_detail", "user_subcount", "user_level"]
        }
        _ => &[],
    }
}

/// Whether a hit is fresh, or old enough that it should be refreshed behind the
/// answer.
pub(crate) enum Hit {
    Fresh(Arc<str>),
    /// Serve this, then refetch. The caller does the refetch; the cache only
    /// says it is due.
    Stale(Arc<str>),
}

struct Entry {
    body: Arc<str>,
    expires: Instant,
    /// When this stops being servable at all.
    stale_until: Instant,
    /// Set while a stale-triggered refresh is already running, so twenty
    /// components sharing one stale entry cause one refetch rather than twenty.
    refreshing: bool,
}

pub(crate) struct ResponseCache {
    entries: Mutex<HashMap<String, Entry>>,
    /// Tracked alongside the map so [`MAX_BYTES`] does not need a walk.
    bytes: Mutex<usize>,
    /// `None` when there is nowhere to persist to — an isolate built without a
    /// state directory, which is how the offline tests run.
    ///
    /// `Arc` so a write can be handed to a blocking thread — see [`offload`].
    disk: Option<Arc<DiskCache>>,
}

/// Run a filesystem chore nobody is waiting for on a thread that may block.
///
/// [`ResponseCache::put`] is reached from `NcmCore::call`, i.e. from an async
/// worker whose actual job is driving other requests' network waits — and the
/// chore is `fs::write` + `fs::rename` of up to [`MAX_BYTES`]. On Windows that
/// pair goes through the AV filter driver on a *newly created* file, so it is not
/// the microseconds the arithmetic suggests; it is the classic place a few
/// hundred milliseconds appears from nowhere.
///
/// Nothing observes the result. The entry went into memory synchronously, so no
/// reader in this process can see the difference, and a write that never lands
/// costs one refetch after the next launch.
///
/// Inline when there is no runtime, which is how the unit tests below reach it.
fn offload(chore: impl FnOnce() + Send + 'static) {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.spawn_blocking(chore);
        }
        Err(_) => chore(),
    }
}

impl ResponseCache {
    /// `state_dir` is where the disk tier lives, or `None` for memory only.
    pub(crate) fn new(state_dir: Option<&Path>) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            bytes: Mutex::new(0),
            disk: state_dir.and_then(DiskCache::open).map(Arc::new),
        }
    }

    /// Bring the disk tier inside its bounds. Call once, off the startup path.
    pub(crate) fn prune_disk(&self) {
        if let Some(disk) = &self.disk {
            disk.prune();
        }
    }

    /// The cache key for a call, or `None` when the endpoint is not cacheable.
    ///
    /// Contains the caller's cookie, so it must never reach a log.
    pub(crate) fn key(&self, endpoint: &str, query_json: &str) -> Option<CacheKey> {
        let policy = policy_for(endpoint)?;
        Some(CacheKey {
            key: format!("{endpoint}\u{0}{}", canonical_query(query_json)),
            policy,
        })
    }

    /// Drop whatever `endpoint` invalidates, if it invalidates anything.
    ///
    /// Called after a *successful* write. Matched on the whole endpoint segment of
    /// the key — coarse in that it drops every entry for that endpoint regardless
    /// of parameters, which cannot be wrong (one extra refetch), where a narrower
    /// match would have to model each endpoint's parameters and would silently
    /// miss. Both tiers are dropped, and the disk tier by endpoint rather than by
    /// key; see the comment below for why that distinction is the whole fix.
    pub(crate) fn invalidate_for(&self, endpoint: &str) {
        let disturbed = invalidated_by(endpoint);
        if disturbed.is_empty() {
            return;
        }

        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let mut bytes = self.bytes.lock().unwrap_or_else(|e| e.into_inner());
        let mut dropped = 0usize;
        entries.retain(|key, entry| {
            // The whole endpoint segment: a key is `{endpoint}\0{query}`, so the
            // NUL check is what keeps this from matching a longer name that merely
            // starts the same way.
            let stale = disturbed
                .iter()
                .any(|p| key.starts_with(p) && key[p.len()..].starts_with('\u{0}'));
            if stale {
                *bytes -= entry.body.len();
                dropped += 1;
            }
            !stale
        });
        drop(entries);
        drop(bytes);

        // The disk tier is dropped by *endpoint*, not by the keys that happened
        // to be in memory. Those are not the same set, and assuming they were is
        // what let a write leave a stale answer on disk: an entry is evicted from
        // memory soonest when it is largest, and the largest thing a playlist page
        // creates is the `playlist_detail` a write has to invalidate. So a like
        // dropped nothing, and the reconcile behind it was answered — from disk,
        // for the rest of a 24-hour stale window — with the pre-write list.
        let disk_dropped = match &self.disk {
            Some(disk) => disk.remove_endpoints(disturbed),
            None => 0,
        };
        if dropped > 0 || disk_dropped > 0 {
            log::debug!(
                target: "ncm-core",
                "{endpoint} invalidated {dropped} cached entries and {disk_dropped} files"
            );
        }
    }

    /// A usable answer for `key`, and whether it needs refreshing behind the
    /// caller's back.
    ///
    /// Checks memory, then disk. A disk hit is promoted into memory so the
    /// second reader does not touch the filesystem again.
    pub(crate) fn get(&self, key: &CacheKey) -> Option<Hit> {
        let now = Instant::now();
        {
            let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = entries.get_mut(&key.key) {
                if entry.stale_until <= now {
                    // Past even the stale window: gone, rather than left for the
                    // next insert to sweep.
                    if let Some(dead) = entries.remove(&key.key) {
                        *self.bytes.lock().unwrap_or_else(|e| e.into_inner()) -= dead.body.len();
                    }
                    return None;
                }
                if entry.expires > now {
                    return Some(Hit::Fresh(entry.body.clone()));
                }
                // Stale but servable. Only the first caller is told to refresh;
                // the rest just take the answer, so one stale entry shared by
                // twenty components causes one refetch.
                let body = entry.body.clone();
                if entry.refreshing {
                    return Some(Hit::Fresh(body));
                }
                entry.refreshing = true;
                return Some(Hit::Stale(body));
            }
        }

        // Not in memory. This is the cold-start path: the process just started
        // and the answer is on disk from last time.
        //
        // Read inline, unlike the write in `put`: this *is* the answer the caller
        // is blocked on, so there is nothing to overlap it with, and it happens
        // once per key per process. Moving it to a blocking thread would mean
        // making `get` async, which every layer in front of the cache would have
        // to thread through for no measurable win.
        let stored = self.disk.as_ref()?.load(&key.key)?;
        let fresh = stored.expires > SystemTime::now();
        self.insert(
            key.key.clone(),
            stored.body.clone(),
            deadline(stored.expires),
            deadline(stored.stale_until),
            // A stale file is being served *and* refreshed, so mark it as such
            // on the way in rather than letting the next reader trigger a second
            // refetch.
            !fresh,
        );
        Some(if fresh {
            Hit::Fresh(stored.body)
        } else {
            Hit::Stale(stored.body)
        })
    }

    /// Store a fresh answer, if it is one worth storing.
    ///
    /// What is stored is the *cookie-stripped* form — see [`storable_form`] —
    /// so nothing that arrived as a `Set-Cookie` can ever be handed back.
    pub(crate) fn put(&self, key: CacheKey, body: &str) {
        let Some(storable) = storable_form(body).filter(|b| b.len() <= MAX_BYTES) else {
            // A refresh that came back unstorable must still clear the flag, or
            // the entry would never be refreshed again for as long as it is
            // served stale.
            self.clear_refreshing(&key.key);
            return;
        };

        let now = Instant::now();
        // One allocation, shared with the disk write below rather than copied
        // into it — this is up to `MAX_BYTES`.
        let body: Arc<str> = Arc::from(&*storable);
        self.insert(
            key.key.clone(),
            body.clone(),
            now + key.policy.ttl,
            now + key.policy.ttl + key.policy.stale,
            false,
        );

        if key.policy.persist {
            if let Some(disk) = self.disk.clone() {
                let wall = SystemTime::now();
                let disk_key = key.key.clone();
                let (ttl, stale) = (key.policy.ttl, key.policy.stale);
                offload(move || disk.store(&disk_key, &body, wall + ttl, wall + ttl + stale));
            }
        }
    }

    /// Put an entry into memory, evicting as needed.
    fn insert(
        &self,
        key: String,
        body: Arc<str>,
        expires: Instant,
        stale_until: Instant,
        refreshing: bool,
    ) {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let mut bytes = self.bytes.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();

        if let Some(replaced) = entries.remove(&key) {
            *bytes -= replaced.body.len();
        }

        // Entries past their stale window first — free, and usually enough.
        entries.retain(|_, e| {
            if e.stale_until <= now {
                *bytes -= e.body.len();
                false
            } else {
                true
            }
        });

        // Then, if still over either bound, shed whatever stops being useful
        // soonest. Not an LRU: this cache exists to absorb a burst and a
        // re-navigation, and time-to-live orders that well enough without
        // tracking access.
        while (entries.len() >= MAX_ENTRIES || *bytes + body.len() > MAX_BYTES)
            && !entries.is_empty()
        {
            let Some(next) = entries
                .iter()
                .min_by_key(|(_, e)| e.stale_until)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            if let Some(dropped) = entries.remove(&next) {
                *bytes -= dropped.body.len();
            }
        }

        if *bytes + body.len() > MAX_BYTES {
            return;
        }
        *bytes += body.len();
        entries.insert(
            key,
            Entry {
                body,
                expires,
                stale_until,
                refreshing,
            },
        );
    }

    fn clear_refreshing(&self, key: &str) {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = entries.get_mut(key) {
            entry.refreshing = false;
        }
    }

    /// Record that a background refresh did not produce a new answer.
    ///
    /// The stale entry stays exactly as it was and keeps being served until its
    /// window closes — a failed refresh must not turn into a failed page. Only
    /// the in-progress flag is released, so the next caller past the TTL tries
    /// again rather than the entry being pinned stale for its whole window by one
    /// transient failure.
    pub(crate) fn put_failed(&self, key: CacheKey) {
        self.clear_refreshing(&key.key);
    }

    /// Give back a refresh claim that was never acted on.
    ///
    /// [`Hit::Stale`] hands out the claim, and the caller may then decline to
    /// refresh — there is no room for speculative work right now. Without this
    /// the entry would be marked as refreshing by something that never ran, and
    /// would go un-refreshed for the rest of its stale window.
    pub(crate) fn release_refresh(&self, endpoint: &str, query_json: &str) {
        if let Some(key) = self.key(endpoint, query_json) {
            self.clear_refreshing(&key.key);
        }
    }
}

pub(crate) struct CacheKey {
    key: String,
    policy: Policy,
}

impl CacheKey {
    /// The key as a string, for [`crate::inflight`] to coalesce on.
    ///
    /// Contains the caller's cookie, like the key itself, so it must never
    /// reach a log.
    pub(crate) fn id(&self) -> &str {
        &self.key
    }
}

/// A wall-clock deadline as a monotonic one.
///
/// The disk tier records `SystemTime` because it has to survive a reboot, and the
/// memory tier uses `Instant` because it must not be affected by the clock being
/// adjusted. Converting at the boundary keeps each on the clock that suits it.
/// A deadline already in the past — or a `SystemTime` the machine's clock has
/// moved behind — becomes `now`, which reads as expired.
fn deadline(at: SystemTime) -> Instant {
    match at.duration_since(SystemTime::now()) {
        Ok(remaining) => Instant::now() + remaining,
        Err(_) => Instant::now(),
    }
}

/// Whether an envelope may be remembered, and in what form.
///
/// Reads the shape `normalize` produces — `{"ok":…,"status":…,"body":…,
/// "cookie":[…]}` — because parsing a megabyte of `playlist_track_all` to read
/// two fields is not worth it. A change to that shape turns caching off rather
/// than making it wrong.
///
/// ## Why this is not simply "refuse anything that sets a cookie"
///
/// It was, and that quietly disabled most of the cache. The rule existed so a
/// stored `Set-Cookie` could never resurrect a session the app had moved on
/// from, and it was written believing read-only endpoints set no cookies. They
/// do: every **eapi** response — `lyric_new`, `playlist_detail`, `toplist`, most
/// of the allowlist — carries a ten-year tracking cookie, so all of them failed
/// the check and only `song_detail`, which happens to be weapi, was ever cached
/// at all.
///
/// The safety property is kept by *stripping* rather than refusing. The stored
/// form always has an empty cookie array, so nothing can be replayed, and an
/// envelope that was trying to set something credential-shaped is still refused
/// outright rather than stripped — if a read-only endpoint is handing out a
/// `MUSIC_U`, that is not a response to quietly keep half of.
///
/// Dropping the cookies costs the caller nothing: `ncmLocalTransport.ts` reads
/// `ok`, `status` and `body` and never looks at `cookie`, and the endpoints that
/// genuinely carry a session — everything under `login` — are not cacheable.
///
/// Shared with [`crate::disk`], which applies the same test to what it reads
/// back: a file truncated by the process going away mid-write would otherwise be
/// served as half an envelope.
pub(crate) fn is_storable(body: &str) -> bool {
    body.starts_with(r#"{"ok":true"#) && body.ends_with(r#""cookie":[]}"#)
}

/// Where the envelope's trailing cookie array begins.
const COOKIE_FIELD: &str = r#","cookie":["#;

/// The form of `envelope` that may be stored, or `None` if it may not be.
///
/// Borrows when the envelope is already clean, which is the weapi case and costs
/// nothing; allocates only to strip a cookie array that is actually there.
fn storable_form(envelope: &str) -> Option<Cow<'_, str>> {
    if !envelope.starts_with(r#"{"ok":true"#) {
        // An error is not an answer worth remembering, and a transient 502 would
        // otherwise stick for the whole TTL.
        return None;
    }
    if envelope.ends_with(r#""cookie":[]}"#) {
        return Some(Cow::Borrowed(envelope));
    }

    // `normalize` puts `cookie` last, so the final occurrence of the field is
    // the envelope's own rather than anything inside the body.
    let at = envelope.rfind(COOKIE_FIELD)?;
    let cookies = &envelope[at + COOKIE_FIELD.len()..];
    if !cookies.ends_with("]}") {
        return None;
    }
    if carries_a_credential(cookies) {
        return None;
    }
    Some(Cow::Owned(format!(
        "{}{}]}}",
        &envelope[..at],
        r#","cookie":["#
    )))
}

/// Whether a cookie array is trying to set something that authenticates.
///
/// Deliberately broad and case-insensitive: the cost of a false positive is one
/// uncached response, and the cost of a false negative is a stored credential.
fn carries_a_credential(cookies: &str) -> bool {
    const CREDENTIALS: [&str; 5] = ["music_u", "music_a", "__csrf", "sskey", "sessionid"];
    let lower = cookies.to_ascii_lowercase();
    CREDENTIALS.iter().any(|name| lower.contains(name))
}

/// Normalize a query object so equivalent calls share a key.
///
/// Anything that will not parse is passed through verbatim — a weird key that
/// never hits is strictly better than a key that collides.
fn canonical_query(query_json: &str) -> String {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(query_json)
    else {
        return query_json.to_owned();
    };
    canonical_map(&map)
}

/// Normalize an already-parsed query object.
///
/// Sorts keys, so two call sites that pass the same parameters in a different
/// order still hit, and drops [`VOLATILE_KEYS`].
///
/// Shared with [`crate::batch`], which groups mergeable calls by everything
/// *except* their id list and must agree with this on what counts as the same
/// parameters — including that the `timestamp` cache-buster does not.
pub(crate) fn canonical_map(map: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut pairs: Vec<(&String, &serde_json::Value)> = map
        .iter()
        .filter(|(k, _)| !VOLATILE_KEYS.contains(&k.as_str()))
        .collect();
    pairs.sort_by(|a, b| a.0.cmp(b.0));

    let mut out = String::new();
    for (k, v) in pairs {
        out.push_str(k);
        out.push('\u{1}');
        // `to_string` on a `Value` is already canonical for scalars, and nested
        // objects in a query are rare enough that their key order not being
        // normalized only costs a miss.
        out.push_str(&v.to_string());
        out.push('\u{2}');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(body: &str) -> String {
        format!(r#"{{"ok":true,"status":200,"body":{body},"cookie":[]}}"#)
    }

    /// Memory only. The disk tier has its own tests in [`crate::disk`], and a
    /// scratch directory per test here would only slow them down.
    fn cache() -> ResponseCache {
        ResponseCache::new(None)
    }

    /// A key with the bounds set explicitly, for the expiry tests.
    fn key_with(cache: &ResponseCache, ttl: Duration, stale: Duration) -> CacheKey {
        let mut key = cache.key("song_detail", r#"{"ids":"1"}"#).unwrap();
        key.policy.ttl = ttl;
        key.policy.stale = stale;
        key
    }

    fn body_of(hit: Option<Hit>) -> Option<String> {
        match hit {
            Some(Hit::Fresh(b)) | Some(Hit::Stale(b)) => Some(b.to_string()),
            None => None,
        }
    }

    #[test]
    fn only_allowlisted_endpoints_are_cacheable() {
        let cache = cache();
        assert!(cache.key("song_detail", "{}").is_some());
        assert!(cache.key("playlist_detail", "{}").is_some());
        assert!(cache.key("lyric_new", "{}").is_some());

        // The ones that would break playback, or replay a login.
        for endpoint in [
            "song_url_v1",
            "song_download_url",
            "mv_url",
            "personal_fm",
            "recommend_songs",
            "login_status",
            "login_qr_check",
            "scrobble",
            "listentogether_status",
            "playlist_tracks",
        ] {
            assert!(cache.key(endpoint, "{}").is_none(), "{endpoint} must not be cached");
        }
    }

    /// The lists the app mutates are cacheable now, but only because a write
    /// drops them — and never served stale, because after a write the right
    /// answer is the current one.
    #[test]
    fn app_mutated_lists_are_cacheable_but_never_stale() {
        let cache = cache();
        for endpoint in ["likelist", "user_playlist", "album_sublist", "artist_sublist"] {
            let key = cache.key(endpoint, "{}").expect(endpoint);
            assert_eq!(key.policy.stale, Duration::ZERO, "{endpoint} must not go stale");
            assert!(!key.policy.persist, "{endpoint} must not reach the disk");
        }
    }

    /// The cache-buster on 56 call sites would otherwise make every call a
    /// unique miss, which is the whole reason this cache would have done
    /// nothing at all.
    #[test]
    fn the_timestamp_cache_buster_is_ignored() {
        let cache = cache();
        let a = cache.key("song_detail", r#"{"ids":"347230","timestamp":1}"#).unwrap();
        let b = cache.key("song_detail", r#"{"ids":"347230","timestamp":99999}"#).unwrap();
        assert_eq!(a.key, b.key);
    }

    #[test]
    fn key_order_does_not_split_entries() {
        let cache = cache();
        let a = cache.key("song_detail", r#"{"ids":"1","level":"standard"}"#).unwrap();
        let b = cache.key("song_detail", r#"{"level":"standard","ids":"1"}"#).unwrap();
        assert_eq!(a.key, b.key);
    }

    #[test]
    fn accounts_do_not_share_entries() {
        let cache = cache();
        let a = cache.key("user_detail", r#"{"uid":1,"cookie":"MUSIC_U=aaa"}"#).unwrap();
        let b = cache.key("user_detail", r#"{"uid":1,"cookie":"MUSIC_U=bbb"}"#).unwrap();
        assert_ne!(a.key, b.key);

        // Logging out changes the key rather than needing an invalidation hook.
        let out = cache.key("user_detail", r#"{"uid":1}"#).unwrap();
        assert_ne!(a.key, out.key);
    }

    #[test]
    fn a_stored_answer_comes_back_fresh() {
        let cache = cache();
        let key = cache.key("song_detail", r#"{"ids":"1"}"#).unwrap();
        assert!(cache.get(&key).is_none());

        let body = envelope(r#"{"code":200}"#);
        cache.put(cache.key("song_detail", r#"{"ids":"1"}"#).unwrap(), &body);

        match cache.get(&key) {
            Some(Hit::Fresh(held)) => assert_eq!(&*held, body.as_str()),
            other => panic!("expected a fresh hit, got {:?}", body_of(other)),
        }
    }

    #[test]
    fn failures_and_cookie_setting_responses_are_not_stored() {
        let cache = cache();
        let k = || cache.key("song_detail", r#"{"ids":"1"}"#).unwrap();

        cache.put(k(), r#"{"ok":false,"status":502,"body":{"code":502},"cookie":[]}"#);
        assert!(cache.get(&k()).is_none(), "an error must not be remembered");

        cache.put(
            k(),
            r#"{"ok":true,"status":200,"body":{"code":200},"cookie":["MUSIC_U=x"]}"#,
        );
        assert!(
            cache.get(&k()).is_none(),
            "a response setting a credential must not be stored at all"
        );
    }

    /// The bug that had most of the cache switched off. Every eapi response —
    /// `lyric_new`, `playlist_detail`, `toplist` — carries a ten-year tracking
    /// cookie, and refusing those meant only weapi endpoints were ever cached.
    #[test]
    fn a_tracking_cookie_is_stripped_rather_than_refused() {
        let cache = cache();
        let real = r#"{"ok":true,"status":200,"body":{"code":200,"lrc":{"lyric":"hi"}},"cookie":["NMTID=00Oabc; Max-Age=315360000; Expires=Wed, 20 Aug 2036 19:31:17 GMT; Path=/;"]}"#;

        let key = cache.key("lyric_new", r#"{"id":"1"}"#).unwrap();
        cache.put(cache.key("lyric_new", r#"{"id":"1"}"#).unwrap(), real);

        let held = body_of(cache.get(&key)).expect("an eapi response must be cacheable");
        // The body survived...
        assert!(held.contains(r#""lyric":"hi""#), "{held}");
        // ...and the cookie did not, so nothing can be replayed.
        assert!(!held.contains("NMTID"), "the cookie reached the cache: {held}");
        assert!(held.ends_with(r#""cookie":[]}"#), "{held}");
        // The stored form still passes the shape check the disk tier relies on.
        assert!(is_storable(&held));
    }

    /// Stripping must not be fooled by the body containing the same text.
    #[test]
    fn only_the_envelopes_own_cookie_field_is_stripped() {
        let envelope = r#"{"ok":true,"status":200,"body":{"note":"literally ,\"cookie\":[ in a lyric"},"cookie":["NMTID=x"]}"#;
        let stored = storable_form(envelope).expect("should be storable");
        assert!(stored.contains(r#"literally ,\"cookie\":[ in a lyric"#), "{stored}");
        assert!(stored.ends_with(r#""cookie":[]}"#), "{stored}");
        assert!(!stored.contains("NMTID"));
    }

    #[test]
    fn a_clean_envelope_is_not_copied() {
        let envelope = r#"{"ok":true,"status":200,"body":{"code":200},"cookie":[]}"#;
        assert!(matches!(storable_form(envelope), Some(Cow::Borrowed(_))));
    }

    #[test]
    fn every_credential_name_blocks_storage() {
        for cookie in [
            "MUSIC_U=secret",
            "music_a=secret",
            "__csrf=abc",
            "__remember_me=1; MUSIC_U=x",
            "sSKey=abc",
            "JSESSIONID=abc",
        ] {
            let envelope = format!(
                r#"{{"ok":true,"status":200,"body":{{"code":200}},"cookie":["{cookie}"]}}"#
            );
            assert!(
                storable_form(&envelope).is_none(),
                "{cookie} should have blocked storage"
            );
        }
    }

    /// With no stale window, expiry is a plain miss — and the entry is dropped
    /// rather than left to accumulate.
    #[test]
    fn an_expired_entry_with_no_stale_window_is_a_miss() {
        let cache = cache();
        cache.put(key_with(&cache, Duration::ZERO, Duration::ZERO), &envelope("1"));

        let probe = key_with(&cache, Duration::ZERO, Duration::ZERO);
        assert!(cache.get(&probe).is_none());
        assert_eq!(*cache.bytes.lock().unwrap(), 0);
        assert!(cache.entries.lock().unwrap().is_empty());
    }

    /// The property stale-while-revalidate exists for: an entry past its TTL is
    /// still served, immediately, and the caller is told to refresh it.
    #[test]
    fn an_expired_entry_inside_its_stale_window_is_served() {
        let cache = cache();
        let body = envelope(r#"{"code":200}"#);
        cache.put(
            key_with(&cache, Duration::ZERO, Duration::from_secs(600)),
            &body,
        );

        let probe = key_with(&cache, Duration::ZERO, Duration::from_secs(600));
        match cache.get(&probe) {
            Some(Hit::Stale(held)) => assert_eq!(&*held, body.as_str()),
            other => panic!("expected a stale hit, got {:?}", body_of(other)),
        }

        // Only the first caller is asked to refresh. Twenty components sharing
        // one stale entry must not cause twenty refetches.
        for _ in 0..5 {
            match cache.get(&probe) {
                Some(Hit::Fresh(_)) => {}
                other => panic!("expected the refresh to be claimed already, got {:?}", body_of(other)),
            }
        }
    }

    /// A refresh that fails must leave the entry servable *and* refreshable —
    /// not pinned stale for the rest of its window by one transient failure.
    #[test]
    fn a_failed_refresh_releases_the_claim() {
        let cache = cache();
        cache.put(
            key_with(&cache, Duration::ZERO, Duration::from_secs(600)),
            &envelope("1"),
        );
        let probe = key_with(&cache, Duration::ZERO, Duration::from_secs(600));

        assert!(matches!(cache.get(&probe), Some(Hit::Stale(_))));
        cache.put_failed(key_with(&cache, Duration::ZERO, Duration::from_secs(600)));
        // Still served, and the next caller may try the refresh again.
        assert!(matches!(cache.get(&probe), Some(Hit::Stale(_))));
    }

    /// The refresh claim must also come back when the refresh was never started,
    /// which is what happens whenever there is no room for speculative work.
    /// Without this an entry would be marked as refreshing by something that
    /// never ran and would go un-refreshed for the rest of its stale window.
    #[test]
    fn a_declined_refresh_releases_the_claim() {
        let cache = cache();
        // `lyric_new` so the endpoint and query are the ones `release_refresh`
        // rebuilds the key from.
        let mut key = cache.key("lyric_new", r#"{"id":1}"#).unwrap();
        key.policy.ttl = Duration::ZERO;
        key.policy.stale = Duration::from_secs(600);
        cache.put(key, &envelope("1"));

        let probe = {
            let mut k = cache.key("lyric_new", r#"{"id":1}"#).unwrap();
            k.policy.ttl = Duration::ZERO;
            k.policy.stale = Duration::from_secs(600);
            k
        };

        // First caller claims the refresh, then declines to do it.
        assert!(matches!(cache.get(&probe), Some(Hit::Stale(_))));
        cache.release_refresh("lyric_new", r#"{"id":1}"#);

        // The next caller gets the claim rather than being told it is in hand.
        assert!(
            matches!(cache.get(&probe), Some(Hit::Stale(_))),
            "a declined refresh left the entry claimed by nobody"
        );
    }

    /// Past even the stale window there is nothing left to serve.
    #[test]
    fn an_entry_past_its_stale_window_is_gone() {
        let cache = cache();
        cache.put(key_with(&cache, Duration::ZERO, Duration::ZERO), &envelope("1"));
        assert!(cache.get(&key_with(&cache, Duration::ZERO, Duration::ZERO)).is_none());
        assert!(cache.entries.lock().unwrap().is_empty());
    }

    /// A write has to drop what it invalidated, or the heart springs back.
    #[test]
    fn a_write_invalidates_what_it_changed() {
        let cache = cache();
        let liked = cache.key("likelist", r#"{"uid":1}"#).unwrap();
        let unrelated = cache.key("lyric_new", r#"{"id":1}"#).unwrap();
        cache.put(cache.key("likelist", r#"{"uid":1}"#).unwrap(), &envelope("1"));
        cache.put(cache.key("lyric_new", r#"{"id":1}"#).unwrap(), &envelope("2"));

        cache.invalidate_for("like");

        assert!(cache.get(&liked).is_none(), "the like list survived a like");
        assert!(
            cache.get(&unrelated).is_some(),
            "a like must not drop unrelated entries"
        );
        // The byte count has to follow, or the budget leaks.
        assert_eq!(
            *cache.bytes.lock().unwrap(),
            envelope("2").len(),
            "invalidation did not release its bytes"
        );
    }

    /// Prefix matching must not catch an endpoint that merely starts with the
    /// same letters.
    #[test]
    fn invalidation_matches_whole_endpoint_names() {
        let cache = cache();
        // `user_playlist` is invalidated by a playlist write; `user_detail` is
        // not, and shares its first four characters with nothing relevant.
        let detail = cache.key("user_detail", r#"{"uid":1}"#).unwrap();
        cache.put(cache.key("user_detail", r#"{"uid":1}"#).unwrap(), &envelope("1"));
        cache.invalidate_for("playlist_create");
        assert!(cache.get(&detail).is_some(), "user_detail was dropped by mistake");
    }

    /// The endpoint that reaches this tier through a *file*, which is the case the
    /// naming scheme exists for. `user_playlist` is not persisted, so a purely
    /// in-memory test cannot tell the two matchers apart.
    #[test]
    fn invalidation_by_endpoint_is_exact_on_disk_too() {
        let dir = std::env::temp_dir().join("ncm-core-cache-exact-endpoint-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        {
            let cache = ResponseCache::new(Some(&dir));
            // `album` and `album_detail` are both persisted list/metadata classes,
            // and one is a prefix of the other. Only `album_sublist` is invalidated
            // by `album_sub`, so neither may go.
            cache.put(cache.key("album", r#"{"id":1}"#).unwrap(), &envelope("1"));
            cache.put(cache.key("album_detail", r#"{"id":1}"#).unwrap(), &envelope("2"));
        }
        {
            let cache = ResponseCache::new(Some(&dir));
            cache.invalidate_for("album_sub");
            assert!(
                cache.get(&cache.key("album", r#"{"id":1}"#).unwrap()).is_some(),
                "album was dropped by an unrelated write"
            );
            assert!(
                cache.get(&cache.key("album_detail", r#"{"id":1}"#).unwrap()).is_some(),
                "album_detail was dropped by an unrelated write"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_byte_bound_holds() {
        let cache = cache();
        // One entry that is on its own larger than the whole budget.
        let huge = format!(
            r#"{{"ok":true,"status":200,"body":"{}","cookie":[]}}"#,
            "x".repeat(MAX_BYTES)
        );
        cache.put(cache.key("song_detail", r#"{"ids":"big"}"#).unwrap(), &huge);
        assert_eq!(*cache.bytes.lock().unwrap(), 0, "an oversized body is refused");

        for i in 0..MAX_ENTRIES * 2 {
            let q = format!(r#"{{"ids":"{i}"}}"#);
            cache.put(cache.key("song_detail", &q).unwrap(), &envelope("1"));
        }
        let entries = cache.entries.lock().unwrap();
        assert!(entries.len() <= MAX_ENTRIES, "{} entries", entries.len());
        assert!(*cache.bytes.lock().unwrap() <= MAX_BYTES);
    }

    #[test]
    fn a_malformed_query_still_produces_a_usable_key() {
        let cache = cache();
        let a = cache.key("song_detail", "not json").unwrap();
        let b = cache.key("song_detail", "not json").unwrap();
        let c = cache.key("song_detail", "also not json").unwrap();
        assert_eq!(a.key, b.key);
        assert_ne!(a.key, c.key);
    }

    /// The disk tier must be reachable through the same `get`, because that is
    /// what makes a cold start fast.
    #[test]
    fn an_answer_from_a_previous_launch_is_served() {
        let dir = std::env::temp_dir().join("ncm-core-cache-tier-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let body = envelope(r#"{"code":200,"songs":[{"id":1}]}"#);
        {
            // "Last launch."
            let cache = ResponseCache::new(Some(&dir));
            cache.put(cache.key("song_detail", r#"{"ids":"1"}"#).unwrap(), &body);
        }
        {
            // A new process: nothing in memory, everything on disk.
            let cache = ResponseCache::new(Some(&dir));
            assert!(cache.entries.lock().unwrap().is_empty());
            let key = cache.key("song_detail", r#"{"ids":"1"}"#).unwrap();
            match cache.get(&key) {
                Some(Hit::Fresh(held)) => assert_eq!(&*held, body.as_str()),
                other => panic!("cold start did not read the disk: {:?}", body_of(other)),
            }
            // ...and it was promoted, so the next reader does not touch the disk.
            assert_eq!(cache.entries.lock().unwrap().len(), 1);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The disk write is handed to a blocking thread when there is a runtime to
    /// hand it to (see [`offload`]), which is how it runs in the app and *not*
    /// how the test above runs it. So assert the deferred path lands too — a
    /// silently dropped write reads as "the cold start is slow again", with
    /// nothing failing.
    #[test]
    fn a_write_from_inside_a_runtime_still_reaches_the_disk() {
        let dir = std::env::temp_dir().join("ncm-core-cache-offload-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let body = envelope(r#"{"code":200,"songs":[{"id":7}]}"#);
        {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .expect("runtime");
            let cache = ResponseCache::new(Some(&dir));
            runtime.block_on(async {
                cache.put(cache.key("song_detail", r#"{"ids":"7"}"#).unwrap(), &body);
            });
            // Dropping the runtime waits for its blocking pool, which is the
            // only synchronisation point this side has — and wanting one is
            // exactly why nothing in the app reads the file back.
            drop(runtime);
        }
        {
            let cache = ResponseCache::new(Some(&dir));
            let key = cache.key("song_detail", r#"{"ids":"7"}"#).unwrap();
            match cache.get(&key) {
                Some(Hit::Fresh(held)) => assert_eq!(&*held, body.as_str()),
                other => panic!("the offloaded write never landed: {:?}", body_of(other)),
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The half of invalidation that was missing, and the one that actually
    /// showed: a write has to drop the disk copy of an entry this process never
    /// held in memory.
    ///
    /// That is the normal case rather than an edge one. `playlist_detail` is the
    /// largest response the app makes and sits in the shortest-stale class, so
    /// hydrating a playlist evicts it from memory to make room for the
    /// `song_detail` chunks that hydrating produced — and then the like that
    /// follows found nothing to drop. The reconcile behind it was answered from
    /// disk with the pre-write list, which reads as a playlist that will not
    /// update.
    #[test]
    fn a_write_invalidates_the_disk_copy_it_never_held() {
        let dir = std::env::temp_dir().join("ncm-core-cache-invalidate-disk-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let query = r#"{"id":3778678}"#;
        {
            // "Last launch", or simply an entry memory has since evicted.
            let cache = ResponseCache::new(Some(&dir));
            cache.put(cache.key("playlist_detail", query).unwrap(), &envelope("1"));
            cache.put(cache.key("song_detail", r#"{"ids":"1"}"#).unwrap(), &envelope("2"));
        }
        {
            let cache = ResponseCache::new(Some(&dir));
            assert!(cache.entries.lock().unwrap().is_empty(), "nothing in memory");

            cache.invalidate_for("like");

            assert!(
                cache.get(&cache.key("playlist_detail", query).unwrap()).is_none(),
                "the pre-write playlist survived on disk"
            );
            // A like does not change a song's metadata, so that file stays — the
            // whole point of the disk tier is not throwing away what is still
            // true.
            assert!(
                cache.get(&cache.key("song_detail", r#"{"ids":"1"}"#).unwrap()).is_some(),
                "an unrelated endpoint was dropped from disk"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Only the classes worth a file get one. A 60-second entry would be past
    /// its TTL long before the next launch could read it.
    #[test]
    fn short_lived_classes_are_not_persisted() {
        for endpoint in ["user_detail", "cloudsearch", "search_suggest", "likelist"] {
            let cache = cache();
            let key = cache.key(endpoint, "{}").expect(endpoint);
            assert!(!key.policy.persist, "{endpoint} should not be persisted");
        }
        for endpoint in ["song_detail", "lyric_new", "playlist_detail", "toplist"] {
            let cache = cache();
            let key = cache.key(endpoint, "{}").expect(endpoint);
            assert!(key.policy.persist, "{endpoint} should be persisted");
        }
    }

    /// A prefetch hint has to land on the entry the real call will look for, and
    /// getting that wrong fails *silently* — the hint warms something nothing
    /// reads, and the only symptom is that the app is no faster.
    ///
    /// So the exact query pairs are pinned here. Left column is what
    /// `src/utils/ncmPrefetch.ts` sends, right column is what the matching
    /// `src/api/*.ts` function sends. Changing either side without the other
    /// breaks this.
    #[test]
    fn a_hint_and_its_real_call_share_a_cache_key() {
        let cache = cache();
        for (endpoint, hint, real, what) in [
            // `hintTrack` → `NeteaseLyricProvider.getLyric`: numeric id, and the
            // real call sends no cache-buster here.
            ("lyric_new", r#"{"id":347230}"#, r#"{"id":347230}"#, "lyric"),
            // `hintTrack` → `song.getDetail`: ids is a *string* on both sides,
            // and the real call carries a `timestamp` the cache strips.
            (
                "song_detail",
                r#"{"ids":"347230"}"#,
                r#"{"ids":"347230","timestamp":1771000000000}"#,
                "song detail",
            ),
            // `hintPlaylist` → `playlist.getDetail`.
            (
                "playlist_detail",
                r#"{"id":3778678}"#,
                r#"{"id":3778678}"#,
                "playlist",
            ),
            // `hintAlbum` → `album.get`.
            ("album", r#"{"id":32311}"#, r#"{"id":32311}"#, "album"),
        ] {
            let hinted = cache.key(endpoint, hint).expect(endpoint);
            let asked = cache.key(endpoint, real).expect(endpoint);
            assert_eq!(
                hinted.key, asked.key,
                "the {what} hint warms an entry the real call will never read"
            );
        }
    }

    /// The type of an id is part of the key, which is exactly why the pairs above
    /// are pinned rather than trusted.
    #[test]
    fn an_id_of_the_wrong_type_is_a_different_entry() {
        let cache = cache();
        let number = cache.key("lyric_new", r#"{"id":347230}"#).unwrap();
        let string = cache.key("lyric_new", r#"{"id":"347230"}"#).unwrap();
        assert_ne!(
            number.key, string.key,
            "if these ever collide, the parity test above stops proving anything"
        );
    }
}
