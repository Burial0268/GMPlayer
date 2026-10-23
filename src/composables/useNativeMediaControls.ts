import { onMounted, onUnmounted, ref, watch } from "vue";
import { storeToRefs } from "pinia";
import { musicStore } from "@/store";
import { isMobile, isTauri } from "@/utils/tauri";
import { installSessionControlsSubscriber, publishSessionControls } from "@/utils/AudioContext";
import { initializeMediaNotification } from "@/utils/tauri/media/notification";
import {
  initializeNowPlayingControls,
  updateNowPlayingPlayMode,
} from "@/utils/tauri/media/nowPlaying";

/**
 * Native media controls — **play mode, and the platform handshake**.
 *
 * Metadata, playback state and the timeline are pushed to the OS media session
 * by Rust (`src-tauri/src/media`), driven off the audio backend's event stream.
 * That move is not an optimisation: on Android the WebView is destroyed and the
 * page reloaded while playback continues, so anything pushed from here freezes
 * on whatever track was showing when the WebView died — and stays wrong after
 * the backend advances on its own.
 *
 * The *control* direction is Rust's too, on every platform
 * (`media::install_controls`). On Android the reason is the same WebView death:
 * notification buttons, lock screen, media keys and audio focus are delivered
 * by the plugin straight into Rust with no JS runtime involved. On desktop the
 * reason is weaker but sufficient — the SMTC/MPRIS hop out to JS could only
 * work while a page happened to be mounted and listening, it failed with no
 * error anywhere when it wasn't (`AppHandle::emit` reports success even with no
 * listener registered), and writing the store from here made play/pause a
 * second writer on a transport the backend owns.
 *
 * So in a Tauri environment this file no longer handles a single system-media
 * action. The store follows the backend through its own events (`PlayStatus`,
 * `NativePlannerAdvanced`, `SessionControlsChanged`), which is what keeps the
 * app, the notification and the lock screen from disagreeing.
 *
 * What is still owned here:
 * - the platform handshake: `initialize` is what raises Android's
 *   notification-permission prompt
 * - the play-mode push, which covers the window before the backend's own
 *   projection starts (see `syncPlayMode`)
 */

type PlayMode = "normal" | "random" | "single";

interface NativeMediaAdapter {
  name: "media-session" | "now-playing-controls";
  initialize: () => Promise<void | undefined>;
  updatePlayMode: (mode: PlayMode) => Promise<void | undefined>;
}

const mobileMediaSessionAdapter: NativeMediaAdapter = {
  name: "media-session",
  // Still worth calling: this is what prompts for the notification permission.
  initialize: initializeMediaNotification,
  // Android's MediaSession has no shuffle/repeat surface in the notification.
  updatePlayMode: async () => undefined,
};

const desktopNowPlayingAdapter: NativeMediaAdapter = {
  name: "now-playing-controls",
  initialize: initializeNowPlayingControls,
  updatePlayMode: (mode) => updateNowPlayingPlayMode({ mode }),
};

let instanceCount = 0;

export function useNativeMediaControls() {
  const music = musicStore();
  const { persistData } = storeToRefs(music);
  const active = ref(false);
  const adapterName = ref<NativeMediaAdapter["name"] | null>(null);

  let adapter: NativeMediaAdapter | null = null;
  let unlistenSessionControls: (() => void) | null = null;
  // Whether THIS instance holds the singleton slot. A non-claiming instance
  // (mounted while another was active) must not decrement the shared counter
  // on unmount, or the surviving instance is left permanently inert.
  let claimedMediaControls = false;

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

  // No action handler lives here any more. Play/pause, next/previous, seek and
  // the shuffle/repeat requests are all taken by `media::install_controls` and
  // sent to the backend as `AudioThreadMessage`s; the store learns about them
  // the same way it learns about a backend-driven track change.

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
    publishSessionControls({ force: true });
    void syncPlayMode();
  });

  onUnmounted(() => {
    if (claimedMediaControls) {
      claimedMediaControls = false;
      instanceCount = Math.max(0, instanceCount - 1);
    }
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
