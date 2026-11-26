<template>
  <div class="playlists">
    <div class="menu">
      <!-- 分类选择 -->
      <n-button
        strong
        secondary
        class="cat"
        icon-placement="right"
        round
        @click="catModalShow = true"
      >
        <template #icon>
          <n-icon class="up" :component="ChevronRightRound" />
        </template>
        {{ catName }}
      </n-button>
      <n-modal
        class="s-modal"
        v-model:show="catModalShow"
        preset="card"
        :title="$t('general.name.playlistType')"
        :bordered="false"
      >
        <template #header>
          {{ $t("general.name.playlistType") }}
          <n-tag
            round
            class="tag"
            :type="catName == '全部歌单' ? 'primary' : 'default'"
            :style="{
              marginLeft: '12px',
              fontSize: '12px',
              transform: 'translateY(-2px)',
              cursor: 'pointer',
            }"
            :bordered="false"
            @click="changeTagName('全部歌单')"
          >
            {{ $t("general.name.allPLaylist") }}
          </n-tag>
        </template>
        <n-scrollbar>
          <div class="all-cat" v-if="music.catList?.sub[0]">
            <n-list>
              <n-list-item v-for="(cat, key) in music.catList.categories" :key="cat">
                <template #prefix>
                  <n-text class="type"> {{ cat }} </n-text>
                </template>
                <n-space>
                  <n-tag
                    round
                    class="tag"
                    size="large"
                    v-for="item in music.catList.sub.filter((v) => v.category === key)"
                    :key="item"
                    :bordered="false"
                    :type="item.name == catName ? 'primary' : 'default'"
                    @click="changeTagName(item.name)"
                  >
                    {{ item.name }}
                    <template #icon>
                      <n-icon class="icon" v-if="item.hot" :component="LocalFireDepartmentRound" />
                    </template>
                  </n-tag>
                </n-space>
              </n-list-item>
            </n-list>
          </div>
          <div class="error" v-else>
            {{ $t("general.message.acquisitionFailed") }}
          </div>
        </n-scrollbar>
      </n-modal>
      <!-- 精品歌单开关 -->
      <n-space
        v-if="getHaveHqPlaylists(music.highqualityCatList, catName.toString())"
        align="center"
      >
        <n-text>{{ $t("general.name.bestPlaylist") }}</n-text>
        <n-switch v-model:value="hqPLayListOpen" @update:value="hqPLayListChange" :round="false" />
      </n-space>
    </div>
    <PageLoadState v-if="listFailed" error @retry="reloadList" />
    <CoverLists v-else :listData="playlistsData" :loading="listLoading" />
    <Pagination
      v-if="totalCount && !hqPLayListOpen && !listFailed"
      :totalCount="totalCount"
      :pageNumber="pageNumber"
      @pageSizeChange="pageSizeChange"
      @pageNumberChange="pageNumberChange"
    />
    <n-space justify="center">
      <n-button
        v-if="hqPLayListOpen && hasMore && !listFailed && playlistsData.length"
        class="more"
        size="large"
        strong
        secondary
        round
        :loading="loading"
        @click="
          () => {
            loading = true;
            getHqPlaylistData(catName.toString());
          }
        "
      >
        {{ $t("general.name.loadMore") }}
      </n-button>
    </n-space>
  </div>
</template>

<script setup lang="ts">
import { ChevronRightRound, LocalFireDepartmentRound } from "@vicons/material";
import { useRoute } from "vue-router";
import { useLayerNavigation } from "@/utils/navigation";
import { musicStore } from "@/store";
import { getHighqualityPlaylist, getTopPlaylist } from "@/api/playlist";
import { formatNumber } from "@/utils/timeTools";
import { useI18n } from "vue-i18n";
import CoverLists from "@/components/DataList/CoverLists.vue";
import Pagination from "@/components/Pagination/index.vue";
import PageLoadState from "@/components/Navigation/PageLoadState.vue";

const { t } = useI18n();
const route = useRoute();
const navigation = useLayerNavigation();
const music = musicStore();

// 分类数据
const catModalShow = ref(false);
const catName = ref(route.query.cat ? route.query.cat : "全部歌单");

// 歌单数据
const playlistsData = ref([]);
const totalCount = ref(0);
const pagelimit = ref(30);
const pageNumber = ref(route.query.page ? Number(route.query.page) : 1);

// 精品歌单数据
const hqPLayListOpen = ref(route.query.hq ? (route.query.hq === "true" ? true : false) : false);
const hasMore = ref(true);
const loading = ref(false);
// 列表本身的加载态（骨架屏 / 空状态 / 错误）；`loading` 只管「加载更多」按钮。
const listLoading = ref(true);
const listFailed = ref(false);
// 只认最新一次请求：快速切分类时，迟到的旧结果不能写进新分类的列表。
let requestSerial = 0;

// 获取是否有精品歌单
const getHaveHqPlaylists = (array: any[], name: string) => {
  if (name === "全部歌单") {
    return true;
  } else {
    return array.some((item: { name: any }) => {
      return item.name === name;
    });
  }
};

// 精品歌单状态变化
const hqPLayListChange = (val: any) => {
  playlistsData.value = [];
  navigation.replacePage({
    path: "/discover/playlists",
    query: {
      cat: catName.value,
      hq: val ? "true" : "false",
      page: 1,
    },
  });
};

// 获取歌单数据（换页 / 换分类都先清空，期间显示骨架屏，不留旧页）
const getPlaylistData = (cat = "全部歌单", limit = 30, offset = 0) => {
  const serial = ++requestSerial;
  playlistsData.value = [];
  listLoading.value = true;
  listFailed.value = false;
  getTopPlaylist(cat, limit, offset)
    .then((res) => {
      if (serial !== requestSerial) return;
      totalCount.value = res.total;
      playlistsData.value = (res.playlists ?? []).map((v: any) => ({
        id: v.id,
        cover: v.coverImgUrl,
        name: v.name,
        artist: v.creator,
        playCount: formatNumber(v.playCount),
      }));
    })
    .catch((err) => {
      if (serial !== requestSerial) return;
      console.error("[discover] playlists failed", err);
      listFailed.value = true;
    })
    .finally(() => {
      if (serial === requestSerial) listLoading.value = false;
    });
};

// 获取精品歌单数据（按 before 追加）
const getHqPlaylistData = (cat = "全部歌单", limit = 30) => {
  const serial = ++requestSerial;
  const append = playlistsData.value.length > 0;
  const before = append ? playlistsData.value[playlistsData.value.length - 1].updateTime : null;
  if (!append) {
    listLoading.value = true;
    listFailed.value = false;
  }
  getHighqualityPlaylist(cat, limit, before)
    .then((res) => {
      if (serial !== requestSerial) return;
      const playlists = res.playlists ?? [];
      hasMore.value = Boolean(res.more) && playlists.length > 0;
      playlistsData.value.push(
        ...playlists.map((v: any) => ({
          id: v.id,
          cover: v.coverImgUrl,
          name: v.name,
          artist: v.creator,
          playCount: formatNumber(v.playCount),
          updateTime: v.updateTime,
        })),
      );
    })
    .catch((err) => {
      if (serial !== requestSerial) return;
      console.error("[discover] high-quality playlists failed", err);
      // 首屏失败给错误页；加载更多失败只提示，已有内容留着。
      if (append) $message.error(t("general.message.acquisitionFailed"));
      else listFailed.value = true;
    })
    .finally(() => {
      if (serial !== requestSerial) return;
      loading.value = false;
      listLoading.value = false;
    });
};

// 错误页的「重新载入」
const reloadList = () => {
  if (hqPLayListOpen.value) {
    playlistsData.value = [];
    getHqPlaylistData(catName.value.toString());
  } else {
    getPlaylistData(
      catName.value.toString(),
      pagelimit.value,
      (pageNumber.value - 1) * pagelimit.value,
    );
  }
};

// 更换标签名
const changeTagName = (name: any) => {
  playlistsData.value = [];
  navigation.replacePage({
    path: "/discover/playlists",
    query: {
      cat: name,
      page: 1,
    },
  });
  catModalShow.value = false;
};

// 每页个数数据变化
const pageSizeChange = (val: number) => {
  pagelimit.value = val;
  getPlaylistData(catName.value.toString(), val, (pageNumber.value - 1) * pagelimit.value);
};

// 当前页数数据变化
const pageNumberChange = (val: number) => {
  navigation.replacePage({
    path: "/discover/playlists",
    query: {
      cat: catName.value,
      page: val,
    },
  });
};

// 监听路由参数变化
watch(
  () => route.fullPath,
  () => {
    const val = route;
    if (val.name === "dsc-playlists") {
      catName.value = val.query.cat ? val.query.cat : "全部歌单";
      hqPLayListOpen.value = val.query.hq ? (val.query.hq === "true" ? true : false) : false;
      if (hqPLayListOpen.value) {
        playlistsData.value = [];
        getHqPlaylistData(catName.value.toString());
      } else {
        pageNumber.value = val.query.page ? Number(val.query.page) : 1;
        getPlaylistData(
          catName.value.toString(),
          pagelimit.value,
          (pageNumber.value - 1) * pagelimit.value,
        );
      }
    }
  },
);

onMounted(() => {
  $setSiteTitle(t("nav.discover") + " - " + t("general.name.playlist"));
  // 获取歌单分类
  if (!music.catList.sub || !music.highqualityCatList[0]) music.setCatList(true);
  // 获取歌单数据
  if (hqPLayListOpen.value) {
    getHqPlaylistData(catName.value.toString());
  } else {
    getPlaylistData(
      catName.value.toString(),
      pagelimit.value,
      (pageNumber.value - 1) * pagelimit.value,
    );
  }
});
</script>

<style lang="scss" scoped>
.playlists {
  .menu {
    display: flex;
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 20px;
    .order {
      width: 80px;
    }
  }
  .more {
    margin-top: 40px;
    width: 140px;
    font-size: 16px;
    transition: all var(--duration-300) var(--ease-out);
    @media (hover: hover) {
      &:hover {
        background-color: var(--main-second-color);
        color: var(--main-color);
      }
    }
    &:active {
      transform: scale(0.95);
    }
    :deep(.n-button__icon) {
      margin-right: 12px;
    }
  }
}
.all-cat {
  .type {
    white-space: nowrap;
    &::after {
      content: ">";
      margin-left: 6px;
      opacity: 0.6;
    }
  }
}
.tag {
  cursor: pointer;
  transition: all var(--duration-300) var(--ease-out);
  @media (hover: hover) {
    &:hover {
      background-color: var(--main-second-color);
      color: var(--main-color);
    }
  }
  &:active {
    transform: scale(0.95);
  }
  .icon {
    color: var(--main-color);
    font-size: 16px;
    margin-left: 4px;
    transform: translateY(-1px);
  }
}
</style>
