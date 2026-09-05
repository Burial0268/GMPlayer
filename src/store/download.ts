import { acceptHMRUpdate, defineStore } from "pinia";
import {
  type DownloadEvent,
  type DownloadRequest,
  type DownloadSettings,
  type DownloadTask,
  downloadAvailable,
  downloadCancel,
  downloadClearFinished,
  downloadConfigSet,
  downloadDirPick,
  downloadEnqueue,
  downloadList,
  downloadPause,
  downloadResume,
  downloadRetry,
  downloadSettingsGet,
  downloadSubscribe,
  isNeedsDirectory,
} from "@/utils/download";
import { notifyLocalTracksChanged } from "@/utils/localLibraryMutations";

/**
 * Locators of finished downloads waiting to be announced, and the timer that will.
 *
 * Coalesced rather than announced per task, and with a handle of its own rather
 * than through `utils/debounce`, whose timer is a single module-level one shared
 * app-wide. A 300-track album finishing would otherwise fire 300 separate
 * signals, and each one costs the library store a re-page, a `loadGroups` and a
 * `hydrate` — the list would spend the whole batch reloading. One signal carrying
 * many keys is exactly the shape `notifyLocalTracksChanged` is built for.
 */
const landed = new Set<string>();
let landedTimer: ReturnType<typeof setTimeout> | null = null;

/** How long to keep collecting before telling the library. */
const ANNOUNCE_AFTER = 400;

const announceLanded = () => {
  landedTimer = null;
  if (!landed.size) return;
  const keys = [...landed];
  landed.clear();
  notifyLocalTracksChanged({ keys, kind: "added" });
};

const noteLanded = (locator: string) => {
  landed.add(locator);
  if (landedTimer) return;
  landedTimer = setTimeout(announceLanded, ANNOUNCE_AFTER);
};

interface DownloadStoreState {
  /** Mirror of the Rust task table. Never the source of truth. */
  tasks: DownloadTask[];
  settings: DownloadSettings | null;
  /** Whether `hydrate` has run, so the modal can open without a flash of empty. */
  hydrated: boolean;
}

/**
 * The download queue, as the UI sees it.
 *
 * **Deliberately not persisted.** Rust owns both halves — the settings live in
 * `$APPDATA/download.json` and the task table is in-memory there — so anything
 * kept here would be a second copy that can only be wrong. It also keeps a
 * possibly-large task list out of `persistData`, which is localStorage plus a
 * throttled whole-object `JSON.stringify`.
 *
 * Every mutation is a command; the resulting state arrives back over the event
 * channel. That one-writer shape is what lets the queue keep running when this
 * page is gone, which on Android is normal rather than exceptional.
 */
const useDownloadStore = defineStore("download", {
  state: (): DownloadStoreState => ({
    tasks: [],
    settings: null,
    hydrated: false,
  }),
  getters: {
    /** Tasks still going somewhere, newest last. */
    active: (state) =>
      state.tasks.filter((task) =>
        ["queued", "resolving", "downloading", "paused"].includes(task.status),
      ),
    finished: (state) =>
      state.tasks.filter((task) => ["done", "failed", "skipped"].includes(task.status)),
    activeCount(): number {
      return this.active.length;
    },
    /** Whether anything is worth showing a badge for. */
    busy(): boolean {
      return this.active.length > 0;
    },
  },
  actions: {
    /**
     * Read the settings and the current table, and subscribe.
     *
     * Idempotent and cheap enough to call from any surface that shows download
     * state. The subscribe is re-issued on every call on purpose: Rust keeps one
     * channel and replaces it, so a page reload has to re-register or the queue
     * would be pushing into a channel nobody is listening on.
     */
    async hydrate() {
      if (!downloadAvailable()) {
        this.hydrated = true;
        return;
      }
      try {
        const [settings, tasks] = await Promise.all([downloadSettingsGet(), downloadList()]);
        this.settings = settings;
        this.tasks = tasks;
        await downloadSubscribe((event) => this.apply(event));
        this.hydrated = true;
      } catch (error) {
        console.error("[download] could not hydrate the queue:", error);
      }
    },

    /** Fold one pushed event into the mirror. */
    apply(event: DownloadEvent) {
      if (event.type === "snapshot") {
        this.tasks = event.tasks;
        return;
      }
      if (event.type === "configChanged") {
        // `defaultDir`/`requiresPick` are platform facts that do not change, so the
        // patch keeps whatever the first `download_settings_get` reported.
        this.settings = this.settings
          ? { ...this.settings, ...event.config }
          : { ...event.config, defaultDir: "", requiresPick: false };
        return;
      }
      const index = this.tasks.findIndex((task) => task.id === event.task.id);
      // Announced on the *transition* into a terminal state, not on every update:
      // a task keeps pushing progress events, and Rust writes the locator only
      // once the file is published and indexed.
      const previous = index === -1 ? null : this.tasks[index].status;
      if (
        previous !== event.task.status &&
        (event.task.status === "done" || event.task.status === "skipped") &&
        event.task.locator
      ) {
        noteLanded(event.task.locator);
      }
      if (index === -1) {
        this.tasks.push(event.task);
      } else {
        this.tasks[index] = event.task;
      }
    },

    /**
     * Queue songs, opening the directory picker first if Android has none.
     *
     * Retried exactly once after a successful pick: a second refusal means the
     * user cancelled, and asking again in a loop would be a trap. Returns the
     * number of tasks actually added — the queue drops duplicates of anything
     * already in flight, so "download all" pressed twice is not double the work.
     */
    async enqueue(items: DownloadRequest[]): Promise<number> {
      if (!downloadAvailable() || items.length === 0) return 0;
      try {
        const added = await downloadEnqueue(items);
        return added.length;
      } catch (error) {
        if (!isNeedsDirectory(error)) throw error;
        const picked = await downloadDirPick();
        if (!picked) return 0;
        this.settings = picked;
        const added = await downloadEnqueue(items);
        return added.length;
      }
    },

    async pick(): Promise<boolean> {
      const picked = await downloadDirPick();
      if (!picked) return false;
      this.settings = picked;
      return true;
    },

    async patchConfig(patch: Partial<DownloadSettings>) {
      if (!this.settings) await this.hydrate();
      if (!this.settings) return;
      const {
        defaultDir: _defaultDir,
        requiresPick: _requiresPick,
        ...config
      } = {
        ...this.settings,
        ...patch,
      };
      this.settings = await downloadConfigSet(config);
    },

    pause(id: string) {
      return downloadPause(id);
    },
    resume(id: string) {
      return downloadResume(id);
    },
    retry(id: string) {
      return downloadRetry(id);
    },
    remove(id: string) {
      return downloadCancel(id);
    },
    clearFinished() {
      return downloadClearFinished();
    },
  },
});

export default useDownloadStore;

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useDownloadStore, import.meta.hot));
}
