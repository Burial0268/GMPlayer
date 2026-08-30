<template>
  <div :class="['album', { 'is-dark': setting.getSiteTheme === 'dark' }]" v-if="albumDetail">
    <div class="left">
      <div class="cover">
        <n-image
          show-toolbar-tooltip
          class="coverImg"
          :src="getCoverUrl(albumDetail.picUrl, 1024)"
          :previewed-img-props="{ style: { borderRadius: 'var(--radius-md)' } }"
          :preview-src="getCoverUrl(albumDetail.picUrl)"
          fallback-src="/images/pic/default.png"
        />
        <img src="/images/pic/album.png" class="album-disc" alt="album" />
      </div>
      <div class="meta">
        <div class="title">
          <span class="detail-kind">{{ $t("general.name.album") }}</span>
          <n-text class="name">{{ albumDetail.name }}</n-text>
          <n-text class="creator" @click="router.push(`/artist/songs?id=${albumDetail.artist.id}`)">
            {{ albumDetail.artist.name }}
          </n-text>
        </div>
        <div class="detail-stats">
          <div class="num">
            <n-icon :depth="3" :component="Time" />
            <n-text v-html="getLongTime(albumDetail.publishTime)" />
          </div>
          <div class="num" v-if="albumDetail.company">
            <n-icon :depth="3" :component="City" />
            <n-text v-html="albumDetail.company" />
          </div>
          <div class="num" v-if="albumData.length">
            <n-icon :depth="3" :component="MusicList" />
            <n-text>{{ $t("general.name.songSize", { size: albumData.length }) }}</n-text>
          </div>
        </div>
        <div class="intr">
          <span class="name">{{
            $t("general.name.desc", { name: $t("general.name.album") })
          }}</span>
          <span class="desc text-hidden">
            {{ albumDetail.description ? albumDetail.description : $t("other.noDesc") }}
          </span>
          <n-button
            class="all-desc"
            block
            strong
            secondary
            v-if="albumDetail?.description?.length > 70"
            @click="albumDescShow = true"
          >
            {{ $t("general.name.allDesc") }}
          </n-button>
        </div>
        <n-space class="tag" v-if="albumDetail.tags">
          <n-tag class="tags" round :bordered="false" v-for="item in albumDetail.tags" :key="item">
            {{ item }}
          </n-tag>
        </n-space>
        <n-space class="control">
          <n-button strong secondary round type="primary" @click="playAllSong">
            <template #icon>
              <n-icon :component="MusicList" />
            </template>
            {{ $t("general.name.play") }}
          </n-button>
          <n-dropdown
            placement="right-start"
            trigger="click"
            :show-arrow="true"
            :options="dropdownOptions"
          >
            <n-button strong secondary circle>
              <template #icon>
                <n-icon :component="More" />
              </template>
            </n-button>
          </n-dropdown>
        </n-space>
      </div>
    </div>
    <div class="right">
      <div class="meta">
        <n-text class="name">{{ albumDetail.name }}</n-text>
        <n-text class="creator" @click="router.push(`/artist/songs?id=${albumDetail.artist.id}`)">
          <n-icon :depth="3" :component="People" />
          {{ albumDetail.artist.name }}
        </n-text>
        <n-space class="time">
          <div class="num">
            <n-icon :depth="3" :component="Time" />
            <n-text v-html="getLongTime(albumDetail.publishTime)" />
          </div>
          <div class="num" v-if="albumDetail.company">
            <n-icon :depth="3" :component="City" />
            <n-text v-html="albumDetail.company" />
          </div>
        </n-space>
      </div>
      <div class="list-toolbar">
        <n-input
          class="list-search"
          :class="{ 'has-value': !!searchKeyword }"
          v-model:value="searchKeyword"
          clearable
          size="small"
          :placeholder="t('general.name.filterInList')"
        >
          <template #prefix>
            <n-icon :component="Filter" />
          </template>
        </n-input>
      </div>
      <DataLists
        :listData="displayData"
        hideAlbum
        page-window
        :virtual-item-size="54"
        :virtual-threshold="40"
        show-header
        :loading="false"
      />
      <!-- 专辑简介 -->
      <n-modal
        class="s-modal"
        v-model:show="albumDescShow"
        preset="card"
        :title="$t('general.name.desc', { name: $t('general.name.album') })"
        :bordered="false"
      >
        <n-scrollbar>
          <n-text v-html="albumDetail.description.replace(/\n/g, '<br>')" />
        </n-scrollbar>
      </n-modal>
    </div>
  </div>
  <div class="title" v-else-if="!albumId">
    <span class="key">{{ $t("general.name.noKeywords") }}</span>
    <br />
    <n-button strong secondary @click="router.go(-1)" style="margin-top: 20px">
      {{ $t("general.name.goBack") }}
    </n-button>
  </div>
  <div class="loading" v-else>
    <div class="left">
      <div class="cover">
        <n-skeleton class="pic" />
        <n-skeleton class="album-disc" />
      </div>
      <div class="meta loading-meta">
        <n-skeleton text width="64px" />
        <n-skeleton class="loading-title" text width="min(560px, 100%)" />
        <n-skeleton text width="160px" />
        <div class="loading-stats">
          <n-skeleton text width="116px" />
          <n-skeleton text width="136px" />
          <n-skeleton text width="82px" />
        </div>
        <n-skeleton text :repeat="2" width="min(640px, 100%)" />
        <div class="loading-actions">
          <n-skeleton :sharp="false" width="112px" height="34px" />
          <n-skeleton :sharp="false" width="34px" height="34px" />
        </div>
      </div>
    </div>
    <div class="right loading-list">
      <div v-for="item in 8" :key="item" class="loading-row">
        <n-skeleton circle width="38px" height="38px" />
        <div class="loading-row-main">
          <n-skeleton text width="min(360px, 70%)" />
          <n-skeleton text width="min(220px, 44%)" />
        </div>
        <n-skeleton text width="84px" />
        <n-skeleton text width="46px" />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { NIcon, NText } from "naive-ui";
import { getAlbum, likeAlbum } from "@/api/album";
import { useRouter } from "vue-router";
import { getLongTime } from "@/utils/timeTools";
import { transformSongData } from "@/utils/ncm/transformSongData";
import { fuzzyFilterSongs } from "@/utils/fuzzySearch";
import { renderIcon } from "@/utils/ui/renderIcon";
import { buildLikeMessage } from "@/utils/ui/buildLikeMessage";
import { usePlayAllSong } from "@/composables/usePlayAllSong";
import { useContentPanelAccent } from "@/composables/useContentPanelAccent";
import {
  MusicList,
  LinkTwo,
  More,
  Like,
  Unlike,
  People,
  Time,
  City,
  Filter,
} from "@icon-park/vue-next";
import { userStore, musicStore, settingStore } from "@/store";
import { useI18n } from "vue-i18n";
import DataLists from "@/components/DataList/DataLists.vue";
import getCoverUrl from "@/utils/ncm/getCoverUrl";

const { t } = useI18n();
const router = useRouter();
const user = userStore();
const music = musicStore();
const setting = settingStore();
const { playAllSong: playAll } = usePlayAllSong();
const { applyContentPanelAccent } = useContentPanelAccent();

// 专辑数据
const albumId = ref(router.currentRoute.value.query.id);
const albumDetail = ref(null);
const albumData = ref([]);
const albumDescShow = ref(false);

// ── 列表内搜索 ──────────────────────────────────────────────
//
// 专辑一次就把全部曲目取回来了（`getAlbum` 无分页），所以这里是纯本地过滤，不需要
// 像歌单那样补块。
const searchKeyword = ref("");
const normalizedKeyword = computed(() => searchKeyword.value.trim());

const displayData = computed(() =>
  normalizedKeyword.value
    ? fuzzyFilterSongs(albumData.value, normalizedKeyword.value)
    : albumData.value,
);

watch(normalizedKeyword, (keyword, prev) => {
  // 同 PlayList：过滤结果是按相关度重排的新列表，停在原滚动位置没有意义。
  if (keyword !== prev && typeof $scrollToTop !== "undefined") $scrollToTop();
});

// 判断收藏还是取消
const isLikeOrDislike = (id) => {
  return !user.getUserAlbumIds.has(Number(id));
};

// 专辑下拉菜单数据
const dropdownOptions = ref([]);

// 更改专辑下拉菜单数据
const setDropdownOptions = () => {
  dropdownOptions.value = [
    {
      key: "copy",
      label: t("menu.copy", {
        name: t("general.name.album"),
        other: t("general.name.link"),
      }),
      props: {
        onClick: () => {
          if (navigator.clipboard) {
            try {
              navigator.clipboard.writeText(`https://music.163.com/#/album?id=${albumId.value}`);
              $message.success(t("general.message.copySuccess"));
            } catch (err) {
              console.error(t("general.message.copyFailure"), err);
              $message.error(t("general.message.copyFailure"));
            }
          } else {
            $message.error(t("general.message.notSupported"));
          }
        },
      },
      icon: renderIcon(h(LinkTwo)),
    },
    {
      key: "like",
      label: isLikeOrDislike(albumId.value)
        ? t("menu.collection", { name: t("general.name.album") })
        : t("menu.cancelCollection", { name: t("general.name.album") }),
      show: user.userLogin,
      props: {
        onClick: () => {
          toChangeLike(albumId.value);
        },
      },
      icon: renderIcon(h(isLikeOrDislike(albumId.value) ? Like : Unlike)),
    },
  ];
};

// 获取歌单信息
const getAlbumData = (id) => {
  getAlbum(id).then((res) => {
    // 专辑信息
    albumDetail.value = res.album;
    const albumCover = res.album.picUrl;
    applyContentPanelAccent(getCoverUrl(albumCover, 256));
    window.$setSiteTitle(res.album.name + " - " + t("general.name.album"));
    // 专辑歌曲
    if (res.songs) {
      albumData.value = transformSongData(res.songs, {
        sourceId: id,
        albumTransform: (v) => {
          v.al.picUrl = albumCover;
          return v.al;
        },
      });
    } else {
      window.$message.error(t("general.message.acquisitionFailed"));
    }
  });
};

// 播放专辑所有歌曲
const playAllSong = () => {
  playAll(albumData.value);
};

// 收藏/取消收藏
const toChangeLike = async (id) => {
  const type = isLikeOrDislike(id) ? 1 : 2;
  const likeMsg = t("general.name.album");
  try {
    const res = await likeAlbum(type, id);
    if (res.code === 200) {
      $message.success(buildLikeMessage(t, likeMsg, type, "success", setting.language));
      user.setUserAlbumLists(() => {
        setDropdownOptions();
      });
    } else {
      $message.error(buildLikeMessage(t, likeMsg, type, "failed", setting.language));
    }
  } catch (err) {
    console.error(buildLikeMessage(t, likeMsg, type, "failed", setting.language), err);
    $message.error(buildLikeMessage(t, likeMsg, type, "failed", setting.language));
  }
};

onMounted(() => {
  if (albumId.value) {
    getAlbumData(albumId.value);
    if (user.userLogin && !user.getUserAlbumLists.has && !user.getUserAlbumLists.isLoading) {
      user.setUserAlbumLists(() => {
        setDropdownOptions();
      });
    } else {
      setDropdownOptions();
    }
  }
});

// 监听路由参数变化
watch(
  () => router.currentRoute.value,
  (val) => {
    albumId.value = val.query.id;
    if (val.name === "album") {
      getAlbumData(albumId.value);
    }
  },
);
</script>

<style lang="scss" scoped>
.album,
.loading {
  // 悬浮搜索控件的玻璃参数。值抄自 Nav 的悬浮按钮，但 `--floating-control-bg` 是
  // 定义在 `.nav` 内部的、拿不到，所以这里重新声明一份同名不同前缀的。
  // 暗色钩子和 Nav 一致：`setting.getSiteTheme === "dark"`。
  --list-search-bg: rgba(255, 255, 255, 0.48);
  --list-search-border: rgba(0, 0, 0, 0.06);

  &.is-dark {
    --list-search-bg: rgba(24, 24, 24, 0.5);
    --list-search-border: rgba(255, 255, 255, 0.11);
  }

  // 移动端顶部那条带子本身就是模糊 + 着色的，控件要在它之上仍读得出是一个层，
  // 所以抬高填充、收紧描边。这是 Nav 移动端得出的同一个结论。
  @media (max-width: 768px) {
    --list-search-bg: rgba(255, 255, 255, 0.62);
    --list-search-border: rgba(0, 0, 0, 0.07);

    &.is-dark {
      --list-search-bg: rgba(32, 32, 38, 0.6);
      --list-search-border: rgba(255, 255, 255, 0.11);
    }
  }

  display: flex;
  flex-direction: column;
  gap: 22px;
  padding: 10px clamp(16px, 3vw, 36px) 36px;

  .left {
    width: 100%;
    min-height: 0;
    position: relative;
    display: grid;
    grid-template-columns: minmax(176px, 278px) minmax(0, 1fr);
    align-items: center;
    gap: clamp(22px, 4vw, 38px);
    // 底部收窄，理由同歌单页。
    padding: 18px 2px 10px;

    .cover {
      position: relative;
      display: flex;
      align-items: center;
      justify-content: flex-start;
      width: 100%;
      aspect-ratio: 1 / 1;
      border-radius: var(--radius-md);
      transition: transform var(--duration-300) var(--ease-out);
      filter: drop-shadow(0 16px 28px rgba(var(--content-panel-accent-rgb, 0, 0, 0), 0.22));

      &:active {
        transform: scale(0.95);
      }

      .coverImg {
        border-radius: var(--radius-md);
        width: 100%;
        height: 100%;
        overflow: hidden;
        z-index: 1;

        :deep(img) {
          width: 100%;
          height: 100%;
          object-fit: cover;
        }
      }

      .album-disc {
        height: 94%;
        position: absolute;
        top: 3%;
        right: -16%;
        opacity: 0.82;
      }
    }

    .meta {
      width: 100%;
      display: flex;
      flex-direction: column;
      justify-content: flex-end;
      min-width: 0;

      .n-text {
        color: inherit;
      }

      .title {
        display: flex;
        flex-direction: column;
        min-width: 0;
        margin-top: 0;

        .detail-kind {
          margin-bottom: 7px;
          font-size: 11px;
          font-weight: 700;
          line-height: 1;
          text-transform: uppercase;
          color: rgb(var(--content-panel-accent-rgb, 128, 128, 128));
        }

        .name {
          display: -webkit-box;
          max-width: min(780px, 100%);
          overflow: hidden;
          font-size: clamp(32px, 5vw, 56px);
          font-weight: 800;
          line-height: 1.06;
          overflow-wrap: anywhere;
          -webkit-box-orient: vertical;
          -webkit-line-clamp: 2;
          line-clamp: 2;
        }

        .creator {
          width: fit-content;
          margin-top: 10px;
          font-size: 15px;
          font-weight: 700;
          color: var(--n-text-color-2);
          cursor: pointer;
          transition: color var(--duration-200) var(--ease-out);

          &:hover {
            color: rgb(var(--content-panel-accent-rgb, 128, 128, 128));
          }
        }
      }

      .detail-stats {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 8px 14px;
        margin-top: 13px;
        color: var(--n-text-color-3);

        .num {
          display: flex;
          align-items: center;
          min-width: 0;
          font-size: 13px;

          .n-icon {
            flex: 0 0 auto;
            margin-right: 5px;
          }
        }
      }

      .intr {
        max-width: 760px;
        margin-top: 14px;

        .name {
          display: none;
        }

        .desc {
          display: -webkit-box;
          -webkit-line-clamp: 2;
          line-clamp: 2;
          line-height: 22px;
          color: var(--n-text-color-3);
        }

        .all-desc {
          width: fit-content;
          margin-top: 12px;
        }
      }

      .tag {
        margin-top: 13px;

        .tags {
          height: 22px;
          font-size: 12px;
          color: var(--n-text-color-2);
          background-color: color-mix(in srgb, var(--n-border-color) 62%, transparent);
          cursor: pointer;
          transition: all var(--duration-300) var(--ease-out);

          &:hover {
            background-color: color-mix(
              in srgb,
              rgb(var(--content-panel-accent-rgb, 128, 128, 128)) 16%,
              transparent
            );
            color: rgb(var(--content-panel-accent-rgb, 128, 128, 128));
          }

          &:active {
            transform: scale(0.95);
          }
        }
      }
      .control {
        margin-top: 16px;

        :deep(.n-button) {
          --n-color: rgba(var(--content-panel-button-rgb, 226, 154, 128), 0.86);
          --n-color-hover: rgb(var(--content-panel-button-rgb, 226, 154, 128));
          --n-color-pressed: rgba(var(--content-panel-button-rgb, 226, 154, 128), 0.74);
          --n-color-focus: rgba(var(--content-panel-button-rgb, 226, 154, 128), 0.92);
          --n-text-color: rgb(var(--content-panel-on-button-rgb, 18, 18, 22));
          --n-text-color-hover: rgb(var(--content-panel-on-button-rgb, 18, 18, 22));
          --n-text-color-pressed: rgb(var(--content-panel-on-button-rgb, 18, 18, 22));
          --n-text-color-focus: rgb(var(--content-panel-on-button-rgb, 18, 18, 22));
          --n-border: 1px solid rgba(var(--content-panel-button-rgb, 226, 154, 128), 0.24);
          --n-border-hover: 1px solid rgba(var(--content-panel-button-rgb, 226, 154, 128), 0.36);
          --n-border-pressed: 1px solid rgba(var(--content-panel-button-rgb, 226, 154, 128), 0.24);
          --n-border-focus: 1px solid rgba(var(--content-panel-button-rgb, 226, 154, 128), 0.38);

          min-width: 112px;
          height: 34px;
          border: 1px solid rgba(var(--content-panel-on-button-rgb, 18, 18, 22), 0.16);
          box-shadow:
            inset 0 1px 0 rgba(255, 255, 255, 0.26),
            inset 0 0 0 1px rgba(var(--content-panel-button-rgb, 226, 154, 128), 0.2),
            0 8px 18px rgba(var(--content-panel-accent-rgb, 0, 0, 0), 0.12);
          font-weight: 700;
        }

        :deep(.n-button .n-button__border),
        :deep(.n-button .n-button__state-border) {
          border-color: transparent !important;
        }
      }
    }
  }

  .right {
    width: 100%;
    min-width: 0;

    .meta {
      display: none;
    }

    :deep(.datalists) {
      // 列宽的唯一出处：`.songs` 的行和 `.song-list-head` 的列头都读这组变量，
      // 所以不可能出现「改了行没改列头」的错位。
      // name:album 给到 1.6:1（NCM 官方大致是这个比例）——两边都 flex:1 会把专辑名
      // 顶到正中间，中间空一大段。
      --song-lead-size: 38px;
      --song-lead-gap: 14px;
      --song-action-width: 76px;
      --song-time-width: 46px;
      --song-name-flex: 1.6;
      --song-album-flex: 1;
      --song-row-padding-x: 12px;

      --detail-song-list-radius: var(--radius-md);

      margin-top: 2px;
    }

    :deep(.datalists .songs) {
      --n-color: transparent;
      --n-border-color: transparent;

      margin-bottom: 0;
      border: 0;
      border-radius: 0;
      box-shadow: none;
    }

    // 只用 `song-row-*` 类，不用 `:nth-child`。页面窗口模式下行被包在
    // `.song-plain-list` 里且前面有一个占位块，`:nth-child` 的奇偶会整体错位，
    // 而这些类是按**绝对下标**打的，滚到哪里都对。
    :deep(.datalists .songs.song-row-odd) {
      background-color: color-mix(in srgb, var(--n-text-color) 3%, transparent);
    }

    // 见歌单页同名规则：靠右锚在列表右边缘，不在左边浮着。
    // 悬浮，沿用 Nav 的那套语言（见 `components/Nav/index.vue`）。
    //
    // 关键是**横条整条透明，只有药丸自己有玻璃底**。给整条填实色是行不通的：这一页
    // 的底色是封面取样出来的渐变（`--content-panel-stage-gradient` 叠在
    // `--content-panel-bg` 上），实色盖上去必然是一条突兀的横条——试过
    // `--app-shell-bg`，那是渐变底下那层 `#f2f2f4` 冷灰。只让一个控件大小的区域走
    // backdrop-filter，模糊面积和 Nav 的按钮同级，代价可以接受。
    .list-toolbar {
      position: sticky;
      // 移动端顶部那条 42px 的玻璃带（`--nav-blur-edge`）会把滚过它的东西洗白，
      // 所以钉在带子**下沿**；桌面端没这个变量，回退 0。
      top: var(--nav-blur-edge, 0px);
      z-index: 3;
      display: flex;
      align-items: center;
      justify-content: flex-end;
      margin: 2px 0 10px;
      // 一条横跨整宽的透明 sticky 元素会把下面每一行的点击都吃掉。空白处必须放行，
      // 只有药丸本身接收事件。
      pointer-events: none;

      > * {
        pointer-events: auto;
      }
    }

    // 药丸形，对齐 NCM 的观感。见歌单页同名规则。
    .list-search {
      width: 170px;
      transition: width var(--duration-300) var(--ease-out);

      :deep(.n-input__border),
      :deep(.n-input__state-border) {
        border-radius: 999px;
      }

      // 要在**两种**背景上都站得住：封面取样的渐变底，以及滚动时罩在上面那条 42px
      // 玻璃带。带子会把低对比度的东西直接洗掉——纯靠 `--n-text-color` 的淡色 tint
      // 滚进去就只剩一个幽灵轮廓（试过 5%、7%，都不行）。
      //
      // 参数直接取自 Nav 的悬浮按钮，包括它移动端那条注释的结论：带子后面要**抬高**
      // 填充不透明度，并且收紧阴影——宽而软的投影压在模糊上只会糊成一团灰光晕。
      :deep(&.n-input) {
        border-radius: var(--radius-pill);
        background-color: var(--list-search-bg);
        box-shadow:
          0 8px 22px rgb(0 0 0 / 10%),
          inset 0 1px 0 rgb(255 255 255 / 24%);
        -webkit-backdrop-filter: blur(18px) saturate(160%);
        backdrop-filter: blur(18px) saturate(160%);
      }

      :deep(.n-input__border),
      :deep(.n-input__state-border) {
        border: 1px solid var(--list-search-border);
        border-radius: var(--radius-pill);
      }

      &:focus-within,
      &.has-value {
        width: min(300px, 100%);
      }
    }

    :deep(.datalists .songs.song-row-even) {
      background-color: color-mix(in srgb, var(--n-text-color) 6%, transparent);
    }

    :deep(.datalists .songs.song-row-first) {
      border-radius: var(--detail-song-list-radius) var(--detail-song-list-radius) 0 0;
    }

    :deep(.datalists .songs.song-row-last) {
      border-radius: 0 0 var(--detail-song-list-radius) var(--detail-song-list-radius);
    }

    :deep(.datalists .songs.song-row-single) {
      border-radius: var(--detail-song-list-radius);
    }

    :deep(.datalists .songs:hover) {
      background-color: color-mix(in srgb, var(--n-text-color) 10%, transparent);
      box-shadow: none;
    }

    :deep(.datalists .songs.play) {
      background-color: color-mix(in srgb, var(--main-color) 13%, transparent);
    }

    :deep(.datalists .songs .n-card__content) {
      min-height: 52px;
      padding: 8px var(--song-row-padding-x) !important;
    }

    :deep(.datalists .songs .pic),
    :deep(.datalists .songs .num) {
      border-radius: var(--radius-sm);
      font-size: 13px;
    }

    :deep(.datalists .songs .name .title) {
      font-size: 14px;
    }

    :deep(.datalists .songs .name .meta) {
      font-size: 12px;
    }

    :deep(.datalists .songs .album) {
      font-size: 13px;
      opacity: 0.72;
    }

    :deep(.datalists .songs .time) {
      font-size: 12px;
      opacity: 0.64;
    }
  }

  @media (max-width: 768px) {
    gap: 14px;
    padding: 8px 14px 28px;

    .left {
      min-height: 0;
      grid-template-columns: 1fr;
      align-items: start;
      gap: 16px;
      padding: 12px 0 18px;

      .cover {
        justify-self: center;
        width: min(58vw, 260px);
      }

      .meta {
        color: var(--n-text-color);

        .n-text {
          color: inherit;
        }

        .title {
          color: var(--n-text-color);

          .detail-kind {
            margin-bottom: 7px;
            font-size: 11px;
          }

          .name {
            font-size: clamp(25px, 8vw, 36px);
            line-height: 1.12;
          }

          .creator {
            font-size: 14px;
            margin-top: 9px;
          }
        }

        .detail-stats {
          margin-top: 12px;
          color: var(--n-text-color-3);
        }

        .intr {
          margin-top: 16px;

          .desc {
            -webkit-line-clamp: 2;
            line-clamp: 2;
            color: var(--n-text-color-3);
          }
        }

        .control {
          margin-top: 16px;

          :deep(.n-button) {
            height: 38px;
          }
        }
      }
    }

    .right {
      :deep(.datalists) {
        --song-lead-size: 42px;
        --song-lead-gap: 11px;
        --song-row-padding-x: 6px;
      }

      // 窄屏上「靠右的窄输入框 + 左边一大片空」很怪，而且手指要伸到角上。
      // 直接占满一行，也就不需要聚焦展开了。
      // 填充抬高、描边收紧在根的 token 块里按断点声明（见文件上方），这里不重复。
      .list-toolbar {
        margin: 0 0 8px;
      }

      .list-search {
        width: 100%;

        &:focus-within,
        &.has-value {
          width: 100%;
        }
      }

      :deep(.datalists .songs) {
        margin-bottom: 0;
        border-radius: 0;
      }

      :deep(.datalists .songs .n-card__content) {
        min-height: 58px;
        padding: 9px var(--song-row-padding-x) !important;
      }

      :deep(.datalists .songs .name) {
        padding-right: 8px;
      }

      :deep(.datalists .songs .album),
      :deep(.datalists .songs .time) {
        display: none;
      }
    }
  }

  @media (max-width: 540px) {
    .left {
      .cover {
        width: min(64vw, 235px);
      }

      .meta {
        .tag {
          display: none !important;
        }
      }
    }
  }

  @media (max-width: 380px) {
    .left {
      .meta {
        .control {
          :deep(.n-button:first-child) {
            min-width: 96px;
          }
        }
      }
    }
  }
}

.title {
  margin-top: 30px;
  margin-bottom: 20px;
  font-size: 24px;
  .key {
    font-size: 40px;
    font-weight: bold;
    margin-right: 8px;
  }
}
.loading {
  .left {
    .pic {
      width: 100%;
      height: 100%;
      border-radius: var(--radius-md) !important;
    }

    .album-disc {
      height: 94%;
      width: 94%;
      position: absolute;
      top: 3%;
      right: -16%;
      opacity: 0.2;
      border-radius: var(--radius-pill);
    }

    .loading-meta {
      gap: 10px;
    }

    .loading-title {
      :deep(.n-skeleton) {
        height: clamp(32px, 5vw, 52px);
      }
    }

    .loading-stats,
    .loading-actions {
      display: flex;
      flex-wrap: wrap;
      gap: 10px 14px;
      margin-top: 2px;
    }
  }

  .right {
    display: flex;
    flex-direction: column;
    gap: 0;
  }

  .loading-row {
    min-height: 52px;
    display: grid;
    grid-template-columns: 38px minmax(0, 1fr) minmax(70px, 12vw) 46px;
    align-items: center;
    gap: 14px;
    padding: 8px 12px;
    border-radius: 0;

    &:nth-child(odd) {
      background-color: color-mix(in srgb, var(--n-text-color) 3%, transparent);
    }

    &:nth-child(even) {
      background-color: color-mix(in srgb, var(--n-text-color) 6%, transparent);
    }

    &:first-child {
      border-radius: var(--radius-md) var(--radius-md) 0 0;
    }

    &:last-child {
      border-radius: 0 0 var(--radius-md) var(--radius-md);
    }

    &:only-child {
      border-radius: var(--radius-md);
    }
  }

  .loading-row-main {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  @media (max-width: 768px) {
    .left {
      .album-disc {
        display: none;
      }
    }

    .loading-row {
      grid-template-columns: 42px minmax(0, 1fr);
      min-height: 58px;

      > :nth-child(n + 3) {
        display: none;
      }
    }
  }
}
</style>
