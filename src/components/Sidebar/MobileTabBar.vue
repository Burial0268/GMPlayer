<template>
  <div :class="['mobile-tab-bar', { dark: setting.getSiteTheme === 'dark' }]">
    <button
      v-for="tab in tabs"
      :key="tab.key"
      :class="['tab-item', { active: isActive(tab) }]"
      type="button"
      :aria-current="isActive(tab) ? 'page' : undefined"
      @click="navigation.switchRoot(tab.key, tab.to)"
    >
      <n-icon :size="22" :component="tab.icon" />
      <span class="tab-label">{{ tab.label }}</span>
    </button>
  </div>
</template>

<script setup>
import { NIcon } from "naive-ui";
import { HomeTwo, FindOne, Me, SettingTwo } from "@icon-park/vue-next";
import { settingStore, userStore } from "@/store";
import { useLayerNavigation } from "@/utils/navigation";
import { useI18n } from "vue-i18n";
import { isTauri } from "@/utils/tauri/core/runtime";

const { t } = useI18n();
const navigation = useLayerNavigation();
const setting = settingStore();
const user = userStore();

/**
 * The "library" tab's destination depends on being signed in.
 *
 * `/user` is behind a `needLogin` guard, so for a signed-out user it bounces to
 * `/login` with an error toast — which was, in practice, the *only* way to sign
 * in on mobile: the other two login entry points in the app are the sidebar
 * avatar (desktop-only) and a card on the home page. Sending the tab to
 * `/local` instead removes that bounce, so `views/Local/index.vue` carries a
 * login banner to replace it. The two changes belong together; splitting them
 * leaves a mobile build with no way to log in at all.
 */
const libraryTarget = computed(() => (user.userLogin || !isTauri() ? "/user" : "/local"));

const tabs = computed(() => [
  { key: "home", to: "/", icon: HomeTwo, label: t("sidebar.tab.home") },
  { key: "discover", to: "/discover", icon: FindOne, label: t("sidebar.tab.discover") },
  {
    key: "library",
    to: libraryTarget.value,
    icon: Me,
    label: t("sidebar.tab.library"),
    // Both prefixes, or the tab never highlights while signed out.
    matches: ["/user", "/local"],
  },
  {
    key: "settings",
    to: "/setting/appearance",
    icon: SettingTwo,
    label: t("sidebar.tab.settings"),
  },
]);

const isActive = (tab) => {
  return navigation.current.value?.root === tab.key;
};
</script>

<style lang="scss" scoped>
.mobile-tab-bar {
  display: none;
  position: fixed;
  bottom: 0;
  left: 0;
  right: 0;
  // --app-tab-bar-height is the 56px tab row PLUS --app-safe-area-bottom (the latter is
  // 0px off Tauri mobile). border-box is load-bearing here: this project has no universal
  // box-sizing reset, so under the default content-box the padding below was ADDED to the
  // height and the bar rendered one whole safe-area inset taller than every other
  // bottom-chrome consumer assumed — which is what offset the mini player sitting on it.
  box-sizing: border-box;
  height: var(--app-tab-bar-height);
  padding-bottom: var(--app-safe-area-bottom, 0px);
  // No fill and no top divider: this bar and the mini player above it share one glass
  // surface (.bottom-glass in App.vue). Painting a background here would put a second
  // material on top of it and bring back the color step between the two bars.
  background-color: transparent;
  z-index: var(--mobile-mini-player-bottom-z-index, 1000);
  justify-content: space-around;
  align-items: center;
  pointer-events: var(--mobile-mini-player-bottom-pointer-events, auto);
  transform: translate3d(0, var(--mobile-mini-player-bottom-y, 0%), 0);
  will-change: transform;

  @media (max-width: 768px) {
    display: flex;
  }
}

.tab-item {
  border: 0;
  padding: 0;
  font: inherit;
  background: transparent;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 2px;
  flex: 1;
  height: 100%;
  cursor: pointer;
  // Inactive labels are 10px, so they need the 4.5:1 floor against the glass rather than
  // the ~2.8:1 that #999 gave. The compressed backdrop sits lighter than the flat #fff
  // this used to sit on, which costs contrast instead of granting it.
  color: #6b6b73;
  transition: color var(--duration-300) var(--ease-out);

  .dark & {
    color: #adadb5;
  }

  &.active {
    color: var(--main-color);
  }

  &:active {
    transform: scale(0.92);
  }
}

.tab-label {
  font-size: 10px;
  line-height: 1;
}
</style>
