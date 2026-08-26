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
import { installTauriPinia, startTauriPiniaStores } from "@/utils/tauri/store/piniaPersistence";
import useSettingDataStore, { repairSettingData } from "@/store/settingData";
import useSiteDataStore, { repairSiteData } from "@/store/siteData";
import useUserDataStore from "@/store/userData";
import { setNcmCookieSource } from "@/utils/request";
import { installExternalLinkInterceptor } from "@/utils/openLink";
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

/**
 * 与主窗口同一套分层持久化：localStorage 负责同步 hydrate，Tauri store 负责
 * 落盘与跨窗口同步。设置窗口正是靠后者把改动实时推给主窗口的——在此之前只有
 * playerBridge 里那 16 个歌词字段能回到主窗口。
 */
async function bootstrap() {
  const pinia = createPinia();
  pinia.use(piniaPluginPersistedstate);
  await installTauriPinia(pinia);

  // 顺序是硬约束：pinia.use() 要等 `app.use(pinia)` 之后才真正生效，在那之前
  // 实例化 store，persistedstate 不会 hydrate、$tauri 也不会注入。
  const app = createApp(SlaveApp);
  app.use(pinia);
  app.use(i18n);
  app.use(router);

  const settingData = useSettingDataStore(pinia);
  const siteData = useSiteDataStore(pinia);
  // 持久化层出问题不能让从窗口白屏——歌词浮层还得继续显示。
  try {
    await startTauriPiniaStores([
      { store: settingData, onHydrated: () => repairSettingData(settingData) },
      { store: siteData, onHydrated: () => repairSiteData(siteData) },
    ]);
  } catch (err) {
    console.error("[slave] persistence bootstrap failed", err);
  }

  // 外链统一出口：Tauri 交给系统浏览器，Web 走新标签页
  installExternalLinkInterceptor();

  // 从窗口不 hydrate userData（登录态由主窗口拥有），但 persistedstate 会从
  // localStorage 恢复 store 里的 cookie。内嵌链路必须显式带 cookie，而独立的
  // "cookie" 键只在登录那一刻写过一次，所以宁可读 store。
  // 这里只设置 cookie 来源，不动 transport：从窗口的接口调用跟随主窗口的选择，
  // 而 setNcmTransport 的默认值本来就是 remote。
  setNcmCookieSource(() => useUserDataStore(pinia).cookie ?? "");

  app.mount("#app");
}

bootstrap().catch((err) => {
  console.error("[slave] bootstrap failed", err);
});
