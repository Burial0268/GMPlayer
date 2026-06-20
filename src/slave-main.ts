/**
 * Slave entry point for Mini Player & Desktop Lyrics windows.
 *
 * This is a separate Vue app for auxiliary windows. It avoids the main
 * playback bootstrap, but mounts Pinia so the settings window can edit
 * persisted settings without loading the full app shell.
 */
import { createApp } from "vue";
import { createPinia } from "pinia";
import { createRouter, createWebHashHistory } from "vue-router";
import { createI18n } from "vue-i18n";
import piniaPluginPersistedstate from "pinia-plugin-persistedstate";

import SlaveApp from "@/SlaveApp.vue";
import "@/style/global.scss";
import "@/style/animate.scss";

// i18n messages (same source files as main app)
import en from "@/locale/lang/en";
import zhCN from "@/locale/lang/zh-CN";

// ── Standalone i18n (no Pinia dependency) ──────────────────────────────

function getLanguageFromStorage(): string {
  try {
    const raw = localStorage.getItem("settingData");
    if (raw) {
      const parsed = JSON.parse(raw);
      if (parsed.language) return parsed.language;
    }
  } catch {
    // ignore
  }
  return "zh-CN";
}

const i18n = createI18n({
  legacy: false,
  globalInjection: true,
  locale: getLanguageFromStorage(),
  fallbackLocale: "zh-CN",
  messages: {
    en,
    "zh-CN": zhCN,
  },
});

// ── Minimal router (hash history, slave routes only) ──────────────────

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: "/mini-player",
      name: "mini-player",
      component: () => import("@/views/MiniPlayer/index.vue"),
    },
    {
      path: "/desktop-lyrics",
      name: "desktop-lyrics",
      component: () => import("@/views/DesktopLyrics/index.vue"),
    },
    {
      path: "/taskbar-lyric",
      name: "taskbar-lyric",
      component: () => import("@/views/TaskbarLyrics/index.vue"),
    },
    {
      path: "/tray-popup",
      name: "tray-popup",
      component: () => import("@/views/TrayPopup/index.vue"),
    },
    {
      path: "/settings/:section?",
      name: "slave-settings",
      component: () => import("@/views/Setting/SlaveSettings.vue"),
    },
    {
      path: "/:pathMatch(.*)",
      redirect: "/mini-player",
    },
  ],
});

// ── Mount ─────────────────────────────────────────────────────────────

const pinia = createPinia();
pinia.use(piniaPluginPersistedstate);

const app = createApp(SlaveApp);
app.use(pinia);
app.use(i18n);
app.use(router);
app.mount("#app");
