<template>
  <div class="comment-page" v-if="songId">
    <!-- 与用户页、搜索页同一套头部：封面 + 小标签 + 大标题，右侧放操作。 -->
    <header class="comment-head">
      <img class="cover" :src="coverUrl(song?.album?.picUrl, 128)" alt="" />
      <div class="text">
        <span class="eyebrow s-label">
          {{ $t("general.name.allComments") }}
          <template v-if="thread.total.value"> · {{ formatNumber(thread.total.value) }}</template>
        </span>
        <h1 class="title text-hidden selectable" @click="router.push(`/song?id=${songId}`)">
          {{ song?.name ?? "" }}
        </h1>
        <AllArtists v-if="song?.artist" class="artists text-hidden" :artistsData="song.artist" />
      </div>
      <n-button
        v-if="playingId && playingId !== songId"
        class="to-current"
        strong
        secondary
        round
        @click="navigation.replacePage({ path: '/comment', query: { id: playingId } })"
      >
        {{ $t("general.name.toCurrentlySong") }}
      </n-button>
    </header>

    <n-tabs class="main-tab comment-tab" type="segment" v-model:value="thread.tab.value">
      <n-tab name="hot" :disabled="!thread.hot.value.length">
        {{ $t("general.name.hotComments") }}
      </n-tab>
      <n-tab name="all">{{ $t("general.name.allComments") }}</n-tab>
    </n-tabs>

    <main class="content">
      <PageLoadState v-if="thread.error.value" error @retry="thread.refresh(true)" />
      <div v-else-if="thread.loading.value" class="comment-stack">
        <n-skeleton v-for="index in 5" :key="index" class="row-skeleton" />
      </div>
      <template v-else>
        <div v-if="thread.visible.value.length" class="comment-stack">
          <Comment
            v-for="item in thread.visible.value"
            :key="item.commentId"
            :commentData="item"
            :resourceId="songId"
          />
        </div>
        <n-text v-else class="empty" :depth="3">{{ $t("general.name.allComments") }} · 0</n-text>
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
    </main>
  </div>
  <div class="title-empty" v-else>
    <span class="key">{{ $t("general.name.noKeywords") }}</span>
    <br />
    <n-button strong secondary @click="navigation.closeTop()" style="margin-top: 20px">
      {{ $t("general.name.goBack") }}
    </n-button>
  </div>
</template>

<script setup lang="ts">
import { useLayerNavigation } from "@/utils/navigation";
import { musicStore } from "@/store";
import { useRouter } from "vue-router";
import { getMusicDetail } from "@/api/song";
import { useI18n } from "vue-i18n";
import { coverUrl } from "@/utils/ncm/coverUrl";
import { formatNumber } from "@/utils/timeTools";
import { useCommentThread } from "@/composables/useCommentThread";
import AllArtists from "@/components/DataList/AllArtists.vue";
import Comment from "@/components/Comment/index.vue";
import PageLoadState from "@/components/Navigation/PageLoadState.vue";

const { t } = useI18n();
const router = useRouter();
const navigation = useLayerNavigation();
const music = musicStore();

const readId = () => {
  const id = Number(router.currentRoute.value.query.id);
  return Number.isFinite(id) && id > 0 ? id : null;
};
const songId = ref<number | null>(readId());
const thread = useCommentThread(songId);

// A local file has no thread to jump to.
const playingId = computed(() => {
  const data = music.getPlaySongData;
  const id = Number(data?.id);
  return !data?.local?.uri && id > 0 ? id : null;
});

const song = ref<{ name: string; artist: any[]; album: any } | null>(null);
watch(
  songId,
  (id) => {
    song.value = null;
    if (!id) return;
    getMusicDetail(String(id))
      .then((res) => {
        const detail = res.songs?.[0];
        if (detail && songId.value === id) {
          song.value = { name: detail.name, artist: detail.ar, album: detail.al };
        }
      })
      .catch((err) => console.error(t("general.message.acquisitionFailed"), err));
  },
  { immediate: true },
);

onMounted(() => $setSiteTitle(t("general.name.allComments")));

// 同一页内换歌（「前往当前播放歌曲」，或从别处再次进入）。
watch(
  () => router.currentRoute.value,
  (route) => {
    if (route.name === "comment") songId.value = readId();
  },
);
</script>

<style lang="scss" scoped>
.comment-page {
  display: flex;
  flex-direction: column;
  padding: 6px 0 32px;

  .comment-head {
    display: flex;
    align-items: center;
    gap: 16px;
    min-width: 0;

    .cover {
      --size: clamp(64px, 9vw, 96px);
      width: var(--size);
      height: var(--size);
      flex-shrink: 0;
      border-radius: var(--radius-md);
      object-fit: cover;
      box-shadow: 0 6px 8px -2px rgb(0 0 0 / 16%);
    }

    .text {
      display: flex;
      flex-direction: column;
      flex: 1;
      min-width: 0;

      .eyebrow {
        margin-bottom: 10px;
        color: var(--n-text-color-3);
      }

      .title {
        margin: 0;
        font-size: clamp(26px, 4.6vw, 50px);
        font-weight: 700;
        line-height: 1.08;
        letter-spacing: -0.02em;
        color: var(--n-text-color);
        cursor: pointer;
      }

      .artists {
        margin-top: 8px;
        font-size: 15px;
        font-weight: 600;
      }
    }

    .to-current {
      flex-shrink: 0;
    }

    @media (max-width: 578px) {
      flex-wrap: wrap;

      .to-current {
        width: 100%;
      }
    }
  }

  .comment-tab {
    margin-top: 22px;
    max-width: 360px;
  }

  .content {
    margin-top: 20px;
  }

  .row-skeleton {
    height: 82px;
  }

  .empty {
    display: block;
    padding: 24px 0;
    text-align: center;
  }

  .load-more {
    display: flex;
    justify-content: center;
    margin-top: 20px;
  }
}

.title-empty {
  margin-top: 30px;
  margin-bottom: 20px;

  .key {
    font-size: 40px;
    font-weight: bold;
  }
}
</style>
