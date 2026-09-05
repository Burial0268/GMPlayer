import { createApp, watch } from "vue";
import { createPinia } from "pinia";
import { useI18n } from "@/locale";
import { useSafeAreaVars } from "./composables/useSafeAreaVars";
import piniaPluginPersistedstate from "pinia-plugin-persistedstate";

import App from "@/App.vue";
import router from "@/router/index";
import { audioPreheat } from "@/utils/tauri/audio/bridge";
import { isTauri } from "@/utils/tauri/core/runtime";
import { installTauriPinia, startTauriPiniaStores } from "@/utils/tauri/store/piniaPersistence";
import useSettingDataStore, { repairSettingData } from "@/store/settingData";
import useSiteDataStore, { repairSiteData } from "@/store/siteData";
import useUserDataStore from "@/store/userData";
import { installExternalLinkInterceptor } from "@/utils/openLink";
import { setNcmTransport, setNcmCookieSource } from "@/utils/request";

// 全局样式
import "@/style/global.scss";
import "@/style/animate.scss";

if ("serviceWorker" in navigator) {
  let pwaMessage: { destroy: () => void } | null = null;

  // 检测到更新提醒
  navigator.serviceWorker.addEventListener("onupdatefound", () => {
    console.info("发现站点更新，正在下载新版本");
    pwaMessage = $message.loading("发现站点更新，正在下载新版本", {
      closable: true,
      duration: 0,
    });
  });

  // 更新完成提醒
  navigator.serviceWorker.addEventListener("controllerchange", () => {
    console.info("站点已更新，刷新后生效");
    if (pwaMessage) pwaMessage?.destroy();
    $message.info("站点已更新，刷新后生效", {
      closable: true,
      duration: 0,
    });
  });
}

/**
 * 启动流程之所以是异步的：Tauri 的 store 后端要经过一次 IPC 才能拿到状态，
 * 而 useI18n() 会同步读 settingData.language、主题也在挂载时就要读到位。
 * 先把托管的 store hydrate 完再挂载，挂载后就不会有状态跳变。
 * Web 环境下 installTauriPinia 直接返回 false，await 只多花一个微任务。
 */
async function bootstrap() {
  const pinia = createPinia();
  pinia.use(piniaPluginPersistedstate);
  await installTauriPinia(pinia);

  // 顺序是硬约束：pinia.use() 在 `app.use(pinia)` 之前只是把插件挂进
  // toBeInstalled 队列（pinia 源码里 `use()` 判的是 `this._a`），要等安装到
  // app 上才真正生效。在这之前实例化 store，persistedstate 不会 hydrate、
  // $tauri 也不会注入——store 会静悄悄停在默认值上。
  const app = createApp(App).use(pinia).use(router);

  // 持久化层出任何问题都不能让页面白屏。走到这里 persistedstate 已经
  // hydrate 完了（localStorage 那份是同步的），Tauri 层失败只是少了落盘和
  // 跨窗口同步——Web 上更是整段都不该存在。
  try {
    const settingData = useSettingDataStore(pinia);
    const siteData = useSiteDataStore(pinia);
    await startTauriPiniaStores([
      // onHydrated 是版本修复步：Tauri store 文件可能是旧版本写的，而它的
      // $patch 发生在 persistedstate 的 afterHydrate 之后。回调幂等。
      { store: settingData, onHydrated: () => repairSettingData(settingData) },
      { store: siteData, onHydrated: () => repairSiteData(siteData) },
      useUserDataStore(pinia),
    ]);
  } catch (err) {
    console.error("[main] persistence bootstrap failed", err);
  }

  // NCM 接口链路：必须在 hydrate 之后才知道用户选的是哪条，之后跟随设置变化。
  // Web 环境下 setNcmTransport 自己会退回 remote，不需要在这里分支。
  {
    const settingData = useSettingDataStore(pinia);
    // 内嵌链路必须显式带 cookie（远端链路靠 withCredentials）。store 才是权威副本：
    // 桌面端 userData 落在 Tauri store 文件里，hydrate 只会填回 store，不会重写
    // localStorage 里那个独立的 "cookie" 键——照着 localStorage 读会在已登录的情况下
    // 读到空，而缺 cookie 的 /song/url/v1 不报错，只是安静地返回 30 秒试听片段。
    setNcmCookieSource(() => useUserDataStore(pinia).cookie ?? "");
    setNcmTransport(settingData.ncmTransport);
    // 下载队列活在 Rust 里，所以它拿不到 store——凭据必须推给它。用 watch 而不是
    // 只在启动时推一次：队列会比开启它的那个页面活得更久（Android 上 WebView 被系统
    // 回收是常态），而它没法在需要的时候回头问页面要 cookie。
    if (isTauri()) {
      const userData = useUserDataStore(pinia);
      watch(
        () => userData.cookie,
        (cookie) => {
          void import("@/utils/download").then((m) =>
            m.downloadSetCredentials(cookie || null).catch((err) => {
              console.warn("[main] could not push download credentials", err);
            }),
          );
        },
        { immediate: true },
      );
    }
    watch(
      () => settingData.ncmTransport,
      (mode) => {
        setNcmTransport(mode);
        // 原生播放源解析跟随同一条链路。配置是推送式的（planner 在 WebView 冻结
        // 时仍在解析），所以切换后必须主动重推，不能等下一次自然触发。
        void import("@/utils/AudioContext/NativeResolverConfigSync").then((m) =>
          m.syncNativeResolverConfig({ force: true }),
        );
      },
    );
  }

  // 国际化（读 settingData.language，必须在 hydrate 之后）
  useI18n(app);

  useSafeAreaVars();

  // 外链统一出口：Tauri 交给系统浏览器，Web 走新标签页
  installExternalLinkInterceptor();

  app.mount("#app");

  if (isTauri()) {
    void audioPreheat().catch((err) => {
      console.warn("[main] native audio preheat failed", err);
    });
  }
}

bootstrap().catch((err) => {
  console.error("[main] bootstrap failed", err);
});
