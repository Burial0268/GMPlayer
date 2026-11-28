<template>
  <!-- 高于 GlassSheet（2200）：添加到歌单面板里点「新建」时要盖在它上面。 -->
  <GlassSheet :z-index="2300" v-model:show="createPlaylistShow" :title="$t('menu.create')">
    <n-input
      style="margin-bottom: 12px"
      v-model:value="createName"
      type="text"
      :placeholder="$t('other.newPlaylistName')"
    />
    <n-checkbox v-model:checked="createPrivacy">
      {{ $t("other.setPrivacy") }}
    </n-checkbox>
    <template #footer>
      <n-button @click="closeCreatePlaylist">
        {{ $t("general.dialog.cancel") }}
      </n-button>
      <n-button
        type="primary"
        :disabled="!createName"
        @click="createPlaylistBtn(createName, createPrivacy)"
      >
        {{ $t("general.dialog.create") }}
      </n-button>
    </template>
  </GlassSheet>
</template>

<script setup>
import GlassSheet from "@/components/Common/GlassSheet.vue";
import { userStore } from "@/store";
import { createPlaylist } from "@/api/playlist";
import { useI18n } from "vue-i18n";

const { t } = useI18n();
const user = userStore();

// 新建歌单数据
const createPlaylistShow = ref(false);
const createPrivacy = ref(false);
const createName = ref(null);

// 新建歌单
const createPlaylistBtn = (name, privacy = false) => {
  createPlaylist(name, privacy ? "10" : null).then((res) => {
    if (res.code === 200) {
      closeCreatePlaylist();
      $message.success(t("general.message.createSuccess"));
      user.setUserPlayLists();
    } else {
      $message.error(t("general.message.createFailed"));
    }
  });
};

// 开启新建歌单
const openCreatePlaylist = () => {
  // 面板可从遮罩或下拉关掉，不经过 closeCreatePlaylist，所以打开时清空上一次的输入。
  createName.value = null;
  createPrivacy.value = false;
  createPlaylistShow.value = true;
};

// 取消新建歌单
const closeCreatePlaylist = () => {
  createName.value = null;
  createPrivacy.value = false;
  createPlaylistShow.value = false;
};

// 暴露方法
defineExpose({
  openCreatePlaylist,
});
</script>
