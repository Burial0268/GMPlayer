<template>
  <div v-if="mounted" :class="['action-sheet', { closing: !show }]">
    <Motion class="action-sheet-scrim" :style="scrimStyle" @click="show = false" />
    <Motion
      ref="sheetRef"
      class="action-sheet-panel"
      role="dialog"
      aria-modal="true"
      :aria-label="$t('player.controls.more')"
      :style="sheetStyle"
      @touchstart.passive="drag.handleTouchStart"
      @touchmove="drag.handleTouchMove"
      @touchend.passive="drag.handleTouchEnd"
      @touchcancel.passive="drag.handleTouchCancel"
    >
      <button
        class="sheet-grip"
        type="button"
        :aria-label="$t('general.dialog.cancel')"
        @click="drag.handleGripClick"
      >
        <span class="sheet-grip-bar" />
      </button>

      <div v-if="song" class="sheet-song">
        <img class="sheet-cover" :src="coverUrl(song.album?.picUrl, 120)" alt="" />
        <div class="sheet-song-text">
          <div class="sheet-song-name text-hidden">{{ song.name }}</div>
          <div class="sheet-song-artists text-hidden">{{ artistNames }}</div>
        </div>
      </div>

      <div class="sheet-scroll">
        <section class="sheet-group">
          <div class="sheet-row">
            <n-icon class="row-icon" size="22" :component="AvTimerRound" />
            <span class="row-label">{{ $t("setting.lyricOffset") }}</span>
            <div class="stepper">
              <button
                type="button"
                :aria-label="$t('setting.lyricOffsetDelay')"
                @click="adjustOffset(-LYRIC_OFFSET_STEP)"
              >
                <n-icon size="18" :component="RemoveRound" />
              </button>
              <button
                type="button"
                :class="['stepper-value', { changed: offset !== 0 }]"
                :aria-label="$t('setting.lyricOffsetReset')"
                @click="resetOffset"
              >
                {{ offsetDisplay }}
              </button>
              <button
                type="button"
                :aria-label="$t('setting.lyricOffsetAdvance')"
                @click="adjustOffset(LYRIC_OFFSET_STEP)"
              >
                <n-icon size="18" :component="AddRound" />
              </button>
            </div>
          </div>
          <div v-if="languageOptions.length" class="sheet-row language-row">
            <n-icon class="row-icon" size="22" :component="TranslateRound" />
            <span class="row-label">{{ $t("setting.lyricTranslationLanguage") }}</span>
            <div class="chips">
              <button
                v-for="option in languageOptions"
                :key="option.value"
                type="button"
                :class="['chip', { active: selectedLanguage === option.value }]"
                :aria-pressed="selectedLanguage === option.value"
                @click="selectedLanguage = option.value"
              >
                {{ option.label }}
              </button>
            </div>
          </div>
        </section>

        <section v-if="actions.length" class="sheet-group">
          <button
            v-for="action in actions"
            :key="action.key"
            type="button"
            class="sheet-row"
            @click="action.run"
          >
            <n-icon class="row-icon" size="22" :component="action.icon" />
            <span class="row-label text-hidden">{{ action.label }}</span>
            <n-icon
              v-if="action.page"
              class="row-chevron"
              size="20"
              :component="ChevronRightRound"
            />
          </button>
        </section>
      </div>
    </Motion>
  </div>
  <!-- Loaded on first use, as in DataLists: both open through a method on their ref. -->
  <component :is="AddPlaylistComponent" v-if="AddPlaylistComponent" ref="addPlaylistRef" />
  <component :is="DownloadSongComponent" v-if="DownloadSongComponent" ref="downloadSongRef" />
</template>

<script setup lang="ts">
/**
 * 大播放器（移动端）的「更多」面板：歌词偏移 / 翻译语言 + 当前歌曲的操作。
 *
 * 外观和手势与 PlayListSheet 同一套（玻璃面板、抓手、跟手下拉、遮罩随进度），手势本身
 * 共用 `useSheetDrag`。挂在 BigPlayer 根上而不是 MobilePlayerLayout 里：后者整页在
 * plus-lighter 混合组中，面板放进去会被加亮成一块发白的玻璃。
 * 系统返回 / Escape 由 useLayerPresentation 识别 `.action-sheet` 后派发 Escape 关闭。
 */
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, watch, type Component } from "vue";
import type { RouteLocationRaw } from "vue-router";
import { useI18n } from "vue-i18n";
import { animate, Motion, useMotionValue, useTransform } from "motion-v";
import {
  AddRound,
  AlbumRound,
  AvTimerRound,
  ChevronRightRound,
  DownloadRound,
  InfoRound,
  LinkRound,
  PersonRound,
  PlaylistAddRound,
  RemoveRound,
  SmartDisplayRound,
  TranslateRound,
} from "@vicons/material";
import { musicStore } from "@/store";
import { coverUrl } from "@/utils/coverUrl";
import { layerMotion } from "@/utils/navigation/motion";
import { prefersReducedMotion } from "@/utils/reducedMotion";
import { LYRIC_OFFSET_STEP, useLyricControls } from "@/composables/useLyricControls";
import { useSheetDrag } from "@/composables/useSheetDrag";

const show = defineModel<boolean>("show", { required: true });
const emit = defineEmits<{ openPage: [to: RouteLocationRaw] }>();

const { t } = useI18n();
const music = musicStore();
const { languageOptions, selectedLanguage, offset, offsetDisplay, adjustOffset, resetOffset } =
  useLyricControls();

const song = computed(() => music.getPlaySongData);
const artistNames = computed(() =>
  (song.value?.artist ?? []).map((artist) => artist?.name).join(" / "),
);

// ── 开合：与 PlayListSheet 相同的 progress 驱动，收起动画跑完才卸载 ──
const mounted = ref(false);
const sheetRef = ref<{ $el: HTMLElement } | null>(null);
const progress = useMotionValue(0);
const sheetHeight = useMotionValue(0);
const travel = () => sheetHeight.get() || window.innerHeight || 1;
const sheetY = useTransform(() => (1 - Math.min(1, Math.max(0, progress.get()))) * travel());
const scrimOpacity = useTransform(() => Math.min(1, Math.max(0, progress.get())));
const sheetStyle = { y: sheetY };
const scrimStyle = { opacity: scrimOpacity };
let settle: ReturnType<typeof animate> | null = null;

const stopSettle = () => {
  settle?.stop();
  settle = null;
};
const settleTo = (target: 0 | 1) => {
  stopSettle();
  if (prefersReducedMotion()) {
    progress.set(target);
    if (!target) mounted.value = false;
    return;
  }
  const animation = animate(progress, target, {
    ...layerMotion.settle,
    onComplete: () => {
      if (settle !== animation) return;
      settle = null;
      if (!target) mounted.value = false;
    },
  });
  settle = animation;
};

const drag = useSheetDrag({
  progress,
  travel,
  stop: stopSettle,
  settleOpen: () => settleTo(1),
  dismiss: () => (show.value = false),
  scrollers: ".sheet-scroll",
  grip: ".sheet-grip",
});

watch(show, async (open) => {
  drag.reset();
  if (!open) return settleTo(0);
  if (!mounted.value) {
    progress.set(0);
    mounted.value = true;
  }
  await nextTick();
  const height = sheetRef.value?.$el?.clientHeight;
  if (height) sheetHeight.set(height);
  if (show.value) settleTo(1);
});

// Escape（系统返回经 useLayerPresentation 也派发 Escape）收起面板。
const onKeydown = (event: KeyboardEvent) => {
  if (event.key !== "Escape" || !show.value || event.defaultPrevented) return;
  event.preventDefault();
  show.value = false;
};
watch(show, (open) =>
  open
    ? document.addEventListener("keydown", onKeydown)
    : document.removeEventListener("keydown", onKeydown),
);
onBeforeUnmount(() => {
  stopSettle();
  document.removeEventListener("keydown", onKeydown);
});

// ── 歌曲操作 ──
type Action = { key: string; icon: Component; label: string; run: () => void; page?: boolean };

const go = (to: RouteLocationRaw) => {
  show.value = false;
  emit("openPage", to);
};
const now = (action: (target: NonNullable<typeof song.value>) => void) => () => {
  const target = song.value;
  show.value = false;
  if (target) action(target);
};

const actions = computed<Action[]>(() => {
  const target = song.value;
  if (!target) return [];
  const local = Boolean(target.local?.uri);
  const list: Action[] = [];
  if (!local) {
    list.push(
      { key: "add", icon: PlaylistAddRound, label: t("menu.add"), run: now(addToPlaylist) },
      { key: "download", icon: DownloadRound, label: t("menu.download"), run: now(download) },
    );
  }
  list.push({
    key: "detail",
    icon: InfoRound,
    label: t("menu.songDetail"),
    page: true,
    run: () =>
      go(
        local
          ? { path: "/local/song", query: { key: String(target.local.uri) } }
          : { path: "/song", query: { id: target.id } },
      ),
  });
  if (target.mv) {
    list.push({
      key: "mv",
      icon: SmartDisplayRound,
      label: t("menu.mv"),
      page: true,
      run: () => go({ path: "/video", query: { id: target.mv } }),
    });
  }
  if (!local) {
    list.push({
      key: "copy",
      icon: LinkRound,
      label: t("menu.copy", { name: t("general.name.song"), other: t("general.name.link") }),
      run: now(copyLink),
    });
  }
  // 本地曲目的歌手和专辑来自文件标签：id 是 0，没有可去的网易页面。
  for (const artist of target.artist ?? []) {
    if (!artist || !(Number(artist.id) > 0)) continue;
    list.push({
      key: `artist-${artist.id}`,
      icon: PersonRound,
      label: artist.name,
      page: true,
      run: () => go({ path: "/artist/songs", query: { id: artist.id } }),
    });
  }
  if (Number(target.album?.id) > 0) {
    list.push({
      key: "album",
      icon: AlbumRound,
      label: target.album.name,
      page: true,
      run: () => go({ path: "/album", query: { id: target.album.id } }),
    });
  }
  return list;
});

const addPlaylistRef = ref();
const downloadSongRef = ref();
const AddPlaylistComponent = shallowRef();
const DownloadSongComponent = shallowRef();
const openLazy = async (holder, load, instance, invoke) => {
  if (!holder.value) {
    holder.value = (await load()).default;
    await nextTick();
  }
  invoke(instance.value);
};
const addToPlaylist = (target) =>
  openLazy(
    AddPlaylistComponent,
    () => import("@/components/DataModal/AddPlaylist.vue"),
    addPlaylistRef,
    (modal) => modal?.openAddToPlaylist(target.id),
  );
const download = (target) =>
  openLazy(
    DownloadSongComponent,
    () => import("@/components/DataModal/DownloadSong.vue"),
    downloadSongRef,
    (modal) => modal?.openDownloadModal(target),
  );
const copyLink = async (target) => {
  try {
    await navigator.clipboard.writeText(`https://music.163.com/#/song?id=${target.id}`);
    $message.success(t("general.message.copySuccess"));
  } catch (err) {
    console.error(t("general.message.copyFailure"), err);
    $message.error(t("general.message.copyFailure"));
  }
};
</script>

<style lang="scss" scoped>
// 外壳与 PlayListSheet 一致；颜色走 :root 令牌，因为 BigPlayer teleport 到 body，
// 继承不到 naive 写在组件根上的 --n-text-color。
.action-sheet {
  position: fixed;
  inset: 0;
  z-index: 100;
  color: var(--text-primary);
  --sheet-fill: color-mix(in srgb, var(--text-primary) 6%, transparent);
  --sheet-separator: color-mix(in srgb, var(--text-primary) 10%, transparent);

  &.closing {
    pointer-events: none;
  }
}

.action-sheet-scrim {
  position: absolute;
  inset: 0;
  background-color: rgb(0 0 0 / 42%);
  will-change: opacity;
}

.action-sheet-panel {
  position: absolute;
  right: 0;
  bottom: 0;
  left: 0;
  max-height: calc(100vh - 84px);
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border-radius: 16px 16px 0 0;
  background-color: rgba(var(--app-shell-rgb, 242, 242, 244), 0.86);
  -webkit-backdrop-filter: blur(26px) saturate(180%);
  backdrop-filter: blur(26px) saturate(180%);
  box-shadow:
    0 -18px 46px rgb(0 0 0 / 14%),
    inset 0 1px 0 rgb(255 255 255 / 40%);
  will-change: transform;
}

.sheet-grip {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  justify-content: center;
  height: 26px;
  padding: 0;
  border: 0;
  background: transparent;
  cursor: pointer;
  touch-action: none;

  &:focus-visible {
    outline: none;
    border-radius: var(--radius-pill);
    box-shadow: var(--focus-ring);
  }
}

.sheet-grip-bar {
  width: 36px;
  height: 4px;
  border-radius: var(--radius-pill);
  background-color: color-mix(in srgb, var(--text-primary) 24%, transparent);
}

.sheet-song {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 4px 20px 14px;
}

.sheet-cover {
  width: 48px;
  height: 48px;
  flex-shrink: 0;
  border-radius: var(--radius-sm);
  object-fit: cover;
  background: var(--sheet-fill);
}

.sheet-song-text {
  min-width: 0;
}

.sheet-song-name {
  font-size: 1rem;
  font-weight: 600;
  line-height: 1.3;
}

.sheet-song-artists {
  margin-top: 2px;
  font-size: 0.82rem;
  color: var(--text-secondary);
}

.sheet-scroll {
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
  padding: 0 16px calc(16px + var(--app-safe-area-bottom, 0px));
}

// 分组卡片 + 行间细线，同一种行高与图标列，和设置页的分组列表同一语法。
.sheet-group {
  overflow: hidden;
  border-radius: var(--radius-md);
  background: var(--sheet-fill);

  & + & {
    margin-top: 12px;
  }
}

.sheet-row {
  appearance: none;
  position: relative;
  width: 100%;
  min-height: 52px;
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 0 14px;
  border: 0;
  background: transparent;
  color: inherit;
  font: inherit;
  font-size: 0.95rem;
  text-align: left;
  box-sizing: border-box;

  // 细线从文字起点开始，不穿过图标列。
  & + &::before {
    content: "";
    position: absolute;
    top: 0;
    right: 0;
    left: 50px;
    height: 1px;
    background: var(--sheet-separator);
  }

  &:is(button) {
    cursor: pointer;
    transition: background-color var(--duration-150) var(--ease-out);

    &:active {
      background: var(--sheet-fill);
    }
  }

  &:focus-visible {
    outline: none;
    box-shadow: inset var(--focus-ring);
  }
}

.row-icon {
  flex-shrink: 0;
  color: var(--main-color);
}

.row-label {
  flex: 1;
  min-width: 0;
}

.row-chevron {
  flex-shrink: 0;
  color: var(--text-secondary);
  opacity: 0.6;
}

.stepper {
  display: flex;
  align-items: center;
  flex-shrink: 0;
  border-radius: var(--radius-pill);
  background: var(--sheet-fill);

  button {
    appearance: none;
    display: flex;
    align-items: center;
    justify-content: center;
    min-width: 36px;
    height: 32px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius-pill);
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;

    &:active {
      background: var(--sheet-separator);
    }
  }

  .stepper-value {
    min-width: 56px;
    font-variant-numeric: tabular-nums;
    color: var(--text-secondary);

    &.changed {
      color: var(--main-color);
      font-weight: 600;
    }
  }
}

// 语言可能有好几种：标签换到下一行，选项横向排开，不挤占标题。
.language-row {
  flex-wrap: wrap;
  row-gap: 8px;
  padding-top: 12px;
  padding-bottom: 12px;
}

.chips {
  flex-basis: 100%;
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  padding-left: 36px;
}

.chip {
  appearance: none;
  height: 30px;
  padding: 0 12px;
  border: 0;
  border-radius: var(--radius-pill);
  background: var(--sheet-fill);
  color: inherit;
  font: inherit;
  font-size: 0.82rem;
  cursor: pointer;

  &.active {
    background: var(--main-color);
    color: #fff;
  }
}
</style>
