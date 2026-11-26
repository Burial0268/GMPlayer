<template>
  <GlassSheet v-model:show="cloudMatchModal" :label="$t('menu.match')">
    <template #header>
      <div class="sheet-song">
        <div class="sheet-song-text">
          <div class="sheet-song-name">{{ $t("menu.match") }}</div>
        </div>
      </div>
    </template>

    <p class="group-label">{{ $t("other.sData") }}</p>
    <section class="sheet-group">
      <div class="sheet-row song-row">
        <img class="song-cover" :src="coverUrl(cloudMatchBeforeData?.album?.picUrl, 64)" alt="" />
        <span class="row-label">
          <span class="text-hidden">{{ cloudMatchBeforeData?.name }}</span>
          <span class="row-meta text-hidden">{{ artistsOf(cloudMatchBeforeData) }}</span>
        </span>
      </div>
    </section>

    <p class="group-label">{{ $t("other.asId") }}</p>
    <div class="id-row">
      <n-input-number
        v-model:value="cloudMatchValue.asid"
        :placeholder="$t('other.asIdDes')"
        :show-button="false"
      />
      <n-button
        secondary
        :aria-label="$t('menu.search')"
        @click="toSearch(cloudMatchBeforeData?.name)"
      >
        <template #icon>
          <n-icon :component="Search" />
        </template>
      </n-button>
      <n-button
        secondary
        :disabled="!cloudMatchValue.asid"
        :loading="checking"
        @click="checkTarget"
      >
        {{ $t("general.dialog.check") }}
      </n-button>
    </div>

    <section v-if="target" class="sheet-group target">
      <div class="sheet-row song-row">
        <img class="song-cover" :src="coverUrl(target.album?.picUrl, 64)" alt="" />
        <span class="row-label">
          <span class="text-hidden">{{ target.name }}</span>
          <span class="row-meta text-hidden">{{ artistsOf(target) }}</span>
        </span>
      </div>
    </section>

    <template #footer>
      <n-button strong secondary round @click="closeCloudMatch">
        {{ $t("general.dialog.cancel") }}
      </n-button>
      <n-button
        type="primary"
        strong
        secondary
        round
        :disabled="!cloudMatchValue.asid"
        @click="setCloudMatchBtn(cloudMatchValue)"
      >
        {{ $t("general.dialog.match") }}
      </n-button>
    </template>
  </GlassSheet>
</template>

<script setup>
import { setCloudMatch } from "@/api/user";
import { userStore } from "@/store";
import { useI18n } from "vue-i18n";
import { Search } from "@icon-park/vue-next";
import { getMusicDetail } from "@/api/song";
import { coverUrl } from "@/utils/ncm/coverUrl";
import GlassSheet from "@/components/Common/GlassSheet.vue";

const { t } = useI18n();
const user = userStore();

// 歌曲信息纠正数据
const cloudDataLoad = inject("cloudDataLoad", null);
const cloudMatchModal = ref(false);
const cloudMatchBeforeData = ref(null);
// 「检查」查到的目标歌曲；只有查到了才允许匹配。
const target = ref(null);
const checking = ref(false);
const artistsOf = (song) => (song?.artist ?? []).map((artist) => artist?.name).join(" / ");

const checkTarget = async () => {
  const asid = cloudMatchValue.value.asid;
  if (!asid) return;
  checking.value = true;
  target.value = null;
  try {
    const res = await getMusicDetail(String(asid));
    const song = res.songs?.[0];
    // 输入框在查询期间可能又改了：只认仍然对应当前输入的结果。
    if (song && cloudMatchValue.value.asid === asid) {
      target.value = { name: song.name, artist: song.ar, album: song.al };
    } else if (!song) {
      $message.error(t("general.message.acquisitionFailed"));
    }
  } catch (err) {
    console.error(t("general.message.acquisitionFailed"), err);
    $message.error(t("general.message.acquisitionFailed"));
  } finally {
    checking.value = false;
  }
};
// 换了 id 之前的检查结果就不算数了。
watch(
  () => cloudMatchValue.value.asid,
  () => (target.value = null),
);
const cloudMatchValue = ref({
  uid: user.getUserData.userId,
  sid: null,
  asid: null,
});

// 歌曲纠正
const setCloudMatchBtn = (data) => {
  if (data.sid === data.asid) {
    $message.error(t("other.noNeedMatch"));
  } else {
    if (!target.value) {
      $message.error(t("other.plaseCheck"));
    } else {
      setCloudMatch(data.uid, data.sid, data.asid).then((res) => {
        if (res.data) {
          closeCloudMatch();
          $message.success(t("other.matchSuccess"));
          cloudDataLoad();
        } else {
          $message.error(t("other.matchFailed"));
        }
      });
    }
  }
};

// 同名搜索
const toSearch = (name) => {
  window.open(`/search/songs?keywords=${name}&page=1`);
};

// 开启歌曲纠正
const openCloudMatch = (data) => {
  cloudMatchValue.value.sid = data.id;
  cloudMatchValue.value.asid = null;
  target.value = null;
  cloudMatchBeforeData.value = data;
  cloudMatchModal.value = true;
};

// 关闭歌曲纠正
// 原曲留到下次打开时再换：面板收起动画期间还要显示它。
const closeCloudMatch = () => {
  cloudMatchModal.value = false;
};

// 暴露方法
defineExpose({
  openCloudMatch,
});
</script>

<style lang="scss" scoped>
.group-label {
  margin: 4px 4px 8px;
  font-size: 0.82rem;
  color: var(--text-secondary);
}

.sheet-group + .group-label {
  margin-top: 16px;
}

.song-row {
  --sheet-row-inset: 70px;
  min-height: 64px;
}

.song-cover {
  width: 42px;
  height: 42px;
  flex-shrink: 0;
  border-radius: var(--radius-sm);
  object-fit: cover;
  background: var(--sheet-fill);
}

.row-label {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.id-row {
  display: flex;
  gap: 8px;

  :deep(.n-input-number) {
    flex: 1;
  }
}

.target {
  margin-top: 12px;
}
</style>
