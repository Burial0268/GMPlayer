<template>
  <n-modal
    v-model:show="show"
    :bordered="false"
    :z-index="10000"
    class="s-modal lyric-share"
    preset="card"
    :title="t('player.share.title')"
    @after-leave="clearImage"
  >
    <template v-if="!imageUrl">
      <n-scrollbar class="lyric-share-list">
        <div
          v-for="(line, index) in lines"
          :key="index"
          :ref="(el) => index === range[0] && (anchorEl = el as HTMLElement)"
          :class="['line', { selected: index >= range[0] && index <= range[1] }]"
          @click="toggle(index)"
        >
          <div class="text">{{ line.text }}</div>
          <div v-for="(sub, i) in line.subs" :key="i" class="sub">{{ sub }}</div>
        </div>
      </n-scrollbar>
      <div class="lyric-share-footer">
        <span class="count">
          {{ t("player.share.selected", { count: range[1] - range[0] + 1, max: MAX_LINES }) }}
        </span>
        <n-space>
          <n-button strong secondary round @click="copyText">{{ t("player.share.copy") }}</n-button>
          <n-button v-if="canShareText" strong secondary round @click="shareText">
            {{ t("player.share.share") }}
          </n-button>
          <n-button type="primary" strong round :loading="rendering" @click="makeImage">
            {{ t("player.share.image") }}
          </n-button>
        </n-space>
      </div>
    </template>
    <template v-else>
      <n-scrollbar class="lyric-share-list">
        <img class="preview" :src="imageUrl" alt="" />
      </n-scrollbar>
      <div class="lyric-share-footer">
        <n-button strong secondary round @click="clearImage">{{ t("player.share.back") }}</n-button>
        <n-space>
          <n-button v-if="canCopyImage" strong secondary round @click="copyImage">
            {{ t("player.share.copyImage") }}
          </n-button>
          <n-button v-if="!isMobileApp" type="primary" strong round @click="saveImage">
            {{ t("player.share.saveImage") }}
          </n-button>
          <n-button v-if="canShareImage" type="primary" strong round @click="shareImage">
            {{ t("player.share.share") }}
          </n-button>
        </n-space>
      </div>
    </template>
  </n-modal>
</template>

<script setup lang="ts">
import { nextTick, ref, shallowRef } from "vue";
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

/** Opens on the line that was right-clicked / long-pressed, by its start time. */
function open(amllLines: AMLLLine[], startTime: number) {
  const usable = amllLines.filter((line) => !line.isBG && !isInterludeLine(line));
  if (!usable.length) return;
  lines.value = usable.map((line) => ({
    text: line.words.map((w) => w.word).join(""),
    subs: [line.translatedLyric, line.romanLyric].filter((s): s is string => Boolean(s?.trim())),
  }));
  // An interlude or background line has no entry of its own: take the last
  // line that started before it.
  let index = usable.findIndex((line) => line.startTime > startTime) - 1;
  if (index < -1) index = usable.length - 1;
  range.value = [Math.max(0, index), Math.max(0, index)];
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

<style lang="scss">
.n-card.lyric-share {
  width: min(560px, calc(100vw - 32px));
  background-color: rgb(255 255 255 / 0.28);
  color: #fff;
  -webkit-backdrop-filter: blur(28px);
  backdrop-filter: blur(28px);

  .n-card-header .n-card-header__main,
  .n-card-header .n-card-header__close {
    color: #fff;
  }

  .lyric-share-list {
    max-height: min(60vh, 560px);
  }

  .line {
    padding: 10px 14px;
    border-radius: 12px;
    cursor: pointer;
    opacity: 0.45;
    transition:
      opacity var(--duration-200, 0.2s),
      background-color var(--duration-200, 0.2s);
    user-select: none;

    .text {
      font-size: 18px;
      font-weight: 700;
    }

    .sub {
      font-size: 14px;
      opacity: 0.75;
    }

    &.selected {
      opacity: 1;
      background-color: rgb(255 255 255 / 0.16);
    }
  }

  .preview {
    display: block;
    width: 100%;
    border-radius: 12px;
  }

  .lyric-share-footer {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    align-items: center;
    justify-content: space-between;
    margin-top: 16px;

    .count {
      opacity: 0.7;
      font-size: 13px;
    }
  }
}
</style>
