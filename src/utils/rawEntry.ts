import { markRaw, toRaw } from "vue";

/**
 * 把「只读列表条目」标记为 raw，使其永远不进入深层响应式。
 *
 * 歌曲、歌单、专辑、歌手这类条目一经构造就不再改写：变化的是数组本身
 * （长度、顺序、当前索引），不是条目内部的字段。但只要它们躺在 reactive
 * 状态里，任何一次读取都会就地生成 Proxy 并为每个 key 登记 Dep + Link；
 * 而 store 的持久化 watcher 是 deep 的，会把整张列表一次性全部展开。
 *
 * 一首歌大约 40 个可追踪 key（自身 12 + album 8 + artist 数组与其成员），
 * 每个 key 一份 Dep + Link + Map 槽位。千首规模的队列光依赖图就是数 MB，
 * 是条目原始数据本身的十倍量级 —— 而这些依赖没有任何一个会被触发。
 *
 * 标成 raw 之后：reactive() 原样返回对象，模板照常读字段，
 * deep traverse 在这一层短路（traverse 会检查 `__v_skip`）。
 *
 * 传进来的值可能已经是响应式代理（模板直接把列表项交给 store），
 * 所以先 toRaw 再标记，避免把 `__v_skip` 透过代理写回目标、触发一次无谓更新。
 * `markRaw` 用的是不可枚举属性，因此不会被 JSON.stringify 带进 localStorage。
 */
export const asRawEntry = <T>(entry: T): T => {
  if (!entry || typeof entry !== "object") return entry;
  return markRaw(toRaw(entry as object)) as T;
};

/** 返回逐项标记过的新数组，可直接替代调用点原本的 `.slice()`。 */
export const asRawEntries = <T>(entries: readonly T[]): T[] =>
  entries.map((entry) => asRawEntry(entry));
