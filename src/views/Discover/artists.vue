<template>
  <div class="artists">
    <div class="menu">
      <n-space class="initial">
        <n-tag
          class="tag"
          round
          v-for="item in artistInitials"
          :key="item.key"
          :bordered="false"
          :type="item.key == artistInitialChoose ? 'primary' : 'default'"
          @click="artistInitialChange(item.key)"
        >
          {{ item.value }}
        </n-tag>
      </n-space>
      <n-space class="category">
        <n-tag
          class="tag"
          round
          :class="item.length > 2 ? 'hidden' : 'show'"
          v-for="(item, index) in artistTypeNames"
          :key="item"
          :bordered="false"
          :type="index == artistTypeNamesChoose ? 'primary' : 'default'"
          @click="artistTypeChange(index)"
        >
          {{ item }}
        </n-tag>
      </n-space>
    </div>
    <PageLoadState v-if="listFailed" error @retry="reloadArtists" />
    <ArtistLists v-else :listData="artistsData" :loadingNum="30" :loading="listLoading" />
    <n-space justify="center">
      <n-button
        v-if="hasMore && !listFailed && artistsData.length"
        class="more"
        size="large"
        strong
        secondary
        round
        :loading="loading"
        @click="loadingMore"
      >
        {{ $t("general.name.loadMore") }}
      </n-button>
    </n-space>
  </div>
</template>

<script setup lang="ts">
import { useRoute } from "vue-router";
import { useLayerNavigation } from "@/utils/navigation";
import { useI18n } from "vue-i18n";
import { getArtistList } from "@/api/artist";
import ArtistLists from "@/components/DataList/ArtistLists.vue";
import PageLoadState from "@/components/Navigation/PageLoadState.vue";
import { ArtistArea, ArtistType } from "@/api";

const { t } = useI18n();
const route = useRoute();
const navigation = useLayerNavigation();

// 歌手标签数据
const artistInitials = [
  { key: "-1", value: t("general.type.hot") },
  ...Array.from({ length: 26 }, (_, i) => String.fromCharCode(i + 65)).map((v) => ({
    key: v,
    value: v,
  })),
  { key: "0", value: "#" },
];
const artistInitialChoose = ref(route.query.initial ? route.query.initial : artistInitials[0].key);

// 歌手分类数据
const artistTypeNames = [
  t("general.type.all"),
  t("general.type.china"),
  t("general.type.chinaMale"),
  t("general.type.chinaFemale"),
  t("general.type.chinaGroup"),
  t("general.type.western"),
  t("general.type.westernMale"),
  t("general.type.westernFemale"),
  t("general.type.westernGroup"),
  t("general.type.japan"),
  t("general.type.japanMale"),
  t("general.type.japanFemale"),
  t("general.type.japanGroup"),
  t("general.type.korea"),
  t("general.type.koreaMale"),
  t("general.type.koreaFemale"),
  t("general.type.koreaGroup"),
  t("general.type.other"),
];
const artistType = [-1, -1, 1, 2, 3, -1, 1, 2, 3, -1, 1, 2, 3, -1, 1, 2, 3, -1];
const artistArea = [-1, 7, 7, 7, 7, 96, 96, 96, 96, 8, 8, 8, 8, 16, 16, 16, 16, 0];
const artistTypeNamesChoose = ref(route.query.type ? Number(route.query.type) : 0);

// 歌手数据
const artistsData = ref([]);
const artistsOffset = ref(0);
const hasMore = ref(true);
const loading = ref(false);
// 列表本身的加载态（骨架屏 / 空状态 / 错误）；`loading` 只管「加载更多」按钮。
const listLoading = ref(true);
const listFailed = ref(false);
// 只认最新一次请求：快速切分类时，迟到的旧结果不能追加进新分类的列表。
let requestSerial = 0;

// 获取歌手数据
const getArtistListData = (
  type = -1 as ArtistType,
  area = -1 as ArtistArea,
  limit = 30,
  offset = 0,
  initial = -1,
) => {
  const serial = ++requestSerial;
  const append = offset > 0;
  if (!append) {
    listLoading.value = true;
    listFailed.value = false;
  }
  getArtistList(type, area, limit, offset, initial)
    .then((res) => {
      if (serial !== requestSerial) return;
      const artists = res.artists ?? [];
      hasMore.value = Boolean(res.more) && artists.length > 0;
      artistsData.value.push(
        ...artists.map((v: any) => ({
          id: v.id,
          name: v.name,
          cover: v.img1v1Url,
          size: v.musicSize,
        })),
      );
    })
    .catch((err) => {
      if (serial !== requestSerial) return;
      console.error("[discover] artists failed", err);
      if (append) {
        // 退回这一页，下次「加载更多」重新要它，而不是跳过。
        if (artistsOffset.value === offset) artistsOffset.value -= limit;
        $message.error(t("general.message.acquisitionFailed"));
      } else {
        listFailed.value = true;
      }
    })
    .finally(() => {
      if (serial !== requestSerial) return;
      loading.value = false;
      listLoading.value = false;
    });
};

// 错误页的「重新载入」
const reloadArtists = () => {
  artistsOffset.value = 0;
  artistsData.value = [];
  getArtistListData(
    artistType[artistTypeNamesChoose.value] as ArtistType,
    artistArea[artistTypeNamesChoose.value] as ArtistArea,
    30,
    0,
    Number(artistInitialChoose.value),
  );
};

// 歌手标签变化
const artistInitialChange = (key) => {
  artistsData.value = [];
  navigation.replacePage({
    path: "/discover/artists",
    query: {
      type: artistTypeNamesChoose.value,
      initial: key,
      page: 1,
    },
  });
};

// 歌手分类变化
const artistTypeChange = (index) => {
  artistsData.value = [];
  navigation.replacePage({
    path: "/discover/artists",
    query: {
      type: index,
      initial: artistInitialChoose.value,
      page: 1,
    },
  });
};

// 加载更多
const loadingMore = () => {
  loading.value = true;
  artistsOffset.value += 30;
  getArtistListData(
    artistType[artistTypeNamesChoose.value] as ArtistType,
    artistArea[artistTypeNamesChoose.value] as ArtistArea,
    30,
    artistsOffset.value,
    Number(artistInitialChoose.value),
  );
};

// 监听路由参数变化
watch(
  () => route.fullPath,
  () => {
    const val = route;
    artistTypeNamesChoose.value = Number(val.query.type ? val.query.type : 0);
    artistInitialChoose.value = val.query.initial ? val.query.initial : artistInitials[0].key;
    artistsOffset.value = 0;
    if (val.name === "dsc-artists") {
      artistsData.value = [];
      getArtistListData(
        artistType[artistTypeNamesChoose.value] as ArtistType,
        artistArea[artistTypeNamesChoose.value] as ArtistArea,
        30,
        0,
        Number(artistInitialChoose.value),
      );
    }
  },
);

onMounted(() => {
  $setSiteTitle(t("nav.discover") + " - " + t("nav.discoverChildren.artists"));
  // 获取歌手数据
  getArtistListData(
    artistType[artistTypeNamesChoose.value] as ArtistType,
    artistArea[artistTypeNamesChoose.value] as ArtistArea,
    30,
    0,
    Number(artistInitialChoose.value),
  );
});
</script>

<style lang="scss" scoped>
.artists {
  .menu {
    margin-bottom: 16px;

    @media (max-width: 768px) {
      .initial {
        display: none !important;
      }
    }

    @media (max-width: 480px) {
      .category {
        gap: initial !important;

        .hidden {
          display: none !important;
        }

        .show {
          margin-right: 12px;
          margin-bottom: 8px;
        }
      }
    }

    .tag {
      font-size: 13px;
      padding: 0 16px;
      line-height: 0;
      cursor: pointer;
      transition: all var(--duration-300) var(--ease-out);

      @media (hover: hover) {
        &:hover {
          background-color: var(--main-second-color);
          color: var(--main-color);
        }
      }

      &:active {
        transform: scale(0.9);
      }
    }

    .category {
      margin-top: 18px;
    }
  }

  .artistlists {
    @media (max-width: 480px) {
      padding-top: 12px;
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
</style>
