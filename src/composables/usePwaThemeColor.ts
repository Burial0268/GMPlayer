import { watch } from "vue";
import { musicStore, settingStore, siteStore } from "@/store";
import { storeToRefs } from "pinia";

export function usePwaThemeColor() {
  const music = musicStore();
  const setting = settingStore();
  const site = siteStore();
  const { songPicColor } = storeToRefs(site);

  const changePwaColor = () => {
    const themeColorMeta = document.querySelector('meta[name="theme-color"]');
    if (!themeColorMeta) return;

    if (music.showBigPlayer) {
      themeColorMeta.setAttribute("content", songPicColor.value);
    } else {
      if (setting.getSiteTheme === "light") {
        themeColorMeta.setAttribute("content", "#ffffff");
      } else if (setting.getSiteTheme === "dark") {
        themeColorMeta.setAttribute("content", "#18181c");
      }
    }
  };

  watch(() => music.showBigPlayer, () => changePwaColor());
  watch(() => songPicColor.value, () => changePwaColor());

  return { changePwaColor };
}
