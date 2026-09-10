import { createRouter, createWebHistory } from "vue-router";
import type { RouteLocationRaw } from "vue-router";
import routes from "./routes";
import { getLoginState } from "@/api/login";
import { userStore } from "@/store";
import { installLayerNavigation } from "@/utils/navigation";
import type { RootEntry } from "@/utils/navigation/layers";

declare module "vue-router" {
  interface RouteMeta {
    title?: string;
    needLogin?: boolean;
    hideLoadingBar?: boolean;
    navigationRoot?: RootEntry;
    navigationFallback?: RootEntry;
    navigationLabel?: string;
    navigationDetail?: "album" | "playlist" | "artist";
  }
}

const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes,
});

installLayerNavigation(router);

let routeLoadingBarActive = false;

// 路由守卫
router.beforeEach(async (to, from): Promise<RouteLocationRaw | void> => {
  if (to.fullPath === from.fullPath) return;
  const user = userStore();
  const showLoadingBar =
    !to.meta.hideLoadingBar &&
    (to.meta.needLogin || !window.matchMedia("(max-width: 768px)").matches);

  // 开始进度条
  routeLoadingBarActive = showLoadingBar;
  if (showLoadingBar && typeof $loadingBar !== "undefined") $loadingBar.start();

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
  if (routeLoadingBarActive && typeof $loadingBar !== "undefined") $loadingBar.finish();
  routeLoadingBarActive = false;
});

export default router;
