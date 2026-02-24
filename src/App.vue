<template>
  <Provider>
    <div :class="['app-body', music.showBigPlayer ? 'bigplayer-open' : '']">
    <n-layout class="app-layout" style="height: 100vh">
      <n-layout-header bordered>
        <Nav />
      </n-layout-header>
      <n-layout-content position="absolute" :class="music.getPlaylists[0] && music.showPlayBar ? 'show' : ''"
        :native-scrollbar="false" embedded>
        <main ref="mainContent" class="main" id="mainContent" :class="{
          playlist: music.showPlayList,
          search: site.searchInputActive,
        }">
          <n-back-top :bottom="music.getPlaylists[0] && music.showPlayBar ? 100 : 40"
            style="transition: all 0.3s; z-index: 999" />
          <router-view v-slot="{ Component, route }">
            <transition
              name="fade-scale"
              mode="out-in"
            >
              <keep-alive :max="15">
                <component :is="Component" :key="(route.matched[0]?.path ?? route.path) + (route.query.id ? `_${route.query.id}` : '')" />
              </keep-alive>
            </transition>
          </router-view>
          <Player />
        </main>
      </n-layout-content>
    </n-layout>
    </div>
  </Provider>
</template>

<script setup lang="ts">
import { musicStore, userStore, settingStore, siteStore } from "@/store";
import { useRouter } from "vue-router";
import { getLoginState, refreshLogin } from "@/api/login";
import { userDailySignin, userYunbeiSign } from "@/api/user";
import { useI18n } from "vue-i18n";
import Provider from "@/components/Provider/index.vue";
import Nav from "@/components/Nav/index.vue";
import Player from "@/components/Player/index.vue";
import packageJson from "@/../package.json";
import { ref, watch } from 'vue';

const { t } = useI18n();
const music = musicStore();
const user = userStore();
const setting = settingStore();
const site = siteStore();
const router = useRouter();
const mainContent = ref(null);

// 公告数据
const annShow =
  import.meta.env.VITE_ANN_TITLE && import.meta.env.VITE_ANN_CONTENT
    ? true
    : false;
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
    : sessionStorage.getItem("siteTitle") ?? import.meta.env.VITE_SITE_TITLE;
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
  const lastRefreshDate = new Date(
    localStorage.getItem("lastRefreshDate")
  ).getTime();
  if (today - lastRefreshDate >= threeDays || !lastRefreshDate) {
    refreshLogin().then((res) => {
      if (res.code === 200) {
        localStorage.setItem(
          "lastRefreshDate",
          new Date(today).toLocaleDateString()
        );
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
  $message ? $message.success(t("other.cleanAll")) : alert(t("other.cleanAll"));
  localStorage.clear();
  document.location.reload();
};

// 滚动至顶部
const scrollToTop = () => {
  nextTick().then(() => {
    if (mainContent.value) {
      mainContent.value?.scrollIntoView({ behavior: "smooth" });
    } else {
      const mainContent = document.getElementById("mainContent");
      mainContent?.scrollIntoView({ behavior: "smooth" });
    }
  });
};

onMounted(() => {
  // 挂载方法至全局
  window.$scrollToTop = scrollToTop;
  window.$cleanAll = cleanAll;
  window.$signIn = signIn;
  window.$setSiteTitle = setSiteTitle;

  // 更改页面语言
  const html = document.documentElement;
  if (html) html.setAttribute("lang", setting.language);

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
    "font-size:16px"
  );
  console.info(
    "若站点出现异常，可尝试在下方输入 %c$cleanAll()%c 然后按回车来重置",
    "background: #eaeffd;color:#f55e55;padding: 4px 6px;border-radius:8px;",
    "background:unset;color:unset;"
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
});
</script>

<style lang="scss" scoped>
.titlebar {
  height: 30px;
  background: transparent;
  user-select: none;
  display: flex;
  justify-content: flex-end;
  position: fixed;
  top: 0;
  left: 3px;
  right: 0;
}

.titlebar-button {
  margin-top: -3px;
  transform: translateX(5%);
  display: inline-flex;
  justify-content: center;
  align-items: center;
  width: 30px;
  height: 20px;
}

.titlebar-button:hover {
  background: #f32156;
}

.n-layout-header {
  height: 60px;
  display: flex;
  flex-direction: row;
  justify-content: space-between;
  align-items: center;
}

.main-content {
  transition: transform .3s, opacity .3s;

  .bigplayer-on {
    opacity: 0;
    transform: scale(.9);
  }
}

.n-layout-content {
  top: 60px;
  transition: all 0.3s;

  &.show {
    bottom: 70px;
  }

  :deep(.n-scrollbar-rail--vertical) {
    right: 0;
  }

  .main {
    max-width: 1400px;
    margin: 0 auto;

    div:nth-of-type(2) {
      transition: all 0.3s;

      &::after {
        content: "";
        position: absolute;
        width: 100%;
        height: 100%;
        top: 0;
        left: 0;
        transition: all 0.3s;
        pointer-events: none;
        z-index: 2;
      }
    }

    &.playlist {
      div:nth-of-type(2) {
        transform: scale(0.98);
      }
    }

    &.search {
      div:nth-of-type(2) {
        &::after {
          pointer-events: all;
          background-color: #00000040;
        }
      }
    }
  }
}

// AMLL-style: .app-body is the outer wrapper (no transform → ::after position:fixed works)
// .app-layout is the inner container that scales down
.app-body {
  height: 100vh;
  overflow: hidden;
  background-color: #000;

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
    transition: opacity 0.5s cubic-bezier(0.25, 1, 0.5, 1);
    z-index: 1999;
  }

  &.bigplayer-open::after {
    opacity: 0.75;
  }
}

.app-layout {
  transition: scale 0.5s cubic-bezier(0.25, 1, 0.5, 1);
  scale: 1;
  transform-origin: center center;

  .bigplayer-open & {
    scale: 0.95;
    border-radius: 0.75em;
    overflow: hidden;
  }
}
</style>
