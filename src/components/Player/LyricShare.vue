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
        :class="['lyric-share', { desktop: !isMobile }]"
        data-navigation-layer="share-player"
        role="dialog"
        aria-modal="true"
        :aria-label="t('player.share.title')"
        :style="layerStyle"
        @contextmenu.prevent
      >
        <div class="column">
          <header v-if="isMobile" class="head">
            <img class="cover" :src="cover" alt="" />
            <div class="meta">
              <div class="title">{{ song?.name }}</div>
              <div class="artist">{{ artists }}</div>
            </div>
            <n-button quaternary circle :aria-label="t('general.dialog.cancel')" @click="close">
              <template #icon><Close /></template>
            </n-button>
          </header>

          <div
            v-if="!isMobile || !imageUrl"
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
                  v-memo="[row.line, isSelected(row.key), row.key === originIndex]"
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
                  <!-- AMLL's units and its breaks between them (`lyricLineBreak`). -->
                  <div class="text">
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
          <div v-else class="preview">
            <img :src="imageUrl" alt="" />
          </div>

          <footer v-if="isMobile" class="foot">
            <template v-if="!imageUrl">
              <span class="count">
                {{ t("player.share.selected", { count: range[1] - range[0] + 1, max: MAX_LINES }) }}
              </span>
              <div class="actions">
                <n-button round secondary @click="copyText">
                  <template #icon><Copy /></template>
                  {{ t("player.share.copy") }}
                </n-button>
                <n-button v-if="canShareText" round secondary @click="shareText">
                  <template #icon><ShareTwo /></template>
                  {{ t("player.share.share") }}
                </n-button>
                <n-button
                  round
                  color="#fff"
                  text-color="#111"
                  :loading="rendering"
                  @click="makeImage"
                >
                  <template #icon><Pic /></template>
                  {{ t("player.share.image") }}
                </n-button>
              </div>
            </template>
            <template v-else>
              <n-button round secondary @click="clearImage">
                <template #icon><Left /></template>
                {{ t("player.share.back") }}
              </n-button>
              <div class="actions">
                <n-button
                  v-for="action in imageActions"
                  :key="action.key"
                  round
                  v-bind="action.primary ? PRIMARY : SECONDARY"
                  :tag="action.href ? 'a' : 'button'"
                  :href="action.href"
                  :download="action.download"
                  @click="action.run?.()"
                >
                  <template #icon><component :is="action.icon" /></template>
                  {{ action.label }}
                </n-button>
              </div>
            </template>
          </footer>
        </div>

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
            <n-button round secondary @click="copyText">
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
        <n-button
          v-if="!isMobile"
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
import { computed, nextTick, ref, useTemplateRef, watch, type Component } from "vue";
import type { GlobalThemeOverrides, NVirtualList } from "naive-ui";
import { Close, Copy, Download, Left, Pic, ShareTwo } from "@icon-park/vue-next";
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
import {
  drawLyricCard,
  encodeLyricCard,
  loadCardAssets,
  renderLyricCard,
  type CardAssets,
  type LyricCardInput,
  type LyricShareLine,
} from "@/utils/lyricShareCard";

const props = defineProps<{
  /** A still of the player's fluid background, when that is what it shows. */
  captureStill: () => Promise<HTMLCanvasElement | undefined>;
}>();

const MAX_LINES = 5;
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
const canShareText = isMobileApp || (!inTauri && typeof navigator.share === "function");
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
const range = ref<[number, number]>([0, 0]);
const rendering = ref(false);
const imageUrl = ref("");
// The PNG of whatever card is showing; see `cardBlob`.
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
// vueuc renders `listHeight / itemSize + 1` rows from the window's start, so an
// estimate above any real row leaves the window short: the shortest a line can
// be, one text row (AMLL's 1.2 line height) plus its padding.
const rowFloor = computed(() => {
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

// The pressed line opens selected and placed exactly where it was pressed, so
// the layer takes it over without it moving. AMLL puts a line anywhere
// (centred at the end of a song, say), where a list would run out of content,
// so the list gets the room above and below that this takes. Sized from the
// estimate before the first scroll: vueuc picks its window from the last
// scroll it saw, so padding changed after a scroll renders the wrong rows.
// One line of slack covers the estimate, which near either end spans few lines.
// About a second at most; done once it has held for a few frames.
const SETTLE_FRAMES = 60;
const SETTLED_FRAMES = 3;
const pad = ref<[number, number]>([0, 0]);
watch(
  visible,
  async (shown) => {
    if (!shown || !request.value || !list.value) return;
    range.value = [originIndex.value, originIndex.value];
    clearImage();
    still = props.captureStill();
    assets = undefined;
    const size = rowFloor.value;
    const target = request.value.origin.rect.top - list.value.getBoundingClientRect().top;
    const above = originIndex.value * size;
    const below = (lines.value.length - originIndex.value) * size;
    pad.value = [
      Math.max(0, target - above + size),
      Math.max(0, list.value.clientHeight - target - below + size),
    ];
    await nextTick();
    scroller.value?.scrollTo({ index: originIndex.value });
    settle(SETTLE_FRAMES);
  },
  { flush: "post" },
);

// vueuc renders a new window on the scroll event and measures its rows a frame
// later, which moves them; a long jump keeps measuring for a while. Re-aim each
// frame until the line has held still for a few, within a bound.
function settle(left: number, still = 0) {
  requestAnimationFrame(() => {
    const container = scroller.value?.getScrollContainer();
    if (!visible.value || !request.value || !container) return;
    const anchor = lineEls[originIndex.value];
    let moved = true;
    if (!anchor) scroller.value?.scrollTo({ index: originIndex.value });
    else {
      // Still lifted while it settles: aim the resting box, which shares the lifted one's centre.
      const box = anchor.getBoundingClientRect();
      const top = box.top + (box.height - anchor.offsetHeight) / 2;
      const delta = top - request.value.origin.rect.top;
      moved = Math.abs(delta) > 0.5;
      if (moved) scroller.value?.scrollTo({ top: container.scrollTop + delta });
    }
    const held = moved ? 0 : still + 1;
    if (held < SETTLED_FRAMES && left > 1) settle(left - 1, held);
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
  drawn = undefined;
  clear();
  clearImage();
}

// Selection stays one consecutive run: tapping a neighbour grows it, tapping an
// end shrinks it, tapping inside cuts it there, tapping elsewhere starts over.
function toggle(index: number) {
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
    return;
  } else if (index === start) {
    range.value = [start + 1, end];
  } else {
    range.value = [start, index === end ? end - 1 : index];
  }
}

// The list takes AMLL's column and type, so the pressed line lines up with
// the one it replaces.
const layerStyle = computed(() => {
  const origin = request.value?.origin;
  return {
    "--share-left": `${origin?.rect.left ?? 0}px`,
    "--share-width": `${origin?.rect.width ?? 0}px`,
    "--share-font-size": `${origin?.fontSize ?? 24}px`,
    "--share-padding": origin?.padding ?? "0px",
    "--share-font": setting.lyricFont,
    "--share-weight": setting.lyricFontWeight,
    "--share-spacing": setting.lyricLetterSpacing,
    "--share-align": setting.lyricsPosition === "center" ? "center" : "left",
  };
});
const isSelected = (index: number) => index >= range.value[0] && index <= range.value[1];
const song = computed(() => music.getPlaySongData);
// PlayerCover's size, so the header shows an image that is already decoded.
const cover = computed(() => coverUrl(song.value?.album?.picUrl, 1024));
const artists = computed(() => (song.value?.artist ?? []).map((a) => a.name).join(" / "));
const fileName = computed(() => [song.value?.name, artists.value].filter(Boolean).join(" - "));
const selected = computed(() => lines.value.slice(range.value[0], range.value[1] + 1));
const lyricText = computed(() =>
  selected.value.map((line) => [line.text, ...line.subs].join("\n")).join("\n"),
);

// Desktop draws the card straight into the preview as the selection changes,
// at the size it is shown: the full 1080-wide card is rasterised and encoded
// only when an action needs the PNG. A draw the selection has outrun is dropped.
const PREVIEW_WIDTH = 420;
const LINK_DELAY = 400;
const cardA = useTemplateRef<HTMLCanvasElement>("cardA");
const cardB = useTemplateRef<HTMLCanvasElement>("cardB");
const front = ref(-1);
let assets: Promise<CardAssets> | undefined;
let drawn: { input: LyricCardInput; assets: CardAssets } | undefined;
let drawRun = 0;
let linkTimer = 0;
watch([visible, selected, isMobile], ([shown, , mobile]) => {
  if (!shown || mobile) return;
  const current = ++drawRun;
  run(async () => {
    const input = cardInput(await still);
    assets ??= loadCardAssets(input);
    const loaded = await assets;
    const target = front.value === 0 ? cardB.value : cardA.value;
    if (current !== drawRun || !target) return;
    const scale = Math.min(1, (PREVIEW_WIDTH * window.devicePixelRatio) / 1080);
    drawLyricCard(input, loaded, target, scale);
    front.value = target === cardA.value ? 0 : 1;
    clearImage();
    drawn = { input, assets: loaded };
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

/** The PNG being shared: the phone's generated image, or desktop's last draw at full size. */
function cardBlob(): Promise<Blob> {
  if (!imageBlob && drawn) {
    const { input, assets: loaded } = drawn;
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

const shareText = () =>
  run(async () => {
    const text = `${lyricText.value}\n\n—— ${fileName.value}`;
    if (isMobileApp) await getTauri()!.core.invoke("share_text", { text });
    else await navigator.share({ text }).catch(ignoreAbort);
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

const makeImage = () =>
  run(async () => {
    rendering.value = true;
    try {
      const blob = await renderLyricCard(cardInput(await still));
      clearImage();
      imageBlob = Promise.resolve(blob);
      imageUrl.value = URL.createObjectURL(blob);
    } finally {
      rendering.value = false;
    }
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
  // Desktop: nothing to act on before the first draw, nor to link before its encode.
  const disabled = front.value < 0;
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

.head {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 20px 4px 12px 12px;

  .cover {
    width: 48px;
    height: 48px;
    border-radius: 8px;
    object-fit: cover;
    box-shadow: 0 4px 12px rgb(0 0 0 / 0.25);
  }

  .meta {
    flex: 1;
    min-width: 0;

    .title,
    .artist {
      overflow: hidden;
      white-space: nowrap;
      text-overflow: ellipsis;
    }

    .title {
      font-size: 16px;
      font-weight: 700;
    }

    .artist {
      font-size: 13px;
      opacity: 0.7;
    }
  }
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

// AMLL's line box and type (`lyricMainLine` 1em / 1.2, `lyricSubLine` 0.5em / 1.5),
// and its word layout below.
.line {
  padding: var(--share-padding);
  font-size: var(--share-font-size);
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

.preview {
  flex: 1;
  min-height: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 8px 0;

  img {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
    border-radius: 16px;
    box-shadow: 0 12px 32px rgb(0 0 0 / 0.35);
  }
}

.foot {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  align-items: center;
  justify-content: space-between;
  padding: 12px 4px 16px 12px;

  .count {
    font-size: 13px;
    opacity: 0.7;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-left: auto;
  }

  @media (max-width: 768px) {
    .count {
      width: 100%;
    }

    .actions {
      width: 100%;
      margin-left: 0;

      > * {
        flex: 1;
      }
    }
  }
}

// Apple Music's order: the pressed line is already lifted (LyricPlayer lifts
// it on a long press), the player fades from around it, the other lines and
// the bars come in, and the lifted line settles into its selection box.
.lyric-share-enter-active {
  .head,
  .foot,
  .stage,
  .dismiss {
    transition:
      opacity 0.3s ease-out 0.2s,
      translate 0.45s cubic-bezier(0.32, 0.72, 0, 1) 0.2s;
  }

  .line {
    transition:
      opacity 0.3s ease-out 0.15s,
      scale 0.4s cubic-bezier(0.32, 0.72, 0, 1) 0.2s,
      background-color 0.3s ease-out 0.3s;
  }
}

.lyric-share-enter-from {
  .head {
    opacity: 0;
    translate: 0 -16px;
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

  .line {
    opacity: 0;
  }

  .line.origin {
    opacity: 1;
    scale: 1.04;
    background-color: transparent;
  }
}

.lyric-share-leave-active {
  transition: opacity 0.25s ease-in;
}

.lyric-share-leave-to {
  opacity: 0;
}
</style>
