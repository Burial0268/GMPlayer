import { acceptHMRUpdate, defineStore } from "pinia";
import { reactive, toRaw, watch } from "vue";
import { throttle } from "throttle-debounce";
import { createDefaultPersistData, type PersistData } from "./musicTypes";
import { asRawEntry } from "@/utils/rawEntry";

/**
 * persistData 落在 IndexedDB，不在 localStorage。
 *
 * 它装着整条播放队列（千首量级、数 MB）。放在 localStorage 有两个代价：每个碰过
 * localStorage 的窗口渲染进程都缓存整份数据（从窗口只读设置也要连带装进队列），
 * 以及 ~5 MB 的配额——超了以后写入静默失败，索引、音量、shuffleState 全都不再落盘。
 *
 * 分三条记录存 JSON 文本：`queue`（playlists）、`history`（playHistory）、`state`
 * （其余）。拆开是为了拖音量不再重新序列化整条队列；三条总在同一个事务里写，
 * 所以 playSongIndex / shuffleState 与它们描述的队列始终一起落盘。存 JSON 而不是
 * structured clone：语义与旧的 localStorage 数据逐字节一致，条目里混进的 Proxy
 * 之类也不会让写入抛 DataCloneError。
 *
 * IndexedDB 只有异步接口，而 musicData 的 `state()` 要同步读 persistData，所以
 * main.ts 在任何 store 实例化之前 `await loadMusicPersist()`，hydrate 读的是预载
 * 好的快照。
 */
const LEGACY_STORAGE_KEY = "musicData";
const DB_NAME = "gmplayer-music";
const STORE = "persist";
/** 挂载路径上的 await，IndexedDB 卡住不能把启动卡死。 */
const IDB_TIMEOUT_MS = 3000;
// 首个改动立即落盘，之后最多每 400ms 一次（含尾调用）。
// 切歌 / 换队列这类孤立改动仍然是立即持久化的，只有连续突发
// （音量滑块每次 pointermove 都会写 persistData）才会被合并。
const PERSIST_THROTTLE_MS = 400;

/**
 * - `idb`：正常路径。
 * - `local`：IndexedDB 打不开或写失败，退回旧的 localStorage 整块写。localStorage
 *   里有 `musicData` 就说明它是最新的一份（迁移成功后会删掉它），下次启动会被重新
 *   迁进 IndexedDB——所以两个写者不会互相覆盖出旧数据。
 * - `off`：不写。读 IndexedDB 失败时用，宁可这次不落盘也不能拿默认值盖掉读不到的
 *   用户数据；`loadMusicPersist` 没跑过（从窗口）也是这个状态。
 */
type PersistMode = "idb" | "local" | "off";

let mode: PersistMode = "off";
let db: IDBDatabase | null = null;
/** `undefined` = loadMusicPersist 没跑过；`null` = 跑过但没有存档。 */
let preloaded: Record<string, any> | null | undefined;
/** 快照已被某个 store 实例取走。 */
let consumed = false;
/** 写者实例注册的立即落盘入口，见 flushMusicPersist。 */
let flushImpl: (() => Promise<void>) | null = null;

// 「系统重置」之后不能再有残留的节流尾调用或 pagehide 补写把队列写回去。
let persistSuspended = false;

const isPlainObject = (value: unknown): value is Record<string, any> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const withTimeout = <T>(task: Promise<T>, label: string): Promise<T> =>
  new Promise<T>((resolve, reject) => {
    const timer = setTimeout(
      () => reject(new Error(`${label} timed out after ${IDB_TIMEOUT_MS}ms`)),
      IDB_TIMEOUT_MS,
    );
    task.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (error) => {
        clearTimeout(timer);
        reject(error);
      },
    );
  });

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const factory: IDBFactory | undefined = window.indexedDB;
    if (!factory) {
      reject(new Error("indexedDB unavailable"));
      return;
    }
    const req = factory.open(DB_NAME, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

const runTx = (
  database: IDBDatabase,
  txMode: IDBTransactionMode,
  body: (store: IDBObjectStore) => void,
  durability?: IDBTransactionDurability,
): Promise<void> =>
  new Promise((resolve, reject) => {
    const tx = database.transaction(STORE, txMode, durability ? { durability } : undefined);
    body(tx.objectStore(STORE));
    tx.oncomplete = () => resolve();
    tx.onerror = tx.onabort = () => reject(tx.error);
  });

/** persistData 拆成三条记录的 JSON 文本。数组缺失按空数组写，和默认值等价。 */
const splitRecords = (data: Record<string, any>, parts: Set<string>): [string, string][] => {
  const { playlists, playHistory, ...rest } = data;
  const records: [string, string][] = [["state", JSON.stringify(rest)]];
  if (parts.has("queue")) records.push(["queue", JSON.stringify(playlists ?? [])]);
  if (parts.has("history")) records.push(["history", JSON.stringify(playHistory ?? [])]);
  return records;
};

const ALL_PARTS = new Set(["queue", "history"]);

async function readRecords(database: IDBDatabase): Promise<Record<string, any> | null> {
  const raw: Record<string, string | undefined> = {};
  await runTx(database, "readonly", (store) => {
    for (const key of ["state", "queue", "history"]) {
      const req = store.get(key);
      req.onsuccess = () => (raw[key] = req.result);
    }
  });
  // 三条总是一起写的，state 就是「有没有存档」的标记。
  if (raw.state === undefined) return null;
  try {
    const data = JSON.parse(raw.state);
    if (!isPlainObject(data)) return null;
    if (raw.queue !== undefined) data.playlists = JSON.parse(raw.queue);
    if (raw.history !== undefined) data.playHistory = JSON.parse(raw.history);
    return data;
  } catch (error) {
    // 内容损坏：与旧实现读到坏 JSON 一样，用默认值起步并覆盖它。
    console.error("[musicPersistedData] 存档损坏，已忽略", error);
    return null;
  }
}

const readLegacy = (): Record<string, any> | null => {
  try {
    const raw = window.localStorage.getItem(LEGACY_STORAGE_KEY);
    if (raw === null) return null;
    const parsed = JSON.parse(raw);
    return isPlainObject(parsed) && isPlainObject(parsed.persistData) ? parsed.persistData : null;
  } catch (error) {
    console.error("[musicPersistedData] 读取 localStorage 存档失败", error);
    return null;
  }
};

const hasLegacy = (): boolean => {
  try {
    return window.localStorage.getItem(LEGACY_STORAGE_KEY) !== null;
  } catch {
    return false;
  }
};

/**
 * 启动时调用一次，必须在 musicPersistedData store 实例化之前完成。永远 resolve。
 *
 * localStorage 里有 `musicData` 时以它为准（第一次升级，或上一次会话退回过
 * localStorage）：写进 IndexedDB，事务以 strict 持久性提交之后才删掉旧的那份。
 * 写失败就保留旧数据，本次会话继续用 localStorage。
 */
export async function loadMusicPersist(): Promise<void> {
  if (typeof window === "undefined" || preloaded !== undefined || consumed) return;
  const legacyPresent = hasLegacy();
  const legacy = legacyPresent ? readLegacy() : null;

  try {
    db = await withTimeout(openDb(), "open");
  } catch (error) {
    console.error("[musicPersistedData] IndexedDB 不可用，退回 localStorage", error);
    mode = "local";
    preloaded = legacy;
    return;
  }

  if (legacy) {
    preloaded = legacy;
    try {
      const records = splitRecords(legacy, ALL_PARTS);
      await withTimeout(
        runTx(db, "readwrite", (store) => records.forEach(([k, v]) => store.put(v, k)), "strict"),
        "migrate",
      );
      window.localStorage.removeItem(LEGACY_STORAGE_KEY);
      mode = "idb";
      console.info("[musicPersistedData] 已从 localStorage 迁移到 IndexedDB");
    } catch (error) {
      console.error("[musicPersistedData] 迁移失败，保留 localStorage 存档", error);
      mode = "local";
    }
    return;
  }

  // 有键但解析不了：旧实现会用默认值覆盖它，这里同样丢弃，免得它下次被当成最新存档。
  if (legacyPresent) window.localStorage.removeItem(LEGACY_STORAGE_KEY);

  try {
    preloaded = await withTimeout(readRecords(db), "read");
    mode = "idb";
  } catch (error) {
    console.error("[musicPersistedData] 读取 IndexedDB 失败，本次不落盘", error);
    preloaded = null;
    mode = "off";
  }
}

/**
 * 立即写出完整存档并等它提交。退出前、进入后台时用：节流窗口里的改动和刚写的
 * 播放位置不能等下一次节流。本窗口没有写者实例时什么也不做。
 */
export const flushMusicPersist = (): Promise<void> => flushImpl?.() ?? Promise.resolve();

/** 一次落盘只重写小的 `state` 记录。退回 localStorage 时每次都要整块重写队列。 */
export const isMusicPersistCheap = (): boolean => mode === "idb";

/**
 * 「系统重置」用：此后不再写入，并清空 IndexedDB 存档。
 * 从设置窗口调用时本窗口没打开过数据库，所以按需打开。
 */
export async function clearMusicPersist(): Promise<void> {
  persistSuspended = true;
  try {
    const database = db ?? (await withTimeout(openDb(), "open"));
    await withTimeout(
      runTx(database, "readwrite", (store) => store.clear()),
      "clear",
    );
  } catch (error) {
    console.error("[musicPersistedData] 清空 IndexedDB 存档失败", error);
  }
}

/**
 * 复刻 pinia $patch 的合并语义：普通对象递归合并，数组与基础类型整体替换。
 * 这样即使存档是旧版本、缺字段的数据，也能保留新默认值。
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

/** 返回本实例是否是存档的写者。没有预载、或快照已被别的实例取走时不写，免得拿默认值覆盖存档。 */
const hydrate = (persistData: PersistData): boolean => {
  if (preloaded === undefined || consumed) {
    console.warn(
      "[musicPersistedData] 没有可用的预载快照（loadMusicPersist 未先完成），本实例不落盘",
    );
    return false;
  }
  consumed = true;
  const stored = preloaded;
  // 快照只用一次；store 接管之后不再持有第二份队列。
  preloaded = null;
  if (!stored) return true;
  try {
    // Older queues used the restore snapshot as their completion marker.
    if (stored.shuffleState === undefined) {
      stored.shuffleState =
        stored.playSongMode === "random" && stored.preShuffleOrder?.length
          ? {
              id: `legacy:${stored.pendingReshuffleRevision ?? "applied"}`,
              applied:
                stored.pendingReshuffleRevision === null ||
                stored.pendingReshuffleRevision === undefined,
            }
          : null;
    }
    delete stored.pendingReshuffleRevision;
    markStoredSongsRaw(stored);
    mergeStored(persistData, stored);
  } catch (error) {
    console.error("[musicPersistedData] 读取本地数据失败", error);
  }
  return true;
};

export const useMusicPersistedDataStore = defineStore("musicPersistedData", () => {
  const persistData = reactive<PersistData>(createDefaultPersistData());

  // 必须在 return 之前同步完成：musicData 的 state() 会立刻读取
  // persistData.playlists / playSongIndex 来推导 playingSongId。
  const owner = hydrate(persistData);

  // 这里刻意不使用 pinia-plugin-persistedstate：它在每次 mutation 上同步序列化
  // 整个 store。dirtyParts 记录哪几条大记录需要重写，state 每次都写（很小）。
  let dirty = false;
  const dirtyParts = new Set<string>();

  const writeLocal = (): void => {
    try {
      window.localStorage.setItem(
        LEGACY_STORAGE_KEY,
        JSON.stringify({ persistData: toRaw(persistData) }),
      );
    } catch (error) {
      console.error("[musicPersistedData] 写入本地数据失败", error);
    }
  };

  /**
   * `force` 写全部三条记录并等事务提交，给退出 / 进入后台用：此时不能等节流，也不能
   * 依赖 watcher 已经标好脏位（同一 tick 里刚改的值它们还没看到）。
   */
  const persist = async (force: boolean): Promise<void> => {
    if (!owner || persistSuspended || mode === "off" || typeof window === "undefined") return;
    if (!force && !dirty) return;
    dirty = false;
    if (mode === "local" || !db) {
      dirtyParts.clear();
      writeLocal();
      return;
    }
    let records: [string, string][];
    try {
      records = splitRecords(toRaw(persistData), force ? ALL_PARTS : dirtyParts);
    } catch (error) {
      console.error("[musicPersistedData] 序列化失败", error);
      return;
    }
    dirtyParts.clear();
    try {
      // 同一个 store 上的 readwrite 事务按创建顺序执行，不会乱序覆盖。
      await runTx(
        db,
        "readwrite",
        (store) => records.forEach(([k, v]) => store.put(v, k)),
        force ? "strict" : undefined,
      );
    } catch (error) {
      // 写不进 IndexedDB 就整块写回 localStorage：它在下次启动时会被当成最新存档。
      console.error("[musicPersistedData] 写入 IndexedDB 失败，退回 localStorage", error);
      if (persistSuspended) return;
      mode = "local";
      writeLocal();
    }
  };

  const writeNow = (): void => void persist(false);
  // 只有写者能登记，后建的非写者实例不能把它顶掉。
  if (owner) flushImpl = () => persist(true);

  const schedulePersist = throttle(PERSIST_THROTTLE_MS, writeNow);

  // 同一次 flush 里几个 watcher 先后触发；推迟到微任务再写，保证队列和它的索引、
  // shuffleState 落进同一个事务（throttle 的前沿调用是同步的）。
  let writeQueued = false;
  const markDirty = (part?: string): void => {
    dirty = true;
    if (part) dirtyParts.add(part);
    if (writeQueued) return;
    writeQueued = true;
    queueMicrotask(() => {
      writeQueued = false;
      schedulePersist();
    });
  };

  // 条目是 markRaw 的，deep 只遍历数组本身：替换、push、splice、按下标赋值都能捕获。
  watch(
    () => persistData.playlists,
    () => markDirty("queue"),
    { deep: true },
  );
  watch(
    () => persistData.playHistory,
    () => markDirty("history"),
    { deep: true },
  );
  watch(persistData, () => markDirty(), { deep: true });

  if (typeof window !== "undefined") {
    // 页面隐藏 / 卸载时补写节流窗口内未落盘的改动。dirty 判定保证
    // 「没有改动」时不写——否则系统重置清空之后会被重新写回。
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
