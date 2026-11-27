<template>
  <!-- 关于本站：与设置页「关于」分区共用同一份内容 -->
  <GlassSheet v-model:show="showAboutModal" :title="$t('nav.avatar.about')">
    <SettingsAbout />
  </GlassSheet>
</template>

<script setup>
import GlassSheet from "@/components/Common/GlassSheet.vue";
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
