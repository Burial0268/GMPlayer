/**
 * Tauri 后端的 Pinia 持久化层。
 *
 * 这是**分层**而不是替换：
 *   - `pinia-plugin-persistedstate` → localStorage，Web 与 Tauri 都跑。它是
 *     同步 hydrate 的，负责「首帧就有正确的主题 / 语言 / 登录态」，同时也是
 *     本次迁移的数据来源——老用户的数据全都在 localStorage 里。
 *   - `@tauri-store/pinia` → Tauri store 文件，只在 Tauri 里注册。它提供落盘到
 *     真实文件、跨窗口实时同步、以及 Rust 侧可读的状态。
 *
 * 之所以不能只留后者：它的 hydrate 入口 `$tauri.start()` 是异步 IPC，而
 * `useI18n()`、主题应用、登录态判断都在挂载前**同步**读这些 store，纯异步
 * hydrate 会留下一帧默认值闪烁。这里的做法是启动时 await 一次 `start()`，
 * 在挂载之前就把后端状态合并进来，挂载后不会再有状态跳变。
 *
 * 哪些 store 被托管由 store 自己的 `tauri` 选项决定（见 settingData /
 * siteData / userData）。默认 `save: false, sync: false`，所以没有显式开启的
 * store（musicData、musicPersistedData、listenTogether）只走 localStorage。
 * musicPersistedData 尤其要留在原地：它装着整条播放列表，有自己的节流落盘
 * 方案，而且 musicData 的 `state()` 会同步读它推导 playingSongId。
 */
import type { Pinia } from "pinia";
import type { TauriStoreContract } from "@tauri-store/pinia";
import { invoke, isTauri } from "../core/runtime";

/** 具备 Tauri 持久化能力的 store 实例（`$tauri` 由插件注入）。 */
export interface TauriPersistedStore {
  readonly $id: string;
  readonly $tauri: TauriStoreContract;
}

/**
 * 托管条目。`onHydrated` 在后端状态合并进 store 之后运行一次。
 *
 * 版本迁移的「修复步」必须挂在这里，不能只挂 persistedstate 的 afterHydrate：
 * `$tauri.start()` 的 `$patch(后端状态)` 发生在那之后，旧版本文件里的非法
 * 结构会被重新灌进来。回调必须幂等。
 */
export interface ManagedStoreEntry {
  readonly store: TauriPersistedStore;
  readonly onHydrated?: () => void;
}

const toEntry = (item: TauriPersistedStore | ManagedStoreEntry): ManagedStoreEntry =>
  "store" in item ? item : { store: item };

/**
 * 单个 store 的启动超时。这是挂载路径上的 await，后端异常（缺权限、插件没
 * 注册）不能把启动卡死——超时就放弃 Tauri 层，localStorage 仍然有效。
 */
const START_TIMEOUT_MS = 3000;

/** 「这条 store 已经从 localStorage 迁移过了」的标记前缀，见 seedBackend。 */
const SEEDED_FLAG_PREFIX = "tauriPiniaSeeded:";

let installed = false;
let plugin: typeof import("@tauri-store/pinia") | null = null;

const logError = (error: unknown): void => {
  console.error("[tauri-pinia]", error);
};

/**
 * 在 Tauri 中注册 `@tauri-store/pinia`。必须在任何 store 被实例化之前调用。
 *
 * Web 环境直接返回 false，且不会把该包（以及它依赖的 `@tauri-apps/api`）
 * 打进首屏——所以这里是动态 import，vite.config 里也给了独立 chunk。
 */
export async function installTauriPinia(pinia: Pinia): Promise<boolean> {
  if (!isTauri() || installed) return installed;
  try {
    plugin = await import("@tauri-store/pinia");
    const extend = plugin.TauriPluginPinia({
      // 由 startTauriPiniaStores() 显式驱动，保证在挂载前完成 hydrate。
      autoStart: false,
      // 改动即落盘，但按 1s debounce 合并，避免连续调节设置时反复写文件。
      saveOnChange: true,
      saveStrategy: "debounce",
      saveInterval: 1000,
      // 往 Rust 推状态同样合并：EQ 增益、字号这类滑块是按 pointermove 改的。
      syncStrategy: "debounce",
      syncInterval: 300,
      hooks: { error: logError },
    });
    // 只给声明了 `tauri` 选项的 store 建插件实例。插件的 Store 构造函数是有
    // 代价的：两个 TimeStrategy、一份合并后的选项对象，外加**无条件**两次
    // IPC（updateDenylist 的 allow/deny save+sync）。本项目有 9 个 store，
    // 托管的只有 3 个——不设这道闸的话，每个窗口启动都要白跑 6 个 Store 实例
    // 和 12 次 IPC，还会把 6 个 id 塞进 Rust 侧的 denylist HashSet。
    //
    // 用「有没有 tauri 选项」当开关，而不是另外维护一份 id 名单：开关就写在
    // store 自己的定义里，不会漂移。
    pinia.use((ctx) => (ctx.options?.tauri ? extend(ctx) : undefined));
    installed = true;
  } catch (error) {
    logError(error);
  }
  return installed;
}

/** 插件是否已成功注册。未注册时 `$tauri` 不存在，调用方应跳过。 */
export function isTauriPiniaInstalled(): boolean {
  return installed;
}

const withTimeout = <T>(task: Promise<T>, fallback: T, label: string): Promise<T> =>
  new Promise<T>((resolve) => {
    const timer = setTimeout(() => {
      logError(new Error(`store "${label}" timed out after ${START_TIMEOUT_MS}ms`));
      resolve(fallback);
    }, START_TIMEOUT_MS);
    task.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (error) => {
        clearTimeout(timer);
        logError(error);
        resolve(fallback);
      },
    );
  });

/**
 * 首次迁移：后端还没有这条 store 时，把 localStorage 里已有的那份推上去。
 *
 * 不做这一步的话，插件的 watcher 只在**状态发生变化**时才推送，老用户的账号
 * 和设置会一直停留在 localStorage 里，直到他碰巧改了一个设置为止——文件是空的，
 * Rust 侧读不到，跨窗口同步也没有基准。
 *
 * 种子直接读 localStorage，而不是读 `store.$state`。这是刻意的：`$state` 是否
 * 已经 hydrate 取决于插件注册时机（pinia 的 `use()` 要等 `app.use(pinia)` 才
 * 生效），一旦顺序出错，快照就是一份默认值——把它推上去等于**清空用户数据**。
 * localStorage 那份则要么存在且正确，要么不存在（此时本就无可迁移）。
 *
 * 键名就是 store id：三个托管 store 都没自定义 persist key。
 *
 * 返回是否真的写了种子。`plugin:pinia|patch` 是插件内部命令，但它在
 * `pinia:default` 权限集里（allow-patch），失败也只是回落到「改一次才迁移」。
 */
async function seedBackend(store: TauriPersistedStore): Promise<boolean> {
  if (!plugin) return false;

  // 迁移只会发生一次。留个标记，之后每次启动就不用再为每个 store 发一趟
  // getStoreState 探测——那三次 IPC 是挂在 await 上、直接顶在挂载前面的。
  //
  // 标记丢了也不会坏：重新探测时后端已经有数据，照样跳过播种。
  const seededKey = `${SEEDED_FLAG_PREFIX}${store.$id}`;
  if (localStorage.getItem(seededKey)) return false;

  const raw = localStorage.getItem(store.$id);
  if (!raw) return false;
  let snapshot: Record<string, unknown>;
  try {
    snapshot = JSON.parse(raw);
  } catch {
    return false;
  }
  if (!snapshot || typeof snapshot !== "object" || Object.keys(snapshot).length === 0) return false;

  const backendState = await plugin
    .getStoreState<Record<string, unknown>>(store.$id)
    .catch(() => null);
  // 拿不到 / 空对象都当作「后端还没有这条 store」。
  if (backendState && Object.keys(backendState).length > 0) {
    localStorage.setItem(seededKey, "1");
    return false;
  }

  await invoke("plugin:pinia|patch", { id: store.$id, state: snapshot });
  localStorage.setItem(seededKey, "1");
  return true;
}

async function startOne(entry: ManagedStoreEntry): Promise<boolean> {
  const { store, onHydrated } = entry;
  // $tauri 缺失 = 插件没挂到这个 store 上，几乎只可能是 store 早于
  // `app.use(pinia)` 被实例化。这种情况下 persistedstate 多半也没跑，整个
  // store 都不可信——直接报错退出，不要碰后端。
  if (typeof store.$tauri?.start !== "function") {
    throw new Error(
      `store "${store.$id}" has no $tauri: pinia plugins were not applied before it was instantiated`,
    );
  }
  // 顺序很重要：先种子再 start()。start() 内部是 `$patch(后端状态)`——合并
  // 语义——所以它读回来的正是刚推上去的那份，不会把前端状态打回默认值。
  const seeded = await seedBackend(store);
  await store.$tauri.start();
  // 后端那份可能是旧版本写的，跑一次版本修复。修复若真的改了值，watcher 会
  // 顺带把修正后的完整状态推回后端，下次启动就是干净的。
  onHydrated?.();
  return seeded;
}

/**
 * 启动传入 store 的后端同步，并等待首次 hydrate（含首次迁移与版本修复）完成。
 *
 * 调用方负责实例化 store（`useXxxStore(pinia)`），这样本模块不用 import
 * 任何 store，主窗口与从窗口也可以各自决定托管哪几个。
 *
 * 永远 resolve：Tauri 层失败时降级为纯 localStorage，不阻塞挂载。
 */
export async function startTauriPiniaStores(
  items: (TauriPersistedStore | ManagedStoreEntry)[],
): Promise<void> {
  const entries = items.map(toEntry);
  if (!installed || !plugin) {
    // Web 分支：persistedstate 已经 hydrate 过了，版本修复照样要跑。
    entries.forEach((entry) => {
      try {
        entry.onHydrated?.();
      } catch (error) {
        logError(error);
      }
    });
    return;
  }
  const seeded = await Promise.all(
    entries.map((entry) => withTimeout(startOne(entry), false, entry.store.$id)),
  );
  // 种子是在 start() 之前推的，那时 saveOnChange 这些选项还没下发给 Rust
  // （setOptions 在 start() 里），所以显式强制落盘一次。
  if (seeded.some(Boolean)) {
    await withTimeout(plugin.saveAllNow(), undefined, "saveAllNow");
  }
}

/**
 * 销毁传入 store 的 Rust 状态并删除落盘文件。
 *
 * 「系统重置」必须调用它：只清 localStorage 的话，重载后 Tauri store 会把旧
 * 设置重新 hydrate 回来，重置等于没做。
 */
export async function destroyTauriPiniaStores(stores: TauriPersistedStore[]): Promise<void> {
  if (!installed) return;
  await Promise.all(
    stores.map((store) =>
      withTimeout(Promise.resolve(store.$tauri?.destroy()), undefined, `${store.$id} (destroy)`),
    ),
  );
}
