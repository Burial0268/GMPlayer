/**
 * TTML 原文的落盘缓存（IndexedDB，Web 与 Tauri 共用）。
 *
 * 缓存的是 AMLL TTML DB 的原文而不是解析结果：解析器和 AMLL 数据结构会升级，
 * 原文不会，所以这里不需要版本迁移。按平台 ID 查到的结果会随词库更新变化
 * （官方文档只担保 id / filename 查询是永久不变的），因此设有过期；过期后仍可在
 * 网络失败时兜底返回。
 */

const DB_NAME = "gmplayer-ttml";
const STORE = "ncm";
/** 命中缓存的有效期 */
const POSITIVE_TTL_MS = 7 * 24 * 60 * 60 * 1000;
/** 「词库里没有这首」的有效期，避免每首没有逐字词的歌都把四个镜像打一遍 */
const NEGATIVE_TTL_MS = 72 * 60 * 60 * 1000;
/** 单条 TTML 约数十 KB，超出就按写入时间淘汰最旧的 */
const MAX_ENTRIES = 300;

interface Entry {
  id: number;
  /** null 表示负缓存 */
  content: string | null;
  fetchedAt: number;
}

let dbPromise: Promise<IDBDatabase> | null = null;

function openDb(): Promise<IDBDatabase> {
  dbPromise ??= new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, 1);
    req.onupgradeneeded = () => {
      const store = req.result.createObjectStore(STORE, { keyPath: "id" });
      store.createIndex("fetchedAt", "fetchedAt");
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
  // 打不开（隐私模式等）不能把失败缓存住，下次还可以再试
  dbPromise.catch(() => (dbPromise = null));
  return dbPromise;
}

const wrap = <T>(req: IDBRequest<T>): Promise<T> =>
  new Promise((resolve, reject) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });

/** 读缓存行；缓存不可用时当作未命中，不影响取词 */
export async function getCachedTTML(
  id: number,
): Promise<{ content: string | null; fresh: boolean } | null> {
  try {
    const db = await openDb();
    const row = (await wrap(db.transaction(STORE).objectStore(STORE).get(id))) as Entry | undefined;
    if (!row) return null;
    const ttl = row.content === null ? NEGATIVE_TTL_MS : POSITIVE_TTL_MS;
    return { content: row.content, fresh: Date.now() - row.fetchedAt < ttl };
  } catch (error) {
    console.warn("[ttmlCache] read failed:", error);
    return null;
  }
}

/** 写缓存；content 为 null 记负缓存 */
export async function setCachedTTML(id: number, content: string | null): Promise<void> {
  try {
    const db = await openDb();
    const store = db.transaction(STORE, "readwrite").objectStore(STORE);
    await wrap(store.put({ id, content, fetchedAt: Date.now() } satisfies Entry));

    const count = await wrap(store.count());
    if (count <= MAX_ENTRIES) return;
    // fetchedAt 索引升序 = 最旧在前
    let excess = count - MAX_ENTRIES;
    store.index("fetchedAt").openKeyCursor().onsuccess = (event) => {
      const cursor = (event.target as IDBRequest<IDBCursor | null>).result;
      if (!cursor || excess-- <= 0) return;
      store.delete(cursor.primaryKey);
      cursor.continue();
    };
  } catch (error) {
    console.warn("[ttmlCache] write failed:", error);
  }
}
