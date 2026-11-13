<template>
  <n-result
    class="error"
    status="404"
    :title="$t('state.notFound')"
    :description="$t('state.notFoundDesc')"
  >
    <template #footer>
      <n-button type="primary" @click="navigation.closeTop()">{{
        $t("general.name.goBack")
      }}</n-button>
    </template>
  </n-result>
</template>

<script setup lang="ts">
import { useLayerNavigation } from "@/utils/navigation";
const navigation = useLayerNavigation();

onMounted(() => {
  $setSiteTitle("404");
});
</script>

<style lang="scss" scoped>
.error {
  height: calc(100vh - 208px);
  display: flex;
  justify-content: center;
  align-items: center;
  flex-direction: column;
}
</style>
