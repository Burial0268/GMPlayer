<template>
  <div class="video">
    <section class="mainVideo">
      <video ref="videoRef" class="plyr" />
      <div class="info" v-if="videoData">
        <span class="title">{{ videoData.name }}</span>
        <AllArtists :artistsData="videoData.artists" />
        <n-ellipsis
          v-if="videoData.desc"
          class="desc"
          expand-trigger="click"
          line-clamp="3"
          :tooltip="false"
          v-html="videoData.desc.replace(/\n/g, '<br>')"
        />
        <div class="num">
          <span class="playCount">
            <n-icon :component="OndemandVideoFilled" />
            {{ formatNumber(videoData.playCount) }}
          </span>
          <span class="shareCount">
            <n-icon :component="ShareFilled" />
            {{ formatNumber(videoData.shareCount) }}
          </span>
          <span class="commentCount">
            <n-icon :component="MessageOutlined" />
            {{ formatNumber(videoData.commentCount) }}
          </span>
        </div>
      </div>
      <!-- 与评论页同一套：热门 / 全部 + 加载更多（useCommentThread）。 -->
      <div
        class="comment-section"
        v-if="thread.loading.value || thread.error.value || thread.total.value"
      >
        <n-tabs class="comment-tab" type="segment" v-model:value="thread.tab.value">
          <n-tab name="hot" :disabled="!thread.hot.value.length">
            {{ $t("general.name.hotComments") }}
          </n-tab>
          <n-tab name="all">
            {{ $t("general.name.allComments") }} · {{ formatNumber(thread.total.value) }}
          </n-tab>
        </n-tabs>
        <PageLoadState v-if="thread.error.value" error @retry="thread.refresh(true)" />
        <div v-else-if="thread.loading.value" class="comment-stack">
          <n-skeleton v-for="index in 4" :key="index" class="row-skeleton" />
        </div>
        <template v-else>
          <div class="content comment-stack">
            <Comment
              v-for="item in thread.visible.value"
              :key="item.commentId"
              :commentData="item"
              :resourceId="commentResourceId"
              resourceType="mv"
            />
          </div>
          <div v-if="thread.tab.value === 'all' && thread.hasMore.value" class="load-more">
            <n-button
              strong
              secondary
              round
              :loading="thread.loadingMore.value"
              @click="thread.loadMore"
            >
              {{ $t("general.name.loadMore") }}
            </n-button>
          </div>
        </template>
      </div>
    </section>
    <section class="simiVideo">
      <n-h6 prefix="bar"> {{ $t("general.name.simiVideo") }} </n-h6>
      <VideoLists v-if="simiVideo[0]" :listData="simiVideo" />
    </section>
  </div>
</template>

<script setup lang="ts">
import { useRouter } from "vue-router";
import { musicStore, settingStore } from "@/store";
import { getVideoDetail, getVideoUrl, getSimiVideo } from "@/api/video";
import { useCommentThread } from "@/composables/useCommentThread";
import { formatNumber, getSongTime } from "@/utils/timeTools";
import { OndemandVideoFilled, ShareFilled, MessageOutlined } from "@vicons/material";
import { useI18n } from "vue-i18n";
import VideoLists from "@/components/DataList/VideoLists.vue";
import AllArtists from "@/components/DataList/AllArtists.vue";
import Comment from "@/components/Comment/index.vue";
import PageLoadState from "@/components/Navigation/PageLoadState.vue";
import Plyr from "plyr";
import "plyr/dist/plyr.css";
import { lockLandscape, restoreDefaultOrientation } from "@/utils/tauri/platform/screenOrientation";
import { isTauri } from "@/utils/tauri";

const { t } = useI18n();
const router = useRouter();
const music = musicStore();
const setting = settingStore();

// 视频 id
const videoId = ref(router.currentRoute.value.query.id);

// 相似视频
const simiVideo = ref([]);

// 播放器数据
const videoData = ref(null);
const videoRef = ref(null);
const player = ref(null);
const playerOptions = {
  settings: ["quality", "speed"],
  invertTime: false,
  autoplay: false,
  quality: {
    default: 1080,
    options: [1080, 720, 480, 240],
  },
  i18n:
    setting.language === "zh-CN"
      ? {
          play: "播放",
          pause: "暂停",
          speed: "速度",
          settings: "设置",
          normal: "正常",
          quality: "画质",
          pip: "画中画",
          enterFullscreen: "开启全屏",
          exitFullscreen: "退出全屏",
          mute: "音量",
          unmute: "静音",
        }
      : {},
  tooltips: {
    controls: true,
  },
};

// 评论数据
const commentResourceId = computed(() => {
  const id = Number(videoId.value);
  return Number.isFinite(id) && id > 0 ? id : null;
});
const thread = useCommentThread(commentResourceId, "mv");

// 获取视频数据
const getVideoData = (id) => {
  getVideoDetail(id).then((res) => {
    videoData.value = res.data;
    $setSiteTitle(
      res.data.name + " - " + res.data.artists[0].name + " - " + t("general.name.videos"),
    );
    const requests = res.data.brs.map((v) => {
      return getVideoUrl(id, v.br);
    });
    Promise.all(requests)
      .then((results) => {
        const sources = results.map((result) => {
          return {
            src: result.data.url.replace(/^http:/, "https:"),
            type: "video/mp4",
            size: result.data.r,
          };
        });
        player.value.source = {
          type: "video",
          title: videoData.value.name,
          sources,
          poster: videoData.value.cover.replace(/^http:/, "https:"),
        };
      })
      .catch((err) => {
        console.error(t("general.message.userChildren.playlist"), err);
        $message.error(t("general.message.userChildren.playlist"));
      });
    // 请求后回顶
    if (typeof $scrollToTop !== "undefined") $scrollToTop();
  });

  // 获取相似视频
  getSimiVideo(id).then((res) => {
    simiVideo.value = [];
    res.mvs.forEach((v) => {
      simiVideo.value.push({
        id: v.id,
        cover: v.cover,
        name: v.name,
        artist: v.artists,
        playCount: formatNumber(v.playCount),
        duration: getSongTime(v.duration),
      });
    });
  });
};

onMounted(() => {
  // 初始化播放器
  player.value = new Plyr(videoRef.value, playerOptions);
  // 获取视频数据
  getVideoData(videoId.value);
  // 播放器事件
  player.value.on("playing", () => {
    // 隐藏控制条及暂停音乐
    music.setPlayBarState(false);
    music.setPlayState(false);
  });
  // 监听全屏变化，在 Tauri Android 上控制屏幕方向
  if (isTauri()) {
    player.value.on("enterfullscreen", () => {
      lockLandscape();
    });
    player.value.on("exitfullscreen", () => {
      restoreDefaultOrientation();
    });
  }
});

onActivated(() => {
  // keep-alive 复用时也需隐藏控制条
  music.setPlayBarState(false);
});

onDeactivated(() => {
  // keep-alive 缓存时恢复控制条，暂停视频
  music.setPlayBarState(true);
  if (player.value) {
    player.value.pause();
  }
});

onBeforeUnmount(() => {
  // 恢复控制条，销毁播放器
  music.setPlayBarState(true);
  if (player.value) {
    player.value.destroy();
  }
  // 恢复屏幕方向为应用默认（如果之前在视频全屏时锁定了 landscape）
  if (isTauri()) {
    restoreDefaultOrientation();
  }
});

// 监听路由参数变化
watch(
  () => router.currentRoute.value,
  (val) => {
    if (val.name === "video") {
      videoId.value = val.query.id;
      getVideoData(val.query.id);
    }
  },
);
</script>

<style lang="scss" scoped>
.video {
  --plyr-color-main: var(--main-color);
  --plyr-control-radius: 8px;

  display: flex;
  flex-direction: row;
  @media (max-width: 990px) {
    flex-direction: column;
  }
  .mainVideo {
    flex: 1;
    :deep(.plyr) {
      border-radius: var(--radius-md);
      overflow: hidden;
    }
    .info {
      margin-top: 20px;
      .title {
        font-size: 24px;
        font-weight: bold;
      }
      .artists {
        font-size: 16px;
        opacity: 0.8;
        margin-top: 2px;
      }
      .desc {
        margin-top: 8px;
      }
      .num {
        margin-top: 8px;
        display: flex;
        flex-direction: row;
        opacity: 0.8;
        span {
          display: flex;
          flex-direction: row;
          align-items: center;
          cursor: pointer;
          transition: all var(--duration-300) var(--ease-out);
          &::after {
            content: "·";
            margin: 0 6px;
          }
          &:nth-last-child(1) {
            &::after {
              display: none;
            }
          }
          @media (hover: hover) {
            &:hover {
              color: var(--main-color);
            }
          }
          .n-icon {
            margin-right: 6px;
          }
        }
        @media (max-width: 578px) {
          flex-direction: column;
          span {
            &::after {
              display: none;
            }
          }
        }
      }
    }
    .comment-section {
      margin-top: 28px;
      .comment-tab {
        max-width: 360px;
        margin-bottom: 16px;
      }
      .row-skeleton {
        height: 82px;
      }
      .load-more {
        display: flex;
        justify-content: center;
        margin-top: 20px;
      }
    }
  }
  .simiVideo {
    width: 20vw;
    min-width: 200px;
    max-width: 300px;
    margin-left: 30px;
    @media (max-width: 990px) {
      max-width: 100%;
      width: 100%;
      margin-left: 0;
      margin-top: 40px;
    }
    .videolists {
      :deep(.n-grid) {
        @media (min-width: 990px) {
          grid-template-columns: repeat(1, minmax(0px, 1fr)) !important;
        }
      }
    }
  }
}
</style>
