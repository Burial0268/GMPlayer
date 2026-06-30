import { computed } from "vue";
import { useI18n } from "vue-i18n";
import {
  ColorLensRound,
  TuneRound,
  MusicNoteRound,
  GraphicEqRound,
  SlideshowRound,
  SubtitlesRound,
  AutoAwesomeRound,
  BuildRound,
} from "@vicons/material";
import { settingStore, userStore } from "@/store";
import { isTauri } from "@/utils/tauri";
import { isWindowsTauri, windowManager } from "@/utils/tauri/windowManager";
import type { SettingsSection } from "./types";

declare const $message: any;

export const SETTINGS_SECTION_ALIASES: Record<string, string> = {
  main: "appearance",
  player: "player",
  other: "system",
};

export function useSettingsSections() {
  const setting = settingStore();
  const user = userStore();
  const { locale, t } = useI18n();
  const hasUnmServer = !!import.meta.env.VITE_UNM_API;

  const changeLanguage = (value: unknown) => {
    if (typeof value !== "string") return;
    locale.value = value;
    document.documentElement.setAttribute("lang", value);
    $message?.success(t("setting.changeLanguage", { name: value }));
  };

  const closeTaskbarLyricsWhenDisabled = (enabled: unknown) => {
    if (enabled) return;
    windowManager.closeTaskbarLyrics();
  };

  const sections = computed<SettingsSection[]>(() => [
    {
      key: "appearance",
      label: "setting.sectionAppearance",
      icon: ColorLensRound,
      searchText: "setting.themeType setting.language setting.theme",
      items: [
        {
          key: "themeColors",
          label: "setting.themeType",
          tip: "setting.themeTypeTip",
          control: "custom",
          slot: "themeColors",
        },
        {
          key: "language",
          label: "setting.language",
          control: "select",
          options: [
            { label: "🇨🇳 简体中文", value: "zh-CN" },
            { label: "🇬🇧 English", value: "en" },
          ],
          onUpdate: changeLanguage,
        },
        {
          key: "themeMode",
          label: "setting.theme",
          control: "select",
          options: [
            { label: "nav.avatar.light", value: "light" },
            { label: "nav.avatar.dark", value: "dark" },
            { label: "setting.themeModeSystem", value: "system" },
          ],
        },
      ],
    },
    {
      key: "general",
      label: "setting.sectionGeneral",
      icon: TuneRound,
      searchText: "setting.bannerShow setting.searchHistory setting.bottomLyricShow",
      items: [
        {
          key: "autoSignIn",
          label: "setting.autoSignIn",
          tip: "setting.autoSignInTip",
          control: "switch",
        },
        { key: "bannerShow", label: "setting.bannerShow", control: "switch" },
        {
          key: "listClickMode",
          label: "setting.listClickMode",
          tip: "setting.listClickModeTip",
          control: "select",
          options: [
            { label: "setting.dblclick", value: "dblclick" },
            { label: "setting.click", value: "click" },
          ],
        },
        { key: "searchHistory", label: "setting.searchHistory", control: "switch" },
        {
          key: "bottomLyricShow",
          label: "setting.bottomLyricShow",
          tip: "setting.bottomLyricShowTip",
          control: "switch",
        },
        {
          key: "bottomClick",
          label: "setting.bottomClick",
          tip: "setting.bottomClickTip",
          control: "switch",
        },
        {
          key: "showLyricSetting",
          label: "setting.showLyricSetting",
          tip: "setting.showLyricSettingTip",
          control: "switch",
        },
        {
          key: "sidebarCollapsed",
          label: "setting.sidebarCollapsed",
          tip: "setting.sidebarCollapsedTip",
          control: "switch",
        },
      ],
    },
    {
      key: "playback",
      label: "setting.sectionPlayback",
      icon: MusicNoteRound,
      searchText: "setting.songLevel AutoMix UNM",
      items: [
        {
          key: "songLevel",
          label: "setting.songLevel",
          tip: "setting.songLevelTip",
          control: "select",
          options: [
            { label: "setting.standard", value: "standard" },
            { label: "setting.higher", value: "higher" },
            { label: "setting.exhigh", value: "exhigh" },
            { label: "setting.lossless", value: "lossless", disabled: !user.userData?.vipType },
            { label: "setting.hires", value: "hires", disabled: !user.userData?.vipType },
            { label: "setting.jyeffect", value: "jyeffect", disabled: !user.userData?.vipType },
            { label: "setting.jymaster", value: "jymaster", disabled: !user.userData?.vipType },
          ],
        },
        {
          key: "useUnmServer",
          label: "setting.useUnmServerShow",
          tip: hasUnmServer ? "setting.useUnmServerShowTip1" : "setting.useUnmServerShowTip2",
          control: "switch",
          disabled: () => !hasUnmServer,
        },
        {
          key: "songVolumeFade",
          label: "setting.songVolumeFade",
          tip: "setting.songVolumeFadeTip",
          control: "switch",
        },
        {
          key: "memoryLastPlaybackPosition",
          label: "setting.memoryLastPlaybackPosition",
          tip: "setting.memoryLastPlaybackPositionTip",
          control: "switch",
        },
      ],
    },
    {
      key: "dsp",
      label: "setting.sectionDsp",
      icon: GraphicEqRound,
      searchText: "DSP EQ Equalizer limiter",
      items: [
        {
          key: "dspSettings",
          label: "setting.dspTitle",
          tip: "setting.dspTip",
          control: "custom",
          slot: "dspSettings",
        },
      ],
    },
    {
      key: "player",
      label: "setting.sectionPlayerVisual",
      icon: SlideshowRound,
      searchText: "setting.playerStyle setting.backgroundImageShow setting.musicFrequency",
      items: [
        {
          key: "playerStyle",
          label: "setting.playerStyle",
          tip: "setting.playerStyleTip",
          control: "select",
          options: [
            { label: "setting.cover", value: "cover" },
            { label: "setting.record", value: "record" },
          ],
        },
        {
          key: "backgroundImageShow",
          label: "setting.backgroundImageShow",
          tip: `setting.backgroundImageShowTip_${setting.backgroundImageShow}`,
          control: "select",
          options: [
            { label: "setting.solid", value: "solid" },
            { label: "setting.blur", value: "blur" },
            { label: "setting.eplor", value: "eplor" },
          ],
        },
        {
          key: "openEploryConfig",
          label: "setting.eploryBackgroundConfig",
          tip: "setting.eploryBackgroundConfigTip",
          control: "button",
          buttonText: "setting.configure",
          show: () => setting.backgroundImageShow === "eplor",
        },
        {
          key: "openBlurConfig",
          label: "setting.blurBackgroundConfig",
          tip: "setting.blurBackgroundConfigTip",
          control: "button",
          buttonText: "setting.configure",
          show: () => setting.backgroundImageShow === "blur",
        },
        {
          key: "immersivePlayer",
          label: "setting.immersivePlayer",
          tip: "setting.immersivePlayerTip",
          control: "switch",
          dev: true,
        },
        {
          key: "colorType",
          label: "setting.colorType",
          tip: "setting.colorTypeTip",
          control: "select",
          dev: true,
          show: () => setting.immersivePlayer,
          options: [
            { label: "setting.colorNeutral", value: "neutral" },
            { label: "setting.colorNeutralVariant", value: "neutralVariant" },
            { label: "setting.colorPrimary", value: "primary" },
            { label: "setting.colorSecondary", value: "secondary" },
            { label: "setting.colorTertiary", value: "tertiary" },
          ],
        },
        {
          key: "musicFrequency",
          label: "setting.musicFrequency",
          tip: "setting.musicFrequencyTip",
          control: "switch",
        },
        {
          key: "taskbarLyrics",
          label: "setting.taskbarLyrics",
          tip: "setting.taskbarLyricsTip",
          control: "switch",
          show: () => isWindowsTauri(),
          onUpdate: closeTaskbarLyricsWhenDisabled,
        },
      ],
    },
    {
      key: "lyrics",
      label: "setting.sectionLyrics",
      icon: SubtitlesRound,
      searchText: "YRC TTML AMLL lyric",
      items: [
        {
          key: "useTTMLRepo",
          label: "setting.useTTMLRepo",
          tip: "setting.useTTMLRepoTip",
          control: "switch",
          dev: true,
        },
        {
          key: "showYrc",
          label: "setting.showYrc",
          tip: "setting.showYrcTip",
          control: "switch",
          dev: true,
        },
        {
          key: "showYrcAnimation",
          label: "setting.showYrcAnimation",
          tip: "setting.showYrcAnimationTip",
          control: "switch",
          show: () => setting.showYrc,
        },
        {
          key: "springParams",
          label: "setting.springParams",
          tip: "setting.springParamsTip",
          control: "custom",
          slot: "springParams",
          show: () => setting.showYrc,
        },
        {
          key: "showTransl",
          label: "setting.showTransl",
          tip: "setting.showTranslTip",
          control: "switch",
        },
        {
          key: "showRoma",
          label: "setting.showRoma",
          tip: "setting.showRomaTip",
          control: "switch",
        },
        {
          key: "countDownShow",
          label: "setting.countDownShow",
          tip: "setting.countDownShowTip",
          control: "switch",
        },
        {
          key: "lrcMousePause",
          label: "setting.lrcMousePause",
          tip: "setting.lrcMousePauseTip",
          control: "switch",
        },
        {
          key: "lyricsBlock",
          label: "setting.lyricsBlock",
          tip: "setting.lyricsBlockTip",
          control: "select",
          options: [
            { label: "setting.blockStart", value: "start" },
            { label: "setting.blockCenter", value: "center" },
          ],
        },
        {
          key: "lyricsFontSize",
          label: "setting.lyricsFontSize",
          control: "slider",
          min: 3,
          max: 4,
          step: 0.01,
          marks: {
            3: "setting.lyrics1",
            3.6: "setting.lyrics2",
            4: "setting.lyrics3",
          },
        },
        {
          key: "lyricFont",
          label: "setting.lyricFont",
          control: "select",
          options: [
            { label: "HarmonyOS Sans SC", value: "HarmonyOS Sans SC" },
            { label: "PingFang SC", value: "PingFang SC" },
            { label: "Microsoft YaHei", value: "Microsoft YaHei" },
            { label: "Noto Sans SC", value: "Noto Sans SC" },
            { label: "SF Pro Display", value: "SF Pro Display" },
          ],
        },
        {
          key: "lyricFontWeight",
          label: "setting.lyricFontWeight",
          control: "select",
          options: [
            { label: "setting.fontWeightNormal", value: "normal" },
            { label: "setting.fontWeightMedium", value: "500" },
            { label: "setting.fontWeightBold", value: "bold" },
          ],
        },
        {
          key: "lyricLetterSpacing",
          label: "setting.lyricLetterSpacing",
          control: "select",
          options: [
            { label: "setting.letterSpacingNormal", value: "normal" },
            { label: "setting.letterSpacingTight", value: "-0.05em" },
            { label: "setting.letterSpacingLoose", value: "0.05em" },
          ],
        },
        {
          key: "lyricLineHeight",
          label: "setting.lyricLineHeight",
          control: "number",
          min: 1,
          max: 3,
          step: 0.1,
        },
        {
          key: "lyricsPosition",
          label: "setting.lyricsPosition",
          control: "select",
          options: [
            { label: "setting.positionLeft", value: "left" },
            { label: "setting.positionCenter", value: "center" },
          ],
        },
        {
          key: "lyricsBlur",
          label: "setting.lyricsBlur",
          tip: "setting.lyricsBlurTip",
          control: "switch",
        },
        {
          key: "lyricTimeOffset",
          label: "setting.lyricOffset",
          tip: "setting.lyricOffsetTip",
          control: "number",
          min: -5000,
          max: 5000,
          step: 100,
        },
      ],
    },
    {
      key: "automix",
      label: "setting.sectionAutoMix",
      icon: AutoAwesomeRound,
      searchText: "AutoMix crossfade bpm",
      items: [
        {
          key: "autoMixEnabled",
          label: "setting.autoMixEnabled",
          tip: "setting.autoMixEnabledTip",
          control: "switch",
          dev: true,
        },
        {
          key: "autoMixCrossfadeDuration",
          label: "setting.autoMixCrossfadeDuration",
          tip: "setting.autoMixCrossfadeDurationTip",
          control: "number",
          min: 3,
          max: 12,
          step: 1,
          show: () => setting.autoMixEnabled,
        },
        {
          key: "autoMixTransitionStyle",
          label: "setting.autoMixTransitionStyle",
          tip: "setting.autoMixTransitionStyleTip",
          control: "select",
          show: () => setting.autoMixEnabled,
          options: [
            { label: "setting.autoMixEqualPower", value: "equalPower" },
            { label: "setting.autoMixLinear", value: "linear" },
            { label: "setting.autoMixSCurve", value: "sCurve" },
          ],
        },
        {
          key: "autoMixSmartCurve",
          label: "setting.autoMixSmartCurve",
          tip: "setting.autoMixSmartCurveTip",
          control: "switch",
          show: () => setting.autoMixEnabled,
        },
        {
          key: "autoMixVolumeNorm",
          label: "setting.autoMixVolumeNorm",
          tip: "setting.autoMixVolumeNormTip",
          control: "switch",
          show: () => setting.autoMixEnabled,
        },
        {
          key: "autoMixBpmMatch",
          label: "setting.autoMixBpmMatch",
          tip: "setting.autoMixBpmMatchTip",
          control: "switch",
          show: () => setting.autoMixEnabled,
        },
        {
          key: "autoMixBeatAlign",
          label: "setting.autoMixBeatAlign",
          tip: "setting.autoMixBeatAlignTip",
          control: "switch",
          show: () => setting.autoMixEnabled,
          disabled: () => !setting.autoMixBpmMatch,
        },
        {
          key: "autoMixTransitionEffects",
          label: "setting.autoMixTransitionEffects",
          tip: "setting.autoMixTransitionEffectsTip",
          control: "switch",
          show: () => setting.autoMixEnabled,
        },
        {
          key: "autoMixVocalGuard",
          label: "setting.autoMixVocalGuard",
          tip: "setting.autoMixVocalGuardTip",
          control: "switch",
          show: () => setting.autoMixEnabled,
        },
      ],
    },
    {
      key: "system",
      label: "setting.sectionSystem",
      icon: BuildRound,
      searchText: "setting.appUpdate setting.resetApp",
      items: [
        {
          key: "appUpdate",
          label: "setting.appUpdate",
          tip: "setting.appUpdateTip",
          control: "custom",
          slot: "appUpdate",
          show: () => isTauri(),
        },
        {
          key: "resetApp",
          label: "setting.resetApp",
          tip: "setting.resetAppTip",
          control: "button",
          buttonText: "general.name.restore",
          buttonType: "error",
        },
      ],
    },
  ]);

  return {
    sections,
  };
}
