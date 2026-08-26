//! Backend-side track metadata hydration.
//!
//! The backend advances tracks on its own, and on Android it does so while the
//! WebView is destroyed. Until now the *display* half of that still came from
//! JS: the frontend fetched song details and pushed them into the manifest. So
//! when the WebView died, tracks the planner moved to had no title, artist or
//! artwork — the media session fell back to whatever the decoder could read,
//! which for a Netease CDN stream is a content hash.
//!
//! This module closes that gap by fetching `/song/detail` through the embedded
//! protocol layer (`ncm-core`, reached via `source_resolver`'s `NcmCallHook`).
//! Same seam as URL resolution, same reason: this crate also builds for
//! `wasm32-unknown-unknown`, so it cannot depend on the isolate directly.
//!
//! Scheduling mirrors the one-ahead resolver: the fetch runs on
//! `spawn_blocking` and reports back through a channel, so the player loop
//! never waits on the network and nothing here runs near an audio callback.
//!
//! Secrets: the cookie is forwarded so entitlement-sensitive fields match what
//! the user would see, and is never logged.

use crate::types::TrackIdentity;

use super::now_playing::PendingDisplay;
use super::source_resolver;

/// Result of one hydration attempt, handed back to the player loop.
#[derive(Debug)]
pub(super) struct MetadataResult {
    /// Identity the fetch was issued for. The player discards results whose
    /// identity no longer matches what is loaded — by the time a network round
    /// trip lands, the user may well have skipped.
    pub identity: TrackIdentity,
    pub display: PendingDisplay,
}

/// Fetch display metadata for one Netease track.
///
/// Returns `None` when there is no protocol layer installed, the response is
/// unusable, or the track is not a Netease one — every case where the caller
/// should simply keep what it already has.
pub(super) fn fetch_display_blocking(
    identity: &TrackIdentity,
    cookie: Option<&str>,
) -> Option<PendingDisplay> {
    let song_id = identity.netease_id()?;
    let hook = source_resolver::ncm_call_hook()?;

    let mut query = serde_json::json!({ "ids": song_id });
    if let Some(cookie) = cookie.map(str::trim).filter(|c| !c.is_empty()) {
        query["cookie"] = serde_json::Value::String(cookie.to_string());
    }

    let envelope = match hook("song_detail", &query.to_string()) {
        Ok(body) => body,
        Err(err) => {
            log::warn!(
                target: "ncm-metadata",
                "song_detail failed: {}",
                source_resolver::redact(&err)
            );
            return None;
        }
    };

    let parsed: serde_json::Value = serde_json::from_str(&envelope).ok()?;
    if parsed.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        return None;
    }

    // `/song/detail` returns the same row shape the frontend hands us in
    // `SongData::Custom`, so the one parser covers both.
    let song = parsed.get("body")?.get("songs")?.get(0)?;
    let display = PendingDisplay::from_netease_json(song);
    (!display.is_empty()).then_some(display)
}

impl super::AudioPlayer {
    /// Publish the track that is about to load, before its URL is resolved.
    ///
    /// Cheap and synchronous — no network. The point is purely ordering: the
    /// media session swaps to the new track immediately instead of after the
    /// resolve and download, which on a slow link is seconds.
    pub(super) async fn announce_track(
        &mut self,
        identity: crate::types::TrackIdentity,
        display: crate::types::TrackDisplay,
    ) {
        if !self.set_announcement(identity, display) {
            return;
        }
        self.sync_ui().await;
    }

    /// Same, without the `SyncStatus` snapshot.
    ///
    /// For backend-driven advancement, which announces from the window between
    /// tracks where `current_song` is already `None` — see
    /// [`AudioPlayer::publish_now_playing`].
    pub(super) async fn announce_track_quietly(
        &mut self,
        identity: crate::types::TrackIdentity,
        display: crate::types::TrackDisplay,
    ) {
        if !self.set_announcement(identity, display) {
            return;
        }
        self.publish_now_playing().await;
    }

    /// Record the announcement. `false` when there is nothing to announce.
    fn set_announcement(
        &mut self,
        identity: crate::types::TrackIdentity,
        display: crate::types::TrackDisplay,
    ) -> bool {
        // Already the loaded track — nothing to announce, and replacing the
        // live projection with a duration-less one would make the session
        // stutter backwards.
        if self.current_identity.as_ref() == Some(&identity) {
            self.announced_track = None;
            return false;
        }
        self.announced_track = Some((identity, display));
        true
    }

    /// Drop an announcement once its track is really loaded, or once something
    /// else loaded instead (a failed resolve that the user skipped past).
    pub(super) fn settle_announcement(&mut self) {
        if self.announced_track.is_some() {
            self.announced_track = None;
        }
    }

    /// Hydrate display metadata for the loaded track if nothing else has it.
    ///
    /// Called from the load path. Cheap and silent in the common case: when the
    /// manifest already carries a title (frontend-pushed, or a previous fetch)
    /// this returns immediately without touching the network.
    ///
    /// The fetch is fire-and-forget — playback never waits on it. When it lands,
    /// `handle_metadata_result` re-publishes the media session.
    pub(super) fn hydrate_current_metadata(&mut self) {
        let Some(identity) = self.current_identity.clone() else {
            return;
        };
        // Only Netease tracks have anything to fetch; local files use tags.
        if identity.netease_id().is_none() {
            return;
        }
        if !super::source_resolver::has_ncm_call_hook() {
            return;
        }
        // Already know the title, from the manifest or from the caller's blob.
        if self.pending_display.title.is_some() || self.current_manifest_title().is_some() {
            return;
        }

        let cookie = self.resolver_config.cookie.clone();
        let tx = self.metadata_tx.clone();
        self.tasks.push(tokio::task::spawn(async move {
            let fetch_identity = identity.clone();
            let display = tokio::task::spawn_blocking(move || {
                fetch_display_blocking(&fetch_identity, cookie.as_deref())
            })
            .await
            .ok()
            .flatten();

            if let Some(display) = display {
                let _ = tx.send(MetadataResult { identity, display });
            }
        }));
    }

    pub(super) async fn handle_metadata_result(&mut self, result: MetadataResult) {
        // A round trip is long enough for the user to have skipped; applying a
        // stale result would put the wrong track in the notification.
        if self.current_identity.as_ref() != Some(&result.identity) {
            return;
        }
        // Do not clobber anything better that arrived meanwhile.
        if self.pending_display.title.is_some() {
            return;
        }
        self.pending_display = result.display;
        self.sync_ui().await;
    }
}

