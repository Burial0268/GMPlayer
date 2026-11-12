import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { musicStore, settingStore } from "@/store";

const AUTO = "__auto";
export const LYRIC_OFFSET_STEP = 500;

/** Lyric offset and translation language, shared by the in-lyrics control and the player menu. */
export function useLyricControls() {
  const { t } = useI18n();
  const setting = settingStore();
  const music = musicStore();

  const languageName = (tag: string) => {
    try {
      return new Intl.DisplayNames([setting.language], { type: "language" }).of(tag) ?? tag;
    } catch {
      return tag;
    }
  };

  const languageOptions = computed(() => {
    const languages = music.songLyric.translationLanguages;
    if (languages.length < 2) return [];
    return [
      { label: t("setting.lyricTranslationLanguageAuto"), value: AUTO },
      ...languages.map((tag) => ({ label: languageName(tag), value: tag })),
    ];
  });

  // 明确选过且这首歌里有的才算选中，否则显示「跟随界面语言」
  const selectedLanguage = computed({
    get: () =>
      music.songLyric.translationLanguages.includes(setting.lyricTranslationLanguage)
        ? setting.lyricTranslationLanguage
        : AUTO,
    set: (value: string) => {
      setting.lyricTranslationLanguage = value === AUTO ? "" : value;
    },
  });

  const offset = computed(() => setting.lyricTimeOffset ?? 0);

  const offsetDisplay = computed(() => {
    const seconds = offset.value / 1000;
    const sign = seconds > 0 ? "+" : "";
    return `${sign}${seconds.toFixed(1)}s`;
  });

  const adjustOffset = (delta: number) => {
    setting.lyricTimeOffset += delta;
  };

  const resetOffset = () => {
    setting.lyricTimeOffset = 0;
  };

  return {
    languageOptions,
    selectedLanguage,
    offset,
    offsetDisplay,
    adjustOffset,
    resetOffset,
  };
}
