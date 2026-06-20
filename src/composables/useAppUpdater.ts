import { computed, reactive } from "vue";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import { isTauri } from "@/utils/tauri";

type UpdaterStatus =
  | "idle"
  | "checking"
  | "available"
  | "not-available"
  | "downloading"
  | "installing"
  | "installed"
  | "error";

const state = reactive({
  checked: false,
  status: "idle" as UpdaterStatus,
  update: null as Update | null,
  error: "",
  downloadedBytes: 0,
  contentLength: 0,
  downloadSpeed: 0,
  lastCheckedAt: 0,
  installedVersion: "",
});

let activeCheck: Promise<Update | null> | null = null;

// Download speed tracking (smoothed bytes/sec) — kept outside reactive state
// so frequent updates during download don't trigger extra reactivity churn.
let speedAnchorAt = 0;
let speedAnchorBytes = 0;

const resetDownloadStats = () => {
  state.downloadedBytes = 0;
  state.contentLength = 0;
  state.downloadSpeed = 0;
  speedAnchorAt = 0;
  speedAnchorBytes = 0;
};

const normalizeError = (error: unknown) => {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return String(error ?? "");
};

const releaseUpdate = async () => {
  if (!state.update) return;
  try {
    await state.update.close();
  } catch {
    // Resource cleanup is best effort; updater commands remain usable.
  }
  state.update = null;
};

export function useAppUpdater() {
  const supported = computed(() => isTauri());
  const hasUpdate = computed(() => state.status === "available" && !!state.update);
  const isBusy = computed(() => ["checking", "downloading", "installing"].includes(state.status));
  const progressPercent = computed(() => {
    if (!state.contentLength) return null;
    return Math.min(100, Math.round((state.downloadedBytes / state.contentLength) * 100));
  });
  const etaSeconds = computed(() => {
    if (state.status !== "downloading") return null;
    if (!state.contentLength || state.downloadSpeed <= 0) return null;
    const remaining = state.contentLength - state.downloadedBytes;
    return remaining > 0 ? remaining / state.downloadSpeed : 0;
  });

  const checkForUpdate = async (options: { silent?: boolean } = {}) => {
    if (!supported.value) return null;
    if (activeCheck) return activeCheck;

    state.error = "";
    state.installedVersion = "";
    state.status = "checking";
    resetDownloadStats();

    activeCheck = check()
      .then(async (update) => {
        state.checked = true;
        state.lastCheckedAt = Date.now();
        if (update) {
          await releaseUpdate();
          state.update = update;
          state.status = "available";
        } else {
          await releaseUpdate();
          state.status = "not-available";
        }
        return update;
      })
      .catch((error) => {
        state.error = normalizeError(error);
        state.status = options.silent ? "idle" : "error";
        return null;
      })
      .finally(() => {
        activeCheck = null;
      });

    return activeCheck;
  };

  const installAvailableUpdate = async () => {
    const update = state.update ?? (await checkForUpdate());
    if (!update) return false;

    state.error = "";
    state.status = "downloading";
    resetDownloadStats();

    try {
      await update.downloadAndInstall((event: DownloadEvent) => {
        if (event.event === "Started") {
          state.status = "downloading";
          state.contentLength = event.data.contentLength ?? 0;
          state.downloadedBytes = 0;
          state.downloadSpeed = 0;
          speedAnchorAt = Date.now();
          speedAnchorBytes = 0;
        } else if (event.event === "Progress") {
          state.downloadedBytes += event.data.chunkLength;
          const now = Date.now();
          const elapsed = now - speedAnchorAt;
          // Sample roughly 4×/sec and smooth with an EMA to avoid a jittery readout.
          if (speedAnchorAt && elapsed >= 250) {
            const instSpeed = ((state.downloadedBytes - speedAnchorBytes) / elapsed) * 1000;
            state.downloadSpeed = state.downloadSpeed
              ? state.downloadSpeed * 0.6 + instSpeed * 0.4
              : instSpeed;
            speedAnchorAt = now;
            speedAnchorBytes = state.downloadedBytes;
          }
        } else if (event.event === "Finished") {
          state.status = "installing";
          state.downloadSpeed = 0;
        }
      });
      state.installedVersion = update.version;
      state.status = "installed";
      await releaseUpdate();
      return true;
    } catch (error) {
      state.error = normalizeError(error);
      state.status = "error";
      return false;
    }
  };

  return {
    updaterState: state,
    supported,
    hasUpdate,
    isBusy,
    progressPercent,
    etaSeconds,
    checkForUpdate,
    installAvailableUpdate,
  };
}
