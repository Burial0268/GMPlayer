//! Where downloads land, and how the queue behaves.
//!
//! Owned by Rust rather than by `settingData`, and that is not a style choice.
//! The queue has to keep running with no page alive — on Android the WebView is
//! routinely killed and rebuilt (`MainActivity.installRenderProcessGuard` exists
//! for exactly that), and a Pinia-only setting is unreadable at that moment. So
//! `$APPDATA/download.json` is the truth and the settings page is a client of it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

/// Subdirectory created under the platform's music directory.
pub const DIR_NAME: &str = "GMPlayer";

/// Where the Android directory picker opens, relative to primary shared storage.
///
/// `Download` is the folder people ask for and the one the platform will not
/// give: from target SDK 30 `ACTION_OPEN_DOCUMENT_TREE` refuses to grant it, the
/// internal storage root, or the root of any SD card the device considers
/// reliable — the entry is listed with its confirm button greyed out, so there is
/// no way to reach it through a grant at all.
///
/// `Music` is not on that list, and is the better destination for a music player
/// regardless: it is the collection every other audio app on the device already
/// indexes, and it is what `audio_dir()` resolves to on desktop, so both
/// platforms end up under the same named folder for the same reason.
pub const PICKER_INITIAL_DIR: &str = "Music";

const FILE_NAME: &str = "download.json";

/// Returned when Android has no directory yet, so the frontend can tell "the
/// user has not picked a folder" apart from a real failure and open the picker
/// instead of showing an error.
pub const NEEDS_DIRECTORY: &str = "needs-directory";

/// Hard ceiling on parallel downloads.
///
/// Netease sheds a client that opens too many connections at once — the same
/// politeness limit `ncm-core`'s `throttle::MAX_IN_FLIGHT` encodes — and these
/// are multi-megabyte transfers, so more streams mostly means each one is
/// slower.
pub const MAX_CONCURRENCY: usize = 6;

/// Hard ceiling on how many streams one file may be split across.
///
/// Eight is where the returns stop on a CDN that limits a single connection: the
/// link, not the stream count, is the wall past that, and every extra stream is
/// another handshake plus another part file to create, list and concatenate.
pub const MAX_SEGMENTS: usize = 8;

/// Hard ceiling on sockets this module holds open at once, across every task.
///
/// Splitting a download multiplies connections by [`DownloadConfig::segments`], so
/// without a shared ceiling six parallel tasks at four segments each would open
/// twenty-four — which is exactly the burst that gets a client shed (the same
/// politeness argument as `ncm-core`'s `throttle::MAX_IN_FLIGHT`, and the reason
/// [`MAX_CONCURRENCY`] exists at all). Every task always gets one stream; what
/// this bounds is how many *extra* ones the splits may add, so a lone download
/// takes the whole budget and a full queue quietly stops splitting.
pub const MAX_TOTAL_STREAMS: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
// `default` on the container, not a version field: every field can be filled
// from a default, so an older file (or a hand-edited one missing a key) loads
// rather than resetting the lot.
#[serde(rename_all = "camelCase", default)]
pub struct DownloadConfig {
    /// An absolute path on desktop, a SAF tree URI on Android. `None` means
    /// "not decided yet", which only Android ever really sees — desktop fills in
    /// a default on first use.
    pub dir: Option<String>,
    /// Human-readable name for [`Self::dir`].
    ///
    /// Needed because a `content://…/tree/primary%3AMusic` is not something to
    /// show a user, and the only moment its real name is available is when the
    /// picker returns.
    pub dir_label: Option<String>,
    pub concurrency: usize,
    /// How many `Range` streams one file may be split across.
    ///
    /// `1` turns splitting off. Capped by [`MAX_SEGMENTS`], and only ever reached
    /// for a file large enough to be worth dividing — `fetch::segments_for` makes
    /// that call, and the shared [`MAX_TOTAL_STREAMS`] budget decides how many of
    /// the segments run at the same time.
    pub segments: usize,
    /// Placeholders: `{artist}`, `{title}`, `{album}`, `{index}`.
    pub filename_template: String,
    /// The `br` parameter for `/song/download/url`.
    pub default_br: u32,
    /// Write the lyric next to the audio as a sibling `.lrc`.
    pub write_lyric: bool,
    /// Write the album cover next to the audio as `cover.jpg`.
    pub write_cover: bool,
    /// Write title/artist/album, the lyric and the cover *into* the audio file.
    ///
    /// On by default, because an NCM streaming source carries none of them: without
    /// this a downloaded track describes itself only by its filename, to other
    /// players and to our own library's tag probe alike.
    pub embed_tags: bool,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        DownloadConfig {
            dir: None,
            dir_label: None,
            concurrency: 3,
            // Four is where a single connection's rate limit stops being the wall
            // on a typical link, and it fits the shared stream budget alongside
            // the default concurrency without either starving the other.
            segments: 4,
            filename_template: "{artist} - {title}".to_string(),
            default_br: 320_000,
            write_lyric: true,
            // Off by default. The downloaded audio almost always embeds its own
            // artwork and the library extracts that at scan time, so a sidecar is
            // a second copy that only other players would read — and one extra
            // request per track to produce it.
            write_cover: false,
            // On by default, and it is what makes the sentence above true: the
            // artwork is embedded *because this does it*. A Netease stream has none
            // of its own.
            embed_tags: true,
        }
    }
}

impl DownloadConfig {
    /// Clamp anything a hand-edited file (or a future frontend bug) could get
    /// wrong. Called on both load and save, so the in-memory copy is always sane.
    pub fn normalize(&mut self) {
        self.concurrency = self.concurrency.clamp(1, MAX_CONCURRENCY);
        self.segments = self.segments.clamp(1, MAX_SEGMENTS);
        if self.filename_template.trim().is_empty() {
            self.filename_template = DownloadConfig::default().filename_template;
        }
        if self.default_br == 0 {
            self.default_br = DownloadConfig::default().default_br;
        }
        if let Some(dir) = &self.dir {
            if dir.trim().is_empty() {
                self.dir = None;
            }
        }
        if self.dir.is_none() {
            self.dir_label = None;
        }
    }
}

/// Where the bytes go. The one place the platform split is expressed.
#[derive(Debug, Clone)]
pub enum DownloadTarget {
    /// A real directory. Desktop, and any Android path we can open directly.
    Path(PathBuf),
    /// A SAF tree the user granted read+write. Android.
    Tree(String),
}

impl DownloadTarget {
    /// The string the local library uses as this source's locator.
    pub fn locator(&self) -> String {
        match self {
            DownloadTarget::Path(path) => path.to_string_lossy().to_string(),
            DownloadTarget::Tree(uri) => uri.clone(),
        }
    }
}

fn config_path<R: Runtime>(app: &AppHandle<R>) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(FILE_NAME)
}

pub fn load<R: Runtime>(app: &AppHandle<R>) -> DownloadConfig {
    let path = config_path(app);
    let mut config = match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<DownloadConfig>(&text) {
            Ok(config) => config,
            Err(err) => {
                // Loud, and the file is left alone: it holds a directory the user
                // chose (on Android, one backed by a grant we cannot re-derive),
                // and silently overwriting it would look like the app forgetting.
                log::error!(
                    target: "download",
                    "download settings could not be read ({err}); leaving {} in place",
                    path.display()
                );
                DownloadConfig::default()
            }
        },
        Err(_) => DownloadConfig::default(),
    };
    config.normalize();
    config
}

pub fn save<R: Runtime>(app: &AppHandle<R>, config: &DownloadConfig) -> Result<(), String> {
    let path = config_path(app);
    let bytes = serde_json::to_vec_pretty(config).map_err(|e| e.to_string())?;
    crate::local::index::write_atomic(&path, &bytes).map_err(|e| e.to_string())
}

/// Longest filename stem we will produce, in characters.
///
/// Not a byte budget on purpose: most filesystems cap a *component* at 255
/// bytes, and a CJK title is three bytes a character, so counting characters and
/// stopping well short is simpler than getting the byte arithmetic right at
/// every boundary. Nothing legible is lost — a 120-character song title is
/// already a paragraph.
const MAX_STEM_CHARS: usize = 120;

/// Make one template substitution safe to put in a filename.
///
/// The union of what Windows, ext4 and a SAF `DocumentsProvider` reject, plus
/// the two Windows rules people forget: a trailing dot or space is silently
/// stripped by the shell, which turns "file ." and "file" into a collision.
pub fn sanitize_component(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => out.push('_'),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    // Collapse runs of whitespace so a stripped control character does not leave
    // a double space behind.
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.trim_matches(|c: char| c == '.' || c.is_whitespace()).to_string()
}

/// Fill `template` and append `extension`.
///
/// Returns a complete filename, always non-empty and always carrying the
/// extension — `local::scan` decides what is audio by extension alone, so a
/// download that lost its own would never be indexed.
pub fn render_filename(
    template: &str,
    artist: &str,
    title: &str,
    album: &str,
    index: usize,
    extension: &str,
) -> String {
    let stem = template
        .replace("{artist}", &sanitize_component(artist))
        .replace("{title}", &sanitize_component(title))
        .replace("{album}", &sanitize_component(album))
        .replace("{index}", &format!("{index:02}"));
    let mut stem = sanitize_component(&stem);
    if stem.is_empty() {
        stem = sanitize_component(title);
    }
    if stem.is_empty() {
        stem = "track".to_string();
    }
    if stem.chars().count() > MAX_STEM_CHARS {
        stem = stem.chars().take(MAX_STEM_CHARS).collect::<String>();
        stem = sanitize_component(&stem);
    }
    let extension = extension.trim().trim_start_matches('.').to_ascii_lowercase();
    if extension.is_empty() {
        stem
    } else {
        format!("{stem}.{extension}")
    }
}

/// Where downloads land when the user has not chosen a folder.
///
/// `audio_dir()` is `FOLDERID_Music` on Windows, `~/Music` on macOS and
/// `$XDG_MUSIC_DIR` on Linux — the place a system music app already indexes,
/// which is the point of resolving it rather than inventing one.
pub fn default_path<R: Runtime>(app: &AppHandle<R>) -> PathBuf {
    let resolver = app.path();
    let base = resolver
        .audio_dir()
        .or_else(|_| resolver.download_dir())
        .or_else(|_| resolver.app_data_dir())
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join(DIR_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filename_survives_every_reserved_character() {
        let name = render_filename(
            "{artist} - {title}",
            "AC/DC",
            r#"Who Made Who? <live>"#,
            "",
            1,
            "flac",
        );
        assert_eq!(name, "AC_DC - Who Made Who_ _live_.flac");
        assert!(!name.contains('/'));
    }

    #[test]
    fn a_trailing_dot_is_stripped_before_the_extension_is_added() {
        // Windows drops a trailing dot silently, so "Intro." and "Intro" would be
        // the same file — and the second download would overwrite the first.
        let name = render_filename("{title}", "", "Intro.", "", 1, "mp3");
        assert_eq!(name, "Intro.mp3");
    }

    #[test]
    fn an_empty_render_still_produces_a_usable_name() {
        assert_eq!(render_filename("{artist}", "", "", "", 1, "mp3"), "track.mp3");
        // Falls back to the title before the generic name.
        assert_eq!(render_filename("{artist}", "", "Song", "", 1, "mp3"), "Song.mp3");
    }

    #[test]
    fn the_stem_is_capped_but_keeps_its_extension() {
        let long = "长".repeat(400);
        let name = render_filename("{title}", "", &long, "", 1, "flac");
        assert!(name.ends_with(".flac"));
        assert_eq!(name.chars().count(), MAX_STEM_CHARS + ".flac".len());
    }

    #[test]
    fn concurrency_is_clamped_into_range() {
        let mut config = DownloadConfig {
            concurrency: 99,
            ..Default::default()
        };
        config.normalize();
        assert_eq!(config.concurrency, MAX_CONCURRENCY);

        let mut config = DownloadConfig {
            concurrency: 0,
            ..Default::default()
        };
        config.normalize();
        assert_eq!(config.concurrency, 1);
    }

    #[test]
    fn the_segment_count_is_clamped_into_range() {
        // Zero is the shape a hand-edited file can arrive in, and it must read as
        // "do not split" rather than as "split into nothing". A file written
        // before this key existed gets the default instead, through the container's
        // `serde(default)`.
        let mut config = DownloadConfig {
            segments: 0,
            ..Default::default()
        };
        config.normalize();
        assert_eq!(config.segments, 1);

        let mut config = DownloadConfig {
            segments: 99,
            ..Default::default()
        };
        config.normalize();
        assert_eq!(config.segments, MAX_SEGMENTS);
    }

    #[test]
    fn a_blank_directory_reads_as_unset() {
        let mut config = DownloadConfig {
            dir: Some("   ".to_string()),
            ..Default::default()
        };
        config.normalize();
        assert!(config.dir.is_none());
    }
}
