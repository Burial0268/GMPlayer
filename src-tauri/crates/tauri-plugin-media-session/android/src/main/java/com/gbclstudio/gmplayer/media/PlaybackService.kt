package com.gbclstudio.gmplayer.media

import android.annotation.SuppressLint
import android.app.Notification
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ServiceInfo
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import android.util.Log
import androidx.core.app.NotificationManagerCompat
import androidx.core.app.ServiceCompat

/**
 * Foreground service backing the media notification.
 *
 * Created once and kept for the life of the process. Nothing here ever calls
 * `stopSelf()`: clearing the session *demotes* the service and removes the
 * notification instead. The previous implementation stopped the service on
 * every clear, and because `stopSelf()` only schedules `onDestroy` — it runs
 * later on the main looper — the next track's update would build a fresh
 * session and notification that the late `onDestroy` then tore down again.
 * That is how the notification used to disappear mid-playlist while playback
 * carried on.
 *
 * Foreground promotion follows the model in
 * [media-kit](https://github.com/Moriafly/media-kit): re-send the start
 * contract from *inside* the running service and fulfil it on the very next
 * line, so the contract can never time out, and treat a refusal as "post the
 * notification without a service" rather than as a crash.
 *
 * Resources are tied to playback, not to the service's lifetime:
 * - wake lock: held only while a track is playing or loading;
 * - audio focus: requested when playback starts and **re-requested after a
 *   loss**, which the old code never did — once focus was gone permanently, a
 *   manual resume played on with no focus at all;
 * - becoming-noisy receiver: registered for the whole service lifetime, but
 *   only acted on while playing.
 */
class PlaybackService : Service() {

    private var isInForeground = false

    /**
     * Built in [onCreate], not as a field initialiser: a Service has no base
     * context until the framework attaches one, so touching
     * `applicationContext` during construction throws.
     */
    private lateinit var wakeLock: WakeLockManager

    private var focusRequest: AudioFocusRequest? = null
    private var hasAudioFocus = false
    private var noisyReceiver: BroadcastReceiver? = null

    private val focusListener = AudioManager.OnAudioFocusChangeListener { change ->
        when (change) {
            AudioManager.AUDIOFOCUS_GAIN -> hasAudioFocus = true
            // Permanent loss: the framework has already dropped our request, so
            // the handle is dead and the next play has to build a new one.
            AudioManager.AUDIOFOCUS_LOSS -> {
                hasAudioFocus = false
                focusRequest = null
            }
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> hasAudioFocus = false
        }
        MediaSessionController.onAudioFocusChange(change)
    }

    // ── Service lifecycle ───────────────────────────────────────────────

    override fun onCreate() {
        super.onCreate()
        MediaSessionController.attach(this)
        wakeLock = WakeLockManager(this)
        wakeLock.setEnabled(true)
        registerNoisyReceiver()
        // Published last: `instance` is what routes work in here, and nothing
        // may arrive before the fields that work touches exist.
        instance = this
        Log.d(TAG, "onCreate")
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        instance = this
        // This start command may carry an unfulfilled `startForegroundService`
        // contract, and there is no way to withdraw one — Google closed that as
        // working-as-intended, and an unfulfilled contract is a
        // ForegroundServiceDidNotStartInTimeException. So promote first and let
        // `applyPending` demote again if that is what the state actually wants.
        settleStartContract()
        applyPending()
        return START_NOT_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    /**
     * The user swiped the app out of recents.
     *
     * Playback continues. A foreground media service that stops when its task
     * is removed is a service that had no reason to be foreground; the old
     * implementation went further and killed the process outright, which took
     * the Rust audio thread with it mid-track.
     */
    override fun onTaskRemoved(rootIntent: Intent?) {
        Log.d(TAG, "onTaskRemoved — playback continues")
        super.onTaskRemoved(rootIntent)
    }

    /**
     * Release only what this service owns.
     *
     * Deliberately does not touch the session, the notification state or the
     * action channel: those belong to [MediaSessionController] and have to
     * survive the service being killed and restarted.
     */
    override fun onDestroy() {
        Log.d(TAG, "onDestroy")
        if (instance === this) instance = null
        unregisterNoisyReceiver()
        abandonAudioFocus()
        if (::wakeLock.isInitialized) wakeLock.setEnabled(false)
        isInForeground = false
        super.onDestroy()
    }

    // ── Applying a request ──────────────────────────────────────────────

    private fun applyPending() {
        val request = pending ?: return
        wakeLock.setStayAwake(request.playbackActive)
        if (request.playbackActive) requestAudioFocus()

        val notification = request.notification
        if (notification == null) {
            abandonAudioFocus()
            removeNotification()
            return
        }
        promote(notification)
    }

    /**
     * Promote and post in one step.
     *
     * The nested `startForegroundService` is intentional: on API 31+ a
     * background app may only call `startForeground` inside the window that a
     * start contract opens, and the service being already up means the contract
     * is settled by the very next statement. Recursion is bounded — the start
     * command it schedules finds `isInForeground` true and falls through to a
     * plain post.
     */
    @SuppressLint("InlinedApi")
    private fun promote(notification: Notification) {
        if (isInForeground) {
            postPlain(notification)
            return
        }
        try {
            startForegroundService(Intent(this, PlaybackService::class.java))
            startForegroundCompat(notification)
            isInForeground = true
        } catch (e: Exception) {
            // ForegroundServiceStartNotAllowedException (API 31+), a missing or
            // mismatched service type (34+), or an OEM policy. A media
            // notification with no service behind it is still better than none.
            Log.w(TAG, "foreground promotion refused (${e.javaClass.simpleName}): ${e.message}")
            postPlain(notification)
        }
    }

    /**
     * `startForeground` with the media-playback type where the platform has one.
     *
     * Called directly rather than through `ServiceCompat`: the overload that
     * takes a service type only exists from androidx.core 1.12, and this module
     * pins 1.9 to stay in step with the rest of the app.
     */
    private fun startForegroundCompat(notification: Notification) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(
                NOTIFICATION_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK,
            )
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    /** Nothing left to show: drop both the notification and the foreground slot. */
    private fun removeNotification() {
        if (isInForeground) {
            ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
            isInForeground = false
        } else {
            NotificationManagerCompat.from(this).cancel(NOTIFICATION_ID)
        }
    }

    @SuppressLint("MissingPermission")
    private fun postPlain(notification: Notification) {
        // A media notification needs no POST_NOTIFICATIONS grant to exist; if
        // the user denied it, the notify is a no-op rather than a throw.
        try {
            NotificationManagerCompat.from(this).notify(NOTIFICATION_ID, notification)
        } catch (e: Exception) {
            Log.w(TAG, "notify failed: ${e.message}")
        }
    }

    /** See the comment in [onStartCommand]. */
    @SuppressLint("InlinedApi")
    private fun settleStartContract() {
        if (isInForeground) return
        val notification = MediaSessionController.serviceNotification(this) ?: return
        try {
            startForegroundCompat(notification)
            isInForeground = true
        } catch (e: Exception) {
            Log.w(TAG, "start contract could not be settled: ${e.message}")
        }
    }

    // ── Audio focus ─────────────────────────────────────────────────────

    private fun requestAudioFocus() {
        if (hasAudioFocus) return
        val manager = getSystemService(AUDIO_SERVICE) as? AudioManager ?: return
        val request = focusRequest ?: AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                    .build()
            )
            .setAcceptsDelayedFocusGain(true)
            // Left at the default `false` on purpose: the framework then ducks
            // us automatically on a transient-can-duck loss, which is better
            // than any pause we could do from here.
            .setOnAudioFocusChangeListener(focusListener)
            .build()
        focusRequest = request

        val result = manager.requestAudioFocus(request)
        hasAudioFocus = result == AudioManager.AUDIOFOCUS_REQUEST_GRANTED
        when (result) {
            AudioManager.AUDIOFOCUS_REQUEST_GRANTED -> Log.d(TAG, "AudioFocus granted")
            // Delayed: another app holds transient focus. The framework will
            // hand it over with an AUDIOFOCUS_GAIN when it can.
            AudioManager.AUDIOFOCUS_REQUEST_DELAYED -> Log.d(TAG, "AudioFocus delayed")
            else -> {
                Log.w(TAG, "AudioFocus denied (result=$result)")
                focusRequest = null
            }
        }
    }

    private fun abandonAudioFocus() {
        val request = focusRequest ?: return
        focusRequest = null
        hasAudioFocus = false
        val manager = getSystemService(AUDIO_SERVICE) as? AudioManager ?: return
        manager.abandonAudioFocusRequest(request)
        Log.d(TAG, "AudioFocus abandoned")
    }

    // ── Becoming noisy (headphone unplug / BT disconnect) ───────────────

    private fun registerNoisyReceiver() {
        if (noisyReceiver != null) return
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent?) {
                if (intent?.action != AudioManager.ACTION_AUDIO_BECOMING_NOISY) return
                MediaSessionController.onBecomingNoisy()
            }
        }
        // No export flag: `ACTION_AUDIO_BECOMING_NOISY` is a protected system
        // broadcast, and receivers that only listen to those are exempt from
        // the API 33+ requirement to declare one.
        registerReceiver(receiver, IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY))
        noisyReceiver = receiver
    }

    private fun unregisterNoisyReceiver() {
        val receiver = noisyReceiver ?: return
        noisyReceiver = null
        try {
            unregisterReceiver(receiver)
        } catch (e: Exception) {
            Log.w(TAG, "noisy receiver already gone: ${e.message}")
        }
    }

    // ── Wake lock ───────────────────────────────────────────────────────

    /**
     * Two switches, mirroring Media3's `WakeLockManager`: [setEnabled] follows
     * the service's lifetime and [setStayAwake] follows playback. Holding the
     * lock for the whole session — which is what the old code did — keeps the
     * CPU awake through every pause.
     */
    private class WakeLockManager(context: Context) {
        private val appContext = context.applicationContext
        private var wakeLock: PowerManager.WakeLock? = null
        private var enabled = false
        private var stayAwake = false

        fun setEnabled(enabled: Boolean) {
            if (enabled && wakeLock == null) {
                val power = appContext.getSystemService(Context.POWER_SERVICE) as? PowerManager
                if (power == null) {
                    Log.w(TAG, "no PowerManager, running without a wake lock")
                    return
                }
                wakeLock = power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, WAKE_LOCK_TAG)
                    .apply { setReferenceCounted(false) }
            }
            this.enabled = enabled
            update()
        }

        fun setStayAwake(stayAwake: Boolean) {
            this.stayAwake = stayAwake
            update()
        }

        @SuppressLint("WakelockTimeout")
        private fun update() {
            val lock = wakeLock ?: return
            // No timeout: a listening session with the screen off can run for
            // hours, and any bound we picked would cut one of them short.
            if (enabled && stayAwake) {
                if (!lock.isHeld) lock.acquire()
            } else {
                if (lock.isHeld) lock.release()
            }
        }
    }

    // ── Static entry points ─────────────────────────────────────────────

    companion object {
        private const val TAG = "gmplayer/media"
        private const val WAKE_LOCK_TAG = "gmplayer:PlaybackWakeLock"

        internal const val NOTIFICATION_ID = 9401

        @Volatile
        private var instance: PlaybackService? = null

        @Volatile
        private var pending: Request? = null

        private class Request(
            val notification: Notification?,
            val playbackActive: Boolean,
        )

        /**
         * Publish the media notification and promote the service. Main thread
         * only.
         *
         * The service stays foreground for as long as a track is loaded, paused
         * included. Media3 and media-kit demote on pause, and that is the more
         * frugal choice — but re-promoting is the fragile operation here, since
         * the resume usually arrives while the app is in the background and
         * API 31+ refuses a foreground start from there without an exemption our
         * own notification-action broadcast does not carry. Holding the slot is
         * also the honest description of this process: it owns the Rust audio
         * thread, the decoder and the queue, none of which are cheap to rebuild.
         * The wake lock is still released on pause, which is the part that
         * actually costs battery.
         */
        fun post(context: Context, notification: Notification, playbackActive: Boolean) {
            pending = Request(notification, playbackActive)
            val service = instance
            if (service != null) service.applyPending() else start(context)
        }

        /**
         * Nothing to show: drop the notification and the playback resources.
         *
         * The service itself stays alive. If the system reclaims it later that
         * is fine — [onDestroy] only releases what this class owns, and the
         * next [post] starts it again.
         */
        fun release(context: Context) {
            pending = Request(null, playbackActive = false)
            val service = instance
            if (service != null) {
                service.applyPending()
            } else {
                // No service to demote, but the fallback path may have posted a
                // notification without one.
                NotificationManagerCompat.from(context).cancel(NOTIFICATION_ID)
            }
        }

        /**
         * Greedy start, after media-kit's `tryStartService`.
         *
         * `startService` is tried first because it creates no start contract at
         * all; it throws only when the app is not considered foreground, and
         * that is exactly when the contract-bearing call is the one with the
         * exemptions we need.
         */
        private fun start(context: Context) {
            val intent = Intent(context, PlaybackService::class.java)
            try {
                context.startService(intent)
                return
            } catch (e: IllegalStateException) {
                Log.d(TAG, "startService refused (${e.javaClass.simpleName}), trying foreground")
            } catch (e: Exception) {
                Log.w(TAG, "startService failed: ${e.message}")
            }
            try {
                context.startForegroundService(intent)
            } catch (e: Exception) {
                // Background FGS start restriction (API 31+) with no exemption.
                // Show the notification anyway so the user still has transport
                // controls; the next update from the foreground will promote.
                Log.w(TAG, "startForegroundService refused: ${e.message}")
                pending?.notification?.let { notification ->
                    try {
                        NotificationManagerCompat.from(context)
                            .notify(NOTIFICATION_ID, notification)
                    } catch (inner: Exception) {
                        Log.w(TAG, "notify fallback failed: ${inner.message}")
                    }
                }
            }
        }
    }
}
