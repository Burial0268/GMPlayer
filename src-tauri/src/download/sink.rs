//! The write target for one download.
//!
//! Two shapes behind one set of functions: a real directory (desktop) and a SAF
//! tree the user granted read+write (Android). The difference is entirely here;
//! `queue` and `fetch` never branch on platform.
//!
//! # `.part` is the commit protocol
//!
//! Bytes always go to `<name>.part` and the file is renamed onto its real name
//! only once the last byte has landed. `part` is not in
//! `local::scan::AUDIO_EXTENSIONS` and not in the Kotlin walk's
//! `CANDIDATE_EXTENSIONS`, so a partial file is invisible to the library — which
//! matters because the library indexes by walking, and an interrupted download
//! left under its final name would be indexed as a track that is a truncated
//! file. It is also what makes resume possible: the partial file *is* the
//! progress record, so nothing has to be persisted alongside it.
//!
//! # A split download is n part files, not one file with holes
//!
//! Downloading over several `Range` streams at once means bytes arriving out of
//! order, and the usual answer — preallocate the whole file, `pwrite` each stream
//! at its own offset, keep a control file on the side — breaks the sentence above
//! twice over. The length would stop being the progress record, and a process
//! killed mid-transfer would leave a full-length file full of holes that the next
//! run reads as *finished* and publishes: a silently corrupt track rather than a
//! wasted transfer.
//!
//! So each segment gets its own file, `<name>.part<i>of<n>.<size>`, and each one
//! is appended to exactly like the single `.part` always was. The invariant is not
//! replaced, it is repeated n times: **every part's length is its own progress
//! record**, so a resume needs nothing but the files themselves. `<size>` is in
//! the name because it is the only other thing the set depends on — the spans are
//! derived from it, so bytes fetched against one size must never be adopted by a
//! download of another (the same track at a different `br` has the same filename
//! whenever the container matches).
//!
//! What it costs is the concatenation in [`commit_parts`], and an unsplit download
//! keeps the plain `.part` name so that path is byte-for-byte what it was.


use std::fs::File;

use tauri::{AppHandle, Runtime};
use tauri_plugin_local_files::WriteMode;

use super::config::DownloadTarget;
use super::tags;

/// Something already sitting at the destination.
#[derive(Debug, Clone)]
pub struct Existing {
    pub locator: String,
    pub size: Option<u64>,
}

/// An open partial file, positioned to append.
pub struct PartFile {
    pub file: File,
    /// Bytes already present — where this part resumes from.
    pub offset: u64,
    /// Locator of the partial file, so a failure can delete exactly it.
    pub locator: String,
    /// How many bytes this part should end up holding.
    ///
    /// `None` only for an unsplit download, where the wire behaviour is
    /// deliberately what it always was: an open-ended `Range` and truncation
    /// detected from `Content-Length` rather than from a length we asked for.
    pub want: Option<u64>,
}

/// The partial files one download is being assembled from.
///
/// Exactly one entry unless the download was split, and the first entry is the
/// one the others are concatenated into.
pub struct Parts {
    pub files: Vec<PartFile>,
    /// The name the set takes on commit.
    pub final_name: String,
}

impl Parts {
    /// Bytes already on disk across the whole set.
    pub fn received(&self) -> u64 {
        self.files.iter().map(|part| part.offset).sum()
    }

    /// Every part's locator, for a caller that has to throw the set away.
    pub fn locators(&self) -> Vec<String> {
        self.files.iter().map(|part| part.locator.clone()).collect()
    }

    /// The finished file's size, when the parts between them declare it.
    fn declared_size(&self) -> Option<u64> {
        self.files.iter().map(|part| part.want).sum()
    }
}

/// A committed download.
#[derive(Debug, Clone)]
pub struct Published {
    /// The library's key for this file.
    pub locator: String,
    /// What the destination actually called it — a provider renames on collision.
    pub display_name: String,
    /// The size it has as published, which is *after* tagging: embedding a cover
    /// and a lyric grows the file, and a size taken before that would put a length
    /// the file no longer has into the library index.
    pub size: u64,
}

fn part_name(name: &str) -> String {
    format!("{name}.part")
}

/// The `index`-th of `n` partial files for a download of `size` bytes.
///
/// `n == 1` is the plain `.part`, so an unsplit download uses the same name it
/// always did and a partial file left by an older build still resumes. See the
/// module header for why `size` is in the split name.
fn segment_name(name: &str, index: usize, n: usize, size: u64) -> String {
    if n <= 1 {
        part_name(name)
    } else {
        format!("{name}.part{index}of{n}.{size}")
    }
}

/// The MIME handed to `createDocument`, and it is deliberately one nothing knows.
///
/// A `DocumentsProvider` does not simply accept the display name you ask for. On
/// the way through `FileUtils.buildUniqueFile` it compares the name's extension
/// against the MIME you passed, and **on a mismatch it appends the extension the
/// MIME maps to**. Passing `audio/flac` for `Song.flac.part` therefore creates
/// `Song.flac.part.flac` — an audio-shaped name, which is exactly the partial file
/// the `.part` scheme exists to hide from the library walk. `text/plain` for
/// `Song.lrc` is the same trap in the other direction: `.lrc` is not in Android's
/// MIME table, so the sidecar would land as `Song.lrc.txt` and never pair with its
/// track.
///
/// The escape is that the mismatch branch uses `getExtensionFromMimeType`, which
/// answers `null` for a MIME the platform does not know — and a `null` extension
/// leaves the name untouched. So an unregistered vendor type is not a placeholder
/// here, it is the mechanism: it cannot map to an extension, therefore the name we
/// ask for is the name we get.
///
/// Nothing is lost by it. `ExternalStorageProvider` derives a document's reported
/// MIME from its extension when queried, so the finished file still describes
/// itself correctly.
const CREATE_MIME: &str = "application/vnd.gmplayer.opaque";

/// Open the partial files for `name`, continuing whatever is already on disk.
///
/// `wants` is one entry per segment: the length that segment has to end up with,
/// or a single `None` for an unsplit download whose size the server never
/// declared. The names are derived from it (see [`segment_name`]), so a set fetched
/// against a different size is never adopted — it is simply not found.
///
/// A set left inconsistent by a crash *inside* [`commit_parts`] is thrown away and
/// started again rather than reported. Part 0 is the concatenation target, so a
/// part 0 longer than its own span carries no record of how much of the tail it
/// already absorbed and there is nothing to resume from; losing those bytes is the
/// only terminating answer, and the alternative is a task that fails the same way
/// forever.
pub fn open_parts<R: Runtime>(
    app: &AppHandle<R>,
    target: &DownloadTarget,
    name: &str,
    wants: &[Option<u64>],
) -> Result<Parts, String> {
    for _ in 0..2 {
        let parts = Parts {
            files: open_all(app, target, name, wants)?,
            final_name: name.to_string(),
        };
        if parts_are_usable(&parts) {
            return Ok(parts);
        }
        log::warn!(
            target: "download",
            "the partial files for {name} do not add up; starting the transfer again"
        );
        // The handles go before the documents do: Windows refuses to remove a file
        // that is still open.
        let locators = parts.locators();
        drop(parts);
        for locator in &locators {
            discard(app, target, locator);
        }
    }
    Err("the partial files for this download could not be reset".to_string())
}

/// Whether the set can be continued from as it stands.
///
/// Only a split set can fail this. Every part is append-only and `fetch::stream`
/// clamps what it writes to the span it asked for, so the one way a part ends up
/// longer than its span is [`commit_parts`] concatenating into part 0 and not
/// finishing. A part 0 that reached the full size is the exception: that is a
/// concatenation that completed and a rename that did not, which is the same
/// recoverable state an unsplit `.part` at full size has always been left in.
fn parts_are_usable(parts: &Parts) -> bool {
    if parts.files.len() <= 1 {
        return true;
    }
    if parts.declared_size() == Some(parts.files[0].offset) {
        return true;
    }
    parts
        .files
        .iter()
        .all(|part| part.offset <= part.want.unwrap_or(u64::MAX))
}

fn open_all<R: Runtime>(
    app: &AppHandle<R>,
    target: &DownloadTarget,
    name: &str,
    wants: &[Option<u64>],
) -> Result<Vec<PartFile>, String> {
    /// An empty request is an unsplit download. Nothing should ask for it, and
    /// answering with no parts at all would leave the caller with nothing to
    /// concatenate into.
    const WHOLE: [Option<u64>; 1] = [None];
    let wants = if wants.is_empty() { &WHOLE[..] } else { wants };

    let n = wants.len();
    let size: u64 = wants.iter().flatten().sum();
    let mut files = Vec::with_capacity(n);
    for (index, want) in wants.iter().enumerate() {
        let part = segment_name(name, index, n, size);
        files.push(open_one(app, target, &part, *want)?);
    }
    Ok(files)
}

/// Open one partial file, creating it if it is not there.
///
/// Always opened in append mode, on both platforms, so the returned offset and
/// the file position cannot disagree: with `O_APPEND` (`"wa"` on the SAF side) a
/// write goes to the end no matter what any intervening seek did.
fn open_one<R: Runtime>(
    app: &AppHandle<R>,
    target: &DownloadTarget,
    part: &str,
    want: Option<u64>,
) -> Result<PartFile, String> {
    match target {
        DownloadTarget::Path(dir) => {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            let path = dir.join(part);
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|e| format!("could not open {}: {e}", path.display()))?;
            let offset = file.metadata().map(|meta| meta.len()).unwrap_or(0);
            Ok(PartFile {
                file,
                offset,
                locator: path.to_string_lossy().to_string(),
                want,
            })
        }
        DownloadTarget::Tree(tree) => {
            let plugin = plugin(app)?;
            let (uri, offset) = match plugin.find_document(tree, part).map_err(|e| e.to_string())? {
                Some(doc) => (doc.uri, doc.size.unwrap_or(0)),
                None => {
                    // See [`CREATE_MIME`]: the sentinel type is what keeps the
                    // provider from welding an audio extension onto the partial
                    // file's name and thereby making it visible to the walk.
                    let created = plugin
                        .create_document(tree, part, Some(CREATE_MIME))
                        .map_err(|e| e.to_string())?;
                    (created.uri, 0)
                }
            };
            let opened = plugin
                .open_write_fd(&uri, WriteMode::Append)
                .map_err(|e| e.to_string())?;
            let file = adopt_fd(opened.fd)?;
            Ok(PartFile {
                file,
                offset,
                locator: uri,
                want,
            })
        }
    }
}

/// Whatever is already sitting under `name` at the destination, if anything.
///
/// On Android this costs one children query — a tree has no `stat` by name — and
/// it is what answers both "already downloaded, skip it" and "resume this".
pub fn probe<R: Runtime>(
    app: &AppHandle<R>,
    target: &DownloadTarget,
    name: &str,
) -> Result<Option<Existing>, String> {
    match target {
        DownloadTarget::Path(dir) => {
            let path = dir.join(name);
            match std::fs::metadata(&path) {
                Ok(meta) if meta.is_file() => Ok(Some(Existing {
                    locator: path.to_string_lossy().to_string(),
                    size: Some(meta.len()),
                })),
                _ => Ok(None),
            }
        }
        DownloadTarget::Tree(tree) => {
            let plugin = plugin(app)?;
            let found = plugin
                .find_document(tree, name)
                .map_err(|e| e.to_string())?;
            Ok(found.map(|doc| Existing {
                locator: doc.uri,
                size: doc.size,
            }))
        }
    }
}

/// Bytes sitting in the *unsplit* `<name>.part`, if any.
///
/// A split download uses different filenames, so this is what tells the caller an
/// earlier unsplit attempt left a resumable prefix behind. Those bytes are worth
/// more than the split is: splitting anyway would mean re-fetching every one of
/// them, since the parts of a split set are addressed by span and a prefix is not
/// one of them.
pub fn unsplit_progress<R: Runtime>(
    app: &AppHandle<R>,
    target: &DownloadTarget,
    name: &str,
) -> Result<u64, String> {
    let found = probe(app, target, &part_name(name))?;
    Ok(found.and_then(|part| part.size).unwrap_or(0))
}

/// Publish a finished download: concatenate its parts and rename onto
/// `final_name`.
///
/// Blocking, and the concatenation is the whole price of having split the
/// transfer: `(n-1)/n` of the file is copied into part 0. On Linux and Android
/// `std::io::copy` specialises a File→File copy to `copy_file_range`, so those
/// bytes never reach user space; on Windows it is a buffered copy of a few tens of
/// megabytes, which against a transfer measured in tens of seconds is noise. An
/// unsplit download copies nothing at all.
///
/// `replacing` is the locator of a file already under that name, which the
/// caller learned from [`probe`]. It has to be removed first on both platforms
/// and for different reasons: `std::fs::rename` fails outright on Windows when
/// the destination exists, and a `DocumentsProvider` succeeds but renames to
/// `name (1)` — which would leave the old file in the library and add a second
/// row for the new one.
pub fn commit_parts<R: Runtime>(
    app: &AppHandle<R>,
    target: &DownloadTarget,
    parts: Parts,
    replacing: Option<&str>,
    tags: Option<&tags::Tags>,
) -> Result<Published, String> {
    let head_offset = parts.files.first().map(|part| part.offset).unwrap_or(0);
    // A part 0 already holding the whole file is a concatenation that finished and a
    // rename that did not, so there is nothing left to join.
    let merged = parts.files.len() > 1 && parts.declared_size() == Some(head_offset);
    let Parts {
        mut files,
        final_name,
    } = parts;
    let tail = files.split_off(1);
    let head = files
        .pop()
        .ok_or_else(|| "this download has no partial file to publish".to_string())?;
    let PartFile {
        mut file,
        locator,
        offset,
        want,
    } = head;

    if !merged {
        verify_length(offset, want)?;
        for part in &tail {
            verify_length(part.offset, part.want)?;
        }
        for part in &tail {
            append_onto(app, target, &mut file, &part.locator)?;
        }
    }
    // Flush before the rename, not after: a rename that beats the contents to
    // disk leaves a correctly-named truncated file, which is worse than none
    // because it looks valid — the same reason `local::index::write_atomic`
    // fsyncs.
    file.sync_all().map_err(|e| e.to_string())?;
    let joined_size = file.metadata().map(|meta| meta.len()).unwrap_or(offset);
    // The append handle goes before the tagging one arrives: an append-only handle
    // cannot be read from or seeked, which is exactly what rewriting a container
    // needs, so `tags` gets a handle of its own on the same locator.
    drop(file);
    let size = match tags.filter(|tags| !tags.is_empty()) {
        Some(tags) => embed(app, target, &locator, tags).unwrap_or(joined_size),
        None => joined_size,
    };

    // The handles go before the documents do: Windows refuses to remove a file that
    // is still open, and a tail part left behind is a copy of a slice of the track
    // sitting in the user's music folder for good.
    let joined: Vec<String> = tail.iter().map(|part| part.locator.clone()).collect();
    drop(tail);

    // Renaming before deleting is deliberate. A rename that fails leaves part 0
    // holding the whole file, which [`parts_are_usable`] recognises and the retry
    // commits from — where deleting first and failing second would leave nothing to
    // try again with.
    let (locator, display_name) = rename_onto(app, target, &locator, &final_name, replacing)?;
    for locator in &joined {
        discard(app, target, locator);
    }
    Ok(Published {
        locator,
        display_name,
        size,
    })
}

/// Write `tags` into the joined file, answering its new length.
///
/// Best-effort by construction, and the caller keeps the untagged length when this
/// answers `None`: every byte of audio has already landed, so a container lofty does
/// not recognise, or a provider that will not open a document read-write, must not
/// turn a finished download into a failed one. See [`tags`] for the rest of that
/// argument.
fn embed<R: Runtime>(
    app: &AppHandle<R>,
    target: &DownloadTarget,
    locator: &str,
    tags: &tags::Tags,
) -> Option<u64> {
    let mut file = match read_write_handle(app, target, locator) {
        Ok(file) => file,
        Err(err) => {
            log::warn!(target: "download", "cannot tag {locator} in place: {err}");
            return None;
        }
    };
    match tags::write(&mut file, tags) {
        Ok(size) => Some(size),
        Err(err) => {
            log::warn!(target: "download", "could not tag {locator}: {err}");
            None
        }
    }
}

/// A readable, writable, seekable handle on a part file.
///
/// The `"rw"` mode is the one a `DocumentsProvider` is entitled to refuse — a
/// cloud-backed document is not a file — so this is the call whose failure the
/// tagging path treats as an outcome rather than an error.
fn read_write_handle<R: Runtime>(
    app: &AppHandle<R>,
    target: &DownloadTarget,
    locator: &str,
) -> Result<File, String> {
    match target {
        DownloadTarget::Path(_) => std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(locator)
            .map_err(|e| format!("could not reopen {locator}: {e}")),
        DownloadTarget::Tree(_) => {
            let plugin = plugin(app)?;
            let opened = plugin
                .open_write_fd(locator, WriteMode::Random)
                .map_err(|e| e.to_string())?;
            if !opened.is_regular_file {
                // A pipe cannot be seeked, and lofty rewrites the container around
                // the tag rather than streaming it.
                let _ = adopt_fd(opened.fd);
                return Err("this document provider hands out a pipe".to_string());
            }
            adopt_fd(opened.fd)
        }
    }
}

/// A part that is not exactly as long as its span was truncated or over-sent, and
/// concatenating it would splice the wrong bytes into the middle of a file that
/// would otherwise look perfectly valid. Refused here, with every part still on
/// disk, so the retry resumes rather than restarts.
fn verify_length(offset: u64, want: Option<u64>) -> Result<(), String> {
    match want {
        Some(want) if offset != want => {
            Err(format!("a downloaded part holds {offset} bytes, not {want}"))
        }
        _ => Ok(()),
    }
}

/// Append the bytes of the part at `locator` onto `head`.
fn append_onto<R: Runtime>(
    app: &AppHandle<R>,
    target: &DownloadTarget,
    head: &mut File,
    locator: &str,
) -> Result<(), String> {
    let mut source = read_handle(app, target, locator)?;
    std::io::copy(&mut source, head)
        .map_err(|e| format!("could not join the downloaded parts: {e}"))?;
    Ok(())
}

/// A read handle on a part file.
///
/// The handles this module hands out are append-only and therefore not readable,
/// so the concatenation reopens: by path on desktop, and through the plugin's own
/// `openFd` on a tree — the same descriptor hand-off the decoder uses on the read
/// path, and the reason nothing here needs a `"rw"` mode a document provider might
/// not support.
fn read_handle<R: Runtime>(
    app: &AppHandle<R>,
    target: &DownloadTarget,
    locator: &str,
) -> Result<File, String> {
    match target {
        DownloadTarget::Path(_) => {
            File::open(locator).map_err(|e| format!("could not read {locator}: {e}"))
        }
        DownloadTarget::Tree(_) => {
            let plugin = plugin(app)?;
            let opened = plugin.open_fd(locator).map_err(|e| e.to_string())?;
            adopt_fd(opened.fd)
        }
    }
}

fn rename_onto<R: Runtime>(
    app: &AppHandle<R>,
    target: &DownloadTarget,
    locator: &str,
    final_name: &str,
    replacing: Option<&str>,
) -> Result<(String, String), String> {
    match target {
        DownloadTarget::Path(dir) => {
            let final_path = dir.join(final_name);
            if replacing.is_some() || final_path.exists() {
                let _ = std::fs::remove_file(&final_path);
            }
            std::fs::rename(locator, &final_path)
                .map_err(|e| format!("could not publish {}: {e}", final_path.display()))?;
            Ok((
                final_path.to_string_lossy().to_string(),
                final_name.to_string(),
            ))
        }
        DownloadTarget::Tree(_) => {
            let plugin = plugin(app)?;
            if let Some(existing) = replacing {
                let _ = plugin.delete_document(existing);
            }
            let renamed = plugin
                .rename_document(locator, final_name)
                .map_err(|e| e.to_string())?;
            Ok((renamed.uri, renamed.display_name))
        }
    }
}

/// Throw away a partial file.
///
/// Only for a *failure the user cannot resume from* — a cancel keeps the `.part`
/// so pressing retry does not start the transfer over.
pub fn discard<R: Runtime>(app: &AppHandle<R>, target: &DownloadTarget, locator: &str) {
    match target {
        DownloadTarget::Path(_) => {
            let _ = std::fs::remove_file(locator);
        }
        DownloadTarget::Tree(_) => {
            if let Ok(plugin) = plugin(app) {
                let _ = plugin.delete_document(locator);
            }
        }
    }
}

/// Write a small sibling file — a `.lrc`, a `cover.jpg` — replacing any existing
/// one.
///
/// Not routed through the `.part` protocol: these are kilobytes written in one
/// call, so there is no window worth protecting, and a lyric file is not
/// something the library would index as a broken track.
pub fn write_sibling<R: Runtime>(
    app: &AppHandle<R>,
    target: &DownloadTarget,
    name: &str,
    bytes: &[u8],
) -> Result<String, String> {
    match target {
        DownloadTarget::Path(dir) => {
            let path = dir.join(name);
            std::fs::write(&path, bytes)
                .map_err(|e| format!("could not write {}: {e}", path.display()))?;
            Ok(path.to_string_lossy().to_string())
        }
        DownloadTarget::Tree(tree) => {
            let plugin = plugin(app)?;
            // Replaced rather than opened: a provider appends `(1)` instead of
            // overwriting, and `"w"` is not reliably truncating across providers.
            if let Some(existing) = plugin.find_document(tree, name).map_err(|e| e.to_string())? {
                let _ = plugin.delete_document(&existing.uri);
            }
            // [`CREATE_MIME`] rather than a real type: `.lrc` is not in Android's
            // MIME table, so `text/plain` would land the file as `<stem>.lrc.txt`
            // and it would never pair with its track.
            let created = plugin
                .create_document(tree, name, Some(CREATE_MIME))
                .map_err(|e| e.to_string())?;
            let opened = plugin
                .open_write_fd(&created.uri, WriteMode::Replace)
                .map_err(|e| e.to_string())?;
            {
                use std::io::Write;
                let mut file = adopt_fd(opened.fd)?;
                file.write_all(bytes).map_err(|e| e.to_string())?;
                file.flush().map_err(|e| e.to_string())?;
            }
            Ok(created.uri)
        }
    }
}

fn plugin<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<&tauri_plugin_local_files::LocalFiles<R>, String> {
    use tauri_plugin_local_files::LocalFilesExt;
    app.local_files()
        .ok_or_else(|| "a directory grant is only available on Android".to_string())
}

/// Take ownership of a descriptor the Kotlin side has already `detachFd()`-ed.
///
/// Nothing else will ever close it — the JVM gave it up — so the `File` returned
/// here is the only owner, exactly as on the read path in `audio-backend`.
#[cfg(unix)]
fn adopt_fd(fd: i32) -> Result<File, String> {
    use std::os::fd::FromRawFd;
    if fd < 0 {
        return Err("the document provider returned no descriptor".to_string());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(not(unix))]
fn adopt_fd(_fd: i32) -> Result<File, String> {
    // Unreachable: the only caller is the SAF branch, and `local_files()` is
    // `None` everywhere but Android. Present so the module compiles on Windows.
    Err("descriptor hand-off is Android-only".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_partial_file_is_named_so_no_listing_indexes_it() {
        let part = part_name("Artist - Title.flac");
        assert_eq!(part, "Artist - Title.flac.part");
        // Both listings decide by the *last* extension, which is the whole point.
        let extension = part.rsplit_once('.').unwrap().1;
        assert!(!crate::local::scan::AUDIO_EXTENSIONS.contains(&extension));
        assert!(!crate::local::scan::LYRIC_EXTENSIONS.contains(&extension));
    }

    #[test]
    fn a_split_download_names_its_parts_so_no_listing_indexes_them_either() {
        let name = "Artist - Title.flac";
        // One segment is the plain `.part`, so an unsplit download keeps the name
        // it always had and a partial file from an older build still resumes.
        assert_eq!(segment_name(name, 0, 1, 4096), part_name(name));
        let part = segment_name(name, 2, 4, 10_485_760);
        assert_eq!(part, "Artist - Title.flac.part2of4.10485760");
        let extension = part.rsplit_once('.').unwrap().1;
        assert!(!crate::local::scan::AUDIO_EXTENSIONS.contains(&extension));
        assert!(!crate::local::scan::LYRIC_EXTENSIONS.contains(&extension));
    }

    #[test]
    fn parts_fetched_against_another_size_are_a_different_set() {
        // The same track at a different `br` keeps the same filename whenever the
        // container matches, and a different size means different spans — so the
        // names have to differ too, or a resume would splice two encodings
        // together and produce a file that looks perfectly valid.
        assert_ne!(
            segment_name("Song.mp3", 1, 4, 10_000_000),
            segment_name("Song.mp3", 1, 4, 6_000_000)
        );
    }

    #[test]
    fn a_part_that_does_not_fill_its_span_is_refused() {
        assert!(verify_length(100, Some(100)).is_ok());
        assert!(verify_length(99, Some(100)).is_err());
        assert!(verify_length(101, Some(100)).is_err());
        // An unsplit download declares no length here; `fetch` catches a short body
        // against `Content-Length` instead.
        assert!(verify_length(7, None).is_ok());
    }

    #[test]
    fn the_create_mime_is_one_the_platform_cannot_map_to_an_extension() {
        // Everything in [`CREATE_MIME`] rests on `getExtensionFromMimeType`
        // answering `null`, which it does for any type absent from Android's own
        // table — and a vendor tree is the one namespace the platform is
        // guaranteed never to have registered. A real type here would be mapped,
        // and `Song.flac.part` would come back as `Song.flac.part.flac`, which is
        // an audio-shaped name and therefore exactly the partial file the `.part`
        // scheme exists to hide from the library walk.
        assert!(CREATE_MIME.starts_with("application/vnd."));
        // And not one of the trees Android does know, in both directions.
        for known in ["audio/", "text/", "video/", "image/"] {
            assert!(!CREATE_MIME.starts_with(known));
        }
    }
}
