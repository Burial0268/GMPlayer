<template>
  <aside
    ref="panelRef"
    :class="['desktop-queue-panel', { show }]"
    :aria-hidden="!show"
    :inert="!show"
    data-navigation-layer="queue-player"
  >
    <div class="queue-surface" aria-hidden="true"></div>
    <div class="queue-content">
      <PlayerQueueList
        v-if="music.getPlaylists.length"
        ref="queueListRef"
        class="queue-list"
        row-prefix="desktop-queue"
      />
      <div class="queue-empty" v-else>
        {{ $t("other.playlistEmpty") }}
      </div>
    </div>
  </aside>
</template>

<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from "vue";
import { musicStore } from "@/store";
import { useLayerNavigation } from "@/utils/navigation";
import PlayerQueueList from "./PlayerQueueList.vue";

const props = defineProps<{
  show: boolean;
}>();

const music = musicStore();
const queueListRef = ref<InstanceType<typeof PlayerQueueList> | null>(null);
const scrollTimer = ref<number | null>(null);
const scrollCurrentQueueSong = (smooth = false) => queueListRef.value?.scrollToCurrent(smooth);

watch(
  () => [props.show, music.persistData.playSongIndex] as const,
  ([show], [wasShown]) => {
    if (scrollTimer.value) window.clearTimeout(scrollTimer.value);
    if (!show) return;
    // 刚打开就对准：面板一直挂着，滑入过程中看到的已经是当前行。打开期间切歌则稍后跟过去。
    if (!wasShown) nextTick(() => scrollCurrentQueueSong());
    else scrollTimer.value = window.setTimeout(() => scrollCurrentQueueSong(true), 360);
  },
);

// A press anywhere outside closes the panel, as a popover does; the press still lands on what it
// was aimed at. Not the toggle (it would close here, then reopen on its click), nor what Naive
// teleports out of the player (the output menu, dialogs, toasts), nor the window controls.
const navigation = useLayerNavigation();
const panelRef = ref<HTMLElement | null>(null);
const KEEP_OPEN =
  ".desktop-toggle-controls, .v-binder-follower-container, .n-modal-container, .n-drawer-container, .n-message-container, .titlebar-area--floating";
const closeOnOutsidePress = (event: PointerEvent) => {
  const target = event.target;
  if (event.button !== 0 || !(target instanceof Element)) return;
  if (panelRef.value?.contains(target) || target.closest(KEEP_OPEN)) return;
  navigation.closeQueue();
};
watch(
  () => props.show,
  (show) => {
    if (show) window.addEventListener("pointerdown", closeOnOutsidePress, true);
    else window.removeEventListener("pointerdown", closeOnOutsidePress, true);
  },
  { immediate: true },
);

onBeforeUnmount(() => {
  if (scrollTimer.value) window.clearTimeout(scrollTimer.value);
  window.removeEventListener("pointerdown", closeOnOutsidePress, true);
});
</script>

<style lang="scss" scoped>
.desktop-queue-panel {
  position: absolute;
  top: clamp(86px, 10vh, 116px);
  right: clamp(24px, 4vw, 64px);
  bottom: clamp(88px, 12vh, 118px);
  width: clamp(340px, 28vw, 430px);
  box-sizing: border-box;
  pointer-events: none;

  // 面板自己不挂 opacity / transform / filter / z-index：那是 stacking context，
  // 里面的 plus-lighter 在滑入期间混不出去，落定才突然加亮。毛玻璃和文字拆成同级，
  // 各自套同一套显隐；文字自己带混合，自己动不隔断。
  &.show {
    pointer-events: auto;

    .queue-surface {
      opacity: 1;
      transform: none;
    }

    .queue-content {
      --queue-fade: 1;
      inset: 0;
    }
  }
}

// The text must not sit under an opacity or a transform, even mid-animation: either isolates it,
// and its plus-lighter would blend with the panel's transparent box until the panel settled. So
// the content slides by `inset` and fades through `--queue-fade`, which only the blending boxes
// read (PlayerQueueList). The surface holds no blend and animates as usual.
@property --queue-fade {
  syntax: "<number>";
  inherits: true;
  initial-value: 1;
}

.queue-surface,
.queue-content {
  position: absolute;
  border-radius: var(--radius-md);
}

.queue-surface {
  inset: 0;
  opacity: 0;
  transform: translate3d(28px, 0, 0);
  transition:
    opacity 0.32s cubic-bezier(0.25, 1, 0.5, 1),
    transform 0.46s cubic-bezier(0.25, 1, 0.5, 1);
}

.queue-content {
  --queue-fade: 0;
  inset: 0 -28px 0 28px;
  transition:
    --queue-fade 0.32s cubic-bezier(0.25, 1, 0.5, 1),
    inset 0.46s cubic-bezier(0.25, 1, 0.5, 1);
}

// No z-index on either: a z-index makes the content a stacking context, and the rows' plus-lighter
// would blend with its transparent box instead of the cover. DOM order already puts the panel over
// the lyrics column (DesktopPlayerLayout `.right`) and the text over the surface.
.queue-surface {
  background: rgba(20, 20, 20, 0.2);
  border: 1px solid rgba(255, 255, 255, 0.12);
  box-shadow: 0 24px 72px rgba(0, 0, 0, 0.26);
  -webkit-backdrop-filter: blur(36px) saturate(1.2);
  backdrop-filter: blur(36px) saturate(1.2);
  pointer-events: none;
}

.queue-content {
  display: grid;
  grid-template-rows: minmax(0, 1fr);
  padding: 18px 14px 14px;
  box-sizing: border-box;
  color: var(--main-cover-mix-color, rgb(239, 239, 239));
  // 混合不挂在这块上：封面是它的子节点，会跟着被加亮。
  // The list blends its own boxes (PlayerQueueList).
  .queue-empty {
    mix-blend-mode: plus-lighter;
  }
}

.queue-list {
  min-height: 0;
  overflow-x: clip;
  overscroll-behavior: contain;
  padding: 2px 2px 8px;
  // No `contain: paint`: it isolates the list the same way.
}

.queue-empty {
  height: 40vh;
  display: flex;
  align-items: center;
  justify-content: center;
  text-align: center;
  opacity: calc(0.56 * var(--queue-fade, 1));
}

@media (max-width: 1180px) {
  .desktop-queue-panel {
    right: 24px;
    width: min(390px, calc(100vw - 48px));
  }
}
</style>
