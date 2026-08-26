package com.gbclstudio.gmplayer.media

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.drawable.BitmapDrawable
import android.graphics.drawable.Drawable
import android.media.AudioManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.support.v4.media.MediaMetadataCompat
import android.support.v4.media.session.MediaSessionCompat
import android.support.v4.media.session.PlaybackStateCompat
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.media.app.NotificationCompat as MediaNotificationCompat
import app.tauri.plugin.Channel
import app.tauri.plugin.JSObject
import java.lang.ref.WeakReference
import java.net.HttpURLConnection
import java.net.URL

/**
 * Process-scoped owner of the OS media session.
 *
 * Everything durable lives here rather than on [MediaSessionPlugin] because a
 * Tauri plugin instance does not outlive the Activity in the way the session
 * has to. Tauri's `PluginManager` is a process-scoped object that constructs
 * each plugin exactly once and never replaces it, but it still forwards
 * `onDestroy` when the Activity goes away — so an Activity recreation used to
 * run the plugin's teardown with no matching setup afterwards. That left the
 * notification cancelled, the session released and the action channel dropped,
 * permanently, while the Rust process kept playing.
 *
 * That is not a hypothetical: `MainActivity.installRenderProcessGuard` calls
 * `recreate()` on purpose when the WebView render process dies, precisely so
 * playback survives.
 *
 * All entry points are main-thread only. Tauri dispatches `@Command` methods
 * there, the foreground service's callbacks run there, and the artwork thread
 * hops back through [mainHandler].
 */
internal object MediaSessionController {

    private const val TAG = "gmplayer/media"
    private const val MAX_ARTWORK_SIZE = 512
    private const val RC_PLAY = 1
    private const val RC_PAUSE = 2
    private const val RC_NEXT = 3
    private const val RC_PREV = 4
    private const val RC_FAVOURITE = 5
    private const val RC_PLAY_MODE = 6

    /**
     * Action names for the two session controls.
     *
     * These are the snake_case serde names of `MediaAction::ToggleFavourite` and
     * `MediaAction::CyclePlayMode` on the Rust side. The channel payload is
     * deserialized straight into that enum, so a rename on either side has to be
     * made on both — an unrecognised name is dropped silently.
     */
    private const val ACTION_FAVOURITE = "toggle_favourite"
    private const val ACTION_PLAY_MODE = "cycle_play_mode"

    private val mainHandler = Handler(Looper.getMainLooper())

    /** Application context. Never an Activity — see the class doc. */
    private var appContext: Context? = null

    /**
     * The live Activity, when there is one.
     *
     * Weak, and only used for things that genuinely need an Activity: the
     * runtime notification-permission prompt. Everything else resolves through
     * [appContext] so it keeps working with no UI at all.
     */
    private var activityRef: WeakReference<android.app.Activity>? = null

    /**
     * Action sink back into Rust.
     *
     * Survives Activity recreation on purpose: the channel is a plain
     * `(String) -> Unit` onto `PluginManager.sendChannelData`, which is a static
     * JNI hop with no Activity in it. Rust hands it over once at plugin setup
     * and never again, so dropping it means the notification's buttons stop
     * reaching playback for the rest of the process's life.
     */
    private var eventChannel: Channel? = null

    private var mediaSession: MediaSessionCompat? = null

    /** Whether there is anything to show. Cleared by [clear], set by [updateState]. */
    private var active = false

    /** The most recent notification, so the service can promote itself with it. */
    private var notification: Notification? = null

    // ── Merge state — omitted fields keep their previous values ──────────

    private var currentTitle: String = ""
    private var currentArtist: String = ""
    private var currentAlbum: String = ""
    private var currentDuration: Double = 0.0
    private var currentPosition: Double = 0.0

    /**
     * When [currentPosition] was true, on the same clock the framework
     * extrapolates from.
     *
     * A position is meaningless without its instant. `PlaybackState` is an
     * *anchor* — the OS renders `position + (now - updateTime) * speed` — and
     * steady playback deliberately pushes no positions at all, because that
     * extrapolation is already correct and each push costs a JNI round trip plus
     * a notification rebuild. So [currentPosition] is routinely minutes old, and
     * stamping it with `elapsedRealtime()` at publish time claimed it was true
     * *now*: any rebuild that carried no position of its own — the favourite and
     * play-mode pushes, which change only a glyph — reset the progress bar to
     * wherever the last real push left it, normally 0:00. Pause/resume appeared
     * to "fix" it only because those commands carry a fresh position.
     *
     * Re-publishing the original pair instead leaves the OS's extrapolation
     * exactly as it was, so a glyph change is invisible to the timeline. Every
     * discontinuity (seek, pause, track change) carries a position and re-anchors
     * here, which is what keeps the pair honest.
     */
    private var positionAnchorRealtime: Long = SystemClock.elapsedRealtime()
    private var currentPlaybackSpeed: Double = 1.0
    private var currentIsPlaying: Boolean = false
    private var currentIsLoading: Boolean = false
    private var currentCanPrev: Boolean = false
    private var currentCanNext: Boolean = false
    private var currentCanSeek: Boolean = true

    /**
     * Session controls, pushed by Rust and never decided here.
     *
     * The notification is one of three surfaces showing the same two values, so
     * this object renders them and reports presses — it does not own them. A
     * press emits an *intent* ("cycle the mode", "toggle the heart") and the new
     * value arrives on the next `updateState`, which is what keeps the app, the
     * lock screen and the notification from disagreeing.
     *
     * There is deliberately no `canFavourite` here. Whether a like is *possible*
     * is the backend's business — it holds the credential — and letting it decide
     * whether the button is drawn made the heart disappear whenever any of its
     * three inputs was momentarily absent. Both controls are always rendered;
     * only the glyph varies.
     */
    private var currentPlayMode: String = "normal"
    private var currentFavourite: Boolean = false

    /**
     * Playback was interrupted by a *transient* focus loss (a call, a voice
     * message), so focus coming back means resume. A permanent loss, or a pause
     * the user asked for, must never set this.
     */
    private var resumeOnFocusGain = false

    // ── Artwork ─────────────────────────────────────────────────────────

    private var cachedArtworkUrl: String? = null
    private var cachedArtwork: Bitmap? = null
    private var fallbackArtwork: Bitmap? = null

    /**
     * URL of the download in flight.
     *
     * Deliberately not cleared by `artworkUrl = ""`, so an in-flight download
     * still resolves against the request that started it instead of being
     * adopted by a later clear.
     */
    private var downloadingArtworkUrl: String? = null

    // ── Identifiers derived from the host app ───────────────────────────

    private val channelId: String
        get() = "${requireContext().packageName}.media"

    private val sessionTag: String
        get() = "${requireContext().packageName}.MediaSession"

    // ── Attachment ──────────────────────────────────────────────────────

    fun attach(context: Context) {
        if (appContext == null) appContext = context.applicationContext
    }

    fun attachActivity(activity: android.app.Activity) {
        attach(activity)
        activityRef = WeakReference(activity)
    }

    /**
     * Drop the Activity reference and nothing else.
     *
     * The session, the notification and the action channel all outlive the UI
     * on purpose — the whole point of driving this from Rust is that it keeps
     * working while there is no Activity and no WebView.
     */
    fun detachActivity(activity: android.app.Activity) {
        if (activityRef?.get() === activity) activityRef = null
    }

    fun setEventChannel(channel: Channel) {
        eventChannel = channel
    }

    fun currentActivity(): android.app.Activity? = activityRef?.get()

    private fun requireContext(): Context =
        appContext ?: throw IllegalStateException("MediaSessionController used before attach()")

    private fun contextOrNull(): Context? = appContext

    // ── Public state ────────────────────────────────────────────────────

    /** Whether the session is playing or on its way there. */
    val wantsPlayback: Boolean
        get() = active && (currentIsPlaying || currentIsLoading)

    /** The notification the foreground service should promote itself with. */
    fun currentNotification(): Notification? = if (active) notification else null

    /**
     * Something the service can always hand to `startForeground`.
     *
     * A start command can arrive carrying a contract that has to be fulfilled
     * even when there is nothing to show — a `clear` that raced the start, or a
     * restart after the system reclaimed the service. Returning a bare
     * notification lets the service settle that contract and withdraw in the
     * same pass, which is the only safe way out of one.
     */
    fun serviceNotification(context: Context): Notification? {
        currentNotification()?.let { return it }
        attach(context)
        return try {
            ensureChannel(context)
            NotificationCompat.Builder(context, channelId)
                .setSmallIcon(smallIcon(context))
                .setContentTitle(
                    context.applicationInfo.loadLabel(context.packageManager).toString()
                )
                .setCategory(NotificationCompat.CATEGORY_TRANSPORT)
                .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
                .setSilent(true)
                .setShowWhen(false)
                .build()
        } catch (e: Throwable) {
            Log.w(TAG, "could not build a placeholder notification: ${e.message}")
            null
        }
    }

    // ── Commands ────────────────────────────────────────────────────────

    fun updateState(args: UpdateStateArgs) {
        val context = contextOrNull() ?: run {
            Log.e(TAG, "updateState before attach()")
            return
        }

        args.title?.trim()?.let { currentTitle = it }
        args.artist?.trim()?.let { currentArtist = it }
        args.album?.trim()?.let { currentAlbum = it }
        args.duration?.let { currentDuration = it }
        args.position?.let { anchorPosition(it) }
        args.playbackSpeed?.let { currentPlaybackSpeed = it }
        args.isPlaying?.let { currentIsPlaying = it }
        args.isLoading?.let { currentIsLoading = it }
        args.canPrev?.let { currentCanPrev = it }
        args.canNext?.let { currentCanNext = it }
        args.canSeek?.let { currentCanSeek = it }
        args.playMode?.let { currentPlayMode = it }
        args.favourite?.let { currentFavourite = it }
        active = true

        args.artworkUrl?.let { url ->
            if (url.isEmpty()) {
                applyFallbackArtwork(null)
                cachedArtworkUrl = null
                downloadingArtworkUrl = null
            } else if (url != cachedArtworkUrl) {
                cachedArtworkUrl = url
                downloadingArtworkUrl = url
                downloadArtwork(url)
            }
        }

        publish(context)

        Log.d(
            TAG,
            "updateState: \"$currentTitle\" by $currentArtist, playing=$currentIsPlaying, " +
                "loading=$currentIsLoading, pos=${currentPosition}s/${currentDuration}s, " +
                "artwork=${if (cachedArtwork != null) "yes" else "none"}"
        )
    }

    /**
     * Position/speed only — updates `PlaybackState` without rebuilding the
     * notification, so it costs no artwork re-fetch.
     *
     * Throws when there is no session yet. The bridge treats a failed push as
     * "the OS state is now unknown" and re-asserts the full projection on the
     * next event, which is the recovery this case wants; resolving quietly would
     * drop the update and leave the timeline wrong.
     */
    fun updateTimeline(args: UpdateTimelineArgs) {
        val session = mediaSession
            ?: throw IllegalStateException("media session not initialized — call updateState first")
        args.position?.let { anchorPosition(it) }
        args.duration?.let { currentDuration = it }
        args.playbackSpeed?.let { currentPlaybackSpeed = it }

        session.setPlaybackState(buildPlaybackState())
        if (args.duration != null) session.setMetadata(buildMetadata())
    }

    /**
     * Nothing is playing: hide the session and drop the notification.
     *
     * Deliberately *not* a teardown. The session object is deactivated and
     * emptied but kept, and the foreground service is demoted rather than
     * stopped. The previous implementation released both, and because
     * `stopSelf()` schedules `onDestroy` asynchronously, the next track's
     * `updateState` would build a fresh session and notification that the late
     * `onDestroy` then tore down again — which is exactly how the notification
     * used to vanish mid-playlist.
     *
     * It also does not emit a transport action: with actions wired straight to
     * the backend, a pause from here would stop a track that is starting.
     */
    fun clear() {
        Log.d(TAG, "clear: hiding session, keeping service")
        active = false
        notification = null

        mediaSession?.let { session ->
            try {
                session.setPlaybackState(
                    PlaybackStateCompat.Builder()
                        .setActions(0)
                        .setState(PlaybackStateCompat.STATE_NONE, 0L, 0f)
                        .build()
                )
                session.setMetadata(MediaMetadataCompat.Builder().build())
            } catch (e: Throwable) {
                Log.w(TAG, "clear: could not reset session state: ${e.message}")
            }
            session.isActive = false
        }

        currentTitle = ""; currentArtist = ""; currentAlbum = ""
        currentDuration = 0.0; anchorPosition(0.0); currentPlaybackSpeed = 1.0
        currentIsPlaying = false; currentIsLoading = false
        currentCanPrev = false; currentCanNext = false; currentCanSeek = true
        currentPlayMode = "normal"; currentFavourite = false
        resumeOnFocusGain = false

        cachedArtwork = null
        cachedArtworkUrl = null
        downloadingArtworkUrl = null

        contextOrNull()?.let { PlaybackService.release(it) }
    }

    // ── Publishing ──────────────────────────────────────────────────────

    private fun publish(context: Context) {
        val session = ensureSession(context) ?: return
        val metadata = buildMetadata()
        session.setMetadata(metadata)
        session.setPlaybackState(buildPlaybackState())
        applyPlayModeToSession(session)
        session.isActive = true

        val built = buildNotification(context, session, metadata) ?: return
        notification = built
        // Buffering counts as playing: the track is starting, and holding the
        // wake lock through the resolve window is the point of having one.
        PlaybackService.post(context, built, playbackActive = wantsPlayback)
    }

    /** Re-publish after an artwork change, without touching playback state. */
    private fun republishArtwork() {
        val context = contextOrNull() ?: return
        if (!active) return
        val session = mediaSession ?: return
        val metadata = buildMetadata()
        session.setMetadata(metadata)
        val built = buildNotification(context, session, metadata) ?: return
        notification = built
        PlaybackService.post(context, built, playbackActive = wantsPlayback)
    }

    // ── Session ─────────────────────────────────────────────────────────

    @Suppress("DEPRECATION")
    private fun ensureSession(context: Context): MediaSessionCompat? {
        mediaSession?.let { return it }

        Log.d(TAG, "ensureSession: creating session (tag=$sessionTag, channel=$channelId)")
        val session = MediaSessionCompat(context, sessionTag)
        session.setCallback(sessionCallback)
        session.setFlags(
            MediaSessionCompat.FLAG_HANDLES_MEDIA_BUTTONS or
                MediaSessionCompat.FLAG_HANDLES_TRANSPORT_CONTROLS
        )
        session.isActive = true
        launchPendingIntent(context)?.let { session.setSessionActivity(it) }

        mediaSession = session
        return session
    }

    private fun buildPlaybackState(): PlaybackStateCompat {
        val positionMs = (currentPosition * 1000.0).toLong()
        // `STATE_BUFFERING` is what makes the system media controls render a
        // spinner instead of a transport button. Speed 0 with it, so the OS
        // does not extrapolate a timeline that is not moving.
        val state = when {
            currentIsLoading -> PlaybackStateCompat.STATE_BUFFERING
            currentIsPlaying -> PlaybackStateCompat.STATE_PLAYING
            else -> PlaybackStateCompat.STATE_PAUSED
        }
        val speed =
            if (currentIsPlaying && !currentIsLoading) currentPlaybackSpeed.toFloat() else 0.0f

        var actions = PlaybackStateCompat.ACTION_PLAY_PAUSE or
            PlaybackStateCompat.ACTION_PLAY or
            PlaybackStateCompat.ACTION_PAUSE or
            PlaybackStateCompat.ACTION_STOP or
            // Declared so the framework routes Android Auto's and the lock
            // screen's own shuffle/repeat controls to `sessionCallback`. Without
            // these the callbacks are never invoked and those surfaces render
            // the mode read-only.
            PlaybackStateCompat.ACTION_SET_SHUFFLE_MODE or
            PlaybackStateCompat.ACTION_SET_REPEAT_MODE
        if (currentCanSeek) actions = actions or PlaybackStateCompat.ACTION_SEEK_TO
        if (currentCanPrev) actions = actions or PlaybackStateCompat.ACTION_SKIP_TO_PREVIOUS
        if (currentCanNext) actions = actions or PlaybackStateCompat.ACTION_SKIP_TO_NEXT

        return PlaybackStateCompat.Builder()
            .setActions(actions)
            // The anchor's own instant, never "now" — see [positionAnchorRealtime].
            .setState(state, positionMs, speed, positionAnchorRealtime)
            .also(::addSessionControlActions)
            .build()
    }

    /**
     * Record a position together with the instant it was true at.
     *
     * The only way [currentPosition] may be written: the two fields are one
     * value, and updating either alone makes the OS extrapolate from a lie.
     */
    private fun anchorPosition(seconds: Double) {
        currentPosition = seconds
        positionAnchorRealtime = SystemClock.elapsedRealtime()
    }

    /**
     * Attach the two session controls as **PlaybackState custom actions**.
     *
     * This is the only way they render on Android 13+. From API 33 the system
     * builds the media controls from the `PlaybackState` and ignores the
     * `Notification.Action` list entirely: slot 1 is play/pause, 2 and 3 are
     * previous/next when `ACTION_SKIP_TO_*` are declared, and slots 4 and 5 are
     * filled from custom actions **in the order they were added here**. The
     * notification's own actions are kept as well, because below API 33 the
     * reverse is true — there the `Notification.Action` list is what is drawn.
     *
     * Both are added unconditionally, and that is deliberate. Gating the heart
     * on `canFavourite` made it the one control that could silently disappear:
     * it depends on a credential, a track identity and a fetched like list, and
     * any one of them being absent for a moment took the button out of the
     * notification entirely rather than merely leaving it unfilled. A control
     * that comes and goes is worse than one that is occasionally a no-op, so
     * `canFavourite` now only decides how the glyph *looks* and whether the
     * press does anything — never whether it exists. Play mode has always
     * worked this way, which is why it was the one that showed up.
     */
    private fun addSessionControlActions(builder: PlaybackStateCompat.Builder) {
        builder.addCustomAction(
            PlaybackStateCompat.CustomAction.Builder(
                ACTION_PLAY_MODE, playModeLabel(), playModeIcon()
            ).build()
        )
        builder.addCustomAction(
            PlaybackStateCompat.CustomAction.Builder(
                ACTION_FAVOURITE, favouriteLabel(), favouriteIcon()
            ).build()
        )
    }

    /** Glyph for the like state. Outline until the account says otherwise. */
    private fun favouriteIcon(): Int =
        if (currentFavourite) R.drawable.ic_media_session_favourite_on
        else R.drawable.ic_media_session_favourite_off

    private fun favouriteLabel(): String =
        if (currentFavourite) "Unfavourite" else "Favourite"

    /**
     * Mirror the traversal mode onto the session's own shuffle/repeat state.
     *
     * The notification draws our glyph, but the lock screen, Android Auto and
     * Assistant read these instead — they have no idea about custom actions. So
     * both are set from the same value rather than one being derived from the
     * other. Shuffle and repeat are independent properties there: random still
     * repeats the list, because shuffling is a traversal choice.
     */
    private fun applyPlayModeToSession(session: MediaSessionCompat) {
        session.setShuffleMode(
            if (currentPlayMode == "random") PlaybackStateCompat.SHUFFLE_MODE_ALL
            else PlaybackStateCompat.SHUFFLE_MODE_NONE
        )
        session.setRepeatMode(
            if (currentPlayMode == "single") PlaybackStateCompat.REPEAT_MODE_ONE
            else PlaybackStateCompat.REPEAT_MODE_ALL
        )
    }

    private fun buildMetadata(): MediaMetadataCompat {
        val builder = MediaMetadataCompat.Builder()
        if (currentTitle.isNotEmpty()) {
            builder.putString(MediaMetadataCompat.METADATA_KEY_TITLE, currentTitle)
        }
        if (currentArtist.isNotEmpty()) {
            builder.putString(MediaMetadataCompat.METADATA_KEY_ARTIST, currentArtist)
        }
        if (currentAlbum.isNotEmpty()) {
            builder.putString(MediaMetadataCompat.METADATA_KEY_ALBUM, currentAlbum)
        }
        val durationMs = (currentDuration * 1000.0).toLong()
        if (durationMs > 0) {
            builder.putLong(MediaMetadataCompat.METADATA_KEY_DURATION, durationMs)
        }
        cachedArtwork?.let { builder.putBitmap(MediaMetadataCompat.METADATA_KEY_ALBUM_ART, it) }
        return builder.build()
    }

    // ── Notification ────────────────────────────────────────────────────

    private fun buildNotification(
        context: Context,
        session: MediaSessionCompat,
        metadata: MediaMetadataCompat,
    ): Notification? {
        ensureChannel(context)

        val title = metadata.getString(MediaMetadataCompat.METADATA_KEY_TITLE)
            ?: context.applicationInfo.loadLabel(context.packageManager).toString()
        val artist = metadata.getString(MediaMetadataCompat.METADATA_KEY_ARTIST)
        val album = metadata.getString(MediaMetadataCompat.METADATA_KEY_ALBUM)
        val subtitle = listOfNotNull(artist, album)
            .filter { it.isNotBlank() }
            .joinToString(" — ")
        val artwork = metadata.getBitmap(MediaMetadataCompat.METADATA_KEY_ALBUM_ART)

        val actions = mutableListOf<NotificationCompat.Action>()
        /**
         * Slots the collapsed notification shows. Tracked as we build rather
         * than assumed to be the first three: `setShowActionsInCompactView`
         * takes indices, so hard-coding `0..2` breaks the moment anything is
         * added that is not part of the transport.
         */
        val transportIndices = mutableListOf<Int>()
        if (currentCanPrev) {
            transportIndices.add(actions.size)
            actions.add(
                NotificationCompat.Action(
                    android.R.drawable.ic_media_previous, "Previous",
                    actionIntent(context, "previous", RC_PREV)
                )
            )
        }
        transportIndices.add(actions.size)
        actions.add(
            // While a track is being prepared the transport button becomes a
            // loading glyph, but keeps the intent it would otherwise have:
            // tapping a spinner should still do the obvious thing rather than
            // swallow the press. Request codes stay paired with their action
            // string — `FLAG_UPDATE_CURRENT` rewrites the extras of whatever
            // PendingIntent shares a code.
            if (currentIsLoading) {
                NotificationCompat.Action(
                    R.drawable.ic_media_session_loading, "Loading",
                    if (currentIsPlaying) actionIntent(context, "pause", RC_PAUSE)
                    else actionIntent(context, "play", RC_PLAY)
                )
            } else if (currentIsPlaying) {
                NotificationCompat.Action(
                    android.R.drawable.ic_media_pause, "Pause",
                    actionIntent(context, "pause", RC_PAUSE)
                )
            } else {
                NotificationCompat.Action(
                    android.R.drawable.ic_media_play, "Play",
                    actionIntent(context, "play", RC_PLAY)
                )
            }
        )
        if (currentCanNext) {
            transportIndices.add(actions.size)
            actions.add(
                NotificationCompat.Action(
                    android.R.drawable.ic_media_next, "Next",
                    actionIntent(context, "next", RC_NEXT)
                )
            )
        }
        actions.add(
            NotificationCompat.Action(
                playModeIcon(), playModeLabel(),
                actionIntent(context, ACTION_PLAY_MODE, RC_PLAY_MODE)
            )
        )
        // Same order as the custom actions above, so the row reads the same on
        // either side of API 33: transport, play mode, then the heart last.
        actions.add(
            NotificationCompat.Action(
                favouriteIcon(), favouriteLabel(),
                actionIntent(context, ACTION_FAVOURITE, RC_FAVOURITE)
            )
        )

        val builder = NotificationCompat.Builder(context, channelId)
            .setSmallIcon(smallIcon(context))
            .setContentTitle(title)
            .setContentText(subtitle)
            .setCategory(NotificationCompat.CATEGORY_TRANSPORT)
            .setOnlyAlertOnce(true)
            .setShowWhen(false)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setOngoing(wantsPlayback)
            .setSilent(true)
            .setColorized(true)

        if (artwork != null) builder.setLargeIcon(artwork)
        launchPendingIntent(context)?.let { builder.setContentIntent(it) }

        val compactIndices = transportIndices.take(3).toIntArray()
        val style = MediaNotificationCompat.MediaStyle().setMediaSession(session.sessionToken)
        if (compactIndices.isNotEmpty()) style.setShowActionsInCompactView(*compactIndices)
        builder.setStyle(style)
        actions.forEach { builder.addAction(it) }

        return builder.build()
    }

    /** Glyph for the current traversal mode. */
    private fun playModeIcon(): Int = when (currentPlayMode) {
        "random" -> R.drawable.ic_media_session_shuffle
        "single" -> R.drawable.ic_media_session_repeat_one
        else -> R.drawable.ic_media_session_repeat
    }

    private fun playModeLabel(): String = when (currentPlayMode) {
        "random" -> "Shuffle"
        "single" -> "Repeat one"
        else -> "Repeat all"
    }

    private fun ensureChannel(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val manager = context.getSystemService(NotificationManager::class.java) ?: return
        // Never deleted anywhere: a channel is a long-lived asset that carries
        // the user's own sound/importance overrides, and posting to a deleted
        // one is silently dropped.
        if (manager.getNotificationChannel(channelId) != null) return
        manager.createNotificationChannel(
            NotificationChannel(channelId, "Media playback", NotificationManager.IMPORTANCE_LOW)
                .apply { description = "Media playback controls" }
        )
    }

    private fun actionIntent(context: Context, action: String, requestCode: Int): PendingIntent {
        val intent = Intent(context, MediaButtonReceiver::class.java)
            .setPackage(context.packageName)
            .putExtra(MediaButtonReceiver.EXTRA_ACTION, action)
        return PendingIntent.getBroadcast(context, requestCode, intent, pendingIntentFlags())
    }

    private fun launchPendingIntent(context: Context): PendingIntent? {
        val launch = context.packageManager.getLaunchIntentForPackage(context.packageName)
            ?: return null
        return PendingIntent.getActivity(context, 0, launch, pendingIntentFlags())
    }

    private fun pendingIntentFlags(): Int =
        PendingIntent.FLAG_UPDATE_CURRENT or
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) PendingIntent.FLAG_IMMUTABLE else 0

    private fun smallIcon(context: Context): Int = hostIcon(context)
        ?: R.drawable.ic_media_session_notification

    /**
     * A host-app override, looked up once.
     *
     * Android tints the small icon down to a silhouette, so this has to be a
     * monochrome glyph — a full-colour launcher icon renders as a solid block.
     * `ic_launcher_foreground` is deliberately *not* in this chain: on API 24+
     * it resolves to the Android Studio template robot, which is where the
     * bugdroid in the notification came from.
     */
    private var hostIconCache: Int? = null
    private var hostIconResolved = false

    private fun hostIcon(context: Context): Int? {
        if (hostIconResolved) return hostIconCache
        hostIconResolved = true
        for (type in arrayOf("drawable", "mipmap")) {
            val id = context.resources.getIdentifier("ic_notification", type, context.packageName)
            if (id != 0) {
                hostIconCache = id
                return id
            }
        }
        return null
    }

    // ── Artwork download (native HTTP, no CORS) ─────────────────────────

    private fun downloadArtwork(url: String) {
        Thread {
            val bitmap = try {
                fetchBitmap(url)
            } catch (e: Exception) {
                Log.w(TAG, "artwork: failed for $url: ${e.message}")
                null
            }
            if (bitmap == null) {
                applyFallbackArtwork(url)
                return@Thread
            }
            mainHandler.post {
                // Apply only if this is still the most recently requested URL.
                if (downloadingArtworkUrl != url) return@post
                downloadingArtworkUrl = null
                // The previous bitmap is *not* recycled. It is still referenced
                // by the notification already posted and by the metadata the
                // session published, and SystemUI draws and parcels both well
                // after we move on — recycling it there throws
                // "Canvas: trying to use a recycled bitmap" inside SystemUI and
                // the notification is dropped. Dropping the reference is enough;
                // the GC owns the rest.
                cachedArtwork = bitmap
                cachedArtworkUrl = url
                republishArtwork()
            }
        }.start()
    }

    private fun fetchBitmap(url: String): Bitmap? {
        var connection: HttpURLConnection? = null
        return try {
            connection = URL(url).openConnection() as HttpURLConnection
            connection.connectTimeout = 10000
            connection.readTimeout = 10000
            connection.instanceFollowRedirects = true
            connection.connect()
            val contentType = connection.contentType?.lowercase() ?: ""
            if (contentType.startsWith("video/")) {
                null
            } else {
                val bytes = connection.inputStream.use { it.readBytes() }
                decodeSampled(bytes, MAX_ARTWORK_SIZE)
            }
        } catch (e: Exception) {
            Log.w(TAG, "artwork: fetch failed for $url: ${e.message}")
            null
        } finally {
            connection?.disconnect()
        }
    }

    private fun applyFallbackArtwork(expectedUrl: String?) {
        mainHandler.post {
            if (expectedUrl != null && downloadingArtworkUrl != expectedUrl) return@post
            downloadingArtworkUrl = null
            cachedArtworkUrl = null
            cachedArtwork = ensureFallbackArtwork()
            republishArtwork()
        }
    }

    private fun ensureFallbackArtwork(): Bitmap? {
        fallbackArtwork?.let { return it }
        val context = contextOrNull() ?: return null
        return try {
            val drawable = context.packageManager.getApplicationIcon(context.applicationInfo)
            fallbackArtwork = scaleToMax(drawableToBitmap(drawable), MAX_ARTWORK_SIZE)
            fallbackArtwork
        } catch (e: Throwable) {
            Log.w(TAG, "artwork: no fallback available: ${e.message}")
            null
        }
    }

    private fun drawableToBitmap(drawable: Drawable): Bitmap {
        if (drawable is BitmapDrawable && drawable.bitmap != null) return drawable.bitmap
        val width = if (drawable.intrinsicWidth > 0) drawable.intrinsicWidth else MAX_ARTWORK_SIZE
        val height = if (drawable.intrinsicHeight > 0) drawable.intrinsicHeight else MAX_ARTWORK_SIZE
        val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        drawable.setBounds(0, 0, canvas.width, canvas.height)
        drawable.draw(canvas)
        return bitmap
    }

    /** Scales down when needed. Never recycles `source` — see [downloadArtwork]. */
    private fun scaleToMax(source: Bitmap, maxSize: Int): Bitmap {
        val width = source.width
        val height = source.height
        if (width <= maxSize && height <= maxSize) return source
        val scale = minOf(maxSize.toFloat() / width, maxSize.toFloat() / height)
        return Bitmap.createScaledBitmap(
            source,
            (width * scale).toInt().coerceAtLeast(1),
            (height * scale).toInt().coerceAtLeast(1),
            true
        )
    }

    private fun decodeSampled(bytes: ByteArray, maxSize: Int): Bitmap? {
        val opts = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, opts)
        var sample = 1
        var w = opts.outWidth
        var h = opts.outHeight
        while (w / 2 >= maxSize && h / 2 >= maxSize) {
            sample *= 2; w /= 2; h /= 2
        }
        opts.inSampleSize = sample
        opts.inJustDecodeBounds = false
        return BitmapFactory.decodeByteArray(bytes, 0, bytes.size, opts)
    }

    // ── Events → Rust (via Channel) ─────────────────────────────────────

    fun emitAction(action: String) {
        val channel = eventChannel
        if (channel == null) {
            Log.w(TAG, "emit: no event channel, dropping \"$action\"")
            return
        }
        Log.d(TAG, "emit: action=\"$action\"")
        val payload = JSObject()
        payload.put("action", action)
        mainHandler.post { channel.send(payload) }
    }

    private fun emitSeek(positionMs: Long) {
        val channel = eventChannel ?: return
        val seconds = positionMs / 1000.0
        Log.d(TAG, "emit: action=\"seek\", seekPosition=${seconds}s")
        val payload = JSObject()
        payload.put("action", "seek")
        payload.put("seekPosition", seconds)
        mainHandler.post { channel.send(payload) }
    }

    private val sessionCallback = object : MediaSessionCompat.Callback() {
        override fun onPlay() = emitAction("play")
        override fun onPause() = emitAction("pause")
        override fun onStop() = emitAction("stop")
        override fun onSkipToNext() = emitAction("next")
        override fun onSkipToPrevious() = emitAction("previous")
        override fun onSeekTo(pos: Long) = emitSeek(pos)

        /**
         * Where the two session controls arrive from Android 13+.
         *
         * On those releases the notification's own action buttons are never
         * drawn for a media session, so the presses come back through the
         * PlaybackState custom action that replaced them. Below API 33 the
         * `Notification.Action` PendingIntents fire `MediaButtonReceiver`
         * instead — same action strings either way, so both land on the same
         * intent in Rust.
         */
        override fun onCustomAction(action: String?, extras: Bundle?) {
            Log.d(TAG, "onCustomAction: \"$action\"")
            when (action) {
                ACTION_FAVOURITE, ACTION_PLAY_MODE -> emitAction(action)
                else -> Log.w(TAG, "unknown custom action \"$action\"")
            }
        }

        // Android Auto and the lock screen set a *value*; our own notification
        // button sends a cycle intent. Both funnel to the same place because the
        // backend owns the mode — so a requested value we are already on is a
        // no-op, and any other value is one step away on a three-mode ring.
        // Translating rather than sending the raw value keeps a single writer.
        override fun onSetShuffleMode(shuffleMode: Int) {
            val wantsRandom = shuffleMode != PlaybackStateCompat.SHUFFLE_MODE_NONE
            if (wantsRandom != (currentPlayMode == "random")) emitAction(ACTION_PLAY_MODE)
        }

        override fun onSetRepeatMode(repeatMode: Int) {
            val wantsSingle = repeatMode == PlaybackStateCompat.REPEAT_MODE_ONE
            if (wantsSingle != (currentPlayMode == "single")) emitAction(ACTION_PLAY_MODE)
        }
    }

    // ── Audio focus policy ──────────────────────────────────────────────

    /**
     * Decide what a focus change means for playback.
     *
     * The policy lives here, not in the service, because every branch depends
     * on playback state this object owns — "resume when focus comes back" is
     * only correct if *we* were the ones playing when it was taken away.
     */
    fun onAudioFocusChange(change: Int) {
        when (change) {
            // Permanent: another app owns audio now. Do not come back on its
            // own — the user has moved on.
            AudioManager.AUDIOFOCUS_LOSS -> {
                Log.i(TAG, "AudioFocus lost permanently — pausing")
                resumeOnFocusGain = false
                if (wantsPlayback) emitAction("pause")
            }
            // A call, a voice message, a navigation prompt: pause and resume
            // afterwards. This is the branch that has to auto-resume.
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> {
                Log.i(TAG, "AudioFocus lost transiently — pausing, will resume")
                resumeOnFocusGain = wantsPlayback
                if (wantsPlayback) emitAction("pause")
            }
            // From API 26 the framework ducks for us unless the focus request
            // opted out with `setWillPauseWhenDucked(true)` — and it does not.
            // Pausing here would turn a notification chime into a gap in the
            // music.
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK -> {
                Log.d(TAG, "AudioFocus duck requested — system handles it")
            }
            AudioManager.AUDIOFOCUS_GAIN -> {
                if (resumeOnFocusGain) {
                    resumeOnFocusGain = false
                    Log.i(TAG, "AudioFocus regained — resuming")
                    emitAction("play")
                } else {
                    Log.d(TAG, "AudioFocus regained — nothing to resume")
                }
            }
            else -> Log.d(TAG, "AudioFocus change: $change")
        }
    }

    /** Headphones unplugged / Bluetooth gone. Only meaningful while playing. */
    fun onBecomingNoisy() {
        if (!wantsPlayback) return
        Log.d(TAG, "Audio becoming noisy — pausing")
        resumeOnFocusGain = false
        emitAction("pause")
    }
}
