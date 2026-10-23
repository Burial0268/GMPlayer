//! Imported lyric files.
//!
//! Under **app data**, not the cache: a lyric a person went and found is not
//! something a re-scan can reproduce, so it belongs next to the playlists and
//! favourites rather than next to the extracted covers. It is also why the text
//! is a file of its own instead of a field in `local-user-data.json` — that file
//! is rewritten every time a heart is pressed, and a few thousand lines of TTML
//! per track would make one press a multi-megabyte write.
//!
//! Named from the track *key*, so re-importing replaces rather than accumulates.
//! The file is not the identity; [`super::model::LyricImport`] in the user-data
//! file is, and this directory is prunable against it.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::index::fnv1a64;
use super::model::LyricKind;

/// Subdirectory of `$APPDATA` holding imported lyric files.
pub const LYRIC_DIR: &str = "local-lyrics";

/// Cap on an imported lyric. TTML with per-word timing is the largest real
/// format and lands in the tens of kilobytes; a megabyte is something else
/// entirely and is refused rather than stored.
pub const MAX_LYRIC_BYTES: usize = 1024 * 1024;

pub fn lyric_dir(data_root: &Path) -> PathBuf {
    data_root.join(LYRIC_DIR)
}

/// Extension for a kind. Word-timed formats all keep `.yrc` on disk: the
/// frontend picks the exact parser from the content, and inventing three
/// extensions here would imply a distinction this side does not actually make.
fn extension_for(kind: LyricKind) -> &'static str {
    match kind {
        LyricKind::Ttml => "ttml",
        LyricKind::Word => "yrc",
        LyricKind::Lrc => "lrc",
    }
}

/// Which of a track's lyric documents a file holds.
///
/// One track can have three at once — the lyric, its translation, its romanisation
/// — and they are separate documents because Netease serves them separately and
/// the frontend's parser aligns them line by line at render time. The variant is
/// in the *middle* of the name so the extension stays last, which is what
/// [`LyricKind::from_extension`] and every listing read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Main,
    Translation,
    Romanisation,
}

impl Variant {
    fn infix(self) -> &'static str {
        match self {
            // Empty, so a main lyric keeps the name it has had since this directory
            // existed and nothing has to be migrated.
            Variant::Main => "",
            Variant::Translation => "t.",
            Variant::Romanisation => "r.",
        }
    }
}

/// The filename an import for `key` gets.
pub fn file_name_for(key: &str, kind: LyricKind, variant: Variant) -> String {
    format!(
        "{:016x}.{}{}",
        fnv1a64(key.as_bytes()),
        variant.infix(),
        extension_for(kind)
    )
}

/// Write `text` for `key` and return the filename it was stored under.
///
/// Temp-then-rename, like the index: a torn write here would be parsed as a
/// truncated lyric on every load, and the name is derived so nothing would ever
/// rewrite it.
pub fn store(
    dir: &Path,
    key: &str,
    kind: LyricKind,
    variant: Variant,
    text: &str,
) -> std::io::Result<String> {
    if text.len() > MAX_LYRIC_BYTES {
        return Err(std::io::Error::other(format!(
            "lyric is {} bytes, over the {MAX_LYRIC_BYTES} limit",
            text.len()
        )));
    }
    std::fs::create_dir_all(dir)?;
    let name = file_name_for(key, kind, variant);
    let path = dir.join(&name);
    let temp = path.with_extension("part");
    std::fs::write(&temp, text.as_bytes())?;
    if let Err(err) = std::fs::rename(&temp, &path) {
        let _ = std::fs::remove_file(&temp);
        return Err(err);
    }
    Ok(name)
}

pub fn read(dir: &Path, file: &str) -> Option<String> {
    // `file` comes from our own user-data file, but that file is documented as
    // hand-editable, so a name that tries to climb out of the directory must not
    // be followed.
    if file.contains('/') || file.contains('\\') || file.contains("..") {
        log::warn!(target: "local", "refusing a lyric filename that is not a plain name: {file}");
        return None;
    }
    std::fs::read_to_string(dir.join(file)).ok()
}

pub fn remove(dir: &Path, file: &str) {
    if file.contains('/') || file.contains('\\') || file.contains("..") {
        return;
    }
    let _ = std::fs::remove_file(dir.join(file));
}

/// Delete lyric files no import references any more.
pub fn prune(dir: &Path, referenced: &HashSet<String>) -> usize {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_maps_to_one_extension_each_way() {
        assert_eq!(LyricKind::from_extension(".LRC"), LyricKind::Lrc);
        assert_eq!(LyricKind::from_extension("ttml"), LyricKind::Ttml);
        assert_eq!(LyricKind::from_extension("qrc"), LyricKind::Word);
        assert_eq!(LyricKind::from_extension("eslrc"), LyricKind::Word);
        assert_eq!(LyricKind::from_extension("yrc"), LyricKind::Word);
        // Anything unknown degrades to LRC rather than being refused.
        assert_eq!(LyricKind::from_extension("txt"), LyricKind::Lrc);
    }

    #[test]
    fn re_importing_replaces_rather_than_accumulates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let key = "D:\\Music\\a.flac";
        let first = store(dir.path(), key, LyricKind::Lrc, Variant::Main, "[00:01.00]one").expect("store");
        let second = store(dir.path(), key, LyricKind::Lrc, Variant::Main, "[00:02.00]two").expect("store");
        assert_eq!(first, second);
        assert_eq!(read(dir.path(), &second).as_deref(), Some("[00:02.00]two"));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    /// Switching format leaves a differently-named file behind, which is exactly
    /// what `prune` is for — the record only ever names one.
    #[test]
    fn prune_drops_files_no_import_references() {
        let dir = tempfile::tempdir().expect("tempdir");
        let key = "/music/a.flac";
        let lrc = store(dir.path(), key, LyricKind::Lrc, Variant::Main, "[00:01.00]x").expect("store");
        let ttml = store(dir.path(), key, LyricKind::Ttml, Variant::Main, "<tt/>").expect("store");
        assert_ne!(lrc, ttml);

        let referenced = [ttml.clone()].into_iter().collect();
        assert_eq!(prune(dir.path(), &referenced), 1);
        assert!(dir.path().join(ttml).exists());
    }

    #[test]
    fn a_traversing_name_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(read(dir.path(), "../local-user-data.json").is_none());
    }

    /// The three documents of one track share a hash and differ only by the infix,
    /// so they cannot collide and the extension stays last — which is what
    /// `LyricKind::from_extension` and the prune both read.
    #[test]
    fn each_variant_of_one_track_gets_its_own_name() {
        let key = "/music/a.flac";
        let main = file_name_for(key, LyricKind::Lrc, Variant::Main);
        let translation = file_name_for(key, LyricKind::Lrc, Variant::Translation);
        let romanisation = file_name_for(key, LyricKind::Lrc, Variant::Romanisation);
        assert_eq!(main, format!("{:016x}.lrc", fnv1a64(key.as_bytes())));
        assert_eq!(translation, format!("{:016x}.t.lrc", fnv1a64(key.as_bytes())));
        assert_eq!(romanisation, format!("{:016x}.r.lrc", fnv1a64(key.as_bytes())));
        for name in [&translation, &romanisation] {
            assert!(name.ends_with(".lrc"));
            assert_ne!(name, &main);
        }
    }

    #[test]
    fn an_oversized_lyric_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let huge = "x".repeat(MAX_LYRIC_BYTES + 1);
        assert!(store(dir.path(), "k", LyricKind::Lrc, Variant::Main, &huge).is_err());
        assert!(!dir.path().join(file_name_for("k", LyricKind::Lrc, Variant::Main)).exists());
    }
}
