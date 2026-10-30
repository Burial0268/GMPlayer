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
        class="lyric-share"
        data-navigation-layer="share-player"
        role="dialog"
        aria-modal="true"
        :aria-label="t('player.share.title')"
        :style="layerStyle"
      >
        <div class="column">
          <header class="head">
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
            v-if="!imageUrl"
            ref="list"
            class="lines"
            role="group"
            :aria-label="t('player.share.title')"
          >
            <!-- Room above and below, so any line can be scrolled to where it was pressed. -->
            <n-scrollbar ref="scroller" content-style="padding-block: 30vh">
              <div
                v-for="(line, index) in lines"
                :key="index"
                :ref="(el) => (lineEls[index] = el as HTMLElement)"
                v-memo="[line, isSelected(index), index === originIndex]"
                :class="['line', { selected: isSelected(index), origin: index === originIndex }]"
                role="checkbox"
                :aria-checked="isSelected(index)"
                tabindex="0"
                @click="toggle(index)"
                @keydown.enter.space.prevent="toggle(index)"
              >
                <div class="text">{{ line.text }}</div>
                <div v-for="(sub, i) in line.subs" :key="i" class="sub">{{ sub }}</div>
              </div>
            </n-scrollbar>
          </div>
          <div v-else class="preview">
            <img :src="imageUrl" alt="" />
          </div>

          <footer class="foot">
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
                <n-button v-if="canCopyImage" round secondary @click="copyImage">
                  <template #icon><Copy /></template>
                  {{ t("player.share.copyImage") }}
                </n-button>
                <!-- The browser saves a plain download link; Tauri goes through a native dialog. -->
                <n-button
                  v-if="!inTauri"
                  round
                  color="#fff"
                  text-color="#111"
                  tag="a"
                  :href="imageUrl"
                  :download="`${fileName || 'lyrics'}.png`"
                >
                  <template #icon><Download /></template>
                  {{ t("player.share.saveImage") }}
                </n-button>
                <n-button
                  v-else-if="!isMobileApp"
                  round
                  color="#fff"
                  text-color="#111"
                  @click="saveImage"
                >
                  <template #icon><Download /></template>
                  {{ t("player.share.saveImage") }}
                </n-button>
                <n-button
                  v-if="canShareImage"
                  round
                  color="#fff"
                  text-color="#111"
                  @click="shareImage"
                >
                  <template #icon><ShareTwo /></template>
                  {{ t("player.share.share") }}
                </n-button>
              </div>
            </template>
          </footer>
        </div>
      </div>
    </Transition>
  </n-config-provider>
</template>

<script setup lang="ts">
import { computed, ref, useTemplateRef, watch } from "vue";
import type { GlobalThemeOverrides, ScrollbarInst } from "naive-ui";
import { Close, Copy, Download, Left, Pic, ShareTwo } from "@icon-park/vue-next";
import { useI18n } from "vue-i18n";
import { musicStore, settingStore, siteStore } from "@/store";
import { coverUrl } from "@/utils/coverUrl";
import { isInterludeLine } from "@/utils/LyricsProcessor";
import { getTauri, isTauri } from "@/utils/tauri/core/runtime";
import { isMobileDevice } from "@/utils/tauri/platform/mobile";
import { useResponsiveLayout } from "@/composables/useResponsiveLayout";
import { useLyricShare } from "@/composables/useLyricShare";
import { useLayerNavigation } from "@/utils/navigation";
import { renderLyricCard, type LyricShareLine } from "@/utils/lyricShareCard";

const props = defineProps<{
  /** A still of the player's fluid background, when that is what it shows. */
  captureStill: () => Promise<HTMLCanvasElement | undefined>;
}>();

const MAX_LINES = 5;

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
const scroller = useTemplateRef<ScrollbarInst>("scroller");
const lineEls: HTMLElement[] = [];
const range = ref<[number, number]>([0, 0]);
const rendering = ref(false);
const imageUrl = ref("");
let imageBlob: Blob | null = null;

const visible = computed(() => navigation.playerShareVisible.value && Boolean(request.value));

// Interludes and background vocals are not lines anyone shares.
const usable = computed(() =>
  (request.value?.lines ?? []).flatMap((line, raw) =>
    line.isBG || isInterludeLine(line) ? [] : [{ line, raw }],
  ),
);
const lines = computed<LyricShareLine[]>(() =>
  usable.value.map(({ line }) => ({
    text: line.words.map((w) => w.word).join(""),
    subs: [line.translatedLyric, line.romanLyric].filter((s): s is string => Boolean(s?.trim())),
  })),
);
// An interlude has no entry of its own: take the last line before it.
const originIndex = computed(() => {
  const pressed = request.value?.lineIndex ?? 0;
  return usable.value.reduce((found, { raw }, i) => (raw <= pressed ? i : found), 0);
});

// The pressed line opens selected and scrolled to exactly where it was
// pressed, so the layer takes it over without it moving.
watch(
  visible,
  (shown) => {
    if (!shown || !request.value) return;
    range.value = [originIndex.value, originIndex.value];
    clearImage();
    const anchor = lineEls[originIndex.value];
    if (!anchor || !list.value) return;
    const listTop = list.value.getBoundingClientRect().top;
    scroller.value?.scrollTo({ top: anchor.offsetTop - (request.value.origin.rect.top - listTop) });
  },
  { flush: "post" },
);

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
  if (offset) scroller.value?.scrollBy({ top: offset, behavior: "smooth" });
}

const close = () => navigation.closeTop("share");

function leave() {
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

const makeImage = () =>
  run(async () => {
    rendering.value = true;
    try {
      imageBlob = await renderLyricCard({
        lines: selected.value,
        title: song.value?.name ?? "",
        artist: artists.value,
        coverSrc: coverUrl(song.value?.album?.picUrl, 512),
        backdrop: await props.captureStill(),
        themeRgb: site.songPicColor,
        fontFamily: setting.lyricFont,
        brand: "GMPlayer",
        logoSrc: "/images/logo/favicon.png",
      });
      imageUrl.value = URL.createObjectURL(imageBlob);
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
    await navigator.clipboard.write([new ClipboardItem({ "image/png": imageBlob! })]);
    window.$message.success(t("general.message.copySuccess"));
  });

const nativeImageCall = async (cmd: string) =>
  getTauri()!.core.invoke<boolean | null>(cmd, new Uint8Array(await imageBlob!.arrayBuffer()), {
    headers: { "x-share": new URLSearchParams({ name: fileName.value }).toString() },
  });

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
    const file = new File([imageBlob!], `${fileName.value || "lyrics"}.png`, { type: "image/png" });
    await navigator.share({ files: [file] }).catch(ignoreAbort);
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
  font-family: var(--share-font);
  font-weight: var(--share-weight);
  letter-spacing: var(--share-spacing);
  text-align: var(--share-align);
}

// AMLL's line box and type (`lyricMainLine` 1em / 1.2, `lyricSubLine` 0.5em / 1.5).
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
  .foot {
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
