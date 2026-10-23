/**
 * 「系统重置」的持久化清理入口。
 *
 * localStorage 只是其中一层：Tauri 环境下 settingData / siteData / userData
 * 还落在 Tauri store 文件里。只 `localStorage.clear()` 的话，重载后这几个
 * store 会从文件里重新 hydrate 回来，重置等于没做。
 *
 * 本地音乐库**不在**这几层里，因此默认不受影响；要清它必须显式传
 * `clearLocalLibrary: true`（见 [`ResetOptions`]）。
 *
 * 主窗口（App.vue 的 $cleanAll）和从设置窗口（SettingsWorkspace 的兜底
 * 实现）都走这里，避免两处逻辑漂移。
 */
import {
  destroyTauriPiniaStores,
  isTauriPiniaInstalled,
} from "@/utils/tauri/store/piniaPersistence";
import { localLibraryReset } from "@/utils/localLibrary";
import { suspendMusicPersist } from "./musicPersistedData";
import useSettingDataStore, { settingStorage } from "./settingData";
import useSiteDataStore from "./siteData";
import useUserDataStore from "./userData";

export interface ResetOptions {
  /**
   * 同时清除本地音乐库（来源、索引、本地歌单、本地收藏）。
   *
   * **默认 false，且必须由调用方显式传 true。** 本地库不在 localStorage、也不在
   * Tauri store 里，它落在 `$APPDATA/local-library.bin` 与 `local-user-data.json`
   * ——所以下面那些清理天然碰不到它。但「碰巧幸存」不是一种语义：用户按下重置时
   * 应该明确知道本地音乐是留还是清，因此重置对话框给出一个默认不勾的选项，而不是
   * 由代码替他决定。
   */
  clearLocalLibrary?: boolean;
}

/**
 * 清空所有持久化层。调用方负责随后重载页面。
 *
 * 先销毁 Tauri store 再清 localStorage：destroy 内部有超时兜底，即使 IPC
 * 卡住，localStorage 也一定会被清掉。
 */
export async function resetPersistedStorage(options: ResetOptions = {}): Promise<void> {
  suspendMusicPersist();
  // settingData 的写入是节流合并的，不挂起的话 clear 之后还会有尾调用落地，
  // 把刚清掉的设置原样写回去。
  settingStorage.suspend();
  if (isTauriPiniaInstalled()) {
    await destroyTauriPiniaStores([useSettingDataStore(), useSiteDataStore(), useUserDataStore()]);
  }
  if (options.clearLocalLibrary) {
    // 放在 localStorage.clear() 之前：这一步要走 IPC，而清完 localStorage 之后
    // 页面随即重载，命令可能来不及发出去。
    try {
      await localLibraryReset();
    } catch (err) {
      console.error("清除本地音乐库失败", err);
    }
  }
  localStorage.clear();
}
