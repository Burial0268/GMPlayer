import { defineStore, acceptHMRUpdate } from "pinia";
import { NIcon } from "naive-ui";
import { WbSunnyFilled, DarkModeFilled } from "@vicons/material";
import { h } from "vue";
import getLanguageData from "@/utils/getLanguageData";
import { createThrottledStorage } from "./throttledStorage";

declare const $message: any;

interface SpringParams {
  mass: number;
  damping: number;
  stiffness: number;
}

interface DspEqBandSetting {
  enabled: boolean;
  filterType: "peaking" | "lowShelf" | "highShelf";
  frequency: number;
  gainDb: number;
  q: number;
}

interface SettingDataState {
  theme: "light" | "dark";
  themeMode: "light" | "dark" | "system";
  themeAuto: boolean;
  themeType: string;
  themeData: Record<string, any>;
  searchHistory: boolean;
  bannerShow: boolean;
  autoSignIn: boolean;
  listClickMode: "click" | "dblclick";
  useTTMLRepo: boolean;
  playerStyle: string;
  bottomLyricShow: boolean;
  showYrc: boolean;
  showYrcAnimation: boolean;
  showYrcTransform: boolean;
  showTransl: boolean;
  showRoma: boolean;
  taskbarLyrics: boolean;
  songLevel: string;
  lyricsPosition: string;
  lyricsBlock: string;
  lyricsFontSize: number;
  desktopLyricsFontSizeOffset: number;
  lyricFont: string;
  lyricFontWeight: string;
  lyricLetterSpacing: string;
  hidePassedLines: boolean;
  lyricLineHeight: number;
  lyricsBlur: boolean;
  musicFrequency: boolean;
  lrcMousePause: boolean;
  useUnmServer: boolean;
  /**
   * NCM 接口走哪条链路。`remote` = 已部署的 NeteaseCloudMusicApi；
   * `local` = Tauri 内嵌协议层，少一跳网络（实测约 38~48 ms/请求）。
   *
   * Tauri 上默认 local。Web 上没有内嵌协议层，`setNcmTransport` 会强制回落到
   * remote，设置项也不显示——那里没有可切换的对象。
   *
   * remote 后端永不移除：收益与地理位置相关（离网易云远的用户，中转反而可能
   * 更快），且本地直连会把请求出口 IP 从服务器换成用户真机，风控画像随之改变。
   * 本地链路连续失败时会自动降级回 remote。
   */
  ncmTransport: "remote" | "local";
  backgroundImageShow: string;
  blurAmount: number;
  contrastAmount: number;
  fps: number;
  flowSpeed: number;
  renderScale: number;
  albumImageUrl: string;
  dynamicFlowSpeed: boolean;
  dynamicFlowSpeedScale: number;
  countDownShow: boolean;
  showLyricSetting: boolean;
  songVolumeFade: boolean;
  listNumber: number;
  memoryLastPlaybackPosition: boolean;
  language: string;
  bottomClick: boolean;
  immersivePlayer: boolean;
  colorType: string;
  springParams: {
    posX: SpringParams;
    posY: SpringParams;
    scale: SpringParams;
  };
  // AutoMix settings
  autoMixEnabled: boolean;
  autoMixCrossfadeDuration: number;
  autoMixBpmMatch: boolean;
  autoMixBeatAlign: boolean;
  autoMixVolumeNorm: boolean;
  autoMixTransitionStyle: "linear" | "equalPower" | "sCurve";
  autoMixSmartCurve: boolean;
  autoMixTransitionEffects: boolean;
  autoMixVocalGuard: boolean;
  // DSP settings. Defaults must resolve to a native bypass path.
  dspEnabled: boolean;
  dspEqEnabled: boolean;
  dspEqPreampDb: number;
  dspEqPreset: string;
  dspEqBandCount: number;
  dspEqBands: DspEqBandSetting[];
  dspLimiterEnabled: boolean;
  dspLimiterThresholdDb: number;
  dspLimiterCeilingDb: number;
  dspLimiterReleaseMs: number;
  // Lyric time offset (ms). Positive = lyrics advance, Negative = lyrics delay
  lyricTimeOffset: number;
  // Close behavior for Tauri desktop app
  closeBehavior: "ask" | "tray" | "exit";
  // Sidebar collapsed state
  sidebarCollapsed: boolean;
}

const DSP_EQ_FREQUENCY_SETS: Record<number, number[]> = {
  10: [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000],
  15: [25, 40, 63, 100, 160, 250, 400, 630, 1000, 1600, 2500, 4000, 6300, 10000, 16000],
  31: [
    20, 25, 31.5, 40, 50, 63, 80, 100, 125, 160, 200, 250, 315, 400, 500, 630, 800, 1000, 1250,
    1600, 2000, 2500, 3150, 4000, 5000, 6300, 8000, 10000, 12500, 16000, 20000,
  ],
};

const defaultDspEqBands = (count = 10): DspEqBandSetting[] => {
  const frequencies = DSP_EQ_FREQUENCY_SETS[count] ?? DSP_EQ_FREQUENCY_SETS[10];
  return frequencies.map((frequency, index) => ({
    enabled: true,
    filterType:
      index === 0 ? "lowShelf" : index === frequencies.length - 1 ? "highShelf" : "peaking",
    frequency,
    gainDb: 0,
    q: frequencies.length >= 31 ? 4.318 : 1.414,
  }));
};

const DSP_EQ_BAND_COUNTS = [10, 15, 31];

/**
 * settingData 的写入门面。导出是给「系统重置」用的：清 localStorage 之前
 * 必须先 suspend，否则节流窗口里的尾调用会在 clear 之后把旧值写回去。
 */
export const settingStorage = createThrottledStorage(400);

/**
 * 载荷级迁移：把任意来源（localStorage / Tauri store 文件）读出来的**原始载荷**
 * 修正成当前版本的字段结构。
 *
 * 之所以放在载荷级而不是 store 级：这些迁移要判断「键在不在」，而一旦并进
 * store，缺失的键就被默认值填上了，再也分不清「用户没设过」和「用户把它设成了
 * 默认值」。两条 hydrate 路径共用这一个函数——localStorage 走
 * `serializer.deserialize`，Tauri store 走 `tauri.hooks.beforeFrontendSync`。
 *
 * 必须幂等：跨窗口同步会对同一份数据反复调用。旧键在迁移后直接删掉，
 * 既保证幂等，也让它不会一直躺在落盘文件里。
 */
export function migrateLegacySettingPayload(payload: any): any {
  if (!payload || typeof payload !== "object") return payload;
  // useLyricAtlasAPI → useTTMLRepo
  if ("useLyricAtlasAPI" in payload) {
    if (!("useTTMLRepo" in payload)) payload.useTTMLRepo = payload.useLyricAtlasAPI;
    delete payload.useLyricAtlasAPI;
  }
  // themeAuto + theme → themeMode
  if (!("themeMode" in payload) && ("themeAuto" in payload || "theme" in payload)) {
    payload.themeMode = payload.themeAuto ? "system" : payload.theme;
  }
  return payload;
}

/**
 * store 级修复：把已经并进 store 的值夹回合法范围、补齐结构。与来源无关。
 *
 * 不能只挂在 persistedstate 的 `afterHydrate` 上——Tauri store 的
 * `$patch(后端状态)` 发生在那之后，旧版本文件里的 dspEqBands 长度、越界的
 * 字号偏移会被重新灌进来。所以 `$tauri.start()` 之后必须再跑一次。
 *
 * 全程原地改且只在值非法时才写：幂等，且不会凭空触发一次同步推送。
 */
export function repairSettingData(store: any): void {
  if (
    typeof store.desktopLyricsFontSizeOffset !== "number" ||
    !Number.isFinite(store.desktopLyricsFontSizeOffset)
  ) {
    store.desktopLyricsFontSizeOffset = 0;
  } else {
    store.desktopLyricsFontSizeOffset = Math.max(
      -20,
      Math.min(40, store.desktopLyricsFontSizeOffset),
    );
  }

  if (!DSP_EQ_BAND_COUNTS.includes(store.dspEqBandCount)) {
    store.dspEqBandCount = Array.isArray(store.dspEqBands)
      ? (DSP_EQ_BAND_COUNTS.find((count) => count === store.dspEqBands.length) ?? 10)
      : 10;
  }
  if (!Array.isArray(store.dspEqBands) || store.dspEqBands.length !== store.dspEqBandCount) {
    store.dspEqBands = defaultDspEqBands(store.dspEqBandCount);
  } else {
    const defaults = defaultDspEqBands(store.dspEqBandCount);
    store.dspEqBands.forEach((band: Partial<DspEqBandSetting>, index: number) => {
      if (typeof band.enabled !== "boolean") band.enabled = true;
      if (!band.filterType) band.filterType = defaults[index]?.filterType ?? "peaking";
      if (!Number.isFinite(band.frequency)) band.frequency = defaults[index]?.frequency ?? 1000;
      if (!Number.isFinite(band.gainDb)) band.gainDb = 0;
      if (!Number.isFinite(band.q)) band.q = defaults[index]?.q ?? 1.414;
    });
  }

  if (typeof store.dspEnabled !== "boolean") store.dspEnabled = false;
  if (typeof store.dspEqEnabled !== "boolean") store.dspEqEnabled = true;
  if (typeof store.dspLimiterEnabled !== "boolean") store.dspLimiterEnabled = false;
}

const useSettingDataStore = defineStore("settingData", {
  state: (): SettingDataState => {
    return {
      theme: "light",
      themeMode: "system",
      themeAuto: true,
      themeType: "red",
      themeData: {},
      searchHistory: true,
      bannerShow: true,
      autoSignIn: true,
      listClickMode: "dblclick",
      useTTMLRepo: false,
      playerStyle: "cover",
      bottomLyricShow: true,
      showYrc: true,
      showYrcAnimation: true,
      showYrcTransform: false,
      showTransl: false,
      showRoma: false,
      taskbarLyrics: true,
      songLevel: "exhigh",
      lyricsPosition: "left",
      lyricsBlock: "top",
      lyricsFontSize: 3.6,
      desktopLyricsFontSizeOffset: 0,
      lyricFont: "HarmonyOS Sans SC",
      lyricFontWeight: "normal",
      lyricLetterSpacing: "normal",
      hidePassedLines: false,
      lyricLineHeight: 1.8,
      lyricsBlur: true,
      musicFrequency: false,
      lrcMousePause: false,
      useUnmServer: true,
      ncmTransport: "local",
      backgroundImageShow: "eplor",
      blurAmount: 10,
      contrastAmount: 1.2,
      fps: 60,
      flowSpeed: 2,
      renderScale: 0.5,
      albumImageUrl: "none",
      dynamicFlowSpeed: true,
      dynamicFlowSpeedScale: 1,
      countDownShow: true,
      showLyricSetting: false,
      songVolumeFade: true,
      listNumber: 30,
      memoryLastPlaybackPosition: true,
      language: "zh-CN",
      bottomClick: false,
      immersivePlayer: false,
      colorType: "secondary",
      springParams: {
        posX: { mass: 1, damping: 10, stiffness: 100 },
        posY: { mass: 1, damping: 15, stiffness: 100 },
        scale: { mass: 1, damping: 20, stiffness: 100 },
      },
      // AutoMix defaults
      autoMixEnabled: false,
      autoMixCrossfadeDuration: 8,
      autoMixBpmMatch: true,
      autoMixBeatAlign: true,
      autoMixVolumeNorm: true,
      autoMixTransitionStyle: "equalPower",
      autoMixSmartCurve: true,
      autoMixTransitionEffects: true,
      autoMixVocalGuard: true,
      // DSP defaults: native mixer stays bypassed until explicitly enabled.
      dspEnabled: false,
      dspEqEnabled: true,
      dspEqPreampDb: 0,
      dspEqPreset: "flat",
      dspEqBandCount: 10,
      dspEqBands: defaultDspEqBands(),
      dspLimiterEnabled: false,
      dspLimiterThresholdDb: -1,
      dspLimiterCeilingDb: -1,
      dspLimiterReleaseMs: 80,
      // Lyric time offset (ms)
      lyricTimeOffset: 0,
      // Close behavior (Tauri): 'ask' | 'tray' | 'exit'
      closeBehavior: "ask",
      // Sidebar collapsed state
      sidebarCollapsed: false,
    };
  },
  getters: {
    getSiteTheme(state): "light" | "dark" {
      return state.theme;
    },
  },
  actions: {
    setSiteTheme(value: "light" | "dark") {
      const isLightMode = value === "light";
      const message = isLightMode ? getLanguageData("lightMode") : getLanguageData("darkMode");
      const icon = isLightMode ? WbSunnyFilled : DarkModeFilled;
      this.theme = value;
      this.themeMode = value;
      this.themeAuto = false;
      $message.info(message, {
        icon: () => h(NIcon, null, { default: () => h(icon) }),
      });
    },
    setShowTransl(value: boolean) {
      this.showTransl = value;
    },
  },
  persist: [
    {
      // 合并写入：EQ 增益 / 字号偏移是按 pointermove 频率改的，而 persistedstate
      // 每次 mutation 都会同步写一遍完整 settingData。见 throttledStorage。
      storage: settingStorage,
      // 载荷级迁移挂在 deserialize 上，和 Tauri 分支的 beforeFrontendSync
      // 走同一个函数，保证两条 hydrate 路径的迁移结果一致。
      serializer: {
        serialize: (data: any) => JSON.stringify(data),
        deserialize: (data: string) => migrateLegacySettingPayload(JSON.parse(data)),
      },
      afterHydrate(ctx: { store: any }) {
        repairSettingData(ctx.store);
      },
    },
  ],
  // Tauri 层：设置落到真实文件，并在主窗口 / 设置窗 / 迷你播放器之间实时同步
  // ——在此之前，从窗口改的设置只有 16 个歌词相关字段能经 playerBridge 回到
  // 主窗口，主题、DSP 等改动要重启才生效。
  // 这里不过滤字段：settingData 的每一项本来就是要持久化的。
  tauri: {
    save: true,
    sync: true,
    hooks: {
      // Tauri store 文件可能是旧版本写的，并进 store 之前先做同样的载荷级迁移。
      beforeFrontendSync: (state: any) => migrateLegacySettingPayload(state),
    },
  },
});

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useSettingDataStore, import.meta.hot));
}

export default useSettingDataStore;
