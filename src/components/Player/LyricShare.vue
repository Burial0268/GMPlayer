<template>
  <n-modal v-model:show="show" :z-index="2200" :auto-focus="false" @after-leave="clearImage">
    <div
      class="lyric-share"
      :style="{
        '--share-theme': `rgb(${site.songPicColor})`,
        '--lyric-share-font': setting.lyricFont,
      }"
    >
      <img class="backdrop" :src="cover" alt="" aria-hidden="true" />
      <header class="head">
        <img class="cover" :src="cover" alt="" />
        <div class="meta">
          <div class="title">{{ song()?.name }}</div>
          <div class="artist">{{ artists() }}</div>
        </div>
        <n-button quaternary circle :aria-label="t('general.dialog.cancel')" @click="show = false">
          <template #icon><Close /></template>
        </n-button>
      </header>

      <div v-if="!imageUrl" class="lines" role="group" :aria-label="t('player.share.title')">
        <div
          v-for="(line, index) in lines"
          :key="index"
          :ref="(el) => index === range[0] && (anchorEl = el as HTMLElement)"
          :class="['line', { selected: isSelected(index) }]"
          role="checkbox"
          :aria-checked="isSelected(index)"
          tabindex="0"
          @click="toggle(index)"
          @keydown.enter.space.prevent="toggle(index)"
        >
          <div class="text">{{ line.text }}</div>
          <div v-for="(sub, i) in line.subs" :key="i" class="sub">{{ sub }}</div>
        </div>
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
            <n-button class="primary" round :loading="rendering" @click="makeImage">
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
            <n-button v-if="!isMobileApp" class="primary" round @click="saveImage">
              <template #icon><Download /></template>
              {{ t("player.share.saveImage") }}
            </n-button>
            <n-button v-if="canShareImage" class="primary" round @click="shareImage">
              <template #icon><ShareTwo /></template>
              {{ t("player.share.share") }}
            </n-button>
          </div>
        </template>
      </footer>
    </div>
  </n-modal>
</template>

<script setup lang="ts">
import { computed, nextTick, ref, shallowRef } from "vue";
import { Close, Copy, Download, Left, Pic, ShareTwo } from "@icon-park/vue-next";
import { useI18n } from "vue-i18n";
import { musicStore, settingStore, siteStore } from "@/store";
import { coverUrl } from "@/utils/coverUrl";
import { isInterludeLine, type AMLLLine } from "@/utils/LyricsProcessor";
import { getTauri, isTauri } from "@/utils/tauri/core/runtime";
import { isMobileDevice } from "@/utils/tauri/platform/mobile";
import { renderLyricCard, type LyricShareLine } from "@/utils/lyricShareCard";

const MAX_LINES = 5;

const { t } = useI18n();
const music = musicStore();
const setting = settingStore();
const site = siteStore();

// The share commands are registered for mobile builds, the saver for desktop.
const isMobileApp = isTauri() && isMobileDevice();
const canShareText = isMobileApp || (!isTauri() && typeof navigator.share === "function");
const canShareImage = isMobileApp || (!isTauri() && typeof navigator.canShare === "function");
// Mobile WebViews have no image clipboard; the share sheet covers it there.
const canCopyImage = !isMobileApp && typeof ClipboardItem !== "undefined";

const show = ref(false);
const lines = shallowRef<LyricShareLine[]>([]);
const range = ref<[number, number]>([0, 0]);
const anchorEl = ref<HTMLElement | null>(null);
const rendering = ref(false);
const imageUrl = ref("");
let imageBlob: Blob | null = null;

/** Opens on the line that was right-clicked / long-pressed. */
function open(amllLines: AMLLLine[], lineIndex: number) {
  const usable = amllLines.flatMap((line, raw) =>
    line.isBG || isInterludeLine(line) ? [] : [{ line, raw }],
  );
  if (!usable.length) return;
  lines.value = usable.map(({ line }) => ({
    text: line.words.map((w) => w.word).join(""),
    subs: [line.translatedLyric, line.romanLyric].filter((s): s is string => Boolean(s?.trim())),
  }));
  // An interlude has no entry of its own: take the last line before it.
  const index = usable.reduce((found, { raw }, i) => (raw <= lineIndex ? i : found), 0);
  range.value = [index, index];
  clearImage();
  show.value = true;
  nextTick(() => anchorEl.value?.scrollIntoView({ block: "center" }));
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

const isSelected = (index: number) => index >= range.value[0] && index <= range.value[1];
const cover = computed(() => coverUrl(song()?.album?.picUrl, 256));
const selected = () => lines.value.slice(range.value[0], range.value[1] + 1);
const song = () => music.getPlaySongData;
const artists = () => (song()?.artist ?? []).map((a) => a.name).join(" / ");
const lyricText = () =>
  selected()
    .map((line) => [line.text, ...line.subs].join("\n"))
    .join("\n");
const fileName = () => [song()?.name, artists()].filter(Boolean).join(" - ");

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
    await navigator.clipboard.writeText(lyricText());
    window.$message.success(t("general.message.copySuccess"));
  });

const shareText = () =>
  run(async () => {
    const text = `${lyricText()}\n\n—— ${fileName()}`;
    if (isMobileApp) await getTauri()!.core.invoke("share_text", { text });
    else await navigator.share({ text }).catch(ignoreAbort);
  });

const makeImage = () =>
  run(async () => {
    rendering.value = true;
    try {
      imageBlob = await renderLyricCard({
        lines: selected(),
        title: song()?.name ?? "",
        artist: artists(),
        coverSrc: coverUrl(song()?.album?.picUrl, 512),
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
    headers: { "x-share": new URLSearchParams({ name: fileName() }).toString() },
  });

const saveImage = () =>
  run(async () => {
    if (isTauri()) {
      if (await nativeImageCall("share_save_image")) {
        window.$message.success(t("player.share.saved"));
      }
      return;
    }
    const a = document.createElement("a");
    a.href = imageUrl.value;
    a.download = `${fileName() || "lyrics"}.png`;
    a.click();
  });

const shareImage = () =>
  run(async () => {
    if (isMobileApp) {
      await nativeImageCall("share_image");
      return;
    }
    const file = new File([imageBlob!], `${fileName() || "lyrics"}.png`, { type: "image/png" });
    await navigator.share({ files: [file] }).catch(ignoreAbort);
  });

// Dismissing the browser share sheet rejects with AbortError; that is not a failure.
function ignoreAbort(error: unknown) {
  if (!(error instanceof DOMException && error.name === "AbortError")) throw error;
}

defineExpose({ open });
</script>

<style lang="scss" scoped>
.lyric-share {
  position: relative;
  display: flex;
  flex-direction: column;
  width: min(520px, calc(100vw - 32px));
  height: min(720px, calc(100vh - 64px));
  overflow: hidden;
  border-radius: 24px;
  color: #fff;
  background-color: var(--share-theme);
  box-shadow: 0 24px 64px rgb(0 0 0 / 0.35);

  // The cover, blurred past recognition, under a shade dark enough for white
  // text on any artwork — the same treatment the generated card gets.
  .backdrop {
    position: absolute;
    inset: -20%;
    width: 140%;
    height: 140%;
    object-fit: cover;
    filter: blur(60px) saturate(1.4);
    pointer-events: none;
  }

  &::before {
    content: "";
    position: absolute;
    inset: 0;
    background: rgb(0 0 0 / 0.32);
    pointer-events: none;
  }

  > * {
    position: relative;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 20px 16px 12px 20px;

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

    :deep(.n-button) {
      color: #fff;
    }
  }

  .lines {
    flex: 1;
    overflow-y: auto;
    padding: 8px 12px;
    scrollbar-width: none;
    font-family: var(--lyric-share-font);

    &::-webkit-scrollbar {
      display: none;
    }
  }

  .line {
    padding: 10px 12px;
    border-radius: 14px;
    cursor: pointer;
    opacity: 0.35;
    outline: none;
    user-select: none;
    transition:
      opacity var(--duration-200, 0.2s),
      background-color var(--duration-200, 0.2s);

    .text {
      font-size: 24px;
      font-weight: 800;
      line-height: 1.3;
    }

    .sub {
      margin-top: 2px;
      font-size: 15px;
      font-weight: 600;
      opacity: 0.75;
    }

    &:hover {
      opacity: 0.6;
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
    padding: 8px 20px;

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
    padding: 12px 16px 16px 20px;
    background: linear-gradient(transparent, rgb(0 0 0 / 0.18));

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

    :deep(.n-button) {
      --n-color: rgb(255 255 255 / 0.16);
      --n-color-hover: rgb(255 255 255 / 0.24);
      --n-color-pressed: rgb(255 255 255 / 0.3);
      --n-color-focus: rgb(255 255 255 / 0.24);
      --n-text-color: #fff;
      --n-text-color-hover: #fff;
      --n-text-color-pressed: #fff;
      --n-text-color-focus: #fff;
      font-weight: 600;
    }

    :deep(.n-button.primary) {
      --n-color: rgb(255 255 255 / 0.92);
      --n-color-hover: #fff;
      --n-color-pressed: rgb(255 255 255 / 0.8);
      --n-color-focus: #fff;
      --n-text-color: #111;
      --n-text-color-hover: #111;
      --n-text-color-pressed: #111;
      --n-text-color-focus: #111;
      --n-border: none;
      --n-border-hover: none;
      --n-border-pressed: none;
      --n-border-focus: none;
    }
  }

  // Phones get the whole screen, like the sheet it imitates.
  @media (max-width: 768px) {
    width: 100vw;
    height: 100dvh;
    border-radius: 0;
    padding-top: env(safe-area-inset-top);
    padding-bottom: env(safe-area-inset-bottom);

    .line .text {
      font-size: 22px;
    }

    .foot {
      .count {
        width: 100%;
      }

      .actions {
        width: 100%;
        margin-left: 0;

        :deep(.n-button) {
          flex: 1;
        }
      }
    }
  }
}
</style>
