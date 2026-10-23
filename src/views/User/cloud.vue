<template>
  <div class="cloud">
    <div class="data">
      <n-button class="up" type="primary" strong secondary round @click="upSongRef.click()">
        <template #icon>
          <n-icon :component="BackupRound" />
        </template>
        {{ $t("general.name.upCloud") }}
      </n-button>
      <input
        ref="upSongRef"
        type="file"
        style="display: none"
        accept="audio/*"
        @change="upCloudSongData"
      />
      <div class="space" v-if="cloudSpace[0]">
        <span>{{ cloudSpace[0] }} G</span>
        <n-popover trigger="hover">
          <template #trigger>
            <n-progress
              type="line"
              :color="setting.themeData.primaryColor"
              class="progress"
              :show-indicator="false"
              :percentage="100 / (cloudSpace[1] / cloudSpace[0])"
            />
          </template>
          <n-text>
            {{
              $t("general.name.cloudUsed", {
                used: (100 / (cloudSpace[1] / cloudSpace[0])).toFixed(),
                remaining: (cloudSpace[1] - cloudSpace[0]).toFixed(),
              })
            }}
          </n-text>
        </n-popover>
        <span>{{ cloudSpace[1] }} G</span>
      </div>
    </div>
    <div class="song-panel">
      <DataLists
        :listData="cloudData"
        virtual
        virtual-height="min(68vh, 760px)"
        :virtual-item-size="54"
        :virtual-threshold="40"
      />
    </div>
    <Pagination
      :totalCount="totalCount"
      :pageNumber="pageNumber"
      @pageSizeChange="pageSizeChange"
      @pageNumberChange="pageNumberChange"
    />
    <!-- 上传进度弹窗 -->
    <n-modal
      class="s-modal close"
      v-model:show="upSongModal"
      preset="card"
      :title="$t('general.name.upCloud')"
      :auto-focus="false"
      :bordered="false"
      :close-on-esc="false"
      :esc="false"
      :mask-closable="false"
    >
      <n-progress
        type="line"
        :status="upSongType"
        :percentage="upSongCompleted"
        :indicator-placement="'inside'"
        processing
      />
      <template #footer>
        <n-space justify="end" v-if="upSongType === 'error'">
          <n-button @click="closeUpSongModal">
            {{ $t("general.dialog.cancel") }}
          </n-button>
          <n-button type="primary" @click="resetUpSongModal">
            {{ $t("general.dialog.resetUp") }}
          </n-button>
        </n-space>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { getCloud, upCloudSong } from "@/api/user";
import { useRoute } from "vue-router";
import { useLayerNavigation } from "@/utils/navigation";
import { settingStore } from "@/store";
import { asRawEntry } from "@/utils/rawEntry";
import { getSongTime } from "@/utils/timeTools";
import { BackupRound } from "@vicons/material";
import { useI18n } from "vue-i18n";
import DataLists from "@/components/DataList/DataLists.vue";
import Pagination from "@/components/Pagination/index.vue";
import type { ProgressStatus } from "naive-ui";

const { t } = useI18n();
const route = useRoute();
const navigation = useLayerNavigation();
const setting = settingStore();

// 云盘数据
const cloudSpace = ref([]);
const cloudData = ref([]);
const pagelimit = ref(30);
const pageNumber = ref(route.query.page ? Number(route.query.page) : 1);
const totalCount = ref(0);

// 上传歌曲数据
const upSongRef = ref(null);
const upSongType = ref<ProgressStatus>("success");
const upSongModal = ref(false);
const upSongCompleted = ref(0);

// 获取云盘数据
const getCloudData = (limit = 30, offset = 0, scroll = true) => {
  if (scroll && typeof $scrollToTop !== "undefined") $scrollToTop();
  getCloud(limit, offset).then((res) => {
    console.log(res);
    totalCount.value = res.count;
    cloudData.value = [];
    // 云盘空间
    cloudSpace.value = [
      (res.size / Math.pow(1024, 3)).toFixed(2),
      (res.maxSize / Math.pow(1024, 3)).toFixed(0),
    ];
    // 全部歌曲
    if (res.data) {
      res.data.forEach((v, i) => {
        cloudData.value.push(
          asRawEntry({
            id: v.songId,
            num: i + 1 + (pageNumber.value - 1) * pagelimit.value,
            name: v.simpleSong.name,
            artist: v.simpleSong.ar,
            album: v.simpleSong.al,
            alia: v.simpleSong.alia,
            mv: v.simpleSong.mv,
            time: getSongTime(v.simpleSong.dt),
          }),
        );
      });
    } else {
      $message.error(t("general.message.acquisitionFailed"));
    }
  });
};

// 上传进度条
const onUploadProgress = (progressEvent) => {
  const { loaded, total } = progressEvent;
  const percentCompleted = Math.round((loaded * 100) / total);
  upSongCompleted.value = Number(percentCompleted);
};

// 歌曲上传
const upCloudSongData = (e) => {
  console.log(e);
  const files = e.target.files;
  if (!files[0]) return false;
  upSongType.value = "success";
  upSongModal.value = true;
  upCloudSong(files[0], onUploadProgress)
    .then((res) => {
      console.log(res);
      if (res.code === 200) {
        closeUpSongModal();
        if (!res.privateCloud.simpleSong.al?.name) {
          $message.warning(t("general.message.upCloudNotHas"));
        }
        $message.success(
          t("general.message.upCloudSuccess", {
            name: res.privateCloud.simpleSong?.name,
          }),
        );
        getCloudData(pagelimit.value, (pageNumber.value - 1) * pagelimit.value);
      } else {
        upSongType.value = "error";
        $message.error(t("general.message.upCloudError"));
        console.error(t("general.message.upCloudError"));
      }
    })
    .catch((err) => {
      upSongType.value = "error";
      closeUpSongModal();
      $message.error(t("general.message.upCloudFailure"));
      console.error(t("general.message.upCloudFailure"), err);
    });
};

// 关闭上传弹窗
const closeUpSongModal = () => {
  upSongModal.value = false;
  upSongCompleted.value = 0;
  upSongRef.value.value = null;
};

// 重新上传
const resetUpSongModal = () => {
  closeUpSongModal();
  upSongRef.value.click();
};

// 每页个数数据变化
const pageSizeChange = (val) => {
  console.log(val);
  pagelimit.value = val;
  getCloudData(val, (pageNumber.value - 1) * pagelimit.value);
};

// 当前页数数据变化
const pageNumberChange = (val) => {
  navigation.replacePage({
    path: "/user/cloud",
    query: {
      page: val,
    },
  });
};

// 当前页数据重载
const cloudDataLoad = (scroll = false) => {
  getCloudData(pagelimit.value, (pageNumber.value - 1) * pagelimit.value, scroll);
};
provide("cloudDataLoad", cloudDataLoad);

// 监听路由参数变化
watch(
  () => route.fullPath,
  () => {
    const val = route;
    if (val.name === "user-cloud") {
      pageNumber.value = Number(val.query.page ? val.query.page : 1);
      getCloudData(pagelimit.value, (pageNumber.value - 1) * pagelimit.value);
    }
  },
);

onMounted(() => {
  $setSiteTitle(t("nav.user") + " - " + t("nav.userChildren.cloud"));
  getCloudData(pagelimit.value, (pageNumber.value - 1) * pagelimit.value);
});
</script>

<style lang="scss" scoped>
.cloud {
  .data {
    width: 100%;
    margin: 20px 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    .space {
      width: 160px;
      display: flex;
      align-items: center;
      span {
        white-space: nowrap;
        font-size: 13px;
      }
      .progress {
        margin: 0 8px;
      }
    }
  }

  .song-panel {
    --detail-song-list-radius: var(--radius-md);

    width: 100%;
    min-width: 0;

    :deep(.datalists .songs) {
      --n-color: transparent;
      --n-border-color: transparent;

      margin-bottom: 0;
      border: 0;
      border-radius: 0;
      background-color: transparent;
      box-shadow: none;
    }

    :deep(.datalists .songs:nth-child(odd)),
    :deep(.datalists .songs.song-row-odd) {
      background-color: color-mix(in srgb, var(--n-text-color) 3%, transparent);
    }

    :deep(.datalists .songs:nth-child(even)),
    :deep(.datalists .songs.song-row-even) {
      background-color: color-mix(in srgb, var(--n-text-color) 6%, transparent);
    }

    :deep(.datalists .songs.song-row-first) {
      border-radius: var(--detail-song-list-radius) var(--detail-song-list-radius) 0 0;
    }

    :deep(.datalists .songs.song-row-last) {
      border-radius: 0 0 var(--detail-song-list-radius) var(--detail-song-list-radius);
    }

    :deep(.datalists .songs.song-row-single) {
      border-radius: var(--detail-song-list-radius);
    }

    :deep(.datalists .songs:hover) {
      background-color: color-mix(in srgb, var(--n-text-color) 10%, transparent);
      box-shadow: none;
    }

    :deep(.datalists .songs.play) {
      background-color: color-mix(in srgb, var(--main-color) 13%, transparent);
    }

    :deep(.datalists .songs .n-card__content) {
      min-height: 52px;
      padding: 8px 12px !important;
    }

    :deep(.datalists .songs .pic),
    :deep(.datalists .songs .num) {
      width: 38px;
      height: 38px;
      min-width: 38px;
      margin-right: 14px;
      border-radius: var(--radius-sm);
      font-size: 13px;
    }

    :deep(.datalists .songs .name .title) {
      font-size: 14px;
    }

    :deep(.datalists .songs .name .meta) {
      font-size: 12px;
    }

    :deep(.datalists .songs .album) {
      font-size: 13px;
      opacity: 0.72;
    }

    :deep(.datalists .songs .time) {
      font-size: 12px;
      opacity: 0.64;
    }

    :deep(.datalists .songs .action) {
      width: 76px;
    }
  }

  @media (max-width: 768px) {
    .song-panel {
      :deep(.datalists .songs .n-card__content) {
        min-height: 58px;
        padding: 9px 6px !important;
      }

      :deep(.datalists .songs .pic),
      :deep(.datalists .songs .num) {
        width: 42px;
        height: 42px;
        min-width: 42px;
        margin-right: 11px;
      }

      :deep(.datalists .songs .name) {
        padding-right: 8px;
      }

      :deep(.datalists .songs .album),
      :deep(.datalists .songs .time) {
        display: none;
      }
    }
  }
}
</style>
