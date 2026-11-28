<template>
  <GlassSheet v-model:show="downloadModal" :label="$t('menu.download')">
    <template #header>
      <div class="sheet-song">
        <img class="sheet-cover" :src="coverUrl(songData?.album?.picUrl, 128)" alt="" />
        <div class="sheet-song-text">
          <div class="sheet-song-name text-hidden">
            {{ songData?.name ?? $t("general.message.isLoading") }}
          </div>
          <div class="sheet-song-artists text-hidden">
            {{ (songData?.artist ?? []).map((artist) => artist?.name).join(" / ") }}
          </div>
        </div>
      </div>
    </template>

    <p v-if="songData?.pc" class="cloud-tip">{{ $t("other.cloudTip") }}</p>
    <section class="sheet-group" role="radiogroup" :aria-label="$t('menu.download')">
      <template v-if="downloadLevel">
        <button
          v-for="item in downloadLevel"
          :key="item.value"
          type="button"
          role="radio"
          class="sheet-row"
          :aria-checked="downloadChoose === item.value"
          :disabled="item.disabled"
          @click="downloadChoose = item.value"
        >
          <span class="row-label">{{ item.label }}</span>
          <span class="row-meta">
            <template v-if="item.size">{{ item.size }} MB</template>
            <template v-else-if="!item.disabled">
              {{ $t("general.message.acquisitionFailed") }}
            </template>
          </span>
          <n-icon
            class="row-icon check"
            size="20"
            :component="CheckRound"
            :style="{ visibility: downloadChoose === item.value ? 'visible' : 'hidden' }"
          />
        </button>
      </template>
      <template v-else>
        <div v-for="index in 5" :key="index" class="sheet-row">
          <n-skeleton text style="width: 40%" />
        </div>
      </template>
    </section>

    <template #footer>
      <n-button strong secondary round @click="closeDownloadModal">
        {{ $t("general.dialog.cancel") }}
      </n-button>
      <n-button
        :disabled="!downloadChoose"
        :loading="downloadStatus"
        type="primary"
        strong
        secondary
        round
        @click="toSongDownload"
      >
        {{ downloadStatus ? $t("general.dialog.downloadingNow") : $t("general.dialog.download") }}
      </n-button>
    </template>
  </GlassSheet>
</template>

<script setup>
import { userStore, useDownloadStore } from "@/store";
import { useRouter } from "vue-router";
import { getMusicDetail } from "@/api/song";
import { downloadAvailable } from "@/utils/library/download";
import { useI18n } from "vue-i18n";
import { CheckRound } from "@vicons/material";
import { coverUrl } from "@/utils/ncm/coverUrl";
import GlassSheet from "@/components/Common/GlassSheet.vue";

const { t } = useI18n();
const user = userStore();
const download = useDownloadStore();
const router = useRouter();

// 歌曲下载数据
const songId = ref(null);
const songData = ref(null);
const downloadStatus = ref(false);
const downloadModal = ref(false);
const downloadChoose = ref(null);
const downloadLevel = ref(null);

/**
 * 把这首歌交给 Rust 的下载队列。
 *
 * 这里**不再**自己发请求。以前是 `fetch` → `blob` → 造一个 `<a download>` 点一下：
 * Android 上完全无效（WebView 没有 DownloadListener，`blob:` 也交不给系统下载器），
 * 桌面上落点归 WebView2 所有、应用拿不到路径因此无法入库，而且整首歌会先在渲染进程
 * 里驻留一份。签名 URL 的解析也挪到了队列里——它会过期，批量下载的队尾必然 403。
 *
 * 入队即关：进度在下载管理器里看，把弹窗按在这里等一首 40 MB 的无损没有意义。
 */
const toSongDownload = async () => {
  if (!songId.value || !downloadChoose.value) return;
  downloadStatus.value = true;
  try {
    const added = await download.enqueue([
      {
        songId: songId.value,
        title: songData.value?.name ?? t("general.name.unknownSong"),
        artist: songData.value?.artist?.[0]?.name ?? "",
        album: songData.value?.album?.name ?? "",
        br: Number(downloadChoose.value),
        coverUrl: songData.value?.album?.picUrl ?? undefined,
      },
    ]);
    if (added > 0) {
      $message.success(t("download.queued", { count: added }));
      closeDownloadModal();
    } else {
      // 0 有两种来路：这首已经在队列里，或者用户在目录选择器上点了取消。
      downloadStatus.value = false;
    }
  } catch (error) {
    console.error(t("general.message.downloadError"), error);
    $message.error(t("general.message.downloadError"));
    downloadStatus.value = false;
  }
};

// 获取歌曲详情
const getMusicDetailData = (id) => {
  getMusicDetail(id)
    .then((res) => {
      if (res.songs[0] && res.privileges[0]) {
        songData.value = {
          album: res.songs[0].al,
          artist: res.songs[0].ar,
          name: res.songs[0].name,
          id: res.songs[0].id,
          pc: res.songs[0]?.pc,
        };
        // 生成音质列表
        generateLists(res);
      } else {
        $message.error(t("general.message.acquisitionFailed"));
      }
    })
    .catch((err) => {
      closeDownloadModal();
      console.error(t("general.message.acquisitionFailed"), err);
      $message.error(t("general.message.acquisitionFailed"));
    });
};

// 生成可下载列表
const generateLists = (data) => {
  const br = data.privileges[0].downloadMaxbr;
  downloadLevel.value = [
    {
      value: "128000",
      label: t("general.type.quality.l"),
      disabled: br >= 128000 ? false : true,
      size: getSongSize(data, "l"),
    },
    {
      value: "192000",
      label: t("general.type.quality.m"),
      disabled: br >= 192000 ? false : true,
      size: getSongSize(data, "m"),
    },
    {
      value: "320000",
      label: t("general.type.quality.h"),
      disabled: br >= 320000 ? false : true,
      size: getSongSize(data, "h"),
    },
    {
      value: "420000",
      label: t("general.type.quality.sq"),
      disabled: [128000, 192000, 320000].includes(parseInt(br)),
      size: getSongSize(data, "sq"),
    },
    {
      value: "999000",
      label: "Hi-Res",
      disabled: br >= 999000 ? false : true,
      size: getSongSize(data, "hr"),
    },
  ];
  // 预选：设置里的默认音质，前提是这首歌确实给这个档位。不给就退回最高的可用档，
  // 免得用户每次都得先点一下才能按「下载」。
  const preferred = download.settings?.defaultBr;
  const enabled = downloadLevel.value.filter((item) => !item.disabled);
  const match = enabled.find((item) => Number(item.value) === Number(preferred));
  downloadChoose.value = (match ?? enabled[enabled.length - 1])?.value ?? null;
};

// 获取下载大小
const getSongSize = (data, type) => {
  let fileSize = 0;
  // 转换文件大小
  const convertSize = (num) => {
    if (!num) return 0;
    return (num / (1024 * 1024)).toFixed(2);
  };
  if (type === "l") {
    fileSize = convertSize(data.songs[0]?.l?.size);
  } else if (type === "m") {
    fileSize = convertSize(data.songs[0]?.m?.size);
  } else if (type === "h") {
    fileSize = convertSize(data.songs[0]?.h?.size);
  } else if (type === "sq") {
    fileSize = convertSize(data.songs[0]?.sq?.size);
  } else if (type === "hr") {
    fileSize = convertSize(data.songs[0]?.hr?.size);
  }
  return fileSize;
};

// 开启歌曲下载
const openDownloadModal = (data) => {
  // Web 端没有可以落盘、并且随后能被本地库索引的位置，所以这里直说而不是让用户
  // 走到底再失败。
  if (!downloadAvailable()) {
    $message.error(t("download.tauriOnly"));
    return;
  }
  if (user.userLogin) {
    if (
      router.currentRoute.value.name === "user-cloud" ||
      user.userData?.vipType ||
      data?.fee === 0 ||
      data?.pc
    ) {
      songId.value = data.id;
      songData.value = null;
      downloadLevel.value = null;
      downloadChoose.value = null;
      downloadModal.value = true;
      download.hydrate();
      getMusicDetailData(data.id);
    } else {
      $message.error(t("general.message.needVip"));
    }
  } else {
    $message.error(t("general.message.needLogin"));
  }
};

// 关闭歌曲下载
// 数据留到下次打开时再清：面板收起动画期间还要显示这首歌。
const closeDownloadModal = () => {
  downloadStatus.value = false;
  downloadModal.value = false;
};

// 暴露方法
defineExpose({
  openDownloadModal,
});
</script>

<style lang="scss" scoped>
.cloud-tip {
  margin: 0 4px 12px;
  font-size: 0.85rem;
  color: var(--text-secondary);
}

.sheet-row {
  --sheet-row-inset: 14px;
}
</style>
