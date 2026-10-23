//! Storage Access Framework bridge for the local music library.
//!
//! Three things about SAF cannot be done from Rust, and this plugin exists for
//! exactly those three:
//!
//! 1. **Persistable grants.** A directory picked through
//!    `ACTION_OPEN_DOCUMENT_TREE` is readable across restarts only after
//!    `ContentResolver.takePersistableUriPermission`. `tauri-plugin-fs` +
//!    `tauri-plugin-persisted-scope` do *not* cover this: that pair persists
//!    Tauri's own `fs::Scope` ACL, which has no relationship to the system's
//!    URI-permission table and does not touch a single Android API.
//! 2. **The picker itself.** `tauri-plugin-dialog`'s Android implementation
//!    sends `ACTION_GET_CONTENT` (its source still carries
//!    `// TODO: ACTION_OPEN_DOCUMENT ??`), which yields a grant that lasts until
//!    reboot and has no directory mode at all.
//! 3. **A decodable handle.** A `content://` document has no path, so the only
//!    way to hand it to symphonia is a file descriptor from
//!    `openFileDescriptor(uri, "r").detachFd()`.
//!
//! Every command here is called from Rust (`run_mobile_plugin`), never from JS.
//! That is deliberate: a Rust `#[tauri::command]` with the same name as a Kotlin
//! `@Command` silently wins the dispatch and the Kotlin side never runs, and an
//! ACL entry missing from `permissions/default.toml` makes a JS invoke fail with
//! no error anywhere. Keeping the frontend on the app's own `local_*` commands
//! avoids both.

use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

#[cfg(target_os = "android")]
use tauri::Manager;

use serde::{Deserialize, Serialize};

/// Kotlin package holding `LocalFilesPlugin`. Must stay in lockstep with the
/// `namespace` in `android/build.gradle.kts` and the package declaration in the
/// Kotlin sources — Tauri resolves the class as `<identifier>.<class>`.
#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "com.gbclstudio.gmplayer.localfiles";

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("SAF is only available on Android")]
    Unsupported,
    #[error("local-files plugin is not registered")]
    NotRegistered,
    #[error("{0}")]
    Plugin(String),
}

// ── URI shapes ───────────────────────────────────────────────────

/// The persistable grant that covers `uri`, if any.
///
/// A grant is always over a **tree** — `content://<authority>/tree/<id>` — and
/// that exact string is what [`LocalFiles::list_persisted`] reports back. But a
/// *subdirectory* of a grant is addressed as
/// `content://<authority>/tree/<id>/document/<child>`, which names the same grant
/// and is not equal to it, so a caller testing coverage by equality decides a
/// folder it can read perfectly well has gone away. Truncating at the `document`
/// segment is what turns one into the other.
///
/// That case is not hypothetical: the download folder is a `GMPlayer` child of a
/// grant on `Music`, because the platform refuses to grant `Download` at all from
/// target SDK 30 and a grant on the whole of `Music` is both too much and too slow
/// to write into.
///
/// `None` for anything not covered by a *tree* grant — a plain path, or the
/// single-document URI a picked file carries, which is granted in its own right
/// and must be matched exactly.
pub fn grant_uri_for(uri: &str) -> Option<&str> {
    if !uri.starts_with("content://") {
        return None;
    }
    const TREE: &str = "/tree/";
    const DOCUMENT: &str = "/document/";
    let after_tree = uri.find(TREE)? + TREE.len();
    // A document id is one percent-encoded segment, so the first `/document/`
    // after the tree id is the real separator and cannot occur inside the id.
    match uri[after_tree..].find(DOCUMENT) {
        Some(offset) => Some(&uri[..after_tree + offset]),
        None => Some(uri),
    }
}

// ── Wire types ───────────────────────────────────────────────────

/// A directory the user granted through `ACTION_OPEN_DOCUMENT_TREE`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedTree {
    /// The tree URI. Persisted as the source's locator.
    pub tree_uri: String,
    /// Human-readable name for the source list. Best effort.
    pub display_name: String,
}

/// One document from `ACTION_OPEN_DOCUMENT`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedFile {
    pub uri: String,
    pub display_name: String,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    /// Whether `takePersistableUriPermission` succeeded. A file the system
    /// refused to persist still plays *this session*, so it is reported rather
    /// than dropped — but it must not be written to the index as if it were
    /// durable.
    #[serde(default)]
    pub persisted: bool,
}

/// One entry found while walking a tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentEntry {
    /// Fully-qualified document URI, usable with `openFileDescriptor`.
    pub uri: String,
    pub display_name: String,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    /// Milliseconds since the epoch, as the provider reports it.
    #[serde(default)]
    pub last_modified: Option<i64>,
    /// Provider-relative directory path, joined with `/`. Drives the
    /// "by folder" grouping without a second query per file.
    #[serde(default)]
    pub relative_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumerateResult {
    pub entries: Vec<DocumentEntry>,
    /// Whether the walk stopped early (cancelled, or hit the entry cap).
    #[serde(default)]
    pub truncated: bool,
}

/// A descriptor `detachFd()`-ed on the Kotlin side. Ownership transfers with
/// this value: whoever receives it must close it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedFd {
    pub fd: i32,
    /// `false` for pipe-backed providers (some cloud document providers), on
    /// which `lseek` fails and symphonia's probe cannot run.
    #[serde(default)]
    pub is_regular_file: bool,
}

/// How a document is opened for writing.
///
/// String-valued over the wire, so the Kotlin side matches on a name rather than on
/// a pair of booleans that could be spelled in four ways and mean three things.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WriteMode {
    /// `"w"`. A provider does *not* truncate for plain `"w"` in every
    /// implementation, and `"wt"` is not universally supported either, so a caller
    /// that wants to start over asks for a fresh document instead of relying on
    /// either.
    Replace,
    /// `"wa"`. Continues an interrupted write in place, and the only mode under
    /// which a returned offset and the file position cannot disagree.
    Append,
    /// `"rw"`. Readable, writable and seekable — what rewriting a container in
    /// place needs, and the one mode a provider may simply refuse (a cloud
    /// document is not a file). Callers must treat that refusal as an outcome.
    Random,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedPermission {
    pub uri: String,
    #[serde(default)]
    pub read: bool,
    #[serde(default)]
    pub write: bool,
    /// Milliseconds since the epoch, from `UriPermission.getPersistedTime()`.
    /// The LRU release order when the per-app grant quota is reached.
    #[serde(default)]
    pub persisted_time: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadBytesResult {
    pub bytes: Vec<u8>,
    /// Whether the document was longer than `max_len` and got cut off.
    #[serde(default)]
    pub truncated: bool,
}

/// A text document the user picked, already decoded.
///
/// No persistable grant is taken for it: the bytes are read and copied on the
/// spot, so there is nothing to come back to later — and grants are a capped
/// per-app resource that a lyric file has no business consuming.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedText {
    pub display_name: String,
    pub text: String,
    /// Whether the document was longer than the read cap and got cut off.
    #[serde(default)]
    pub truncated: bool,
}

/// A document this app created inside a writable tree.
///
/// `uri` is the provider's answer, not a name we composed: a provider renames on
/// collision (`song (1).flac`), and it is the returned URI — in the same
/// `buildDocumentUriUsingTree` form the tree walk emits — that the local library
/// keys the track by.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedDocument {
    pub uri: String,
    pub display_name: String,
}

/// An existing root-level child of a tree, found by display name.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundDocument {
    pub uri: String,
    pub display_name: String,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    /// Milliseconds since the epoch, as the provider reports it.
    #[serde(default)]
    pub last_modified: Option<i64>,
}

// Kotlin's `Invoke.resolve` only takes an object, so every list-shaped answer
// arrives wrapped. These exist purely to unwrap it.
//
// Android-only, like the `call` methods that use them: off Android every command
// short-circuits to `Error::Unsupported` without a wire format.

#[cfg(target_os = "android")]
#[derive(Deserialize)]
struct PickTreeResponse {
    #[serde(default)]
    picked: Option<PickedTree>,
}

#[cfg(target_os = "android")]
#[derive(Deserialize)]
struct PickFilesResponse {
    #[serde(default)]
    files: Vec<PickedFile>,
}

#[cfg(target_os = "android")]
#[derive(Deserialize)]
struct PersistedResponse {
    #[serde(default)]
    permissions: Vec<PersistedPermission>,
}

#[cfg(target_os = "android")]
#[derive(Deserialize)]
struct ExistsResponse {
    #[serde(default)]
    exists: bool,
}

#[cfg(target_os = "android")]
#[derive(Deserialize)]
struct PickTextResponse {
    #[serde(default)]
    picked: Option<PickedText>,
}

#[cfg(target_os = "android")]
#[derive(Deserialize)]
struct FindDocumentResponse {
    #[serde(default)]
    found: Option<FoundDocument>,
}

#[cfg(target_os = "android")]
#[derive(Deserialize)]
struct DeleteResponse {
    #[serde(default)]
    #[allow(dead_code)]
    deleted: bool,
}

// ── Argument structs ─────────────────────────────────────────────

#[cfg(target_os = "android")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UriArgs<'a> {
    uri: &'a str,
}

#[cfg(target_os = "android")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateDocumentArgs<'a> {
    tree_uri: &'a str,
    display_name: &'a str,
    mime_type: Option<&'a str>,
}

#[cfg(target_os = "android")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FindDocumentArgs<'a> {
    tree_uri: &'a str,
    display_name: &'a str,
}

#[cfg(target_os = "android")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenWriteFdArgs<'a> {
    uri: &'a str,
    mode: WriteMode,
}

#[cfg(target_os = "android")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RenameDocumentArgs<'a> {
    uri: &'a str,
    display_name: &'a str,
}

#[cfg(target_os = "android")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EnumerateArgs<'a> {
    tree_uri: &'a str,
    max_entries: u32,
}

#[cfg(target_os = "android")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReadBytesArgs<'a> {
    uri: &'a str,
    max_len: u64,
}

#[cfg(target_os = "android")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PickWritableTreeArgs<'a> {
    initial_dir: Option<&'a str>,
}

#[cfg(target_os = "android")]
#[derive(Serialize)]
struct NoArgs {}

// ── Handle ───────────────────────────────────────────────────────

pub struct LocalFiles<R: Runtime> {
    #[cfg(target_os = "android")]
    handle: tauri::plugin::PluginHandle<R>,
    #[cfg(not(target_os = "android"))]
    #[allow(dead_code)]
    _marker: std::marker::PhantomData<R>,
}

#[cfg(target_os = "android")]
impl<R: Runtime> LocalFiles<R> {
    fn call<A: Serialize, T: serde::de::DeserializeOwned>(&self, cmd: &str, args: A) -> Result<T> {
        self.handle
            .run_mobile_plugin(cmd, args)
            .map_err(|e| Error::Plugin(e.to_string()))
    }

    /// Show the system directory picker and take a persistable read grant.
    ///
    /// Blocks until the user dismisses the picker: the Kotlin side answers from
    /// an `@ActivityCallback`, so the caller must not be the UI thread. Call it
    /// from a command, never from the audio path.
    pub fn pick_tree(&self) -> Result<Option<PickedTree>> {
        let out: PickTreeResponse = self.call("pickTree", NoArgs {})?;
        Ok(out.picked)
    }

    /// Show the system document picker (multi-select, audio MIME types) and take
    /// a persistable read grant per file.
    pub fn pick_files(&self) -> Result<Vec<PickedFile>> {
        let out: PickFilesResponse = self.call("pickFiles", NoArgs {})?;
        Ok(out.files)
    }

    /// [`pick_tree`](Self::pick_tree), but with a persistable **read+write**
    /// grant, and optionally opened at a public directory.
    ///
    /// Deliberately a second entry point rather than a flag: a folder imported
    /// into the library is one the user asked us to read, and holding a durable
    /// write grant over it is a different permission than the one they granted.
    /// Only the download flow calls this.
    ///
    /// `initial_dir` is a path relative to primary shared storage (`"Music"`) and
    /// is a *hint*: the AOSP picker honours it from API 26, some OEM ones ignore
    /// it, and the user confirms either way. It is worth passing because the
    /// obvious destination is not reachable at all — from target SDK 30 the
    /// platform refuses to grant `Download`, the internal storage root, or a
    /// reliable SD card root through this intent.
    ///
    /// Blocks until the user dismisses the picker.
    pub fn pick_writable_tree(&self, initial_dir: Option<&str>) -> Result<Option<PickedTree>> {
        let out: PickTreeResponse =
            self.call("pickWritableTree", PickWritableTreeArgs { initial_dir })?;
        Ok(out.picked)
    }

    /// Create a document at the root of `tree_uri`.
    ///
    /// The returned URI is the provider's, in the same form the tree walk emits,
    /// and is the only correct locator for the new file — see [`CreatedDocument`].
    pub fn create_document(
        &self,
        tree_uri: &str,
        display_name: &str,
        mime_type: Option<&str>,
    ) -> Result<CreatedDocument> {
        self.call(
            "createDocument",
            CreateDocumentArgs {
                tree_uri,
                display_name,
                mime_type,
            },
        )
    }

    /// Find a root-level child of `tree_uri` by display name.
    ///
    /// A tree cannot be `stat`-ed by name, so this is how "already downloaded"
    /// and "resume the partial file" are answered on Android.
    pub fn find_document(
        &self,
        tree_uri: &str,
        display_name: &str,
    ) -> Result<Option<FoundDocument>> {
        let out: FindDocumentResponse = self.call(
            "findDocument",
            FindDocumentArgs {
                tree_uri,
                display_name,
            },
        )?;
        Ok(out.found)
    }

    /// Open `uri` for writing in the given [`WriteMode`].
    ///
    /// The descriptor is owned by the caller, exactly as with
    /// [`open_fd`](Self::open_fd).
    pub fn open_write_fd(&self, uri: &str, mode: WriteMode) -> Result<OpenedFd> {
        self.call("openWriteFd", OpenWriteFdArgs { uri, mode })
    }

    /// Delete a document. Used to clear a half-written download.
    pub fn delete_document(&self, uri: &str) -> Result<()> {
        let _: DeleteResponse = self.call("deleteDocument", UriArgs { uri })?;
        Ok(())
    }

    /// Rename a document, returning the URI it has afterwards.
    ///
    /// How a finished download is published: bytes go to `<name>.part`, which no
    /// listing treats as audio, and the rename is the commit.
    pub fn rename_document(&self, uri: &str, display_name: &str) -> Result<CreatedDocument> {
        self.call(
            "renameDocument",
            RenameDocumentArgs { uri, display_name },
        )
    }

    /// Walk `tree_uri` and return every audio-ish document under it.
    ///
    /// One call for the whole tree. The walk is iterative on the Kotlin side and
    /// runs off the main thread; `cancel_enumerate` interrupts it.
    pub fn enumerate_tree(&self, tree_uri: &str, max_entries: u32) -> Result<EnumerateResult> {
        self.call(
            "enumerateTree",
            EnumerateArgs {
                tree_uri,
                max_entries,
            },
        )
    }

    /// Ask any in-flight enumeration to stop at its next entry.
    pub fn cancel_enumerate(&self) -> Result<()> {
        self.call("cancelEnumerate", NoArgs {})
    }

    /// Open `uri` for reading. The returned descriptor is owned by the caller.
    ///
    /// Blocking, and deliberately without a timeout. The decoder cannot proceed
    /// without these bytes, so there is nothing useful to do with an early
    /// return — and abandoning the call after Kotlin has already `detachFd()`-ed
    /// would leak the descriptor, which is the one failure here that compounds
    /// (a bulk scan reaches the per-process limit). The Kotlin side services
    /// this on a background executor so a slow document provider cannot wedge
    /// the Android main thread while we wait.
    pub fn open_fd(&self, uri: &str) -> Result<OpenedFd> {
        self.call("openFd", UriArgs { uri })
    }

    /// Read a small document whole — a sibling `.lrc`, a `cover.jpg`.
    ///
    /// Not for audio: that is what [`open_fd`](Self::open_fd) is for. Bytes
    /// cross as a JSON array, which is roughly 4× bloat and fine for kilobytes.
    pub fn read_bytes(&self, uri: &str, max_len: u64) -> Result<ReadBytesResult> {
        self.call("readBytes", ReadBytesArgs { uri, max_len })
    }

    /// Show the document picker for a *text* file and return its contents.
    ///
    /// Separate from [`pick_files`](Self::pick_files) for two reasons: the MIME
    /// filter is text rather than audio, and no persistable grant is taken — the
    /// bytes are read immediately, so a grant would be spent on something we will
    /// never open again.
    ///
    /// Blocks until the user dismisses the picker.
    pub fn pick_text_document(&self) -> Result<Option<PickedText>> {
        let out: PickTextResponse = self.call("pickTextDocument", NoArgs {})?;
        Ok(out.picked)
    }

    /// Whether `uri` still resolves to a readable document.
    ///
    /// A cheap `query` rather than an open, because the resolver asks this for
    /// every local track it plans.
    pub fn document_exists(&self, uri: &str) -> Result<bool> {
        let out: ExistsResponse = self.call("documentExists", UriArgs { uri })?;
        Ok(out.exists)
    }

    /// Every grant this app currently holds. Used at startup to reconcile the
    /// index against reality — a source whose grant is gone is marked
    /// unavailable, never deleted.
    pub fn list_persisted(&self) -> Result<Vec<PersistedPermission>> {
        let out: PersistedResponse = self.call("listPersisted", NoArgs {})?;
        Ok(out.permissions)
    }

    /// Give a grant back. Used by "remove source" and by the LRU that keeps the
    /// per-app grant count under the system quota.
    pub fn release_uri(&self, uri: &str) -> Result<()> {
        self.call("releaseUri", UriArgs { uri })
    }
}

#[cfg(not(target_os = "android"))]
impl<R: Runtime> LocalFiles<R> {
    pub fn pick_tree(&self) -> Result<Option<PickedTree>> {
        Err(Error::Unsupported)
    }
    pub fn pick_files(&self) -> Result<Vec<PickedFile>> {
        Err(Error::Unsupported)
    }
    pub fn pick_writable_tree(&self, _initial_dir: Option<&str>) -> Result<Option<PickedTree>> {
        Err(Error::Unsupported)
    }
    pub fn create_document(
        &self,
        _tree_uri: &str,
        _display_name: &str,
        _mime_type: Option<&str>,
    ) -> Result<CreatedDocument> {
        Err(Error::Unsupported)
    }
    pub fn find_document(
        &self,
        _tree_uri: &str,
        _display_name: &str,
    ) -> Result<Option<FoundDocument>> {
        Err(Error::Unsupported)
    }
    pub fn open_write_fd(&self, _uri: &str, _mode: WriteMode) -> Result<OpenedFd> {
        Err(Error::Unsupported)
    }
    pub fn delete_document(&self, _uri: &str) -> Result<()> {
        Err(Error::Unsupported)
    }
    pub fn rename_document(&self, _uri: &str, _display_name: &str) -> Result<CreatedDocument> {
        Err(Error::Unsupported)
    }
    pub fn enumerate_tree(&self, _tree_uri: &str, _max_entries: u32) -> Result<EnumerateResult> {
        Err(Error::Unsupported)
    }
    pub fn cancel_enumerate(&self) -> Result<()> {
        Err(Error::Unsupported)
    }
    pub fn open_fd(&self, _uri: &str) -> Result<OpenedFd> {
        Err(Error::Unsupported)
    }
    pub fn read_bytes(&self, _uri: &str, _max_len: u64) -> Result<ReadBytesResult> {
        Err(Error::Unsupported)
    }
    pub fn pick_text_document(&self) -> Result<Option<PickedText>> {
        Err(Error::Unsupported)
    }
    pub fn document_exists(&self, _uri: &str) -> Result<bool> {
        Err(Error::Unsupported)
    }
    pub fn list_persisted(&self) -> Result<Vec<PersistedPermission>> {
        Err(Error::Unsupported)
    }
    pub fn release_uri(&self, _uri: &str) -> Result<()> {
        Err(Error::Unsupported)
    }
}

/// Reach the plugin from any Tauri manager.
pub trait LocalFilesExt<R: Runtime> {
    /// `None` on platforms where SAF does not exist, so callers branch on
    /// capability rather than on `cfg`.
    fn local_files(&self) -> Option<&LocalFiles<R>>;
}

#[cfg(target_os = "android")]
impl<R: Runtime, T: tauri::Manager<R>> LocalFilesExt<R> for T {
    fn local_files(&self) -> Option<&LocalFiles<R>> {
        self.try_state::<LocalFiles<R>>().map(|state| state.inner())
    }
}

#[cfg(not(target_os = "android"))]
impl<R: Runtime, T: tauri::Manager<R>> LocalFilesExt<R> for T {
    fn local_files(&self) -> Option<&LocalFiles<R>> {
        None
    }
}

/// Initialize the plugin. A no-op registration off Android, so the host app
/// needs no `cfg` gate around the `.plugin(...)` call.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("local-files")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            {
                let handle = _api.register_android_plugin(PLUGIN_IDENTIFIER, "LocalFilesPlugin")?;
                _app.manage(LocalFiles { handle });
            }
            Ok(())
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subdirectory_resolves_to_the_grant_that_covers_it() {
        // What `ACTION_OPEN_DOCUMENT_TREE` grants, and what `list_persisted`
        // reports.
        let grant = "content://com.android.externalstorage.documents/tree/primary%3AMusic";
        // What addressing `Music/GMPlayer` under that grant produces.
        let child = format!("{grant}/document/primary%3AMusic%2FGMPlayer");
        assert_eq!(grant_uri_for(&child), Some(grant));
        // A grant resolves to itself, so one test covers both callers.
        assert_eq!(grant_uri_for(grant), Some(grant));
    }

    #[test]
    fn a_tree_id_containing_the_word_document_is_not_mistaken_for_a_separator() {
        let grant = "content://com.android.externalstorage.documents/tree/primary%3Adocuments";
        assert_eq!(grant_uri_for(grant), Some(grant));
        let child = format!("{grant}/document/primary%3Adocuments%2FGMPlayer");
        assert_eq!(grant_uri_for(&child), Some(grant));
    }

    #[test]
    fn anything_not_covered_by_a_tree_grant_answers_none() {
        // A picked file is granted in its own right and must be matched exactly —
        // answering a grant here would report it available whenever *any* tree
        // grant existed.
        assert_eq!(
            grant_uri_for("content://com.android.providers.media.documents/document/audio%3A42"),
            None
        );
        assert_eq!(grant_uri_for("I:\\Music\\GMPlayer"), None);
        assert_eq!(grant_uri_for("/storage/emulated/0/Music"), None);
        assert_eq!(grant_uri_for(""), None);
    }
}
