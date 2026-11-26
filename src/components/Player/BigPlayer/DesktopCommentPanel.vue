<template>
  <aside class="desktop-comment-panel" aria-live="polite">
    <header class="comment-header">
      <div class="song-summary">
        <Motion
          :key="sharedLayoutIds.cover"
          as-child
          :layout-id="sharedLayoutIds.cover"
          :transition="sharedContentTransition"
        >
          <img :src="coverUrl" alt="cover" />
        </Motion>
        <div class="song-copy">
          <strong>{{ $t("general.name.comment") }}</strong>
          <Motion
            :key="sharedLayoutIds.title"
            as-child
            :layout-id="sharedLayoutIds.title"
            :transition="sharedContentTransition"
          >
            <span class="text-hidden">{{ songName }}</span>
          </Motion>
          <Motion
            :key="sharedLayoutIds.artists"
            as-child
            :layout-id="sharedLayoutIds.artists"
            :transition="sharedContentTransition"
          >
            <small class="text-hidden">{{ artistNames }}</small>
          </Motion>
        </div>
      </div>

      <div class="header-actions">
        <button
          class="icon-action"
          type="button"
          :title="$t('general.name.reload')"
          :aria-label="$t('general.name.reload')"
          :disabled="loading || loadingMore"
          @click="refreshComments"
        >
          <n-icon
            :class="{ spinning: loading, 'motion-essential': loading }"
            size="20"
            :component="RefreshRound"
          />
        </button>
        <button
          class="icon-action"
          type="button"
          :title="$t('desktopLyrics.close')"
          :aria-label="$t('desktopLyrics.close')"
          @click="$emit('close')"
        >
          <n-icon size="21" :component="CloseRound" />
        </button>
      </div>
    </header>

    <div class="comment-tabs" role="tablist" :aria-label="$t('general.name.comment')">
      <button
        type="button"
        role="tab"
        :aria-selected="activeTab === 'hot'"
        :class="{ active: activeTab === 'hot' }"
        @click="activeTab = 'hot'"
      >
        {{ $t("general.name.hotComments") }}
        <span v-if="hotComments.length">{{ hotComments.length }}</span>
      </button>
      <button
        type="button"
        role="tab"
        :aria-selected="activeTab === 'all'"
        :class="{ active: activeTab === 'all' }"
        @click="activeTab = 'all'"
      >
        {{ $t("general.name.allComments") }}
        <span v-if="commentsCount">{{ commentsCount }}</span>
      </button>
    </div>

    <div class="comment-scroll" :aria-busy="loading || !commentsReady">
      <div v-if="loading || !commentsReady" class="comment-grid skeleton-grid">
        <n-skeleton v-for="index in 6" :key="index" class="comment-skeleton" />
      </div>

      <div v-else-if="loadError" class="comment-state">
        <n-icon size="34" :component="ErrorOutlineRound" />
        <span>{{ $t("general.message.acquisitionFailed") }}</span>
        <button type="button" @click="refreshComments">{{ $t("general.name.reload") }}</button>
      </div>

      <template v-else>
        <div v-if="visibleComments.length" class="comment-grid comment-list-stack">
          <n-virtual-list
            ref="scrollRef"
            class="comment-virtual-list"
            :items="visibleComments"
            :item-size="commentItemSize"
            :item-resizable="true"
            key-field="commentId"
            :show-scrollbar="false"
          >
            <template #default="{ item, index }">
              <div
                :class="[
                  'comment-virtual-item',
                  index % 2 === 0 ? 'comment-row-odd' : 'comment-row-even',
                  index === 0 ? 'comment-row-first' : '',
                  index === visibleComments.length - 1 ? 'comment-row-last' : '',
                ]"
              >
                <Comment :commentData="item" :resourceId="songId" :animated="false" />
              </div>
            </template>
          </n-virtual-list>
          <button
            v-if="activeTab === 'all' && hasMore"
            class="load-more"
            type="button"
            :disabled="loadingMore"
            @click="loadMore"
          >
            <n-spin v-if="loadingMore" :size="16" />
            <span>{{ $t("general.name.loadMore") }}</span>
          </button>
        </div>

        <div v-else class="comment-state empty-state">
          <n-icon size="34" :component="ChatBubbleOutlineRound" />
          <span>{{ $t("general.name.allComments") }} · 0</span>
        </div>
      </template>
    </div>
  </aside>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import {
  ChatBubbleOutlineRound,
  CloseRound,
  ErrorOutlineRound,
  RefreshRound,
} from "@vicons/material";
import { useCommentThread } from "@/composables/useCommentThread";
import Comment from "@/components/Comment/index.vue";
import { musicStore } from "@/store";
// Aliased: this file already exposes a `coverUrl` computed of its own.
import { coverUrl as toCoverUrl } from "@/utils/ncm/coverUrl";
import { Motion } from "motion-v";
import { NVirtualList } from "naive-ui";
import { getDesktopPlayerSharedLayoutIds } from "../desktopSharedLayout";

defineEmits<{
  close: [];
}>();

const music = musicStore();
const sharedContentTransition = {
  type: "spring",
  stiffness: 180,
  damping: 42,
  mass: 1.35,
  restDelta: 0.001,
  restSpeed: 0.01,
} as const;
const commentsReady = ref(false);
// n-virtual-list treats `item-size` as the MINIMUM row height and sizes its render
// window from `ceil(listHeight / itemSize + 1)`. Comment rows vary in height and the
// shortest single-line comment is ~73px, so a larger value under-counts how many rows
// fit the viewport and culls still-visible rows early. Keep this at/below the smallest
// possible row height.
const commentItemSize = 72;
const scrollRef = ref<{
  scrollTo: (options: { top?: number; index?: number; behavior?: ScrollBehavior }) => void;
} | null>(null);
let readyGeneration = 0;
let readyTimer: number | null = null;
let readyFrame: number | null = null;

const songId = computed(() => Number(music.getPlaySongData?.id || 0));
/**
 * An imported local file has no Netease comment thread.
 *
 * Checked separately from `songId`, which cannot express it: a local id is a
 * negative hash, so `Number(-N || 0)` is still `-N` and passes every falsy test
 * here.
 */
const isLocalTrack = computed(() => Boolean(music.getPlaySongData?.local?.uri));
const sharedLayoutIds = computed(() => getDesktopPlayerSharedLayoutIds(music.getPlaySongData?.id));
const songName = computed(() => music.getPlaySongData?.name || "");
const artistNames = computed(() =>
  (music.getPlaySongData?.artist || [])
    .map((artist: { name?: string }) => artist.name)
    .filter(Boolean)
    .join(" / "),
);
const coverUrl = computed(() => toCoverUrl(music.getPlaySongData?.album?.picUrl, 96));
// An imported local file has no thread: the composable shows the empty state without asking.
const threadId = computed(() => (isLocalTrack.value ? null : songId.value || null));
const {
  tab: activeTab,
  hot: hotComments,
  visible: visibleComments,
  total: commentsCount,
  hasMore,
  loading,
  loadingMore,
  error: loadError,
  refresh,
  loadMore,
} = useCommentThread(threadId);

const cancelCommentsReady = () => {
  if (readyTimer !== null) {
    window.clearTimeout(readyTimer);
    readyTimer = null;
  }
  if (readyFrame !== null) {
    window.cancelAnimationFrame(readyFrame);
    readyFrame = null;
  }
};

// Let the stage/shared-element animation get a couple of frames before the
// virtual list measures its first rows. This keeps API resolution from forcing
// a large synchronous layout during the hand-off.
const scheduleCommentsReady = () => {
  const generation = readyGeneration;
  cancelCommentsReady();
  if (typeof window === "undefined") {
    commentsReady.value = true;
    return;
  }
  readyTimer = window.setTimeout(() => {
    readyTimer = null;
    readyFrame = window.requestAnimationFrame(() => {
      readyFrame = null;
      if (generation !== readyGeneration) return;
      commentsReady.value = true;
      void nextTick(() => scrollRef.value?.scrollTo({ top: 0 }));
    });
  }, 48);
};

const refreshComments = () => void refresh(true);

// A new track hides the list until its first page has landed and the stage has settled;
// "no thread" never loads, so it is ready at once.
watch(
  threadId,
  (id) => {
    readyGeneration++;
    cancelCommentsReady();
    commentsReady.value = !id;
  },
  { immediate: true },
);
watch(loading, (now, was) => {
  if (!was || now) return;
  // An error shows at once; a list waits for the hand-off (see scheduleCommentsReady).
  if (loadError.value) commentsReady.value = true;
  else scheduleCommentsReady();
});

onBeforeUnmount(cancelCommentsReady);
</script>

<style lang="scss" scoped>
.desktop-comment-panel {
  width: 100%;
  height: 100%;
  min-height: 0;
  position: relative;
  display: grid;
  grid-template-rows: auto auto minmax(0, 1fr);
  color: var(--main-cover-color);
  background: transparent;
  mix-blend-mode: plus-lighter;
  overflow: hidden;
}

.comment-header {
  min-width: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 2px 4px 12px;
}

.song-summary {
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 11px;

  img {
    width: 42px;
    height: 42px;
    flex: 0 0 auto;
    border-radius: var(--radius-sm);
    object-fit: cover;
    box-shadow: 0 6px 18px rgba(0, 0, 0, 0.24);
  }
}

.song-copy {
  min-width: 0;
  display: grid;
  gap: 1px;

  strong {
    font-size: 1rem;
    line-height: 1.2;
  }

  > span {
    max-width: 25vw;
    font-size: 0.82rem;
    font-weight: 600;
    opacity: 0.82;
  }

  small {
    max-width: 25vw;
    font-size: 0.72rem;
    opacity: 0.5;
  }
}

.header-actions {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  gap: 6px;
}

.icon-action {
  appearance: none;
  width: 32px;
  height: 32px;
  display: grid;
  place-items: center;
  padding: 0;
  border: none;
  border-radius: 50%;
  color: inherit;
  background: color-mix(in srgb, var(--main-cover-color) 8%, transparent);
  cursor: pointer;
  opacity: 0.7;
  transition:
    opacity var(--duration-150) var(--ease-out),
    background-color var(--duration-150) var(--ease-out),
    transform var(--duration-150) var(--ease-out);

  &:focus-visible {
    opacity: 1;
    background: color-mix(in srgb, var(--main-cover-color) 14%, transparent);
  }

  @media (hover: hover) {
    &:hover {
      opacity: 1;
      background: color-mix(in srgb, var(--main-cover-color) 14%, transparent);
    }
  }

  &:active {
    transform: scale(0.92);
  }

  &:disabled {
    cursor: default;
    opacity: 0.36;
  }
}

.comment-tabs {
  display: flex;
  align-items: center;
  gap: 20px;
  padding: 0 4px 9px;
  border-bottom: 1px solid color-mix(in srgb, var(--main-cover-color) 9%, transparent);

  button {
    position: relative;
    appearance: none;
    border: none;
    padding: 7px 0;
    color: inherit;
    background: transparent;
    font: inherit;
    font-size: 0.78rem;
    font-weight: 650;
    opacity: 0.46;
    cursor: pointer;
    transition: opacity var(--duration-150) var(--ease-out);

    span {
      margin-left: 4px;
      font-size: 0.68rem;
      font-variant-numeric: tabular-nums;
      opacity: 0.64;
    }

    &::after {
      content: "";
      position: absolute;
      right: 0;
      bottom: 0;
      left: 0;
      height: 2px;
      border-radius: 2px;
      background: currentColor;
      opacity: 0;
      transform: scaleX(0.4);
      transition:
        opacity var(--duration-200) var(--ease-out),
        transform var(--duration-200) var(--ease-out);
    }

    &:focus-visible,
    &.active {
      opacity: 0.94;
    }

    @media (hover: hover) {
      &:hover {
        opacity: 0.94;
      }
    }

    &.active::after {
      opacity: 0.72;
      transform: scaleX(1);
    }
  }
}

.comment-scroll {
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  overscroll-behavior: contain;
  scrollbar-width: thin;
  scrollbar-color: color-mix(in srgb, var(--main-cover-color) 24%, transparent) transparent;
  padding: 12px 4px 28px;
  -webkit-mask-image: linear-gradient(
    to bottom,
    transparent 0,
    #000 14px,
    #000 calc(100% - 28px),
    transparent 100%
  );
  mask-image: linear-gradient(
    to bottom,
    transparent 0,
    #000 14px,
    #000 calc(100% - 28px),
    transparent 100%
  );
}

.comment-grid {
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;

  &.skeleton-grid {
    flex: 1;
  }

  &.comment-list-stack {
    flex: 1;
    gap: 0;
  }

  :deep(.comment-virtual-list) {
    flex: 1;
    width: 100%;
    min-height: 0;
  }

  .comment-virtual-item {
    box-sizing: border-box;
    padding: 4px 6px;
    border-radius: var(--radius-md);
    background-color: color-mix(in srgb, var(--main-cover-color) 7%, transparent);
    transition: background-color var(--duration-150) var(--ease-out);

    &.comment-row-even {
      background-color: color-mix(in srgb, var(--main-cover-color) 12%, transparent);
    }

    &.comment-row-first {
      border-radius: var(--radius-md) var(--radius-md) 0 0;
    }

    &.comment-row-last {
      border-radius: 0 0 var(--radius-md) var(--radius-md);
    }

    @media (hover: hover) {
      &:hover {
        background-color: color-mix(in srgb, var(--main-cover-color) 18%, transparent);
      }
    }

    &.comment-row-odd :deep(.comment) {
      --n-color: transparent !important;
      --n-color-hover: transparent !important;
    }

    &.comment-row-even :deep(.comment) {
      --n-color: transparent !important;
      --n-color-hover: transparent !important;
    }
  }

  :deep(.comment) {
    --n-color: transparent !important;
    --n-color-hover: transparent !important;
    --n-border-color: transparent !important;
    height: auto;
    margin: 0;
    color: var(--main-cover-color);
    border: 0;
    background-color: transparent !important;
    box-shadow: none;
  }

  :deep(.comment .n-card__content) {
    padding: 11px !important;
  }

  :deep(.comment .n-text) {
    color: inherit;
  }

  :deep(.comment .user) {
    min-width: 38px !important;
    width: 38px !important;
    margin-right: 9px !important;
  }

  :deep(.comment .avatar) {
    width: 36px !important;
    height: 36px !important;
  }

  :deep(.comment .associator) {
    display: none !important;
  }

  :deep(.comment .review) {
    min-width: 0;
  }

  :deep(.comment .content) {
    overflow-wrap: anywhere;
    font-size: 0.78rem;
    line-height: 1.48;
  }

  :deep(.comment .beReplied) {
    font-size: 0.72rem !important;
    background: color-mix(in srgb, var(--main-cover-color) 7%, transparent) !important;
  }

  :deep(.comment .thing) {
    margin-top: 8px !important;
    font-size: 0.68rem;
  }

  :deep(.comment .thing .item) {
    margin-right: 7px !important;
    font-size: 0.68rem !important;
  }
}

.comment-skeleton {
  height: 116px;
  border-radius: var(--radius-md);
  opacity: 0.42;
}

.comment-state {
  min-height: 0;
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  text-align: center;
  opacity: 0.58;

  button {
    appearance: none;
    border: 1px solid color-mix(in srgb, var(--main-cover-color) 16%, transparent);
    border-radius: var(--radius-sm);
    padding: 7px 12px;
    color: inherit;
    background: color-mix(in srgb, var(--main-cover-color) 8%, transparent);
    cursor: pointer;
  }
}

.load-more {
  appearance: none;
  min-width: 112px;
  min-height: 34px;
  margin: 14px auto 0;
  padding: 7px 14px;
  border: 1px solid color-mix(in srgb, var(--main-cover-color) 12%, transparent);
  border-radius: var(--radius-sm);
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: inherit;
  background: color-mix(in srgb, var(--main-cover-color) 7%, transparent);
  font: inherit;
  font-size: 0.76rem;
  cursor: pointer;
  opacity: 0.68;
  transition:
    opacity var(--duration-150) var(--ease-out),
    background-color var(--duration-150) var(--ease-out);

  &:focus-visible {
    opacity: 1;
    background: color-mix(in srgb, var(--main-cover-color) 12%, transparent);
  }

  @media (hover: hover) {
    &:hover {
      opacity: 1;
      background: color-mix(in srgb, var(--main-cover-color) 12%, transparent);
    }
  }
}

// .motion-essential：reduced-motion 下仍要转，它是「正在刷新」的唯一提示
.spinning {
  animation: comment-spin 0.8s linear infinite;
}

@keyframes comment-spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
