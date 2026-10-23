package com.gbclstudio.gmplayer.localfiles

import android.app.Activity
import android.content.Intent
import android.database.Cursor
import android.net.Uri
import android.provider.DocumentsContract
import android.system.Os
import android.system.OsConstants
import android.util.Log
import androidx.activity.result.ActivityResult
import androidx.appcompat.app.AppCompatActivity
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.util.ArrayDeque
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

@InvokeArg
class UriArgs {
    lateinit var uri: String
}

@InvokeArg
class EnumerateArgs {
    lateinit var treeUri: String
    var maxEntries: Int = 0
}

@InvokeArg
class ReadBytesArgs {
    lateinit var uri: String
    var maxLen: Long = 0
}

@InvokeArg
class CreateDocumentArgs {
    lateinit var treeUri: String
    lateinit var displayName: String
    var mimeType: String? = null
}

@InvokeArg
class FindDocumentArgs {
    lateinit var treeUri: String
    lateinit var displayName: String
}

@InvokeArg
class OpenWriteFdArgs {
    lateinit var uri: String

    /**
     * One of the Rust side's `WriteMode` names.
     *
     * `append` opens `"wa"` so an interrupted download can be continued in place.
     * `replace` opens `"w"`, which truncates — a provider does *not* truncate for
     * plain `"w"` in every implementation, but `"wt"` is not universally supported
     * either, so the Rust side asks for a fresh document when it wants to start
     * over rather than relying on either. `random` opens `"rw"`, which is the only
     * mode that is readable and seekable as well and therefore the only one a
     * container rewrite can use — and the only one a provider may simply refuse,
     * since a cloud-backed document is not a file.
     *
     * Defaulted rather than `lateinit` so an argument object without it still
     * parses to the mode every existing caller wanted.
     */
    var mode: String = "append"
}

@InvokeArg
class RenameDocumentArgs {
    lateinit var uri: String
    lateinit var displayName: String
}

@InvokeArg
class PickWritableTreeArgs {
    /**
     * A public directory to open the picker at, relative to primary storage —
     * `"Music"`, say. A hint and nothing more: the user still confirms, and can
     * navigate anywhere they like from there.
     *
     * Only worth setting because the alternative is worse. `EXTRA_INITIAL_URI`
     * is honoured by the AOSP picker (API 26+) and ignored by some OEM ones, so
     * a null or unrecognised value costs nothing but the default landing spot.
     */
    var initialDir: String? = null
}

/**
 * Storage Access Framework bridge.
 *
 * Two rules shape everything here.
 *
 * **Nothing blocking runs on the main thread.** Tauri services a mobile plugin
 * call on the Android main thread and the Rust caller blocks on the reply with
 * no timeout, so a `ContentResolver` round trip to a slow document provider
 * (a cloud one, an unmounted SD card) would stall the UI *and* whichever audio
 * thread is waiting. Every command that touches the resolver hands off to [io]
 * and resolves its `Invoke` from there — `Invoke.resolve` is safe from any
 * thread.
 *
 * **A descriptor has exactly one owner.** `openFd` calls `detachFd()`, after
 * which the JVM will not close it; Rust adopts it into a `File` on the first
 * line of the branch that receives it. Any error path here that has already
 * detached must close the descriptor itself, because nobody else will — and a
 * per-file leak reaches the process fd limit during a several-hundred-track
 * scan, which surfaces as "the import worked and then everything failed at
 * once".
 */
@TauriPlugin
class LocalFilesPlugin(private val activity: Activity) : Plugin(activity) {

    private val io = Executors.newCachedThreadPool { runnable ->
        Thread(runnable, "local-files-io").apply { isDaemon = true }
    }

    private val cancelEnumeration = AtomicBoolean(false)

    // ── Pickers ─────────────────────────────────────────────────────────

    @Command
    fun pickTree(invoke: Invoke) {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
            addFlags(
                Intent.FLAG_GRANT_READ_URI_PERMISSION or
                    Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
            )
        }
        startActivityForResult(invoke, intent, "onTreePicked")
    }

    @ActivityCallback
    fun onTreePicked(invoke: Invoke, result: ActivityResult) {
        finishTreePick(invoke, result, Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }

    /**
     * Like [pickTree], but takes a **read+write** persistable grant, and can open
     * the picker somewhere useful.
     *
     * A separate command rather than a flag on [pickTree] on purpose: a folder
     * imported into the library is something the user asked us to *read*, and
     * quietly holding a durable write grant over their whole music collection is
     * not the same permission. This one is only reached from the download flow,
     * where writing is the point.
     *
     * `initialDir` exists because the destination people ask for cannot be
     * reached any other way. From target SDK 30 the platform refuses to grant
     * `Download` — and the internal storage root, and every reliable SD card
     * root — through `ACTION_OPEN_DOCUMENT_TREE` at all: the directory is listed
     * and its confirm button is greyed out. `Music` is not on that list, so the
     * best available "standard place for downloaded music" is a grant there, and
     * landing the picker on it is what turns choosing one into a single tap.
     */
    @Command
    fun pickWritableTree(invoke: Invoke) {
        val args = invoke.parseArgs(PickWritableTreeArgs::class.java)
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
            addFlags(
                Intent.FLAG_GRANT_READ_URI_PERMISSION or
                    Intent.FLAG_GRANT_WRITE_URI_PERMISSION or
                    Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
            )
            publicDirectoryUri(args.initialDir)?.let {
                putExtra(DocumentsContract.EXTRA_INITIAL_URI, it)
            }
        }
        startActivityForResult(invoke, intent, "onWritableTreePicked")
    }

    /**
     * A document URI naming `relative` under primary shared storage, for use as
     * a picker starting point.
     *
     * Composed rather than queried because there is nothing to query: the app
     * holds no grant on that folder yet, which is the entire reason the picker is
     * being opened. `ExternalStorageProvider` spells its document ids
     * `primary:<relative path>`, so this is a *guess at* an id and the picker is
     * free to ignore it — hence best-effort, never an error.
     */
    private fun publicDirectoryUri(relative: String?): Uri? {
        val trimmed = relative?.trim()?.trim('/') ?: return null
        if (trimmed.isEmpty()) return null
        return try {
            DocumentsContract.buildDocumentUri(EXTERNAL_STORAGE_AUTHORITY, "primary:$trimmed")
        } catch (e: Exception) {
            Log.w(TAG, "could not compose an initial URI for $trimmed", e)
            null
        }
    }

    @ActivityCallback
    fun onWritableTreePicked(invoke: Invoke, result: ActivityResult) {
        finishTreePick(
            invoke,
            result,
            Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
        )
    }

    private fun finishTreePick(invoke: Invoke, result: ActivityResult, flags: Int) {
        val uri = if (result.resultCode == Activity.RESULT_OK) result.data?.data else null
        if (uri == null) {
            invoke.resolve(JSObject().apply { put("picked", null) })
            return
        }

        // Without this the grant dies at the next reboot, which is the entire
        // difference between "a music library" and "files that worked once".
        try {
            activity.contentResolver.takePersistableUriPermission(uri, flags)
        } catch (e: SecurityException) {
            Log.w(TAG, "tree grant is not persistable: $uri", e)
            invoke.reject("directory permission could not be made permanent")
            return
        }

        val picked = JSObject().apply {
            put("treeUri", uri.toString())
            put("displayName", treeDisplayName(uri))
        }
        invoke.resolve(JSObject().apply { put("picked", picked) })
    }

    @Command
    fun pickFiles(invoke: Invoke) {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = "*/*"
            putExtra(Intent.EXTRA_MIME_TYPES, PICKER_MIME_TYPES)
            putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
            addFlags(
                Intent.FLAG_GRANT_READ_URI_PERMISSION or
                    Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
            )
        }
        startActivityForResult(invoke, intent, "onFilesPicked")
    }

    @ActivityCallback
    fun onFilesPicked(invoke: Invoke, result: ActivityResult) {
        val files = JSArray()
        if (result.resultCode != Activity.RESULT_OK) {
            invoke.resolve(JSObject().apply { put("files", files) })
            return
        }

        val data = result.data
        val uris = ArrayList<Uri>()
        data?.clipData?.let { clip ->
            for (i in 0 until clip.itemCount) {
                clip.getItemAt(i).uri?.let { uris.add(it) }
            }
        }
        if (uris.isEmpty()) {
            data?.data?.let { uris.add(it) }
        }

        io.execute {
            for (uri in uris) {
                // Per-file grants count against a per-app quota (historically
                // 128, 512 on newer releases, lower on some ROMs). A refusal is
                // reported rather than thrown away: the file still plays this
                // session, it just must not be written to the index as durable.
                val persisted = try {
                    activity.contentResolver.takePersistableUriPermission(
                        uri,
                        Intent.FLAG_GRANT_READ_URI_PERMISSION
                    )
                    true
                } catch (e: SecurityException) {
                    Log.w(TAG, "file grant is not persistable: $uri", e)
                    false
                }

                val meta = queryDocument(uri)
                files.put(JSObject().apply {
                    put("uri", uri.toString())
                    put("displayName", meta?.displayName ?: lastSegmentName(uri))
                    put("mimeType", meta?.mimeType)
                    meta?.size?.let { put("size", it) }
                    put("persisted", persisted)
                })
            }
            invoke.resolve(JSObject().apply { put("files", files) })
        }
    }

    // ── Enumeration ─────────────────────────────────────────────────────

    @Command
    fun enumerateTree(invoke: Invoke) {
        val args = invoke.parseArgs(EnumerateArgs::class.java)
        val cap = if (args.maxEntries > 0) args.maxEntries else DEFAULT_MAX_ENTRIES
        cancelEnumeration.set(false)

        io.execute {
            try {
                invoke.resolve(walkTree(Uri.parse(args.treeUri), cap))
            } catch (e: SecurityException) {
                // The grant is gone: the user revoked it in system settings, or
                // the volume was unmounted. Not an error the UI should treat as
                // a crash — the source is marked unavailable and kept.
                Log.w(TAG, "tree is no longer accessible: ${args.treeUri}", e)
                invoke.reject("permission-lost")
            } catch (e: Exception) {
                Log.e(TAG, "enumerateTree failed", e)
                invoke.reject(e.message ?: "enumerateTree failed")
            }
        }
    }

    @Command
    fun cancelEnumerate(invoke: Invoke) {
        cancelEnumeration.set(true)
        invoke.resolve()
    }

    /**
     * Iterative, never recursive: a deep tree would otherwise overflow the
     * stack, and the depth is the user's directory layout rather than anything
     * we control. Android's own SAF guidance warns that walking a large tree
     * degrades, so the walk is also capped and cancellable.
     */
    private fun walkTree(treeUri: Uri, cap: Int): JSObject {
        val resolver = activity.contentResolver
        val entries = JSArray()
        var truncated = false
        var count = 0

        val stack = ArrayDeque<Frame>()
        stack.push(Frame(addressedDocumentId(treeUri), ""))

        while (stack.isNotEmpty()) {
            if (cancelEnumeration.get()) {
                truncated = true
                break
            }
            val frame = stack.pop()
            val childrenUri =
                DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, frame.documentId)

            val cursor: Cursor = resolver.query(childrenUri, CHILD_PROJECTION, null, null, null)
                ?: continue
            cursor.use { c ->
                while (c.moveToNext()) {
                    if (cancelEnumeration.get()) {
                        truncated = true
                        return@use
                    }
                    val documentId = c.getString(0) ?: continue
                    val displayName = c.getString(1) ?: continue
                    val mimeType = c.getString(2)

                    if (mimeType == DocumentsContract.Document.MIME_TYPE_DIR) {
                        stack.push(Frame(documentId, joinPath(frame.relativeDir, displayName)))
                        continue
                    }
                    if (!isInteresting(displayName, mimeType)) continue
                    if (count >= cap) {
                        truncated = true
                        return@use
                    }

                    entries.put(JSObject().apply {
                        put(
                            "uri",
                            DocumentsContract
                                .buildDocumentUriUsingTree(treeUri, documentId)
                                .toString()
                        )
                        put("displayName", displayName)
                        put("mimeType", mimeType)
                        if (!c.isNull(3)) put("size", c.getLong(3))
                        if (!c.isNull(4)) put("lastModified", c.getLong(4))
                        put("relativeDir", frame.relativeDir)
                    })
                    count += 1
                }
            }
            if (truncated) break
        }

        return JSObject().apply {
            put("entries", entries)
            put("truncated", truncated)
        }
    }

    // ── Descriptors and bytes ───────────────────────────────────────────

    @Command
    fun openFd(invoke: Invoke) {
        val args = invoke.parseArgs(UriArgs::class.java)
        io.execute {
            var detached = -1
            try {
                val pfd = activity.contentResolver.openFileDescriptor(Uri.parse(args.uri), "r")
                if (pfd == null) {
                    invoke.reject("document could not be opened")
                    return@execute
                }
                val regular = isRegularFile(pfd)
                detached = pfd.detachFd()
                invoke.resolve(JSObject().apply {
                    put("fd", detached)
                    put("isRegularFile", regular)
                })
                detached = -1
            } catch (e: Exception) {
                Log.w(TAG, "openFd failed for ${args.uri}", e)
                // Only reachable if `resolve` itself threw after the detach.
                // Nothing else will ever close this descriptor, so adopt it back
                // and close it here.
                if (detached >= 0) {
                    try {
                        android.os.ParcelFileDescriptor.adoptFd(detached).close()
                    } catch (closeError: Exception) {
                        Log.e(TAG, "leaked fd $detached", closeError)
                    }
                }
                invoke.reject(e.message ?: "openFd failed")
            }
        }
    }

    @Command
    fun readBytes(invoke: Invoke) {
        val args = invoke.parseArgs(ReadBytesArgs::class.java)
        val cap = if (args.maxLen > 0) args.maxLen else DEFAULT_MAX_READ_BYTES
        io.execute {
            try {
                val stream = activity.contentResolver.openInputStream(Uri.parse(args.uri))
                if (stream == null) {
                    invoke.reject("document could not be opened")
                    return@execute
                }
                var truncated = false
                val out = stream.use { input ->
                    val buffer = java.io.ByteArrayOutputStream()
                    val chunk = ByteArray(8192)
                    while (true) {
                        val read = input.read(chunk)
                        if (read <= 0) break
                        val remaining = (cap - buffer.size()).coerceAtMost(Int.MAX_VALUE.toLong())
                        if (read > remaining) {
                            buffer.write(chunk, 0, remaining.toInt())
                            truncated = true
                            break
                        }
                        buffer.write(chunk, 0, read)
                    }
                    buffer.toByteArray()
                }
                val array = JSArray()
                for (byte in out) array.put(byte.toInt() and 0xFF)
                invoke.resolve(JSObject().apply {
                    put("bytes", array)
                    put("truncated", truncated)
                })
            } catch (e: Exception) {
                Log.w(TAG, "readBytes failed for ${args.uri}", e)
                invoke.reject(e.message ?: "readBytes failed")
            }
        }
    }

    /**
     * Pick one text document and hand back its contents.
     *
     * Deliberately does **not** take a persistable grant: the bytes are read here
     * and copied into app storage immediately, so there is nothing to come back
     * to — and per-file grants are a capped per-app resource that belongs to the
     * music library, not to a lyric file imported once.
     */
    @Command
    fun pickTextDocument(invoke: Invoke) {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            // Providers are inconsistent about the type of a `.lrc`: some report
            // `text/plain`, some `application/octet-stream`, some nothing at all.
            // `*/*` with a hint keeps them all selectable.
            type = "*/*"
            putExtra(Intent.EXTRA_MIME_TYPES, TEXT_MIME_TYPES)
        }
        startActivityForResult(invoke, intent, "onTextDocumentPicked")
    }

    @ActivityCallback
    fun onTextDocumentPicked(invoke: Invoke, result: ActivityResult) {
        val uri = if (result.resultCode == Activity.RESULT_OK) result.data?.data else null
        if (uri == null) {
            invoke.resolve(JSObject().apply { put("picked", null) })
            return
        }

        io.execute {
            try {
                val stream = activity.contentResolver.openInputStream(uri)
                if (stream == null) {
                    invoke.reject("document could not be opened")
                    return@execute
                }
                var truncated = false
                val bytes = stream.use { input ->
                    val buffer = java.io.ByteArrayOutputStream()
                    val chunk = ByteArray(8192)
                    while (true) {
                        val read = input.read(chunk)
                        if (read <= 0) break
                        if (buffer.size() + read > MAX_TEXT_BYTES) {
                            buffer.write(chunk, 0, MAX_TEXT_BYTES - buffer.size())
                            truncated = true
                            break
                        }
                        buffer.write(chunk, 0, read)
                    }
                    buffer.toByteArray()
                }
                val picked = JSObject().apply {
                    put("displayName", queryDocument(uri)?.displayName ?: lastSegmentName(uri))
                    // Lossy on purpose: older LRC files are frequently GBK, and a
                    // mostly-correct import beats refusing one.
                    put("text", String(bytes, Charsets.UTF_8))
                    put("truncated", truncated)
                }
                invoke.resolve(JSObject().apply { put("picked", picked) })
            } catch (e: Exception) {
                Log.w(TAG, "pickTextDocument failed for $uri", e)
                invoke.reject(e.message ?: "pickTextDocument failed")
            }
        }
    }

    // ── Writing ─────────────────────────────────────────────────────────

    /**
     * Create a document at the root of `treeUri` and return the URI the provider
     * actually assigned.
     *
     * Two things about this are load-bearing.
     *
     * The parent is [rootDocumentUri] — a **tree-based** document URI — so the
     * returned child URI comes out in the same `buildDocumentUriUsingTree` form
     * that [walkTree] emits. The local library keys a track by that string, so a
     * mismatch here would mean the row written when the file is downloaded and
     * the row written by a later full re-scan are two different tracks.
     *
     * And the provider renames on collision (`song (1).flac`), so the *returned*
     * URI and its queried `displayName` are the answer. Composing the name we
     * asked for onto the parent path would name a file that does not exist.
     */
    @Command
    fun createDocument(invoke: Invoke) {
        val args = invoke.parseArgs(CreateDocumentArgs::class.java)
        io.execute {
            try {
                val treeUri = Uri.parse(args.treeUri)
                val created = DocumentsContract.createDocument(
                    activity.contentResolver,
                    rootDocumentUri(treeUri),
                    args.mimeType ?: DEFAULT_WRITE_MIME,
                    args.displayName
                )
                if (created == null) {
                    invoke.reject("document could not be created")
                    return@execute
                }
                invoke.resolve(JSObject().apply {
                    put("uri", created.toString())
                    put("displayName", queryDocument(created)?.displayName ?: args.displayName)
                })
            } catch (e: SecurityException) {
                Log.w(TAG, "no write grant for ${args.treeUri}", e)
                invoke.reject("permission-lost")
            } catch (e: Exception) {
                Log.e(TAG, "createDocument failed in ${args.treeUri}", e)
                invoke.reject(e.message ?: "createDocument failed")
            }
        }
    }

    /**
     * Look for a root-level child of `treeUri` named `displayName`.
     *
     * A tree has no `stat` by name: the only way to ask "is this file already
     * there, and how big is it" is to query the children and match. That question
     * is what makes both "already downloaded, skip it" and "resume the partial
     * file" possible on Android, so it is worth one query per task.
     */
    @Command
    fun findDocument(invoke: Invoke) {
        val args = invoke.parseArgs(FindDocumentArgs::class.java)
        io.execute {
            try {
                val treeUri = Uri.parse(args.treeUri)
                val childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(
                    treeUri,
                    addressedDocumentId(treeUri)
                )
                var found: JSObject? = null
                activity.contentResolver
                    .query(childrenUri, CHILD_PROJECTION, null, null, null)
                    ?.use { c ->
                        while (c.moveToNext()) {
                            val documentId = c.getString(0) ?: continue
                            if (c.getString(1) != args.displayName) continue
                            found = JSObject().apply {
                                put(
                                    "uri",
                                    DocumentsContract
                                        .buildDocumentUriUsingTree(treeUri, documentId)
                                        .toString()
                                )
                                put("displayName", args.displayName)
                                put("mimeType", c.getString(2))
                                if (!c.isNull(3)) put("size", c.getLong(3))
                                if (!c.isNull(4)) put("lastModified", c.getLong(4))
                            }
                            break
                        }
                    }
                invoke.resolve(JSObject().apply { put("found", found) })
            } catch (e: SecurityException) {
                Log.w(TAG, "no grant for ${args.treeUri}", e)
                invoke.reject("permission-lost")
            } catch (e: Exception) {
                Log.e(TAG, "findDocument failed in ${args.treeUri}", e)
                invoke.reject(e.message ?: "findDocument failed")
            }
        }
    }

    /**
     * Open a document for writing and hand the descriptor to Rust.
     *
     * The same single-owner rule as [openFd] applies, and for the same reason:
     * after `detachFd()` the JVM will not close it, so any error path that has
     * already detached must adopt it back and close it here.
     */
    @Command
    fun openWriteFd(invoke: Invoke) {
        val args = invoke.parseArgs(OpenWriteFdArgs::class.java)
        val mode = when (args.mode) {
            "replace" -> "w"
            "random" -> "rw"
            else -> "wa"
        }
        io.execute {
            var detached = -1
            try {
                val pfd = activity.contentResolver
                    .openFileDescriptor(Uri.parse(args.uri), mode)
                if (pfd == null) {
                    invoke.reject("document could not be opened for writing")
                    return@execute
                }
                val regular = isRegularFile(pfd)
                detached = pfd.detachFd()
                invoke.resolve(JSObject().apply {
                    put("fd", detached)
                    put("isRegularFile", regular)
                })
                detached = -1
            } catch (e: Exception) {
                Log.w(TAG, "openWriteFd($mode) failed for ${args.uri}", e)
                if (detached >= 0) {
                    try {
                        android.os.ParcelFileDescriptor.adoptFd(detached).close()
                    } catch (closeError: Exception) {
                        Log.e(TAG, "leaked fd $detached", closeError)
                    }
                }
                invoke.reject(e.message ?: "openWriteFd failed")
            }
        }
    }

    /** Delete a document. Used to clear a half-written download. */
    @Command
    fun deleteDocument(invoke: Invoke) {
        val args = invoke.parseArgs(UriArgs::class.java)
        io.execute {
            val deleted = try {
                DocumentsContract.deleteDocument(
                    activity.contentResolver,
                    Uri.parse(args.uri)
                )
            } catch (e: Exception) {
                Log.w(TAG, "deleteDocument failed for ${args.uri}", e)
                false
            }
            invoke.resolve(JSObject().apply { put("deleted", deleted) })
        }
    }

    /**
     * Rename a document and return the URI it has afterwards.
     *
     * This is what publishes a finished download: it is written to `<name>.part`
     * — an extension neither [walkTree] nor `local::scan` treats as audio, so a
     * partial file can never be indexed as a broken track — and renamed onto its
     * real name only once the last byte has landed.
     *
     * The result stays in tree form because the input does; `renameDocument`
     * rebuilds the URI from whichever form it was handed.
     */
    @Command
    fun renameDocument(invoke: Invoke) {
        val args = invoke.parseArgs(RenameDocumentArgs::class.java)
        io.execute {
            try {
                val renamed = DocumentsContract.renameDocument(
                    activity.contentResolver,
                    Uri.parse(args.uri),
                    args.displayName
                )
                if (renamed == null) {
                    invoke.reject("document could not be renamed")
                    return@execute
                }
                invoke.resolve(JSObject().apply {
                    put("uri", renamed.toString())
                    put(
                        "displayName",
                        queryDocument(renamed)?.displayName ?: args.displayName
                    )
                })
            } catch (e: Exception) {
                Log.w(TAG, "renameDocument failed for ${args.uri}", e)
                invoke.reject(e.message ?: "renameDocument failed")
            }
        }
    }

    // ── Grants ──────────────────────────────────────────────────────────
    /**
     * A cheap existence check, on the resolver's hot path (every local track the
     * planner prepares asks this). `query` rather than `openFileDescriptor`
     * because opening a cloud-backed document can mean a network fetch.
     */
    @Command
    fun documentExists(invoke: Invoke) {
        val args = invoke.parseArgs(UriArgs::class.java)
        io.execute {
            val exists = try {
                activity.contentResolver.query(
                    Uri.parse(args.uri),
                    arrayOf(DocumentsContract.Document.COLUMN_DOCUMENT_ID),
                    null,
                    null,
                    null
                )?.use { it.count > 0 } ?: false
            } catch (e: Exception) {
                // A revoked grant throws rather than returning empty. Both mean
                // the same thing to the planner: skip this track.
                false
            }
            invoke.resolve(JSObject().apply { put("exists", exists) })
        }
    }

    @Command
    fun listPersisted(invoke: Invoke) {
        io.execute {
            val permissions = JSArray()
            for (permission in activity.contentResolver.persistedUriPermissions) {
                permissions.put(JSObject().apply {
                    put("uri", permission.uri.toString())
                    put("read", permission.isReadPermission)
                    put("write", permission.isWritePermission)
                    put("persistedTime", permission.persistedTime)
                })
            }
            invoke.resolve(JSObject().apply { put("permissions", permissions) })
        }
    }

    @Command
    fun releaseUri(invoke: Invoke) {
        val args = invoke.parseArgs(UriArgs::class.java)
        io.execute {
            try {
                val uri = Uri.parse(args.uri)
                var flags = 0
                for (permission in activity.contentResolver.persistedUriPermissions) {
                    if (permission.uri != uri) continue
                    if (permission.isReadPermission) flags =
                        flags or Intent.FLAG_GRANT_READ_URI_PERMISSION
                    if (permission.isWritePermission) flags =
                        flags or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                }
                if (flags != 0) {
                    activity.contentResolver.releasePersistableUriPermission(uri, flags)
                }
            } catch (e: Exception) {
                Log.w(TAG, "releaseUri failed for ${args.uri}", e)
            }
            invoke.resolve()
        }
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    private class Frame(val documentId: String, val relativeDir: String)

    private class DocumentMeta(
        val displayName: String?,
        val mimeType: String?,
        val size: Long?
    )

    private fun queryDocument(uri: Uri): DocumentMeta? = try {
        activity.contentResolver.query(uri, DOCUMENT_PROJECTION, null, null, null)?.use { c ->
            if (!c.moveToFirst()) null
            else DocumentMeta(
                displayName = c.getString(0),
                mimeType = c.getString(1),
                size = if (c.isNull(2)) null else c.getLong(2)
            )
        }
    } catch (e: Exception) {
        Log.w(TAG, "queryDocument failed for $uri", e)
        null
    }

    /**
     * The document this tree-shaped URI actually addresses.
     *
     * A granted tree is `…/tree/<grant>` and answers only to
     * `getTreeDocumentId`. A **subdirectory** of that grant is
     * `…/tree/<grant>/document/<child>` — the form [rootDocumentUri] and
     * [walkTree] already emit — and carries both halves: the grant that
     * authorises the access, and the folder being addressed.
     *
     * Preferring the document half is what lets one locator string mean either,
     * and that is what makes a download folder *inside* what the user granted
     * possible without a second grant. It matters because of what the platform
     * will not do: `Download` cannot be granted at all from target SDK 30, so the
     * grant is on `Music` — and writing loose into someone's whole music folder
     * would mean the "download source" the library registers is their entire
     * collection, with a children query over all of it before every single track.
     */
    private fun addressedDocumentId(uri: Uri): String {
        val segments = uri.pathSegments
        if (segments.size >= 4 && segments[0] == "tree" && segments[2] == "document") {
            return DocumentsContract.getDocumentId(uri)
        }
        return DocumentsContract.getTreeDocumentId(uri)
    }

    /**
     * The tree's own root as a **tree-based document** URI.
     *
     * `buildTreeDocumentUri` would also identify the root, but a document created
     * under it comes back in the non-tree form — a different string for the same
     * file, and therefore a second library row. See [createDocument].
     */
    private fun rootDocumentUri(treeUri: Uri): Uri =
        DocumentsContract.buildDocumentUriUsingTree(treeUri, addressedDocumentId(treeUri))

    private fun treeDisplayName(treeUri: Uri): String = try {
        queryDocument(rootDocumentUri(treeUri))?.displayName ?: lastSegmentName(treeUri)
    } catch (e: Exception) {
        lastSegmentName(treeUri)
    }

    /**
     * Best-effort readable name from a URI, for when the provider will not
     * answer a query. A tree document id looks like `primary:Music/Albums`, so
     * the tail after the last separator is the folder name.
     */
    private fun lastSegmentName(uri: Uri): String {
        val raw = uri.lastPathSegment ?: return uri.toString()
        return raw.substringAfterLast('/').substringAfterLast(':').ifEmpty { raw }
    }

    private fun joinPath(parent: String, name: String): String =
        if (parent.isEmpty()) name else "$parent/$name"

    private fun isInteresting(displayName: String, mimeType: String?): Boolean {
        if (mimeType != null && mimeType.startsWith("audio/")) return true
        val extension = displayName.substringAfterLast('.', "").lowercase()
        if (extension.isEmpty()) return false
        return extension in CANDIDATE_EXTENSIONS
    }

    /**
     * Whether `lseek` will work on this descriptor.
     *
     * Some document providers (cloud storage especially) back a document with a
     * pipe. symphonia's probe seeks backwards, so it fails at the first format
     * check rather than anywhere that names the cause — the Rust side spools a
     * non-regular source to a file instead.
     */
    private fun isRegularFile(pfd: android.os.ParcelFileDescriptor): Boolean = try {
        OsConstants.S_ISREG(Os.fstat(pfd.fileDescriptor).st_mode)
    } catch (e: Exception) {
        // `getStatSize()` is -1 for anything that is not a regular file.
        pfd.statSize >= 0
    }

    override fun onDestroy(activity: AppCompatActivity) {
        // Nothing here outlives the Activity: no session, no channel, no state
        // the app needs across a WebView render-process restart. The executor is
        // daemon-threaded, so shutting it down is courtesy rather than
        // correctness.
        io.shutdownNow()
        super.onDestroy(activity)
    }

    companion object {
        private const val TAG = "LocalFilesPlugin"

        /**
         * `ExternalStorageProvider`'s authority — the provider behind primary
         * shared storage and SD cards.
         *
         * Named here only to compose a picker starting point in
         * [publicDirectoryUri]. Nothing else in this file assumes a provider:
         * every other URI comes from the system, which is what lets a grant point
         * at Drive or a NAS just as well as at the internal card.
         */
        private const val EXTERNAL_STORAGE_AUTHORITY = "com.android.externalstorage.documents"

        /** Bound on a single tree walk. A library past this is not a library. */
        private const val DEFAULT_MAX_ENTRIES = 50_000

        /** `readBytes` is for sidecar lyric files, not for audio. */
        private const val DEFAULT_MAX_READ_BYTES = 4L * 1024 * 1024

        /** Cap on a picked lyric document. Matches the Rust side's own limit. */
        private const val MAX_TEXT_BYTES = 1024 * 1024

        /**
         * Fallback MIME for a created document.
         *
         * `application/octet-stream` on purpose rather than an `audio` wildcard:
         * several providers append an extension of their own choosing derived
         * from the MIME type, and a wrong one would rename the file out from
         * under the extension check in `local::scan`. The caller passes the real
         * type when it knows it.
         *
         * Spell that wildcard nowhere in a comment, and do not name a comment
         * delimiter in one either. Kotlin block comments **nest**, so a slash
         * immediately followed by a star opens a second comment inside this one
         * and the first closing delimiter below ends only that inner one —
         * leaving the whole companion object commented out. Every constant in it
         * then reports as an unresolved reference, plus an `Unclosed comment` at
         * end of file, and not one of those messages names this line.
         */
        private const val DEFAULT_WRITE_MIME = "application/octet-stream"

        private val TEXT_MIME_TYPES = arrayOf(
            "text/plain",
            "text/xml",
            "application/xml",
            "application/octet-stream"
        )

        private val PICKER_MIME_TYPES = arrayOf(
            "audio/*",
            "application/ogg",
            "application/x-flac"
        )

        private val CHILD_PROJECTION = arrayOf(
            DocumentsContract.Document.COLUMN_DOCUMENT_ID,
            DocumentsContract.Document.COLUMN_DISPLAY_NAME,
            DocumentsContract.Document.COLUMN_MIME_TYPE,
            DocumentsContract.Document.COLUMN_SIZE,
            DocumentsContract.Document.COLUMN_LAST_MODIFIED
        )

        private val DOCUMENT_PROJECTION = arrayOf(
            DocumentsContract.Document.COLUMN_DISPLAY_NAME,
            DocumentsContract.Document.COLUMN_MIME_TYPE,
            DocumentsContract.Document.COLUMN_SIZE
        )

        /**
         * Deliberately wider than what symphonia can decode: providers report
         * MIME types inconsistently, so this is a prefilter and Rust
         * (`local::scan::AUDIO_EXTENSIONS`) makes the real decision. Sidecar
         * lyric files ride along so the scan can pair them without a second
         * walk.
         */
        private val CANDIDATE_EXTENSIONS = setOf(
            "mp3", "flac", "wav", "wave", "m4a", "m4b", "mp4", "aac", "ogg", "oga",
            "opus", "aif", "aiff", "aifc", "alac", "caf", "mka", "lrc", "ttml"
        )
    }
}
