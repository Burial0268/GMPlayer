import type { MessageApi, DialogApi, NotificationApi, LoadingBarApi } from "naive-ui";

declare global {
  interface Window {
    // Audio
    AudioContext: typeof AudioContext;
    webkitAudioContext: typeof AudioContext;

    // Naive UI (mounted by Provider component)
    $message: MessageApi;
    $notification: NotificationApi;
    $dialog: DialogApi;
    $loadingBar: LoadingBarApi;
    $setSiteThemeWithTransition?: (theme: "light" | "dark" | "auto") => void;

    // App-level helpers (mounted in App.vue onMounted)
    $setSiteTitle: (title: string) => void;
    $scrollToTop: () => void;
    $cleanAll: () => Promise<void>;
    $signIn: () => void;
    $getPlaySongData: (data: unknown) => void;
  }

  // Allow bare usage without window. prefix
  var $message: Window["$message"];
  var $notification: Window["$notification"];
  var $dialog: Window["$dialog"];
  var $loadingBar: Window["$loadingBar"];
  var $setSiteTitle: Window["$setSiteTitle"];
  var $scrollToTop: Window["$scrollToTop"];
  var $cleanAll: Window["$cleanAll"];
  var $signIn: Window["$signIn"];
  var $getPlaySongData: Window["$getPlaySongData"];
}

export {};
