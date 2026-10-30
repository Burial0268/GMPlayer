import { computed, markRaw, reactive } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import { fetch as tauriFetch } from "@tauri-apps/plugin-http";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import { openExternalUrl } from "@/utils/openLink";
import { isMobileDevice, isTauri, windowManager } from "@/utils/tauri";

// tauri-plugin-updater 在 Android / iOS 上没有实现（也无法静默安装 APK），
// 移动端只比对版本，再把用户带到 Release 页自行下载。这两个地址要和
// tauri.conf.json 里 updater 的 endpoints 保持一致。
const MOBILE_MANIFEST_URL =
  "https://github.com/Burial0268/GMPlayer/releases/download/master-player-dev/latest.json";
const MOBILE_RELEASE_URL = "https://github.com/Burial0268/GMPlayer/releases/tag/master-player-dev";

// 只有 Android 有 APK 可下载；iOS 没有构建产物，也没有对应的版本号约定。
const isAndroid = () => /Android/i.test(navigator.userAgent);

// 版本形如 `0.1.1+<提交数>`（scripts/gen-dev-version.mjs），semver 会忽略 `+` 之后的部分，
// 所以这里把它当作第四段数字比较。
const versionParts = (v: string) => {
  const [base, build = "0"] = v.split("+");
  return [...base.split(".").map(Number), Number(build)];
};
const isNewerVersion = (latest: string, current: string) => {
  const a = versionParts(latest);
  const b = versionParts(current);
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return a[i] > b[i];
  return false;
};

/** 移动端的「检查更新」：界面只读 version / currentVersion / body，所以不必构造真正的 Update。 */
const checkMobileUpdate = async (): Promise<Update | null> => {
  const res = await tauriFetch(MOBILE_MANIFEST_URL);
  if (!res.ok) throw new Error(`latest.json: HTTP ${res.status}`);
  const manifest = (await res.json()) as { version: string; notes?: string };
  const currentVersion = await getVersion();
  if (!isNewerVersion(manifest.version, currentVersion)) return null;
  return { version: manifest.version, currentVersion, body: manifest.notes } as unknown as Update;
};

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
let activeInstall: Promise<boolean> | null = null;

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
  if (typeof error === "object" && error && "message" in error) {
    return String((error as { message?: unknown }).message ?? error);
  }
  return String(error ?? "");
};

const reportUpdaterError = (phase: string, error: unknown) => {
  const message = normalizeError(error);
  console.error(`[app-updater] ${phase} failed:`, error);
  return message;
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

/**
 * Persist what the run loop would have persisted, because the installer will
 * not let it run. Best effort: a failure here is not worth aborting an update
 * the user already accepted.
 */
const flushPersistedStateBeforeExit = async () => {
  try {
    await windowManager.flushPersistedState();
  } catch (error) {
    console.warn("[app-updater] flushing persisted state failed:", error);
  }
};

export function useAppUpdater() {
  const linkOnly = isAndroid();
  // 其余移动端（iOS）既没有官方 updater 也没有可下载的包，不检查。
  const supported = computed(() => isTauri() && (linkOnly || !isMobileDevice()));
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

    activeCheck = (linkOnly ? checkMobileUpdate() : check())
      .then(async (update) => {
        state.checked = true;
        state.lastCheckedAt = Date.now();
        if (update) {
          await releaseUpdate();
          state.update = markRaw(update);
          state.status = "available";
        } else {
          await releaseUpdate();
          state.status = "not-available";
        }
        return update;
      })
      .catch((error) => {
        state.error = reportUpdaterError("check", error);
        state.status = options.silent ? "idle" : "error";
        return null;
      })
      .finally(() => {
        activeCheck = null;
      });

    return activeCheck;
  };

  const installAvailableUpdate = async () => {
    if (activeInstall) return activeInstall;

    activeInstall = (async () => {
      const update = state.update ?? (await checkForUpdate());
      if (!update) return false;

      state.error = "";
      state.status = "downloading";
      resetDownloadStats();

      try {
        await update.download((event: DownloadEvent) => {
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
            // Sample roughly 4x/sec and smooth with an EMA to avoid a jittery readout.
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

        // Deliberately not `downloadAndInstall`: `install()` hands off to the
        // NSIS/MSI installer and ends in `std::process::exit(0)`, which skips
        // the Tauri run loop — so `RunEvent::Exit` never fires and the two
        // plugins that only write there (window-state, pinia) lose everything
        // they were holding. That is why window geometry came back wrong after
        // an update. Splitting the call gives us a point where the process is
        // still ours; doing it from the `Finished` callback would not, since
        // nothing on the Rust side waits for that channel message.
        await flushPersistedStateBeforeExit();

        await update.install();
        state.installedVersion = update.version;
        state.status = "installed";
        await releaseUpdate();
        return true;
      } catch (error) {
        state.error = reportUpdaterError("install", error);
        state.status = "error";
        return false;
      }
    })().finally(() => {
      activeInstall = null;
    });

    return activeInstall;
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
    linkOnly,
    openReleasePage: () => openExternalUrl(MOBILE_RELEASE_URL),
  };
}
