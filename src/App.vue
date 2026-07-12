<template>
  <!-- Standalone windows (tray popup etc.) — bare router-view, no nav/player/titlebar -->
  <template v-if="isStandaloneWindow">
    <router-view />
  </template>
  <!-- Normal app layout -->
  <Provider v-else>
    <div :class="appBodyClasses">
      <div
        class="app-layout-wrapper"
        :style="{ '--sidebar-width': setting.sidebarCollapsed ? '56px' : '208px' }"
      >
        <Sidebar />
        <n-layout
          :class="[
            'app-layout',
            {
              'player-visible': hasPlayBar,
              'queue-open': showInlineQueue,
            },
          ]"
          style="height: 100vh"
        >
          <div v-if="usesDesktopTauriChrome" class="nav-drag-layer" data-tauri-drag-region />
          <Nav :class="['app-nav-overlay', { 'tauri-nav': usesDesktopTauriChrome }]" />
          <div class="content-panel-frame" aria-hidden="true" />
          <n-layout-content
            position="absolute"
            :class="[
              hasPlayBar ? 'show' : '',
              {
                'settings-route': route.name === 'setting',
              },
            ]"
            :native-scrollbar="false"
            embedded
          >
            <div ref="contentStage" :class="['content-stage', { 'queue-open': showInlineQueue }]">
              <main
                ref="mainContent"
                :class="['main', { 'settings-main': route.name === 'setting' }]"
                id="mainContent"
              >
                <n-back-top
                  :bottom="music.getPlaylists[0] && music.showPlayBar ? 100 : 40"
                  style="transition: all 0.3s; z-index: 999"
                />
                <router-view v-slot="{ Component, route }">
                  <transition name="fade-scale" mode="out-in">
                    <keep-alive :max="15">
                      <component
                        :is="Component"
                        :key="
                          (route.matched[0]?.path ?? route.path) +
                          (route.query.id ? `_${route.query.id}` : '')
                        "
                      />
                    </keep-alive>
                  </transition>
                </router-view>
              </main>
              <aside class="queue-column" :aria-hidden="!showInlineQueue">
                <QueuePanel v-if="isInlineQueueLayout" />
              </aside>
            </div>
          </n-layout-content>
          <Player />
        </n-layout>
      </div>
      <MobileTabBar />
    </div>
    <TitleBar v-if="usesDesktopTauriChrome" />
  </Provider>
</template>

<script setup lang="ts">
import { musicStore, userStore, settingStore, siteStore } from "@/store";
import { useRouter, useRoute } from "vue-router";
import { getLoginState, refreshLogin } from "@/api/login";
import { userDailySignin, userYunbeiSign } from "@/api/user";
import { useI18n } from "vue-i18n";
import {
  getDesktopEnvironment,
  isMobile,
  isTauri,
  windowManager,
  type DesktopEnvironment,
} from "@/utils/tauri";

import { setPageVisible } from "@/utils/AudioContext";
import Provider from "@/components/Provider/index.vue";
import Nav from "@/components/Nav/index.vue";
import Player from "@/components/Player/index.vue";
import TitleBar from "@/components/TitleBar/index.vue";
import Sidebar from "@/components/Sidebar/index.vue";
import MobileTabBar from "@/components/Sidebar/MobileTabBar.vue";
import QueuePanel from "@/components/QueuePanel/index.vue";
import packageJson from "@/../package.json";
import { INLINE_QUEUE_MEDIA_QUERY } from "@/utils/playlistLayout";
import { ref, watch, computed, h } from "vue";

const { t } = useI18n();
const music = musicStore();
const user = userStore();
const setting = settingStore();
const site = siteStore();
const router = useRouter();
const route = useRoute();
const contentStage = ref<HTMLElement | null>(null);
const mainContent = ref<HTMLElement | null>(null);
const isInlineQueueLayout = ref(false);
const desktopEnvironment = ref<DesktopEnvironment | null>(null);
const usesDesktopTauriChrome = ref(false);
let inlineQueueMediaQuery: MediaQueryList | null = null;

const showInlineQueue = computed(() => isInlineQueueLayout.value && music.showPlayList);
const hasPlayBar = computed(() => Boolean(music.getPlaylists[0] && music.showPlayBar));
const appBodyClasses = computed(() => [
  "app-body",
  {
    "bigplayer-open": music.showBigPlayer,
    "native-traffic-lights": desktopEnvironment.value?.usesNativeTrafficLights ?? false,
    "hyprland-shell": desktopEnvironment.value?.isHyprland ?? false,
    "linux-shell": desktopEnvironment.value?.isLinux ?? false,
  },
]);

const syncInlineQueueLayout = (event?: MediaQueryListEvent) => {
  if (event) {
    isInlineQueueLayout.value = event.matches;
    return;
  }
  isInlineQueueLayout.value = inlineQueueMediaQuery?.matches ?? false;
};

// Standalone window detection (tray popup, etc.)
const isStandaloneWindow = computed(() => !!route.meta.standalone);

// 公告数据
const annShow = import.meta.env.VITE_ANN_TITLE && import.meta.env.VITE_ANN_CONTENT ? true : false;
const annTitle = import.meta.env.VITE_ANN_TITLE;
const annContene = import.meta.env.VITE_ANN_CONTENT;
const annDuration = Number(import.meta.env.VITE_ANN_DURATION);

// 空格暂停与播放
const spacePlayOrPause = (e) => {
  if (e.code === "Space") {
    console.log(e.target.tagName);
    if (router.currentRoute.value.name === "video") return false;
    if (e.target.tagName === "BODY") {
      e.preventDefault();
      music.setPlayState(!music.getPlayState);
    } else {
      return false;
    }
  }
};

// 更改页面标题
const setSiteTitle = (val) => {
  const title = val
    ? val === import.meta.env.VITE_SITE_TITLE
      ? val
      : val + " - " + import.meta.env.VITE_SITE_TITLE
    : (sessionStorage.getItem("siteTitle") ?? import.meta.env.VITE_SITE_TITLE);
  site.siteTitle = title;
  sessionStorage.setItem("siteTitle", title);
  if (!music.getPlayState) {
    window.document.title = title;
  }
};

// 刷新登录
const toRefreshLogin = () => {
  const today = Date.now();
  const threeDays = 3 * 24 * 60 * 60 * 1000;
  const lastRefreshDate = new Date(localStorage.getItem("lastRefreshDate")).getTime();
  if (today - lastRefreshDate >= threeDays || !lastRefreshDate) {
    refreshLogin().then((res) => {
      if (res.code === 200) {
        localStorage.setItem("lastRefreshDate", new Date(today).toLocaleDateString());
        console.log("刷新登录成功");
      } else {
        console.error("刷新登录失败");
      }
    });
  }
};

// 用户签到
const signIn = () => {
  const today = new Date().toLocaleDateString();
  const lastSignInDate = localStorage.getItem("lastSignInDate");
  if (lastSignInDate !== today) {
    const signInPromises = [userDailySignin(0), userYunbeiSign()];
    Promise.all(signInPromises)
      .then((results) => {
        localStorage.setItem("lastSignInDate", today);
        console.log(t("general.message.signInSuccess"), results[0], results[1]);
        $notification["success"]({
          content: t("general.message.signInSuccess"),
          meta: t("general.message.signInSuccessDesc"),
          duration: 3000,
        });
      })
      .catch((error) => {
        console.error(t("general.message.signInFailed"), error);
        $message.error(t("general.message.signInFailed"));
      });
  }
};

// 系统重置
const cleanAll = () => {
  if ($message) {
    $message.success(t("other.cleanAll"));
  } else {
    alert(t("other.cleanAll"));
  }
  localStorage.clear();
  document.location.reload();
};

// 滚动至顶部
const scrollToTop = () => {
  nextTick().then(() => {
    if (contentStage.value || mainContent.value) {
      (contentStage.value ?? mainContent.value)?.scrollIntoView({ behavior: "smooth" });
    } else {
      const mainContent = document.getElementById("mainContent");
      mainContent?.scrollIntoView({ behavior: "smooth" });
    }
  });
};

// Tauri: handle close behavior (hide-to-tray vs exit vs ask)
const rememberClose = ref(false);
const handleCloseRequested = () => {
  const behavior = setting.closeBehavior;
  if (behavior === "tray") {
    windowManager.hideWindow("main");
  } else if (behavior === "exit") {
    windowManager.quitApp();
  } else {
    // "ask" — show dialog with "remember" checkbox
    rememberClose.value = false;
    $dialog.create({
      title: t("closeDialog.title"),
      content: () =>
        h("div", [
          h("p", { style: "margin: 0 0 12px 0" }, t("closeDialog.message")),
          h(
            "label",
            {
              style:
                "display: flex; align-items: center; gap: 6px; cursor: pointer; font-size: 13px",
            },
            [
              h("input", {
                type: "checkbox",
                checked: rememberClose.value,
                onChange: (e: Event) => {
                  rememberClose.value = (e.target as HTMLInputElement | null)?.checked ?? false;
                },
              }),
              t("closeDialog.remember"),
            ],
          ),
        ]),
      positiveText: t("closeDialog.hideToTray"),
      negativeText: t("closeDialog.exit"),
      type: "info",
      onPositiveClick: () => {
        if (rememberClose.value) setting.closeBehavior = "tray";
        windowManager.hideWindow("main");
      },
      onNegativeClick: () => {
        if (rememberClose.value) setting.closeBehavior = "exit";
        windowManager.quitApp();
      },
    });
  }
};

onMounted(() => {
  if (typeof window !== "undefined") {
    inlineQueueMediaQuery = window.matchMedia(INLINE_QUEUE_MEDIA_QUERY);
    syncInlineQueueLayout();
    inlineQueueMediaQuery.addEventListener("change", syncInlineQueueLayout);
  }

  // 挂载方法至全局
  window.$scrollToTop = scrollToTop;
  window.$cleanAll = cleanAll;
  window.$signIn = signIn;
  window.$setSiteTitle = setSiteTitle;

  // 更改页面语言
  const html = document.documentElement;
  if (html) html.setAttribute("lang", setting.language);

  // Tauri 环境标识
  if (typeof window !== "undefined" && "__TAURI__" in window) {
    document.documentElement.classList.add("tauri-app");
    isMobile()
      .then((mobile) => {
        usesDesktopTauriChrome.value = !mobile;
        if (mobile) return null;
        return getDesktopEnvironment();
      })
      .then((environment) => {
        if (environment) desktopEnvironment.value = environment;
      })
      .catch(() => {});
  }

  // 公告
  if (annShow) {
    $notification["info"]({
      content: annTitle,
      meta: annContene,
      duration: annDuration,
    });
  }

  // 版权声明
  const logoText = import.meta.env.VITE_SITE_TITLE;
  const copyrightNotice = `\n\n版本: ${packageJson.version}\n作者: ${packageJson.author}\n作者主页: ${packageJson.home}\nGitHub: ${packageJson.github}`;
  console.info(
    `%c${logoText} %c ${copyrightNotice}`,
    "color:#f55e55;font-size:26px;font-weight:bold;",
    "font-size:16px",
  );
  console.info(
    "若站点出现异常，可尝试在下方输入 %c$cleanAll()%c 然后按回车来重置",
    "background: #eaeffd;color:#f55e55;padding: 4px 6px;border-radius:8px;",
    "background:unset;color:unset;",
  );

  // 检查账号登录状态
  getLoginState()
    .then((res) => {
      if (res.data.profile && user.userLogin) {
        // 签到
        if (setting.autoSignIn) signIn();
        // 刷新登录
        toRefreshLogin();
        // 保存登录信息
        user.userLogin = true;
        user.setUserData(res.data.profile);
        user.setUserOtherData();
      } else {
        user.userLogOut();
        if (music.getPlayListMode === "cloud") {
          $message.info(t("other.loginExpired"));
          music.setPlaylists([]);
          music.setPlayListMode("list");
        }
      }
    })
    .catch((err) => {
      console.error(t("general.message.acquisitionFailed"), err);
      $message.error(t("general.message.acquisitionFailed"));
      router.push("/500");
      return false;
    });

  // 获取喜欢音乐列表
  music.setLikeList();

  // 键盘监听
  window.addEventListener("keydown", spacePlayOrPause);

  // Tauri: handle main window close-requested event
  if (isTauri()) {
    window.__TAURI__?.event
      .listen("main-close-requested", () => {
        handleCloseRequested();
      })
      .catch(() => {});

    // Suspend animations when main window is hidden (close-to-tray)
    windowManager.onMainWindowVisibility((visible) => {
      setPageVisible(visible);
    });
  }
});

onBeforeUnmount(() => {
  inlineQueueMediaQuery?.removeEventListener("change", syncInlineQueueLayout);
});
</script>

<style lang="scss" scoped>
.main-content {
  transition:
    transform 0.3s,
    opacity 0.3s;

  .bigplayer-on {
    opacity: 0;
    transform: scale(0.9);
  }
}

.n-layout-content {
  top: 0;
  bottom: var(--layout-content-bottom);
  scroll-padding-top: var(--content-stage-padding-top);
  clip-path: inset(
    var(--content-stage-padding-top) var(--content-stage-padding-right)
      var(--content-stage-padding-y) var(--content-stage-padding-x) round var(--radius-panel)
  );
  transition: all var(--duration-300) var(--ease-in-out);
  background-color: transparent !important;
  z-index: 1;

  :deep(.n-scrollbar-rail--vertical) {
    right: var(--content-scrollbar-right) !important;
    // `.main` 设了 position:relative + z-index:2，与滚动条 thumb(Naive 内部 z-index:1)
    // 共享同一层叠上下文（.n-scrollbar / .n-scrollbar-container 均不成栈），
    // 内容层因此盖在 thumb 之上、吞掉拖拽。抬高 rail 使 thumb 位于内容之上即可恢复拖拽；
    // rail 轨道本身 pointer-events:none，普通内容点击不受影响。
    z-index: 3;
  }

  &.settings-route {
    overflow: hidden;

    :deep(.n-layout-scroll-container) {
      overflow: hidden !important;
    }

    .content-stage {
      height: 100%;
      min-height: 0;
      overflow: hidden;
    }
  }

  .main {
    position: relative;
    z-index: 2;
    flex: 1 1 auto;
    box-sizing: border-box;
    width: 100%;
    min-width: 0;
    min-height: var(--content-panel-height);
    margin: 0;
    padding-top: 48px;
    scroll-margin-top: var(--content-stage-padding-top);
    background: transparent;
    transition:
      min-height var(--duration-300) var(--ease-in-out),
      background-color var(--duration-200) var(--ease-out);

    &.settings-main {
      height: var(--content-panel-height);
      min-height: var(--content-panel-height);
      max-height: var(--content-panel-height);
      overflow: hidden;
    }
  }

  .content-stage {
    position: relative;
    min-height: 100%;
    box-sizing: border-box;
    display: flex;
    align-items: stretch;
    justify-content: flex-start;
    gap: 0;
    padding: var(--content-stage-padding-y) var(--content-stage-padding-right)
      var(--content-stage-padding-y) var(--content-stage-padding-x);
    padding-top: var(--content-stage-padding-top);
    scroll-margin-top: 0;
    transition: gap var(--duration-300) var(--ease-in-out);

    &.queue-open {
      gap: 0;

      .queue-column {
        flex-basis: var(--queue-column-width);
        width: var(--queue-column-width);
        opacity: 1;
        transform: translateX(0);
        pointer-events: auto;
      }
    }
  }

  .queue-column {
    flex: 0 0 0;
    z-index: 0;
    width: 0;
    min-width: 0;
    height: var(--content-panel-height);
    position: sticky;
    top: var(--content-stage-padding-top);
    overflow: hidden;
    opacity: 0;
    transform: translateX(16px);
    pointer-events: none;
    transition:
      flex-basis var(--duration-300) var(--ease-in-out),
      width var(--duration-300) var(--ease-in-out),
      opacity var(--duration-200) var(--ease-out),
      transform var(--duration-300) var(--ease-in-out);

    :deep(.queue-panel) {
      height: 100%;
    }
  }

  @media (max-width: 768px) {
    clip-path: none;

    .queue-column {
      display: none;
    }

    .content-stage,
    .content-stage.queue-open {
      padding: 0;
    }

    .main {
      min-height: 100%;
      padding-top: calc(52px + var(--app-safe-area-top, 0px));

      &.settings-main {
        height: auto;
        max-height: none;
        overflow: visible;
      }
    }
  }
}

// AMLL-style: .app-body is the outer wrapper (no transform → ::after position:fixed works)
// .app-layout-wrapper is the flexbox container that holds sidebar + content
.app-body {
  height: 100vh;
  overflow: hidden;
  background-color: var(--app-shell-bg, var(--layout-bg, #fff));

  // Dark overlay — on the non-transformed wrapper so position:fixed covers the full viewport
  &::after {
    content: "";
    display: block;
    position: fixed;
    left: 0;
    top: 0;
    width: 100vw;
    height: 100vh;
    pointer-events: none;
    opacity: 0;
    background-color: #000;
    transition: opacity var(--duration-500) var(--ease-out);
    z-index: 1999;
  }

  &.bigplayer-open::after {
    opacity: 0.75;
    // Scrim 作为模态背板：BigPlayer 打开时吞掉未被其捕获的指针输入，
    // 防止点击穿透到下方层级更低的 mini player(.player z-index:2) 与主内容。
    // BigPlayer(z-index:2000) 在 scrim(1999) 之上，其自身交互不受影响；
    // TitleBar(9999)、侧栏搜索浮层(--z-search-overlay:2200) 亦在其上，窗口控制仍可点。
    pointer-events: auto;
  }

  &.native-traffic-lights {
    --app-titlebar-width: 0px;
    --app-titlebar-gap: 0px;
    --app-native-traffic-light-reserve-y: 42px;

    :deep(.sidebar .sidebar-header) {
      min-height: calc(var(--app-native-traffic-light-reserve-y) + 46px);
      padding-top: var(--app-native-traffic-light-reserve-y);
    }
  }
}

.app-layout-wrapper {
  display: flex;
  height: 100vh;
  background-color: var(--app-shell-bg, var(--layout-bg, #fff));
  overflow: hidden;

  .bigplayer-open & {
    overflow: hidden;
  }
}

.app-layout {
  position: relative;
  flex: 1;
  min-width: 0;
  --content-stage-padding-x: 0px;
  --content-stage-padding-right: 8px;
  --content-stage-padding-y: 0px;
  --content-stage-padding-top: var(--app-shell-top-gap);
  --content-scrollbar-right: calc(var(--content-stage-padding-right) + 2px);
  --player-right-inset: var(--content-stage-padding-right);
  --layout-content-bottom: 0px;
  --queue-column-width: clamp(292px, 23vw, 342px);
  --content-panel-height: calc(
    100vh - var(--layout-content-bottom) - var(--content-stage-padding-top) - var(
        --content-stage-padding-y
      )
  );
  --content-panel-border-color: var(
    --content-panel-border,
    color-mix(
      in srgb,
      rgb(var(--content-panel-accent-rgb, 128, 128, 128))
        var(--content-panel-border-accent-strength, 18%),
      var(--content-panel-border-base, rgba(0, 0, 0, 0.12))
    )
  );
  background-color: var(--app-shell-bg, var(--layout-bg, #fff)) !important;

  &.player-visible {
    --layout-content-bottom: 70px;
  }

  &.queue-open {
    --content-stage-padding-right: 0px;
    --content-scrollbar-right: calc(var(--queue-column-width) + 2px);
    --player-right-inset: calc(var(--queue-column-width) + 8px);
  }

  @media (min-width: 1041px) and (max-width: 1180px) {
    --content-stage-padding-x: 0px;
    --content-stage-padding-right: 8px;
    --content-stage-padding-y: 0px;
    --queue-column-width: clamp(252px, 24vw, 292px);

    &.queue-open {
      --content-stage-padding-right: 0px;
      --content-scrollbar-right: calc(var(--queue-column-width) + 2px);
      --player-right-inset: calc(var(--queue-column-width) + 8px);
    }
  }

  @media (max-width: 768px) {
    --content-stage-padding-top: 0px;
    --content-stage-padding-right: 0px;
    --content-scrollbar-right: 0px;
    --player-right-inset: 0px;
    // 56px tab bar only + safe-area-bottom (home indicator).
    --layout-content-bottom: calc(56px + var(--app-safe-area-bottom, 0px));

    &.player-visible {
      // 70px player + 56px tab bar + safe-area-bottom.
      --layout-content-bottom: calc(126px + var(--app-safe-area-bottom, 0px));
    }
  }
}

.content-panel-frame {
  position: absolute;
  top: var(--content-stage-padding-top);
  right: var(--content-stage-padding-right);
  bottom: calc(var(--layout-content-bottom) + var(--content-stage-padding-y));
  left: var(--content-stage-padding-x);
  z-index: 0;
  pointer-events: none;
  border: 1px solid var(--content-panel-border-color);
  border-radius: var(--radius-panel);
  background:
    var(--content-panel-stage-gradient, linear-gradient(transparent, transparent)),
    var(--content-panel-bg, var(--app-shell-bg, #fff));
  box-shadow: var(
    --content-panel-shadow,
    inset 0 1px 0 rgba(255, 255, 255, 0.32),
    inset 1px 0 0 rgba(255, 255, 255, 0.18)
  );
  transition:
    right var(--duration-300) var(--ease-in-out),
    bottom var(--duration-300) var(--ease-in-out),
    border-color var(--duration-200) var(--ease-out),
    border-radius var(--duration-300) var(--ease-in-out),
    background-color var(--duration-200) var(--ease-out),
    box-shadow var(--duration-200) var(--ease-out);

  &::before,
  &::after {
    content: "";
    position: absolute;
    top: 18px;
    bottom: var(--radius-panel);
    width: 18px;
    pointer-events: none;
    transition: opacity var(--duration-200) var(--ease-out);
  }

  &::before {
    left: -18px;
    background: linear-gradient(to right, transparent, var(--content-panel-edge-shadow));
  }

  &::after {
    right: -18px;
    opacity: 0;
    background: linear-gradient(to left, transparent, var(--content-panel-edge-shadow));
  }

  .app-layout.queue-open & {
    right: calc(var(--content-stage-padding-right) + var(--queue-column-width));
    border-radius: var(--radius-panel);

    &::after {
      opacity: 1;
    }
  }

  @media (max-width: 768px) {
    display: none;
  }
}

.nav-drag-layer {
  position: fixed;
  top: 0;
  left: var(--sidebar-width, 208px);
  right: calc(
    var(--app-floating-control-inset, 14px) + var(--app-titlebar-width, 114px) +
      var(--app-titlebar-gap, 10px)
  );
  height: var(--app-drag-region-height);
  z-index: 1500;
  pointer-events: auto;

  @media (max-width: 768px) {
    display: none;
  }
}

.app-nav-overlay {
  position: fixed;
  top: var(--app-floating-control-top);
  left: calc(var(--sidebar-width, 208px) + var(--app-floating-control-inset, 14px));
  right: var(--app-floating-control-inset, 14px);
  width: auto;
  z-index: 1600;
  pointer-events: none;

  &.tauri-nav {
    right: calc(
      var(--app-floating-control-inset, 14px) + var(--app-titlebar-width, 114px) +
        var(--app-titlebar-gap, 10px)
    );
  }

  @media (max-width: 768px) {
    top: 0;
    left: 12px;
    right: 12px;
    width: auto;
  }
}
</style>
