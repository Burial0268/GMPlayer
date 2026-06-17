<template>
  <div class="playlist" v-if="playListDetail">
    <div class="left">
      <div class="cover">
        <n-image
          show-toolbar-tooltip
          class="coverImg"
          :src="getCoverUrl(playListDetail.coverImgUrl, 1024)"
          :previewed-img-props="{ style: { borderRadius: 'var(--radius-md)' } }"
          :preview-src="getCoverUrl(playListDetail.coverImgUrl, 512)"
          fallback-src="/images/pic/default.png"
        />
        <n-image
          preview-disabled
          class="shadow"
          :src="getCoverUrl(playListDetail.coverImgUrl, 1024)"
          fallback-src="/images/pic/default.png"
        />
      </div>
      <div class="meta">
        <div class="title">
          <span class="detail-kind">{{ t("general.name.playlist") }}</span>
          <n-text class="name text-hidden">{{ playListDetail!.name }}</n-text>
          <n-text class="creator">{{ playListDetail!.creator.nickname }}</n-text>
        </div>
        <div class="detail-stats">
          <div class="num" v-if="playListDetail && playListDetail.createTime">
            <n-icon :depth="3" :component="Newlybuild" />
            <n-text v-html="getLongTime(playListDetail.createTime)" />
          </div>
          <div class="num" v-if="playListDetail && playListDetail.updateTime">
            <n-icon :depth="3" :component="Write" />
            <n-text v-html="getLongTime(playListDetail.updateTime)" />
          </div>
          <div class="num" v-if="totalCount">
            <n-icon :depth="3" :component="MusicList" />
            <n-text>{{ t("general.name.songSize", { size: totalCount }) }}</n-text>
          </div>
        </div>
        <div class="intr">
          <span class="name">{{
            t("general.name.desc", { name: t("general.name.playlist") })
          }}</span>
          <span class="desc text-hidden">
            {{
              playListDetail && playListDetail.description
                ? playListDetail.description
                : t("other.noDesc")
            }}
          </span>
          <n-button
            class="all-desc"
            block
            strong
            secondary
            v-if="
              playListDetail && playListDetail.description && playListDetail.description.length > 70
            "
            @click="playListDescShow = true"
          >
            {{ t("general.name.allDesc") }}
          </n-button>
        </div>
        <n-space class="tag" v-if="playListDetail && playListDetail.tags">
          <n-tag
            class="tags"
            round
            :bordered="false"
            v-for="item in playListDetail!.tags"
            :key="item"
            @click="router.push(`/discover/playlists?cat=${item}&page=1`)"
          >
            {{ item }}
          </n-tag>
        </n-space>
        <n-space class="control">
          <n-button strong secondary round type="primary" @click="playAllSong">
            <template #icon>
              <n-icon :component="MusicList" />
            </template>
            {{ t("general.name.play") }}
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
        <n-text class="name">{{ playListDetail!.name }}</n-text>
        <n-text class="creator">
          <n-icon :depth="3" :component="People" />
          {{ playListDetail!.creator.nickname }}
        </n-text>
        <n-space class="time">
          <div class="num">
            <n-icon :depth="3" :component="Newlybuild" />
            <n-text
              v-if="playListDetail && playListDetail.createTime"
              v-html="getLongTime(playListDetail.createTime)"
            />
          </div>
          <div class="num">
            <n-icon :depth="3" :component="Write" />
            <n-text
              v-if="playListDetail && playListDetail.updateTime"
              v-html="getLongTime(playListDetail.updateTime)"
            />
          </div>
        </n-space>
      </div>
      <DataLists :listData="playListData" />
      <Pagination
        :totalCount="totalCount"
        :pageNumber="pageNumber"
        :showSizePicker="false"
        :showQuickJumper="false"
        @pageSizeChange="pageSizeChange"
        @pageNumberChange="pageNumberChange"
      />
      <!-- 歌单简介 -->
      <n-modal
        class="s-modal"
        v-model:show="playListDescShow"
        preset="card"
        :title="t('general.name.desc', { name: t('general.name.playlist') })"
        :bordered="false"
      >
        <n-scrollbar v-if="hasPlaylistDescription">
          <n-text v-html="playlistDescriptionHtml" />
        </n-scrollbar>
      </n-modal>
    </div>
  </div>
  <div class="title" v-else-if="!playListId || !loadingState">
    <span class="key">{{
      loadingState ? t("general.name.noKeywords") : t("general.message.acquisitionFailed")
    }}</span>
    <br />
    <n-button strong secondary @click="router.go(-1)" style="margin-top: 20px">
      {{ t("general.name.goBack") }}
    </n-button>
  </div>
  <div class="loading" v-else>
    <div class="left">
      <div class="cover">
        <n-skeleton class="pic" />
        <n-skeleton class="shadow" />
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
        <n-skeleton text width="min(180px, 16vw)" />
        <n-skeleton text width="46px" />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import type { DropdownMixedOption } from "naive-ui/es/dropdown/src/interface";
import { NIcon, NText } from "naive-ui";
import { getPlayListDetail, getAllPlayList, delPlayList, likePlaylist } from "@/api/playlist";
import { useRouter } from "vue-router";
import { userStore, musicStore, settingStore } from "@/store";
import { getLongTime } from "@/utils/timeTools";
import { transformSongData } from "@/utils/ncm/transformSongData";
import { renderIcon } from "@/utils/ui/renderIcon";
import { buildLikeMessage } from "@/utils/ui/buildLikeMessage";
import { usePlayAllSong } from "@/composables/usePlayAllSong";
import { useContentPanelAccent } from "@/composables/useContentPanelAccent";
import {
  MusicList,
  LinkTwo,
  More,
  DeleteFour,
  Like,
  Unlike,
  Newlybuild,
  Write,
  People,
} from "@icon-park/vue-next";
import { useI18n } from "vue-i18n";
import DataLists from "@/components/DataList/DataLists.vue";
import Pagination from "@/components/Pagination/index.vue";
import getCoverUrl from "@/utils/ncm/getCoverUrl";

const { t } = useI18n();
const router = useRouter();
const user = userStore();
const music = musicStore();
const setting = settingStore();
const { playAllSong: playAll } = usePlayAllSong();
const { applyContentPanelAccent } = useContentPanelAccent();

// 歌单数据
const playListId = ref<string | number | string[] | undefined>(
  router.currentRoute.value.query.id as string | number | string[] | undefined,
);

interface PlaylistCreator {
  nickname: string;
}

interface PlaylistDetail {
  id: number;
  name: string;
  coverImgUrl: string;
  creator: PlaylistCreator;
  description?: string;
  tags?: string[];
  createTime?: number;
  updateTime?: number;
}

const playListDetail = ref<PlaylistDetail | null>(null);
const playListData = ref<unknown[]>([]);
const playListDescShow = ref(false);
const pagelimit = ref(30);
const loadingState = ref(true);
const pageNumber = ref<number>(
  router.currentRoute.value.query.page ? Number(router.currentRoute.value.query.page) : 1,
);
const totalCount = ref(0);

const hasPlaylistDescription = computed(
  () => !!playListDetail.value && !!playListDetail.value.description,
);

const playlistDescriptionHtml = computed(
  () => playListDetail.value?.description?.replace(/\n/g, "<br>") ?? "",
);

const normalizePlaylistId = (id: string | number | string[]) =>
  Number(Array.isArray(id) ? id[0] : id);

// 判断收藏还是取消
const isLikeOrDislike = (id: string | number | string[]) => {
  const playlists = user.getUserPlayLists.like;
  if (playlists.length) {
    return !playlists.some((item) => item.id === Number(id));
  }
  return true;
};

// 判断是否可删除
const isCanDelete = (id: string | number | string[]) => {
  const playlists = user.getUserPlayLists.own;
  if (playlists.length) {
    return playlists.some((item) => item.id === Number(id));
  }
  return false;
};

// 歌单下拉菜单数据
const dropdownOptions = ref<DropdownMixedOption[]>([]);

// 更改歌单下拉菜单数据
const setDropdownOptions = () => {
  dropdownOptions.value = [
    {
      key: "copy",
      label: t("menu.copy", {
        name: t("general.name.playlist"),
        other: t("general.name.link"),
      }),
      props: {
        onClick: () => {
          if (navigator.clipboard) {
            try {
              navigator.clipboard.writeText(
                `https://music.163.com/#/playlist?id=${playListId.value}`,
              );
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
      icon: renderIcon(h(LinkTwo) as any),
    },
    {
      key: "del",
      label: t("menu.del"),
      show: user.userLogin && isCanDelete(playListId.value),
      props: {
        onClick: () => {
          toDelPlayList(playListDetail.value);
        },
      },
      icon: renderIcon(h(DeleteFour) as any),
    },
    {
      key: "like",
      label: isLikeOrDislike(playListId.value)
        ? t("menu.collection", { name: t("general.name.playlist") })
        : t("menu.cancelCollection", { name: t("general.name.playlist") }),
      show: user.userLogin && !isCanDelete(playListId.value),
      props: {
        onClick: () => {
          toChangeLike(playListId.value);
        },
      },
      icon: renderIcon(h(isLikeOrDislike(playListId.value) ? Like : Unlike) as any),
    },
  ];
};

// 获取歌单信息
const getPlayListDetailData = (id: string | number | string[]) => {
  getPlayListDetail(normalizePlaylistId(id))
    .then((res) => {
      // 歌单总数
      totalCount.value = res.playlist.trackCount;
      // 歌单信息
      playListDetail.value = res.playlist;
      applyContentPanelAccent(getCoverUrl(res.playlist.coverImgUrl, 256));
      $setSiteTitle(res.playlist.name + " - " + t("general.name.playlist"));
    })
    .catch((err) => {
      $setSiteTitle(t("general.name.playlist"));
      loadingState.value = false;
      console.error(t("general.message.acquisitionFailed"), err);
      $message.error(t("general.message.acquisitionFailed"));
    });
};

// 获取歌单所有歌曲
const getAllPlayListData = (id: string | number | string[], limit = 30, offset = 0) => {
  const sourceId = normalizePlaylistId(id);
  getAllPlayList(sourceId, limit, offset).then((res) => {
    if (res.songs) {
      playListData.value = transformSongData(res.songs, {
        offset: (pageNumber.value - 1) * pagelimit.value,
        sourceId,
      });
    } else {
      $message.error(t("general.message.acquisitionFailed"));
    }
    // 请求后回顶
    if (typeof $scrollToTop !== "undefined") $scrollToTop();
  });
};

// 播放歌单所有歌曲
const playAllSong = () => {
  playAll(playListData.value);
};

// 删除歌单
const toDelPlayList = (data: { id: number; name: any }) => {
  if (data.id === user.getUserPlayLists?.own[0].id) {
    $message.warning(t("menu.unableToDelete"));
    return false;
  }
  $dialog.warning({
    class: "s-dialog",
    title: t("general.dialog.delete"),
    content: t("menu.delQuestion", {
      name: data.name,
    }),
    positiveText: t("general.dialog.delete"),
    negativeText: t("general.dialog.cancel"),
    onPositiveClick: () => {
      delPlayList(data.id).then((res) => {
        if (res.code === 200) {
          $message.success(t("general.message.deleteSuccess"));
          user.setUserPlayLists();
          router.push("/user/playlists");
        }
      });
    },
  });
};

// 收藏/取消收藏
const toChangeLike = async (id: string | number | string[]) => {
  const type = isLikeOrDislike(id.toString()) ? 1 : 2;
  const likeMsg = t("general.name.playlist");
  try {
    const res = await likePlaylist(normalizePlaylistId(id), type);
    if (res.code === 200) {
      $message.success(buildLikeMessage(t, likeMsg, type, "success", setting.language));
      user.setUserPlayLists(() => {
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
  if (playListId.value) {
    getPlayListDetailData(playListId.value);
    getAllPlayListData(playListId.value, pagelimit.value, (pageNumber.value - 1) * pagelimit.value);
    if (user.userLogin && !user.getUserPlayLists.has && !user.getUserPlayLists.isLoading) {
      user.setUserPlayLists(() => {
        setDropdownOptions();
      });
    } else {
      setDropdownOptions();
    }
  }
});

// 每页个数数据变化
const pageSizeChange = (val: number) => {
  pagelimit.value = val;
  getAllPlayListData(playListId.value, val, (pageNumber.value - 1) * pagelimit.value);
};

// 当前页数数据变化
const pageNumberChange = (val: number) => {
  router.push({
    path: "/playlist",
    query: {
      id: playListId.value,
      page: val,
    },
  });
};

// 监听路由参数变化
watch(
  () => router.currentRoute.value,
  (val, oldVal) => {
    if (val.name === "playlist") {
      playListId.value = val.query.id;
      pageNumber.value = Number(val.query.page ? val.query.page : 1);
      if (val.query.id !== oldVal?.query?.id) {
        getPlayListDetailData(playListId.value);
        getAllPlayListData(
          playListId.value,
          pagelimit.value,
          (pageNumber.value - 1) * pagelimit.value,
        );
      } else {
        getAllPlayListData(
          playListId.value,
          pagelimit.value,
          (pageNumber.value - 1) * pagelimit.value,
        );
      }
    }
  },
);
</script>

<style lang="scss" scoped>
.playlist,
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

      .shadow {
        position: absolute;
        inset: 10px 0 0;
        height: 100%;
        width: 100%;
        filter: blur(18px) opacity(0.28);
        transform: scale(0.92, 0.94);
        z-index: 0;
        background-size: cover;
        aspect-ratio: 1/1;
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
      background-color: transparent;
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

    :deep(.pagination) {
      margin-top: 18px;
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
            font-size: 15px;
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
      position: relative;
      z-index: 1;
      width: 100%;
      height: 100%;
      border-radius: var(--radius-md) !important;
    }

    .shadow {
      position: absolute;
      inset: 10px 0 0;
      width: 100%;
      height: 100%;
      border-radius: var(--radius-md);
      filter: blur(18px) opacity(0.24);
      transform: scale(0.92, 0.94);
      z-index: 0;
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
    grid-template-columns: 38px minmax(0, 1fr) minmax(84px, 16vw) 46px;
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
      .shadow {
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
