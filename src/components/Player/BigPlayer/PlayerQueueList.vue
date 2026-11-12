<template>
  <n-virtual-list
    ref="listRef"
    v-bind="$attrs"
    :class="['player-queue-list', { reordering: !!drag, settling }]"
    :items="items"
    :item-size="rowHeight"
    :item-resizable="true"
    key-field="key"
    :show-scrollbar="false"
  >
    <template #default="{ item }">
      <div v-if="item.kind === 'section'" class="queue-section">
        <span class="queue-section-title">{{ item.label }}</span>
        <button v-if="item.clear" class="queue-clear" type="button" @click="music.clearPlaylists()">
          {{ $t("player.queue.clear") }}
        </button>
      </div>

      <div v-else-if="item.kind === 'now'" :id="`${rowPrefix}-${item.index}`" class="queue-now">
        <img class="queue-now-cover" :src="cover(item.song, 160)" alt="" />
        <div class="queue-info">
          <div class="queue-name text-hidden">{{ item.song.name }}</div>
          <div class="queue-artists text-hidden">{{ artists(item.song) }}</div>
        </div>
        <div v-if="!music.getPersonalFmMode" class="queue-modes">
          <button
            type="button"
            class="queue-mode"
            :aria-pressed="mode === 'random'"
            :aria-label="$t('player.queue.shuffle')"
            @click="music.setPlaySongMode(mode === 'random' ? 'normal' : 'random')"
          >
            <component :is="mode === 'random' ? IconShuffleOn : IconShuffle" />
          </button>
          <!-- No "repeat off" exists: list and shuffle repeat the queue, single repeats one. -->
          <button
            type="button"
            class="queue-mode"
            aria-pressed="true"
            :aria-label="$t('player.queue.repeat')"
            @click="music.setPlaySongMode(mode === 'single' ? 'normal' : 'single')"
          >
            <component :is="mode === 'single' ? IconRepeatOneOn : IconRepeatOn" />
          </button>
        </div>
      </div>

      <div
        v-else
        :id="`${rowPrefix}-${item.index}`"
        :class="[
          'queue-row',
          { history: item.history, lifted: drag?.from === item.index, released: drag?.released },
        ]"
        :style="rowStyle(item.index)"
      >
        <!-- The row is see-through (it blends with the cover), so the action exists only where a
             swipe has uncovered it, sized to the swipe. -->
        <button
          v-if="swipe?.index === item.index"
          class="queue-row-delete"
          type="button"
          tabindex="-1"
          :style="{ width: `${-swipe.x}px`, transition: swipe.tracking ? 'none' : undefined }"
          @click="remove(item.index)"
        >
          {{ $t("player.queue.remove") }}
        </button>
        <div
          class="queue-row-body"
          :style="bodyStyle(item.index)"
          role="button"
          tabindex="0"
          @pointerdown="startSwipe($event, item.index)"
          @click="select(item.index)"
          @keydown.enter.prevent="select(item.index)"
        >
          <img class="queue-cover" :src="cover(item.song, 96)" alt="" />
          <div class="queue-info">
            <div class="queue-name text-hidden">{{ item.song.name }}</div>
            <div class="queue-artists text-hidden">{{ artists(item.song) }}</div>
          </div>
          <button
            v-if="!touchOnly"
            class="queue-remove"
            type="button"
            :aria-label="$t('player.queue.remove')"
            @click.stop="remove(item.index)"
          >
            <n-icon size="18" :component="DeleteRound" />
          </button>
          <button
            v-if="reorderable && !item.history"
            class="queue-handle"
            type="button"
            :aria-label="$t('player.queue.reorder')"
            @pointerdown.stop="startDrag($event, item.index)"
            @click.stop
          >
            <n-icon size="22" :component="DragHandleRound" />
          </button>
        </div>
      </div>
    </template>
  </n-virtual-list>
</template>

<script setup lang="ts">
/**
 * The big player's queue, as Apple Music lays it out: what has played (dimmed) above, the
 * playing song with the shuffle / repeat toggles, then "Playing Next". Shared by the mobile
 * queue page and the desktop queue panel; each owns its own chrome around it.
 *
 * Only "Playing Next" can be reordered, by the ≡ handle, as in Apple Music. In shuffle the
 * handles are hidden: on Tauri the backend owns the permutation and would put a moved song
 * back (see `moveSong`). Touch removes a song by swiping it left; a mouse gets a delete button.
 */
import { computed, onBeforeUnmount, ref, watch, type CSSProperties } from "vue";
import { NVirtualList } from "naive-ui";
import { DeleteRound, DragHandleRound } from "@vicons/material";
import { musicStore } from "@/store";
import { coverUrl } from "@/utils/coverUrl";
import { isTouchOnly } from "@/utils/touchOnly";
import { prefersReducedMotion } from "@/utils/reducedMotion";
import { locateVirtualRow, type LocateHandle, type VirtualListInst } from "@/utils/locateRow";
import IconShuffle from "../icons/IconShuffle.vue";
import IconShuffleOn from "../icons/IconShuffleOn.vue";
import IconRepeatOn from "../icons/IconRepeatOn.vue";
import IconRepeatOneOn from "../icons/IconRepeatOneOn.vue";
import { useI18n } from "vue-i18n";

defineOptions({ inheritAttrs: false });
const props = withDefaults(defineProps<{ rowPrefix: string; rowHeight?: number }>(), {
  rowHeight: 56,
});

type Song = { id: number; name: string; artist?: { name: string }[]; album?: { picUrl?: string } };
type Item =
  | { kind: "section"; key: string; label: string; clear?: boolean }
  | { kind: "now"; key: string; index: number; song: Song }
  | { kind: "song"; key: string; index: number; song: Song; history: boolean };

const { t } = useI18n();
const music = musicStore();
const listRef = ref<VirtualListInst | null>(null);
const touchOnly = isTouchOnly();

const mode = computed(() => music.persistData.playSongMode);
const current = computed(() => music.persistData.playSongIndex);
// Personal FM owns its own queue; shuffle's order belongs to the backend on Tauri.
const reorderable = computed(() => mode.value !== "random" && !music.getPersonalFmMode);

const cover = (song: Song, size: number) => coverUrl(song.album?.picUrl, size);
const artists = (song: Song) =>
  (song.artist ?? [])
    .filter(Boolean)
    .map((artist) => artist.name)
    .join(" / ");

const items = computed<Item[]>(() => {
  const list: Song[] = music.getPlaylists;
  const at = current.value;
  // Keyed by song, not slot, so a reordered row keeps its element; a repeated song gets a count.
  const seen = new Map<number, number>();
  const keyOf = (song: Song) => {
    const n = (seen.get(song.id) ?? 0) + 1;
    seen.set(song.id, n);
    return `${song.id}#${n}`;
  };
  const rows: Item[] = [];
  if (at > 0) rows.push({ kind: "section", key: "history", label: t("player.queue.history") });
  list.forEach((song, index) => {
    if (index === at) {
      rows.push({ kind: "now", key: keyOf(song), index, song });
      if (index < list.length - 1)
        rows.push({
          kind: "section",
          key: "next",
          label: t("player.queue.playingNext"),
          clear: true,
        });
    } else rows.push({ kind: "song", key: keyOf(song), index, song, history: index < at });
  });
  return rows;
});

const select = (index: number) => {
  if (swipe.value || suppressClick) {
    suppressClick = false;
    closeSwipe();
    return;
  }
  music.selectPlaySongByIndex(index);
};

// ── Drag to reorder ──────────────────────────────────────────────────────────
type Drag = {
  from: number;
  to: number;
  dy: number;
  height: number;
  pointerY: number;
  startY: number;
  startScroll: number;
  released: boolean;
};
const drag = ref<Drag | null>(null);
// One frame with transitions off while the committed order replaces the transforms.
const settling = ref(false);
let autoScroll = 0;
const EDGE = 56; // px from the list edge where dragging scrolls it
const MAX_SCROLL = 14; // px per frame at the very edge

const scroller = () => listRef.value?.getScrollContainer() ?? null;

const rowStyle = (index: number): CSSProperties | undefined => {
  const d = drag.value;
  if (!d) return undefined;
  if (index === d.from) return { transform: `translateY(${d.dy}px)`, zIndex: 2 };
  if (d.from < d.to && index > d.from && index <= d.to)
    return { transform: `translateY(${-d.height}px)` };
  if (d.to < d.from && index >= d.to && index < d.from)
    return { transform: `translateY(${d.height}px)` };
  return undefined;
};

const updateDrag = () => {
  const d = drag.value;
  const container = scroller();
  if (!d || !container) return;
  d.dy = d.pointerY - d.startY + (container.scrollTop - d.startScroll);
  const last = music.getPlaylists.length - 1;
  d.to = Math.min(last, Math.max(current.value + 1, d.from + Math.round(d.dy / d.height)));
};

const scrollStep = () => {
  const d = drag.value;
  const container = scroller();
  if (!d || d.released || !container) return;
  const box = container.getBoundingClientRect();
  const speed =
    d.pointerY < box.top + EDGE
      ? -MAX_SCROLL * ((box.top + EDGE - d.pointerY) / EDGE)
      : d.pointerY > box.bottom - EDGE
        ? MAX_SCROLL * ((d.pointerY - (box.bottom - EDGE)) / EDGE)
        : 0;
  if (speed) {
    container.scrollTop += speed;
    updateDrag();
  }
  autoScroll = requestAnimationFrame(scrollStep);
};

const onDragMove = (event: PointerEvent) => {
  if (!drag.value) return;
  drag.value.pointerY = event.clientY;
  updateDrag();
};

const endDragListeners = () => {
  window.removeEventListener("pointermove", onDragMove);
  window.removeEventListener("pointerup", endDrag);
  window.removeEventListener("pointercancel", endDrag);
  cancelAnimationFrame(autoScroll);
};

const endDrag = () => {
  endDragListeners();
  const d = drag.value;
  if (!d || d.released) return;
  // Settle the lifted row into its slot, then commit; the commit frame runs without transitions
  // so the new order replaces the transforms in place instead of animating from them.
  d.released = true;
  d.dy = (d.to - d.from) * d.height;
  window.setTimeout(
    () => {
      settling.value = true;
      if (d.to !== d.from) music.moveSong(d.from, d.to);
      drag.value = null;
      requestAnimationFrame(() => requestAnimationFrame(() => (settling.value = false)));
    },
    prefersReducedMotion() ? 0 : 160,
  );
};

const startDrag = (event: PointerEvent, index: number) => {
  const row = (event.currentTarget as HTMLElement).closest(".queue-row");
  const container = scroller();
  if (!row || !container || drag.value) return;
  closeSwipe();
  event.preventDefault();
  drag.value = {
    from: index,
    to: index,
    dy: 0,
    height: row.getBoundingClientRect().height,
    pointerY: event.clientY,
    startY: event.clientY,
    startScroll: container.scrollTop,
    released: false,
  };
  window.addEventListener("pointermove", onDragMove);
  window.addEventListener("pointerup", endDrag);
  window.addEventListener("pointercancel", endDrag);
  autoScroll = requestAnimationFrame(scrollStep);
};

// The song changing under a drag moves the "Playing Next" boundary the indices were taken
// against; `moveSong` would refuse the drop anyway, so let go now rather than mid-air.
watch(current, () => {
  if (!drag.value || drag.value.released) return;
  endDragListeners();
  drag.value = null;
});

// ── Swipe left to remove (touch) ─────────────────────────────────────────────
type Swipe = { index: number; x: number; base: number; width: number; tracking: boolean };
const swipe = ref<Swipe | null>(null);
const REVEAL = 88; // px the delete action takes when a row rests open
let swipeStart: { index: number; x: number; y: number; width: number; base: number } | null = null;
let suppressClick = false;

const bodyStyle = (index: number): CSSProperties | undefined => {
  const s = swipe.value;
  if (!s || s.index !== index) return undefined;
  return {
    transform: `translateX(${s.x}px)`,
    transition: s.tracking ? "none" : undefined,
  };
};

const closeSwipe = () => {
  if (swipe.value) swipe.value = { ...swipe.value, x: 0, tracking: false };
  window.setTimeout(() => {
    if (swipe.value?.x === 0) swipe.value = null;
  }, 220);
};

const onSwipeMove = (event: PointerEvent) => {
  const start = swipeStart;
  if (!start) return;
  const dx = event.clientX - start.x;
  const dy = event.clientY - start.y;
  if (!swipe.value?.tracking) {
    if (Math.max(Math.abs(dx), Math.abs(dy)) < 8) return;
    // Vertical: the list (or the page) scrolls; this was never a swipe.
    if (Math.abs(dx) < Math.abs(dy) * 1.2) return void stopSwipeTracking();
    swipe.value = {
      index: start.index,
      x: start.base,
      base: start.base,
      width: start.width,
      tracking: true,
    };
  }
  swipe.value.x = Math.min(0, start.base + dx);
};

const stopSwipeTracking = () => {
  window.removeEventListener("pointermove", onSwipeMove);
  window.removeEventListener("pointerup", endSwipe);
  window.removeEventListener("pointercancel", endSwipe);
  swipeStart = null;
};

const endSwipe = () => {
  const s = swipe.value;
  stopSwipeTracking();
  if (!s?.tracking) return;
  suppressClick = true;
  if (-s.x > s.width * 0.55) {
    // A long swipe deletes outright, the row sliding out first.
    swipe.value = { ...s, x: -s.width, tracking: false };
    window.setTimeout(() => remove(s.index), prefersReducedMotion() ? 0 : 180);
  } else if (-s.x > REVEAL * 0.6) swipe.value = { ...s, x: -REVEAL, tracking: false };
  else closeSwipe();
};

const startSwipe = (event: PointerEvent, index: number) => {
  // Every gesture starts clean: a swipe released off the row never got its click to consume.
  suppressClick = false;
  if (!touchOnly || event.pointerType !== "touch" || drag.value) return;
  if (swipe.value && swipe.value.index !== index) closeSwipe();
  const body = event.currentTarget as HTMLElement;
  swipeStart = {
    index,
    x: event.clientX,
    y: event.clientY,
    width: body.getBoundingClientRect().width,
    base: swipe.value?.index === index ? swipe.value.x : 0,
  };
  window.addEventListener("pointermove", onSwipeMove);
  window.addEventListener("pointerup", endSwipe);
  window.addEventListener("pointercancel", endSwipe);
};

const remove = (index: number) => {
  swipe.value = null;
  music.removeSong(index);
};

// ── Locate ───────────────────────────────────────────────────────────────────
let locateHandle: LocateHandle | null = null;
/** Brings the playing song's card to the top; what has played stays above it. */
const scrollToCurrent = () => {
  const index = current.value;
  if (index < 0 || index >= music.getPlaylists.length) return;
  const position = items.value.findIndex((item) => item.kind === "now");
  if (position < 0) return;
  locateHandle?.cancel();
  locateHandle = locateVirtualRow(listRef.value, position, {
    rowSelector: `#${props.rowPrefix}-${index}`,
    align: "start",
    instant: prefersReducedMotion(),
  });
};

onBeforeUnmount(() => {
  endDrag();
  stopSwipeTracking();
  locateHandle?.cancel();
});

defineExpose({ scrollToCurrent });
</script>

<style lang="scss" scoped>
// Text and icons blend with the cover-coloured background; covers and the delete action do not.
.player-queue-list {
  color: var(--main-cover-mix-color, rgb(239, 239, 239));

  :is(.queue-section, .queue-info, .queue-mode, .queue-remove, .queue-handle) {
    mix-blend-mode: plus-lighter;
  }
}

.queue-section {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 18px 8px 8px;
}

.queue-section-title {
  font-size: 1.05rem;
  font-weight: 700;
}

.queue-clear {
  appearance: none;
  padding: 4px 10px;
  border: 0;
  border-radius: var(--radius-pill);
  background: color-mix(in srgb, var(--main-cover-color, #fff) 14%, transparent);
  color: inherit;
  font: inherit;
  font-size: 0.78rem;
  cursor: pointer;
  opacity: 0.8;

  &:active {
    opacity: 1;
  }
}

.queue-now {
  display: grid;
  grid-template-columns: 56px minmax(0, 1fr) auto;
  align-items: center;
  gap: 12px;
  padding: 10px 8px;
}

.queue-now-cover {
  width: 56px;
  height: 56px;
  border-radius: var(--radius-sm);
  object-fit: cover;
}

.queue-modes {
  display: flex;
  gap: 6px;
}

.queue-mode {
  appearance: none;
  display: flex;
  width: 36px;
  height: 36px;
  padding: 0;
  border: 0;
  background: transparent;
  color: inherit;
  cursor: pointer;
  opacity: 0.55;
  transition:
    opacity var(--duration-200) var(--ease-out),
    transform var(--duration-150) var(--ease-out);

  &[aria-pressed="true"] {
    opacity: 1;
  }

  &:active {
    transform: scale(0.9);
  }

  :deep(svg) {
    width: 100%;
    height: 100%;
  }
}

.queue-row {
  position: relative;
  overflow: hidden;
  border-radius: var(--radius-md);
  transition: transform 0.2s var(--ease-out);

  &.lifted {
    transition: none;
    background: color-mix(in srgb, var(--main-cover-color, #fff) 16%, transparent);
    box-shadow: 0 8px 24px rgb(0 0 0 / 22%);
  }

  &.lifted.released {
    transition: transform 0.16s var(--ease-out);
  }

  &.history .queue-row-body {
    opacity: 0.55;
  }
}

.settling .queue-row,
.settling .queue-row-body {
  transition: none !important;
}

.queue-row-body {
  position: relative;
  min-height: 56px;
  display: grid;
  grid-template-columns: 44px minmax(0, 1fr) auto auto;
  align-items: center;
  gap: 12px;
  padding: 6px 8px;
  box-sizing: border-box;
  cursor: pointer;
  // Vertical pans scroll the list; horizontal ones are the swipe to remove.
  touch-action: pan-y;
  transition: transform 0.22s var(--ease-out);

  &:active {
    background: color-mix(in srgb, var(--main-cover-color, #fff) 8%, transparent);
  }
}

.queue-row-delete {
  position: absolute;
  inset: 0 0 0 auto;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  padding: 0;
  white-space: nowrap;
  transition: width 0.22s var(--ease-out);
  border: 0;
  background: #ff3b30;
  color: #fff;
  font: inherit;
  font-size: 0.9rem;
  font-weight: 600;
}

.queue-cover {
  width: 44px;
  height: 44px;
  border-radius: var(--radius-sm);
  object-fit: cover;
}

.queue-info {
  min-width: 0;
}

.queue-name {
  font-size: 0.92rem;
  font-weight: 600;
  line-height: 1.3;
}

.queue-artists {
  margin-top: 2px;
  font-size: 0.78rem;
  opacity: 0.62;
}

.queue-remove,
.queue-handle {
  appearance: none;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  padding: 0;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: inherit;
  cursor: pointer;
  opacity: 0.55;
}

.queue-handle {
  cursor: grab;
  // The handle takes the whole gesture: no scroll, no swipe.
  touch-action: none;

  .reordering & {
    cursor: grabbing;
  }
}

.queue-remove {
  opacity: 0;

  &:focus-visible {
    opacity: 0.8;
  }
}

@media (hover: hover) {
  .queue-row-body:hover .queue-remove {
    opacity: 0.7;
  }
}
</style>
