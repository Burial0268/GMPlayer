import { getCoverPalette, type CoverPalette } from "@/utils/color/coverPalette";
import { onActivated, onBeforeUnmount, onDeactivated, ref } from "vue";

const PANEL_ACCENT_VARS = [
  "--content-panel-accent-rgb",
  "--content-panel-secondary-rgb",
  "--content-panel-tertiary-rgb",
  "--content-panel-surface-rgb",
  "--content-panel-button-rgb",
  "--content-panel-on-button-rgb",
  "--content-panel-on-accent-rgb",
  "--content-panel-stage-gradient",
  "--content-panel-border-accent-strength",
];

export const useContentPanelAccent = () => {
  let requestId = 0;
  let lastCoverUrl: string | undefined;
  const currentPalette = ref<CoverPalette | null>(null);

  const setPanelAccent = (palette: CoverPalette) => {
    if (typeof document === "undefined") return;
    const root = document.documentElement;
    root.style.setProperty("--content-panel-accent-rgb", palette.panelAccentColor);
    root.style.setProperty("--content-panel-secondary-rgb", palette.secondaryColor);
    root.style.setProperty("--content-panel-tertiary-rgb", palette.tertiaryColor);
    root.style.setProperty("--content-panel-surface-rgb", palette.surfaceColor);
    root.style.setProperty("--content-panel-button-rgb", palette.buttonColor);
    root.style.setProperty("--content-panel-on-button-rgb", palette.onButtonColor);
    root.style.setProperty("--content-panel-on-accent-rgb", palette.onAccentColor);
    root.style.setProperty("--content-panel-stage-gradient", palette.panelGradient);
    root.style.setProperty("--content-panel-border-accent-strength", "12%");
  };

  const resetPanelAccent = () => {
    requestId += 1;
    if (typeof document === "undefined") return;
    PANEL_ACCENT_VARS.forEach((name) => {
      document.documentElement.style.removeProperty(name);
    });
  };

  const applyContentPanelAccent = async (coverUrl?: string) => {
    if (!coverUrl) {
      lastCoverUrl = undefined;
      currentPalette.value = null;
      resetPanelAccent();
      return;
    }

    lastCoverUrl = coverUrl;
    const id = ++requestId;
    try {
      const palette = await getCoverPalette(coverUrl);
      if (id !== requestId) return;
      currentPalette.value = palette;
      setPanelAccent(palette);
    } catch {
      if (id !== requestId) return;
      currentPalette.value = null;
      resetPanelAccent();
    }
  };

  onActivated(() => {
    if (currentPalette.value) {
      setPanelAccent(currentPalette.value);
    } else if (lastCoverUrl) {
      applyContentPanelAccent(lastCoverUrl);
    }
  });

  onDeactivated(resetPanelAccent);
  onBeforeUnmount(resetPanelAccent);

  return {
    applyContentPanelAccent,
    resetPanelAccent,
  };
};
