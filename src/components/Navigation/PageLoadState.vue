<template>
  <div v-if="error || loading" class="page-load-state" :role="error ? 'alert' : 'status'">
    <template v-if="error">
      <span>{{ $t("general.message.acquisitionFailed") }}</span>
      <n-button secondary round @click="$emit('retry')">{{ $t("general.name.reload") }}</n-button>
    </template>
    <n-spin v-else size="small" />
  </div>
</template>

<script setup lang="ts">
defineProps<{ loading?: boolean; error?: boolean }>();
defineEmits<{ retry: [] }>();
</script>

<style scoped>
.page-load-state {
  display: flex;
  align-items: center;
  justify-content: center;
  flex-direction: column;
  gap: 16px;
  min-height: 180px;
  padding: 32px 20px;
}
</style>
