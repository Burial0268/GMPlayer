<template>
  <n-result
    class="error"
    status="403"
    :title="$t('state.prohibition')"
    :description="$t('state.prohibitionDesc')"
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
  $setSiteTitle("403");
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
