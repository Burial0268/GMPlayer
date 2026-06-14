import { createRouter, createWebHistory } from "vue-router";
import type { RouteLocationRaw } from "vue-router";
import routes from "./routes";
import { getLoginState } from "@/api/login";
import { userStore, musicStore } from "@/store";

declare module "vue-router" {
  interface RouteMeta {
    title?: string;
    needLogin?: boolean;
  }
}

const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes,
});

// 路由守卫
router.beforeEach(async (to): Promise<RouteLocationRaw | void> => {
  const user = userStore();
  const music = musicStore();

  // 关闭播放器
  music.setBigPlayerState(false);

  // 开始进度条
  if (typeof $loadingBar !== "undefined") $loadingBar.start();

  // 判断是否需要登录
  if (to.meta.needLogin) {
    try {
      const res = await getLoginState();
      if (res.data?.profile && user.userLogin) {
        user.setUserData(res.data.profile);
        if (!Object.keys(user.getUserOtherData).length) {
          user.setUserOtherData();
        }
        return;
      }

      $message.error(localStorage.getItem("cookie") ? "登录过期，请重新登录" : "请登录账号后使用");
      user.userLogOut();
      return "/login";
    } catch (err) {
      $message.error("请求发生错误");
      console.error("请求发生错误", err);
      return "/500";
    }
  }

  if (!Object.keys(user.getUserOtherData).length) user.setUserOtherData();
});

router.afterEach(() => {
  if (typeof $loadingBar !== "undefined") $loadingBar.finish();
});

export default router;
