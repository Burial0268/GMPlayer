<template>
  <div class="playlist" v-if="playListDetail">
    <div class="left">
      <div class="cover">
        <n-image
          show-toolbar-tooltip
          class="coverImg"
          :src="getCoverUrl(playListDetail.coverImgUrl, 1024)"
          :previewed-img-props="{ style: { borderRadius: '8px' } }"
          :preview-src="getCoverUrl(playListDetail.coverImgUrl)"
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
          <n-text class="name text-hidden">{{ playListDetail.name }}</n-text>
          <n-text class="creator">{{ playListDetail.creator.nickname }}</n-text>
        </div>
        <div class="intr">
          <span class="name">{{
            $t("general.name.desc", { name: $t("general.name.playlist") })
          }}</span>
          <span class="desc text-hidden">
            {{
              playListDetail.description
                ? playListDetail.description
                : $t("other.noDesc")
            }}
          </span>
          <n-button
            class="all-desc"
            block
            strong
            secondary
            v-if="playListDetail?.description?.length > 70"
            @click="playListDescShow = true"
          >
            {{ $t("general.name.allDesc") }}
          </n-button>
        </div>
        <n-space class="tag" v-if="playListDetail.tags">
          <n-tag
            class="tags"
            round
            :bordered="false"
            v-for="item in playListDetail.tags"
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
        <n-text class="name">{{ playListDetail.name }}</n-text>
        <n-text class="creator">
          <n-icon :depth="3" :component="People" />
          {{ playListDetail.creator.nickname }}
        </n-text>
        <n-space class="time">
          <div class="num">
            <n-icon :depth="3" :component="Newlybuild" />
            <n-text v-html="getLongTime(playListDetail.createTime)" />
          </div>
          <div class="num">
            <n-icon :depth="3" :component="Write" />
            <n-text v-html="getLongTime(playListDetail.updateTime)" />
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
        :title="$t('general.name.desc', { name: $t('general.name.playlist') })"
        :bordered="false"
      >
        <n-scrollbar>
          <n-text v-html="playListDetail.description.replace(/\n/g, '<br>')" />
        </n-scrollbar>
      </n-modal>
    </div>
  </div>
  <div class="title" v-else-if="!playListId || !loadingState">
    <span class="key">{{
      loadingState
        ? $t("general.name.noKeywords")
        : $t("general.message.acquisitionFailed")
    }}</span>
    <br />
    <n-button strong secondary @click="router.go(-1)" style="margin-top: 20px">
      {{ $t("general.name.goBack") }}
    </n-button>
  </div>
  <div class="loading" v-else>
    <div class="left">
      <n-skeleton class="pic" />
      <n-skeleton text :repeat="5" />
      <n-skeleton text style="width: 60%" />
    </div>
    <div class="right">
      <n-skeleton :sharp="false" height="80px" width="60%" />
      <n-skeleton height="100%" width="100%" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { NIcon, NText } from "naive-ui";
import {
  getPlayListDetail,
  getAllPlayList,
  delPlayList,
  likePlaylist,
} from "@/api/playlist";
import { useRouter } from "vue-router";
import { userStore, musicStore, settingStore } from "@/store";
import { getLongTime } from "@/utils/timeTools";
import { transformSongData } from "@/utils/ncm/transformSongData";
import { renderIcon } from "@/utils/ui/renderIcon";
import { buildLikeMessage } from "@/utils/ui/buildLikeMessage";
import { usePlayAllSong } from "@/composables/usePlayAllSong";
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

// 歌单数据
const playListId = ref(router.currentRoute.value.query.id);
const playListDetail = ref(null);
const playListData = ref([]);
const playListDescShow = ref(false);
const pagelimit = ref(30);
const loadingState = ref(true);
const pageNumber = ref(
  router.currentRoute.value.query.page
    ? Number(router.currentRoute.value.query.page)
    : 1
);
const totalCount = ref(0);

const normalizePlaylistId = (id: string | number | string[]) =>
  Number(Array.isArray(id) ? id[0] : id);

// 判断收藏还是取消
const isLikeOrDislike = (id: string | string[]) => {
  const playlists = user.getUserPlayLists.like;
  if (playlists.length) {
    return !playlists.some((item) => item.id === Number(id));
  }
  return true;
};

// 判断是否可删除
const isCanDelete = (id: string | string[]) => {
  const playlists = user.getUserPlayLists.own;
  if (playlists.length) {
    return playlists.some((item) => item.id === Number(id));
  }
  return false;
};

// 歌单下拉菜单数据
const dropdownOptions = ref([]);

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
                `https://music.163.com/#/playlist?id=${playListId.value}`
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
      icon: renderIcon(h(LinkTwo)),
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
      icon: renderIcon(h(DeleteFour)),
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
      icon: renderIcon(h(isLikeOrDislike(playListId.value) ? Like : Unlike)),
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
const toDelPlayList = (data: { id: number; name: any; }) => {
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
    getAllPlayListData(
      playListId.value,
      pagelimit.value,
      (pageNumber.value - 1) * pagelimit.value
    );
    if (
      user.userLogin &&
      !user.getUserPlayLists.has &&
      !user.getUserPlayLists.isLoading
    ) {
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
  getAllPlayListData(
    playListId.value,
    val,
    (pageNumber.value - 1) * pagelimit.value
  );
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
    if (val.name == "playlist") {
      playListId.value = val.query.id;
      pageNumber.value = Number(val.query.page ? val.query.page : 1);
      if (val.query.id != oldVal?.query?.id) {
        getPlayListDetailData(playListId.value);
        getAllPlayListData(
          playListId.value,
          pagelimit.value,
          (pageNumber.value - 1) * pagelimit.value
        );
      } else {
        getAllPlayListData(
          playListId.value,
          pagelimit.value,
          (pageNumber.value - 1) * pagelimit.value
        );
      }
    }
  }
);
</script>

<style lang="scss" scoped>
.playlist,
.loading {
  display: flex;
  flex-direction: row;
  .left {
    width: 40vw;
    height: 100%;
    max-width: 320px;
    min-width: 200px;
    margin-right: 20px;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    position: sticky;
    top: 24px;
    @media (max-width: 990px) {
      margin-right: 0;
      width: 30vw;
    }
    .cover {
      position: relative;
      display: flex;
      align-items: center;
      justify-content: flex-start;
      width: 80%;
      height: 80%;
      border-radius: 8px;
      position: relative;
      transition: transform 0.3s;
      &:active {
        transform: scale(0.95);
      }
      .coverImg {
        border-radius: 8px;
        width: 100%;
        height: 100%;
        overflow: hidden;
        z-index: 1;
        :deep(img) {
          width: 100%;
        }
      }
      .shadow {
        position: absolute;
        top: 12px;
        height: 100%;
        width: 100%;
        filter: blur(16px) opacity(0.6);
        transform: scale(0.92, 0.96);
        z-index: 0;
        background-size: cover;
        aspect-ratio: 1/1;
      }
    }
    .meta {
      width: 100%;
      height: 100%;
      display: flex;
      flex-direction: column;
      justify-content: flex-start;
      .title {
        display: none;
        flex-direction: column;
        margin-top: 0;
        .name {
          font-size: 28px;
          font-weight: bold;
          -webkit-line-clamp: 2;
          line-clamp: 2;
        }
        .creator {
          margin-top: 6px;
          font-size: 16px;
          opacity: 0.8;
        }
      }
      .intr {
        margin-top: 24px;
        width: 80%;
        padding-left: 4px;
        .name {
          display: block;
          font-size: 20px;
          font-weight: bold;
          margin-bottom: 12px;
          @media (max-width: 990px) {
            font-size: 18px;
          }
        }
        .desc {
          -webkit-line-clamp: 4;
          line-clamp: 4;
          line-height: 26px;
          margin-bottom: 16px;
        }
      }
      .tag {
        margin-top: 20px;
        .tags {
          line-height: 0;
          font-size: 13px;
          cursor: pointer;
          transition: all 0.3s;
          &:hover {
            background-color: var(--main-second-color);
            color: var(--main-color);
          }
          &:active {
            transform: scale(0.95);
          }
        }
      }
      .control {
        margin-top: 20px;
      }
    }
  }
  .right {
    flex: 1;
    .meta {
      display: flex;
      flex-direction: column;
      margin-top: 20px;
      margin-bottom: 20px;
      .name {
        font-size: 30px;
        font-weight: bold;
      }
      .creator {
        display: flex;
        align-items: center;
        margin-top: 6px;
        font-size: 16px;
        opacity: 0.8;
        cursor: pointer;
        transition: all 0.3s;
        &:hover {
          opacity: 1;
          color: var(--main-color);
        }
        .n-icon {
          margin-right: 6px;
        }
      }
      .time {
        margin-top: 8px;
        display: flex;
        flex-direction: row;
        align-items: center;
        @media (max-width: 768px) {
          flex-direction: column;
          align-items: flex-start;
        }
        .num {
          // color: #999;
          display: flex;
          flex-direction: row;
          align-items: center;
          .n-icon {
            margin-right: 6px;
          }
        }
      }
    }
    .datalists {
      :deep(.songs) {
        @media (max-width: 990px) {
          .album,
          .time {
            display: none;
          }
        }
      }
    }
  }
  @media (max-width: 768px) {
    flex-direction: column;
    .left {
      position: relative;
      top: 0;
      width: 100%;
      height: 40vw;
      max-width: none;
      display: flex;
      flex-direction: row;
      .cover {
        height: 100%;
        min-width: 40vw;
        margin-right: 30px;
      }
      .meta {
        .title {
          display: flex;
          margin-bottom: 16px;
          .name {
            font-size: 25px;
          }
          .creator {
            font-size: 15px;
          }
        }
        .intr {
          margin-top: 0;
          padding-left: 0;
          .name,
          .all-desc {
            display: none;
          }
          .desc {
            -webkit-line-clamp: 2;
            line-clamp: 2;
            margin-bottom: 0;
          }
        }
        .control {
          position: absolute;
          left: 0;
          bottom: -60px;
        }
      }
    }
    .right {
      margin-top: 80px;
      .meta {
        display: none;
      }
    }
  }
  @media (max-width: 540px) {
    .left {
      .cover {
        margin-right: 20px;
      }
      .meta {
        .title {
          .name {
            font-size: 24px;
          }
        }
        .intr,
        .tag {
          display: none !important;
        }
        .control {
          position: static;
        }
      }
    }
    .right {
      margin-top: 30px;
    }
  }
  @media (max-width: 520px) {
    .left {
      .meta {
        .title {
          margin-bottom: 0;
          .name {
            font-size: 20px;
          }
          .creator {
            font-size: 12px;
          }
        }
      }
    }
  }
  @media (max-width: 370px) {
    .left {
      .meta {
        .title {
          .name {
            -webkit-line-clamp: 3;
            line-clamp: 3;
          }
        }
        .control {
          position: absolute;
        }
      }
    }
    .right {
      margin-top: 80px;
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
    display: block;
    .pic {
      padding-bottom: 100%;
      width: 100%;
      height: 0;
      border-radius: 8px !important;
      margin-bottom: 20px;
    }
  }
  .right {
    .n-skeleton {
      margin-bottom: 20px;
    }
  }
}
</style>
