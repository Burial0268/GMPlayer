//! Local playlists, and the M3U import/export that feeds them.
//!
//! A playlist stores track *keys*, never copies of the rows — see
//! [`super::model::LocalPlaylist`]. Everything here is a plain in-memory edit
//! followed by a write: unlike a Netease playlist there is no server to be
//! read-after-write inconsistent with, so none of `utils/playlistMutations.ts`'s
//! patch-then-reconcile machinery applies and none of it is reproduced.

use std::collections::HashMap;
use std::path::Path;

use super::index::fnv1a64;
use super::model::LocalPlaylist;

/// Stable-ish id for a new playlist. Derived from name + creation time so two
/// playlists made in the same second with the same name still differ.
pub fn playlist_id(name: &str, created_at: i64) -> String {
    format!("lp-{:016x}", fnv1a64(format!("{name}\u{0}{created_at}").as_bytes()))
}

/// Parse an M3U/M3U8 file into ordered track locators.
///
/// Resolves relative entries against `base_dir` — the overwhelmingly common
/// shape, since players write playlists that sit next to the music. Absolute
/// paths and `file://` URLs are taken as-is; anything remote is dropped, because
/// a local playlist that silently contains an http stream would be a track the
/// local resolver can never play.
pub fn parse_m3u(contents: &str, base_dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for raw in contents.lines() {
        let line = raw.trim();
        // `#EXTINF` and friends are metadata we deliberately ignore: the index
        // already knows every track's real tags, and trusting a playlist's copy
        // would show one title in the list and another in the player.
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with("http://") || line.starts_with("https://") {
            continue;
        }

        let candidate = if let Some(rest) = line.strip_prefix("file:///") {
            decode_percent(rest)
        } else if let Some(rest) = line.strip_prefix("file://") {
            decode_percent(rest)
        } else {
            line.to_string()
        };

        let path = Path::new(&candidate);
        let resolved = if path.is_absolute() || candidate.contains(':') {
            candidate
        } else {
            base_dir
                .join(candidate.replace('\\', "/"))
                .to_string_lossy()
                .to_string()
        };
        out.push(normalize_separators(&resolved));
    }
    out
}

/// Render a playlist as M3U8 with `#EXTINF` lines.
///
/// Absolute paths only. A relative playlist would be smaller but would break the
/// moment it is opened from anywhere but its own directory, and the point of
/// exporting is to hand the file to another player.
pub fn render_m3u(name: &str, entries: &[(String, String, String, u64)]) -> String {
    let mut out = String::from("#EXTM3U\n");
    out.push_str(&format!("#PLAYLIST:{name}\n"));
    for (key, title, artist, duration_ms) in entries {
        let seconds = (*duration_ms as f64 / 1000.0).round() as i64;
        let label = if artist.trim().is_empty() {
            title.clone()
        } else {
            format!("{artist} - {title}")
        };
        out.push_str(&format!("#EXTINF:{seconds},{label}\n"));
        out.push_str(key);
        out.push('\n');
    }
    out
}

/// Match parsed playlist entries against the index.
///
/// Path comparison is normalised (separators, case on Windows) because a
/// playlist written by another player will not spell a path the way our walk
/// did, and an exact match would import an empty playlist and look like a
/// parsing failure.
pub fn resolve_entries<'a, I>(entries: &[String], known_keys: I) -> (Vec<String>, usize)
where
    I: Iterator<Item = &'a str>,
{
    let lookup: HashMap<String, &str> = known_keys
        .map(|key| (comparison_form(key), key))
        .collect();

    let mut resolved = Vec::new();
    let mut missing = 0;
    for entry in entries {
        match lookup.get(&comparison_form(entry)) {
            Some(key) => resolved.push((*key).to_string()),
            None => missing += 1,
        }
    }
    (resolved, missing)
}

fn normalize_separators(value: &str) -> String {
    if cfg!(windows) {
        value.replace('/', "\\")
    } else {
        value.to_string()
    }
}

/// The form two locators are compared in.
///
/// Case-insensitive only where the filesystem is: folding case on Linux would
/// merge two files that genuinely differ.
fn comparison_form(value: &str) -> String {
    let unified = value.replace('\\', "/");
    if cfg!(windows) || cfg!(target_os = "macos") {
        unified.to_lowercase()
    } else {
        unified
    }
}

fn decode_percent(value: &str) -> String {
    percent_decode(value)
}

/// Decode `%XX` escapes. Used for `file://` playlist entries and for SAF
/// document ids, which arrive with `/` and `:` escaped.
///
/// Deliberately not a URL parser: it decodes escapes and touches nothing else,
/// because both callers are handling a path that merely happens to be escaped,
/// not a URL whose structure matters.
pub fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Drop every occurrence of `keys` from `playlist`.
pub fn remove_tracks(playlist: &mut LocalPlaylist, keys: &[String]) -> usize {
    let before = playlist.tracks.len();
    playlist.tracks.retain(|key| !keys.contains(key));
    before - playlist.tracks.len()
}

/// Append `keys`, skipping ones already present.
///
/// A local playlist is a set with an order, not a bag: adding an album twice
/// should not double it. Netease's own behaviour here is the same.
pub fn add_tracks(playlist: &mut LocalPlaylist, keys: &[String]) -> usize {
    let mut added = 0;
    for key in keys {
        if playlist.tracks.iter().any(|existing| existing == key) {
            continue;
        }
        playlist.tracks.push(key.clone());
        added += 1;
    }
    added
}

/// Move the entry at `from` to `to`, clamping both into range.
pub fn reorder(playlist: &mut LocalPlaylist, from: usize, to: usize) -> bool {
    if playlist.tracks.is_empty() || from >= playlist.tracks.len() {
        return false;
    }
    let to = to.min(playlist.tracks.len() - 1);
    if from == to {
        return false;
    }
    let key = playlist.tracks.remove(from);
    playlist.tracks.insert(to, key);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playlist(tracks: &[&str]) -> LocalPlaylist {
        LocalPlaylist {
            id: "p".into(),
            name: "n".into(),
            description: String::new(),
            created_at: 0,
            updated_at: 0,
            tracks: tracks.iter().map(|t| t.to_string()).collect(),
            imported_from: None,
        }
    }

    #[test]
    fn m3u_comments_and_remote_entries_are_dropped() {
        let base = Path::new("/music");
        let parsed = parse_m3u(
            "#EXTM3U\n#EXTINF:180,Artist - Song\nsong.flac\n\nhttps://cdn/x.mp3\n",
            base,
        );
        assert_eq!(parsed.len(), 1);
        assert!(parsed[0].ends_with("song.flac"));
    }

    #[test]
    fn relative_entries_resolve_against_the_playlist_directory() {
        let parsed = parse_m3u("Album/01.flac\n", Path::new("/music"));
        assert_eq!(parsed.len(), 1);
        assert!(
            parsed[0].contains("Album") && parsed[0].contains("01.flac"),
            "got {}",
            parsed[0]
        );
    }

    #[test]
    fn file_urls_are_percent_decoded() {
        let parsed = parse_m3u("file:///music/My%20Song.flac\n", Path::new("/other"));
        assert_eq!(parsed.len(), 1);
        assert!(parsed[0].contains("My Song.flac"), "got {}", parsed[0]);
    }

    /// An import that matches nothing looks exactly like a parser bug, so the
    /// comparison has to tolerate the separator and case differences another
    /// player will write.
    #[test]
    fn resolution_tolerates_separator_differences() {
        let known = ["D:\\Music\\Album\\01.flac"];
        let (resolved, missing) = resolve_entries(
            &["D:/Music/Album/01.flac".to_string()],
            known.iter().copied(),
        );
        assert_eq!(missing, 0);
        assert_eq!(resolved, vec!["D:\\Music\\Album\\01.flac".to_string()]);
    }

    #[test]
    fn unresolvable_entries_are_counted_not_invented() {
        let known = ["/music/a.flac"];
        let (resolved, missing) =
            resolve_entries(&["/music/gone.flac".to_string()], known.iter().copied());
        assert!(resolved.is_empty());
        assert_eq!(missing, 1);
    }

    #[test]
    fn adding_is_idempotent() {
        let mut list = playlist(&["a"]);
        assert_eq!(add_tracks(&mut list, &["a".into(), "b".into()]), 1);
        assert_eq!(list.tracks, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn reorder_clamps_and_reports_no_ops() {
        let mut list = playlist(&["a", "b", "c"]);
        assert!(reorder(&mut list, 0, 99));
        assert_eq!(list.tracks, vec!["b", "c", "a"]);
        assert!(!reorder(&mut list, 1, 1));
        assert!(!reorder(&mut list, 9, 0));
    }

    #[test]
    fn render_round_trips_through_the_parser() {
        let rendered = render_m3u(
            "mine",
            &[(
                "/music/a.flac".to_string(),
                "Song".to_string(),
                "Artist".to_string(),
                180_000,
            )],
        );
        assert!(rendered.contains("#EXTINF:180,Artist - Song"));
        let parsed = parse_m3u(&rendered, Path::new("/elsewhere"));
        assert_eq!(parsed.len(), 1);
        assert!(parsed[0].ends_with("a.flac"));
    }
}
