<template>
  <n-modal
    class="s-modal downloadModal"
    v-model:show="downloadModal"
    preset="card"
    :title="$t('menu.download')"
    :bordered="false"
    :on-after-leave="closeDownloadModal"
  >
    <Transition mode="out-in">
      <div v-if="songData">
        <SmallSongData ref="smallSongDataRef" :songData="songData" notJump />
        <n-alert v-if="songData.pc" class="tip" type="info" :show-icon="false">
          {{ $t("other.cloudTip") }}
        </n-alert>
        <n-radio-group class="downloadGroup" v-model:value="downloadChoose" name="downloadGroup">
          <n-space vertical>
            <n-radio
              v-for="item in downloadLevel"
              :key="item"
              :value="item.value"
              :disabled="item.disabled"
            >
              <div :class="item.disabled ? 'text disabled' : 'text'">
                <n-text class="name">{{ item.label }}</n-text>
                <n-text v-if="item.size" class="size" :depth="3">
                  {{ item.size }}
                </n-text>
                <n-text v-else-if="!item.disabled" class="error" :depth="3">
                  {{ $t("general.message.acquisitionFailed") }}
                </n-text>
              </div>
            </n-radio>
          </n-space>
        </n-radio-group>
      </div>
      <n-text v-else>{{ $t("general.message.isLoading") }}</n-text>
    </Transition>
    <template #footer>
      <n-space justify="end">
        <n-button @click="closeDownloadModal">
          {{ $t("general.dialog.cancel") }}
        </n-button>
        <n-button
          :disabled="!downloadChoose"
          :loading="downloadStatus"
          type="primary"
          @click="toSongDownload"
        >
          {{ downloadStatus ? $t("general.dialog.downloadingNow") : $t("general.dialog.download") }}
        </n-button>
      </n-space>
    </template>
  </n-modal>
</template>

<script setup>
import { userStore, useDownloadStore } from "@/store";
import { useRouter } from "vue-router";
import { getMusicDetail } from "@/api/song";
import { downloadAvailable } from "@/utils/download";
import { useI18n } from "vue-i18n";
import SmallSongData from "@/components/DataList/SmallSongData.vue";

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
  console.log(downloadLevel.value);
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
const closeDownloadModal = () => {
  songId.value = null;
  songData.value = null;
  downloadStatus.value = false;
  downloadModal.value = false;
  downloadChoose.value = null;
};

// 暴露方法
defineExpose({
  openDownloadModal,
});
</script>

<style lang="scss" scoped>
.downloadModal {
  .v-enter-active,
  .v-leave-active {
    transition: opacity var(--duration-300) var(--ease-out);
  }

  .v-enter-from,
  .v-leave-to {
    opacity: 0;
  }
  .tip {
    margin-top: 20px;
  }
  .downloadGroup {
    margin-top: 20px;
    .text {
      &.disabled {
        span {
          color: var(--n-text-color-disabled);
        }
      }
      .size {
        font-size: 13px;
        &::before {
          content: "-";
          margin: 0 4px;
        }
        &::after {
          content: "Mb";
          margin-left: 4px;
        }
      }
      .error {
        font-size: 13px;
        &::before {
          content: "-";
          transform: translateY(-1.5px);
          display: inline-block;
          margin: 0 4px;
        }
      }
    }
  }
}
</style>
