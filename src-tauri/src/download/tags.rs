//! Writing what Netease knows into the file Netease sent.
//!
//! A streaming source from NCM carries **no artwork and no lyric**, and routinely
//! no title either — so a freshly downloaded track has nothing but its filename to
//! describe it. That costs twice over: other players show it as unknown, and *our
//! own* library derives its row from `local::scan`'s symphonia tag probe, which
//! finds the same nothing and falls back to parsing the name. Everything written
//! here is data the queue already had in hand for the sidecars.
//!
//! Symphonia cannot help with this half — it reads tags and has no writer — hence
//! `lofty`, which is the only new dependency the download queue has.
//!
//! # Why the tags go on before the rename
//!
//! `save_to` rewrites the container, and a rewrite interrupted half way leaves a
//! file that no longer decodes. Doing it while the bytes are still under `.part`
//! keeps that failure invisible to the library — the whole point of the `.part`
//! protocol — and it means the size the index records is the size *after* tagging
//! rather than a length the file no longer has.
//!
//! # Everything here is best-effort
//!
//! A container lofty does not recognise, a picture it rejects, a `content://`
//! document whose provider will not open read-write: none of those are reasons to
//! fail a download that has already landed every byte. They are logged and the
//! track is published untagged, which is exactly what it would have been before.

use std::fs::File;
use std::io::Seek;

use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::tag::{Accessor, ItemKey, ItemValue, Tag, TagItem};

/// What the queue knows that the downloaded file does not.
#[derive(Debug, Clone, Default)]
pub struct Tags {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Plain LRC, exactly as the `.lrc` sidecar would have got it.
    pub lyric: Option<String>,
    /// Cover art bytes, already fetched for the sidecar.
    pub cover: Option<Vec<u8>>,
}

impl Tags {
    /// Whether there is anything worth rewriting the file for.
    pub fn is_empty(&self) -> bool {
        self.title.trim().is_empty()
            && self.artist.trim().is_empty()
            && self.album.trim().is_empty()
            && self.lyric.is_none()
            && self.cover.is_none()
    }
}

/// Write `tags` into `file`, returning the length it has afterwards.
///
/// The handle has to be readable, writable and seekable: lofty reads the container
/// to find out where the tag belongs and then rewrites the file around it. That is
/// the one requirement the rest of this module's handles do not meet, which is why
/// `sink` opens a separate one for this and why the SAF side needed a `"rw"` mode.
pub fn write(file: &mut File, tags: &Tags) -> Result<u64, String> {
    let mut tagged = lofty::read_from(file).map_err(|err| format!("unreadable container: {err}"))?;
    let kind = tagged.primary_tag_type();
    if tagged.primary_tag().is_none() {
        // A streaming source usually arrives with no tag of any kind, so the common
        // case is creating the primary one for the container rather than editing it.
        tagged.insert_tag(Tag::new(kind));
    }
    let tag = tagged
        .primary_tag_mut()
        .ok_or_else(|| format!("{kind:?} accepts no tag"))?;

    // Only fields with something in them: an empty string is a *worse* answer than
    // an absent tag, because a player shows it as a blank line where it would
    // otherwise fall back to the filename.
    if !tags.title.trim().is_empty() {
        tag.set_title(tags.title.trim().to_string());
    }
    if !tags.artist.trim().is_empty() {
        tag.set_artist(tags.artist.trim().to_string());
    }
    if !tags.album.trim().is_empty() {
        tag.set_album(tags.album.trim().to_string());
    }
    if let Some(lyric) = tags.lyric.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
        // The timestamps are kept. `ItemKey::Lyrics` lands in ID3v2's `USLT` and in
        // a Vorbis comment's `LYRICS`, and both are where players look for a synced
        // lyric — an unsynced copy would throw away the only thing that makes it
        // useful.
        tag.insert(TagItem::new(
            ItemKey::Lyrics,
            ItemValue::Text(lyric.to_string()),
        ));
    }
    if let Some(cover) = tags.cover.as_deref().filter(|bytes| !bytes.is_empty()) {
        match picture_mime(cover) {
            Some(mime) => tag.push_picture(Picture::new_unchecked(
                PictureType::CoverFront,
                Some(mime),
                None,
                cover.to_vec(),
            )),
            // Netease serves JPEG, so anything else is a redirect to something that
            // is not an image at all rather than an exotic format worth supporting.
            None => log::warn!(target: "download", "cover art is neither JPEG nor PNG; not embedding it"),
        }
    }

    // `save_to` seeks for itself, but it is handed a file whose cursor `read_from`
    // left somewhere in the middle, and rewinding first costs nothing next to being
    // wrong about that.
    file.rewind().map_err(|err| err.to_string())?;
    tagged
        .save_to(file, WriteOptions::default())
        .map_err(|err| format!("could not write the tags: {err}"))?;
    file.sync_all().map_err(|err| err.to_string())?;
    file.metadata()
        .map(|meta| meta.len())
        .map_err(|err| err.to_string())
}

/// The picture's type, from its own bytes rather than from the URL it came from.
///
/// A cover URL's extension is a claim by whoever built the link; the magic number
/// is the file. Getting it wrong writes a picture frame that declares one format
/// and holds another, which players either ignore or draw as a broken image.
fn picture_mime(bytes: &[u8]) -> Option<MimeType> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(MimeType::Jpeg);
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(MimeType::Png);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_typed_from_its_bytes_not_its_url() {
        assert_eq!(picture_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(MimeType::Jpeg));
        assert_eq!(picture_mime(b"\x89PNG\r\n\x1a\n\x00"), Some(MimeType::Png));
        // An HTML error page served where a cover was expected.
        assert_eq!(picture_mime(b"<!DOCTYPE html>"), None);
        assert_eq!(picture_mime(&[]), None);
    }

    #[test]
    fn nothing_to_say_means_nothing_to_rewrite() {
        assert!(Tags::default().is_empty());
        // Whitespace is not a title. Rewriting a multi-megabyte file to store a
        // blank line is the one case where doing nothing is strictly better.
        let blank = Tags {
            title: "   ".to_string(),
            ..Default::default()
        };
        assert!(blank.is_empty());
        let real = Tags {
            title: "Song".to_string(),
            ..Default::default()
        };
        assert!(!real.is_empty());
    }

    /// A silent MP3, built by hand because the repository carries no audio.
    ///
    /// MPEG-1 Layer III, 128 kbps, 44.1 kHz stereo: `FF FB 90 00` then 413 zero
    /// bytes makes one 417-byte frame. Several of them, because a format probe
    /// looks for consecutive sync words rather than trusting the first one it sees.
    fn silent_mp3() -> Vec<u8> {
        const FRAME: usize = 417;
        let mut out = Vec::with_capacity(FRAME * 24);
        for _ in 0..24 {
            out.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
            out.resize(out.len() + FRAME - 4, 0);
        }
        out
    }

    /// The contract the whole feature rests on: what lofty writes, symphonia reads.
    ///
    /// Two crates with no relationship to each other agreeing on `USLT` and on an
    /// `APIC` frame is the entire reason embedding is worth doing — it is what makes
    /// `local_lyric_for`'s embedded branch and the library's title/artist/album find
    /// anything at all. A test on our side of that boundary only would pass while
    /// the feature did nothing.
    #[test]
    fn what_is_embedded_is_what_the_library_reads_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("track.mp3");
        std::fs::write(&path, silent_mp3()).expect("the synthetic mp3");

        let cover = b"\xFF\xD8\xFFtiny jpeg".to_vec();
        let tags = Tags {
            title: "标题".to_string(),
            artist: "歌手".to_string(),
            album: "专辑".to_string(),
            lyric: Some("[00:01.00]第一行\n[00:02.00]第二行".to_string()),
            cover: Some(cover.clone()),
        };
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .expect("a read-write handle");
        let size = write(&mut file, &tags).expect("the tags");
        drop(file);
        assert_eq!(size, std::fs::metadata(&path).expect("the file").len());

        let (_, read_back, _) =
            gmplayer_audio_backend::extract_track_tags(&path, true).expect("the probe");
        assert_eq!(read_back.title.as_deref(), Some("标题"));
        assert_eq!(read_back.artist.as_deref(), Some("歌手"));
        assert_eq!(read_back.album.as_deref(), Some("专辑"));
        assert_eq!(read_back.lyrics.as_deref(), tags.lyric.as_deref());
        assert_eq!(read_back.cover.map(|art| art.data), Some(cover));
    }
}
