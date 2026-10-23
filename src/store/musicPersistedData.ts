import { acceptHMRUpdate, defineStore } from "pinia";
import { reactive, toRaw, watch } from "vue";
import { throttle } from "throttle-debounce";
import { createDefaultPersistData, type PersistData } from "./musicTypes";
import { asRawEntry } from "@/utils/rawEntry";

const STORAGE_KEY = "musicData";
// 首个改动立即落盘，之后最多每 400ms 一次（含尾调用）。
// 切歌 / 换队列这类孤立改动仍然是同步持久化的，只有连续突发
// （音量滑块每次 pointermove 都会写 persistData）才会被合并。
const PERSIST_THROTTLE_MS = 400;

// 「系统重置」会 localStorage.clear() 后再 reload。挂起标记保证在那之后
// 不会有残留的节流尾调用或 pagehide 补写把队列重新写回去。
let persistSuspended = false;

/** 供系统重置流程调用：此后不再写入 localStorage。 */
export const suspendMusicPersist = (): void => {
  persistSuspended = true;
};

const isPlainObject = (value: unknown): value is Record<string, any> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/**
 * 复刻 pinia $patch 的合并语义：普通对象递归合并，数组与基础类型整体替换。
 * 这样即使 localStorage 里是旧版本、缺字段的数据，也能保留新默认值。
 */
const mergeStored = (target: Record<string, any>, source: Record<string, any>): void => {
  for (const key of Object.keys(source)) {
    const incoming = source[key];
    if (incoming === undefined) continue;
    const current = target[key];
    if (isPlainObject(current) && isPlainObject(incoming)) {
      mergeStored(current, incoming);
    } else {
      target[key] = incoming;
    }
  }
};

/**
 * 落盘的歌曲条目重新读回来时是全新的普通对象，必须在并入 reactive 之前
 * 标记为 raw：一旦它们进了 persistData，之后任何一次读取都会就地生成 Proxy
 * 并登记依赖，恢复千首队列时正是这一步最贵。见 utils/rawEntry。
 *
 * 只处理这两个数组：mergeStored 对数组是整块赋值，标记因此能保留下来；
 * personalFmData 走的是逐字段合并分支（落进默认对象），在这里标记没有意义，
 * 它会在下一次 setPersonalFmData 时被整体替换成已标记的对象。
 */
const markStoredSongsRaw = (stored: Record<string, any>): void => {
  for (const key of ["playlists", "playHistory"]) {
    const list = stored[key];
    if (!Array.isArray(list)) continue;
    for (let i = 0; i < list.length; i++) asRawEntry(list[i]);
  }
};

const hydrate = (persistData: PersistData): void => {
  if (typeof window === "undefined") return;
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return;
    const parsed = JSON.parse(raw);
    if (isPlainObject(parsed) && isPlainObject(parsed.persistData)) {
      markStoredSongsRaw(parsed.persistData);
      mergeStored(persistData, parsed.persistData);
    }
  } catch (error) {
    console.error("[musicPersistedData] 读取本地数据失败", error);
  }
};

export const useMusicPersistedDataStore = defineStore("musicPersistedData", () => {
  const persistData = reactive<PersistData>(createDefaultPersistData());

  // 必须在 return 之前同步完成：musicData 的 state() 会立刻读取
  // persistData.playlists / playSongIndex 来推导 playingSongId。
  hydrate(persistData);

  // 这里刻意不使用 pinia-plugin-persistedstate：它在每次 mutation 上同步执行
  // JSON.stringify + localStorage.setItem，而 persistData 里装着整条播放列表
  // （千首歌量级、数百 KB）。音量滑块按 pointermove 频率写 playVolume，
  // 于是拖动一次音量就会把整条队列反复序列化上百次。
  // toRaw 让 stringify 走原始对象而不是 reactive 代理。
  let dirty = false;

  const writeNow = (): void => {
    if (persistSuspended || !dirty || typeof window === "undefined") return;
    dirty = false;
    try {
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify({ persistData: toRaw(persistData) }));
    } catch (error) {
      console.error("[musicPersistedData] 写入本地数据失败", error);
    }
  };

  const schedulePersist = throttle(PERSIST_THROTTLE_MS, writeNow);

  watch(
    persistData,
    () => {
      dirty = true;
      schedulePersist();
    },
    { deep: true },
  );

  if (typeof window !== "undefined") {
    // 页面隐藏 / 卸载时补写节流窗口内未落盘的改动。dirty 判定保证
    // 「没有改动」时不写——否则系统重置清空 localStorage 后会被重新写回。
    window.addEventListener("pagehide", writeNow);
    document.addEventListener("visibilitychange", () => {
      if (document.hidden) writeNow();
    });
  }

  return { persistData };
});

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useMusicPersistedDataStore, import.meta.hot));
}

export default useMusicPersistedDataStore;
