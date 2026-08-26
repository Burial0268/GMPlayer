import { onMounted, onUnmounted, ref, watch } from "vue";
import { storeToRefs } from "pinia";
import { musicStore } from "@/store";
import { isMobile, isTauri } from "@/utils/tauri";
import { setSeek } from "@/utils/AudioContext";
import {
  installSessionControlsSubscriber,
  publishSessionControls,
  requestNextPlayMode,
} from "@/utils/AudioContext";
import { initializeMediaNotification } from "@/utils/tauri/media/notification";
import {
  initializeNowPlayingControls,
  listenNowPlayingAction,
  updateNowPlayingPlayMode,
  type NowPlayingActionPayload,
} from "@/utils/tauri/media/nowPlaying";

/**
 * Native media controls — **desktop control path, plus play mode**.
 *
 * Metadata, playback state and the timeline are pushed to the OS media session
 * by Rust (`src-tauri/src/media`), driven off the audio backend's event stream.
 * That move is not an optimisation: on Android the WebView is destroyed and the
 * page reloaded while playback continues, so anything pushed from here freezes
 * on whatever track was showing when the WebView died — and stays wrong after
 * the backend advances on its own.
 *
 * The Android *control* path moved for the same reason: notification buttons,
 * lock screen, media keys and audio focus are delivered by the plugin straight
 * into Rust (`media::install_controls`), which drives the backend with no JS
 * runtime involved. A button that only reaches this file is a button that stops
 * working exactly when the notification is the only UI the user has. The store
 * still follows, through the backend's own events (`PlayStatus`,
 * `NativePlannerAdvanced`), so the transport keeps a single writer.
 *
 * What is still owned here:
 * - desktop system-control actions (`now-playing-controls`, SMTC/MPRIS): no
 *   WebView-death problem there, and no Rust-side consumer
 *
 * Play mode used to be owned here. It is not any more: it is one of two session
 * controls the backend holds (see `NativeSessionControlsSync`), because the
 * Android notification renders and sets it, SMTC/MPRIS expose it as native
 * properties, and the Rust planner has to honour it on the next hop with no page
 * alive. This file now only *asks* — `requestNextPlayMode` — and adopts what
 * comes back.
 */

type PlayMode = "normal" | "random" | "single";
type NativeMediaAction = NowPlayingActionPayload;

interface NativeMediaAdapter {
  name: "media-session" | "now-playing-controls";
  initialize: () => Promise<void | undefined>;
  updatePlayMode: (mode: PlayMode) => Promise<void | undefined>;
  /** Absent when the platform delivers its actions to Rust instead. */
  listenAction?: (handler: (payload: NativeMediaAction) => void) => Promise<() => void>;
}

const mobileMediaSessionAdapter: NativeMediaAdapter = {
  name: "media-session",
  // Still worth calling: this is what prompts for the notification permission.
  initialize: initializeMediaNotification,
  // Android's MediaSession has no shuffle/repeat surface in the notification.
  updatePlayMode: async () => undefined,
  // No `listenAction` / audio-focus listener: Rust owns Android transport.
};

const desktopNowPlayingAdapter: NativeMediaAdapter = {
  name: "now-playing-controls",
  initialize: initializeNowPlayingControls,
  updatePlayMode: (mode) => updateNowPlayingPlayMode({ mode }),
  listenAction: listenNowPlayingAction,
};

let instanceCount = 0;

export function useNativeMediaControls() {
  const music = musicStore();
  const { persistData } = storeToRefs(music);
  const active = ref(false);
  const adapterName = ref<NativeMediaAdapter["name"] | null>(null);

  let adapter: NativeMediaAdapter | null = null;
  let unlistenMediaAction: (() => void) | null = null;
  let unlistenSessionControls: (() => void) | null = null;
  // Whether THIS instance holds the singleton slot. A non-claiming instance
  // (mounted while another was active) must not decrement the shared counter
  // on unmount, or the surviving instance is left permanently inert.
  let claimedMediaControls = false;

  /**
   * Live playback clock in seconds. Prefers the active sound's timeline
   * (anchor + extrapolation — accurate even right after a backend-initiated
   * track advance) over the store snapshot, which the RAF loop can leave
   * seconds stale.
   */
  function livePlayback(): { position: number; duration: number } {
    const playSongTime = music.getPlaySongTime;
    let position = playSongTime?.currentTime || 0;
    let duration = playSongTime?.duration || 0;

    const player = window.$player;
    if (player) {
      try {
        const livePosition = player.seek();
        if (typeof livePosition === "number" && Number.isFinite(livePosition)) {
          position = livePosition;
        }
        const liveDuration = player.duration();
        if (Number.isFinite(liveDuration) && liveDuration > 0) {
          duration = liveDuration;
        }
      } catch {
        /* destroyed/mid-swap sound — store snapshot fallback is fine */
      }
    }
    return { position: Math.max(0, position), duration: Math.max(0, duration) };
  }

  /**
   * Push the play mode to the desktop system session.
   *
   * Rust projects this too, off `SessionControls` — but its projection is
   * deliberately silent until a track is loaded (an empty session with a shuffle
   * icon is not a session). This covers that window, and pushes the same value
   * from the same store, so the two cannot disagree.
   */
  async function syncPlayMode(): Promise<void> {
    if (!active.value || !adapter) return;
    await adapter.updatePlayMode(persistData.value.playSongMode || "normal");
  }

  function handleMediaAction(payload: NativeMediaAction): void {
    switch (payload.action) {
      case "play":
        music.setPlayState(true);
        break;
      case "pause":
        music.setPlayState(false);
        break;
      case "next":
        music.setPlaySongIndex("next");
        break;
      case "previous":
        music.setPlaySongIndex("prev");
        break;
      case "stop":
        music.setPlayState(false);
        break;
      case "seek":
        if (typeof payload.position === "number") {
          const { duration } = livePlayback();
          let seekSec = Math.max(0, payload.position / 1_000);
          if (duration > 0) seekSec = Math.min(seekSec, duration);
          if (window.$player) {
            setSeek(window.$player, seekSec);
          }
          music.setPlaySongTime({
            currentTime: seekSec,
            duration: music.getPlaySongTime?.duration || 0,
          });
          // No notification push here: the backend emits a position anchor on
          // seek commit and the Rust bridge re-anchors the system timeline.
        }
        break;
      case "toggleShuffle":
      case "toggleRepeat":
        // Both are one press on the same three-mode ring, and the ring is the
        // backend's — it holds the value the app, the notification and SMTC all
        // render. Writing the store here instead would make this a second
        // writer, which is exactly how the surfaces used to drift apart.
        // `requestNextPlayMode` returns false only when there is no backend to
        // ask, in which case the local setter is the whole truth.
        if (!requestNextPlayMode()) {
          music.setPlaySongMode(
            payload.action === "toggleShuffle"
              ? persistData.value.playSongMode === "random"
                ? "normal"
                : "random"
              : persistData.value.playSongMode === "single"
                ? "normal"
                : "single",
          );
        }
        break;
      case "setVolume":
        if (typeof payload.volume === "number") {
          persistData.value.playVolume = Math.max(0, Math.min(1, payload.volume));
        }
        break;
      case "setRate":
        break;
      default:
        console.warn("[NativeMediaControls] Unknown media action:", payload);
    }
  }

  // Audio focus is handled natively: the Android plugin decides what a focus
  // change means (pause on transient loss and resume after it, let the
  // framework duck for a can-duck loss) and drives the backend directly, so it
  // keeps working with no page loaded. The old JS bookkeeping that used to live
  // here could only run while the WebView was alive — which is never the case
  // when a call comes in with the app in the background.

  onMounted(async () => {
    if (instanceCount > 0) return;
    instanceCount++;
    claimedMediaControls = true;

    if (!isTauri()) return;

    // Before anything that can reject. `adapter.initialize()` is a JNI round
    // trip that also raises the notification-permission prompt, and an async
    // `onMounted` that throws is swallowed by Vue — which used to leave the
    // subscriber uninstalled and the app deaf to every notification button.
    unlistenSessionControls = installSessionControlsSubscriber();

    adapter = (await isMobile()) ? mobileMediaSessionAdapter : desktopNowPlayingAdapter;
    adapterName.value = adapter.name;
    active.value = true;

    // Still worth calling: on Android this is what prompts for the
    // notification permission. The session itself is created lazily by the
    // first push from Rust.
    await adapter.initialize();
    if (adapter.listenAction) {
      unlistenMediaAction = await adapter.listenAction(handleMediaAction);
    }
    publishSessionControls({ force: true });
    void syncPlayMode();
  });

  onUnmounted(() => {
    if (claimedMediaControls) {
      claimedMediaControls = false;
      instanceCount = Math.max(0, instanceCount - 1);
    }
    unlistenMediaAction?.();
    unlistenMediaAction = null;
    unlistenSessionControls?.();
    unlistenSessionControls = null;
    // Deliberately NOT clearing the session: Rust owns its lifetime now and
    // playback outlives this component (WebView reload, HMR). The bridge
    // clears it when the backend reports no track.
    active.value = false;
    adapterName.value = null;
    adapter = null;
  });

  watch(
    () => persistData.value.playSongMode,
    () => {
      void syncPlayMode();
    },
  );

  return { active, adapterName, syncPlayMode };
}
