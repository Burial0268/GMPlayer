<template>
  <div class="player-queue">
    <!-- The playing song stays put; only the list under it scrolls. -->
    <div v-if="playing" class="queue-now">
      <img class="queue-now-cover" :src="cover(playing, 160)" alt="" />
      <div class="queue-info">
        <div class="queue-name text-hidden">{{ playing.name }}</div>
        <div class="queue-artists text-hidden">{{ artists(playing) }}</div>
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
        <!-- The modes are exclusive (list loop, shuffle, single loop), so exactly one of the two
             is lit: repeat is off in shuffle, and from shuffle it switches to list loop. -->
        <button
          type="button"
          class="queue-mode"
          :aria-pressed="mode !== 'random'"
          :aria-label="$t('player.queue.repeat')"
          @click="music.setPlaySongMode(mode === 'normal' ? 'single' : 'normal')"
        >
          <component
            :is="
              mode === 'single' ? IconRepeatOneOn : mode === 'normal' ? IconRepeatOn : IconRepeat
            "
          />
        </button>
      </div>
    </div>
    <div
      ref="scrollRef"
      :class="['player-queue-list', 'player-queue-scroll', { reordering: !!drag, settling }]"
      @scroll="onListScroll"
    >
      <!-- Every section is in the DOM so its title can be natively sticky: the browser keeps it
           under the playing song and lets the next title push it out on the compositor, in step
           with the scroll. Only the rows are windowed, with spacers for the rest. -->
      <section
        v-for="group in windowed"
        :key="group.key"
        :id="group.anchor ? `${rowPrefix}-upcoming` : undefined"
        class="queue-group"
      >
        <div class="queue-section">
          <span class="queue-section-title">{{ group.label }}</span>
          <button
            v-if="group.clear"
            class="queue-clear"
            type="button"
            @click="music.clearUpcoming()"
          >
            {{ $t("player.queue.clear") }}
          </button>
        </div>
        <div :style="{ height: `${group.before * ROW}px` }" aria-hidden="true"></div>
        <div
          v-for="item in group.rows"
          :key="item.key"
          :id="item.history ? undefined : `${rowPrefix}-${item.index}`"
          :class="[
            'queue-row',
            {
              history: item.history,
              lifted: drag?.from === item.index,
              released: drag?.released,
            },
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
            @pointerdown="!item.history && startSwipe($event, item.index)"
            @click="tap(item)"
            @keydown.enter.prevent="tap(item)"
          >
            <img class="queue-cover" :src="cover(item.song, 96)" alt="" loading="lazy" />
            <!-- Everything but the cover in one box, which is the row's only blend and fade. -->
            <div class="queue-row-text">
              <div class="queue-info">
                <div class="queue-name text-hidden">{{ item.song.name }}</div>
                <div class="queue-artists text-hidden">{{ artists(item.song) }}</div>
              </div>
              <button
                v-if="!touchOnly && !item.history"
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
        </div>
        <div :style="{ height: `${group.after * ROW}px` }" aria-hidden="true"></div>
      </section>
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * The big player's queue, as Apple Music lays it out. Shared by the mobile queue page and the
 * desktop queue panel; each owns its own chrome around it. Top to bottom:
 *
 * - History: songs actually played (`playHistory`), oldest first, so the most recent sits just
 *   above the playing song. Not queue positions: the songs before the cursor were not
 *   necessarily played (start an album at track 5), and the queue plays them after its end.
 * - The playing song, with the shuffle / repeat toggles.
 * - Playing Next: the songs after the cursor; then "from the top", the songs before it, which
 *   list and shuffle repeat reach after the end and Next wraps to. Each reorders within itself
 *   by its ≡ handle (`moveSong`). In shuffle the handles are hidden: on Tauri the backend owns
 *   the permutation and would put a moved song back. Clear empties both and keeps the playing
 *   song. Touch removes a song by swiping it left; a mouse gets a delete button.
 */
import { computed, onBeforeUnmount, onMounted, ref, watch, type CSSProperties } from "vue";
import { DeleteRound, DragHandleRound } from "@vicons/material";
import { musicStore } from "@/store";
import { coverUrl } from "@/utils/coverUrl";
import { isTouchOnly } from "@/utils/touchOnly";
import { prefersReducedMotion } from "@/utils/reducedMotion";
import IconShuffle from "../icons/IconShuffle.vue";
import IconShuffleOn from "../icons/IconShuffleOn.vue";
import IconRepeat from "../icons/IconRepeat.vue";
import IconRepeatOn from "../icons/IconRepeatOn.vue";
import IconRepeatOneOn from "../icons/IconRepeatOneOn.vue";
import { useI18n } from "vue-i18n";

const emit = defineEmits<{ scroll: [event: Event] }>();
defineProps<{ rowPrefix: string }>();

// Fixed heights, so a group's offset and each window are arithmetic, not measurement; the
// stylesheet pins `.queue-section` and `.queue-row` to these.
const ROW = 56;
const HEADER = 52;
// Rows rendered beyond each edge of the viewport.
const BUFFER = 8;

type Song = { id: number; name: string; artist?: { name: string }[]; album?: { picUrl?: string } };
// Enough to look back with; the store keeps about a hundred.
const HISTORY_LIMIT = 20;

type Row = { key: string; index: number; song: Song; history: boolean };
// `anchor`: the first upcoming section, which opening the queue puts under the card.
type Group = { key: string; label: string; clear?: boolean; anchor?: boolean; rows: Row[] };

const { t } = useI18n();
const music = musicStore();
const scrollRef = ref<HTMLElement | null>(null);
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

const playing = computed<Song | undefined>(() => music.getPlaylists[current.value]);

const groups = computed<Group[]>(() => {
  const list: Song[] = music.getPlaylists;
  const at = current.value;
  const nowPlaying = playing.value;
  // Keyed by song, not slot, so a reordered row keeps its element; a repeated song gets a count.
  const seen = new Map<number, number>();
  const keyOf = (song: Song) => {
    const n = (seen.get(song.id) ?? 0) + 1;
    seen.set(song.id, n);
    return `${song.id}#${n}`;
  };
  const queueRow = (song: Song, index: number): Row => ({
    key: keyOf(song),
    index,
    song,
    history: false,
  });
  const result: Group[] = [];

  const history = (music.getPlayHistory as Song[])
    .filter((song) => song.id !== nowPlaying?.id)
    .slice(0, HISTORY_LIMIT)
    .reverse();
  if (history.length)
    result.push({
      key: "history",
      label: t("player.queue.history"),
      rows: history.map((song) => ({
        key: `history-${song.id}`,
        index: -1,
        song,
        history: true,
      })),
    });
  if (!nowPlaying) return result;

  const after = list.slice(at + 1);
  const before = list.slice(0, at);
  if (after.length)
    result.push({
      key: "next",
      label: t("player.queue.playingNext"),
      clear: true,
      anchor: true,
      rows: after.map((song, offset) => queueRow(song, at + 1 + offset)),
    });
  if (before.length)
    result.push({
      key: "from-top",
      label: t("player.queue.fromTop"),
      clear: !after.length,
      anchor: !after.length,
      rows: before.map((song, index) => queueRow(song, index)),
    });
  return result;
});

const groupTop = (index: number) =>
  groups.value.slice(0, index).reduce((top, group) => top + HEADER + group.rows.length * ROW, 0);

// ── Windowing ────────────────────────────────────────────────────────────────
// Quantised to whole rows, so scrolling re-renders only when a row crosses the edge.
const firstVisibleRow = ref(0);
const visibleRows = ref(16);

const windowed = computed(() => {
  const from = (firstVisibleRow.value - BUFFER) * ROW;
  const to = (firstVisibleRow.value + visibleRows.value + BUFFER) * ROW;
  let top = 0;
  return groups.value.map((group) => {
    const rowsTop = top + HEADER;
    const count = group.rows.length;
    top = rowsTop + count * ROW;
    const first = Math.min(count, Math.max(0, Math.floor((from - rowsTop) / ROW)));
    const last = Math.min(count, Math.max(first, Math.ceil((to - rowsTop) / ROW)));
    return { ...group, rows: group.rows.slice(first, last), before: first, after: count - last };
  });
});

const measureWindow = () => {
  const scroller = scrollRef.value;
  if (!scroller) return;
  const first = Math.floor(scroller.scrollTop / ROW);
  const visible = Math.ceil(scroller.clientHeight / ROW) + 1;
  if (first !== firstVisibleRow.value) firstVisibleRow.value = first;
  if (visible !== visibleRows.value) visibleRows.value = visible;
};

/**
 * Rows fade as they slide under the sticky title, as Apple's do. Where the browser has
 * scroll-driven animations (the stylesheet's `view()` timeline) that runs on the compositor and
 * this does nothing; elsewhere it fades the two or three rows at the edge, found by hit-testing
 * rather than walking the list. Per row, not a mask: a mask on the list would cut every row's
 * plus-lighter off from the background.
 */
const scrollFade =
  typeof CSS !== "undefined" && CSS.supports?.("animation-timeline: view()") === true;
const fadedRows = new Set<HTMLElement>();
const fadeFallback = () => {
  const scroller = scrollRef.value;
  if (scrollFade || !scroller) return;
  const box = scroller.getBoundingClientRect();
  const edge = box.top + HEADER;
  const hit = new Set<HTMLElement>();
  for (const y of [edge - 48, edge - 24, edge - 2]) {
    if (y < box.top) continue;
    for (const node of document.elementsFromPoint(box.left + 24, y)) {
      const row = node.closest<HTMLElement>(".queue-row");
      if (row && scroller.contains(row)) hit.add(row);
    }
  }
  for (const row of fadedRows) if (!hit.has(row)) row.style.removeProperty("--row-opacity");
  fadedRows.clear();
  for (const row of hit) {
    const rowBox = row.getBoundingClientRect();
    // Gone by the time 40% of it is under the title, so its text never sits on the title's.
    const shown = Math.max(0, Math.min(1, 1 - (edge - rowBox.top) / (rowBox.height * 0.4)));
    if (shown < 1 && !row.classList.contains("lifted")) {
      // The variable, not the row's opacity, which would isolate its plus-lighter.
      const resting = row.classList.contains("history") ? 0.55 : 1;
      row.style.setProperty("--row-opacity", String(shown * resting));
      fadedRows.add(row);
    } else row.style.removeProperty("--row-opacity");
  }
};

const onListScroll = (event: Event) => {
  emit("scroll", event);
  measureWindow();
  fadeFallback();
};

let resizeObserver: ResizeObserver | null = null;
onMounted(() => {
  measureWindow();
  if (scrollRef.value) {
    resizeObserver = new ResizeObserver(measureWindow);
    resizeObserver.observe(scrollRef.value);
  }
});

const tap = (item: Row) => {
  // A tap while a row rests open only closes it, as on iOS.
  if (swipe.value || suppressClick) {
    suppressClick = false;
    closeSwipe();
    return;
  }
  // A history song plays from where it is in the queue, or joins the end of it.
  if (item.history) music.addSongToPlaylists(item.song as never, true);
  else music.selectPlaySongByIndex(item.index);
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

const scroller = () => scrollRef.value;

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
  // A row moves within its own section: after the playing song, or "from the top" before it.
  const [low, high] =
    d.from > current.value
      ? [current.value + 1, music.getPlaylists.length - 1]
      : [0, current.value - 1];
  d.to = Math.min(high, Math.max(low, d.from + Math.round(d.dy / d.height)));
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
/**
 * Puts what plays next right under the playing song, history a scroll up away. Instant when the
 * queue opens (Apple Music opens in place); `smooth` follows a song change while it is open.
 * With nothing upcoming, the end of the history is what shows.
 */
const scrollToCurrent = (smooth = false) => {
  const scroller = scrollRef.value;
  if (!scroller) return;
  const anchor = groups.value.findIndex((group) => group.anchor);
  const top = anchor < 0 ? scroller.scrollHeight : groupTop(anchor);
  scroller.scrollTo({ top, behavior: smooth && !prefersReducedMotion() ? "smooth" : "instant" });
  measureWindow();
};

onBeforeUnmount(() => {
  resizeObserver?.disconnect();
  endDrag();
  stopSwipeTracking();
});

defineExpose({ scrollToCurrent });
</script>

<style lang="scss" scoped>
// Text and icons blend with the cover-coloured background; covers and the delete action do not.
// As few blending boxes as possible, never one inside another: each is an offscreen pass every
// frame, and one nested in a blending parent only blends with that parent's transparent group.
// `--queue-fade` is the desktop panel's show/hide fade, carried by the blending boxes themselves
// because an opacity on any ancestor would isolate them (DesktopQueuePanel).
.player-queue {
  display: flex;
  flex-direction: column;
  min-height: 0;
  color: var(--main-cover-mix-color, rgb(239, 239, 239));

  :is(.queue-section, .queue-now .queue-info, .queue-mode, .queue-row-text) {
    mix-blend-mode: plus-lighter;
  }

  :is(.queue-section, .queue-now-cover, .queue-now .queue-info) {
    opacity: var(--queue-fade, 1);
  }
}

.player-queue-list {
  position: relative;
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
  overscroll-behavior: contain;
  scrollbar-width: none;

  &::-webkit-scrollbar {
    display: none;
  }
}

// Sticky within its own group, so the next group's title pushes it out; positioned by the
// browser on the compositor, in step with the scroll, which a scripted copy could not be.
.queue-section {
  position: sticky;
  top: 0;
  z-index: 2;
  height: 52px;
  box-sizing: border-box;
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 12px;
  padding: 0 8px 10px;
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
  flex: 0 0 auto;
  display: grid;
  grid-template-columns: 56px minmax(0, 1fr) auto;
  align-items: center;
  gap: 12px;
  padding: 4px 8px 12px;
  // The list scrolls under this edge; the hairline says where.
  border-bottom: 1px solid
    color-mix(
      in srgb,
      var(--main-cover-mix-color, #fff) calc(12% * var(--queue-fade, 1)),
      transparent
    );
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
  opacity: calc(0.55 * var(--queue-fade, 1));
  transition:
    opacity var(--duration-200) var(--ease-out),
    transform var(--duration-150) var(--ease-out);

  &[aria-pressed="true"] {
    opacity: var(--queue-fade, 1);
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
  height: 56px;
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

  &.history {
    --row-opacity: 0.55;
  }
}

// Rows fade as they slide under the sticky title (52px), gone by 40% under it so their text never
// sits on the title's. The opacity is on the row's children, never on the row: an element with
// (animated) opacity is an isolated group, and every plus-lighter inside it would blend with its
// row instead of with the cover behind. An element carrying both its own opacity and the blend
// still blends. The row only names the timeline its children run on. `--row-opacity` is the
// resting opacity (history is dimmed), read by both the children and the keyframes.
.queue-row-body > * {
  opacity: calc(var(--row-opacity, 1) * var(--queue-fade, 1));
}

@supports (animation-timeline: view()) {
  .queue-row {
    view-timeline: --queue-row block;
  }

  .queue-row-body > * {
    animation: queue-row-under linear both;
    animation-range: exit 0% exit 40%;
  }

  // Rules of their own: the minifier folds a timeline written beside `animation` into the
  // shorthand, which Chromium rejects (dropping the whole declaration).
  .player-queue-list .queue-row {
    view-timeline-inset: 52px 0px;
  }

  .player-queue-list .queue-row-body > * {
    animation-timeline: --queue-row;
  }
}

@keyframes queue-row-under {
  from {
    opacity: calc(var(--row-opacity, 1) * var(--queue-fade, 1));
  }

  to {
    opacity: 0;
  }
}

.settling .queue-row,
.settling .queue-row-body {
  transition: none !important;
}

.queue-row-body {
  position: relative;
  height: 56px;
  display: grid;
  grid-template-columns: 44px minmax(0, 1fr);
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
  // A sibling of the row body, so the body's fade misses it; a swipe left open must still go with
  // the desktop panel (a touch tablet gets the desktop layout).
  opacity: var(--queue-fade, 1);
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

.queue-row-text {
  min-width: 0;
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto auto;
  align-items: center;
  gap: 12px;
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
