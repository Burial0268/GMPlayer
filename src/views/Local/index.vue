<template>
  <div class="local">
    <!--
      Not decoration — this is the mobile login entry point.

      `MobileTabBar` points its "library" tab here when signed out, which removes
      the `/user` → `needLogin` guard → `/login` bounce that used to be the only
      way to sign in on a phone. The other two entry points in the app are the
      sidebar avatar (desktop-only) and a card on the home page, so without this
      banner a signed-out mobile user has no route to a login screen at all.
    -->
    <div v-if="!user.userLogin" class="local-login-banner">
      <div class="text">
        <n-text class="title">{{ $t("local.loginTitle") }}</n-text>
        <n-text class="tip" :depth="3">{{ $t("local.loginTip") }}</n-text>
      </div>
      <n-button
        strong
        secondary
        round
        type="primary"
        @click="navigation.openPage('/login', { origin: $event })"
      >
        {{ $t("nav.avatar.login") }}
      </n-button>
    </div>

    <div class="title">
      <div class="icon-badge">
        <n-icon :size="34" :component="FolderMusic" />
      </div>
      <div class="text">
        <n-text class="key">{{ $t("sidebar.localMusic") }}</n-text>
        <n-text class="tip" :depth="3">
          {{ $t("local.summary", { sources: local.sources.length, tracks: local.trackCount }) }}
        </n-text>
      </div>
      <div class="actions">
        <!--
          下载目录本身就是本地库的一个来源，所以下载管理放在这里而不是只藏在设置里：
          歌下完了就出现在这一页，任务面板理应也在这一页够得着。
        -->
        <n-badge :value="download.activeCount" :max="99" :show="download.busy">
          <n-button strong secondary round @click="downloadRef?.open()">
            <template #icon>
              <n-icon :component="DownloadFour" />
            </template>
            {{ $t("download.manager") }}
          </n-button>
        </n-badge>
        <n-button strong secondary round @click="importRef?.open()">
          <template #icon>
            <n-icon :component="FolderOpen" />
          </template>
          {{ $t("local.manageSources") }}
        </n-button>
      </div>
    </div>

    <n-tabs class="main-tab" type="line" v-model:value="tabValue" @update:value="tabChange">
      <n-tab name="songs">{{ $t("local.tab.songs") }}</n-tab>
      <n-tab name="albums">{{ $t("local.tab.albums") }}</n-tab>
      <n-tab name="artists">{{ $t("local.tab.artists") }}</n-tab>
      <n-tab name="folders">{{ $t("local.tab.folders") }}</n-tab>
      <n-tab name="playlists">{{ $t("local.tab.playlists") }}</n-tab>
    </n-tabs>

    <main class="content">
      <router-view v-slot="{ Component }">
        <Transition :name="transitionName" mode="out-in">
          <keep-alive>
            <component :is="Component" />
          </keep-alive>
        </Transition>
      </router-view>
    </main>

    <ImportLocalMusic ref="importRef" />
    <DownloadManager ref="downloadRef" />
  </div>
</template>

<script setup lang="ts">
import { DownloadFour, FolderMusic, FolderOpen } from "@icon-park/vue-next";
import { useRoute } from "vue-router";
import { useLayerNavigation } from "@/utils/navigation";
import { useI18n } from "vue-i18n";
import { userStore, useDownloadStore, useLocalLibraryStore } from "@/store";
import { useTabTransition } from "@/composables/useTabTransition";
import ImportLocalMusic from "@/components/DataModal/ImportLocalMusic.vue";
import DownloadManager from "@/components/DataModal/DownloadManager.vue";

const { t } = useI18n();
const route = useRoute();
const navigation = useLayerNavigation();
const user = userStore();
const local = useLocalLibraryStore();
const download = useDownloadStore();
const importRef = ref<InstanceType<typeof ImportLocalMusic> | null>(null);
const downloadRef = ref<InstanceType<typeof DownloadManager> | null>(null);

const TABS = ["songs", "albums", "artists", "folders", "playlists"];
const { transitionName, updateDirection, syncIndex } = useTabTransition(TABS);

const tabValue = ref(route.path.split("/")[2] || "songs");
syncIndex(tabValue.value);

const tabChange = (value: string) => {
  updateDirection(value);
  navigation.replacePage({ path: `/local/${value}` });
};

watch(
  () => route.path,
  (path) => {
    // Only react to the tab routes; the detail view lives at `/local/playlist`
    // and would otherwise select a tab that does not exist.
    const segment = path.split("/")[2];
    if (!TABS.includes(segment)) return;
    tabValue.value = segment;
    syncIndex(segment);
  },
);

onMounted(async () => {
  $setSiteTitle(t("sidebar.localMusic"));
  // Cheap and idempotent; the library itself is paged on demand.
  await local.hydrate();
  if (!local.albums.length && local.trackCount > 0) await local.loadGroups();
  // Subscribes to the queue's event channel, so the badge follows a batch that
  // was started somewhere else — or before this page existed.
  void download.hydrate();
});
</script>

<style lang="scss" scoped>
.local {
  .local-login-banner {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-top: 24px;
    padding: 16px 20px;
    border-radius: 12px;
    background-color: rgba(var(--main-color), 0.08);
    .text {
      display: flex;
      flex-direction: column;
      gap: 2px;
      min-width: 0;
      .title {
        font-size: 16px;
        font-weight: bold;
      }
      .tip {
        font-size: 13px;
      }
    }
    button {
      margin-left: auto;
      flex-shrink: 0;
    }
    @media (max-width: 620px) {
      flex-direction: column;
      align-items: flex-start;
      button {
        margin-left: 0;
        width: 100%;
      }
    }
  }

  .title {
    margin-top: 30px;
    margin-bottom: 20px;
    display: flex;
    align-items: center;
    .icon-badge {
      display: flex;
      align-items: center;
      justify-content: center;
      width: 80px;
      height: 80px;
      min-width: 80px;
      margin-right: 16px;
      border-radius: 12px;
      background-color: rgba(var(--main-color), 0.14);
      color: rgb(var(--main-color));
      box-shadow: 0 6px 8px -2px rgb(0 0 0 / 16%);
    }
    .text {
      display: flex;
      flex-direction: column;
      min-width: 0;
      .key {
        font-size: 34px;
        font-weight: bold;
      }
      .tip {
        font-size: 15px;
      }
    }
    .actions {
      margin-left: auto;
      flex-shrink: 0;
      align-self: center;
      display: flex;
      align-items: center;
      gap: 10px;
    }
    @media (max-width: 620px) {
      .icon-badge {
        width: 56px;
        height: 56px;
        min-width: 56px;
      }
      .text .key {
        font-size: 26px;
      }
      // 手机上两颗按钮并排会把标题挤没，所以整块挪到下一行并铺满宽度。
      flex-wrap: wrap;
      .actions {
        margin-left: 0;
        width: 100%;
        margin-top: 12px;
        > * {
          flex: 1;
        }
        :deep(.n-button) {
          width: 100%;
        }
      }
    }
  }

  // 用 `overflow: clip` 而**不是** `hidden`。两者裁剪效果一样（这里只是要把 slide 转场
  // 那 ±20px 的横向位移收住），但 `hidden` 会让这个盒子成为**滚动容器**——于是它就是
  // `songs.vue` 里 `.list-toolbar` 那条 `position: sticky` 解析所依据的 scrollport。
  //
  // 这不是「sticky 不生效」而已：sticky 是「把盒子平移到约束矩形内的最小位移」，而约束
  // 矩形是 scrollport 按 inset 内缩后的框。工具栏的自然位置就在 `.content` 顶端，于是
  // 「距 scrollport 顶端至少 48px」这条要求把它**往下推**了整整一个
  // `--content-sticky-top`（桌面 48px，移动端 46px + 安全区），正好压在
  // `.song-list-head` 的「专辑 / 时长」和头几行上；而这个容器 `scrollTop` 恒为 0，
  // 所以滚动时也永远钉不住。上方那条空带就是它让出来的原位。
  //
  // `clip` 按规范不建立滚动容器，sticky 因此回到外层 `.n-scrollbar-container`（App 那
  // 一个）上解析，`--content-sticky-top` 才对得上它的本意：钉在 Nav 下沿。顺带也没有
  // 了「聚焦输入框把这个容器内部滚一段、整页内容跟着偏移」这种隐蔽故障。
  //
  // 同样的坑对 `/artist`、`/user`、`/discover`、`/search`、`/profile` 的标签页壳子都成立
  // ——它们眼下没有 sticky 构件，所以没有症状，但要往里放浮动控件时先改这里。
  .content {
    position: relative;
    overflow: clip;
    margin-top: 20px;
  }
}
</style>
