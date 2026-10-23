import { throttle } from "throttle-debounce";

/**
 * 写入合并的 localStorage 门面，给 pinia-plugin-persistedstate 用。
 *
 * persistedstate 在**每一次** mutation 上同步执行 serialize + `setItem`。
 * settingData 有 73 个键（含最多 31 段 EQ，序列化约 2.7 KB），而 DSP 增益、
 * 字号偏移这些是 `n-slider` 按 pointermove 频率写的——拖一次滑块就会同步写
 * localStorage 上百次，每次都是一趟到存储后端的同步调用。
 *
 * 这里只合并写入，不改变读语义：
 *   - 首个改动立即落盘（throttle 默认带前沿调用），所以「改一次语言」这类
 *     孤立改动仍然是同步持久化的——slave-main 启动时直读 localStorage 取
 *     语言就依赖这一点。
 *   - 之后最多每 delayMs 一次，含尾调用。
 *   - `getItem` 优先读挂起值，保证 read-after-write 一致。
 *
 * 与 musicPersistedData 是同一套思路，见那里的注释。
 */
export function createThrottledStorage(delayMs: number) {
  const pending = new Map<string, string>();
  let suspended = false;

  const writeNow = (): void => {
    if (suspended || pending.size === 0) return;
    for (const [key, value] of pending) {
      try {
        localStorage.setItem(key, value);
      } catch (error) {
        console.error("[throttledStorage] 写入失败", key, error);
      }
    }
    pending.clear();
  };

  const schedule = throttle(delayMs, writeNow);

  if (typeof window !== "undefined") {
    // 页面隐藏 / 卸载时补写节流窗口内未落盘的改动。
    window.addEventListener("pagehide", writeNow);
    document.addEventListener("visibilitychange", () => {
      if (document.hidden) writeNow();
    });
  }

  return {
    getItem: (key: string): string | null =>
      suspended ? null : (pending.get(key) ?? localStorage.getItem(key)),
    setItem: (key: string, value: string): void => {
      if (suspended || pending.get(key) === value) return;
      pending.set(key, value);
      schedule();
    },
    removeItem: (key: string): void => {
      pending.delete(key);
      if (!suspended) localStorage.removeItem(key);
    },
    /**
     * 「系统重置」用：丢弃挂起的写入并从此不再写。
     *
     * 不能用 flush 代替——重置的顺序是 clear 之后立刻 reload，若在 clear 之后
     * 还有节流尾调用落地，刚清掉的设置会被原样写回去。与 musicPersistedData
     * 的 suspendMusicPersist 是同一个理由。
     */
    suspend: (): void => {
      suspended = true;
      pending.clear();
    },
  };
}
