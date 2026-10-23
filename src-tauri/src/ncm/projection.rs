//! Cut an endpoint's answer down to what the UI actually reads, in Rust, before
//! it crosses IPC.
//!
//! Two endpoints in this app answer in megabytes, and both are consumed by the
//! same list rows. Measured from real cached envelopes:
//!
//! | endpoint | as it comes | projected | `JSON.parse` |
//! |---|---|---|---|
//! | `playlist_detail`, 879 tracks | 2,449,577 chars | 520,610 (21.3%) | 39.3 → 9.6 ms |
//! | `song_detail`, 100 ids | 216,329 chars | 37,282 (17.2%) | 1.17 → 0.25 ms |
//!
//! `song_detail` is asked 1000 ids at a time (`HYDRATE_CHUNK`), so per hydrate
//! chunk that is **2.06 MB → 0.36 MB**. Where it goes is the whole point: the
//! response reaches the WebView as raw bytes over the IPC custom protocol, and
//! the `JSON.parse` that follows is the one step in the entire path — network,
//! isolate, transport, IPC — that runs on the thread drawing the UI.
//!
//! Two thirds of what is dropped is not even fields-we-ignore, it is
//! `body.privileges`: 40% of a `playlist_detail` answer and 43% of a
//! `song_detail` one, read by exactly one call site in the app (the download
//! modal's quality list), which keeps using the unprojected path.
//!
//! This is deliberately *not* streaming. An eapi body has to be whole before it
//! decrypts at all (`body.toString('hex')` over the entire buffer), every
//! endpoint module is handed a materialised object which `normalize()`
//! stringifies once, and a Tauri command has exactly one response — the only
//! streaming primitive is `Channel`, which past 1 KB of raw bytes costs an `eval`
//! plus a second `invoke` per chunk. Sending less beats sending the same amount
//! in pieces.
//!
//! ## Opt-in, and a view rather than a replacement
//!
//! [`crate::ncm::ncm_request_projected`] runs the *same* `NcmCore::call` as
//! `ncm_request`, so the cache, `batch` and `inflight` all behave identically and
//! the entry they store is the **full** envelope. Projection happens after, per
//! call. So a caller that needs `privileges` simply keeps using `ncm_request` and
//! is answered from the same cached bytes.
//!
//! ## Shape compatibility
//!
//! Output keeps upstream's field *names*, so the frontend does not fork: tracks
//! are still spelled the way `transformSongData` reads them, and
//! `playlist.trackIds` is still `trackIds`, flattened to bare ids — which
//! `extractManifestIds` accepts alongside the original `[{id, …}]` form. It has
//! to accept both: the `remote` transport and Web never reach this code.
//!
//! Anything unexpected — a non-`ok` envelope, a missing `playlist` — returns
//! `None` so the caller forwards the original bytes untouched. Same rule as
//! `batch::Merged::slice_for`: a shape we did not predict is handed over as it
//! came, so in-band codes like 301 survive.

use serde_json::{Map, Value};

/// The track fields `src/utils/ncm/transformSongData.ts` reads.
///
/// `ar` and `al` are kept **verbatim** rather than trimmed to `{id, name}`:
/// `ensureAlbumPicUrl` falls back to `al.pic_str` / `al.pic` when `picUrl` is
/// absent, so pruning inside `al` would silently cost covers on exactly the
/// tracks that need the fallback. Both objects are small (an artist is ~50
/// chars) and keeping them whole is worth ~4 points of the projected size.
const TRACK_FIELDS: [&str; 9] = ["id", "name", "ar", "al", "alia", "dt", "fee", "pc", "mv"];

/// Whether this endpoint has a projection at all.
///
/// The frontend sends the endpoint name, so this is the guard: an endpoint that
/// is not named here is answered in full, exactly as `ncm_request` would.
pub(crate) fn is_projectable(endpoint: &str) -> bool {
    matches!(endpoint, "playlist_detail" | "song_detail")
}

/// Project an envelope, or `None` if it is not a shape this understands.
pub(crate) fn project(endpoint: &str, envelope: &str) -> Option<String> {
    let Value::Object(mut root) = serde_json::from_str::<Value>(envelope).ok()? else {
        return None;
    };
    // A failed call is forwarded verbatim: the frontend acts on `ok: false` and
    // on the status behind it, and there is nothing here worth shrinking.
    if root.get("ok").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let status = root.get("status").cloned().unwrap_or_else(|| Value::from(200));
    let cookie = root.remove("cookie").unwrap_or_else(|| Value::Array(Vec::new()));
    let Some(Value::Object(mut body)) = root.remove("body") else {
        return None;
    };

    match endpoint {
        "playlist_detail" => project_playlist(&mut body)?,
        "song_detail" => project_songs(&mut body)?,
        _ => return None,
    }

    // Hand-formatted for the key order `ok, status, body, cookie`. Nothing
    // downstream of here parses the envelope by shape, but the order is the
    // house rule for anything that builds one in Rust (`serde_json::Map` sorts,
    // so serialising a struct would come out `{"body":…`), and one path that
    // does care is one too many to risk.
    Some(format!(
        "{{\"ok\":true,\"status\":{status},\"body\":{},\"cookie\":{cookie}}}",
        Value::Object(body)
    ))
}

/// `{ playlist: { trackIds, tracks, … }, privileges, … }`.
fn project_playlist(body: &mut Map<String, Value>) -> Option<()> {
    let Some(Value::Object(mut playlist)) = body.remove("playlist") else {
        return None;
    };
    // 40% of the payload, and not one field of it is read here: the row's `fee`
    // and `pc` come from the track object itself.
    body.remove("privileges");
    flatten_track_ids(&mut playlist);
    if let Some(Value::Array(tracks)) = playlist.get_mut("tracks") {
        trim_songs(tracks);
    }
    body.insert("playlist".to_owned(), Value::Object(playlist));
    Some(())
}

/// `{ songs, privileges }`.
fn project_songs(body: &mut Map<String, Value>) -> Option<()> {
    let Some(Value::Array(songs)) = body.get_mut("songs") else {
        return None;
    };
    trim_songs(songs);
    // Read only by `DataModal/DownloadSong.vue` (`privileges[0].downloadMaxbr`),
    // which asks for one id at a time through the unprojected path.
    body.remove("privileges");
    Some(())
}

/// `[{id, v, t, at, alg, uid, rcmdReason, …}]` → `[id, …]`.
///
/// All or nothing: if a single entry has no `id` the array is left alone, since
/// a silently short manifest is a wrong playlist length, wrong scroll height and
/// a hydrate cursor that never reaches the end.
fn flatten_track_ids(playlist: &mut Map<String, Value>) {
    let Some(Value::Array(ids)) = playlist.get_mut("trackIds") else {
        return;
    };
    let flat: Vec<Value> = ids
        .iter()
        .filter_map(|entry| entry.get("id").cloned())
        .collect();
    if flat.len() == ids.len() {
        *ids = flat;
    }
}

/// Keep [`TRACK_FIELDS`], drop the other ~45.
fn trim_songs(songs: &mut [Value]) {
    for song in songs.iter_mut() {
        let Value::Object(map) = song else { continue };
        let mut slim = Map::new();
        for field in TRACK_FIELDS {
            if let Some(value) = map.remove(field) {
                slim.insert(field.to_owned(), value);
            }
        }
        // `DataLists` renders `row.item.alia[0]` unguarded, so an absent `alia`
        // is a TypeError rather than a missing subtitle. Upstream always sends
        // one; costs nothing to be sure of it.
        slim.entry("alia".to_owned())
            .or_insert_with(|| Value::Array(Vec::new()));
        *map = slim;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: u32) -> Value {
        serde_json::json!({
            "id": id,
            "name": "song",
            "ar": [{ "id": 9, "name": "artist", "tns": [] }],
            "al": { "id": 3, "name": "album", "pic_str": "1234" },
            "alia": ["also known as"],
            "dt": 210000,
            "fee": 8,
            "pc": null,
            "mv": 0,
            "h": { "br": 320000, "size": 8400000 },
            "privilege": { "id": id, "st": 0 },
            "publishTime": 0
        })
    }

    /// An envelope shaped like the real one, small enough to assert on.
    fn playlist_envelope() -> String {
        serde_json::json!({
            "ok": true,
            "status": 200,
            "body": {
                "code": 200,
                "privileges": [{ "id": 1, "fee": 8, "payed": 0, "st": 0 }],
                "playlist": {
                    "id": 42,
                    "name": "list",
                    "trackCount": 2,
                    "creator": { "nickname": "someone", "userId": 7 },
                    "trackIds": [
                        { "id": 1, "v": 8, "alg": null, "rcmdReason": "" },
                        { "id": 2, "v": 3, "alg": null, "rcmdReason": "" }
                    ],
                    "tracks": [song(1)]
                }
            },
            "cookie": ["MUSIC_U=redacted"]
        })
        .to_string()
    }

    fn song_envelope() -> String {
        serde_json::json!({
            "ok": true,
            "status": 200,
            "body": {
                "code": 200,
                "songs": [song(1), song(2)],
                "privileges": [{ "id": 1, "downloadMaxbr": 999000 }, { "id": 2 }]
            },
            "cookie": []
        })
        .to_string()
    }

    fn assert_is_a_row(track: &Value) {
        let kept: Vec<&String> = track.as_object().unwrap().keys().collect();
        assert_eq!(kept.len(), TRACK_FIELDS.len(), "kept exactly the whitelist");
        for field in TRACK_FIELDS {
            assert!(track.get(field).is_some(), "{field} is read by the frontend");
        }
        // `al` and `ar` whole, because `ensureAlbumPicUrl` needs `pic_str`.
        assert_eq!(track["al"]["pic_str"], "1234");
        assert_eq!(track["ar"][0]["name"], "artist");
    }

    #[test]
    fn playlist_detail_keeps_the_header_the_manifest_and_the_rows() {
        let out = project("playlist_detail", &playlist_envelope()).expect("known shape");
        let value: Value = serde_json::from_str(&out).unwrap();
        let body = &value["body"];

        assert!(body.get("privileges").is_none(), "privileges is dead weight");
        assert_eq!(body["code"], 200, "the small body fields survive");

        let playlist = &body["playlist"];
        assert_eq!(playlist["trackIds"], serde_json::json!([1, 2]));
        assert_eq!(playlist["creator"]["nickname"], "someone");
        assert_eq!(playlist["trackCount"], 2);
        assert_is_a_row(&playlist["tracks"][0]);
    }

    #[test]
    fn song_detail_keeps_the_rows_and_drops_the_privileges() {
        let out = project("song_detail", &song_envelope()).expect("known shape");
        let value: Value = serde_json::from_str(&out).unwrap();
        let body = &value["body"];

        assert!(
            body.get("privileges").is_none(),
            "only DownloadSong reads these, and it does not come through here"
        );
        assert_eq!(body["songs"].as_array().unwrap().len(), 2);
        assert_is_a_row(&body["songs"][0]);
        assert_eq!(body["songs"][1]["id"], 2, "order is the caller's identity");
    }

    /// `is_projectable` is the guard on the command; anything else is answered in
    /// full, so the two must agree on the same set.
    #[test]
    fn only_the_projectable_endpoints_project() {
        for endpoint in ["playlist_detail", "song_detail"] {
            assert!(is_projectable(endpoint), "{endpoint}");
        }
        for endpoint in ["lyric_new", "song_url_v1", "user_playlist", ""] {
            assert!(!is_projectable(endpoint), "{endpoint}");
            assert!(project(endpoint, &song_envelope()).is_none(), "{endpoint}");
        }
    }

    /// `cache::is_storable` reads an envelope's shape rather than parsing it, so
    /// anything that builds one by hand has to put `ok` first.
    #[test]
    fn the_envelope_keeps_its_key_order() {
        let out = project("playlist_detail", &playlist_envelope()).unwrap();
        assert!(out.starts_with(r#"{"ok":true,"status":200,"body":"#), "{out}");
        assert!(out.ends_with(r#","cookie":["MUSIC_U=redacted"]}"#), "{out}");
    }

    /// A non-200 upstream answer is a successful call the caller has to see in
    /// full — 301 "not logged in" is read out of it.
    #[test]
    fn a_failed_call_is_forwarded_untouched() {
        let failed = r#"{"ok":false,"status":301,"body":{"code":301},"cookie":[]}"#;
        assert!(project("playlist_detail", failed).is_none());
        assert!(project("song_detail", failed).is_none());
    }

    #[test]
    fn an_unexpected_shape_is_forwarded_untouched() {
        let empty = r#"{"ok":true,"status":200,"body":{},"cookie":[]}"#;
        assert!(project("playlist_detail", empty).is_none());
        assert!(project("song_detail", empty).is_none());
        assert!(project("song_detail", "not json at all").is_none());
    }

    /// Dropping one id would shorten the manifest, which is a wrong track count,
    /// a wrong scroll height and a hydrate cursor that never reaches the end.
    #[test]
    fn a_manifest_entry_without_an_id_leaves_the_manifest_raw() {
        let mut value: Value = serde_json::from_str(&playlist_envelope()).unwrap();
        value["body"]["playlist"]["trackIds"][1] = serde_json::json!({ "v": 3 });
        let out = project("playlist_detail", &value.to_string()).unwrap();
        let projected: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            projected["body"]["playlist"]["trackIds"][0]["id"], 1,
            "left as [{{id, …}}] so the frontend's other branch handles it"
        );
    }

    #[test]
    fn a_track_without_alia_still_gets_an_array() {
        let mut value: Value = serde_json::from_str(&song_envelope()).unwrap();
        value["body"]["songs"][0]
            .as_object_mut()
            .unwrap()
            .remove("alia");
        let out = project("song_detail", &value.to_string()).unwrap();
        let projected: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(projected["body"]["songs"][0]["alia"], serde_json::json!([]));
    }
}
