<template>
  <!-- 关于本站：与设置页「关于」分区共用同一份内容 -->
  <n-modal
    class="s-modal"
    v-model:show="showAboutModal"
    preset="card"
    :title="$t('nav.avatar.about')"
    :bordered="false"
    transform-origin="center"
  >
    <SettingsAbout />
  </n-modal>
</template>

<script setup>
import SettingsAbout from "@/components/Settings/SettingsAbout.vue";

const showAboutModal = ref(false);

// 开启本站数据弹窗
const openAboutSite = () => {
  showAboutModal.value = true;
};

// 暴露方法
defineExpose({
  openAboutSite,
});
</script>
