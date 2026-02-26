/**
 * Slave entry point for Mini Player & Desktop Lyrics windows.
 *
 * This is a completely separate Vue app — NO Pinia, NO persistence,
 * NO store hydration, NO autoplay. Only loads the bridge composable
 * and i18n with language read directly from localStorage.
 */
import { createApp } from "vue";
import { createRouter, createWebHashHistory } from "vue-router";
import { createI18n } from "vue-i18n";

import SlaveApp from "@/SlaveApp.vue";

// i18n messages (same source files as main app)
import en from "@/locale/lang/en.js";
import zhCN from "@/locale/lang/zh-CN.js";

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
      path: "/:pathMatch(.*)",
      redirect: "/mini-player",
    },
  ],
});

// ── Mount ─────────────────────────────────────────────────────────────

const app = createApp(SlaveApp);
app.use(i18n);
app.use(router);
app.mount("#app");
