//! Session controls: the state every surface shares.
//!
//! Play mode and "favourite" are not audio state — they are the user's own
//! choices — but they have to be *rendered* next to the transport and
//! *settable* from it. That makes three writers for one value: the app UI, the
//! OS notification, and (for play mode) the planner that has to honour it on the
//! next hop.
//!
//! The shape here is the same one the transport already uses, for the same
//! reason. There is exactly one copy of the value, it lives in the backend, and
//! every surface is a subscriber:
//!
//! ```text
//!   frontend  --SetSessionControls(patch)-->  \
//!   OS button --SetNextPlayMode / ToggleFav-->  AudioPlayer.session_controls
//!                                             /          |
//!                                            /            \--> SessionControlsChanged
//!                                                                |        |
//!                                                          frontend    media bridge
//! ```
//!
//! A surface never writes its own copy and never trusts its own optimism: it
//! sends an intent and adopts what comes back. That is what makes the
//! notification button work with no WebView alive — the case the whole
//! Rust-owned session exists for — and what keeps the app and the notification
//! from disagreeing after either one changes it.
//!
//! `SessionControlsChanged` is emitted only on a real change, so a surface can
//! adopt unconditionally without the adopt → republish → adopt loop an echo
//! would produce.

use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use tracing::{info, warn};

use crate::types::{AudioThreadEvent, SessionControls, SessionControlsPatch, TrackIdentity};

use super::source_resolver;
use super::AudioPlayer;

/// Where a *local* track's like state lives.
///
/// Local favourites cannot reuse the Netease path for one concrete reason:
/// `musicData.ts` replaces `persistData.likeList` wholesale from `/likelist` on
/// every login, so a local entry mixed into it is erased each time the user signs
/// in — and the symptom ("my local favourites vanished after logging in") points
/// at the login code rather than at the storage. So they are a separate set,
/// owned by the host app's library index, reached through this hook for the same
/// reason `install_ncm_call_hook` exists: the store needs an `AppHandle`, and this
/// crate also builds for wasm32.
pub trait LocalFavouriteStore: Send + Sync {
    fn is_favourite(&self, key: &str) -> bool;

    /// Flip the stored value. Returns whether anything changed.
    ///
    /// Must not block on I/O: this is called from the player's event loop, which
    /// also drives the audio timeline. Implementations flip in memory and
    /// persist on a worker.
    fn set_favourite(&self, key: &str, favourite: bool) -> bool;
}

static LOCAL_FAVOURITES: OnceLock<Arc<dyn LocalFavouriteStore>> = OnceLock::new();

/// Install the local favourite store. First install wins.
pub fn install_local_favourite_store(store: Arc<dyn LocalFavouriteStore>) -> bool {
    LOCAL_FAVOURITES.set(store).is_ok()
}

pub fn has_local_favourite_store() -> bool {
    LOCAL_FAVOURITES.get().is_some()
}

fn local_favourites() -> Option<&'static Arc<dyn LocalFavouriteStore>> {
    LOCAL_FAVOURITES.get()
}

/// What the heart should read for `identity`, and whether it should be drawn.
///
/// Pulled out of [`AudioPlayer::refresh_favourite_for_current_track`] so the
/// branch that matters can be pinned by a test: constructing an `AudioPlayer`
/// needs an output device, and the rule this encodes — *an unknown state is
/// `false`, never `true`* — is one whose failure direction is destructive. A
/// wrongly-filled heart means one tap unlikes a track the user does like.
///
/// Returns `(can_favourite, favourite)`.
fn derive_favourite(
    identity: Option<&TrackIdentity>,
    netease_capable: bool,
    likelist: Option<&HashSet<String>>,
    local: Option<&dyn LocalFavouriteStore>,
) -> (bool, bool) {
    match identity {
        // A local file needs no credential at all: the set is on this machine.
        // This is the one case where the heart is drawn while signed out.
        Some(TrackIdentity::Local { path }) => match local {
            Some(store) => (true, store.is_favourite(path)),
            // Nothing could store a press, so do not draw a control that would
            // silently do nothing.
            None => (false, false),
        },
        Some(TrackIdentity::Netease { id }) => (
            netease_capable,
            likelist.is_some_and(|list| list.contains(id)),
        ),
        None => (false, false),
    }
}

/// Outcome of a like/unlike round trip, handed back to the event loop.
///
/// The call is blocking network I/O on a worker thread, so it cannot touch the
/// player directly — and by the time it lands the track may well have changed,
/// which is what `identity_key` is checked against.
pub(super) struct FavouriteResult {
    /// Track the call was made for, so a late answer cannot be applied to a
    /// track the user has already skipped past.
    pub(super) identity_key: String,
    /// The value we asked the account to store.
    pub(super) favourite: bool,
    pub(super) outcome: Result<(), String>,
}

/// A fetched like list, or the reason there is none.
pub(super) struct LikelistResult {
    /// Account the list belongs to. A late answer for an account the user has
    /// since signed out of (or switched away from) must be dropped.
    pub(super) user_id: Option<String>,
    pub(super) outcome: Result<Vec<String>, String>,
}

impl AudioPlayer {
    pub(super) fn session_controls(&self) -> SessionControls {
        self.session_controls
    }

    /// Merge a patch and fan the result out if it changed anything.
    pub(super) async fn apply_session_controls(&mut self, patch: &SessionControlsPatch) {
        // A claimed like is news about the *account*, not just about the
        // projection, and the frontend is the only side that hears about one
        // performed in the app. Recording it in the like list is what keeps the
        // toggle pointing the right way: `refresh_favourite_for_current_track`
        // re-derives `favourite` from that list at every load, so a list that
        // never learned about an in-app like makes the backend believe the track
        // is unliked — and the next press on the notification's heart *likes it
        // again* instead of unliking, which reads as a button that does nothing.
        //
        // Safe to trust because the frontend withholds `favourite` entirely
        // while its own `likeList` is still empty (see
        // `NativeSessionControlsSync.publishSessionControls`), so a present
        // value is one it actually knows — never a cold-start `false`.
        //
        // Done before the change check: the projection and the list can disagree
        // (a value published before the list was fetched settles the former and
        // not the latter), and an unchanged patch is exactly that case.
        if let Some(favourite) = patch.favourite {
            match self.current_identity.clone() {
                // An in-app like on a local track is the same kind of news, and
                // the local store is where the backend re-derives it from at the
                // next load. The frontend's own command has almost certainly
                // written it already, so this is normally a no-op — but it keeps
                // the two paths from being able to disagree.
                Some(TrackIdentity::Local { path }) => {
                    if let Some(store) = local_favourites() {
                        store.set_favourite(&path, favourite);
                    }
                }
                Some(identity) => {
                    if let Some(id) = identity.netease_id() {
                        let id = id.to_string();
                        self.remember_favourite(&id, favourite);
                    }
                }
                None => {}
            }
        }

        let previous_mode = self.session_controls.play_mode;
        if !self.session_controls.apply(patch) {
            return;
        }
        if self.session_controls.play_mode != previous_mode {
            self.adopt_play_mode(self.session_controls.play_mode);
        }
        self.publish_session_controls().await;
    }

    /// Advance the play mode one step. The OS button sends an intent rather than
    /// a value because it cannot know the current mode — only the backend can.
    pub(super) async fn cycle_play_mode(&mut self) {
        let next = self.session_controls.play_mode.cycled();
        self.session_controls.play_mode = next;
        self.adopt_play_mode(next);
        self.publish_session_controls().await;
    }

    /// Teach the planner the new traversal, so the *next* hop honours it even
    /// with no frontend alive to republish a manifest.
    ///
    /// The frontend's later republish (at a higher revision) replaces this order
    /// with its own; both sides converge there. Without this the mode change
    /// would only take effect after the page woke up, which on Android can be
    /// several tracks later.
    fn adopt_play_mode(&mut self, mode: crate::types::NativePlaybackMode) {
        let playing = self
            .current_identity
            .as_ref()
            .map(|identity| identity.key())
            .and_then(|key| self.manifest.position_of_key(&key));
        if self.manifest.set_mode(mode, playing) {
            // The memoized next pick and the source prepared for it were both
            // chosen under the old traversal. The cursor itself is still right —
            // the playing track has not moved — so only the lookahead goes.
            self.planner.invalidate_plan();
            self.source_cache.invalidate();
            self.prefetch_next_source();
        }
    }

    /// Publish the controls to every subscriber: the frontend, and the OS
    /// session through the media bridge's `NowPlayingChanged` projection.
    pub(super) async fn publish_session_controls(&mut self) {
        let controls = self.session_controls;
        // Whether the heart is *drawn* is `can_favourite`, and it is derived
        // from three things that can each be missing for their own reason — a
        // credential, an identity, a fetched list. Logging the resolved triple
        // is the difference between "the button is not there" and knowing which
        // of the three it was.
        info!(
            "session controls: mode={:?} favourite={} can_favourite={} (identity={:?}, likelist={})",
            controls.play_mode,
            controls.favourite,
            controls.can_favourite,
            self.current_identity.as_ref().map(|id| id.key()),
            self.likelist.as_ref().map_or("none".to_string(), |l| l.len().to_string()),
        );
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::SessionControlsChanged { controls })
            .await;
        // The notification renders these next to the metadata, so it needs the
        // whole projection rather than a controls-only delta it would have to
        // merge itself.
        self.publish_now_playing().await;
    }

    /// Toggle the loaded track's like state through the account.
    ///
    /// Optimism is deliberate and bounded: the notification is a one-tap
    /// surface, and waiting for a network round trip before the heart fills
    /// reads as a dead button. The value is reverted if the call fails, which is
    /// the same contract the app UI already has.
    ///
    /// Deliberately *not* gated on `can_favourite`. That flag describes what we
    /// managed to derive, and the button is now always drawn — so treating a
    /// derivation gap as "ignore the press" would turn a visible control into a
    /// silent no-op. What actually decides is whether the call can be made at
    /// all: a Netease track and a credential. Anything missing is logged, so a
    /// dead press says why instead of nothing.
    pub(super) async fn toggle_favourite(&mut self) {
        let Some(identity) = self.current_identity.clone() else {
            warn!("favourite pressed with no identity for the loaded track");
            return;
        };

        // A local track's like state is a local write: authoritative the moment
        // it returns, so none of the optimism-and-revert machinery below applies
        // and neither does the single-flight guard, which exists to stop two
        // presses racing a *network* round trip.
        if let TrackIdentity::Local { path } = &identity {
            let Some(store) = local_favourites() else {
                warn!("favourite pressed on a local track with no local store installed");
                return;
            };
            let target = !self.session_controls.favourite;
            if store.set_favourite(path, target) || self.session_controls.favourite != target {
                self.session_controls.favourite = target;
                self.publish_session_controls().await;
            }
            return;
        }

        if self.favourite_in_flight {
            // A second press before the first landed would race it to the
            // opposite value; the user's intent is already in flight.
            return;
        }
        let Some(song_id) = identity.netease_id().map(str::to_string) else {
            // Only Netease tracks have a like list to be in.
            warn!("favourite pressed on a non-netease track: {}", identity.key());
            return;
        };
        if !self.favourite_capable() {
            warn!("favourite pressed while signed out");
            return;
        }

        let target = !self.session_controls.favourite;
        let identity_key = identity.key();
        let config = self.resolver_config.clone();
        let tx = self.favourite_tx.clone();

        self.favourite_in_flight = true;
        self.session_controls.favourite = target;
        // Record it now so a track change during the call still derives the new
        // value; reverted below if the account refuses.
        self.remember_favourite(&song_id, target);
        self.publish_session_controls().await;

        tokio::task::spawn_blocking(move || {
            let outcome = source_resolver::set_favourite(&song_id, target, &config);
            let _ = tx.send(FavouriteResult {
                identity_key,
                favourite: target,
                outcome,
            });
        });
    }

    /// Reconcile a completed like/unlike.
    pub(super) async fn handle_favourite_result(&mut self, result: FavouriteResult) {
        self.favourite_in_flight = false;

        if let Err(err) = &result.outcome {
            warn!("like failed: {err}");
            // Undo the cached membership regardless of which track is loaded
            // now: the account never stored it, so the list must not claim it.
            if let Some(id) = result.identity_key.strip_prefix("netease:") {
                self.remember_favourite(id, !result.favourite);
            }
        }

        // The track changed while the call was in flight. The optimistic flip
        // was already replaced by whatever the new track's like state is, so
        // there is nothing to revert — and reverting would corrupt it.
        let current = self.current_identity.as_ref().map(|id| id.key());
        if current.as_deref() != Some(result.identity_key.as_str()) {
            return;
        }

        if result.outcome.is_err() && self.session_controls.favourite == result.favourite {
            // Revert only if nothing else has moved it since.
            self.session_controls.favourite = !result.favourite;
            self.publish_session_controls().await;
        }
    }

    /// A new track has its own like state, and the backend resolves it from the
    /// like list it fetched itself.
    ///
    /// Deriving rather than waiting is the whole point: the frontend's push is
    /// the fast path, but it only exists while a page does. On Android the
    /// planner advances with the WebView destroyed, so a heart that depends on JS
    /// is a heart that is empty — or absent — exactly when the notification is
    /// the only UI. A stale `true` is the dangerous direction (one tap would
    /// unlike a track the user does like), so an unknown list means `false`.
    pub(super) fn refresh_favourite_for_current_track(&mut self) {
        let (can_favourite, favourite) = derive_favourite(
            self.current_identity.as_ref(),
            // `can_favourite` describes the *account*, not the frontend's
            // liveness, so it comes from the credential we hold.
            self.favourite_capable(),
            self.likelist.as_ref(),
            local_favourites().map(|store| store.as_ref()),
        );
        self.session_controls.can_favourite = can_favourite;
        self.session_controls.favourite = favourite;
    }

    /// Whether a like can be *performed*: `/like` authenticates with the cookie
    /// alone.
    ///
    /// Deliberately not gated on `user_id`. That is only needed to *fetch* the
    /// list, and conflating the two meant an account whose id we had not been
    /// told yet got no heart at all rather than a heart of unknown state — the
    /// button vanishing is far worse than it starting out empty.
    fn favourite_capable(&self) -> bool {
        self.resolver_config
            .cookie
            .as_deref()
            .is_some_and(|c| !c.trim().is_empty())
    }

    /// Whether the like *list* can be fetched, which additionally needs the uid
    /// `/likelist` is keyed by.
    fn likelist_capable(&self) -> bool {
        self.favourite_capable()
            && self
                .resolver_config
                .user_id
                .as_deref()
                .is_some_and(|u| !u.trim().is_empty())
    }

    /// Fetch the like list for the signed-in account, unless one is already in
    /// flight. Called when the credentials change — i.e. on login, logout and
    /// account switch.
    pub(super) fn refresh_likelist(&mut self) {
        if self.likelist_in_flight {
            return;
        }
        if !self.favourite_capable() {
            // Signed out: drop the list rather than keep a previous account's.
            self.likelist = None;
            return;
        }
        if !self.likelist_capable() {
            // Signed in but we have not been told the uid. The heart still
            // works — `/like` needs only the cookie — we just cannot say which
            // way it points yet.
            return;
        }
        let config = self.resolver_config.clone();
        let user_id = config.user_id.clone();
        let tx = self.likelist_tx.clone();
        self.likelist_in_flight = true;
        tokio::task::spawn_blocking(move || {
            let outcome = source_resolver::fetch_likelist(&config);
            let _ = tx.send(LikelistResult { user_id, outcome });
        });
    }

    pub(super) async fn handle_likelist_result(&mut self, result: LikelistResult) {
        self.likelist_in_flight = false;
        // Answer for an account we are no longer signed in as.
        if result.user_id.as_deref() != self.resolver_config.user_id.as_deref() {
            return;
        }
        match result.outcome {
            Ok(ids) => {
                self.likelist = Some(ids.into_iter().collect::<HashSet<_>>());
                let before = self.session_controls;
                self.refresh_favourite_for_current_track();
                if self.session_controls != before {
                    self.publish_session_controls().await;
                }
            }
            Err(err) => warn!("likelist fetch failed: {err}"),
        }
    }

    /// Record a like the account now holds, so the next track load derives the
    /// right value without another fetch.
    ///
    /// Starts a list when there is none rather than dropping the fact. `None` and
    /// an empty set already behave identically in
    /// [`Self::refresh_favourite_for_current_track`] — both answer `false` for
    /// every track — so remembering can only ever improve the derivation, and it
    /// is the only thing that keeps the heart correct on an account whose
    /// `/likelist` never landed (no uid yet, or a failed fetch). Without it every
    /// press on such an account re-derives `false` at the next load and toggles
    /// in the same direction forever. A completed fetch replaces the list
    /// wholesale, and signing out drops it, so nothing accumulates.
    fn remember_favourite(&mut self, netease_id: &str, favourite: bool) {
        let list = self.likelist.get_or_insert_with(HashSet::new);
        if favourite {
            list.insert(netease_id.to_string());
        } else {
            list.remove(netease_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    struct FakeLocalStore {
        liked: Mutex<HashSet<String>>,
    }

    impl FakeLocalStore {
        fn with(keys: &[&str]) -> Self {
            FakeLocalStore {
                liked: Mutex::new(keys.iter().map(|k| k.to_string()).collect()),
            }
        }
    }

    impl LocalFavouriteStore for FakeLocalStore {
        fn is_favourite(&self, key: &str) -> bool {
            self.liked.lock().contains(key)
        }

        fn set_favourite(&self, key: &str, favourite: bool) -> bool {
            let mut liked = self.liked.lock();
            if favourite {
                liked.insert(key.to_string())
            } else {
                liked.remove(key)
            }
        }
    }

    fn local(path: &str) -> TrackIdentity {
        TrackIdentity::Local {
            path: path.to_string(),
        }
    }

    fn netease(id: &str) -> TrackIdentity {
        TrackIdentity::Netease { id: id.to_string() }
    }

    /// The whole point of the local branch: no account, still a working heart.
    #[test]
    fn a_local_track_is_favouritable_without_a_credential() {
        let store = FakeLocalStore::with(&["/music/a.flac"]);
        assert_eq!(
            derive_favourite(Some(&local("/music/a.flac")), false, None, Some(&store)),
            (true, true)
        );
        assert_eq!(
            derive_favourite(Some(&local("/music/b.flac")), false, None, Some(&store)),
            (true, false)
        );
    }

    /// With no store installed nothing could record a press, so the control must
    /// not be drawn — a visible button that does nothing is worse than none.
    #[test]
    fn a_local_track_without_a_store_draws_no_heart() {
        assert_eq!(
            derive_favourite(Some(&local("/music/a.flac")), true, None, None),
            (false, false)
        );
    }

    #[test]
    fn a_netease_track_needs_a_credential() {
        let list: HashSet<String> = ["123".to_string()].into_iter().collect();
        assert_eq!(
            derive_favourite(Some(&netease("123")), true, Some(&list), None),
            (true, true)
        );
        // Signed out: no heart at all rather than an empty one that fails on tap.
        // The list is `None` because signing out drops it (`refresh_likelist`),
        // which is what keeps one account's likes off another's tracks.
        assert_eq!(
            derive_favourite(Some(&netease("123")), false, None, None),
            (false, false)
        );
    }

    /// An unfetched like list must read `false`, never `true`: a wrongly-filled
    /// heart means one tap unlikes a track the user does like.
    #[test]
    fn an_unknown_netease_like_state_reads_false() {
        assert_eq!(
            derive_favourite(Some(&netease("123")), true, None, None),
            (true, false)
        );
        let empty: HashSet<String> = HashSet::new();
        assert_eq!(
            derive_favourite(Some(&netease("123")), true, Some(&empty), None),
            (true, false)
        );
    }

    /// Netease ids stay strings the whole way. Routing one through `f64` would
    /// corrupt anything past 2^53 — i.e. exactly the newest tracks — and the
    /// symptom would be "the newest songs are never liked".
    #[test]
    fn large_netease_ids_are_matched_exactly() {
        let big = "9007199254740993"; // 2^53 + 1
        let list: HashSet<String> = [big.to_string()].into_iter().collect();
        assert_eq!(
            derive_favourite(Some(&netease(big)), true, Some(&list), None),
            (true, true)
        );
        assert_eq!(
            derive_favourite(Some(&netease("9007199254740992")), true, Some(&list), None),
            (true, false)
        );
    }

    #[test]
    fn no_track_means_no_heart() {
        assert_eq!(derive_favourite(None, true, None, None), (false, false));
    }

    #[test]
    fn a_local_store_reports_whether_it_changed() {
        let store = FakeLocalStore::with(&[]);
        assert!(store.set_favourite("k", true));
        assert!(!store.set_favourite("k", true));
        assert!(store.set_favourite("k", false));
    }
}
