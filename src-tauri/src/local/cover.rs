//! Extracted cover art on disk.
//!
//! Content-addressed: the filename is a digest of the image *source* bytes, so
//! the twelve tracks of an album that each embed the same JPEG share one file
//! instead of twelve copies. It also makes writing idempotent — a re-scan that
//! finds the same picture does no I/O at all.
//!
//! Stored **downscaled**. Embedded art is routinely 1500-3000px, and unlike a
//! Netease cover there is no `?param=WxH` to lean on: the file on disk is what
//! every consumer decodes, at whatever size it was written. A 3000px JPEG being
//! decoded for a 60px list avatar costs tens of milliseconds *per row scrolled
//! into view* and once more on every track change, all of it on the WebView's
//! main thread. Paying one decode+encode per unique picture at scan time — on the
//! scan's worker threads, where it is parallel and nobody is waiting — is the
//! trade the plan document called for.
//!
//! 512px is sized for the largest consumer, which is the OS media session
//! (notification, lock screen, SMTC), not the in-app views.

use std::path::{Path, PathBuf};

use image::ImageEncoder;
use sha2::{Digest, Sha256};

use gmplayer_audio_backend::CoverArt;

/// Subdirectory of the app cache holding extracted covers.
///
/// Under the *cache* directory, not app data: these are derived files, and a
/// system that reclaims the cache costs a re-scan of covers rather than the
/// user's library. It is also the directory named in `tauri.conf.json`'s
/// `assetProtocol.scope`, which is what lets `convertFileSrc` reach them —
/// that scope is static and narrow on purpose, so no dynamic ACL (and therefore
/// no `tauri-plugin-persisted-scope`) is involved.
pub const COVER_DIR: &str = "local-covers";

/// Where spooled `content://` bodies land. Same reasoning: derived, disposable.
pub const STREAM_CACHE_DIR: &str = "local-stream";

/// Longest edge of a stored cover, in pixels.
const MAX_COVER_EDGE: u32 = 512;

/// JPEG quality for a re-encoded cover. 85 is the usual "no visible loss at
/// display size" point; at 512px the file lands around 40-80 KB.
const COVER_QUALITY: u8 = 85;

/// Bumped when the *derivation* changes, not when the source bytes do.
///
/// The digest describes the input, so without this an already-cached full-size
/// cover from an earlier version would keep satisfying the existence check and
/// never be replaced. With it, a re-scan writes new names and [`prune`] removes
/// the old ones as unreferenced.
const COVER_REVISION: &str = "r1";

pub fn cover_dir(cache_root: &Path) -> PathBuf {
    cache_root.join(COVER_DIR)
}

/// Store `art` and return its cache filename.
///
/// Returns `None` rather than an error when the write fails: a missing cover is
/// a cosmetic loss, and failing the scan over one would strand the whole import.
pub fn store(dir: &Path, art: &CoverArt) -> Option<String> {
    if art.data.is_empty() {
        return None;
    }

    let mut hasher = Sha256::new();
    hasher.update(&art.data);
    let digest = hasher.finalize();

    // Decode once, here, so the name can carry the format actually written.
    // A picture we cannot decode (an exotic or truncated one) is stored verbatim
    // rather than dropped — that is the pre-existing behaviour and it is still
    // better than no cover.
    let derived = downscale(&art.data);
    let (bytes, ext): (&[u8], &str) = match &derived {
        Some(reduced) => (&reduced.data, reduced.extension),
        None => (&art.data, extension_for(&art.media_type, &art.data)),
    };
    let name = format!("{}-{COVER_REVISION}.{ext}", hex16(&digest));

    let path = dir.join(&name);
    if path.exists() {
        return Some(name);
    }

    if let Err(err) = std::fs::create_dir_all(dir) {
        log::warn!(target: "local", "cover cache directory is unusable: {err}");
        return None;
    }
    // Same temp-then-rename as the index: a torn write here would be served to
    // an `<img>` as a corrupt image forever, since the name is content-derived
    // and would never be rewritten.
    let temp = path.with_extension("part");
    if let Err(err) = std::fs::write(&temp, bytes) {
        log::warn!(target: "local", "could not write cover art: {err}");
        return None;
    }
    if let Err(err) = std::fs::rename(&temp, &path) {
        log::warn!(target: "local", "could not publish cover art: {err}");
        let _ = std::fs::remove_file(&temp);
        return None;
    }
    Some(name)
}

struct Derived {
    data: Vec<u8>,
    extension: &'static str,
}

/// Decode, shrink to [`MAX_COVER_EDGE`] and re-encode.
///
/// `None` means "store the original": the picture is already small enough, or it
/// is in a format the build cannot decode (WebP and GIF art both land here — the
/// `image` features enabled are `jpeg` and `png`, which is what taggers actually
/// write).
fn downscale(data: &[u8]) -> Option<Derived> {
    let decoded = image::load_from_memory(data).ok()?;
    if decoded.width().max(decoded.height()) <= MAX_COVER_EDGE {
        return None;
    }

    // Triangle rather than Lanczos3: this is a large *reduction*, where the
    // difference is invisible at display size and the cost is not — a scan can
    // hit hundreds of unique covers.
    let thumb = decoded.resize(
        MAX_COVER_EDGE,
        MAX_COVER_EDGE,
        image::imageops::FilterType::Triangle,
    );

    // JPEG cannot carry alpha, and flattening it would put album art with a
    // transparent background on black. Rare, but it would be a visible
    // regression, so those stay PNG.
    if decoded.color().has_alpha() {
        let rgba = thumb.to_rgba8();
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(
                rgba.as_raw(),
                rgba.width(),
                rgba.height(),
                image::ExtendedColorType::Rgba8,
            )
            .ok()?;
        return Some(Derived {
            data: out,
            extension: "png",
        });
    }

    let rgb = thumb.to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, COVER_QUALITY)
        .encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .ok()?;
    Some(Derived {
        data: out,
        extension: "jpg",
    })
}

/// Delete covers no live track references any more.
pub fn prune(dir: &Path, referenced: &std::collections::HashSet<String>) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if referenced.contains(&name) {
            continue;
        }
        if std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

fn hex16(digest: &[u8]) -> String {
    digest
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Whether `data` starts like an image file.
///
/// Magic bytes rather than a decode, deliberately: the build enables only the
/// `jpeg` and `png` features of `image`, so a perfectly good WebP or GIF fails
/// `load_from_memory` — and [`store`] already handles those by writing them
/// verbatim, which the WebView renders fine. A decode-based check would reject
/// exactly the formats the pass-through branch exists for.
///
/// The point is to refuse a *non*-image: a `.txt` renamed to `.jpg`, or an HTML
/// error page. Storing one would leave a permanently broken `<img>` behind a
/// content-addressed name that nothing will ever rewrite.
pub fn looks_like_image(data: &[u8]) -> bool {
    matches!(
        data,
        [0xff, 0xd8, 0xff, ..]                                          // JPEG
            | [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, ..]      // PNG
            | [b'G', b'I', b'F', b'8', ..]                              // GIF
            | [b'B', b'M', ..]                                          // BMP
    ) || matches!(
        data,
        // WebP and the ISO-BMFF family (AVIF, HEIC) both carry their marker
        // after a length field, so they cannot be matched by a prefix alone.
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..]
            | [_, _, _, _, b'f', b't', b'y', b'p', ..]
    )
}

/// Pick an extension from the declared MIME type, falling back to the magic
/// bytes — plenty of taggers write `image/jpg`, `JPG`, or nothing at all, and
/// the extension is what the WebView and `BitmapFactory` sniff first.
fn extension_for(media_type: &str, data: &[u8]) -> &'static str {
    let normalized = media_type.trim().to_ascii_lowercase();
    if normalized.contains("png") {
        return "png";
    }
    if normalized.contains("webp") {
        return "webp";
    }
    if normalized.contains("gif") {
        return "gif";
    }
    if normalized.contains("jpeg") || normalized.contains("jpg") {
        return "jpg";
    }
    match data {
        [0x89, b'P', b'N', b'G', ..] => "png",
        [b'G', b'I', b'F', ..] => "gif",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "webp",
        _ => "jpg",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_prefers_the_declared_type() {
        assert_eq!(extension_for("image/png", &[]), "png");
        assert_eq!(extension_for("image/jpeg", &[]), "jpg");
        // Taggers write this often enough to matter.
        assert_eq!(extension_for("image/jpg", &[]), "jpg");
        assert_eq!(extension_for("IMAGE/WEBP", &[]), "webp");
    }

    #[test]
    fn extension_falls_back_to_magic_bytes() {
        assert_eq!(extension_for("", &[0x89, b'P', b'N', b'G', 0x0d]), "png");
        assert_eq!(extension_for("application/octet-stream", &[0xff, 0xd8]), "jpg");
        assert_eq!(
            extension_for("", b"RIFF\0\0\0\0WEBPVP8 "),
            "webp"
        );
    }

    #[test]
    fn identical_art_is_stored_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let art = CoverArt {
            media_type: "image/jpeg".into(),
            data: vec![1, 2, 3, 4],
        };
        let first = store(dir.path(), &art).expect("stored");
        let second = store(dir.path(), &art).expect("stored");
        assert_eq!(first, second);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    /// The four bytes above are not a decodable image, which is the pass-through
    /// branch. This pins the branch that matters for jank: a large picture comes
    /// back smaller, as a JPEG, and re-storing it is still idempotent.
    #[test]
    fn a_large_cover_is_downscaled_and_reencoded() {
        let dir = tempfile::tempdir().expect("tempdir");
        let big = image::RgbImage::from_fn(1400, 1400, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(big)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode source");

        let art = CoverArt {
            media_type: "image/png".into(),
            data: png,
        };
        let name = store(dir.path(), &art).expect("stored");
        assert!(name.ends_with(".jpg"), "opaque art re-encodes as jpeg: {name}");

        let written = std::fs::read(dir.path().join(&name)).expect("read back");
        let stored = image::load_from_memory(&written).expect("decode stored");
        // The pixel count is the assertion that matters: it is what every
        // consumer's decode cost scales with. Byte size deliberately is not —
        // a synthetic gradient compresses unlike real album art either way.
        assert_eq!(stored.width().max(stored.height()), MAX_COVER_EDGE);
        assert_eq!(store(dir.path(), &art).as_deref(), Some(name.as_str()));
    }

    /// Flattening a transparent cover onto black would be a visible regression,
    /// so those keep an alpha-capable format.
    #[test]
    fn transparent_art_stays_lossless() {
        let dir = tempfile::tempdir().expect("tempdir");
        let big = image::RgbaImage::from_fn(900, 900, |x, _| {
            image::Rgba([255, 0, 0, (x % 256) as u8])
        });
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(big)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode source");

        let name = store(
            dir.path(),
            &CoverArt {
                media_type: "image/png".into(),
                data: png,
            },
        )
        .expect("stored");
        assert!(name.ends_with(".png"), "alpha art stays png: {name}");
    }

    /// A cover we cannot decode must still be stored, at its original size —
    /// dropping it would leave the track with no art at all.
    #[test]
    fn undecodable_art_falls_back_to_the_original_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = b"RIFF\0\0\0\0WEBPVP8 not-really".to_vec();
        let name = store(
            dir.path(),
            &CoverArt {
                media_type: "image/webp".into(),
                data: data.clone(),
            },
        )
        .expect("stored");
        assert!(name.ends_with(".webp"), "{name}");
        assert_eq!(std::fs::read(dir.path().join(name)).unwrap(), data);
    }

    #[test]
    fn empty_art_is_not_stored() {
        let dir = tempfile::tempdir().expect("tempdir");
        let art = CoverArt {
            media_type: "image/jpeg".into(),
            data: vec![],
        };
        assert!(store(dir.path(), &art).is_none());
    }

    #[test]
    fn prune_keeps_referenced_covers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let kept = store(
            dir.path(),
            &CoverArt {
                media_type: "image/jpeg".into(),
                data: vec![1],
            },
        )
        .expect("stored");
        store(
            dir.path(),
            &CoverArt {
                media_type: "image/jpeg".into(),
                data: vec![2],
            },
        )
        .expect("stored");

        let referenced = [kept.clone()].into_iter().collect();
        assert_eq!(prune(dir.path(), &referenced), 1);
        assert!(dir.path().join(kept).exists());
    }
}
