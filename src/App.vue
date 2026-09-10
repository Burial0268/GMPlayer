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
          :theme-overrides="usesNativeWindowEffect ? transparentLayoutTheme : undefined"
          style="height: 100vh"
        >
          <div v-if="usesDesktopTauriChrome" class="nav-drag-layer" data-tauri-drag-region />
          <Nav :class="['app-nav-overlay', { 'tauri-nav': usesDesktopTauriChrome }]" />
          <div class="content-panel-frame" aria-hidden="true" />
          <div
            ref="topShadow"
            :class="[
              'content-top-shadow',
              {
                dark: setting.getSiteTheme === 'dark',
                scrolled: contentScrolled,
              },
            ]"
            aria-hidden="true"
          >
            <div class="nav-blur-step" />
            <div class="nav-blur-step" />
            <div class="nav-blur-step" />
            <div class="nav-blur-step" />
            <div class="nav-blur-step" />
            <div class="nav-blur-tint" />
          </div>
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
            @scroll="handleContentScroll"
          >
            <div ref="contentStage" :class="['content-stage', { 'queue-open': showInlineQueue }]">
              <main
                ref="mainContent"
                :class="['main', { 'settings-main': route.name === 'setting' }]"
                id="mainContent"
              >
                <!-- BackTop teleports to <body>, so it is outside .app-layout and cannot
                     inherit --layout-content-bottom; it reads the :root bottom-chrome
                     tokens instead. z-index 999 keeps it above the content and the mini
                     player (2) but under the tab bar (1000) and BigPlayer (2000). -->
                <n-back-top
                  :right="'var(--app-back-top-right)'"
                  :bottom="
                    hasPlayBar
                      ? 'var(--app-back-top-bottom-with-player)'
                      : 'var(--app-back-top-bottom)'
                  "
                  style="transition: all var(--duration-300) var(--ease-out); z-index: 999"
                />
                <AppLayerHost />
              </main>
              <aside class="queue-column" :aria-hidden="!showInlineQueue">
                <QueuePanel v-if="isInlineQueueLayout" />
              </aside>
            </div>
          </n-layout-content>
          <Player />
        </n-layout>
      </div>
      <div
        :class="[
          'bottom-glass',
          { dark: setting.getSiteTheme === 'dark', 'has-player': hasPlayBar },
        ]"
        aria-hidden="true"
      />
      <MobileTabBar />
    </div>
    <TitleBar v-if="usesDesktopTauriChrome" />
  </Provider>
</template>

<script setup lang="ts">
import { musicStore, userStore, settingStore, siteStore, localLibraryStore } from "@/store";
import { resetPersistedStorage } from "@/store/resetPersistence";
import { useRouter, useRoute } from "vue-router";
import { getLoginState, refreshLogin } from "@/api/login";
import { userDailySignin, userYunbeiSign } from "@/api/user";
import { useI18n } from "vue-i18n";
import {
  getDesktopEnvironment,
  isMobile,
  isMobileDevice,
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
import AppLayerHost from "@/components/Navigation/AppLayerHost.vue";
import { appInfo } from "@/utils/appInfo";
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
// Mirrors iOS scrollEdgeAppearance: the mobile header glass only materializes
// once content actually passes beneath it. At the scroll edge the band is fully
// transparent, so its blur can never soften the first line of page content —
// and there is zero backdrop-filter work while the view sits idle at the top.
const contentScrolled = ref(false);
const topShadow = ref<HTMLElement | null>(null);
const isInlineQueueLayout = ref(false);
const isDesktopTauriRuntime = isTauri() && !isMobileDevice();
const isNativeEffectPlatform = /Win|Mac/i.test(
  window.navigator.platform || window.navigator.userAgent,
);
// Windows-only marker for the native acrylic backdrop tint (option 1). macOS
// vibrancy and non-native platforms must keep the bare transparent shell.
const isWindowsNativeEffect = /Win/i.test(window.navigator.platform || window.navigator.userAgent);
const desktopEnvironment = ref<DesktopEnvironment | null>(null);
const usesDesktopTauriChrome = ref(isDesktopTauriRuntime);
let inlineQueueMediaQuery: MediaQueryList | null = null;

const showInlineQueue = computed(() => isInlineQueueLayout.value && music.showPlayList);
const hasPlayBar = computed(() => Boolean(music.getPlaylists[0] && music.showPlayBar));
const usesNativeWindowEffect = computed(
  () =>
    usesDesktopTauriChrome.value &&
    (desktopEnvironment.value?.isMacos ||
      desktopEnvironment.value?.os === "windows" ||
      isNativeEffectPlatform),
);
const transparentLayoutTheme = {
  color: "transparent",
  colorEmbedded: "transparent",
};
const appBodyClasses = computed(() => [
  "app-body",
  {
    "bigplayer-open": music.showBigPlayer,
    "native-window-effect": usesNativeWindowEffect.value,
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

watch(
  usesNativeWindowEffect,
  (enabled) => {
    document.documentElement.classList.toggle("native-window-effect-root", enabled);
    // Gate the native acrylic tint to Windows so macOS vibrancy is untouched.
    document.documentElement.classList.toggle(
      "windows-native-effect-root",
      enabled && isWindowsNativeEffect,
    );
  },
  { immediate: true },
);

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
const cleanAll = async () => {
  if ($message) {
    $message.success(t("other.cleanAll"));
  } else {
    alert(t("other.cleanAll"));
  }
  await resetPersistedStorage();
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

// Scroll-linked header material. Scroll position sets a TARGET, and the value
// actually rendered chases it with exponential smoothing. Two failure modes this
// resolves, having tried both endpoints:
//   - A timed transition on a boolean threshold ignores the input entirely: an 8px
//     flick spends 300ms slamming the full material in.
//   - A pure scrub (including native `animation-timeline: scroll()`) tracks input
//     perfectly but cannot rate-limit itself. Flicking to the top crosses the last
//     72px in a few frames, so the tint vanishes as a cut — and because the tint is
//     the most visible layer, that is the part that looks wrong.
// Smoothing gives scrub-like tracking during ordinary scrolling, where the target
// moves gradually and the lag is 1-2 frames, while imposing a floor on how fast the
// material can change when the target jumps. That floor is the whole point: it also
// covers instant jumps that no scroll-linked approach can smooth, like a
// programmatic scroll-to-top or a route resetting the shared scroller.
// The publish-progress-as-a-custom-property idea is TriggerJS's; the library itself
// measures elements against the viewport, which does not fit a nested scroller.
const NAV_GLASS_RANGE = 72;
// Max time, ms, for the material to traverse its full range. These are a RATE
// LIMIT, not a lag filter: while the target moves slower than the cap — ordinary
// scrolling — the value tracks it exactly, frame for frame, so the scrub stays
// 1:1 with the finger. The cap only engages when the target jumps faster than the
// eye accepts, which is precisely the flick-to-top case.
//
// Exponential smoothing was tried first and is worse on both counts: it lags every
// scroll however gentle, and its tail means "done" is asymptotic — a release slow
// enough to look right left the value creeping for over a second, holding the
// backdrop passes alive with it. A rate limit is bounded and terminates exactly.
//
// Asymmetric, the fast-attack / slow-release envelope this codebase already uses
// for audio peak detection. Rising must keep up with the finger; falling should
// linger, because the tint is the layer the eye follows and its exit is what reads
// as abrupt.
const NAV_GLASS_RISE_MS = 180;
const NAV_GLASS_FALL_MS = 340;
let glassTarget = 0;
let glassCurrent = 0;
let glassRaf = 0;
// The move in flight is described by where it began and when, so every frame
// derives its value from real elapsed time rather than accumulating per-frame
// deltas.
//
// Accumulating is what made the fade outlast its own duration. A clamped per-frame
// dt cannot consume more time than the clamp, so each dropped frame stretched the
// fade in wall clock — measured, a 286ms fall took 625ms at 8fps — and the backlog
// then landed in one visible jump. Mobile momentum scroll over five backdrop
// passes starves the main thread exactly that badly, which is why it read as
// "lingers, then disappears at once". Anchored, a starved frame costs smoothness
// but never duration.
let glassFromValue = 0;
let glassFromTs = 0;
let glassWritten = -1;

const commitGlassProgress = () => {
  const el = topShadow.value;
  if (!el) return;
  // Quantised to 1/1000. Coarser steps were fine when this drove a sub-pixel blur
  // radius, but it now drives layer opacity, where 1/200 is visible as banding.
  const next = Math.round(glassCurrent * 1000) / 1000;
  if (next === glassWritten) return;
  glassWritten = next;
  el.style.setProperty("--nav-glass-progress", String(next));
  // Gates the five backdrop passes at rest. Flips only at exactly zero, where
  // there is nothing left to see, so it can never register as a pop.
  contentScrolled.value = next > 0;
};

const stepGlassProgress = (ts: number) => {
  glassRaf = 0;
  const span = glassTarget - glassFromValue;
  // Distance-proportional, so this stays a rate limit: a short hop finishes
  // quickly, only a full traverse costs the whole duration.
  const ms = Math.abs(span) * (span > 0 ? NAV_GLASS_RISE_MS : NAV_GLASS_FALL_MS);
  // Lower clamp matters: an already-pending callback carries its frame-start ts,
  // which can predate an anchor set later in that same frame.
  const f = ms > 0 ? Math.min(Math.max((ts - glassFromTs) / ms, 0), 1) : 1;
  glassCurrent = f >= 1 ? glassTarget : glassFromValue + span * f;
  commitGlassProgress();
  if (glassCurrent !== glassTarget) glassRaf = requestAnimationFrame(stepGlassProgress);
};

const syncGlassProgress = (top: number, ts = performance.now()) => {
  const next = Math.min(Math.max(top / NAV_GLASS_RANGE, 0), 1);
  if (next === glassTarget) return;
  // Re-anchor on the live value so a reversal eases out of wherever it had got
  // to rather than snapping.
  glassTarget = next;
  glassFromValue = glassCurrent;
  glassFromTs = ts;
  if (glassCurrent !== glassTarget && !glassRaf) {
    glassRaf = requestAnimationFrame(stepGlassProgress);
  }
};

const handleContentScroll = (e: Event) => {
  syncGlassProgress((e.target as HTMLElement | null)?.scrollTop ?? 0);
};

// The scroll container is shared across routes, so a keep-alive'd page can come
// back already offset without emitting a scroll event. Resync from the live
// element rather than assuming the new route starts at the top.
watch(
  () => route.fullPath,
  () => {
    nextTick().then(() => {
      const scroller = contentStage.value?.closest(".n-scrollbar-container");
      syncGlassProgress(scroller instanceof HTMLElement ? scroller.scrollTop : 0);
    });
  },
);

// Tauri: handle close behavior (hide-to-tray vs exit vs ask)
const rememberClose = ref(false);
let unlistenCloseRequested: (() => void) | null = null;
let unlistenMainVisibility: (() => void) | null = null;
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

// 挂载方法至全局
//
// 在 setup 里挂，**不能**放进 onMounted：Vue 的挂载顺序是子先于父，所以路由页面的
// `onMounted` 比 App 自己的先跑。放在 onMounted 里的话，任何在自己 onMounted 里同步
// 调 `$setSiteTitle` 的页面，一旦是**刷新后的首屏**就会抛 TypeError —— 而异常发生在
// mounted 钩子里，Vue 只打一条 warn 并中止该子树的挂载，外层 `<transition mode="out-in">`
// 的进场因此永远不完成，`<main>` 里就只剩两个注释占位符。表现是「刷新后主内容空白、
// 控制台没有 error」。
//
// 大部分页面躲过了这个坑只是因为它们在网络回调里才调（那时 App 早挂好了）；
// 同步调的有 19 个 —— `/login`、`/setting`、`/history`、`/404`、整个 `/user/*`、
// `/discover/*` 和本地音乐各页，它们刷新时全都会中招。
window.$scrollToTop = scrollToTop;
window.$cleanAll = cleanAll;
window.$signIn = signIn;
window.$setSiteTitle = setSiteTitle;

onMounted(() => {
  if (typeof window !== "undefined") {
    inlineQueueMediaQuery = window.matchMedia(INLINE_QUEUE_MEDIA_QUERY);
    syncInlineQueueLayout();
    inlineQueueMediaQuery.addEventListener("change", syncInlineQueueLayout);
  }

  // A reload can restore the scroller mid-page, which emits no scroll event — the
  // band would then be missing until the user next scrolled.
  nextTick().then(() => {
    const scroller = contentStage.value?.closest(".n-scrollbar-container");
    if (scroller instanceof HTMLElement) syncGlassProgress(scroller.scrollTop);
  });

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
  const copyrightNotice = `\n\n版本: ${appInfo.version}\n作者: ${appInfo.author}\n作者主页: ${appInfo.home}\nGitHub: ${appInfo.github}`;
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

  // 本地库的来源 / 歌单 / 收藏。
  //
  // 必须在启动时拉一次，而不是等用户打开本地页：恢复出来的播放队列里可能就有本地
  // 曲目，`getSongIsLike` 读的是这个 store，没 hydrate 的话通知栏与播放条上的
  // 心形会一律显示未收藏——而那正是「本地收藏丢了」的错觉来源。三个命令都只读
  // 内存里的索引，代价可以忽略。
  if (isTauri()) {
    const local = localLibraryStore();
    void local
      .hydrate()
      .then(async () => {
        // A queued local row is a snapshot from when it was added, and the queue
        // outlives a re-scan. Re-reading the rows keeps a re-tagged file's title
        // right and — the visible half — keeps its cover pointing at a file that
        // still exists, since covers are content-addressed and the old one is
        // pruned when nothing references it.
        const refreshed = await local.refreshRows(music.persistData.playlists);
        if (refreshed !== music.persistData.playlists) {
          music.persistData.playlists = refreshed;
        }
      })
      .catch((err) => console.error("hydrate local library failed", err));
  }

  // 键盘监听
  window.addEventListener("keydown", spacePlayOrPause);

  // Tauri: handle main window close-requested event
  if (isTauri()) {
    window.__TAURI__?.event
      .listen("main-close-requested", () => {
        handleCloseRequested();
      })
      .then((unlisten) => {
        unlistenCloseRequested = unlisten;
      })
      .catch(() => {});

    // Suspend animations when main window is hidden (close-to-tray)
    windowManager
      .onMainWindowVisibility((visible) => {
        setPageVisible(visible);
      })
      .then((unlisten) => {
        unlistenMainVisibility = unlisten;
      })
      .catch(() => {});
  }
});

onBeforeUnmount(() => {
  inlineQueueMediaQuery?.removeEventListener("change", syncInlineQueueLayout);
  window.removeEventListener("keydown", spacePlayOrPause);
  if (glassRaf) cancelAnimationFrame(glassRaf);
  glassRaf = 0;
  unlistenCloseRequested?.();
  unlistenCloseRequested = null;
  unlistenMainVisibility?.();
  unlistenMainVisibility = null;
  document.documentElement.classList.remove("native-window-effect-root");
  document.documentElement.classList.remove("windows-native-effect-root");
});
</script>

<style lang="scss" scoped>
:global(html.native-window-effect-root),
:global(html.native-window-effect-root body),
:global(html.native-window-effect-root #app) {
  background: transparent !important;
}

// Option 1 (Windows only): native acrylic backdrop tint. DWM composes the acrylic
// behind the transparent window, and *it* provides the natural, smooth color
// transition — its luminosity blend of the blurred desktop. This layer only adds a
// light app tint on top; keep it translucent so the acrylic's transition still
// reads through, rather than a heavy flat wash (which flattens it and reads as
// cheap). No grain — real acrylic is smooth, not textured. It is window content, so
// it also renders in the taskbar thumbnail / Aero Peek preview. Tune the alphas:
// lighter = more natural, closer to the taskbar, but more see-through. macOS
// vibrancy is untouched.
:global(html.windows-native-effect-root[data-theme="dark"]) {
  background: rgba(18, 18, 22, 0.7) !important;
}
:global(html.windows-native-effect-root[data-theme="light"]) {
  background: rgba(251, 252, 254, 0.68) !important;
}

.main-content {
  transition:
    transform var(--duration-300) var(--ease-out),
    opacity var(--duration-300) var(--ease-out);

  .bigplayer-on {
    opacity: 0;
    transform: scale(0.9);
  }
}

.n-layout-content {
  top: 0;
  bottom: var(--layout-content-bottom);
  scroll-padding-top: var(--content-stage-padding-top);
  // Mirrors the clip-path's bottom inset so programmatic scrolls (scrollIntoView,
  // hash anchors) can't park a target in the clipped strip below the frame.
  scroll-padding-bottom: var(--content-stage-padding-y);
  clip-path: inset(
    var(--content-stage-padding-top) var(--content-stage-padding-right)
      var(--content-stage-padding-y) var(--content-stage-padding-x) round var(--radius-panel)
  );
  transition: all var(--duration-300) var(--ease-in-out);
  background-color: transparent !important;
  z-index: 1;

  // Only offset the scrollbar owned by this layout content. A descendant selector
  // also catches rails inside routed pages (for example Settings' virtual list),
  // shifting those rails left by the queue width when the inline queue is open.
  > :deep(.n-scrollbar > .n-scrollbar-rail--vertical) {
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
    // The bottom chrome (mini player + tab bar) is glass, so content has to pass
    // *under* it — a scroller that stops above the bars would leave the blur with
    // nothing but flat shell color to sample. Run the viewport to the window
    // bottom and give the inset back as padding, so the last row is still
    // reachable above the bars instead of being permanently parked behind them.
    bottom: 0;
    scroll-padding-bottom: var(--layout-content-bottom);

    .queue-column {
      display: none;
    }

    .content-stage,
    .content-stage.queue-open {
      padding: 0;
      padding-bottom: var(--layout-content-bottom);
    }

    .main {
      min-height: 100%;
      padding-top: calc(var(--app-mobile-nav-height) + 10px);

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

  &.native-window-effect {
    background: transparent;

    .app-layout-wrapper {
      background: transparent !important;
    }

    .app-layout {
      background: transparent !important;
    }

    :deep(.sidebar) {
      background: transparent;
    }

    :deep(.sidebar .sidebar-header),
    :deep(.sidebar .sidebar-footer) {
      background: transparent;
    }

    :deep(.sidebar .sidebar-header::after) {
      background: linear-gradient(
        to bottom,
        color-mix(in srgb, var(--n-text-color) 10%, transparent),
        transparent
      );
    }

    :deep(.sidebar .sidebar-footer::before) {
      background: linear-gradient(
        to top,
        color-mix(in srgb, var(--n-text-color) 10%, transparent),
        transparent
      );
    }

    :deep(.queue-panel) {
      background: transparent;
    }

    :deep(.player) {
      --player-surface-bg: transparent;
      --player-surface-border: transparent;
      background: transparent !important;
      border-top-color: transparent;
      box-shadow: none;
    }

    :deep(.player::before) {
      background: transparent;
      box-shadow: none;
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
  // Effectively the *bottom* gap: the top edge has its own
  // --content-stage-padding-top, and .content-stage re-overrides padding-top
  // after the shorthand uses this for both.
  --content-stage-padding-y: 0px;
  --content-stage-padding-top: var(--app-shell-top-gap);
  // Where a routed page's own `position: sticky` chrome may pin. The Nav floats
  // *over* the scrollport (position: fixed, z-index 1600) and the scrollport's top
  // is additionally clipped away by --content-stage-padding-top, so a page pinning
  // at 0 pins behind the Nav and outside the clip — the element simply disappears
  // as you scroll. This is the Nav's bottom edge (--app-floating-control-top plus
  // the 34px .nav height) plus 2px, i.e. the same 48px `.main`'s padding-top uses
  // to clear it. Pages cannot compute this themselves: --nav-control-height and the
  // Nav's own tokens are declared inside `.nav`.
  --content-sticky-top: calc(var(--app-floating-control-top) + 36px);
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

  // With the play bar hidden the frame had nothing holding it off the window
  // bottom, so it ran flush to the edge while the top kept its shell gap. Mirror
  // the top gap. When the play bar IS visible it already provides that inset via
  // --layout-content-bottom, so this stays 0 there.
  //
  // Desktop only: the mobile shell is edge-to-edge (padding-top and
  // padding-right both collapse to 0 under 768px) and always has the tab bar
  // below, so a gap there would be out of place.
  @media (min-width: 769px) {
    &:not(.player-visible) {
      --content-stage-padding-y: var(--app-shell-top-gap);
    }
  }

  &.queue-open {
    --content-stage-padding-right: 0px;
    --content-scrollbar-right: calc(var(--queue-column-width) + 2px);
    --player-right-inset: calc(var(--queue-column-width) + 8px);
  }

  @media (min-width: 1041px) and (max-width: 1180px) {
    --content-stage-padding-x: 0px;
    --content-stage-padding-right: 8px;
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
    --content-sticky-top: calc(var(--app-mobile-nav-height) + 4px);
    // 底部 chrome 的高度统一由 :root 的 --app-bottom-chrome* 提供
    // (global.scss)，其中 tab bar 高度已经含 safe-area-bottom，不要再加一次。
    --layout-content-bottom: var(--app-bottom-chrome);

    &.player-visible {
      --layout-content-bottom: var(--app-bottom-chrome-with-player);
    }
  }
}

.content-panel-frame {
  position: absolute;
  // The border is painted inward. Move the outer edge up by 1px so its inner
  // edge aligns with the content clip instead of leaving content above the frame.
  top: calc(var(--content-stage-padding-top) - 1px);
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
  }

  @media (max-width: 768px) {
    display: none;
  }
}

:global(html[data-theme="light"].native-window-effect-root) .content-panel-frame {
  --content-panel-bg: rgba(248, 248, 250, 0.46);
  --content-panel-gradient-overlay: rgba(255, 255, 255, 0.28);
}

:global(html[data-theme="dark"].native-window-effect-root) .content-panel-frame {
  --content-panel-bg: rgba(24, 24, 28, 0.52);
  --content-panel-gradient-overlay: rgba(24, 24, 28, 0.34);
}

// A custom property is only interpolatable if it is registered with a type —
// an unregistered one is a token string and would jump 0 -> 1 with no
// in-between. Registering <number> is what makes the blur radius itself
// animate, rather than cross-fading a sharp copy against a blurred one.
// Must be top-level: @property is not scoped and is ignored inside a selector.
@property --nav-glass-progress {
  syntax: "<number>";
  inherits: true;
  initial-value: 0;
}

// Mobile header glass, modelled on the iOS UIVisualEffectView backdrop.
//
// The system effect is not a blur — it is a filter chain on a CABackdropLayer:
// gaussianBlur -> colorSaturate (~1.8) -> colorBrightness -> luminanceCurveMap.
// The last two are what sell it: they compress the backdrop's dynamic range
// toward the material's own tone, so it reads as frosted glass instead of a
// blurry photograph. Blur alone always looks cheap, however well it is ramped.
//
// Each step below carries a *fifth* of the grade. Because a backdrop-filter
// samples everything already painted behind it — earlier siblings included — the
// grades compound multiplicatively down the stack, so each value is the 5th root
// of the target rather than the target itself. The payoff is that the color
// grading ramps out along the exact same masks as the blur, for free, instead of
// needing its own gradient. Do not "simplify" these into one strong grade on a
// single layer: that reintroduces a hard edge in color where the blur has none.
// --nav-glass-progress (0..1) scales every radius and the whole grade, so the
// material genuinely grows out of nothing instead of being cross-faded on top of
// the sharp page. It is a registered property (see @property above) purely so it
// can be interpolated; where that is unsupported it still substitutes fine and
// just switches discretely.
// $in/$out is the progress window over which this layer fades up. Layers are
// staggered, so the *effective* radius still ramps with progress even though each
// layer's own radius never changes.
//
// Fixed radius is the point. A changing blur radius invalidates the filter chain
// every frame, forcing all five gaussian passes to re-run — including after
// scrolling stops, when the backdrop is static and nothing else needs redoing.
// Opacity leaves the blurred result cacheable and composites it at varying alpha,
// which is why the fade no longer stutters. Cross-fading adjacent layers is
// imperceptible here because the radii differ by under a pixel.
@mixin nav-blur-step($radius, $solid, $mid, $fade, $in, $out) {
  -webkit-backdrop-filter: blur($radius) var(--nav-glass-grade);
  backdrop-filter: blur($radius) var(--nav-glass-grade);
  opacity: clamp(0, calc((var(--nav-glass-progress) - #{$in}) / #{$out - $in}), 1);
  // A three-stop mask; the middle stop biases the falloff toward ease-out so the
  // ramp does not read as a straight linear wipe.
  -webkit-mask-image: linear-gradient(
    to bottom,
    #000 $solid,
    rgba(0, 0, 0, 0.45) $mid,
    transparent $fade
  );
  mask-image: linear-gradient(to bottom, #000 $solid, rgba(0, 0, 0, 0.45) $mid, transparent $fade);
}

.content-top-shadow {
  // Per-step color grade. Compounds across the 5 steps — see the mixin comment.
  // Saturation is the iOS "material" tell; brightness/contrast stand in for
  // colorBrightness + luminanceCurveMap (CSS has no per-channel curve map).
  // Each value is the 5th root of the target, since all five steps compound:
  // 1.125^5 = 1.80 saturation, 1.012^5 = 1.06 brightness, 0.988^5 = 0.94 contrast.
  //
  // Static, NOT scaled by progress. Layer opacity already ramps the grade — a
  // half-faded layer contributes half its grade — and keeping the filter chain
  // constant is what lets the blurred result stay cached across the fade instead of
  // being recomputed every frame.
  --nav-glass-grade: saturate(112.5%) brightness(101.2%) contrast(98.8%);
  // Tint = the material's own tone. Kept low because the grade above is doing
  // most of the separation work; a heavy tint just reproduces the old gray mask.
  --nav-blur-tint-top: rgba(252, 252, 253, 0.44);
  --nav-blur-tint-mid: rgba(252, 252, 253, 0.13);

  display: none;
  position: absolute;
  top: calc(var(--content-stage-padding-top) - 1px);
  right: calc(var(--content-stage-padding-right) + 1px);
  left: calc(var(--content-stage-padding-x) + 1px);
  z-index: 2;
  height: 128px;
  overflow: hidden;
  pointer-events: none;
  border-radius: calc(var(--radius-panel) - 1px) calc(var(--radius-panel) - 1px) 0 0;
  // --nav-glass-progress (0..1) is written from JS each frame; see the scroll
  // section in the script. It scrubs against scroll position rather than playing as
  // a fixed-duration animation, so the material tracks the finger instead of firing
  // a 300ms cue on a threshold, and it is rate-limited there so a flick back to the
  // top still unwinds as a visible fade.
  //
  // Deliberately NOT a CSS transition on this variable: that would re-add lag on
  // every scroll, and cannot distinguish an ordinary scroll from a jump. Also
  // deliberately not the parent's opacity — an ancestor with opacity < 1 becomes a
  // backdrop root, isolating descendant backdrop-filters so they sample an empty
  // group and the blur vanishes. visibility only gates the passes at rest, and
  // flips at exactly zero, where there is nothing left to see.
  --nav-glass-progress: 0;
  visibility: hidden;
  transition: right var(--duration-300) var(--ease-in-out);

  &.scrolled {
    visibility: visible;
  }

  &.dark {
    // Dark materials pull the backdrop down AND compress it. Contrast must stay
    // below 100% here: iOS's luminanceCurveMap brings highlights down hard and
    // lifts blacks slightly, so bright album art can't punch through the bar.
    // Going above 100% (as a "darker = punchier" instinct suggests) crushes blacks
    // to pure black while leaving highlights hot — the range widens instead of
    // narrowing, and the glass stops reading as a surface.
    // Fifth roots: 0.944^5 = 0.75 brightness, 0.968^5 = 0.85 contrast.
    // Net effect: white backdrop lands at ~0.34 luma, black at ~0.06 — a 0.28
    // range, down from 0.43. That compression IS the frosted look.
    --nav-glass-grade: saturate(111%) brightness(94.4%) contrast(96.8%);
    // Slightly heavier + slightly warmer than a neutral gray: pure neutral over a
    // compressed backdrop reads as the old flat mask again.
    --nav-blur-tint-top: rgba(22, 22, 27, 0.56);
    --nav-blur-tint-mid: rgba(22, 22, 27, 0.16);
  }

  .app-layout.queue-open & {
    right: calc(var(--content-stage-padding-right) + var(--queue-column-width) + 1px);
  }

  // The blur layers only exist for the mobile header; on desktop the parent is
  // display:none, so they never paint. No opacity or per-step stagger here any
  // more: --nav-glass-progress scales the radii directly, so the blur genuinely
  // grows from nothing and unwinds the same way, with no sharp copy to ghost
  // against and nothing to sequence.
  .nav-blur-step,
  .nav-blur-tint {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    left: 0;
    pointer-events: none;
  }

  // iOS uses a true variable blur (private CAFilter "variableBlur"): one pass whose
  // radius is modulated per-pixel by a mask. CSS has no equivalent, so this
  // approximates it with five passes of rising radius, each masked to end higher
  // than the last. Radii compound in quadrature, so they are solved backwards from
  // the effective ramp we want — ~5px under the buttons stepping down through
  // 3.6 / 2.4 / 1.4 / 0.6 — rather than picked as round numbers. Nowhere in the
  // band does the radius hold constant, which is what removes the visible layer.
  //
  // --nav-blur-edge is the Nav's bottom edge. Stops are anchored to it in px
  // rather than percentages so the ramp keeps its position relative to the
  // buttons instead of drifting with notch height. The button row sits above
  // every step's solid threshold, so it gets the full effect; the taper happens
  // in the ~34px below it.
  .nav-blur-step {
    --nav-blur-edge: var(--app-mobile-nav-height);

    &:nth-child(1) {
      @include nav-blur-step(
        0.6px,
        calc(var(--nav-blur-edge) + 18px),
        calc(var(--nav-blur-edge) + 27px),
        calc(var(--nav-blur-edge) + 34px),
        0,
        0.28
      );
    }

    &:nth-child(2) {
      @include nav-blur-step(
        1.25px,
        calc(var(--nav-blur-edge) + 10px),
        calc(var(--nav-blur-edge) + 19px),
        calc(var(--nav-blur-edge) + 26px),
        0.1,
        0.42
      );
    }

    &:nth-child(3) {
      @include nav-blur-step(
        1.95px,
        calc(var(--nav-blur-edge) + 2px),
        calc(var(--nav-blur-edge) + 11px),
        calc(var(--nav-blur-edge) + 18px),
        0.22,
        0.58
      );
    }

    &:nth-child(4) {
      @include nav-blur-step(
        2.7px,
        calc(var(--nav-blur-edge) - 6px),
        calc(var(--nav-blur-edge) + 3px),
        calc(var(--nav-blur-edge) + 10px),
        0.36,
        0.76
      );
    }

    &:nth-child(5) {
      @include nav-blur-step(
        3.5px,
        calc(var(--nav-blur-edge) - 14px),
        calc(var(--nav-blur-edge) - 5px),
        calc(var(--nav-blur-edge) + 2px),
        0.52,
        1
      );
    }
  }

  // Tint fades out ahead of the blur ramp — a tint edge is far more visible than a
  // blur edge. No specular highlight here: this band is flush to the screen top,
  // and iOS only lights the leading edge of *inset* glass. A 1px line at y=0 would
  // read as a rendering artifact across the status bar. The Nav pills carry the
  // specular instead, since those are the inset elements.
  .nav-blur-tint {
    background: linear-gradient(
      to bottom,
      var(--nav-blur-tint-top) 0,
      var(--nav-blur-tint-mid) calc(var(--app-mobile-nav-height) - 2px),
      transparent calc(var(--app-mobile-nav-height) + 22px)
    );
    // Rides the same progress as the blur, but SQUARED. The tint is the most
    // visible layer, so a linear ramp makes it the thing you notice arriving and
    // leaving. Squaring keeps it near-invisible through the early part of the
    // range, so by the time the top edge is reached there is almost nothing left
    // to disappear — the blur leads, the color follows. Opacity is safe on this
    // element specifically: it carries no backdrop-filter, so it forms no backdrop
    // root for anything.
    opacity: calc(var(--nav-glass-progress) * var(--nav-glass-progress));
  }

  @media (max-width: 768px) {
    display: block;
    top: 0;
    right: 0;
    left: 0;
    height: calc(var(--app-mobile-nav-height) + 42px);
    border-radius: 0;
  }
}

// Mobile bottom glass — one shared surface for the mini player AND the tab bar. Both of
// those paint transparent on mobile, so the two bars read as a single material with no
// seam and no color step between them. Sits at z-index 1 (above the content layer, below
// .player's 2 and the tab bar's 1000), so the bars' own text and controls stay crisp on
// top of it.
//
// The top edge is a hard boundary, deliberately: the glass covers the bar rect and
// nothing above it. A gradient taper reaching up into the page blurs the bottom of the
// content itself, which reads as a smeared strip rather than as a surface.
.bottom-glass {
  display: none;
  position: fixed;
  right: 0;
  bottom: 0;
  left: 0;
  // Exactly the bars that are on screen right now — the tab bar alone, or the tab bar plus
  // the mini player. --app-tab-bar-height already carries the home-indicator inset.
  height: var(--app-bottom-chrome);
  z-index: 1;
  pointer-events: none;
  --bottom-glass-tint: rgba(250, 250, 252, 0.72);
  background-color: var(--bottom-glass-tint);
  // The grade matters more than the radius. iOS's material is
  // gaussianBlur -> colorSaturate -> colorBrightness -> luminanceCurveMap, and the last
  // two are what stop a bright backdrop from punching through: they COMPRESS the
  // backdrop's dynamic range toward the material's own tone. Hence contrast below 100%
  // — the instinctive "more contrast = punchier glass" widens the range instead and the
  // album art behind the bar starts competing with the track title in front of it.
  // Saturation is held near neutral for the same reason; pushing it (the usual
  // saturate(180%) glass recipe) amplifies exactly the cover colors we need to sit back.
  // 40px rather than 20px so the backdrop collapses into flat color instead of staying a
  // recognizable smeared image — shape is as distracting as color here.
  //
  // Prefixed FIRST, standard LAST — not cosmetic. The CSS minifier treats the two as
  // duplicate declarations of one property and keeps only the last, so writing the
  // standard one first ships a -webkit--only rule and the blur disappears entirely in
  // any engine without the alias. Every other backdrop-filter in this project is ordered
  // the same way for that reason.
  -webkit-backdrop-filter: blur(40px) saturate(112%) brightness(1.08) contrast(0.9);
  backdrop-filter: blur(40px) saturate(112%) brightness(1.08) contrast(0.9);
  // Dissolves with the mini bar's own surface when BigPlayer takes over, so the glass
  // does not outlive the bar it belongs to. Safe on this element: its own opacity
  // composites the already-filtered backdrop. It would NOT be safe on an ancestor —
  // opacity < 1 there makes a backdrop root and the blur silently samples an empty group.
  opacity: var(--mobile-mini-player-surface-opacity, 1);
  // Grown from the bottom edge rather than translated. The previous version kept one box
  // as tall as the *full* bar stack and pushed the mini player's share below the window
  // when there was no mini player — so 70 of its 126px hung outside the viewport. A
  // backdrop-filter box that extends past the viewport edge does not get a backdrop
  // snapshot for the part that is outside it, and Chromium (WebView2 and Android WebView
  // included) then paints the whole box as its flat tint with no blur at all. That is why
  // the tab bar stopped frosting the content behind it in exactly the state where the mini
  // player was hidden, while the identical glass looked right the moment a track started.
  // Sizing the box keeps every pixel of it on screen in both states.
  //
  // Height is not compositable, so this repaints the blur for the 300ms of the mini
  // player's own enter/leave — bounded, and cheaper at rest than permanently filtering a
  // box that was 55% off-screen.
  transition:
    height var(--duration-300) cubic-bezier(0.65, 0.05, 0.36, 1),
    background-color var(--duration-300) var(--ease-out);
  // Two jobs in one property. The inset hairline is the specular top edge every real
  // glass surface has, and it is what makes this read as a pane rather than as a washed
  // rectangle. It doubles as the readability fix the removed border used to provide: a
  // defined edge separates bar from content, where a blur gradient only muddled both.
  // The outer shadow is deliberately tight — enough to lift the bar, not enough to
  // register as a band of its own.
  box-shadow:
    inset 0 1px 0 rgb(255 255 255 / 55%),
    0 -1px 3px rgb(0 0 0 / 4%);

  &.has-player {
    height: var(--app-bottom-chrome-with-player);
  }

  &.dark {
    --bottom-glass-tint: rgba(16, 16, 20, 0.66);
    -webkit-backdrop-filter: blur(40px) saturate(108%) brightness(0.68) contrast(0.86);
    backdrop-filter: blur(40px) saturate(108%) brightness(0.68) contrast(0.86);
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 8%),
      0 -1px 3px rgb(0 0 0 / 18%);
  }
}

@media (max-width: 768px) {
  .bottom-glass {
    display: block;
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
  // Keep the native drag region in lockstep with the animated sidebar.
  // --sidebar-width changes immediately when the setting flips, while the
  // Sidebar itself eases to its new width over 220ms.
  transition:
    left 0.22s ease-in-out,
    right 0.22s ease-in-out;

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
  // The sidebar width custom property is discrete; animate dependent fixed
  // offsets so the nav does not jump ahead of the sidebar's width tween.
  transition:
    left 0.22s ease-in-out,
    right 0.22s ease-in-out;

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
