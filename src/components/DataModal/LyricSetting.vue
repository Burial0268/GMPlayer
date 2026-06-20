<template>
  <n-modal
    v-model:show="lyricSettingModal"
    :bordered="false"
    :z-index="10000"
    class="s-modal lyric-set"
    preset="card"
    :title="t('setting.sectionLyrics')"
  >
    <n-scrollbar class="lyric-settings-scroll">
      <SettingsWorkspace section="lyrics" variant="modal" />
      <n-space justify="center">
        <n-button class="more" size="large" strong secondary round @click="openFullSettings">
          {{ t("setting.moreSettings") }}
        </n-button>
      </n-space>
    </n-scrollbar>
  </n-modal>
</template>

<script setup lang="ts">
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { musicStore } from "@/store";
import SettingsWorkspace from "@/components/Settings/SettingsWorkspace.vue";

const router = useRouter();
const music = musicStore();
const { t } = useI18n();
const lyricSettingModal = ref(false);

const openLyricSetting = () => {
  lyricSettingModal.value = true;
};

const openFullSettings = () => {
  lyricSettingModal.value = false;
  music.setBigPlayerState(false);
  router.push("/setting/lyrics");
};

defineExpose({
  openLyricSetting,
});
</script>

<style lang="scss">
.n-card {
  &.lyric-set {
    background-color: rgb(255 255 255 / 0.28);
    color: #fff;
    -webkit-backdrop-filter: blur(28px);
    backdrop-filter: blur(28px);

    .n-card-header {
      .n-card-header__main,
      .n-card-header__close {
        color: #fff;
      }
    }
  }
}
</style>

<style lang="scss" scoped>
.lyric-settings-scroll {
  max-height: min(70vh, 620px);
}

.more {
  margin: 14px 0 4px;
  color: #fff;
  background-color: rgb(255 255 255 / 0.18);

  &:hover {
    background-color: rgb(255 255 255 / 0.26);
  }
}
</style>
