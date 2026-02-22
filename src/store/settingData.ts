import { defineStore, acceptHMRUpdate } from "pinia";
import { NIcon } from "naive-ui";
import { WbSunnyFilled, DarkModeFilled } from "@vicons/material";
import { h } from "vue";
import getLanguageData from "@/utils/getLanguageData";

declare const $message: any;

interface SpringParams {
  mass: number;
  damping: number;
  stiffness: number;
}

interface SettingDataState {
  theme: "light" | "dark";
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
  songLevel: string;
  lyricsPosition: string;
  lyricsBlock: string;
  lyricsFontSize: number;
  lyricFont: string;
  lyricFontWeight: string;
  lyricLetterSpacing: string;
  lyricLineHeight: number;
  lyricsBlur: boolean;
  musicFrequency: boolean;
  lrcMousePause: boolean;
  useUnmServer: boolean;
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
}

const useSettingDataStore = defineStore("settingData", {
  state: (): SettingDataState => {
    return {
      theme: "light",
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
      songLevel: "exhigh",
      lyricsPosition: "left",
      lyricsBlock: "top",
      lyricsFontSize: 3.6,
      lyricFont: "HarmonyOS Sans SC",
      lyricFontWeight: "normal",
      lyricLetterSpacing: "normal",
      lyricLineHeight: 1.8,
      lyricsBlur: true,
      musicFrequency: false,
      lrcMousePause: false,
      useUnmServer: true,
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
      const message = isLightMode
        ? getLanguageData("lightMode")
        : getLanguageData("darkMode");
      const icon = isLightMode ? WbSunnyFilled : DarkModeFilled;
      this.theme = value;
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
      storage: localStorage,
      afterHydrate(ctx: { store: any; }) {
        // Migrate old useLyricAtlasAPI → useTTMLRepo
        const store = ctx.store;
        const raw = localStorage.getItem('settingData');
        if (raw) {
          try {
            const parsed = JSON.parse(raw);
            if ('useLyricAtlasAPI' in parsed && !('useTTMLRepo' in parsed)) {
              store.useTTMLRepo = parsed.useLyricAtlasAPI;
            }
          } catch (_) { /* ignore */ }
        }
      },
    },
  ],
});

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useSettingDataStore, import.meta.hot))
}

export default useSettingDataStore;
