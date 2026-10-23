package com.gbclstudio.gmplayer.media

import android.Manifest
import android.app.Activity
import android.content.pm.PackageManager
import android.os.Build
import android.util.Log
import androidx.appcompat.app.AppCompatActivity
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@InvokeArg
class UpdateStateArgs {
    var title: String? = null
    var artist: String? = null
    var album: String? = null
    var artworkUrl: String? = null
    var duration: Double? = null
    var position: Double? = null
    var playbackSpeed: Double? = null
    var isPlaying: Boolean? = null

    /**
     * The session is preparing the track (resolving, downloading, buffering)
     * rather than paused. Rendered as `STATE_BUFFERING` plus a loading glyph on
     * the transport button, so a slow start looks different from a play button
     * that does nothing.
     */
    var isLoading: Boolean? = null
    var canPrev: Boolean? = null
    var canNext: Boolean? = null
    var canSeek: Boolean? = null

    /**
     * Traversal mode: "normal", "random" or "single".
     *
     * A string so a value this build does not know about falls back to the
     * default glyph rather than picking the wrong one.
     */
    var playMode: String? = null

    /** Whether the current track is in the user's 我喜欢的音乐. */
    var favourite: Boolean? = null
}

@InvokeArg
class UpdateTimelineArgs {
    var position: Double? = null
    var duration: Double? = null
    var playbackSpeed: Double? = null
}

@InvokeArg
class SetEventHandlerArgs {
    lateinit var handler: Channel
}

/**
 * Tauri entry point. Deliberately thin.
 *
 * Every piece of state that has to outlive the UI lives in
 * [MediaSessionController]; this class only parses arguments, owns the runtime
 * permission prompt, and hands the action channel over.
 *
 * That split exists because of how Tauri's Android runtime works: its
 * `PluginManager` is a process-scoped object that builds each plugin exactly
 * once and never replaces it, yet still forwards `onDestroy` when the Activity
 * goes away. The old implementation released the session and dropped the action
 * channel there, so a single Activity recreation — which
 * `MainActivity.installRenderProcessGuard` performs on purpose when the WebView
 * render process dies — permanently killed the notification and its buttons
 * while Rust carried on playing.
 */
@TauriPlugin
class MediaSessionPlugin(activity: Activity) : Plugin(activity) {

    private var notificationPermissionRequested = false

    init {
        Log.d(TAG, "init")
        MediaSessionController.attachActivity(activity)
    }

    // ── Commands ────────────────────────────────────────────────────────

    @Command
    fun initialize(invoke: Invoke) {
        Log.d(TAG, "initialize")
        requestNotificationPermission()
        invoke.resolve()
    }

    @Command
    fun setEventHandler(invoke: Invoke) {
        val args = invoke.parseArgs(SetEventHandlerArgs::class.java)
        MediaSessionController.setEventChannel(args.handler)
        Log.d(TAG, "setEventHandler: event channel registered")
        invoke.resolve()
    }

    @Command
    fun updateState(invoke: Invoke) {
        val args = invoke.parseArgs(UpdateStateArgs::class.java)
        requestNotificationPermission()
        try {
            MediaSessionController.updateState(args)
            invoke.resolve()
        } catch (e: Throwable) {
            Log.e(TAG, "updateState failed: ${e.message}", e)
            invoke.reject("media session update failed: ${e.message}")
        }
    }

    @Command
    fun updateTimeline(invoke: Invoke) {
        val args = invoke.parseArgs(UpdateTimelineArgs::class.java)
        try {
            MediaSessionController.updateTimeline(args)
            invoke.resolve()
        } catch (e: Throwable) {
            Log.e(TAG, "updateTimeline failed: ${e.message}", e)
            invoke.reject("media session timeline update failed: ${e.message}")
        }
    }

    @Command
    fun clear(invoke: Invoke) {
        try {
            MediaSessionController.clear()
            invoke.resolve()
        } catch (e: Throwable) {
            Log.e(TAG, "clear failed: ${e.message}", e)
            invoke.reject("media session clear failed: ${e.message}")
        }
    }

    // ── Lifecycle ───────────────────────────────────────────────────────

    /**
     * Drop the Activity reference and nothing else.
     *
     * Releasing the session here is what used to make the notification vanish
     * on an Activity recreation — see the class doc.
     */
    override fun onDestroy(activity: AppCompatActivity) {
        Log.d(TAG, "onDestroy: detaching activity, session untouched")
        MediaSessionController.detachActivity(activity)
    }

    // ── Permissions ─────────────────────────────────────────────────────

    private fun requestNotificationPermission() {
        if (notificationPermissionRequested) return
        notificationPermissionRequested = true
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return

        // The live Activity, not the one this plugin was built with: after a
        // recreation the constructor's Activity is destroyed and prompting
        // through it is a no-op.
        val host = MediaSessionController.currentActivity() ?: return
        if (host.isFinishing || host.isDestroyed) return
        if (
            ContextCompat.checkSelfPermission(host, Manifest.permission.POST_NOTIFICATIONS) ==
            PackageManager.PERMISSION_GRANTED
        ) {
            return
        }
        try {
            ActivityCompat.requestPermissions(
                host,
                arrayOf(Manifest.permission.POST_NOTIFICATIONS),
                NOTIFICATION_PERMISSION_REQUEST_CODE,
            )
        } catch (e: Exception) {
            Log.w(TAG, "could not request POST_NOTIFICATIONS: ${e.message}")
        }
    }

    companion object {
        private const val TAG = "gmplayer/media"
        private const val NOTIFICATION_PERMISSION_REQUEST_CODE = 9402
    }
}
