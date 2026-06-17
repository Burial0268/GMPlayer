<template>
  <div class="album" v-if="albumDetail">
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
      <DataLists :listData="albumData" hideAlbum />
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
import { renderIcon } from "@/utils/ui/renderIcon";
import { buildLikeMessage } from "@/utils/ui/buildLikeMessage";
import { usePlayAllSong } from "@/composables/usePlayAllSong";
import { useContentPanelAccent } from "@/composables/useContentPanelAccent";
import { MusicList, LinkTwo, More, Like, Unlike, People, Time, City } from "@icon-park/vue-next";
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

// 判断收藏还是取消
const isLikeOrDislike = (id) => {
  const playlists = user.getUserAlbumLists.list;
  if (playlists.length) {
    return !playlists.some((item) => item.id === Number(id));
  }
  return true;
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
    padding: 18px 2px 24px;

    .cover {
      position: relative;
      display: flex;
      align-items: center;
      justify-content: flex-start;
      width: 100%;
      aspect-ratio: 1 / 1;
      border-radius: var(--radius-md);
      transition: transform 0.3s;
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
          max-width: 780px;
          font-size: clamp(32px, 5vw, 56px);
          font-weight: 800;
          line-height: 1.06;
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
          transition: color 0.2s;

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
          transition: all 0.3s;

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

    :deep(.datalists .songs:nth-child(odd)) {
      background-color: color-mix(in srgb, var(--n-text-color) 3%, transparent);
    }

    :deep(.datalists .songs:nth-child(even)) {
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
      padding: 8px 12px !important;
    }

    :deep(.datalists .songs .pic),
    :deep(.datalists .songs .num) {
      width: 38px;
      height: 38px;
      min-width: 38px;
      margin-right: 14px;
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

    :deep(.datalists .songs .action) {
      width: 76px;
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
      :deep(.datalists .songs) {
        margin-bottom: 0;
        border-radius: 0;
      }

      :deep(.datalists .songs .n-card__content) {
        min-height: 58px;
        padding: 9px 6px !important;
      }

      :deep(.datalists .songs .pic),
      :deep(.datalists .songs .num) {
        width: 42px;
        height: 42px;
        min-width: 42px;
        margin-right: 11px;
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
