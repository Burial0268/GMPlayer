<template>
  <n-config-provider abstract :theme-overrides="shareTheme">
    <!-- A page of the player, like its queue: it shows over the player's own background. -->
    <Transition
      name="lyric-share"
      appear
      :duration="{ enter: 650, leave: 250 }"
      @after-enter="revealOrigin"
      @after-leave="leave"
    >
      <div
        v-if="visible"
        :class="['lyric-share', `phase-${phase}`, { desktop: !isMobile, aligning }]"
        data-navigation-layer="share-player"
        role="dialog"
        aria-modal="true"
        :aria-label="t('player.share.title')"
        :style="layerStyle"
        @contextmenu.prevent
      >
        <!-- On a phone the column is a card of its own, laid out as the image is:
             lines are picked where they will be shared. -->
        <div ref="column" class="column">
          <canvas v-if="isMobile" ref="backdrop" :class="['backdrop', { shown: backdropShown }]" />
          <div
            ref="list"
            class="lines"
            role="group"
            tabindex="-1"
            autofocus
            :aria-label="t('player.share.title')"
          >
            <!-- Virtual: a lyric can run to thousands of lines. -->
            <n-virtual-list
              ref="scroller"
              class="virtual"
              :items="rows"
              :item-size="rowFloor"
              item-resizable
              :padding-top="pad[0]"
              :padding-bottom="pad[1]"
            >
              <template #default="{ item: row }: { item: LineRow }">
                <div
                  :key="row.key"
                  :ref="(el) => (lineEls[row.key] = el as HTMLElement)"
                  v-memo="[row.line, isSelected(row.key), row.key === originIndex, isMobile]"
                  :class="[
                    'line',
                    { selected: isSelected(row.key), origin: row.key === originIndex },
                  ]"
                  role="checkbox"
                  :aria-checked="isSelected(row.key)"
                  tabindex="0"
                  @click="toggle(row.key)"
                  @keydown.enter.space.prevent="toggle(row.key)"
                >
                  <!-- The card wraps as the image does; desktop keeps AMLL's units and
                       its breaks between them (`lyricLineBreak`). -->
                  <div v-if="isMobile" class="text">{{ row.line.text }}</div>
                  <div v-else class="text">
                    <template v-for="(units, r) in rowsOf(row)" :key="r">
                      <br v-if="r" />
                      <template v-for="(unit, u) in units" :key="u">
                        <template v-if="plainText || unit.isSpace">{{ unit.text }}</template>
                        <span v-else>{{ unit.text }}</span>
                      </template>
                    </template>
                  </div>
                  <div v-for="(sub, i) in row.line.subs" :key="i" class="sub">{{ sub }}</div>
                </div>
              </template>
            </n-virtual-list>
          </div>
          <!-- The image's footer: artwork, title and artist, the app mark. -->
          <div v-if="isMobile" class="meta">
            <img class="cover" :src="cover" alt="" />
            <div class="text">
              <div class="title">{{ song?.name }}</div>
              <div class="artist">{{ artists }}</div>
            </div>
            <div class="brand">
              <img src="/images/logo/favicon.png" alt="" />
              GMPlayer
            </div>
          </div>
        </div>

        <footer v-if="isMobile" class="foot">
          <n-button round secondary :disabled="!range" @click="copyText">
            <template #icon><Copy /></template>
            {{ t("player.share.copy") }}
          </n-button>
          <n-button
            v-for="action in imageActions"
            :key="action.key"
            round
            v-bind="action.primary ? PRIMARY : SECONDARY"
            :disabled="action.disabled"
            :tag="action.href ? 'a' : 'button'"
            :href="action.href"
            :download="action.download"
            @click="action.run?.()"
          >
            <template #icon><component :is="action.icon" /></template>
            {{ action.label }}
          </n-button>
        </footer>

        <!-- Desktop has the room to show the card as it will be shared, so there
             is no separate preview step. -->
        <aside v-if="!isMobile" class="stage">
          <!-- Two canvases: each draw goes into the hidden one and fades in over
               the other. The frame holds their place, so the buttons stay put. -->
          <div class="frame" role="img" :aria-label="t('player.share.image')">
            <canvas ref="cardA" :class="['card', { shown: front === 0 }]" />
            <canvas ref="cardB" :class="['card', { shown: front === 1 }]" />
          </div>
          <div class="actions">
            <n-button round secondary :disabled="!range" @click="copyText">
              <template #icon><Copy /></template>
              {{ t("player.share.copy") }}
            </n-button>
            <n-button
              v-for="action in imageActions"
              :key="action.key"
              round
              v-bind="action.primary ? PRIMARY : SECONDARY"
              :disabled="action.disabled"
              :tag="action.href ? 'a' : 'button'"
              :href="action.href"
              :download="action.download"
              @click="action.run?.()"
            >
              <template #icon><component :is="action.icon" /></template>
              {{ action.label }}
            </n-button>
          </div>
        </aside>
        <!-- The pressed line as AMLL lifted it, flying into its place in the card. -->
        <div
          v-if="isMobile && flying && originRow"
          ref="ghost"
          class="ghost"
          :style="{ transform: liftTransform }"
        >
          <div class="text">
            <template v-for="(units, r) in rowsOf(originRow)" :key="r">
              <br v-if="r" />
              <template v-for="(unit, u) in units" :key="u">
                <template v-if="plainText || unit.isSpace">{{ unit.text }}</template>
                <span v-else>{{ unit.text }}</span>
              </template>
            </template>
          </div>
          <div v-for="(sub, i) in originRow.line.subs" :key="i" class="sub">{{ sub }}</div>
        </div>
        <!-- The card's line, flying with the ghost and fading in over it: the two
             type sizes and wraps trade places in motion, not on top of each other. -->
        <div v-if="isMobile && flying && originRow" ref="twin" class="twin">
          <div class="text">{{ originRow.line.text }}</div>
          <div v-for="(sub, i) in originRow.line.subs" :key="i" class="sub">{{ sub }}</div>
        </div>

        <n-button
          class="dismiss"
          quaternary
          circle
          size="large"
          :aria-label="t('general.dialog.cancel')"
          @click="close"
        >
          <template #icon><Close /></template>
        </n-button>
      </div>
    </Transition>
  </n-config-provider>
</template>

<script setup lang="ts">
import { computed, nextTick, ref, shallowRef, useTemplateRef, watch, type Component } from "vue";
import { animateMini } from "motion-v";
import type { GlobalThemeOverrides, NVirtualList } from "naive-ui";
import { Close, Copy, Download, ShareTwo } from "@icon-park/vue-next";
import { useI18n } from "vue-i18n";
import { musicStore, settingStore, siteStore } from "@/store";
import { coverUrl } from "@/utils/coverUrl";
import type { AMLLLine } from "@/utils/LyricsProcessor";
import { balancedBreaks, lineUnits, type BreakUnit } from "@/utils/lyricLineBreak";
import { getTauri, isTauri } from "@/utils/tauri/core/runtime";
import { isMobileDevice } from "@/utils/tauri/platform/mobile";
import { useResponsiveLayout } from "@/composables/useResponsiveLayout";
import { useLyricShare } from "@/composables/useLyricShare";
import { useLayerNavigation } from "@/utils/navigation";
import { motionDuration } from "@/utils/navigation/motion";
import {
  CARD_METRICS,
  drawCardBackdrop,
  drawLyricCard,
  encodeLyricCard,
  loadCardAssets,
  type CardAssets,
  type LyricCardInput,
  type LyricShareLine,
} from "@/utils/lyricShareCard";

const props = defineProps<{
  /** A still of the player's fluid background, when that is what it shows. */
  captureStill: () => Promise<HTMLCanvasElement | undefined>;
}>();

const MAX_LINES = 5;
// LyricPlayer's lift: scale and ease.
const LIFT = 1.04;
const LIFT_EASE = [0.32, 0.72, 0, 1] as const;
const FLIGHT = 0.5;
// The mobile card's opacity for a selected line.
const CARD_SELECTED = 0.9;
const PRIMARY = { color: "#fff", textColor: "#111" };
const SECONDARY = { secondary: true };

// The layer sits on the player's background, dark whatever the app theme.
const BUTTON_THEME: GlobalThemeOverrides["Button"] = {
  fontWeight: "600",
  textColor: "#fff",
  colorSecondary: "rgb(255 255 255 / 0.16)",
  colorSecondaryHover: "rgb(255 255 255 / 0.24)",
  colorSecondaryPressed: "rgb(255 255 255 / 0.3)",
  colorQuaternaryHover: "rgb(255 255 255 / 0.12)",
  colorQuaternaryPressed: "rgb(255 255 255 / 0.18)",
};

const { t } = useI18n();
const music = musicStore();
const setting = settingStore();
const site = siteStore();
const navigation = useLayerNavigation();
const { request, clear } = useLyricShare();

// The share commands are registered for mobile builds, the saver for desktop.
const inTauri = isTauri();
const isMobileApp = inTauri && isMobileDevice();
const canShareImage = isMobileApp || (!inTauri && typeof navigator.canShare === "function");
// Mobile WebViews have no image clipboard; the share sheet covers it there.
const canCopyImage = !isMobileApp && typeof ClipboardItem !== "undefined";

const { isMobile } = useResponsiveLayout();
// Touch fires the scrollbar's hover show/hide on every tap, flashing the bar
// in and out; phones scroll by touch and get no bar, as naive does on iOS.
const shareTheme = computed<GlobalThemeOverrides>(() => ({
  Button: BUTTON_THEME,
  Scrollbar: isMobile.value ? { color: "transparent", colorHover: "transparent" } : undefined,
}));

const list = useTemplateRef<HTMLElement>("list");
const scroller = useTemplateRef<InstanceType<typeof NVirtualList>>("scroller");
const lineEls: HTMLElement[] = [];
// A consecutive run, or nothing: the pressed line can be let go of like any other.
const range = ref<[number, number] | null>(null);
const imageUrl = ref("");
// The PNG of the card as last drawn; see `cardBlob`.
let imageBlob: Promise<Blob> | null = null;
// Taken once per opening: every card drawn from it then shows the same frame.
let still: Promise<HTMLCanvasElement | undefined> = Promise.resolve(undefined);

const visible = computed(() => navigation.playerShareVisible.value && Boolean(request.value));

// Every line AMLL shows as a row is selectable, `♪` and `……` included: the
// list mirrors the view. Background vocals ride inside their main line's row.
const usable = computed(() =>
  (request.value?.lines ?? []).flatMap((line, raw) => (line.isBG ? [] : [{ line, raw }])),
);
const lines = computed<LyricShareLine[]>(() =>
  usable.value.map(({ line }) => ({
    text: line.words.map((w) => w.word).join(""),
    subs: [line.translatedLyric, line.romanLyric].filter((s): s is string => Boolean(s?.trim())),
  })),
);
// The phone's card, in pixels per 1080-wide unit of the image; measured on opening.
const cardUnit = ref(0);
// vueuc renders `listHeight / itemSize + 1` rows from the window's start, so an
// estimate above any real row leaves the window short: the shortest a line can
// be, one text row (AMLL's 1.2 line height, the card's 1.25) plus its padding.
const rowFloor = computed(() => {
  if (isMobile.value) return cardUnit.value * (CARD_METRICS.main * 1.25 + CARD_METRICS.gap) || 24;
  const origin = request.value?.origin;
  if (!origin) return 48;
  const [top, , bottom = top] = origin.padding.split(" ").map(Number.parseFloat);
  return origin.fontSize * 1.2 + top + bottom;
});

interface LineRow {
  key: number;
  line: LyricShareLine;
  words: AMLLLine["words"];
}
const rows = computed<LineRow[]>(() =>
  usable.value.map(({ line }, key) => ({ key, line: lines.value[key], words: line.words })),
);

// Lines break where AMLL breaks them. AMLL treats a lyric as plain text when no
// line has more than one word, and balances nothing without a segmenter.
const segmenter =
  typeof Intl.Segmenter === "function"
    ? new Intl.Segmenter(undefined, { granularity: "word" })
    : null;
const plainText = computed(() => (request.value?.lines ?? []).every((l) => l.words.length <= 1));
let measureCtx: CanvasRenderingContext2D | null = null;
// Per opening, and only for the rows the list renders.
const breakCache = computed(() => (request.value, new Map<number, BreakUnit[][]>()));
function rowsOf(row: LineRow): BreakUnit[][] {
  const cached = breakCache.value.get(row.key);
  if (cached) return cached;
  const origin = request.value?.origin;
  const units = segmenter ? lineUnits(row.words, plainText.value, segmenter) : [];
  let rowsOut = [units.length ? units : [{ text: row.line.text, isSpace: false }]];
  measureCtx ??= document.createElement("canvas").getContext("2d");
  if (segmenter && origin && measureCtx) {
    const [, right = 0, , left = right] = origin.padding.split(" ").map(Number.parseFloat);
    measureCtx.font = `${setting.lyricFontWeight} ${origin.fontSize}px ${setting.lyricFont}`;
    measureCtx.letterSpacing =
      setting.lyricLetterSpacing && setting.lyricLetterSpacing !== "normal"
        ? setting.lyricLetterSpacing
        : "0px";
    const widths = units.map((unit) => measureCtx!.measureText(unit.text).width);
    const breaks = balancedBreaks(units, widths, origin.rect.width - left - right, segmenter);
    rowsOut = [0, ...breaks].map((start, i) => units.slice(start, breaks[i] ?? units.length));
  }
  breakCache.value.set(row.key, rowsOut);
  return rowsOut;
}
// A background line has no row of its own: take the main line before it.
const originIndex = computed(() => {
  const pressed = request.value?.lineIndex ?? 0;
  return usable.value.reduce((found, { raw }, i) => (raw <= pressed ? i : found), 0);
});
const originRow = computed<LineRow | undefined>(() => rows.value[originIndex.value]);

// Desktop: the pressed line opens selected and placed exactly where it was
// pressed, so the layer takes it over without it moving. Phone: it is placed a
// third of the way down the card, and flies there from where it was pressed. AMLL puts a line anywhere
// (centred at the end of a song, say), where a list would run out of content,
// so the list gets the room above and below that this takes. Sized from the
// estimate before the first scroll: vueuc picks its window from the last
// scroll it saw, so padding changed after a scroll renders the wrong rows.
// One line of slack covers the estimate, which near either end spans few lines.
// About a second at most; done once it has held for a few frames.
const SETTLE_FRAMES = 60;
const SETTLED_FRAMES = 3;
const pad = ref<[number, number]>([0, 0]);
// The pressed line stays hidden until it sits where it was pressed: AMLL's own,
// still on screen under the layer, covers it meanwhile.
const aligning = ref(false);
// Phone only: lifted where it was pressed, flying into the card, landed.
const phase = ref<"lift" | "fly" | "landing" | "done">("done");
const flying = computed(() => phase.value === "lift" || phase.value === "fly");
const ghost = useTemplateRef<HTMLElement>("ghost");
const twin = useTemplateRef<HTMLElement>("twin");
const column = useTemplateRef<HTMLElement>("column");
const backdrop = useTemplateRef<HTMLCanvasElement>("backdrop");
const backdropShown = ref(false);
// Before the layer's first render: rendered as "done" first, the measuring
// below resolves the card visible and its opacity transition then fades it out.
watch(visible, (shown) => {
  if (!shown) return;
  aligning.value = true;
  phase.value = isMobile.value ? "lift" : "done";
});
watch(
  visible,
  async (shown) => {
    if (!shown || !request.value || !list.value) return;
    range.value = [originIndex.value, originIndex.value];
    clearImage();
    still = props.captureStill();
    assets = undefined;
    cardUnit.value = list.value.clientWidth / CARD_METRICS.width;
    if (isMobile.value) paintBackdrop();
    const size = rowFloor.value;
    const listTop = list.value.getBoundingClientRect().top;
    const target = isMobile.value
      ? list.value.clientHeight / 3
      : request.value.origin.rect.top - listTop;
    const above = originIndex.value * size;
    const below = (lines.value.length - originIndex.value) * size;
    pad.value = [
      Math.max(0, target - above + size),
      Math.max(0, list.value.clientHeight - target - below + size),
    ];
    await nextTick();
    scroller.value?.scrollTo({ index: originIndex.value });
    settle(listTop + target, SETTLE_FRAMES);
  },
  { flush: "post" },
);

// vueuc renders a new window on the scroll event and measures its rows a frame
// later, which moves them; a long jump keeps measuring for a while. Re-aim each
// frame until the line has held still for a few, within a bound.
function settle(aim: number, left: number, still = 0) {
  requestAnimationFrame(() => {
    const container = scroller.value?.getScrollContainer();
    if (!visible.value || !request.value || !container) {
      aligning.value = false;
      phase.value = "done";
      return;
    }
    const anchor = lineEls[originIndex.value];
    let moved = true;
    if (!anchor) scroller.value?.scrollTo({ index: originIndex.value });
    else {
      // Still lifted while it settles: aim the resting box, which shares the lifted one's centre.
      const box = anchor.getBoundingClientRect();
      const top = box.top + (box.height - anchor.offsetHeight) / 2;
      const delta = top - aim;
      moved = Math.abs(delta) > 0.5;
      if (moved) scroller.value?.scrollTo({ top: container.scrollTop + delta });
    }
    const held = moved ? 0 : still + 1;
    if (held < SETTLED_FRAMES && left > 1) settle(aim, left - 1, held);
    else {
      aligning.value = false;
      if (phase.value === "lift") fly();
    }
  });
}

// The ghost starts as AMLL left the line, lifted about AMLL's transform origin
// (expressed from its top-left corner, which the flight scales about), and ends
// on the card's line at the card's type size; the two cross-fade on landing.
const lift = computed(() => {
  const rect = request.value?.origin.rect;
  if (!rect) return [0, 0];
  const pivotX = setting.lyricsPosition === "center" ? rect.width / 2 : 0;
  return [pivotX * (1 - LIFT), (rect.height / 2) * (1 - LIFT)];
});
const liftTransform = computed(
  () => `translate(${lift.value[0]}px, ${lift.value[1]}px) scale(${LIFT})`,
);

function fly() {
  const el = ghost.value;
  const card = twin.value;
  const anchor = lineEls[originIndex.value];
  const origin = request.value?.origin;
  if (!el || !card || !anchor || !column.value || !origin) {
    phase.value = "done";
    return;
  }
  // Measured before the card starts growing in: these are its resting places.
  const [padTop, , , padLeft] = origin.padding.split(" ").map(Number.parseFloat);
  const style = getComputedStyle(anchor);
  const box = anchor.getBoundingClientRect();
  const cardLeft = Number.parseFloat(style.paddingLeft);
  const cardTop = Number.parseFloat(style.paddingTop);
  const scale = Number.parseFloat(style.fontSize) / origin.fontSize;
  const x = box.left + cardLeft - origin.rect.left - padLeft * scale;
  const y = box.top + cardTop - origin.rect.top - padTop * scale;
  // The twin rests on the card's line and starts on the lifted ghost, text start on text start.
  const twinScale = LIFT / scale;
  const twinX = origin.rect.left + lift.value[0] + LIFT * padLeft - twinScale * cardLeft - box.left;
  const twinY = origin.rect.top + lift.value[1] + LIFT * padTop - twinScale * cardTop - box.top;
  card.style.left = `${box.left}px`;
  card.style.top = `${box.top}px`;
  card.style.width = `${anchor.offsetWidth}px`;

  const duration = motionDuration(FLIGHT);
  const path = { duration, ease: LIFT_EASE };
  // While the line is still moving fast, so neither shape is seen standing still.
  const swap = { duration: duration * 0.4, delay: duration * 0.1 };
  phase.value = "fly";
  void Promise.all([
    animateMini(
      el,
      { transform: [liftTransform.value, `translate(${x}px, ${y}px) scale(${scale})`] },
      path,
    ),
    animateMini(el, { opacity: [1, 0] }, swap),
    animateMini(
      card,
      { transform: [`translate(${twinX}px, ${twinY}px) scale(${twinScale})`, "none"] },
      path,
    ),
    animateMini(card, { opacity: [0, CARD_SELECTED] }, swap),
    animateMini(
      column.value,
      { opacity: [0, 1], transform: ["translateY(24px) scale(0.92)", "none"] },
      path,
    ),
  ]).then(async () => {
    if (phase.value !== "fly") return;
    // The card's line takes over unfaded as the twin it matches leaves. Not a
    // frame later: this runs as animations update, before that frame's rAF
    // callbacks, so "landing" would never be styled and the line would fade in.
    phase.value = "landing";
    await nextTick();
    const line = lineEls[originIndex.value];
    if (line) void getComputedStyle(line).opacity;
    if (phase.value === "landing") phase.value = "done";
  });
}

// The card's background is the image's, painted once per opening.
function paintBackdrop() {
  backdropShown.value = false;
  run(async () => {
    const input = cardInput(await still);
    assets ??= loadCardAssets(input);
    const loaded = await assets;
    const canvas = backdrop.value;
    if (!canvas || !visible.value) return;
    const { clientWidth: w, clientHeight: h } = canvas;
    drawCardBackdrop(
      input,
      loaded,
      canvas,
      h / w,
      (w * window.devicePixelRatio) / CARD_METRICS.width,
    );
    backdropShown.value = true;
  });
}

// A share layer restored from history (reload, forward) has nothing to show.
watch(
  () => navigation.playerShareVisible.value && !request.value,
  (stale) => stale && navigation.closeTop("share"),
  { immediate: true },
);

// It opened where it was pressed, which may be under the bars that just came in.
function revealOrigin() {
  const anchor = lineEls[originIndex.value]?.getBoundingClientRect();
  const view = list.value?.getBoundingClientRect();
  if (!anchor || !view) return;
  const offset = Math.max(0, anchor.bottom - view.bottom) || Math.min(0, anchor.top - view.top);
  const scrolled = scroller.value?.getScrollContainer()?.scrollTop ?? 0;
  if (offset) scroller.value?.scrollTo({ top: scrolled + offset, behavior: "smooth" });
}

const close = () => navigation.closeTop("share");

function leave() {
  drawRun++;
  clearTimeout(linkTimer);
  front.value = -1;
  drawn.value = undefined;
  phase.value = "done";
  clear();
  clearImage();
}

// Selection stays one consecutive run: tapping a neighbour grows it, tapping an
// end shrinks it, tapping inside cuts it there, tapping elsewhere starts over,
// and tapping the only line clears it.
function toggle(index: number) {
  if (!range.value) {
    range.value = [index, index];
    return;
  }
  const [start, end] = range.value;
  if (index < start - 1 || index > end + 1) {
    range.value = [index, index];
  } else if (index === start - 1 || index === end + 1) {
    if (end - start + 1 >= MAX_LINES) {
      window.$message.warning(t("player.share.limit", { max: MAX_LINES }));
      return;
    }
    range.value = [Math.min(start, index), Math.max(end, index)];
  } else if (start === end) {
    range.value = null;
  } else if (index === start) {
    range.value = [start + 1, end];
  } else {
    range.value = [start, index === end ? end - 1 : index];
  }
}

// The list takes AMLL's column and type, so the pressed line lines up with
// the one it replaces; the phone's card takes the image's layout instead.
const layerStyle = computed(() => {
  const origin = request.value?.origin;
  return {
    "--share-left": `${origin?.rect.left ?? 0}px`,
    "--share-top": `${origin?.rect.top ?? 0}px`,
    "--share-width": `${origin?.rect.width ?? 0}px`,
    "--share-font-size": `${origin?.fontSize ?? 24}px`,
    "--share-padding": origin?.padding ?? "0px",
    "--card-unit": `${cardUnit.value}px`,
    ...Object.fromEntries(Object.entries(CARD_METRICS).map(([k, v]) => [`--card-${k}`, v])),
    "--share-font": setting.lyricFont,
    "--share-weight": setting.lyricFontWeight,
    "--share-spacing": setting.lyricLetterSpacing,
    "--share-align": setting.lyricsPosition === "center" ? "center" : "left",
  };
});
const isSelected = (index: number) =>
  !!range.value && index >= range.value[0] && index <= range.value[1];
const song = computed(() => music.getPlaySongData);
// PlayerCover's size, so the header shows an image that is already decoded.
const cover = computed(() => coverUrl(song.value?.album?.picUrl, 1024));
const artists = computed(() => (song.value?.artist ?? []).map((a) => a.name).join(" / "));
const fileName = computed(() => [song.value?.name, artists.value].filter(Boolean).join(" - "));
const selected = computed(() =>
  range.value ? lines.value.slice(range.value[0], range.value[1] + 1) : [],
);
const lyricText = computed(() =>
  selected.value.map((line) => [line.text, ...line.subs].join("\n")).join("\n"),
);

// Desktop draws the card straight into the preview as the selection changes,
// at the size it is shown; a phone's card is the live list itself. The full
// 1080-wide card is rasterised and encoded only when an action needs the PNG.
// A draw the selection has outrun is dropped.
const PREVIEW_WIDTH = 420;
const LINK_DELAY = 400;
const cardA = useTemplateRef<HTMLCanvasElement>("cardA");
const cardB = useTemplateRef<HTMLCanvasElement>("cardB");
const front = ref(-1);
let assets: Promise<CardAssets> | undefined;
const drawn = shallowRef<{ input: LyricCardInput; assets: CardAssets }>();
let drawRun = 0;
let linkTimer = 0;
watch([visible, selected, isMobile], ([shown, chosen, mobile]) => {
  if (!shown) return;
  const current = ++drawRun;
  if (!chosen.length) {
    // Nothing selected: nothing to preview or share.
    clearTimeout(linkTimer);
    clearImage();
    front.value = -1;
    drawn.value = undefined;
    return;
  }
  run(async () => {
    const input = cardInput(await still);
    assets ??= loadCardAssets(input);
    const loaded = await assets;
    if (current !== drawRun) return;
    if (!mobile) {
      const target = front.value === 0 ? cardB.value : cardA.value;
      if (!target) return;
      const scale = Math.min(1, (PREVIEW_WIDTH * window.devicePixelRatio) / 1080);
      drawLyricCard(input, loaded, target, scale);
      front.value = target === cardA.value ? 0 : 1;
    }
    clearImage();
    drawn.value = { input, assets: loaded };
    // The browser saves through a link, which needs its URL before the click;
    // made once the selection settles, not per tap.
    if (inTauri) return;
    clearTimeout(linkTimer);
    linkTimer = window.setTimeout(
      () =>
        run(async () => {
          const url = URL.createObjectURL(await cardBlob());
          if (current === drawRun) imageUrl.value = url;
          else URL.revokeObjectURL(url);
        }),
      LINK_DELAY,
    );
  });
});

/** The PNG being shared: the last draw, at full size. */
function cardBlob(): Promise<Blob> {
  if (!imageBlob && drawn.value) {
    const { input, assets: loaded } = drawn.value;
    imageBlob = encodeLyricCard(drawLyricCard(input, loaded), input, loaded);
  }
  return imageBlob!;
}

async function run(task: () => Promise<unknown>) {
  try {
    await task();
  } catch (error) {
    console.error("[lyricShare]", error);
    window.$message.error(t("player.share.failed"));
  }
}

const copyText = () =>
  run(async () => {
    await navigator.clipboard.writeText(lyricText.value);
    window.$message.success(t("general.message.copySuccess"));
  });

const cardInput = (backdrop?: HTMLCanvasElement): LyricCardInput => ({
  lines: selected.value,
  title: song.value?.name ?? "",
  artist: artists.value,
  coverSrc: coverUrl(song.value?.album?.picUrl, 512),
  backdrop,
  themeRgb: site.songPicColor,
  fontFamily: setting.lyricFont,
  brand: "GMPlayer",
  logoSrc: "/images/logo/favicon.png",
});

function clearImage() {
  if (imageUrl.value) URL.revokeObjectURL(imageUrl.value);
  imageUrl.value = "";
  imageBlob = null;
}

const copyImage = () =>
  run(async () => {
    // The promise, not the blob: awaiting the encode first would spend the click's activation.
    await navigator.clipboard.write([new ClipboardItem({ "image/png": cardBlob() })]);
    window.$message.success(t("general.message.copySuccess"));
  });

const nativeImageCall = async (cmd: string) =>
  getTauri()!.core.invoke<boolean | null>(
    cmd,
    new Uint8Array(await (await cardBlob()).arrayBuffer()),
    {
      headers: { "x-share": new URLSearchParams({ name: fileName.value }).toString() },
    },
  );

const saveImage = () =>
  run(async () => {
    if (await nativeImageCall("share_save_image")) {
      window.$message.success(t("player.share.saved"));
    }
  });

const shareImage = () =>
  run(async () => {
    if (isMobileApp) {
      await nativeImageCall("share_image");
      return;
    }
    const file = new File([await cardBlob()], `${fileName.value || "lyrics"}.png`, {
      type: "image/png",
    });
    await navigator.share({ files: [file] }).catch(ignoreAbort);
  });

interface ImageAction {
  key: string;
  label: string;
  icon: Component;
  primary?: boolean;
  run?: () => void;
  /** The browser saves through a plain download link; Tauri goes through a native dialog. */
  href?: string;
  disabled?: boolean;
  download?: string;
}
const imageActions = computed(() => {
  const save = t("player.share.saveImage");
  // Nothing to act on before the first draw, nor to link before its encode.
  const disabled = !drawn.value;
  const actions: ImageAction[] = [];
  if (canCopyImage) {
    actions.push({
      key: "copy",
      label: t("player.share.copyImage"),
      icon: Copy,
      run: copyImage,
      disabled,
    });
  }
  if (!inTauri) {
    const download = `${fileName.value || "lyrics"}.png`;
    actions.push({
      key: "save",
      label: save,
      icon: Download,
      primary: true,
      href: imageUrl.value,
      download,
      disabled: !imageUrl.value,
    });
  } else if (!isMobileApp) {
    actions.push({
      key: "save",
      label: save,
      icon: Download,
      primary: true,
      run: saveImage,
      disabled,
    });
  }
  if (canShareImage) {
    actions.push({
      key: "share",
      label: t("player.share.share"),
      icon: ShareTwo,
      primary: true,
      run: shareImage,
      disabled,
    });
  }
  return actions;
});

// Dismissing the browser share sheet rejects with AbortError; that is not a failure.
function ignoreAbort(error: unknown) {
  if (!(error instanceof DOMException && error.name === "AbortError")) throw error;
}
</script>

<style lang="scss" scoped>
.lyric-share {
  position: absolute;
  inset: 0;
  z-index: 20;
  color: #fff;
}

.dismiss {
  position: absolute;
  top: calc(var(--app-safe-area-top, 0px) + 20px);
  right: 24px;
  z-index: 1;
}

// Everything left of the lyrics column.
.stage {
  position: absolute;
  top: var(--app-safe-area-top, 0px);
  bottom: var(--app-safe-area-bottom, 0px);
  left: 0;
  width: var(--share-left);
  box-sizing: border-box;
  padding: 48px 40px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 24px;

  .frame {
    position: relative;
    flex: 1;
    min-height: 0;
    width: 100%;
  }

  // Stacked and centred at their own aspect ratio; the shown one fades in over the other.
  .card {
    position: absolute;
    inset: 0;
    margin: auto;
    max-width: min(100%, 420px);
    max-height: 100%;
    border-radius: 16px;
    box-shadow: 0 16px 48px rgb(0 0 0 / 0.4);
    opacity: 0;
    transition: opacity 0.2s ease-out;

    &.shown {
      opacity: 1;
    }
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 8px;
  }
}

// AMLL's type (`lyricMainLine` 1em / 1.2, `lyricSubLine` 0.5em / 1.5), and its
// word layout: the desktop list and the line flying into the phone's card.
@mixin amll-type {
  padding: var(--share-padding);
  font-size: var(--share-font-size);

  .text {
    line-height: 1.2;

    // AMLL's `lyricMainLine > span`: a word never breaks, a line breaks between words.
    > span {
      display: inline-block;
      white-space: pre-wrap;
      vertical-align: bottom;
    }
  }

  .sub {
    font-size: 0.5em;
    line-height: 1.5;
    opacity: 0.75;
  }
}

// A length in the image's 1080-wide units, at the phone card's size.
@function card($name) {
  @return calc(var(--card-#{$name}) * var(--card-unit));
}

// The image's type: 800 at its main size, wrapped anywhere a word must break.
@mixin card-type {
  padding: calc(#{card(gap)} / 2) card(pad);
  font-family: var(--share-font), system-ui, sans-serif;
  font-size: card(main);
  font-weight: 800;
  letter-spacing: normal;
  text-align: left;

  .text {
    line-height: 1.25;
    overflow-wrap: anywhere;
  }

  .sub {
    font-size: calc(var(--card-sub) / var(--card-main) * 1em);
    font-weight: 600;
    line-height: 1.4;
    opacity: 0.8;
  }
}

// AMLL's column: the list stays where the lyrics were. Not env(): the Android
// WebView reports only the cutout there, never the system bars.
.column {
  position: absolute;
  top: var(--app-safe-area-top, 0px);
  bottom: var(--app-safe-area-bottom, 0px);
  left: var(--share-left);
  width: var(--share-width);
  display: flex;
  flex-direction: column;
}

.lines {
  flex: 1;
  min-height: 0;
  // Focused on open only as the dialog's start; its lines carry the rings.
  outline: none;
  font-family: var(--share-font);
  font-weight: var(--share-weight);
  letter-spacing: var(--share-spacing);
  text-align: var(--share-align);

  .virtual {
    height: 100%;
  }
}

.line {
  @include amll-type;
  border-radius: 12px;
  cursor: pointer;
  opacity: 0.5;
  outline: none;
  user-select: none;
  -webkit-tap-highlight-color: transparent;
  transform-origin: var(--share-align) center;
  transition:
    opacity var(--duration-200, 0.2s),
    background-color var(--duration-200, 0.2s);

  // Touch keeps :hover on the last tapped line, which then sits between states.
  @media (hover: hover) and (pointer: fine) {
    &:hover {
      opacity: 0.7;
    }
  }

  &:focus-visible {
    background-color: rgb(255 255 255 / 0.1);
  }

  &.selected {
    opacity: 1;
    background-color: rgb(255 255 255 / 0.14);
  }
}

// A phone dims the player and centres the card over its actions.
.lyric-share:not(.desktop) {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: calc(var(--app-safe-area-top, 0px) + 56px) 24px
    calc(var(--app-safe-area-bottom, 0px) + 16px);

  &::before {
    content: "";
    position: absolute;
    inset: 0;
    z-index: -1;
    background: rgb(0 0 0 / 0.35);
  }

  .dismiss {
    top: calc(var(--app-safe-area-top, 0px) + 8px);
    right: 8px;
  }

  // The image's shape at the least; shorter if the screen is.
  .column {
    position: relative;
    inset: auto;
    flex: 0 1 auto;
    width: min(100%, 400px);
    aspect-ratio: 4 / 5;
    min-height: 0;
    max-height: 100%;
    border-radius: 24px;
    overflow: hidden;
    isolation: isolate;
    background: rgb(0 0 0 / 0.3);
    box-shadow:
      inset 0 0 0 1px rgb(255 255 255 / 0.1),
      0 16px 48px rgb(0 0 0 / 0.35);
    transition: opacity 0.4s ease-out;
  }

  // Laid out but unseen while the line is still lifted, so it can be aimed at.
  &.phase-lift .column {
    opacity: 0;
  }

  .backdrop {
    position: absolute;
    inset: 0;
    z-index: -1;
    width: 100%;
    height: 100%;
    opacity: 0;
    transition: opacity 0.3s ease-out;

    &.shown {
      opacity: 1;
    }
  }

  .lines {
    mask-image: linear-gradient(
      transparent,
      #000 #{card(pad)},
      #000 calc(100% - #{card(gap)}),
      transparent
    );
  }

  .line {
    @include card-type;
    border-radius: 0;
    opacity: 0.4;

    // Bright or dim, as the image will have it; no boxes.
    &.selected {
      opacity: 0.9;
      background-color: transparent;
    }
  }

  // The ghost stands in for it until it lands.
  &.phase-lift .line.origin,
  &.phase-fly .line.origin {
    opacity: 0;
  }

  // The twin covers it exactly, so it takes over at once; a fade would dip.
  &.phase-landing .line.origin {
    transition: none;
  }

  .meta {
    display: flex;
    align-items: center;
    gap: calc(28 * var(--card-unit));
    padding: card(gap) card(pad) card(pad);

    .cover {
      width: card(cover);
      height: card(cover);
      border-radius: calc(14 * var(--card-unit));
      object-fit: cover;
      box-shadow: 0 4px 12px rgb(0 0 0 / 0.3);
    }

    .text {
      flex: 1;
      min-width: 0;
    }

    .title,
    .artist {
      overflow: hidden;
      white-space: nowrap;
      text-overflow: ellipsis;
    }

    .title {
      font-size: calc(38 * var(--card-unit));
      font-weight: 700;
      opacity: 0.9;
    }

    .artist {
      font-size: calc(30 * var(--card-unit));
      font-weight: 500;
      opacity: 0.7;
    }

    .brand {
      display: flex;
      align-items: center;
      gap: calc(12 * var(--card-unit));
      font-size: calc(28 * var(--card-unit));
      font-weight: 600;
      opacity: 0.8;

      img {
        width: card(logo);
        height: card(logo);
      }
    }
  }
}

// Where AMLL drew the pressed line, in its type; `fly` moves it from there.
.ghost {
  @include amll-type;
  position: absolute;
  top: var(--share-top);
  left: var(--share-left);
  width: var(--share-width);
  box-sizing: border-box;
  z-index: 2;
  font-family: var(--share-font);
  font-weight: var(--share-weight);
  letter-spacing: var(--share-spacing);
  text-align: var(--share-align);
  transform-origin: top left;
  pointer-events: none;
}

// Positioned on the card's line by `fly`; starts on the ghost.
.twin {
  @include card-type;
  position: absolute;
  z-index: 2;
  box-sizing: border-box;
  opacity: 0;
  transform-origin: top left;
  pointer-events: none;
}

.foot {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  width: min(100%, 400px);
  margin-top: 16px;

  // Fill the row, but never below their own label; wrap instead.
  > * {
    flex: 1 0 auto;
  }
}

// Apple Music's order: the pressed line is already lifted (LyricPlayer lifts
// it on a long press) and the player fades from around it. On desktop the
// other lines and the bars come in and the lifted line settles into its
// selection; on a phone the line flies into the card as the card comes in.
.lyric-share-enter-active {
  &::before,
  .foot,
  .stage,
  .dismiss {
    transition:
      opacity 0.3s ease-out 0.2s,
      translate 0.45s cubic-bezier(0.32, 0.72, 0, 1) 0.2s;
  }

  &.desktop .line {
    transition:
      opacity 0.3s ease-out 0.15s,
      scale 0.4s cubic-bezier(0.32, 0.72, 0, 1) 0.2s,
      background-color 0.3s ease-out 0.3s;
  }
}

.lyric-share-enter-from {
  &::before {
    opacity: 0;
  }

  .foot {
    opacity: 0;
    translate: 0 16px;
  }

  .stage {
    opacity: 0;
    translate: -16px 0;
  }

  .dismiss {
    opacity: 0;
  }

  &.desktop .line {
    opacity: 0;
  }

  &.desktop .line.origin {
    opacity: 1;
    scale: 1.04;
    background-color: transparent;
  }
}

// Until it is aligned; its enter transition runs on underneath, so it shows
// mid-lift, where AMLL's lifted line is.
.lyric-share.aligning .line.origin {
  visibility: hidden;
}

.lyric-share-leave-active {
  transition: opacity 0.25s ease-in;

  // A phone's card sinks back as it fades, the actions drop away under it.
  &:not(.desktop) {
    .column,
    .foot {
      transition:
        scale 0.25s ease-in,
        translate 0.25s ease-in;
    }
  }
}

.lyric-share-leave-to {
  opacity: 0;

  &:not(.desktop) {
    .column {
      scale: 0.94;
    }

    .foot {
      translate: 0 16px;
    }
  }
}
</style>
