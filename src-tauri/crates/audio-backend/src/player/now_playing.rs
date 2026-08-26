//! Resolved "now playing" projection for OS media sessions.
//!
//! The backend advances tracks on its own — planner hops, native AutoMix
//! crossfades — and on Android it does so while the WebView is destroyed. So it
//! must be able to say what is playing without asking JS.
//!
//! Sources, in priority order:
//!
//! 1. the **manifest entry** matched on `current_identity` — authoritative, and
//!    the only source that survives a dead WebView;
//! 2. the **load-time metadata** captured from the `SongData` the caller
//!    handed us (`pending_display`) — available the instant a load starts,
//!    before a single byte is decoded;
//! 3. the **decoder's tag read** (`DisplayAudioInfo`) — the only source for
//!    genuinely local files that were never in a manifest.
//!
//! Why (2) exists: (3) is not available until decoding begins, and when a
//! streamed track carries no ID3 tags `build_display_info` falls back to
//! `Path::file_stem(source)`. For a Netease CDN URL that stem is a content
//! hash, so the media session would show `b0b9f9a2401f...` for a moment and
//! then correct itself. A filename is only ever a sensible display name for a
//! real file on disk, which is exactly what `allow_path_fallback` gates.
//!
//! Nothing here allocates on an audio callback: it runs on the player's control
//! path alongside `sync_ui`.

use crate::types::{DisplayAudioInfo, NativeManifestEntry, NowPlayingInfo, SongData};

use super::AudioPlayer;

/// Display metadata a caller supplied with the track, parsed once at load time.
///
/// Parsed at load rather than per emit: `sync_ui` runs on every position
/// update, and re-parsing a song JSON blob there would be pure waste.
#[derive(Debug, Clone, Default)]
pub(super) struct PendingDisplay {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub artwork_url: Option<String>,
}

impl PendingDisplay {
    /// Pull display fields out of the `SongData` a load was requested with.
    ///
    /// Prefers the typed `display` field the caller sends with the track. The
    /// `song_json_data` path is a fallback for `Custom` rows that predate it.
    /// Anything missing or malformed yields `None` and defers to the next
    /// source — this must never fail a load.
    pub(super) fn from_song(song: &SongData) -> Self {
        let (display, json_blob) = match song {
            SongData::Local { display, .. } => (display.as_ref(), None),
            SongData::Custom {
                display,
                song_json_data,
                ..
            } => (display.as_ref(), Some(song_json_data)),
        };

        if let Some(display) = display {
            let clean = |value: &Option<String>| -> Option<String> {
                let trimmed = value.as_deref()?.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            };
            let parsed = Self {
                title: clean(&display.title),
                artist: clean(&display.artist),
                album: clean(&display.album),
                artwork_url: clean(&display.artwork_url),
            };
            if !parsed.is_empty() {
                return parsed;
            }
        }

        match json_blob.map(|raw| serde_json::from_str::<serde_json::Value>(raw)) {
            Some(Ok(json)) => Self::from_netease_json(&json),
            _ => Self::default(),
        }
    }

    /// Parse a Netease-shaped song row.
    ///
    /// Shared by two callers because the shape is the same: the frontend's
    /// `song_json_data` blob mirrors Netease's, and `/song/detail` returns it
    /// literally. Artists arrive as an array of `{ name }`; the flat aliases
    /// cover hand-built rows.
    pub(super) fn from_netease_json(json: &serde_json::Value) -> Self {
        let text = |value: Option<&serde_json::Value>| -> Option<String> {
            let trimmed = value?.as_str()?.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        };

        let artist = ["ar", "artists"]
            .iter()
            .filter_map(|key| json.get(*key))
            .filter_map(|value| value.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|entry| text(entry.get("name")))
                    .collect::<Vec<_>>()
                    .join(" / ")
            })
            .find(|joined| !joined.is_empty())
            .or_else(|| text(json.get("artist")));

        let album = json
            .get("al")
            .and_then(|al| text(al.get("name")))
            .or_else(|| text(json.get("album")));

        let artwork_url = json
            .get("al")
            .and_then(|al| text(al.get("picUrl")))
            .or_else(|| text(json.get("cover")))
            .or_else(|| text(json.get("coverSize")));

        Self {
            title: text(json.get("name")).or_else(|| text(json.get("title"))),
            artist,
            album,
            artwork_url,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.artist.is_none()
            && self.album.is_none()
            && self.artwork_url.is_none()
    }
}

/// Trim and discard empty strings, so a manifest entry carrying `Some("")`
/// falls through to the next source instead of blanking the notification.
fn non_empty(value: Option<&String>) -> Option<&str> {
    value.map(|text| text.trim()).filter(|text| !text.is_empty())
}

impl AudioPlayer {
    /// Manifest row for whatever is loaded, matched on stable identity.
    ///
    /// `current_play_index` is deliberately not used: for frontend-driven loads
    /// it is a queue-local `orig_order`, not a manifest position.
    fn current_manifest_entry(&self) -> Option<&NativeManifestEntry> {
        let key = self.current_identity.as_ref()?.key();
        let position = self.manifest.position_of_key(&key)?;
        self.manifest.entry_at(position)
    }

    /// Manifest-supplied title for the loaded track, if it has a usable one.
    /// Used by `metadata_fetch` to decide whether a fetch is needed at all.
    pub(super) fn current_manifest_title(&self) -> Option<&str> {
        non_empty(self.current_manifest_entry().and_then(|e| e.title.as_ref()))
    }

    pub(super) fn now_playing_info(
        &self,
        display: &DisplayAudioInfo,
        position: f64,
        is_playing: bool,
    ) -> NowPlayingInfo {
        // An announcement outranks whatever is still loaded: the user has
        // already moved on, and the session should say so rather than keep
        // showing the previous track through the resolve+download window.
        // Dropped as soon as that track actually loads (see `announce_track`).
        if let Some((identity, announced)) = &self.announced_track {
            if self.current_identity.as_ref() != Some(identity) {
                return NowPlayingInfo {
                    has_track: true,
                    identity: Some(identity.clone()),
                    title: announced.title.clone().unwrap_or_default(),
                    artist: announced.artist.clone().unwrap_or_default(),
                    album: announced.album.clone().unwrap_or_default(),
                    artwork_url: announced.artwork_url.clone(),
                    // Not loaded yet, so there is no timeline to report and it
                    // is definitionally not playing — but it is definitionally
                    // *loading*: an announcement exists precisely to cover the
                    // resolve+download window.
                    duration: 0.0,
                    position: 0.0,
                    is_playing: false,
                    is_loading: true,
                    playlist_index: self.current_play_index,
                    controls: self.session_controls(),
                };
            }
        }

        if self.current_song.is_none() {
            return NowPlayingInfo::default();
        }

        let entry = self.current_manifest_entry();
        let pending = &self.pending_display;

        // The decoder reports a name derived from the *source it opened*, which
        // for anything streamed is a temp file this process created — a random
        // stem, never a display name. Only a genuine on-disk file the caller
        // named can justify a path-derived title.
        //
        // `current_local_path` is the temp file; `file_path` is what the caller
        // asked for. They match only for a real local file, and an https
        // `file_path` (how every streamed track arrives) is never one.
        let allow_path_fallback = self.current_song.as_ref().is_some_and(|song| {
            song.file_path().is_some_and(|path| {
                !crate::decoder::is_http_url(path) && self.current_temp_file.is_none()
            })
        });
        let tag_or_path = |tagged: &str| -> Option<String> {
            let tagged = tagged.trim();
            if tagged.is_empty() {
                return None;
            }
            // `display.name` is the only field that can be a path stem.
            if !allow_path_fallback && tagged == display.name.trim() {
                return None;
            }
            Some(tagged.to_string())
        };

        let title = non_empty(entry.and_then(|e| e.title.as_ref()))
            .map(str::to_string)
            .or_else(|| pending.title.clone())
            .or_else(|| tag_or_path(&display.name))
            .unwrap_or_default();
        let artist = non_empty(entry.and_then(|e| e.artist.as_ref()))
            .map(str::to_string)
            .or_else(|| pending.artist.clone())
            .or_else(|| non_empty(Some(&display.artist)).map(str::to_string))
            .unwrap_or_default();
        let album = non_empty(entry.and_then(|e| e.album.as_ref()))
            .map(str::to_string)
            .or_else(|| pending.album.clone())
            .or_else(|| non_empty(Some(&display.album)).map(str::to_string))
            .unwrap_or_default();
        let artwork_url = non_empty(entry.and_then(|e| e.artwork_url.as_ref()))
            .map(str::to_string)
            .or_else(|| pending.artwork_url.clone());

        // Decoded duration wins over the manifest hint: the hint is parsed from
        // a `mm:ss` display string and wraps past an hour.
        let duration = if display.duration > 0.0 {
            display.duration
        } else {
            entry
                .and_then(|e| e.duration_ms)
                .map(|ms| ms as f64 / 1000.0)
                .unwrap_or(0.0)
        };

        NowPlayingInfo {
            has_track: true,
            identity: self.current_identity.clone(),
            title,
            artist,
            album,
            artwork_url,
            duration,
            position,
            is_playing,
            is_loading: self.load_in_flight,
            playlist_index: self.current_play_index,
            controls: self.session_controls(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom(json: &str) -> SongData {
        SongData::Custom {
            id: "1".into(),
            song_json_data: json.into(),
            orig_order: 0,
            display: None,
        }
    }

    #[test]
    fn reads_netease_shaped_metadata() {
        let d = PendingDisplay::from_song(&custom(
            r#"{"name":"海阔天空","ar":[{"name":"Beyond"}],
                "al":{"name":"乐与怒","picUrl":"https://p1.music.126.net/x.jpg"}}"#,
        ));
        assert_eq!(d.title.as_deref(), Some("海阔天空"));
        assert_eq!(d.artist.as_deref(), Some("Beyond"));
        assert_eq!(d.album.as_deref(), Some("乐与怒"));
        assert_eq!(d.artwork_url.as_deref(), Some("https://p1.music.126.net/x.jpg"));
    }

    #[test]
    fn joins_multiple_artists() {
        let d = PendingDisplay::from_song(&custom(
            r#"{"name":"t","ar":[{"name":"A"},{"name":"B"}]}"#,
        ));
        assert_eq!(d.artist.as_deref(), Some("A / B"));
    }

    #[test]
    fn accepts_legacy_aliases() {
        let d = PendingDisplay::from_song(&custom(
            r#"{"title":"t","artists":[{"name":"A"}],"album":"Al","cover":"https://c/x.jpg"}"#,
        ));
        assert_eq!(d.title.as_deref(), Some("t"));
        assert_eq!(d.artist.as_deref(), Some("A"));
        assert_eq!(d.album.as_deref(), Some("Al"));
        assert_eq!(d.artwork_url.as_deref(), Some("https://c/x.jpg"));
    }

    /// Malformed or partial input must degrade to `None`, never fail a load.
    #[test]
    fn tolerates_junk() {
        for raw in ["", "{", "null", "[]", r#"{"name":"   "}"#, r#"{"ar":[]}"#] {
            let d = PendingDisplay::from_song(&custom(raw));
            assert!(d.title.is_none(), "expected no title from {raw:?}");
            assert!(d.artist.is_none(), "expected no artist from {raw:?}");
        }
    }

    /// Local files deliberately carry no side-channel metadata: their own tags,
    /// and failing that their filename, are the correct source.
    #[test]
    fn local_files_have_no_side_channel() {
        let d = PendingDisplay::from_song(&SongData::Local {
            file_path: "C:/music/song.flac".into(),
            orig_order: 0,
            display: None,
        });
        assert!(d.is_empty());
    }

    /// The typed field is what the frontend actually sends now, and it must win
    /// over the legacy JSON blob.
    #[test]
    fn typed_display_is_preferred() {
        let d = PendingDisplay::from_song(&SongData::Custom {
            id: "1".into(),
            song_json_data: r#"{"name":"from blob"}"#.into(),
            orig_order: 0,
            display: Some(crate::types::TrackDisplay {
                title: Some("from field".into()),
                artist: Some("A".into()),
                ..Default::default()
            }),
        });
        assert_eq!(d.title.as_deref(), Some("from field"));
        assert_eq!(d.artist.as_deref(), Some("A"));
    }

    /// A streamed track carries its metadata on `Local`, because that is how
    /// every remote track is queued — this is the case that regressed.
    #[test]
    fn local_streamed_track_uses_sent_display() {
        let d = PendingDisplay::from_song(&SongData::Local {
            file_path: "https://m702.music.126.net/abc.mp3".into(),
            orig_order: 3,
            display: Some(crate::types::TrackDisplay {
                title: Some("海阔天空".into()),
                artist: Some("Beyond".into()),
                album: Some("乐与怒".into()),
                artwork_url: Some("https://p1/x.jpg".into()),
            }),
        });
        assert_eq!(d.title.as_deref(), Some("海阔天空"));
        assert_eq!(d.album.as_deref(), Some("乐与怒"));
    }

    /// An all-blank display must not shadow the JSON blob fallback.
    #[test]
    fn blank_display_falls_through() {
        let d = PendingDisplay::from_song(&SongData::Custom {
            id: "1".into(),
            song_json_data: r#"{"name":"from blob"}"#.into(),
            orig_order: 0,
            display: Some(crate::types::TrackDisplay {
                title: Some("   ".into()),
                ..Default::default()
            }),
        });
        assert_eq!(d.title.as_deref(), Some("from blob"));
    }
}
